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

use std::path::{Path, PathBuf};

use chrono::{NaiveDate, Utc};
use schemars::JsonSchema;
use serde::Deserialize;

use super::ProfileRequest;
use crate::atmosphere::repo::{self, Resolved};
use crate::atmosphere::{self, lexicon};
use crate::error::{Action, Reason};
use crate::op::{Operation, Planned, Probe, Touched};
use crate::plays;
use crate::profile::ProfilePrefs;
use crate::progress::Ctx;
use crate::publish::{self, SetDraft, SetSource, SetTrack};
pub use crate::publish::{PublishPlan, Published, PublishedSettings, SettingsImportPlan};
use crate::settings::SettingsProfile;
use crate::store::Store;
use crate::{Error, Result};

/// Where a planned record goes: the store whose account publishes it, and for a set, the history
/// list's fingerprint to remember its record key by.
#[derive(Debug, Clone)]
pub struct PublishTo {
    root: PathBuf,
    set: Option<String>,
}

/// A record ready to publish, as [`Planned::apply`] takes it.
pub type PlannedPublish = Planned<PublishPlan, PublishTo>;

/// Wrap a record's plan: `what` names it in the summary ("your My Settings").
fn planned(operation: &'static str, what: &str, root: &Path, plan: PublishPlan, set: Option<String>) -> PlannedPublish {
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
    let to = PublishTo { root: root.to_path_buf(), set };
    Planned::new(operation, plan, to).summary(lines).warnings(warnings).noop(noop).destructive(replaces)
}

/// Publish `plan`'s record; for a set, remember its record key.
fn publish_to(to: &PublishTo, plan: &PublishPlan) -> Result<Published> {
    let published = publish::apply(&to.root, plan)?;
    if let (Some(fingerprint), Some(info)) = (&to.set, &plan.account) {
        publish::remember_set_key(&to.root, &info.did, fingerprint, &plan.rkey)?;
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
pub struct HistoryPublishRequest {
    pub drive: String,
    /// The history list: a name or path, as `history` lists them.
    pub list: String,
    /// Also look among the lists the store archived (rekordbox deletes them from the drive).
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub title: Option<String>,
    /// The day it was played: `2026-10-03`. Default: the date it was published with before, else
    /// the day the store first saw the list, else today.
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub venue: Option<String>,
    /// Positions (from 1) of tracks to publish without naming them. Tracks the profile hides
    /// (`profile hide`) are never named either.
    #[serde(default)]
    pub hide: Vec<usize>,
}

/// The plan, and the list's fingerprint (to remember the record key by).
fn history_plan(store_root: &Path, req: &HistoryPublishRequest, ctx: &Ctx<'_>) -> Result<(PublishPlan, String)> {
    let drive = super::resolve_drive(&req.drive)?;
    let on_drive = plays::drive_history(&drive)?;
    let store = Store::open(store_root).ok();
    // The lists the store archived, read when they're needed (it reads every snapshot).
    let mut archive: Option<plays::HistoryArchive> = None;
    let read_archive = |archive: &mut Option<plays::HistoryArchive>| -> Result<()> {
        if let (None, Some(store)) = (&archive, &store) {
            *archive = Some(plays::archive(store, drive.state().and_then(|s| s.library_id), ctx)?);
        }
        Ok(())
    };
    let mut list = plays::find_list(&on_drive.lists, &req.list).cloned();
    if list.is_none() || req.archived {
        read_archive(&mut archive)?;
        if list.is_none()
            && let Some(a) = &archive
        {
            list = plays::find_list(a.lists.iter().map(|a| &a.list), &req.list).cloned();
        }
    }
    let list = list.ok_or_else(|| {
        let hint = if store.is_some() { "" } else { " (and there's no store to look in)" };
        Error::NotFound(format!("history list {:?} on {}{hint}", req.list, on_drive.drive))
    })?;
    if let Some(&bad) = req.hide.iter().find(|&&p| p == 0 || p > list.tracks.len()) {
        return Err(Error::invalid(format!("the list has {} tracks, so there's no track {bad}", list.tracks.len())));
    }
    if list.tracks.is_empty() {
        return Err(Error::invalid(format!("{} has no tracks the library still has", list.name)));
    }

    let mut notes = Vec::new();
    let prefs = store.as_ref().map(ProfilePrefs::load).transpose()?.unwrap_or_default();
    let mut unnamed = Vec::new();
    let tracks: Vec<SetTrack> = list
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            if req.hide.contains(&(i + 1)) {
                SetTrack::unidentified()
            } else if !prefs.may_name(&t.display_name(), &t.artist) {
                unnamed.push((i + 1).to_string());
                SetTrack::unidentified()
            } else {
                SetTrack::named(t)
            }
        })
        .collect();
    if !unnamed.is_empty() {
        notes.push(format!("track {} isn't named: your profile hides it (`profile unhide`)", unnamed.join(", ")));
    }
    if list.missing > 0 {
        notes.push(format!("{} entries are left out: their tracks aren't in the library any more", list.missing));
    }

    // The date: as given, as published before, as the store first saw the list, or today.
    let fingerprint = list.fingerprint();
    let account = atmosphere::status(store_root)?;
    let known_key = match &account {
        Some(info) => publish::published_set_key(store_root, &info.did, &fingerprint)?,
        None => None,
    };
    // What's published under that key: read once, for its date and for the plan.
    let published = match (&account, &known_key) {
        (Some(info), Some(rkey)) => repo::get_record(&info.pds, &info.did, lexicon::SET, rkey)?,
        _ => None,
    };
    let published_date =
        published.as_ref().and_then(|r| NaiveDate::parse_from_str(r.value["date"].as_str()?, "%Y-%m-%d").ok());
    let date = match (&req.date, published_date) {
        (Some(d), _) => NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d")
            .map_err(|_| Error::invalid(format!("{d:?} isn't a date like 2026-10-03")))?,
        (None, Some(d)) => d,
        (None, None) => {
            read_archive(&mut archive)?;
            let archived = archive.iter().flat_map(|a| &a.lists).find(|a| a.list.same_as(&list));
            match archived.map(|a| a.first_seen.created.date_naive()) {
                Some(d) => {
                    notes.push(format!(
                        "dated {d}, the day the store first saw the list (give the day it was played to change it)"
                    ));
                    d
                }
                None => {
                    let today = chrono::Local::now().date_naive();
                    notes.push(format!("dated today, {today} (give the day it was played to change it)"));
                    today
                }
            }
        }
    };
    let text = |s: &Option<String>| s.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let set = SetDraft {
        title: text(&req.title),
        date,
        venue: text(&req.venue),
        source: SetSource::History,
        tracks,
        created_at: Utc::now(),
    };
    let record = publish::set_record(&set);
    let mut plan = match known_key {
        Some(rkey) => publish::plan_over(account, lexicon::SET, &rkey, record, published)?,
        None => publish::plan(store_root, lexicon::SET, &publish::set_key(date, &fingerprint), record)?,
    };
    plan.notes = notes;
    Ok((plan, fingerprint))
}

/// A history list as a set record: its tracks in order (unnamed where the DJ hides them), dated
/// and titled as asked.
pub fn history_publish_plan(store_root: &Path, req: &HistoryPublishRequest, ctx: &Ctx<'_>) -> Result<PlannedPublish> {
    let (plan, fingerprint) = history_plan(store_root, req, ctx)?;
    Ok(planned("history.publish", "this set", store_root, plan, Some(fingerprint)))
}

/// Publish a history list as a set (planned and published in one go). Publishing the same list
/// again replaces its record.
pub fn history_publish_apply(store_root: &Path, req: &HistoryPublishRequest, ctx: &Ctx<'_>) -> Result<Published> {
    let plan = history_publish_plan(store_root, req, ctx)?;
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
