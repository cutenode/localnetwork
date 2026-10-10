//! Publishing to the signed-in Atmosphere account (see [`crate::publish`]), and reading what
//! others published.
//!
//! Each write is a `*_plan` / `*_apply` pair. The plan (a [`Planned`]) holds the exact record,
//! what's published in its place now, and `missing_scope` when the session may not write it yet:
//! sign in again with `publish: true` (`account_login_begin`) first. [`Planned::apply`] publishes
//! exactly that record, and only over what the plan read: if someone published in its place
//! since, it fails with `stale_plan`. The `*_apply` functions plan again and write in one go. A
//! plan needs no sign-in, so a record can be looked at before signing in. Everything published
//! is public.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use chrono::Utc;
use schemars::JsonSchema;
use serde::Deserialize;

use super::ProfileRequest;
use crate::atmosphere::repo::{self, Resolved};
use crate::atmosphere::{self, AccountInfo, lexicon};
use crate::error::{Action, Reason};
use crate::op::{Operation, Planned, Probe, Touched};
use crate::profile::{PREFS_FILE, ProfilePrefs};
use crate::progress::Ctx;
use crate::publish;
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
    /// Their data server's URL (https, or http on this machine), instead of finding it from the
    /// handle or DID.
    #[serde(default)]
    pub pds: Option<String>,
    /// Replace the settings profile's whole base with theirs, so settings they didn't publish
    /// become unset. Default: add theirs to the base, over the values it has for them.
    #[serde(default)]
    pub replace: bool,
}

/// A data server a request names: an https URL, or http on this computer (a test or development
/// server). Checked before anything is fetched from it.
fn named_pds(pds: &str) -> Result<String> {
    let pds = pds.trim_end_matches('/');
    let fit = reqwest::Url::parse(pds)
        .is_ok_and(|u| u.scheme() == "https" || (u.scheme() == "http" && atmosphere::this_computer(&u)));
    match fit {
        true => Ok(pds.to_string()),
        false => Err(Error::invalid(format!("a data server is an https:// URL, not {pds:?}"))),
    }
}

fn source(store_root: &Path, req: &ImportPublishedSettings) -> Result<Resolved> {
    let from = req.from.as_deref().map(|f| f.trim().trim_start_matches('@')).filter(|f| !f.is_empty());
    let pds = req.pds.as_deref().map(str::trim).filter(|p| !p.is_empty()).map(named_pds).transpose()?;
    match (from, pds) {
        (Some(id), Some(pds)) => {
            Ok(Resolved { did: id.to_string(), handle: (!id.starts_with("did:")).then(|| id.to_lowercase()), pds })
        }
        (Some(id), None) => repo::resolve(id),
        (None, pds) => {
            let info = atmosphere::status(store_root)?.ok_or_else(|| {
                Error::Account(
                    "not signed in to an Atmosphere account: sign in, or name whose settings to import".into(),
                )
                .because(Reason::NotSignedIn)
                .action(crate::atmosphere::sign_in())
            })?;
            Ok(Resolved { did: info.did, handle: info.handle, pds: pds.unwrap_or(info.pds) })
        }
    }
}

/// Someone's published My Settings (yours by default), and how importing them would change the
/// settings profile's base: adding them to it, or with `replace` replacing it. Applying it
/// imports exactly the values shown, and is refused (`stale_plan`) if the settings profile
/// changed since.
pub fn settings_import_published_plan(
    store_root: &Path,
    req: &ImportPublishedSettings,
) -> Result<Planned<SettingsImportPlan, PathBuf>> {
    let from = source(store_root, req)?;
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

/// `track 3 isn't named: why`, or `tracks 3, 7 aren't named: why`.
fn unnamed_note(positions: &[usize], why: &str) -> Option<String> {
    let list: Vec<String> = positions.iter().map(usize::to_string).collect();
    match list.as_slice() {
        [] => None,
        [one] => Some(format!("track {one} isn't named: {why}")),
        many => Some(format!("tracks {} aren't named: {why}", many.join(", "))),
    }
}

/// A set as its record: where and when it was played, who with, and its tracks in order
/// (unidentified where the set, the request or the profile says not to name them), keyed by the
/// set's id. Planned signed out, it can only be looked at.
///
/// `plays publish` published history lists as sets under keys of their own (`atmosphere-sets.json`
/// remembers them), in the collection sets had then ([`publish::PLAYS_PUBLISH_COLLECTION`]). A set
/// built from one of those lists alone, with nothing published under its own id, takes over that
/// key the first time it's published, and the time the old record says it first went out;
/// publishing changes the set's id to the key. The old record stays where it is: publishing may
/// only write sets in today's collection.
pub fn set_publish_plan(store_root: &Path, req: &SetPublishRequest) -> Result<PlannedPublish> {
    let store = Store::open(store_root)?;
    let book = SetBook::load(&store)?;
    let set = book.find(&req.set)?;
    if let Some(&bad) = req.hide.iter().find(|&&p| p == 0 || p > set.tracks.len()) {
        return Err(Error::invalid(format!("there's no track {bad} to hide: the set has {}", set.tracks.len())));
    }
    let prefs = ProfilePrefs::load(&store)?;
    let (mut marked, mut asked, mut profile) = (Vec::new(), Vec::new(), Vec::new());
    for (i, e) in set.tracks.iter().enumerate() {
        let n = i + 1;
        if e.unnamed {
            marked.push(n);
        } else if req.hide.contains(&n) {
            asked.push(n);
        } else if !prefs.may_name(&e.name(), &e.artist) {
            profile.push(n);
        }
    }
    let mut notes: Vec<String> = [
        unnamed_note(&marked, "marked unnamed (`sets edit --named`)"),
        unnamed_note(&asked, "hidden as asked"),
        unnamed_note(&profile, "hidden by your profile (`profile unhide`)"),
    ]
    .into_iter()
    .flatten()
    .collect();
    let hidden: BTreeSet<usize> = asked.iter().chain(&profile).copied().collect();
    let mut record = publish::set_record(set, &hidden, Utc::now());
    if record.get("location").is_none() && set.city.as_deref().is_some_and(|c| !c.trim().is_empty()) {
        notes.push("the city is left out: it goes out with a country (`sets edit --country`)".into());
    }

    let account = atmosphere::status(store_root)?;
    let read = |info: &AccountInfo, rkey: &str| repo::get_record(&info.pds, &info.did, lexicon::SET, rkey);
    let mut rkey = set.id.clone();
    let mut current = match &account {
        Some(info) => read(info, &rkey)?,
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
        if let Some(legacy) = repo::get_record(&info.pds, &info.did, publish::PLAYS_PUBLISH_COLLECTION, &old)? {
            notes.push(format!(
                "what `plays publish` published stays in {}: publishing can't remove it",
                publish::PLAYS_PUBLISH_COLLECTION
            ));
            if let Some(first) = legacy.value.get("createdAt").filter(|c| c.is_string() && current.is_none()) {
                record["createdAt"] = first.clone();
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publishing_needs_settings_and_importing_needs_someone() {
        let dir = tempfile::tempdir().unwrap();
        let err = settings_publish_plan(dir.path()).unwrap_err();
        assert!(err.to_string().contains("settings profile is empty"), "{err}");
        let err = settings_import_published_plan(dir.path(), &ImportPublishedSettings::default()).unwrap_err();
        assert_eq!(err.code(), "account");
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none(), "nothing written");

        // Signed out, a plan shows the record without an account to write it to.
        let store = Store::open_or_init(dir.path()).unwrap();
        let mut profile = store.settings_profile().unwrap();
        profile.set(None, "quantize", "off").unwrap();
        store.save_settings_profile(&profile).unwrap();
        let plan = settings_publish_plan(dir.path()).unwrap();
        assert!(plan.account.is_none() && plan.uri.is_none() && plan.current.is_none());
        assert_eq!(plan.record["player"]["quantize"], "off");
        let err = settings_publish_apply(dir.path()).unwrap_err();
        assert!(err.to_string().contains("not signed in"), "{err}");
    }

    /// A data server named in a request is fetched from, so it must be https (or this machine).
    #[test]
    fn a_named_data_server_is_https_or_this_machine() {
        for ok in [
            "https://pds.example/",
            "http://127.0.0.1:2583",
            "http://localhost",
            "http://[::1]:80/",
            "http://127.0.0.2",
            "http://pds.localhost.",
        ] {
            assert_eq!(named_pds(ok).unwrap(), ok.trim_end_matches('/'));
        }
        for bad in [
            "http://pds.example",
            "http://localhost.example",
            "http://localhost:80@pds.example",
            "file:///etc/passwd",
            "pds.example",
        ] {
            assert!(named_pds(bad).is_err(), "{bad}");
        }
        let dir = tempfile::tempdir().unwrap();
        let req = ImportPublishedSettings {
            from: Some("did:plc:abc".into()),
            pds: Some("http://10.0.0.1".into()),
            replace: false,
        };
        let err = settings_import_published_plan(dir.path(), &req).unwrap_err();
        assert!(err.to_string().contains("https://"), "{err}");
    }
}
