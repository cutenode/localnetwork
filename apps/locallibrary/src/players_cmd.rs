//! `locallibrary players`: the player models `check` uses when none are given, and what each
//! model reads.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use console::style;
use locallibrary::api::{self, ClearPlayers, PlayersChange, PlayersPlan, RememberedPlayers, SetPlayers};
use locallibrary::equipment::{self, Exfat, Model};
use locallibrary::op::Planned;
use locallibrary::progress::Ctx;

use crate::output::{self, Outcome, Output, dim, heading, ok};

#[derive(Args)]
pub struct PlayersArgs {
    #[command(subcommand)]
    command: Option<PlayersCommand>,
}

#[derive(Subcommand)]
enum PlayersCommand {
    /// The models `check` uses when none are given (the default)
    Show,
    /// Remember the models you play on, for every library or one (--library)
    Set {
        /// e.g. CDJ-3000X,CDJ-2000NXS2 (repeatable or comma-separated)
        #[arg(required = true, value_delimiter = ',')]
        models: Vec<String>,
        /// Only for drives of this library (name or id)
        #[arg(long, short)]
        library: Option<String>,
        /// Show what would change without remembering them
        #[arg(long)]
        dry_run: bool,
    },
    /// Forget the remembered models (the default ones, or one library's)
    Clear {
        /// Only those remembered for this library (name or id)
        #[arg(long, short)]
        library: Option<String>,
        /// Show what would change without forgetting them
        #[arg(long)]
        dry_run: bool,
    },
    /// What each model reads: databases, file systems, audio formats; with a model, its details and sources
    Models { model: Option<String> },
}

pub fn run(args: PlayersArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let store = || crate::open_store(store_path);
    match args.command.unwrap_or(PlayersCommand::Show) {
        PlayersCommand::Show => {
            // Reading never creates a store.
            let r = match crate::diff_cmd::existing_store(store_path) {
                Some(store) => api::players_show(&store)?,
                None => RememberedPlayers::default(),
            };
            out.emit(&r, show);
        }
        PlayersCommand::Set { models, library, dry_run } => {
            let store = store()?;
            let plan = api::players_set_plan(&store, &SetPlayers { models, library })?;
            return change(&store, plan, dry_run, "Remember them?", "remembered", out, ctx);
        }
        PlayersCommand::Clear { library, dry_run } => {
            let store = store()?;
            let plan = api::players_clear_plan(&store, &ClearPlayers { library })?;
            return change(&store, plan, dry_run, "Forget them?", "forgotten", out, ctx);
        }
        PlayersCommand::Models { model: None } => out.emit_paged(&api::player_models(), |m| models(m)),
        PlayersCommand::Models { model: Some(name) } => {
            let Some(m) = equipment::find(&name) else {
                bail!("not a model locallibrary knows: {name} (`locallibrary players models` lists them)");
            };
            out.emit(m, details);
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Show the plan, ask, and remember (or forget) the models.
fn change(
    store: &locallibrary::Store,
    plan: Planned<PlayersPlan, PlayersChange>,
    dry_run: bool,
    question: &str,
    done: &str,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    if plan.noop {
        return Ok(out.nothing(&plan, "remembered already"));
    }
    if !out.json {
        output::summary(&plan.summary, &plan.warnings);
    }
    if let Some(o) = out.decide(dry_run, question) {
        return Ok(out.not_applied(&plan, o));
    }
    let r = plan.apply(Some(store), ctx)?;
    Ok(out.applied(&plan, &r, Outcome::Applied, || {
        ok(done);
        show(&r);
    }))
}

fn show(r: &RememberedPlayers) {
    if r.default.is_empty() && r.libraries.is_empty() {
        say!("No player models remembered. `locallibrary players set CDJ-3000X,...` makes `check` use them.");
        return;
    }
    if !r.default.is_empty() {
        say!("{:<20} {}", "Every library", r.default.join(", "));
    }
    for l in &r.libraries {
        let name = l.name.clone().unwrap_or_else(|| format!("{} (no longer in the store)", l.id));
        say!("{name:<20} {}", l.models.join(", "));
    }
}

fn reads(m: &Model) -> &'static str {
    match (m.export_pdb.value, m.onelibrary.value) {
        (true, true) => "both",
        (true, false) => "export.pdb",
        (false, true) => "OneLibrary",
        (false, false) => "-",
    }
}

fn exfat(m: &Model) -> String {
    match m.exfat.value {
        Exfat::Yes => "yes".into(),
        Exfat::No => "no".into(),
        Exfat::Firmware(v) => format!("{v}+"),
    }
}

/// `44.1`, `96`.
fn khz(rate: u32) -> String {
    (rate as f64 / 1000.0).to_string()
}

/// `MP3, AAC 48 kHz; WAV, AIFF, ALAC, FLAC 96 kHz`: formats grouped by their highest rate.
fn formats(m: &Model) -> String {
    let mut groups: Vec<(u32, Vec<&str>)> = Vec::new();
    for a in m.audio {
        let top = a.rates.iter().copied().max().unwrap_or(0);
        match groups.iter_mut().find(|(r, _)| *r == top) {
            Some((_, names)) => names.push(a.codec.name()),
            None => groups.push((top, vec![a.codec.name()])),
        }
    }
    let parts: Vec<String> = groups.iter().map(|(r, names)| format!("{} {} kHz", names.join(", "), khz(*r))).collect();
    parts.join("; ")
}

fn inferred(m: &Model) -> bool {
    m.audio_inferred
        || m.audio.iter().any(|a| a.inferred)
        || m.export_pdb.inferred
        || m.onelibrary.inferred
        || m.exfat.inferred
}

fn models(list: &[Model]) {
    say!("{}", style(format!("{:<14} {:<11} {:<6} {}", "Model", "Reads", "exFAT", "Formats (up to)")).bold());
    for m in list {
        let mark = if inferred(m) { "*" } else { "" };
        let what = if m.is_mixer() { "settings and firmware only".to_string() } else { formats(m) };
        say!("{:<14} {:<11} {:<6} {what}{mark}", m.name, reads(m), exfat(m));
    }
    say!();
    crate::output::hint(
        "* some values are inferred rather than documented. `locallibrary players models MODEL` shows the sources.",
    );
}

fn details(m: &Model) {
    heading(m.name);
    let mark = |inferred: bool| if inferred { " (inferred)" } else { "" };
    say!("  reads      {}{}", reads(m), mark(m.export_pdb.inferred || m.onelibrary.inferred));
    let fs = if m.is_mixer() { "FAT16, FAT32" } else { "FAT16, FAT32, HFS+" };
    let ex = match m.exfat.value {
        Exfat::Yes => ", exFAT".to_string(),
        Exfat::No => String::new(),
        Exfat::Firmware(v) => format!(", exFAT (firmware {v}+)"),
    };
    say!("  drives     {fs}{ex} on MBR; not NTFS{}", mark(m.exfat.inferred));
    for a in m.audio {
        let rates: Vec<String> = a.rates.iter().map(|r| khz(*r)).collect();
        let depths = if a.codec.lossless() {
            format!(", {}-bit", a.depths.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("/"))
        } else {
            String::new()
        };
        say!("  {:<10} {} kHz{depths}{}", a.codec.name(), rates.join("/"), mark(a.inferred));
    }
    if !m.is_mixer() && m.audio_inferred {
        say!("  {}", dim("(which formats it plays is inferred)"));
    }
    for n in m.notes {
        say!("  {}", dim(n));
    }
    say!("  {} {}", dim("source"), m.source);
}
