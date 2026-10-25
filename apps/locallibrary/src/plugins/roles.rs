//! What the app runs for the roles core reads (see [`locallibrary::fields::roles`]): core never
//! analyses a track itself, so before a report that needs analysis (doctor's) or a set published
//! with its MusicBrainz IDs, the analyzer chosen for each of its roles runs on the tracks it
//! hasn't analysed yet, with progress, and core reads what they kept.

#[cfg(feature = "account")]
use std::path::Path;

use locallibrary::api::DoctorRequest;
use locallibrary::doctor::{DoctorReport, Section};
use locallibrary::fields::Role;
use locallibrary::fields::roles::{self, RoleField};
use locallibrary::plugin::Analyzed;
use locallibrary::progress::Ctx;
#[cfg(feature = "account")]
use locallibrary::{Device, sets::MusicBrainzIds};
use locallibrary::{Error, Result, Store, api, sound};

use super::host;

/// The analyzers chosen for `roles`, each once, with the settings in `store`.
fn chosen(roles: &[Role], store: &Store) -> Vec<RoleField> {
    let mut chosen: Vec<RoleField> = Vec::new();
    for f in roles.iter().filter_map(|r| roles::chosen(Some(store), *r)) {
        if !chosen.iter().any(|c| c.analyzer == f.analyzer) {
            chosen.push(f);
        }
    }
    chosen
}

/// Runs the analyzer chosen for each of `roles` (each analyzer once) on the tracks of the drive
/// `drive` names that it hasn't analysed (or with `again`, over what it kept), those with ids
/// `tracks` or every one, keeping what it works out in `store`. Refused when no analyzer has any
/// of them.
pub fn analyze(
    drive: &str,
    roles: &[Role],
    tracks: Option<&[u32]>,
    again: bool,
    store: &Store,
    ctx: &Ctx<'_>,
) -> Result<Analyzed> {
    let chosen = chosen(roles, store);
    if chosen.is_empty() {
        let named: Vec<&str> = roles.iter().map(|r| r.name()).collect();
        return Err(Error::Invalid(format!(
            "no plugin works out {} (`locallibrary plugin` lists what they do)",
            named.join(", ")
        )));
    }
    let plugins = host()?;
    let mut done = Analyzed::default();
    for f in chosen {
        let (plugin, analyzer) = (&f.analyzer.plugin, &f.analyzer.analyzer);
        let a = match (again, tracks) {
            (true, Some(tracks)) => plugins.analyze_again(plugin, analyzer, drive, tracks, store, ctx)?,
            _ => plugins.analyze(plugin, analyzer, drive, tracks, store, ctx)?,
        };
        done.analyzed += a.analyzed;
        done.kept += a.kept;
        done.failed += a.failed;
        done.failures.extend(a.failures);
    }
    Ok(done)
}

/// Looks up MusicBrainz IDs for the tracks publishing a set as `req` names, before `sets
/// publish` and as `sets.lookup` (see [`api::set_lookup`]): the analyzers chosen for their roles
/// run on those tracks' drives.
#[cfg(feature = "account")]
pub fn set_lookup(root: &Path, req: &api::SetPublishRequest, ctx: &Ctx<'_>) -> Result<api::SetLookup> {
    let store = Store::open(root)?;
    let run = |d: &Device, ids: &[u32], again: bool| {
        analyze(&d.root().display().to_string(), &MusicBrainzIds::ROLES, Some(ids), again, &store, ctx)
    };
    api::set_lookup(root, req, run, ctx)
}

/// `doctor`, as the command line and the sidecar run it: with its analysis, the analyzers for
/// tempo, key and loudness run first (keeping what they work out in `store`), and the report says
/// what they did.
pub fn doctor(req: &DoctorRequest, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<DoctorReport> {
    let wanted = store.filter(|_| req.options.wants(Section::Analysis));
    let ran = wanted.map(|store| analyze(&req.drive, &sound::ROLES, None, false, store, ctx)).transpose()?;
    let mut report = api::doctor(req, ctx)?;
    if let (Some(a), Some(ran)) = (report.analysis.as_mut(), ran) {
        a.decoded = ran.analyzed;
        a.failed = ran.failures;
    }
    Ok(report)
}
