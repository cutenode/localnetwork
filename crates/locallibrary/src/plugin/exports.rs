//! Plugins' exporters (`export/write`): planned with a dry run, which says which files the export
//! writes, so overwriting any is confirmed first; applied by the plugin writing them. An exporter
//! writes out a drive's library, a set or a mix, as its manifest's `source` says: the host checks
//! the request names that (and only that), and finds it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use indexmap::IndexMap;
use loclib_plugin::{Context, ExportFrom, ExportSource, Exported, ExportedFile, Exporter, WriteExport, method};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use super::Plugins;
use super::process::Slot;
use crate::api;
use crate::error::IoContext;
use crate::op::{Operation, Planned, Probe, Touched};
use crate::progress::Ctx;
use crate::sets::SetBook;
use crate::store::Store;
use crate::{Error, Result};

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
    /// Plans an export with one of a plugin's exporters, of what `source` names as the
    /// exporter's `source` says: the drive `source.drive` names (by id, name or path; the plugin
    /// is told where it's mounted, whatever `source.root` says), the set `source.set` names (as
    /// `sets show` finds one, the plugin told its id) or the mix kept in `store` as
    /// `source.mix`, a set's or a mix's with the drive its tracks are on when it names one. To
    /// `output` (absolute), with `options` (checked against the exporter's schema; `null` is
    /// none), never on a drive or in the store. The plugin says which files it would write,
    /// writing none: the plan (operation `<plugin>.export.<exporter>`) is destructive when it
    /// would overwrite any, and stale once one of them changes. Applying it fails when the plugin
    /// writes other files than it said.
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
        let source = of(declared, source, store)?;
        let drive = Some(source.drive.clone()).filter(|d| !d.is_empty());
        let (context, device) =
            slot.context(Context { drive, set: source.set.clone(), ..Context::default() }, store)?;
        let options = super::fitting(declared.options.as_ref(), options, &context)?;
        let root = device.map(|d| d.root().to_path_buf()).unwrap_or_default();
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

/// `source` as `exporter` writes it out, once it names what the exporter's `source` says, and
/// nothing another kind of exporter's does: a set by its id, found in `store`, and a mix kept
/// there.
fn of(exporter: &Exporter, mut source: ExportSource, store: Option<&Store>) -> Result<ExportSource> {
    let named = |what: &str| Error::invalid(format!("{} writes out {what}: say which", exporter.title));
    let not =
        |what: &str| Error::invalid(format!("{} writes out a {}, not {what}", exporter.title, exporter.source.name()));
    let lists = !source.playlists.is_empty() || !source.history.is_empty();
    let store = || store.ok_or_else(|| Error::invalid("sets and mixes are kept in the store, and there's none"));
    match exporter.source {
        ExportFrom::Drive if source.drive.is_empty() => return Err(named("a drive's library")),
        ExportFrom::Drive if source.set.is_some() => return Err(not("a set")),
        ExportFrom::Drive if source.mix.is_some() => return Err(not("a mix")),
        ExportFrom::Drive => {}
        ExportFrom::Set | ExportFrom::Mix if lists => return Err(not("a drive's playlists")),
        ExportFrom::Set if source.mix.is_some() => return Err(not("a mix")),
        ExportFrom::Set => {
            let set = source.set.as_deref().ok_or_else(|| named("a set"))?;
            source.set = Some(SetBook::load(store()?)?.find(set)?.id.clone());
        }
        ExportFrom::Mix if source.set.is_some() => return Err(not("a set")),
        ExportFrom::Mix => {
            let mix = source.mix.as_deref().ok_or_else(|| named("a mix"))?;
            crate::mix::report::load(store()?.root(), mix)?;
        }
    }
    Ok(source)
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
    let output = resolved(output);
    let drives = api::list_drives(&api::ListDrives { all: true }).into_iter().map(|d| (d.root, "the drive at"));
    let stores = store.cloned().into_iter().chain(api::registry()).map(|s| (s.root().to_path_buf(), "the store at"));
    match drives.chain(stores).find(|(root, _)| output.starts_with(resolved(root))) {
        Some((root, what)) => Err(Error::invalid(format!(
            "a plugin's export isn't written to {what} {}: choose a folder on this computer",
            root.display()
        ))),
        None => Ok(()),
    }
}

/// `path` with its symlinks resolved, as far as it's there.
fn resolved(path: &Path) -> PathBuf {
    let Some((there, rest)) = path.ancestors().find_map(|a| Some((a.canonicalize().ok()?, path.strip_prefix(a).ok()?)))
    else {
        return path.to_path_buf();
    };
    there.join(rest)
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
