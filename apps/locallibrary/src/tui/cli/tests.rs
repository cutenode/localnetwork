use clap::CommandFactory;

use super::super::app::testing::{app_on, blank, demo_drive, job, last_message, press, wait};
use super::super::app::{App, View};
use super::super::keymap::Act;
use super::super::palette::{Picker, commands};
use super::*;

/// The top-level command an action of the palette is, if it's one.
fn stands_for(act: Act) -> Option<&'static str> {
    Some(match act {
        Act::Pull => "pull",
        Act::Check | Act::SetPrep => "check",
        Act::SyncGroup => "sync",
        Act::Pack => "pack",
        // `drive restore --playlist`.
        Act::RestorePlaylist => "drive",
        Act::Replay => "edits",
        Act::Undo => "undo",
        Act::Eject => "eject",
        Act::SmartList | Act::SmartApply | Act::SmartNew => "smart",
        Act::ExportList => "export",
        Act::Snapshots => "library",
        Act::SignIn => "account",
        Act::Tag => "tags",
        Act::Search => "tracks",
        Act::ShowView(View::Drives) => "status",
        Act::ShowView(View::Mixes) => "mix",
        Act::ShowView(View::Profile) => "profile",
        Act::Run(Cli::Doctor | Cli::BySound) => "doctor",
        Act::Run(Cli::Dedupe | Cli::Relocate | Cli::Upgrade) => "track",
        Act::Run(Cli::Refresh) => "refresh",
        Act::Run(Cli::Space) => "drive",
        Act::Run(Cli::PlayStats) => "plays",
        Act::Copy(command) => command.split(' ').next().unwrap_or_default(),
        _ => return None,
    })
}

/// Commands the palette has when a plugin offers them: its analyzers, importers and settings.
const BY_PLUGINS: &[(&str, &str)] = &[
    ("analyze", "each plugin's analyzers are in the palette: analyze the tracks: …"),
    ("import", "each plugin's importers are in the palette: import …"),
    ("plugin", "each plugin's settings and access are in the palette"),
];

/// Commands left out of the palette on purpose, and why.
const LEFT_OUT: &[(&str, &str)] = &[
    ("playlist", "the actions menu (e) makes, renames and deletes playlists; LOC-167 for the rest"),
    ("prep", "each of its steps is in the palette: save a snapshot, sync, check, eject"),
    ("setup", "the first run: the player's next steps do it"),
    ("tui", "it's the player"),
    ("watch", "runs until it's stopped, in a terminal or as a service"),
    ("completions", "for the shell"),
    ("analysis", "measures the analysis against datasets, for working on locallibrary"),
    ("sets", "LOC-167: screens of their own in the player"),
    ("diff", "LOC-167: screens of their own in the player"),
    ("cues", "LOC-167: screens of their own in the player"),
    ("art", "LOC-167: screens of their own in the player"),
    ("settings", "LOC-167: screens of their own in the player"),
    ("merge", "LOC-167: screens of their own in the player"),
    ("convert", "LOC-167: screens of their own in the player"),
    ("collection", "LOC-167: screens of their own in the player"),
    ("players", "LOC-167: screens of their own in the player"),
    ("xml", "LOC-167: screens of their own in the player"),
];

/// Every top-level command the command line lists (its hidden ones are old spellings of others)
/// is in the palette in some view, offered by plugins, or left out on purpose with a reason;
/// and nothing's left out that's there or that isn't a command.
#[test]
fn every_command_is_in_the_palette_or_left_out() {
    let cli = crate::Cli::command();
    let listed: Vec<String> =
        cli.get_subcommands().filter(|c| !c.is_hide_set()).map(|c| c.get_name().to_string()).collect();
    assert!(listed.len() > 30, "{listed:?}");
    let in_palette: Vec<&str> =
        View::ALL.iter().flat_map(|v| commands(*v)).filter_map(|(act, _)| stands_for(act)).collect();
    let set_aside = |name: &str| LEFT_OUT.iter().chain(BY_PLUGINS).any(|(n, _)| *n == name);
    for name in &listed {
        assert!(
            in_palette.contains(&name.as_str()) || set_aside(name),
            "`locallibrary {name}` has no entry in the palette: give it one, or say in LEFT_OUT why not"
        );
    }
    for (name, why) in LEFT_OUT.iter().chain(BY_PLUGINS) {
        assert!(listed.contains(&name.to_string()), "{name} isn't a command any more");
        assert!(!in_palette.contains(name), "{name} is in the palette after all");
        assert!(!why.is_empty());
    }
}

/// The palette opened with `typed` in it.
fn palette_for(app: &mut App, typed: &str) -> Vec<(String, String)> {
    app.open_palette_with(typed);
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no palette") };
    let p: &Picker = p;
    p.shown.iter().map(|&i| (p.items[i].label.clone(), p.items[i].detail.clone())).collect()
}

/// What someone looking for a job would type finds it, among the first few.
#[test]
fn search_words_find_the_command_line_jobs() {
    let (mut app, _dir) = blank(View::Library);
    for (typed, label) in [
        ("duplicates", "health report: duplicates, missing and orphaned files"),
        ("duplicates", "replace duplicates with the copy to keep"),
        ("MP3 to FLAC", "upgrade tracks to lossless files…"),
        ("lossless", "upgrade tracks to lossless files…"),
        ("missing files", "find the tracks' missing files…"),
        ("most played", "play counts: most and never played"),
        ("free space", "what's using the drive's space"),
        ("format", "format a drive (in a terminal)"),
        ("track dedupe", "replace duplicates with the copy to keep"),
    ] {
        let shown = palette_for(&mut app, typed);
        let at = shown.iter().position(|(l, _)| l == label);
        assert!(at.is_some_and(|i| i < 5), "{typed:?}: {label:?} at {at:?} in {shown:?}");
    }
}

/// A command to copy has the drive filled in (quoted for the shell), and runs nothing; with no
/// drive it says where the drive goes.
#[test]
fn a_command_to_copy_has_the_drive_in_it() {
    let (_dir, root) = demo_drive();
    let mut app = app_on(&root);
    palette_for(&mut app, "format");
    press(&mut app, "\n");
    assert!(app.job.is_none(), "it runs nothing");
    let Some(Modal::Report(r)) = &app.modal else { panic!("no command shown: {}", last_message(&app)) };
    let line = format!("locallibrary drive format {}", quoted(&root.display().to_string()));
    let line = match app.store_root == Store::default_root() {
        true => line,
        false => line.replacen(
            "locallibrary",
            &format!("locallibrary --store {}", quoted(&app.store_root.display().to_string())),
            1,
        ),
    };
    assert!(r.lines.iter().any(|(_, l)| *l == line), "{line:?} in {:?}", r.lines);
    assert_eq!(r.title, "In a terminal · drive format");

    let store = Store::default_root();
    assert_eq!(command_line("drive fsck DRIVE", None, &store), "locallibrary drive fsck <drive>");
    assert_eq!(
        command_line("drive fsck DRIVE", Some(Path::new("/Volumes/DJ USB")), Path::new("/tmp/s")),
        "locallibrary --store /tmp/s drive fsck '/Volumes/DJ USB'"
    );
    assert_eq!(quoted("it's"), r"'it'\''s'");
}

/// The jobs need a drive, and wait for one under way, as the drive's own do.
#[test]
fn the_jobs_need_a_drive_that_isnt_busy() {
    let (app, _dir) = blank(View::Library);
    assert_eq!(app.unavailable(Act::Run(Cli::Doctor)).as_deref(), Some("open a drive first"));
    assert_eq!(app.unavailable(Act::Copy("drive format DRIVE")), None, "a command to copy needs nothing");
    let (_d, root) = demo_drive();
    let mut app = app_on(&root);
    assert_eq!(app.unavailable(Act::Run(Cli::Dedupe)), None);
    app.job = Some(job(true, false));
    assert!(app.unavailable(Act::Run(Cli::Dedupe)).is_some_and(|w| w.starts_with("wait for")));
    app.perform(Act::Run(Cli::Dedupe), None);
    assert!(last_message(&app).starts_with("wait for"), "{}", last_message(&app));
}

/// The reports show in a panel; the changes are planned first (on the demo drive, with nothing to
/// change, they say so); relocating and upgrading ask where to look, and upgrading needs a folder.
#[test]
fn the_jobs_run_on_the_drive() {
    let (_dir, root) = demo_drive();
    let mut app = app_on(&root);
    for (job, title) in [(Cli::Doctor, "Doctor · "), (Cli::Space, "Space · "), (Cli::PlayStats, "Plays · ")] {
        app.perform(Act::Run(job), None);
        assert!(app.job.is_some(), "{job:?} runs as a job");
        wait(&mut app, |a| a.job.is_none());
        let Some(Modal::Report(r)) = &app.modal else { panic!("{job:?}: no report: {}", last_message(&app)) };
        assert!(r.title.starts_with(title), "{}", r.title);
        assert!(!r.lines.is_empty());
        press(&mut app, "x");
    }
    app.perform(Act::Run(Cli::Dedupe), None);
    wait(&mut app, |a| a.job.is_none());
    let planned = matches!(app.modal, Some(Modal::Confirm(_))) || last_message(&app).starts_with("nothing to change");
    assert!(planned, "{}", last_message(&app));
    app.modal = None;

    app.perform(Act::Run(Cli::Upgrade), None);
    assert!(matches!(&app.modal, Some(Modal::Prompt(p)) if matches!(p.purpose, PromptFor::LookIn { upgrade: true })));
    press(&mut app, "\n");
    assert!(matches!(app.modal, Some(Modal::Prompt(_))), "upgrading needs a folder");
    assert!(last_message(&app).starts_with("name the folder"), "{}", last_message(&app));
    app.modal = None;

    app.perform(Act::Run(Cli::Relocate), None);
    press(&mut app, "/no/such/folder\n");
    assert!(last_message(&app).ends_with("isn't a folder"), "{}", last_message(&app));
    let Some(Modal::Prompt(p)) = &mut app.modal else { panic!("the prompt stays open") };
    p.text.clear();
    press(&mut app, "\n");
    assert!(app.job.is_some(), "the drive alone: planned");
    wait(&mut app, |a| a.job.is_none());
}

/// The lines of a report say what's fine as well as what isn't.
#[test]
fn a_doctors_report_says_what_it_found() {
    let (_dir, root) = demo_drive();
    let ctx = locallibrary::Ctx::silent();
    let req = api::DoctorRequest { drive: root.display().to_string(), options: DoctorOptions::default() };
    let report = api::doctor(&req, &ctx).unwrap();
    let lines = doctor_lines(&report, false);
    assert!(matches!(lines[0].0, Tone::Ok | Tone::Problem));
    assert!(lines.iter().any(|(_, l)| l.ends_with("tracks") || l.ends_with("track")), "{lines:?}");
}

/// Refreshing after an export shows its steps first, or says why it can't.
#[test]
fn refreshing_is_planned_first() {
    let (_dir, root) = demo_drive();
    let mut app = app_on(&root);
    app.perform(Act::Run(Cli::Refresh), None);
    assert!(app.job.as_ref().is_some_and(|j| j.label == "Planning"));
    wait(&mut app, |a| a.job.is_none());
    match &app.modal {
        Some(Modal::Confirm(c)) => assert_eq!(c.plan.operation(), "refresh"),
        _ => assert!(!last_message(&app).is_empty(), "nothing said"),
    }
}

/// The help lists keys, saying `:` searches everything; a search there that no key matches goes
/// on in the palette, which finds the command.
#[test]
fn the_help_hands_a_search_on_to_the_palette() {
    let (mut app, _dir) = blank(View::Library);
    press(&mut app, "?");
    let s = super::super::ui::screen(&mut app, 100, 30);
    assert!(s.contains(": searches everything"), "{s}");
    press(&mut app, "duplicates");
    let s = super::super::ui::screen(&mut app, 100, 30);
    assert!(s.contains("enter searches the commands (:)"), "{s}");
    press(&mut app, "\n");
    assert!(!app.help);
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no palette") };
    assert_eq!(p.query, "duplicates");
    let first = p.shown.first().map(|&i| p.items[i].label.as_str());
    assert!(first.is_some_and(|l| l.contains("duplicates")), "{first:?}");
}

/// A plugin's operation is found by its command line's name too: `clean` is the demos plugin's.
#[test]
fn clean_finds_the_demos_plugins_operation() {
    let (_dir, root) = demo_drive();
    let mut app = app_on(&root);
    let shown = palette_for(&mut app, "clean");
    let first = shown.first().map(|(l, _)| l.as_str());
    assert_eq!(first, Some("remove rekordbox's demo tracks and their files"), "{shown:?}");
}
