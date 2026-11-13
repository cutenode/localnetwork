//! The command line's jobs, from the palette. Those that report on one drive or plan a change to
//! it run here as the player's own jobs do: a report shows in a panel, a change is planned in the
//! background and confirmed in the same dialog as an edit, and `u` takes it back. Each is the
//! library call its command makes. What the player doesn't do itself (formatting, firmware, the
//! store's upkeep) is shown as the command to run in a terminal, the drive filled in.

use std::path::{Path, PathBuf};

use locallibrary::Store;
use locallibrary::api::{self, DriveRef};
use locallibrary::check::Level as FindingLevel;
use locallibrary::doctor::{DoctorOptions, DoctorReport, Problem, Section};
use locallibrary::format::Verdict;
use locallibrary::op::bytes;
use locallibrary::plays::PlayStats;
use locallibrary::relocate::RelocateOptions;
use locallibrary::space::SpaceReport;
use locallibrary::upgrade::UpgradeOptions;

use super::app::{App, Level};
use super::drives::{Report, Tone};
use super::edits::{Modal, Prompt, PromptFor};
use crate::output::plural;

/// A command line job the player runs on the drive (the open one, or the one selected in the
/// Drives view).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cli {
    /// `doctor`: the drive's health report.
    Doctor,
    /// `doctor --only duplicates --by-sound`.
    BySound,
    /// `track dedupe`.
    Dedupe,
    /// `track relocate`, looking in a folder named too.
    Relocate,
    /// `track upgrade`, from a folder named.
    Upgrade,
    /// `refresh`, after rekordbox exported to the drive.
    Refresh,
    /// `drive space`.
    Space,
    /// `plays stats`.
    PlayStats,
}

/// How many of a report's items are listed before "and N more".
const LISTED: usize = 25;

impl App {
    /// Run `job` on the drive actions work on.
    pub(super) fn run_cli(&mut self, job: Cli) {
        let Some(root) = self.action_drive() else {
            return self.toast("open a drive first", Level::Info);
        };
        let drive = root.display().to_string();
        match job {
            Cli::Doctor => self.doctor(root, false),
            Cli::BySound => self.doctor(root, true),
            Cli::Space => self.drive_space(root),
            Cli::PlayStats => self.play_stats(root),
            Cli::Dedupe => self.plan(root, move |ctx| {
                let req = api::DedupeRequest { drive, hash_audio: false, by_sound: false, metadata: false };
                Ok(Box::new(api::dedupe_plan(&req, ctx)?))
            }),
            Cli::Relocate | Cli::Upgrade => {
                let upgrade = job == Cli::Upgrade;
                let label = match upgrade {
                    true => "Lossless files in",
                    false => "Look for the missing files on the drive and in (empty: the drive alone)",
                };
                let purpose = PromptFor::LookIn { upgrade };
                self.modal = Some(Modal::Prompt(Prompt { label: label.into(), text: String::new(), purpose, root }));
            }
            Cli::Refresh => {
                let store_root = self.store_root.clone();
                self.plan(root, move |ctx| {
                    let store = Store::open_or_init(&store_root)?;
                    let req = api::RefreshRequest { drive: Some(drive), ..Default::default() };
                    Ok(Box::new(api::refresh_plan(&store, &req, ctx)?))
                });
            }
        }
    }

    /// The folder typed for relocating or upgrading: planned when it's one (relocating needs
    /// none), else said, the prompt staying open.
    pub(super) fn looked_in(&mut self, p: Prompt, upgrade: bool) {
        let typed = p.text.trim();
        let folder = (!typed.is_empty()).then(|| home(typed));
        let why = match &folder {
            None if upgrade => Some("name the folder the lossless files are in".to_string()),
            Some(f) if !f.is_dir() => Some(format!("{} isn't a folder", f.display())),
            _ => None,
        };
        if let Some(why) = why {
            self.toast(why, Level::Info);
            self.modal = Some(Modal::Prompt(p));
            return;
        }
        let drive = p.root.display().to_string();
        let folders: Vec<PathBuf> = folder.into_iter().collect();
        match upgrade {
            true => self.plan(p.root, move |ctx| {
                let req = api::UpgradeRequest { drive, options: UpgradeOptions { folders, ..Default::default() } };
                Ok(Box::new(api::upgrade_plan(&req, ctx)?))
            }),
            false => self.plan(p.root, move |ctx| {
                let req = api::RelocateRequest { drive, options: RelocateOptions { folders } };
                Ok(Box::new(api::relocate_plan(&req, ctx)?))
            }),
        }
    }

    /// The drive's health report (`by_sound`: its duplicates alone, found by their sound too), in
    /// a panel.
    fn doctor(&mut self, root: PathBuf, by_sound: bool) {
        let name = self.drive_label(&root);
        let label = if by_sound { format!("Listening for duplicates on {name}") } else { format!("Examining {name}") };
        let drive = root.display().to_string();
        self.task(label, move |ctx| {
            let only = if by_sound { vec![Section::Duplicates] } else { Vec::new() };
            let options = DoctorOptions { by_sound, only, ..Default::default() };
            let report = crate::plugins::roles::doctor(&api::DoctorRequest { drive, options }, None, ctx)?;
            let lines = doctor_lines(&report, by_sound);
            let title = format!("Doctor · {}", report.drive);
            Ok(Box::new(move |app: &mut App| app.modal = Some(Modal::Report(Report { title, lines, scroll: 0 }))))
        });
    }

    /// What's using the drive's space, in a panel.
    fn drive_space(&mut self, root: PathBuf) {
        let label = format!("Measuring {}", self.drive_label(&root));
        let (drive, store_root) = (root.display().to_string(), self.store_root.clone());
        self.task(label, move |ctx| {
            let store = Store::open(&store_root).ok();
            let report = api::drive_space(store.as_ref(), &DriveRef { drive }, ctx)?;
            let (title, lines) = (format!("Space · {}", report.drive), space_lines(&report));
            Ok(Box::new(move |app: &mut App| app.modal = Some(Modal::Report(Report { title, lines, scroll: 0 }))))
        });
    }

    /// The drive's play counts, in a panel.
    fn play_stats(&mut self, root: PathBuf) {
        let label = format!("Counting plays on {}", self.drive_label(&root));
        let (source, store_root) = (root.display().to_string(), self.store_root.clone());
        self.task(label, move |_| {
            let store = Store::open(&store_root).ok();
            let req = api::PlayStatsRequest { source, since: None, limit: Some(LISTED) };
            let stats = api::play_stats(store.as_ref(), &req)?;
            let (title, lines) = (format!("Plays · {}", stats.source), stats_lines(&stats));
            Ok(Box::new(move |app: &mut App| app.modal = Some(Modal::Report(Report { title, lines, scroll: 0 }))))
        });
    }

    /// The command line for what the player doesn't do itself, to copy into a terminal: `DRIVE` in
    /// `command` is the drive actions work on.
    pub(super) fn copy_command(&mut self, command: &'static str) {
        let line = command_line(command, self.action_drive().as_deref(), &self.store_root);
        let mut lines = vec![
            (Tone::Text, "The player doesn't do this itself. In a terminal, run:".to_string()),
            (Tone::Text, String::new()),
            (Tone::Row, line.clone()),
            (Tone::Text, String::new()),
        ];
        if line.contains('<') {
            lines.push((Tone::Faint, "What's in <brackets> is yours to fill in.".into()));
        }
        let words: Vec<&str> =
            command.split_whitespace().take_while(|w| *w != "DRIVE" && !w.starts_with(['<', '-'])).collect();
        lines.push((Tone::Faint, format!("locallibrary {} --help says more.", words.join(" "))));
        lines.push((Tone::Faint, "Most terminals select text over the player with shift (or option) held.".into()));
        let title = format!("In a terminal · {}", words.join(" "));
        self.modal = Some(Modal::Report(Report { title, lines, scroll: 0 }));
    }
}

/// `~/x` as the folder in the home folder.
fn home(typed: &str) -> PathBuf {
    match typed.strip_prefix("~/") {
        Some(rest) => std::env::home_dir().map_or_else(|| typed.into(), |h| h.join(rest)),
        None => typed.into(),
    }
}

/// `command` as a line to paste: `DRIVE` the drive at `root` (`<drive>` with none), and the store
/// named when it isn't the one the command line uses by default.
pub fn command_line(command: &str, root: Option<&Path>, store: &Path) -> String {
    let drive = root.map_or_else(|| "<drive>".to_string(), |r| quoted(&r.display().to_string()));
    let store = match store == Store::default_root() {
        true => String::new(),
        false => format!(" --store {}", quoted(&store.display().to_string())),
    };
    let words: Vec<String> =
        command.split(' ').map(|w| if w == "DRIVE" { drive.clone() } else { w.to_string() }).collect();
    format!("locallibrary{store} {}", words.join(" "))
}

/// `s` as a shell reads it back: in single quotes unless it's only letters, digits and `/._-+:@%,`.
pub fn quoted(s: &str) -> String {
    let plain = !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+:@%,".contains(c));
    if plain { s.to_string() } else { format!("'{}'", s.replace('\'', r"'\''")) }
}

/// Up to [`LISTED`] lines of `items`, then how many more.
fn listed<T>(out: &mut Vec<(Tone, String)>, items: &[T], line: impl Fn(&T) -> String) {
    out.extend(items.iter().take(LISTED).map(|i| (Tone::Faint, format!("    {}", line(i)))));
    if items.len() > LISTED {
        out.push((Tone::Faint, format!("    and {} more", items.len() - LISTED)));
    }
}

/// The lines of a doctor's report: each section run, what it found or that it's fine, and what
/// in the palette fixes it.
pub fn doctor_lines(r: &DoctorReport, by_sound: bool) -> Vec<(Tone, String)> {
    let mut out = vec![
        if r.has_problems() {
            (Tone::Problem, "Something to look at.".to_string())
        } else {
            (Tone::Ok, "Healthy.".to_string())
        },
        (Tone::Faint, plural(r.tracks, "track")),
    ];
    let fine = |out: &mut Vec<(Tone, String)>, what: &str| out.push((Tone::Ok, what.to_string()));
    if let Some(groups) = &r.duplicates {
        if groups.is_empty() {
            fine(&mut out, "no duplicates");
        } else {
            out.push((Tone::Text, String::new()));
            out.push((Tone::Heading, format!("Duplicates ({})", plural(groups.len(), "group"))));
            for g in groups.iter().take(LISTED) {
                let reasons: Vec<&str> = g.reasons.iter().map(|r| crate::doctor_cmd::reason(*r)).collect();
                out.push((Tone::Warning, format!("{} ({})", g.tracks[0].track.name, reasons.join(", "))));
                for t in &g.tracks {
                    let keep = if t.track.id == g.keep { "keep " } else { "     " };
                    out.push((Tone::Faint, format!("    {keep}{}", t.track.file_path)));
                }
            }
            if groups.len() > LISTED {
                out.push((Tone::Faint, format!("    and {} more", groups.len() - LISTED)));
            }
            out.push((
                Tone::Faint,
                match by_sound {
                    false => "    replace them from the commands (:): replace duplicates".into(),
                    true => format!(
                        "    locallibrary track dedupe {} --by-sound replaces them",
                        quoted(&r.root.display().to_string())
                    ),
                },
            ));
        }
    }
    if let Some(list) = &r.missing {
        if list.is_empty() {
            fine(&mut out, "every track's audio, analysis and artwork is there");
        } else {
            out.push((Tone::Text, String::new()));
            out.push((Tone::Heading, format!("Missing or broken ({})", list.len())));
            listed(&mut out, list, |b| {
                format!("{}: {}  {}", crate::doctor_cmd::problem(b.problem), b.track.name, b.path)
            });
            if list.iter().any(|b| b.problem == Problem::MissingAudio) {
                out.push((Tone::Faint, "    find them from the commands (:): find the tracks' missing files".into()));
            }
        }
    }
    if let Some(orphans) = &r.orphans {
        if orphans.is_empty() {
            fine(&mut out, "no orphaned files");
        } else {
            let total: u64 = orphans.iter().map(|o| o.size).sum();
            out.push((Tone::Text, String::new()));
            out.push((Tone::Heading, format!("Orphaned files ({}, {})", orphans.len(), bytes(total))));
            listed(&mut out, orphans, |o| {
                format!("{} {}  {}", crate::doctor_cmd::class_name(o.class), o.path, bytes(o.size))
            });
            out.push((Tone::Faint, "    nothing uses them: locallibrary doctor --fix-orphans deletes them".into()));
        }
    }
    if let Some(unfiled) = &r.unfiled {
        if unfiled.is_empty() {
            fine(&mut out, "every track is in a playlist");
        } else {
            out.push((Tone::Text, String::new()));
            out.push((Tone::Heading, format!("In no playlist ({})", unfiled.len())));
            listed(&mut out, unfiled, |t| t.name.clone());
        }
    }
    if let Some(drift) = &r.drift {
        match (&drift.error, drift.is_empty()) {
            (Some(e), _) => out.push((Tone::Warning, format!("OneLibrary can't be read: {e}"))),
            (None, true) => fine(&mut out, "export.pdb and OneLibrary agree"),
            (None, false) => out.push((
                Tone::Warning,
                format!(
                    "export.pdb and OneLibrary differ: {} track(s) only in export.pdb, {} only in OneLibrary, {} changed",
                    drift.only_in_device_library.len(),
                    drift.only_in_onelibrary.len(),
                    drift.changed_tracks.len()
                ),
            )),
        }
    }
    if let Some(large) = &r.large_files {
        if large.is_empty() {
            fine(&mut out, "no files of 4 GB or more");
        } else {
            out.push((Tone::Problem, format!("{} FAT32 can't hold", plural(large.len(), "file"))));
            listed(&mut out, large, |f| format!("{}  {}", f.path, bytes(f.size)));
        }
    }
    if let Some(c) = &r.compatibility {
        let fs = r.file_system.clone().unwrap_or_else(|| "file system".into());
        let tone = match c.verdict {
            Verdict::AllPlayers => Tone::Ok,
            Verdict::NoPlayers => Tone::Problem,
            _ => Tone::Warning,
        };
        out.push((tone, format!("{fs}: {}", c.summary)));
    }
    for i in r.checks.iter().flatten() {
        let tone = match i.level {
            FindingLevel::Info => Tone::Ok,
            FindingLevel::Warning => Tone::Warning,
            FindingLevel::Problem => Tone::Problem,
        };
        out.push((tone, i.message.clone()));
    }
    out.extend(r.warnings.iter().map(|w| (Tone::Warning, w.clone())));
    out
}

/// The lines of a drive's space report: what's used and free, then each kind of file.
pub fn space_lines(r: &SpaceReport) -> Vec<(Tone, String)> {
    let used = r.categories.iter().map(|c| c.bytes).sum::<u64>();
    let mut out = vec![(Tone::Heading, format!("{} used", bytes(used)))];
    if let (Some(free), Some(total)) = (r.free, r.total) {
        out.push((Tone::Text, format!("{} free of {}", bytes(free), bytes(total))));
    }
    out.push((Tone::Text, String::new()));
    for c in &r.categories {
        let pct = if used > 0 { c.bytes as f64 * 100.0 / used as f64 } else { 0.0 };
        let kind = crate::drive_cmd::kind_name(c.kind);
        out.push((Tone::Row, format!("{kind:<44} {:>10}  {pct:>5.1}%  {}", bytes(c.bytes), plural(c.files, "file"))));
    }
    if let Some(f) = &r.forecast {
        let rate = match f.per_month >= 0 {
            true => format!("grows about {} a month", bytes(f.per_month as u64)),
            false => format!("shrinks about {} a month", bytes(f.per_month.unsigned_abs())),
        };
        let full = f.full_around.map(|d| format!("; full around {}", d.format("%B %Y"))).unwrap_or_default();
        out.push((Tone::Text, String::new()));
        out.push((Tone::Text, format!("{} {rate}{full}", f.library)));
    }
    for d in &r.fits {
        out.push(match d.fits {
            true => (Tone::Faint, format!("fits on {} ({})", d.name, bytes(d.capacity))),
            false => (
                Tone::Warning,
                format!("no longer fits on {} ({} usable, {} needed)", d.name, bytes(d.capacity), bytes(d.needed)),
            ),
        });
    }
    out
}

/// The lines of play counts: how many played, the most played, those never played.
pub fn stats_lines(s: &PlayStats) -> Vec<(Tone, String)> {
    let mut out = vec![(
        Tone::Text,
        format!(
            "{}, {} played, {} never played, {}",
            plural(s.tracks, "track"),
            s.played,
            s.never_played.len(),
            plural(s.history_lists, "history list")
        ),
    )];
    if !s.most_played.is_empty() {
        out.push((Tone::Text, String::new()));
        out.push((Tone::Heading, "Most played".into()));
        out.extend(s.most_played.iter().map(|p| (Tone::Row, format!("{:>4}×  {}", p.play_count, p.track.name))));
    }
    if !s.never_played.is_empty() {
        out.push((Tone::Text, String::new()));
        out.push((Tone::Heading, format!("Never played ({})", s.never_played.len())));
        listed(&mut out, &s.never_played, |t| t.name.clone());
    }
    out
}

#[cfg(test)]
mod tests;
