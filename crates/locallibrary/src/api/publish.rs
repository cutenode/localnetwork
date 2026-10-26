//! Publishing to the signed-in Atmosphere account (see [`crate::publish`]), and reading what
//! others published.
//!
//! Each write is a `*_plan` / `*_apply` pair. The plan (a [`Planned`]) holds the exact record,
//! what's published in its place now (as Local Network's directory has it, which every read goes
//! through), and `missing_scope` when the session may not write it yet:
//! sign in again with `publish: true` (`account_login_begin`) first. [`Planned::apply`] publishes
//! exactly that record, and only over what the plan read: if someone published in its place
//! since, it fails with `stale_plan`. The `*_apply` functions plan again and write in one go. A
//! plan needs no sign-in, so a record can be looked at before signing in. Everything published
//! is public.

use std::path::{Path, PathBuf};

use chrono::Utc;
use loclib_plugin::StrongRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ProfileRequest;
use crate::atmosphere::directory::{self, Fetched};
use crate::atmosphere::repo::{self, Resolved};
use crate::atmosphere::{self, AccountInfo, lexicon};
use crate::error::{Action, Reason};
use crate::mix::report;
use crate::op::{Operation, Planned, Probe, Touched};
use crate::profile::{PREFS_FILE, ProfilePrefs};
use crate::progress::Ctx;
use crate::publish;
use crate::publish::recording::{self, Audio, Encoding};
pub use crate::publish::{PublishPlan, Published, PublishedSettings, SettingsImportPlan};
use crate::sets::{SETS_FILE, SetBook};
use crate::settings::SettingsProfile;
use crate::store::Store;
use crate::{Error, Result};

/// Where a planned record goes: the store whose account publishes it, and, for a set that takes
/// over the key `plays publish` gave its history list, the set's id to change to that key.
#[derive(Debug, Clone)]
pub struct PublishTo {
    root: PathBuf,
    renamed: Option<String>,
}

/// A record ready to publish, as [`Planned::apply`] takes it.
pub type PlannedPublish = Planned<PublishPlan, PublishTo>;

/// Wrap a record's plan: `what` names it in the summary ("your My Settings").
fn planned(
    operation: &'static str,
    what: &str,
    root: &Path,
    plan: PublishPlan,
    renamed: Option<String>,
) -> PlannedPublish {
    let mut lines = Vec::new();
    let mut warnings = Vec::new();
    match (&plan.account, &plan.uri) {
        (Some(account), Some(uri)) => lines.push(match &plan.current {
            None => format!("publish {what} as {} ({uri})", account.name()),
            Some(_) if !plan.changed => format!("{what} is published as {} already, unchanged", account.name()),
            Some(_) => format!("replace {what} published as {} ({uri})", account.name()),
        }),
        _ => {
            lines.push(format!("{what}, as it would be published"));
            warnings.push("not signed in to an Atmosphere account: sign in, allowing publishing, to publish".into());
        }
    }
    lines.extend(plan.notes.iter().cloned());
    if plan.missing_scope.is_some() {
        warnings.push(
            "this session can't publish yet: sign in again allowing publishing (`account.login.begin` with \
             `publish: true`)"
                .into(),
        );
    }
    let noop = plan.account.is_some() && !plan.changed;
    let replaces = plan.current.is_some() && plan.changed;
    let to = PublishTo { root: root.to_path_buf(), renamed };
    Planned::new(operation, plan, to).summary(lines).warnings(warnings).noop(noop).destructive(replaces)
}

/// Publish `plan`'s record; for a set that takes over an older record's key, make that its id.
fn publish_to(to: &PublishTo, plan: &PublishPlan) -> Result<Published> {
    let Some(id) = &to.renamed else { return publish::apply(&to.root, plan) };
    // The store is locked first, so the set's id changes along with the record.
    let store = Store::open(&to.root)?;
    let _lock = store.lock()?;
    let published = publish::apply(&to.root, plan)?;
    let mut book = SetBook::load(&store)?;
    if let Some(set) = book.sets.iter_mut().find(|s| s.id == *id) {
        set.id = plan.rkey.clone();
        book.save(&store)?;
    }
    Ok(published)
}

impl Operation for PublishPlan {
    type Request = PublishTo;
    type Report = Published;

    fn apply(planned: &PlannedPublish, _: Option<&Store>, _: &Ctx<'_>) -> Result<Published> {
        // Published in its place since the plan read it: not what the user confirmed replacing.
        publish_to(planned.request(), planned).map_err(|e| match e {
            e if e.code() == "conflict" => Error::StalePlan(e.to_string()),
            e => e,
        })
    }
}

/// The settings profile's base values, as they'd be published (per-drive overrides stay here).
pub fn settings_publish_plan(store_root: &Path) -> Result<PlannedPublish> {
    let base = match Store::open(store_root) {
        Ok(store) => store.settings_profile()?.base,
        Err(_) => Default::default(),
    };
    if base.is_empty() {
        return Err(Error::invalid("the settings profile is empty: import a drive's settings (or set some) first")
            .because(Reason::EmptyProfile)
            .action(Action::call(
                "settings.import.plan",
                serde_json::json!({ "drive": null }),
                "Import a drive's settings",
            )));
    }
    let plan = publish::plan(store_root, lexicon::SETTINGS, "self", publish::settings_record(&base, Utc::now()))?;
    Ok(planned("settings.publish", "your My Settings", store_root, plan, None).touches(Touched::path(
        store_root,
        false,
        vec![Probe::Content(vec!["settings.json".into()])],
    )))
}

/// Publish the settings profile's base values as the account's My Settings (planned and
/// published in one go).
pub fn settings_publish_apply(store_root: &Path) -> Result<Published> {
    let plan = settings_publish_plan(store_root)?;
    publish_to(plan.request(), &plan)
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ImportPublishedSettings {
    /// Whose My Settings: a handle or DID. Default: the signed-in account's.
    #[serde(default)]
    pub from: Option<String>,
    /// Replace the settings profile's whole base with theirs, so settings they didn't publish
    /// become unset. Default: add theirs to the base, over the values it has for them.
    #[serde(default)]
    pub replace: bool,
}

/// Whose settings `req` imports, and the signed-in account when it's theirs.
fn source(store_root: &Path, req: &ImportPublishedSettings) -> Result<(Resolved, Option<AccountInfo>)> {
    let account = atmosphere::status(store_root)?;
    let from = req.from.as_deref().map(|f| f.trim().trim_start_matches('@')).filter(|f| !f.is_empty());
    let from = match (from, account.as_ref()) {
        (Some(id), _) => repo::resolve(id)?,
        (None, Some(info)) => Resolved { did: info.did.clone(), handle: info.handle.clone() },
        (None, None) => {
            return Err(Error::Account(
                "not signed in to an Atmosphere account: sign in, or name whose settings to import".into(),
            )
            .because(Reason::NotSignedIn)
            .action(crate::atmosphere::sign_in()));
        }
    };
    let own = account.filter(|info| info.did == from.did);
    Ok((from, own))
}

/// Someone's published My Settings (yours by default), and how importing them would change the
/// settings profile's base: adding them to it, or with `replace` replacing it. Applying it
/// imports exactly the values shown, and is refused (`stale_plan`) if the settings profile
/// changed since.
pub fn settings_import_published_plan(
    store_root: &Path,
    req: &ImportPublishedSettings,
) -> Result<Planned<SettingsImportPlan, PathBuf>> {
    let (from, own) = source(store_root, req)?;
    // Your own, published before the directory kept them, may need reading again first.
    let indexing = match own {
        Some(info) => directory::request_index(store_root, &info)?,
        None => None,
    };
    let store = Store::open(store_root).ok();
    let base = match &store {
        Some(store) => store.settings_profile()?.base,
        None => Default::default(),
    };
    let plan = publish::import_plan(from, &base, req.replace)?;
    let (count, name, changes) = (plan.published.values.len(), plan.from.name(), plan.changes.len());
    let line = if plan.replace {
        format!("replace the settings profile's base with the {count} settings {name} published ({changes} change(s))")
    } else {
        format!("add the {count} settings {name} published to the settings profile's base ({changes} change(s))")
    };
    // However many a record holds, a few say enough.
    const SHOWN: usize = 20;
    let skipped = &plan.published.skipped;
    let mut warnings: Vec<String> =
        skipped.iter().take(SHOWN).map(|s| format!("left out, this version doesn't know it: {s}")).collect();
    if skipped.len() > SHOWN {
        warnings.push(format!("left out {} more this version doesn't know", skipped.len() - SHOWN));
    }
    warnings.extend(indexing);
    let noop = plan.changes.is_empty();
    let replaces = plan.changes.iter().any(|c| c.from.is_some());
    let mut planned = Planned::new("settings.import_published", plan, store_root.to_path_buf())
        .line(line)
        .warnings(warnings)
        .noop(noop)
        .destructive(replaces);
    // Without a store yet, importing makes one.
    if store.is_some() {
        planned = planned.touches(Touched::path(store_root, true, vec![Probe::Content(vec!["settings.json".into()])]));
    }
    Ok(planned)
}

impl Operation for SettingsImportPlan {
    /// The store's root.
    type Request = PathBuf;
    type Report = SettingsProfile;

    fn apply(planned: &Planned<Self, PathBuf>, _: Option<&Store>, _: &Ctx<'_>) -> Result<SettingsProfile> {
        let store = Store::open_or_init(planned.request())?;
        let _lock = store.lock()?;
        let mut profile = store.settings_profile()?;
        profile.base = planned.imported(&profile.base);
        store.save_settings_profile(&profile)?;
        Ok(profile)
    }
}

/// Import someone's published My Settings (yours by default) into the settings profile's base,
/// planned and imported in one go. Per-drive overrides stay as they are.
pub fn settings_import_published_apply(store_root: &Path, req: &ImportPublishedSettings) -> Result<SettingsProfile> {
    settings_import_published_plan(store_root, req)?.apply(None, &Ctx::silent())
}

/// The profile document (built as [`super::profile_document`] builds it, so only the sections
/// the DJ included) as it would be published.
pub fn profile_publish_plan(store_root: &Path, req: &ProfileRequest, ctx: &Ctx<'_>) -> Result<PlannedPublish> {
    let store = Store::open(store_root).ok();
    let doc = super::profile_document(store.as_ref(), req, ctx)?;
    let plan = publish::plan(store_root, lexicon::PROFILE, "self", publish::profile_record(&doc))?;
    Ok(planned("profile.publish", "your profile", store_root, plan, None))
}

/// Publish the profile document as the account's profile (planned and published in one go).
pub fn profile_publish_apply(store_root: &Path, req: &ProfileRequest, ctx: &Ctx<'_>) -> Result<Published> {
    let plan = profile_publish_plan(store_root, req, ctx)?;
    publish_to(plan.request(), &plan)
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SetPublishRequest {
    /// The set: its id (or a prefix of 4 or more characters), title or day, or `last`.
    pub set: String,
    /// Positions (from 1) of tracks to publish without naming them. Tracks the set marks unnamed
    /// and tracks the profile hides (`profile hide`) are never named either.
    #[serde(default)]
    pub hide: Vec<usize>,
}

/// A set as its record: where and when it was played, who with, and its tracks in order
/// (unidentified where the set, the request or the profile says not to name them), keyed by the
/// set's id. Planned signed out, it can only be looked at.
///
/// `plays publish` published history lists as sets under keys of their own (`atmosphere-sets.json`
/// remembers them). A set built from one of those lists alone, with nothing published under its
/// own id, takes over that key the first time it's published; publishing changes the set's id to
/// the key.
pub fn set_publish_plan(store_root: &Path, req: &SetPublishRequest) -> Result<PlannedPublish> {
    let store = Store::open(store_root)?;
    let book = SetBook::load(&store)?;
    let set = book.find(&req.set)?;
    let unnamed = set.unnamed(&req.hide, &ProfilePrefs::load(&store)?)?;
    if set.tracks.len() > publish::SET_TRACK_LIMIT {
        return Err(Error::invalid(format!(
            "the set has {} tracks, more than a set record holds ({}): drop some first (`sets edit --drop`)",
            set.tracks.len(),
            publish::SET_TRACK_LIMIT
        )));
    }
    let mut notes = unnamed.notes;
    // How tracks came in is in the recording's report: without it (gone from the store), it's left out.
    let report =
        set.recording.as_ref().and_then(|r| r.report.as_deref()).and_then(|id| report::load(store_root, id).ok());
    let record = publish::set_record(set, &unnamed.positions, report.as_ref(), Utc::now());
    if record.get("location").is_none() && set.city.as_deref().is_some_and(|c| !c.trim().is_empty()) {
        notes.push("the city is left out: it goes out with a country (`sets edit --country`)".into());
    }
    let size = serde_json::to_vec(&record).map_or(0, |r| r.len());
    if size > publish::OLDER_PDS_REQUEST_BYTES {
        notes.push(format!(
            "the record is {} KB: a data server not updated since May 2026 refuses one over {} KB",
            size / 1024,
            publish::OLDER_PDS_REQUEST_BYTES / 1024
        ));
    }

    let account = atmosphere::status(store_root)?;
    let read = |info: &AccountInfo, rkey: &str| directory::get_record(&info.did, lexicon::SET, rkey);
    let mut rkey = set.id.clone();
    let mut current = match &account {
        Some(info) => {
            notes.extend(directory::request_index(store_root, info)?);
            read(info, &rkey)?
        }
        None => None,
    };
    let mut renamed = None;
    if let (Some(info), None, [session]) = (&account, &current, set.sessions.as_slice())
        && let Some(old) = publish::published_set_key(store_root, &info.did, session)?
        && !book.sets.iter().any(|s| s.id == old)
    {
        let short = &session[..session.len().min(8)];
        notes.push(format!("takes the key `plays publish` gave session {short}; the set's id becomes {old}"));
        current = read(info, &old)?;
        renamed = Some(std::mem::replace(&mut rkey, old));
    }
    let mut plan = publish::plan_over(account, lexicon::SET, &rkey, record, current)?;
    plan.notes = notes;
    // The record is built from the set and the profile's choices: either changing makes it stale.
    let read_from =
        Touched::path(store_root, renamed.is_some(), vec![Probe::Content(vec![SETS_FILE.into(), PREFS_FILE.into()])]);
    Ok(planned("sets.publish", "this set", store_root, plan, renamed).touches(read_from))
}

/// Publish a set (planned and published in one go). Publishing it again replaces its record.
pub fn set_publish_apply(store_root: &Path, req: &SetPublishRequest) -> Result<Published> {
    let plan = set_publish_plan(store_root, req)?;
    publish_to(plan.request(), &plan)
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SetUploadRequest {
    /// The set: its id (or a prefix of 4 or more characters), title or day, or `last`.
    pub set: String,
    /// The most the account's data server takes, in bytes, when that's less than the recording
    /// Lexicon's 300 MiB: the recording is encoded to fit.
    #[serde(default)]
    pub max_bytes: Option<u64>,
}

/// A set's recording, ready to upload to the signed-in account and publish as a recording record
/// under the set's key, which the set's record then points at.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RecordingUpload {
    /// The set, by id.
    pub set: String,
    pub audio: Audio,
    /// As it is, or encoded again by ffmpeg.
    pub encoding: Encoding,
    /// The most the data server is taken to take, in bytes.
    pub limit: u64,
    /// The account it goes to. `None`: nobody's signed in, so it can only be looked at.
    pub account: Option<AccountInfo>,
    /// `at://did/computer.localnetwork.dj.recording/<set id>` (`None` while signed out).
    pub uri: Option<String>,
    /// The recording published there now, as Local Network's directory has it, which uploading
    /// replaces.
    pub current: Option<Fetched>,
    /// The scope the session lacks to upload it and write its record: sign in again allowing
    /// publishing (`account.login.begin` with `publish: true`) before applying.
    pub missing_scope: Option<String>,
    pub notes: Vec<String>,
}

/// A recording ready to upload, as [`Planned::apply`] takes it (with the store it's for).
pub type PlannedUpload = Planned<RecordingUpload, PathBuf>;

/// Plan uploading the recording on this computer of the set `req` names: how it goes up (as it is,
/// or encoded by ffmpeg to play in browsers and fit `req.max_bytes`), and where its record goes.
pub fn set_upload_plan(store_root: &Path, req: &SetUploadRequest) -> Result<PlannedUpload> {
    let store = Store::open(store_root)?;
    let book = SetBook::load(&store)?;
    let set = book.find(&req.set)?;
    let on_disk = set.recording.as_ref().map(|r| &r.path).filter(|p| p.is_file()).ok_or_else(|| {
        Error::NotFound(format!(
            "a recording of {} on this computer to upload (`sets edit {} --recording FILE` attaches one)",
            set.heading(),
            set.short_id()
        ))
    })?;
    let audio = recording::probe(on_disk)?;
    let limit = req.max_bytes.map_or(recording::MAX_BYTES, |m| m.min(recording::MAX_BYTES));
    let encoding = recording::encoding(audio.playable.is_some(), audio.bytes, audio.duration_secs, limit)?;
    let account = atmosphere::status(store_root)?;
    let mut plan = RecordingUpload {
        set: set.id.clone(),
        encoding,
        limit,
        account: account.clone(),
        uri: None,
        current: None,
        missing_scope: None,
        notes: Vec::new(),
        audio,
    };
    if let Some(info) = &account {
        plan.notes.extend(directory::request_index(store_root, info)?);
        plan.uri = Some(repo::at_uri(&info.did, lexicon::RECORDING, &set.id));
        plan.current = directory::get_record(&info.did, lexicon::RECORDING, &set.id)?;
        let mime = recording::mime(&plan.audio, encoding);
        plan.missing_scope = atmosphere::missing_scope(info, lexicon::RECORDING)
            .or_else(|| (!atmosphere::may_upload(info, mime)).then(|| atmosphere::UPLOAD_SCOPE.to_string()));
    }
    let name = plan.audio.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let how = match encoding {
        Encoding::AsIs => format!("as it is ({})", crate::op::bytes(plan.audio.bytes)),
        Encoding::Aac { kbps, bytes } => {
            format!("encoded by ffmpeg as AAC at {kbps} kbit/s (about {})", crate::op::bytes(bytes))
        }
    };
    let mut warnings = Vec::new();
    let line = match (&plan.account, &plan.current) {
        (Some(info), None) => format!("upload {name} {how} as {}", info.name()),
        (Some(info), Some(_)) => format!("upload {name} {how} as {}, replacing the recording there", info.name()),
        (None, _) => {
            warnings.push("not signed in to an Atmosphere account: sign in, allowing publishing, to upload".into());
            format!("{name} would be uploaded {how}")
        }
    };
    if plan.missing_scope.is_some() {
        warnings.push(
            "this session can't upload yet: sign in again allowing publishing (`account.login.begin` with \
             `publish: true`)"
                .into(),
        );
    }
    let summary = [line].into_iter().chain(plan.notes.iter().cloned()).collect::<Vec<_>>();
    let replaces = plan.current.is_some();
    // The set's recording, and the set it keeps the record's reference in.
    let read_from = Touched::path(store_root, true, vec![Probe::Content(vec![SETS_FILE.into()])]);
    Ok(Planned::new("sets.upload", plan, store_root.to_path_buf())
        .summary(summary)
        .warnings(warnings)
        .destructive(replaces)
        .touches(read_from))
}

impl Operation for RecordingUpload {
    type Request = PathBuf;
    type Report = Published;

    /// Encode the recording if it has to be, upload it, publish its record (only over the one
    /// the plan read), and keep the record on the set, for the set's record to point at.
    fn apply(planned: &PlannedUpload, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<Published> {
        let root = planned.request();
        let info = planned.account.as_ref().ok_or_else(atmosphere::not_signed_in)?;
        if planned.missing_scope.is_some() {
            return Err(repo::scope_error(info, lexicon::RECORDING));
        }
        let upload = recording::prepare(&planned.audio, planned.encoding, planned.limit, ctx)?;
        let blob = repo::upload_blob(root, &info.did, upload.path(), &upload.mime, ctx)?;
        let record = recording::record(blob, planned.audio.duration_secs, Utc::now());
        let plan = publish::plan_over(
            planned.account.clone(),
            lexicon::RECORDING,
            &planned.set,
            record,
            planned.current.clone(),
        )?;
        let published = publish::apply(root, &plan).map_err(|e| match e {
            e if e.code() == "conflict" => Error::StalePlan(e.to_string()),
            e => e,
        })?;
        let store = Store::open(root)?;
        let _lock = store.lock()?;
        let mut book = SetBook::load(&store)?;
        let set = book.find_mut(&planned.set)?;
        set.recording_record = Some(StrongRef { uri: published.uri.clone(), cid: published.cid.clone() });
        set.updated_at = Utc::now();
        book.save(&store)?;
        Ok(published)
    }
}

#[cfg(test)]
mod tests;
