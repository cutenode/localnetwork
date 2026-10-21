//! Plugins' exporters (`export/write`): planned with a dry run, which says which files the export
//! writes, so overwriting any is confirmed first; applied by the plugin writing them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use indexmap::IndexMap;
use loclib_plugin::{Context, ExportSource, Exported, ExportedFile, WriteExport, method};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use super::Plugins;
use super::process::Slot;
use crate::api;
use crate::error::IoContext;
use crate::op::{Operation, Planned, Probe, Touched};
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result, fsutil};

/// An export a plugin will write.
#[derive(Serialize, JsonSchema)]
pub struct PluginExport {
    pub plugin: String,
    pub exporter: String,
    /// The file, or folder, written to.
    pub output: PathBuf,
    /// The files the export writes.
    pub files: Vec<ExportFile>,
    /// What goes in, in the plugin's words (`{"tracks": 12, "cues": 40}`).
    pub counts: IndexMap<String, u64>,
    /// What the format can't hold, and what to do with the files. (Problems with this library
    /// are the plan's warnings.)
    pub notes: Vec<String>,
    #[serde(skip)]
    #[schemars(skip)]
    write: WriteExport,
    /// The plugin, while it's hosted: a plan kept doesn't keep it.
    #[serde(skip)]
    #[schemars(skip)]
    slot: Weak<Slot>,
}

/// A file an export writes, as the plugin says, and whether it's there already (and so
/// overwritten).
#[derive(Serialize, JsonSchema)]
pub struct ExportFile {
    #[serde(flatten)]
    pub file: ExportedFile,
    pub exists: bool,
}

impl Plugins {
    /// Plans an export with one of a plugin's exporters, of the drive `source.drive` names (by id,
    /// name or path; the plugin is told where it's mounted, whatever `source.root` says) to
    /// `output` (absolute), with `options` (checked against the exporter's
    /// schema; `null` is none), never on a drive or in the store. The plugin says which files it
    /// would write, writing none: the plan (operation `<plugin>.export.<exporter>`) is
    /// destructive when it would overwrite any, and stale once one of them changes. Applying it
    /// fails when the plugin writes other files than it said.
    #[allow(clippy::too_many_arguments)]
    pub fn export_plan(
        &self,
        plugin: &str,
        exporter: &str,
        source: ExportSource,
        output: &Path,
        options: Value,
        store: Option<&Store>,
        ctx: &Ctx<'_>,
    ) -> Result<Planned<PluginExport>> {
        let slot = self.slot(plugin)?;
        let declared = slot.declared("exporter", exporter, &slot.manifest.contributes.exporters, |e| &e.format)?;
        if !output.is_absolute() {
            return Err(Error::invalid(format!(
                "the export's output must be an absolute path, not {}",
                output.display()
            )));
        }
        outside_drives(output, store)?;
        let (context, device) =
            slot.context(Context { drive: Some(source.drive.clone()), ..Context::default() }, store)?;
        let options = super::fitting(declared.options.as_ref(), options, &context)?;
        let root = device.expect("a drive was named").root().to_path_buf();
        let source = ExportSource { drive: context.drive.clone().unwrap_or_default(), root, ..source };
        let mut write =
            WriteExport { exporter: exporter.into(), source, output: output.into(), options, dry_run: true, context };
        let Exported { files, counts, notes, warnings } = slot.call(method::WRITE_EXPORT, &write, store, ctx)?;
        write.dry_run = false;
        // Checked again before it's written: the files in the folder the output is in (an output
        // that's a folder of its own may not be there yet).
        let dir = output.parent().unwrap_or(output);
        let keys = files
            .iter()
            .map(|f| {
                let key = f.path.starts_with(output).then(|| key(dir, &f.path)).flatten();
                key.ok_or_else(|| slot.misbehaved(format!("it would write {} outside the output", f.path.display())))
            })
            .collect::<Result<Vec<_>>>()?;
        let files: Vec<ExportFile> =
            files.into_iter().map(|file| ExportFile { exists: file.path.exists(), file }).collect();
        let overwrites = files.iter().filter(|f| f.exists).count();
        let size: u64 = files.iter().map(|f| f.file.bytes).sum();
        let size = if size > 0 { format!(" ({})", crate::op::bytes(size)) } else { String::new() };
        let mut summary = vec![format!("write {} file(s){size} to {}", files.len(), output.display())];
        if !counts.is_empty() {
            let counted: Vec<String> = counts.iter().map(|(label, n)| format!("{n} {label}")).collect();
            summary.push(counted.join(", "));
        }
        if overwrites > 0 {
            summary.push(format!("overwrite {overwrites} of them"));
        }
        let (destructive, noop) = (overwrites > 0, files.is_empty());
        let export = PluginExport {
            plugin: plugin.into(),
            exporter: exporter.into(),
            output: output.into(),
            files,
            counts,
            notes,
            write,
            slot: Arc::downgrade(slot),
        };
        Ok(Planned::new(format!("{plugin}.export.{exporter}"), export, ())
            .summary(summary)
            .warnings(warnings)
            .destructive(destructive)
            .noop(noop)
            .touches(Touched::path(dir, true, vec![Probe::Stamp(keys)])))
    }
}

/// `file`'s key below `dir`, when it's there.
fn key(dir: &Path, file: &Path) -> Option<String> {
    let parts: Vec<String> =
        file.strip_prefix(dir).ok()?.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    match parts.iter().any(|p| p == "..") {
        true => None,
        false => Some(parts.join("/")),
    }
}

/// Refuses `output` (absolute) on a drive mounted here or in the store: plugins never write
/// to either.
fn outside_drives(output: &Path, store: Option<&Store>) -> Result<()> {
    let output = fsutil::resolved(output);
    let drives = api::list_drives(&api::ListDrives { all: true }).into_iter().map(|d| (d.root, "the drive at"));
    let stores = store.cloned().into_iter().chain(api::registry()).map(|s| (s.root().to_path_buf(), "the store at"));
    match drives.chain(stores).find(|(root, _)| output.starts_with(fsutil::resolved(root))) {
        Some((root, what)) => Err(Error::invalid(format!(
            "a plugin's export isn't written to {what} {}: choose a folder on this computer",
            root.display()
        ))),
        None => Ok(()),
    }
}

impl Operation for PluginExport {
    type Request = ();
    type Report = Exported;

    /// Has the plugin write the files it said it would, refusing (once written) any other. The
    /// folder the output is in is made first, as locallibrary's own exports make it.
    fn apply(planned: &Planned<Self>, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<Exported> {
        let e = &planned.plan;
        outside_drives(&e.output, store)?;
        let dir = e.output.parent().unwrap_or(&e.output);
        std::fs::create_dir_all(dir).at(dir)?;
        let slot = e.slot.upgrade().ok_or_else(|| Error::stale(format!("the {} plugin", e.plugin), "stopped"))?;
        let written: Exported = slot.call(method::WRITE_EXPORT, &e.write, store, ctx)?;
        // By path: a file's size is known exactly only once it's written.
        let said: Vec<&Path> = e.files.iter().map(|f| f.file.path.as_path()).collect();
        let wrote: Vec<&Path> = written.files.iter().map(|f| f.path.as_path()).collect();
        let unannounced: Vec<String> =
            wrote.iter().filter(|f| !said.contains(f)).map(|f| f.display().to_string()).collect();
        if !unannounced.is_empty() {
            return Err(slot.misbehaved(format!("it wrote what it didn't say it would: {}", unannounced.join(", "))));
        }
        let unwritten: Vec<String> =
            said.iter().filter(|f| !wrote.contains(f)).map(|f| f.display().to_string()).collect();
        if !unwritten.is_empty() {
            return Err(slot.misbehaved(format!("it didn't write what it said it would: {}", unwritten.join(", "))));
        }
        Ok(written)
    }
}
