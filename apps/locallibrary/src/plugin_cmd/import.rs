//! `import`: another program's file read with a plugin's importer (`--format`), what it says about
//! a drive's tracks added to them and the plays it logged kept as sessions, shown and asked about
//! first as an edit is.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use clap::builder::{PossibleValue, PossibleValuesParser};
use locallibrary::Store;
use locallibrary::op::Planned;
use locallibrary::plugin::PluginImport;
use locallibrary::progress::Ctx;
use loclib_plugin::{ImportKind, Importer};

use crate::jobs_cmd::usage;
use crate::output::{self, Outcome, Output, dim, warn};

#[derive(Args)]
#[command(after_long_help = "Examples:
  locallibrary import --format traktor collection.nml Home --dry-run
  locallibrary import collection.nml          the importer for .nml files, onto the only drive")]
pub struct ImportArgs {
    /// The file to read
    file: PathBuf,
    #[arg(help = crate::LIBRARY_HELP)]
    drive: Option<String>,
    /// What reads it: a format a plugin reads (`locallibrary plugin info` lists them) [default:
    /// the only one for the file's extension]
    #[arg(long, value_parser = formats())]
    format: Option<String>,
    /// An option for the format, like key=value (repeatable; `locallibrary plugin info` shows them)
    #[arg(long = "option", value_name = "KEY=VALUE")]
    options: Vec<String>,
    /// Show the plan without changing anything
    #[arg(long)]
    dry_run: bool,
}

/// `--format`'s values: the formats plugins read.
fn formats() -> PossibleValuesParser {
    let plugins = crate::plugins::checked().into_iter().flat_map(|p| p.manifests());
    let read = plugins.flat_map(|m| &m.contributes.importers).map(|i| PossibleValue::new(&i.format).help(&i.title));
    PossibleValuesParser::new(read.collect::<Vec<_>>())
}

/// Plans the import, shows it, and asks before making it.
pub fn import(args: ImportArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let plugins = crate::plugins::host()?;
    let all = plugins.manifests().flat_map(|m| m.contributes.importers.iter().map(move |i| (m.id.as_str(), i)));
    let all: Vec<(&str, &Importer)> = all.collect();
    let file = std::path::absolute(&args.file)?;
    let (plugin, importer) = match &args.format {
        Some(format) => *all.iter().find(|(_, i)| i.format == *format).expect("--format takes the formats there are"),
        None => {
            let extension = file.extension().map(|e| e.to_string_lossy().to_lowercase());
            let fits: Vec<_> = all.iter().filter(|(_, i)| i.extension.is_some() && i.extension == extension).collect();
            match fits[..] {
                [one] => *one,
                _ => return Err(usage("say what reads it: --format FORMAT (`locallibrary plugin info` lists them)")),
            }
        }
    };
    let options = super::options("format", importer.options.as_ref(), &args.options)?;
    let drive = crate::jobs_cmd::drive_or_default(args.drive, store_path, "import")?;
    // Sessions are kept in the store, made for them if there's none yet.
    let store = match importer.provides.contains(&ImportKind::Sessions) {
        true => Some(Store::open_or_init(store_path.clone().unwrap_or_else(Store::default_root))?),
        false => crate::diff_cmd::existing_store(store_path)?,
    };
    let plan = plugins.import_plan(plugin, &importer.format, &file, &drive, options, store.as_ref(), ctx)?;
    if !out.json {
        render(&plan);
    }
    if plan.noop {
        return Ok(out.nothing(&plan, "nothing to import"));
    }
    if let Some(o) = out.decide_on(&plan, args.dry_run, "Import it?")? {
        return Ok(out.not_applied(&plan, o));
    }
    let report = plan.apply(store.as_ref(), ctx)?;
    Ok(out.applied(&plan, &report, Outcome::Applied, || {
        if let Some(r) = &report.edits {
            crate::edit_cmd::render_report(r, &drive);
        }
        if !report.sessions.is_empty() {
            output::ok(format!("kept {} session(s) (`locallibrary plays` lists them)", report.sessions.len()));
        }
    }))
}

/// What an import would do: what it found, the edits, the sessions, then what it left out.
fn render(p: &Planned<PluginImport>) {
    output::summary(&p.summary[..1], &[]);
    if !p.edits.is_noop() {
        crate::edit_cmd::render_plan(&p.edits);
    }
    for s in &p.sessions {
        say!("  {} {}", s.name, dim(format!("{} play(s)", s.plays.len())));
    }
    for w in p.warnings.iter().filter(|w| !p.edits.warnings.contains(w)) {
        warn(w);
    }
}
