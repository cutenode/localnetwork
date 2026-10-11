//! `locallibrary doctor`: a drive's health report, and removing orphaned files.

use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, ValueEnum};
use console::style;
use locallibrary::api;
use locallibrary::doctor::{
    BrokenTrack, DoctorOptions, DoctorReport, DuplicateGroup, DuplicateReason, OrphanPlan, Problem, Section,
};
use locallibrary::format::Verdict;
use locallibrary::paths::FileClass;
use locallibrary::progress::Ctx;

use crate::each_drive::{self, Reads};
use crate::output::{self, Outcome, Output, bytes, cap, dim, heading, ok, warn};

#[derive(Args)]
#[command(after_long_help = "Examples:
  locallibrary doctor
  locallibrary doctor Home --only duplicates,unfiled --hash-audio
  locallibrary doctor Home --only duplicates --by-sound   also a FLAC and an MP3 of one recording
  locallibrary doctor Home --fix-orphans --dry-run    what deleting the orphaned files would remove")]
pub struct DoctorArgs {
    #[arg(help = crate::DEFAULT_LIBRARY_HELP)]
    drive: Option<String>,
    /// Also find duplicates by audio content (reads the audio files whose size matches another's)
    #[arg(long)]
    hash_audio: bool,
    /// Also find the same recording in other files and encodings by its sound (decodes every
    /// track the first time; the store keeps the fingerprints)
    #[arg(long)]
    by_sound: bool,
    /// Also open every audio file and decode its end, to find files cut short by an interrupted
    /// copy (reads a little of every file: slow on a big drive)
    #[arg(long)]
    audio: bool,
    /// Only these sections (repeatable, or comma-separated)
    #[arg(long, value_enum, value_delimiter = ',')]
    only: Vec<SectionArg>,
    /// Delete audio, analysis and artwork files no track in either database uses (backed up first;
    /// given no drive, on the only mounted one or the newest of the one library plugged in)
    #[arg(long)]
    fix_orphans: bool,
    /// With --fix-orphans: show what would be deleted, delete nothing
    #[arg(long)]
    dry_run: bool,
    /// List every duplicate group, missing file and orphan (not just the first of each)
    #[arg(long)]
    all: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum SectionArg {
    /// The same recording more than once
    Duplicates,
    /// Missing audio, analysis or artwork, and audio of the wrong size
    Missing,
    /// Files nothing references
    Orphans,
    /// Tracks in no playlist
    Unfiled,
    /// export.pdb and OneLibrary disagreeing
    Drift,
    /// Files of 4 GB or more (too big for FAT32)
    LargeFiles,
    /// How players get on with the drive's file system
    Compatibility,
    /// Audio files cut short or that don't decode (only when named, or with --audio)
    Audio,
}

impl From<SectionArg> for Section {
    fn from(s: SectionArg) -> Self {
        match s {
            SectionArg::Duplicates => Section::Duplicates,
            SectionArg::Missing => Section::Missing,
            SectionArg::Orphans => Section::Orphans,
            SectionArg::Unfiled => Section::Unfiled,
            SectionArg::Drift => Section::Drift,
            SectionArg::LargeFiles => Section::LargeFiles,
            SectionArg::Compatibility => Section::Compatibility,
            SectionArg::Audio => Section::Audio,
        }
    }
}

pub fn run(args: DoctorArgs, store: &Option<std::path::PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    crate::output::show_all(args.all);
    if args.fix_orphans {
        let drive = crate::jobs_cmd::drive_or_default(args.drive, store, "clean up")?;
        return fix_orphans(drive, args.dry_run, out, ctx);
    }
    let only = args.only.into_iter().map(Into::into).collect();
    let options = DoctorOptions {
        hash_audio: args.hash_audio,
        by_sound: args.by_sound,
        only,
        audio: args.audio,
        ..Default::default()
    };
    let reads = each_drive::read(args.drive, |drive| {
        api::doctor(&api::DoctorRequest { drive, options: options.clone() }, ctx)
    })?;
    out.page();
    let each = match &reads {
        Reads::One(report) => {
            let outcome = if report.has_problems() { Outcome::Problems } else { Outcome::Applied };
            return Ok(out.report(report, outcome, render));
        }
        Reads::Each(each) => each,
    };
    let problems = each.iter().filter_map(|r| r.result.as_ref()).any(DoctorReport::has_problems);
    Ok(match reads.emit(out, render) {
        Outcome::Applied if problems => Outcome::Problems,
        outcome => outcome,
    }
    .exit())
}

fn fix_orphans(drive: String, dry_run: bool, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let req = api::DriveRef { drive };
    let plan = api::doctor_orphans_plan(&req, ctx)?;
    if plan.noop {
        return Ok(out.nothing(&plan, "no orphaned files"));
    }
    if !out.json {
        orphan_plan(&plan);
        crate::output::clones(&plan);
    }
    if let Some(o) = out.decide(dry_run, "Delete them?") {
        return Ok(out.not_applied(&plan, o));
    }
    let report = plan.apply(None, ctx)?;
    Ok(out.applied(&plan, &report, Outcome::Applied, || {
        ok(format!("deleted {} file(s), {}", report.files_deleted, bytes(report.freed_bytes)));
        if let Some(b) = &report.backup {
            crate::output::hint(format!("undo with: locallibrary undo {:?} (backup {b})", req.drive));
        }
    }))
}

fn class_name(c: FileClass) -> &'static str {
    match c {
        FileClass::Audio => "audio",
        FileClass::Analysis => "analysis",
        FileClass::Artwork => "artwork",
        _ => "file",
    }
}

fn orphan_plan(p: &OrphanPlan) {
    say!("{}: delete {} file(s) no track uses, {}", style(&p.drive).bold(), p.files.len(), bytes(p.bytes));
    for f in p.files.iter().take(20) {
        say!("    - {:<9}{}  {}", class_name(f.class), f.path, dim(bytes(f.size)));
    }
    if p.files.len() > 20 {
        say!("    {}", dim(format!("... and {} more", p.files.len() - 20)));
    }
    crate::output::hint_at("  ", "they're backed up on the drive first; `locallibrary undo` brings them back");
    for w in &p.warnings {
        warn(w);
    }
}

/// Up to `max` lines, then "... and N more".
fn capped<T>(items: &[T], max: usize, line: impl Fn(&T) -> String) {
    let max = crate::output::cap(max);
    for i in items.iter().take(max) {
        say!("    {}", line(i));
    }
    if items.len() > max {
        say!("    {}", dim(format!("... and {} more", items.len() - max)));
    }
}

fn reason(r: DuplicateReason) -> &'static str {
    match r {
        DuplicateReason::SameFile => "same file",
        DuplicateReason::SameAudio => "same audio",
        DuplicateReason::SameRecording => "same recording, by sound",
        DuplicateReason::SyncManagerCopy => "Sync Manager copy",
        DuplicateReason::SameMetadata => "same artist, title and length",
    }
}

fn duplicates(groups: &[DuplicateGroup], drive: &str) {
    if groups.is_empty() {
        ok("no duplicates");
        return;
    }
    heading(&format!("Duplicates ({} group(s))", groups.len()));
    for g in groups.iter().take(cap(25)) {
        let reasons: Vec<&str> = g.reasons.iter().map(|r| reason(*r)).collect();
        say!("  {}  {}", g.tracks[0].track.name, dim(format!("({})", reasons.join(", "))));
        for t in &g.tracks {
            let keep = t.track.id == g.keep;
            let mark = if keep { style("keep").green().to_string() } else { "    ".into() };
            let added = if t.date_added.is_empty() { String::new() } else { format!(", added {}", t.date_added) };
            let audio = match t.lossless {
                true => format!("lossless {} Hz {}-bit", t.sample_rate, t.bit_depth),
                false => format!("{} kbps", t.bitrate),
            };
            let why = if keep { format!("  ({})", g.keep_reason) } else { String::new() };
            say!(
                "    {mark} {:>6}  {audio}, {} cue(s), {} playlist(s), {}{added}, played {}  {}{}",
                format!("#{}", t.track.id),
                t.cues,
                t.playlists,
                bytes(t.file_size),
                t.play_count,
                dim(&t.track.file_path),
                dim(why)
            );
        }
    }
    if groups.len() > cap(25) {
        say!("  {}", dim(format!("... and {} more groups (--all lists them all)", groups.len() - 25)));
    }
    crate::output::hint_at(
        "  ",
        format!("`locallibrary track dedupe {drive:?}` replaces each with the copy marked keep"),
    );
}

fn problem(p: Problem) -> &'static str {
    match p {
        Problem::MissingAudio => "no audio file",
        Problem::UnsafePath => "unsafe path",
        Problem::Truncated => "cut short",
        Problem::MissingAnalysis => "no analysis",
        Problem::MissingArtwork => "no artwork",
        _ => "broken",
    }
}

fn missing(list: &[BrokenTrack], drive: &str) {
    if list.is_empty() {
        ok("every track's audio, analysis and artwork is there");
        return;
    }
    heading(&format!("Missing or broken ({})", list.len()));
    capped(list, 25, |b| {
        let detail = b.detail.as_ref().map(|d| format!(" ({d})")).unwrap_or_default();
        let db = if b.database == "onelibrary" { " [OneLibrary]" } else { "" };
        format!("{:<14} {}{db}  {}", problem(b.problem), b.track.name, dim(format!("{}{detail}", b.path)))
    });
    if list.iter().any(|b| b.problem == Problem::MissingAudio) {
        crate::output::hint_at(
            "  ",
            format!("`locallibrary track relocate {drive:?} --look-in DIR` finds moved files, and copies in DIR"),
        );
    }
    if list.iter().any(|b| b.problem == Problem::MissingArtwork) {
        crate::output::hint_at(
            "  ",
            format!("`locallibrary art auto {drive:?}` fills in artwork from the files' tags or cover.jpg"),
        );
    }
}

pub fn render(r: &DoctorReport) {
    heading(&format!("{} {}", r.drive, dim(r.root.display())));
    say!("  {} tracks", r.tracks);
    if let Some(groups) = &r.duplicates {
        duplicates(groups, &r.drive);
    }
    if let Some(list) = &r.missing {
        missing(list, &r.drive);
    }
    if let Some(orphans) = &r.orphans {
        if orphans.is_empty() {
            ok("no orphaned files");
        } else {
            let total: u64 = orphans.iter().map(|o| o.size).sum();
            heading(&format!("Orphaned files ({}, {})", orphans.len(), bytes(total)));
            capped(orphans, 15, |o| format!("{:<9}{}  {}", class_name(o.class), o.path, dim(bytes(o.size))));
            crate::output::hint_at(
                "  ",
                format!("nothing uses them; delete them with: locallibrary doctor {:?} --fix-orphans", r.drive),
            );
        }
    }
    if let Some(unfiled) = &r.unfiled {
        if unfiled.is_empty() {
            ok("every track is in a playlist");
        } else {
            heading(&format!("In no playlist ({})", unfiled.len()));
            capped(unfiled, 10, |t| t.name.clone());
            crate::output::hint_at(
                "  ",
                format!(
                    "`locallibrary playlist unfiled {:?}` collects them in a playlist, so players can find them",
                    r.drive
                ),
            );
        }
    }
    if let Some(drift) = &r.drift {
        match (&drift.error, drift.is_empty()) {
            (Some(e), _) => warn(format!("OneLibrary can't be read: {e}")),
            (None, true) => ok("export.pdb and OneLibrary agree"),
            (None, false) => {
                heading("export.pdb and OneLibrary differ");
                say!(
                    "    {} track(s) only in export.pdb, {} only in OneLibrary, {} with different details, {} playlist(s)",
                    drift.only_in_device_library.len(),
                    drift.only_in_onelibrary.len(),
                    drift.changed_tracks.len(),
                    drift.playlists.len()
                );
                crate::output::hint_at(
                    "    ",
                    "see `locallibrary drive info`; `locallibrary convert --replace` rebuilds one from the other",
                );
            }
        }
    }
    if let Some(large) = &r.large_files {
        if large.is_empty() {
            ok("no files of 4 GB or more");
        } else {
            heading(&format!("Files FAT32 can't hold ({})", large.len()));
            capped(large, 10, |f| format!("{}  {}", f.path, dim(bytes(f.size))));
        }
    }
    if let Some(audio) = &r.audio {
        if audio.problems.is_empty() {
            ok(format!("all {} audio file(s) open and decode to the end", audio.checked));
        } else {
            heading(&format!("Audio files with problems ({} of {})", audio.problems.len(), audio.checked));
            capped(&audio.problems, 15, |f| {
                format!("{}  {}", f.track.name, dim(format!("{}  {}", f.problem, f.track.file_path)))
            });
        }
    }
    if let Some(c) = &r.compatibility {
        let fs = r.file_system.clone().unwrap_or_else(|| "file system".into());
        let line = format!("{fs}: {}", c.summary);
        match c.verdict {
            Verdict::AllPlayers => ok(line),
            Verdict::SomePlayers | Verdict::Unknown => warn(line),
            Verdict::NoPlayers => say!("{} {line}", style("✗").red().bold()),
        }
        for n in &c.notes {
            say!("    {}", dim(n));
        }
    }
    for w in &r.warnings {
        output::warn(w);
    }
}
