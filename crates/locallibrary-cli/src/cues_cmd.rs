//! `locallibrary cues`: list and edit a track's hot cues, memory cues and loops.

use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use console::style;
use locallibrary::api::{self, CueOp, EditCues, ListCues, TrackSelector};
use locallibrary::cue_edit::{
    CueInfo, CuePlan, CuePosition, CueRef, CueReport, HotCue, SetCue, TrackCues, UpdateCue, clock,
};
use locallibrary::progress::Ctx;

use crate::output::{Outcome, Output, dim, heading, ok, warn};

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true, subcommand_negates_reqs = true)]
pub struct CuesArgs {
    #[command(subcommand)]
    command: Option<CuesCommand>,
    /// Drive whose track's cues to list (`cues list`)
    drive: Option<String>,
    #[command(flatten)]
    track: TrackArgs,
}

#[derive(Subcommand)]
enum CuesCommand {
    /// A track's cues
    List {
        #[arg(help = crate::DRIVE_HELP)]
        drive: String,
        #[command(flatten)]
        track: TrackArgs,
    },
    /// Add a hot cue or memory cue, or move the hot cue in that slot (it keeps its name and colour)
    #[command(
        after_help = "Examples:\n  locallibrary cues set Main --track 12 --hot B --at 1:02.5 --loop 8 --name Drop --colour red\n  \
locallibrary cues set Main --track 12 --memory --beat 129"
    )]
    Set {
        #[arg(help = crate::DRIVE_HELP)]
        drive: String,
        #[command(flatten)]
        track: TrackArgs,
        #[command(flatten)]
        slot: Slot,
        #[command(flatten)]
        position: Position,
        /// Move --at to the nearest beat of the grid
        #[arg(long)]
        snap: bool,
        /// Make it a loop this many beats long (8, 4, 0.5...)
        #[arg(long = "loop", value_name = "BEATS")]
        loop_beats: Option<f64>,
        /// The cue's name (players show it with the cue)
        #[arg(long)]
        name: Option<String>,
        /// A name (red, orange, yellow, green, aqua, blue, purple, pink...), #rrggbb, or none
        #[arg(long, alias = "color")]
        colour: Option<String>,
        /// Show the plan without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Rename a cue ("" clears the name)
    Rename {
        #[arg(help = crate::DRIVE_HELP)]
        drive: String,
        #[command(flatten)]
        track: TrackArgs,
        #[command(flatten)]
        which: Which,
        name: String,
        /// Show the plan without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Recolour cues: a name (red, aqua, violet...), #rrggbb, or none
    #[command(alias = "color")]
    Colour {
        #[arg(help = crate::DRIVE_HELP)]
        drive: String,
        #[command(flatten)]
        track: TrackArgs,
        #[command(flatten)]
        which: Which,
        colour: String,
        /// Show the plan without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Delete cues
    Rm {
        #[arg(help = crate::DRIVE_HELP)]
        drive: String,
        #[command(flatten)]
        track: TrackArgs,
        #[command(flatten)]
        which: Which,
        /// Show the plan without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Replace tracks' cues with a copy of another track's (e.g. onto a duplicate)
    Copy {
        #[arg(help = crate::DRIVE_HELP)]
        drive: String,
        /// Track to copy from
        #[arg(long, value_name = "ID")]
        from: u32,
        /// Track(s) to copy onto
        #[arg(long, value_name = "ID", required = true)]
        to: Vec<u32>,
        /// Show the plan without changing anything
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Args, Clone)]
struct TrackArgs {
    /// Track id
    #[arg(long = "track", value_name = "ID")]
    tracks: Vec<u32>,
    /// Track matching "artist - title" or path
    #[arg(long)]
    query: Option<String>,
}

impl TrackArgs {
    fn selector(&self) -> TrackSelector {
        TrackSelector { ids: self.tracks.clone(), query: self.query.clone(), ..Default::default() }
    }
}

#[derive(Args)]
#[group(required = true, multiple = false)]
struct Slot {
    /// Hot cue A-H
    #[arg(long, value_name = "A-H")]
    hot: Option<HotCue>,
    /// A memory cue
    #[arg(long)]
    memory: bool,
}

#[derive(Args)]
#[group(required = true, multiple = false)]
struct Position {
    /// Time: 62.5, 1:02.5 or 1:02:03.5
    #[arg(long, value_name = "TIME", value_parser = parse_time)]
    at: Option<u32>,
    /// Beat of the track's beat grid, counting from 1
    #[arg(long, value_name = "N")]
    beat: Option<u32>,
}

impl Slot {
    fn get(&self) -> Option<HotCue> {
        if self.memory { None } else { self.hot }
    }
}

impl Position {
    fn get(&self) -> CuePosition {
        match (self.at, self.beat) {
            (Some(ms), _) => CuePosition::Ms(ms),
            (_, Some(b)) => CuePosition::Beat(b),
            _ => unreachable!("clap requires one"),
        }
    }
}

/// Which cues: a hot cue, the memory cue at a position, or all of them.
#[derive(Args)]
#[group(required = true, multiple = false)]
struct Which {
    /// Hot cue A-H
    #[arg(long, value_name = "A-H")]
    hot: Option<HotCue>,
    /// The memory cue at this time (within 0.1 s)
    #[arg(long, value_name = "TIME", value_parser = parse_time)]
    memory_at: Option<u32>,
    /// The memory cue on this beat
    #[arg(long, value_name = "N")]
    memory_beat: Option<u32>,
    /// Every cue
    #[arg(long)]
    all: bool,
}

impl Which {
    fn get(&self) -> CueRef {
        match (self.hot, self.memory_at, self.memory_beat) {
            (Some(h), _, _) => CueRef::Hot(h),
            (_, Some(ms), _) => CueRef::Memory(CuePosition::Ms(ms)),
            (_, _, Some(b)) => CueRef::Memory(CuePosition::Beat(b)),
            _ => {
                debug_assert!(self.all, "clap requires one");
                CueRef::All
            }
        }
    }
}

/// `62.5`, `1:02.5`, `1:02:03.5` -> milliseconds.
fn parse_time(s: &str) -> std::result::Result<u32, String> {
    let bad = || format!("not a time: {s:?} (like 62.5, 1:02.5 or 1:02:03.5)");
    let parts: Vec<&str> = s.trim().split(':').collect();
    if parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
        return Err(bad());
    }
    let (whole, last) = parts.split_at(parts.len() - 1);
    let secs: f64 = last[0].parse().map_err(|_| bad())?;
    if !secs.is_finite() || secs < 0.0 || (!whole.is_empty() && secs >= 60.0) {
        return Err(bad());
    }
    let mut minutes = 0u64;
    for p in whole {
        let n: u64 = p.parse().map_err(|_| bad())?;
        minutes = minutes * 60 + n;
    }
    let ms = minutes as f64 * 60_000.0 + (secs * 1000.0).round();
    if ms > f64::from(u32::MAX) {
        return Err(bad());
    }
    Ok(ms as u32)
}

pub fn run(args: CuesArgs, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let command = match args.command {
        Some(c) => c,
        None => {
            let Some(drive) = args.drive else {
                bail!("give a drive and a track: locallibrary cues <drive> --track ID")
            };
            CuesCommand::List { drive, track: args.track }
        }
    };
    let (req, dry_run) = match command {
        CuesCommand::List { drive, track } => {
            let list = api::cues_list(&ListCues { drive, tracks: track.selector() })?;
            out.emit(&list, |l| render_list(l));
            return Ok(ExitCode::SUCCESS);
        }
        CuesCommand::Set { drive, track, slot, position, snap, loop_beats, name, colour, dry_run } => {
            let set = SetCue { hot_cue: slot.get(), at: position.get(), snap, loop_beats, name, color: colour };
            (EditCues { drive, tracks: track.selector(), edit: CueOp::Set(set) }, dry_run)
        }
        CuesCommand::Rename { drive, track, which, name, dry_run } => {
            let update = UpdateCue { cue: which.get(), name: Some(name), color: None };
            (EditCues { drive, tracks: track.selector(), edit: CueOp::Update(update) }, dry_run)
        }
        CuesCommand::Colour { drive, track, which, colour, dry_run } => {
            let update = UpdateCue { cue: which.get(), name: None, color: Some(colour) };
            (EditCues { drive, tracks: track.selector(), edit: CueOp::Update(update) }, dry_run)
        }
        CuesCommand::Rm { drive, track, which, dry_run } => {
            (EditCues { drive, tracks: track.selector(), edit: CueOp::Delete { cue: which.get() } }, dry_run)
        }
        CuesCommand::Copy { drive, from, to, dry_run } => {
            let from = TrackSelector { ids: vec![from], ..Default::default() };
            let tracks = TrackSelector { ids: to, ..Default::default() };
            (EditCues { drive, tracks, edit: CueOp::Copy { from } }, dry_run)
        }
    };
    let plan = api::cues_plan(&req, ctx)?;
    if plan.noop {
        if !out.json {
            for w in &plan.warnings {
                warn(w);
            }
        }
        return Ok(out.nothing(&plan, "nothing to change"));
    }
    if !out.json {
        render_plan(&plan);
    }
    if let Some(o) = out.decide(dry_run, "Write these cues?") {
        return Ok(out.not_applied(&plan, o));
    }
    let report = plan.apply(None, ctx)?;
    Ok(out.applied(&plan, &report, Outcome::Applied, || render_report(&report, &req.drive)))
}

/// `B  1:02.500  beat 134  loop 8 beats to 1:06.250  "Drop"  #e62828`
fn cue_line(c: &CueInfo) -> String {
    let mut line = match &c.hot_cue {
        Some(h) => format!("{}  {}", style(h).bold(), clock(c.start_ms)),
        None => format!("{}  {}", style("◆").dim(), clock(c.start_ms)),
    };
    if let Some(b) = c.beat {
        line.push_str(&format!("  {}", dim(format!("beat {b}"))));
    }
    if let Some(end) = c.end_ms {
        let length = c.loop_beats.as_ref().map(|n| format!("{n} beat{} ", if n == "1" { "" } else { "s" }));
        line.push_str(&format!("  loop {}to {}", length.unwrap_or_default(), clock(end)));
        if c.active_loop {
            line.push_str(&format!(" {}", dim("(active)")));
        }
    }
    if !c.name.is_empty() {
        line.push_str(&format!("  {:?}", c.name));
    }
    if let Some(color) = &c.color {
        line.push_str(&format!("  {}", dim(color)));
    }
    line
}

fn render_list(list: &[TrackCues]) {
    for t in list {
        heading(&format!("{}  {}", t.track.name, dim(format!("#{}, {:.2} BPM", t.track.id, t.bpm))));
        if t.cues.is_empty() {
            say!("  {}", dim("no cues"));
        }
        for c in &t.cues {
            say!("  {}", cue_line(c));
        }
        if let Some(n) = t.onelibrary_cues.filter(|n| *n != t.cues.len()) {
            warn(format!(
                "exportLibrary.db has {n} cue(s) for this track; editing its cues makes it match the analysis files"
            ));
        }
        for w in &t.warnings {
            warn(w);
        }
    }
}

fn render_plan(p: &CuePlan) {
    say!("{}: change the cues of {} track(s)", style(&p.drive).bold(), p.tracks.len());
    for t in &p.tracks {
        say!("  {}", t.track.name);
        for c in t.before.iter().filter(|c| !t.after.contains(c)) {
            say!("    {} {}", style("-").red().bold(), cue_line(c));
        }
        for c in t.after.iter().filter(|c| !t.before.contains(c)) {
            say!("    {} {}", style("+").green().bold(), cue_line(c));
        }
        let databases = if t.onelibrary { " and exportLibrary.db" } else { "" };
        say!("    {}", dim(format!("{} analysis file(s){databases}", t.files.len())));
    }
    for w in &p.warnings {
        warn(w);
    }
}

fn render_report(r: &CueReport, drive: &str) {
    ok(format!("updated the cues of {} track(s) ({} analysis file(s))", r.tracks_changed, r.files_written.len()));
    if let Some(c) = &r.onelibrary {
        say!(
            "  {}",
            dim(format!("exportLibrary.db: {} cue(s) updated, {} added, {} removed", c.updated, c.added, c.removed))
        );
    }
    if let Some(b) = &r.backup {
        crate::output::hint(format!("undo with: locallibrary undo {drive:?} (backup {b})"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times() {
        assert_eq!(parse_time("62.5"), Ok(62_500));
        assert_eq!(parse_time("1:02.5"), Ok(62_500));
        assert_eq!(parse_time("1:02:03.5"), Ok(3_723_500));
        assert_eq!(parse_time("0:00.0004"), Ok(0));
        assert!(parse_time("1:75").is_err());
        assert!(parse_time("a:10").is_err());
        assert!(parse_time("1::2").is_err());
        assert!(parse_time("-3").is_err());
    }
}
