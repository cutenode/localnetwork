//! Everything set here to and from one file: My Settings, the players, the profile's preferences
//! and every plugin's settings (see [`crate::settings::transfer`]).

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::op::{self, Operation, Planned, Touched};
use crate::progress::Ctx;
use crate::settings::transfer::{self, Action, Change, SettingsExport};
use crate::store::Store;
use crate::{Error, Result};

/// Everything set in the store, as `settings.import_file` takes it.
pub fn settings_export(store: &Store) -> Result<SettingsExport> {
    transfer::export(store)
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ImportSettingsFile {
    /// A file `settings.export` wrote (`locallibrary settings export`).
    pub file: PathBuf,
    /// Where one of the file's values differs from the one here, take the file's (for a list of
    /// names, the file's whole list). Default: keep the one here.
    #[serde(default)]
    pub replace: bool,
}

/// What importing a settings file would change: the file's values missing here (added), and
/// those that differ (kept as they are here, or with `replace` taken from the file).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ImportSettingsFilePlan {
    pub file: PathBuf,
    pub changes: Vec<Change>,
    /// The file's values that are as they are here already.
    pub unchanged: usize,
}

/// What importing `req.file` would change. Applying it imports what the file held when planned,
/// and is refused (`stale_plan`) if what it writes in the store changed since.
pub fn settings_import_file_plan(
    store: &Store,
    req: &ImportSettingsFile,
) -> Result<Planned<ImportSettingsFilePlan, (SettingsExport, bool)>> {
    let doc = transfer::read(&req.file)?;
    let merged = transfer::merge(store, &doc, req.replace)?;
    let count = |a: Action| merged.changes.iter().filter(|c| c.action == a).count();
    let (added, replaced, kept) = (count(Action::Add), count(Action::Replace), count(Action::Keep));
    let mut line = format!("import settings from {}: {added} to add", req.file.display());
    if req.replace {
        line.push_str(&format!(", {replaced} to replace"));
    }
    let probes = vec![op::Probe::Content(doc.files())];
    let mut planned = Planned::new(
        "settings.import_file",
        ImportSettingsFilePlan { file: req.file.clone(), changes: merged.changes, unchanged: merged.unchanged },
        (doc, req.replace),
    )
    .line(line)
    .noop(added + replaced == 0)
    .destructive(replaced > 0);
    if kept > 0 {
        planned = planned
            .warnings([format!("{kept} value(s) differ here and stay as they are (replacing takes the file's)")]);
    }
    Ok(planned.touches(Touched::path(store.root(), true, probes)))
}

impl Operation for ImportSettingsFilePlan {
    /// The file as it was read, and `replace`.
    type Request = (SettingsExport, bool);
    /// Everything set in the store then.
    type Report = SettingsExport;

    fn apply(
        planned: &Planned<Self, (SettingsExport, bool)>,
        store: Option<&Store>,
        _: &Ctx<'_>,
    ) -> Result<SettingsExport> {
        let store = store.ok_or_else(|| Error::invalid("importing settings needs the store"))?;
        let (doc, replace) = planned.request();
        let _lock = store.lock()?;
        // What's in the store is as it was planned (the plan is checked first), so this is the
        // plan's merge again.
        transfer::merge(store, doc, *replace)?.write(store)?;
        transfer::export(store)
    }
}

/// Import a settings file, planned and applied in one go (see [`settings_import_file_plan`]).
pub fn settings_import_file(store: &Store, req: &ImportSettingsFile) -> Result<SettingsExport> {
    settings_import_file_plan(store, req)?.apply(Some(store), &Ctx::silent())
}
