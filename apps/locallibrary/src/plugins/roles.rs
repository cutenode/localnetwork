//! What the app runs for the roles core reads (see [`locallibrary::fields::roles`]): core never
//! analyses a track itself, so before a report that needs analysis (doctor's), the analyzer
//! chosen for each of its roles runs on the drive's tracks it hasn't analysed yet, with progress,
//! and core reads what they kept.

use locallibrary::api::DoctorRequest;
use locallibrary::doctor::{DoctorReport, Section};
use locallibrary::fields::Role;
use locallibrary::fields::roles::{self, RoleField};
use locallibrary::plugin::Analyzed;
use locallibrary::progress::Ctx;
use locallibrary::{Error, Result, Store, api, sound};

use super::host;

/// Runs the analyzer chosen for each of `roles` (each analyzer once) on the tracks of the drive
/// `drive` names that it hasn't analysed, keeping what it works out in `store`. Refused when no
/// analyzer has any of them.
pub fn analyze(drive: &str, roles: &[Role], store: &Store, ctx: &Ctx<'_>) -> Result<Analyzed> {
    let mut chosen: Vec<RoleField> = Vec::new();
    for f in roles.iter().filter_map(|r| roles::chosen(Some(store), *r)) {
        if !chosen.iter().any(|c| c.analyzer == f.analyzer) {
            chosen.push(f);
        }
    }
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
        let a = plugins.analyze(&f.analyzer.plugin, &f.analyzer.analyzer, drive, None, store, ctx)?;
        done.analyzed += a.analyzed;
        done.kept += a.kept;
        done.failed += a.failed;
        done.failures.extend(a.failures);
    }
    Ok(done)
}

/// `doctor`, as the command line and the sidecar run it: with its analysis, the analyzers for
/// tempo, key and loudness run first (keeping what they work out in `store`), and the report says
/// what they did.
pub fn doctor(req: &DoctorRequest, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<DoctorReport> {
    let wanted = store.filter(|_| req.options.wants(Section::Analysis));
    let ran = wanted.map(|store| analyze(&req.drive, &sound::ROLES, store, ctx)).transpose()?;
    let mut report = api::doctor(req, ctx)?;
    if let (Some(a), Some(ran)) = (report.analysis.as_mut(), ran) {
        a.decoded = ran.analyzed;
        a.failed = ran.failures;
    }
    Ok(report)
}
