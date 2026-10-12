use locallibrary::check::{CheckItem, DrivePlayers, PlayersFrom, Unloadable};
use locallibrary::model::TrackRef;

use super::super::app::testing::{app_on, app_with, blank, demo_drive, fleet, last_message, library, wait};
use super::*;

fn report(problems: usize, warnings: usize) -> CheckReport {
    CheckReport {
        drives: vec!["DNA".into()],
        items: Vec::new(),
        problems,
        warnings,
        players: Vec::new(),
        unloadable: Vec::new(),
        audio: Vec::new(),
        loudness: Vec::new(),
    }
}

/// The demo drive, open in the library and listed as plugged in; and where it's mounted.
fn demo_app() -> (App, tempfile::TempDir, PathBuf) {
    let (dir, root) = demo_drive();
    let mut app = app_on(&root);
    app.drives = vec![locallibrary::Device::open(&root).unwrap().info()];
    app.drive_state.select(Some(0));
    (app, dir, root)
}

#[test]
fn a_checks_report_says_how_it_went_problems_first() {
    assert_eq!(check_lines(&report(0, 0))[0], (Tone::Ok, "Ready: no problems, no warnings.".into()));
    assert_eq!(check_lines(&report(0, 1))[0], (Tone::Warning, "No problems, 1 warning.".into()));
    assert_eq!(check_lines(&report(2, 1))[0], (Tone::Problem, "2 problems, 1 warning.".into()));

    let mut r = report(1, 1);
    let players = |models: &[&str], from| DrivePlayers {
        drive: "DNA".into(),
        models: models.iter().map(|m| m.to_string()).collect(),
        from,
        library: None,
    };
    r.players = vec![
        players(&["CDJ-3000"], PlayersFrom::Given),
        players(&["XDJ-AZ"], PlayersFrom::Library),
        players(&["CDJ-2000NXS2"], PlayersFrom::Default),
        players(&[], PlayersFrom::None),
    ];
    let item = |level, message: &str| CheckItem { drive: "DNA".into(), check: "x", level, message: message.into() };
    r.items = vec![item(CheckLevel::Ok, "the export reads"), item(CheckLevel::Problem, "2 tracks missing")];
    r.unloadable = vec![Unloadable {
        drive: "DNA".into(),
        player: "CDJ-2000NXS2".into(),
        level: CheckLevel::Problem,
        format: "FLAC".into(),
        reason: locallibrary::equipment::Unplayable::Format,
        sample_rate: None,
        bit_depth: None,
        message: "FLAC".into(),
        tracks: (1..=7).map(|id| TrackRef { id, name: format!("Track {id}"), file_path: String::new() }).collect(),
        transcodable: true,
    }];
    let lines: Vec<String> = check_lines(&r).into_iter().map(|(_, l)| l).collect();
    let at = |l: &str| lines.iter().position(|x| x == l).unwrap_or_else(|| panic!("{l}: {lines:#?}"));
    at("DNA: CDJ-3000");
    at("DNA: XDJ-AZ (remembered for its library)");
    at("DNA: CDJ-2000NXS2 (remembered)");
    at("DNA: no player models (locallibrary players set remembers some)");
    assert!(at("2 tracks missing") < at("the export reads"), "problems first");
    at("FLAC won't load on the CDJ-2000NXS2:");
    at("    Track 5");
    at("    and 2 more");
    assert!(!lines.iter().any(|l| l.contains("Track 6")));
}

/// With no drive plugged in or open, a drive's commands say what they need.
#[test]
fn a_drives_commands_need_a_drive() {
    let (mut app, _dir) = blank(View::Drives);
    app.perform(Act::Check, None);
    assert_eq!(last_message(&app), "no drive: plug one in, or open one in the library");
    app.perform(Act::Pack, None);
    assert_eq!(last_message(&app), "open the drive to pack from in the library first");
    app.perform(Act::ExportList, None);
    assert_eq!(last_message(&app), "open a drive first");
    app.toast = None;
    app.perform(Act::ExportXml, None);
    assert_eq!(last_message(&app), "open a drive first");
}

/// Set prep checks the other drives of the drive's library plugged in with it, in one report.
#[test]
fn set_prep_checks_the_librarys_drives_together() {
    let (mut app, _dir, root) = demo_app();
    let (_other_dir, other) = demo_drive();
    let info = |root: &Path| locallibrary::Device::open(root).unwrap().info();
    app.status = Some(Ok(vec![fleet(info(&root), Some("Main")), fleet(info(&other), Some("Main"))]));
    app.drive_act(Act::SetPrep, root);
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Checking DEMO and 1 more"));
    wait(&mut app, |a| a.job.is_none());
    let Some(Modal::Report(r)) = &app.modal else { panic!("no report: {}", last_message(&app)) };
    assert_eq!(r.title, "Check · DEMO, DEMO");
}

/// Syncing a group needs the store to know the drive's library, and another of its drives.
#[test]
fn syncing_needs_the_groups_drives() {
    let (mut app, dir, root) = demo_app();
    app.drive_act(Act::SyncGroup, root.clone());
    assert!(last_message(&app).starts_with("reading the drives' status"), "{}", last_message(&app));
    let primary = |library| FleetDrive {
        role: Some(locallibrary::fleet::Role::Primary),
        ..fleet(app.drives[0].clone(), library)
    };
    let (unknown, main) = (primary(None), primary(Some("Main")));
    app.status = Some(Ok(vec![unknown]));
    app.drive_act(Act::SyncGroup, root.clone());
    assert_eq!(last_message(&app), "DEMO isn't in the store yet: save a snapshot of it first");
    app.status = Some(Ok(vec![main.clone()]));
    app.drive_act(Act::SyncGroup, root.clone());
    assert_eq!(last_message(&app), "DEMO is the only drive of Main plugged in: plug in the others to sync them");

    // With another of its drives, syncing is planned; here there's no store to plan from.
    let mut backup = main.clone();
    backup.status.drive.root = dir.path().join("BACKUP");
    app.status = Some(Ok(vec![main, backup]));
    app.drive_act(Act::SyncGroup, root);
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Planning"));
    wait(&mut app, |a| a.job.is_none());
    assert!(app.toast.as_ref().is_some_and(|t| t.1 == Level::Error), "{:?}", app.toast);
}

/// Packing asks what goes on (the marked playlists, the crate, or what went on last time),
/// then onto which drive; the crate is saved as a playlist first, named by the DJ.
#[test]
fn packing_asks_what_goes_on_and_where() {
    let (mut app, _dir, root) = demo_app();
    app.list_marks = vec!["Sets/Friday".into()];
    let track = app.loaded.as_ref().unwrap().library.tracks[0].clone();
    let crate_track = super::super::crate_list::CrateTrack { root: root.clone(), drive: "DEMO".into(), track };
    app.crate_list.items.push(crate_track);
    app.pack(None);
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no picker") };
    let labels: Vec<&str> = p.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "the marked playlists: Sets/Friday",
            "the crate's 1 track (saved as a playlist first)",
            "what the drive was packed with last time",
        ]
    );
    app.modal = None;

    app.pack_what(root.clone(), None, PackWhat::Again);
    assert_eq!(last_message(&app), "plug in the drive to pack onto");
    let (_other_dir, other) = demo_drive();
    app.drives.push(locallibrary::Device::open(&other).unwrap().info());
    app.pack_what(root.clone(), None, PackWhat::Again);
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no picker") };
    assert_eq!(p.title, "Pack onto which drive? (what's on it is replaced)");
    assert_eq!(p.items.len(), 1, "not the drive it's packed from");

    // From the drive menu of the drive to pack onto, it isn't asked for.
    app.drive_act(Act::Pack, other.clone());
    let Some(Modal::Pick(_)) = app.modal.take() else { panic!("no picker") };
    app.pack_what(root.clone(), Some(other.clone()), PackWhat::Crate(vec!["/Contents/1.mp3".into()]));
    let Some(Modal::Prompt(p)) = &app.modal else { panic!("no prompt") };
    assert_eq!(p.label, "Save the crate as (then pack it onto DEMO)");
    assert_eq!(p.text, format!("Sets/{}", chrono::Local::now().format("%Y-%m-%d")));
    app.modal = None;

    // What was packed last time, planned in the background: here, nothing ever was.
    app.pack_onto(root, other, PackWhat::Again);
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Planning"));
    wait(&mut app, |a| a.job.is_none());
    assert_eq!((last_message(&app), app.modal.is_some()), ("choose the playlists or folders to pack", false));
}

/// Replaying from the drive menu is planned in the background; with no edits recorded for
/// the drive, there's nothing to change.
#[test]
fn replaying_with_no_edits_recorded_changes_nothing() {
    let (mut app, _dir, root) = demo_app();
    app.drive_act(Act::Replay, root);
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Planning"));
    wait(&mut app, |a| a.job.is_none());
    assert_eq!((last_message(&app), app.modal.is_some()), ("nothing to change", false));
}

/// Bringing a playlist back asks which, then from which snapshot of the drive's library in
/// the store, once there is one.
#[test]
fn bringing_back_a_playlist_asks_which_and_from_when() {
    let (mut app, _dir, root) = demo_app();
    app.restore_playlist(root.clone());
    let Some(Modal::Prompt(p)) = &app.modal else { panic!("no prompt") };
    assert_eq!((p.label.as_str(), p.text.as_str()), ("Bring back the playlist or folder", ""));
    app.modal = None;

    app.pick_snapshot(root.clone(), "Friday".into());
    wait(&mut app, |a| a.job.is_none());
    assert!(last_message(&app).contains("a store: locallibrary pull saves the drive in one"), "{}", last_message(&app));

    let store = Store::open_or_init(&app.store_root).unwrap();
    let pull = PullRequest {
        drive: root.display().to_string(),
        library: Some("Demo".into()),
        message: Some("after Friday's export".into()),
        new_library: true,
        ..Default::default()
    };
    api::pull(&store, &pull, &Ctx::silent()).unwrap();
    // Listed again: pulling gave the drive its identity in the store.
    app.drives = vec![locallibrary::Device::open(&root).unwrap().info()];
    app.pick_snapshot(root, "Friday".into());
    wait(&mut app, |a| a.job.is_none());
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no picker: {}", last_message(&app)) };
    assert_eq!(p.title, "Bring back Friday from Demo");
    let labels: Vec<&str> = p.items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.len() == 2 && labels[1].ends_with("after Friday's export"), "{labels:?}");
}

/// Exporting asks where, suggesting a file named for what's exported; a name that's blank
/// says what's wanted.
#[test]
fn exporting_asks_where() {
    let (mut app, _dir, root) = demo_app();
    let asked = |app: &mut App| match app.modal.take() {
        Some(Modal::Prompt(p)) => (p.label, p.text),
        _ => panic!("no prompt: {}", last_message(app)),
    };
    app.export_xml();
    assert_eq!(asked(&mut app), ("rekordbox XML of the drive to".into(), "~/Music/DEMO.xml".into()));
    app.list_marks = vec!["A".into(), "B".into()];
    app.export_xml();
    assert_eq!(asked(&mut app).0, "rekordbox XML of the 2 marked playlists to");

    app.write_export(root, "  ", Export::List(ExportOf::Everything));
    assert_eq!(last_message(&app), "say where: a file name");
}

/// What's exported, brought back or packed is the list shown: a playlist, a session; a
/// category's group isn't a list to export.
#[test]
fn the_list_shown_is_the_one_acted_on() {
    let (mut app, _dir) = app_with(library());
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    let shown = |app: &mut App, node: NodeId| {
        let l = app.loaded.as_mut().unwrap();
        // Showing the list reads its tracks (and a category's groups).
        l.node = node;
        l.base();
        app.export_list();
        match app.modal.take() {
            Some(Modal::Prompt(p)) => p.text,
            _ => last_message(app).to_string(),
        }
    };
    assert_eq!(shown(&mut app, NodeId::Playlist(2)), "~/Music/Sets - Friday.m3u8");
    assert_eq!(shown(&mut app, NodeId::Group(Category::History, "9".into())), "~/Music/HISTORY 001.m3u8");
    assert_eq!(
        shown(&mut app, NodeId::Group(Category::Genre, "house".into())),
        "export writes a playlist, a set, a session or all tracks: open one first"
    );

    app.loaded.as_mut().unwrap().node = NodeId::Playlist(2);
    app.restore_playlist(root.clone());
    let Some(Modal::Prompt(p)) = app.modal.take() else { panic!("no prompt") };
    assert_eq!(p.text, "Sets/Friday", "the playlist shown, to begin with");
    app.pack(None);
    let Some(Modal::Pick(p)) = app.modal.take() else { panic!("no picker") };
    assert_eq!(p.items[0].label, "the playlist Sets/Friday");
}

/// A check's report of any drives, items and tracks the players won't load reads: the
/// verdict first, as the counts say; then each drive under its name, in order, with its
/// items problems first and each group of tracks it won't load (five named, the rest
/// counted).
#[hegel::test(test_cases = 200)]
fn a_checks_report_lists_each_drive_problems_first(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let drives: Vec<String> = ["DNA", "SD", "USB"][..tc.draw(gs::integers::<usize>().min_value(1).max_value(3))]
        .iter()
        .map(|d| d.to_string())
        .collect();
    let pick = |tc: &hegel::TestCase| drives[tc.draw(gs::integers::<usize>().max_value(drives.len() - 1))].clone();
    let levels = [CheckLevel::Ok, CheckLevel::Warning, CheckLevel::Problem];
    let mut r = report(tc.draw(gs::integers::<usize>().max_value(3)), tc.draw(gs::integers::<usize>().max_value(3)));
    r.drives = drives.clone();
    r.items = (0..tc.draw(gs::integers::<usize>().max_value(6)))
        .map(|i| CheckItem {
            drive: pick(&tc),
            check: "x",
            level: levels[tc.draw(gs::integers::<usize>().max_value(2))],
            message: format!("item {i}"),
        })
        .collect();
    r.unloadable = (0..tc.draw(gs::integers::<usize>().max_value(2)))
        .map(|i| Unloadable {
            drive: pick(&tc),
            player: "CDJ-900".into(),
            level: CheckLevel::Problem,
            format: "FLAC".into(),
            reason: locallibrary::equipment::Unplayable::Format,
            sample_rate: None,
            bit_depth: None,
            message: format!("group {i}"),
            tracks: (1..=tc.draw(gs::integers::<u32>().max_value(8)))
                .map(|id| TrackRef { id, name: format!("Track {i}.{id}"), file_path: String::new() })
                .collect(),
            transcodable: false,
        })
        .collect();
    let lines = check_lines(&r);
    let tone = match (r.problems, r.warnings) {
        (0, 0) => Tone::Ok,
        (0, _) => Tone::Warning,
        _ => Tone::Problem,
    };
    assert_eq!(lines[0].0, tone);
    let headings: Vec<&String> = lines.iter().filter(|(t, _)| *t == Tone::Heading).map(|(_, l)| l).collect();
    assert_eq!(headings, drives.iter().collect::<Vec<_>>());
    let rank = |t: &Tone| match t {
        Tone::Problem => 0,
        Tone::Warning => 1,
        _ => 2,
    };
    for d in &drives {
        let start = lines.iter().position(|(t, l)| *t == Tone::Heading && l == d).unwrap() + 1;
        let end = lines[start..].iter().position(|(t, _)| *t == Tone::Heading).map_or(lines.len(), |e| start + e);
        let section = &lines[start..end];
        let items: Vec<&(Tone, String)> = section.iter().filter(|(_, l)| l.starts_with("item ")).collect();
        assert_eq!(items.len(), r.items.iter().filter(|i| &i.drive == d).count(), "{d}'s items");
        assert!(items.windows(2).all(|w| rank(&w[0].0) <= rank(&w[1].0)), "{d}: {items:?}");
        for u in r.unloadable.iter().filter(|u| &u.drive == d) {
            let named =
                section.iter().filter(|(_, l)| l.starts_with(&format!("    Track {}", &u.message[6..]))).count();
            assert_eq!(named, u.tracks.len().min(5), "{d}: {}", u.message);
            let more = section.iter().any(|(_, l)| *l == format!("    and {} more", u.tracks.len().saturating_sub(5)));
            assert_eq!(more, u.tracks.len() > 5);
        }
    }
}
