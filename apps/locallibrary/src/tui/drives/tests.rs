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

/// What the Drives view's keys act on is what's selected: a drive's actions that drive (set prep
/// with its library's other drives plugged in; syncing once the status says its library and
/// another of its drives is here), a library's its clones with an export, and the row of the
/// drives of no library asks for one of them; enter on a library shows it, on a drive with an
/// export reads it, on one without says so.
#[hegel::test(test_cases = 200)]
fn the_drives_views_actions_act_on_what_is_selected(tc: hegel::TestCase) {
    use hegel::generators::{self as gs, Generator};
    let (main, backup) = (Uuid::from_u128(1), Uuid::from_u128(2));
    let (mut app, dir) = blank(View::Drives);
    // (The jobs planned are dropped: nothing waits for them.)
    // Two of Main's drives with exports, and others as they come.
    let n = 2 + tc.draw(gs::integers::<usize>().max_value(2));
    app.drives = (0..n)
        .map(|i| {
            let root = dir.path().join(format!("D{i}"));
            std::fs::create_dir_all(&root).unwrap();
            DeviceInfo {
                name: Some(format!("D{i}")),
                has_export: i < 2 || tc.draw(gs::booleans()),
                library_id: if i < 2 {
                    Some(main)
                } else {
                    tc.draw(gs::sampled_from(&[Some(main), Some(backup), None][..]).print_as_debug())
                },
                ..locallibrary::Device::open(&root).unwrap().info()
            }
        })
        .collect();
    app.library_names = [(main, "Main".to_string()), (backup, "Backup".to_string())].into_iter().collect();
    let name_of = |id: Option<Uuid>| match id {
        Some(id) if id == main => Some("Main"),
        Some(_) => Some("Backup"),
        None => None,
    };
    let known = tc.draw(gs::booleans());
    if known {
        app.status = Some(Ok(app.drives.iter().map(|d| fleet(d.clone(), name_of(d.library_id))).collect()));
    }
    // Every row, every action.
    for row in app.drive_rows() {
        for act in
            [Act::Check, Act::SetPrep, Act::SyncGroup, Act::Pack, Act::Undo, Act::Replay, Act::Eject, Act::Activate]
        {
            (app.job, app.loading, app.view, app.toast, app.modal) = (None, None, View::Drives, None, None);
            acts_on_what_is_selected(&mut app, row, act, known, (main, n));
        }
    }
    // With nothing selected, enter does nothing.
    (app.toast, app.loading, app.drive_header, app.view) = (None, None, None, View::Drives);
    app.drive_state.select(None);
    app.on_key(ratatui::crossterm::event::KeyEvent::from(ratatui::crossterm::event::KeyCode::Enter));
    assert_eq!((app.view, app.loading.as_deref(), last_message(&app)), (View::Drives, None, ""));
}

/// [`the_drives_views_actions_act_on_what_is_selected`] for one row and action (`Act::Activate`
/// is enter).
fn acts_on_what_is_selected(app: &mut App, row: DriveRow, act: Act, known: bool, (main, n): (Uuid, usize)) {
    let name_of = |id: Option<Uuid>| match id {
        Some(id) if id == main => Some("Main"),
        Some(_) => Some("Backup"),
        None => None,
    };
    app.select_drive_row(row);
    let enter = act == Act::Activate;
    if enter {
        app.on_key(ratatui::crossterm::event::KeyEvent::from(ratatui::crossterm::event::KeyCode::Enter));
    } else {
        app.drive_key(act);
    }
    let job = app.job.as_ref().map(|j| j.label.clone());
    let said = last_message(app).to_string();
    let label = |i: usize| format!("D{i}");
    let checking = |drives: &[usize]| match drives {
        [one] => format!("Checking {}", label(*one)),
        [first, rest @ ..] => format!("Checking {} and {} more", label(*first), rest.len()),
        [] => unreachable!(),
    };
    let of = |lib: Option<Uuid>| (0..n).filter(|&i| app.drives[i].library_id == lib).collect::<Vec<_>>();
    match row {
        DriveRow::Library(id) => {
            let name = name_of(Some(id)).unwrap();
            let clones: Vec<usize> = of(Some(id)).into_iter().filter(|&i| app.drives[i].has_export).collect();
            if enter {
                if clones.is_empty() {
                    assert_eq!(said, format!("none of {name}'s drives has a rekordbox export"));
                } else {
                    assert_eq!((app.loading.clone(), app.view), (Some(format!("Reading {name}…")), View::Library));
                }
                return;
            }
            match act {
                _ if clones.is_empty() => assert_eq!(said, format!("none of {name}'s drives is plugged in any more")),
                Act::SetPrep => assert_eq!(job, Some(checking(&clones))),
                Act::SyncGroup | Act::Eject => assert!(job.is_some() || !said.is_empty(), "{act:?}: nothing said"),
                _ => assert_eq!(said, format!("that's for one drive: select one of {name}'s")),
            }
        }
        DriveRow::Unlinked if enter => assert_eq!(app.view, View::Drives),
        DriveRow::Unlinked => assert_eq!(said, "select one of these drives"),
        DriveRow::Drive(i) => {
            let d = &app.drives[i];
            if enter {
                if d.has_export {
                    assert_eq!((app.loading.clone(), app.view), (Some(format!("Reading D{i}…")), View::Library));
                } else {
                    assert_eq!(said, "no rekordbox export on this drive");
                }
                return;
            }
            let lib = d.library_id;
            let group: Vec<usize> =
                std::iter::once(i).chain(of(lib).into_iter().filter(|&j| j != i && known && lib.is_some())).collect();
            match act {
                Act::Check => assert_eq!(job, Some(checking(&[i]))),
                Act::SetPrep => assert_eq!(job, Some(checking(&group))),
                Act::SyncGroup if !known => assert!(said.starts_with("reading the drives' status"), "{said}"),
                Act::SyncGroup if lib.is_none() => {
                    assert_eq!(said, format!("D{i} isn't in the store yet: save a snapshot of it first"))
                }
                Act::SyncGroup if group.len() < 2 => assert_eq!(
                    said,
                    format!(
                        "D{i} is the only drive of {} plugged in: plug in the others to sync them",
                        name_of(lib).unwrap()
                    )
                ),
                Act::Pack => assert_eq!(said, "open the drive to pack from in the library first"),
                _ => assert_eq!(job.as_deref(), Some("Planning"), "{act:?}"),
            }
        }
    }
}

/// Exporting the list shown (every track, a playlist, a session, or a set: its tracks on the
/// drive) asks where, starting from a file named for it in ~/Music; no name, or a place on the
/// drive itself, is refused saying why; a file that's there is replaced only once that's agreed
/// to; and what's written holds the tracks the list shows. A list of lists (Browse) isn't
/// exported.
#[hegel::test(test_cases = 20, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_export_writes_the_list_shown_where_asked(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use locallibrary::sets::{EntryOrigin, SetEntry};

    use super::super::app::Msg;
    use super::super::app::testing::set;

    let (dir, root) = demo_drive();
    let mut app = app_on(&root);
    let lib = app.loaded.as_ref().unwrap().library.clone();
    let generation = app.loaded.as_ref().unwrap().generation;
    let entries: Vec<SetEntry> = (0..tc.draw(gs::integers::<usize>().max_value(3)))
        .map(|_| match tc.draw(gs::integers::<usize>().max_value(lib.tracks.len())) {
            i if i < lib.tracks.len() => SetEntry::from_track(&lib.tracks[i], EntryOrigin::Session, None),
            _ => SetEntry { file: Some("/Contents/elsewhere.mp3".into()), ..SetEntry::added("Elsewhere") },
        })
        .collect();
    app.handle_msg(Msg::Sets(generation, Ok(vec![set("set1", "2026-10-03", Some("Friday"), None, entries)])));
    let playlist = lib.playlists.iter().find(|p| !p.is_folder).map(|p| NodeId::Playlist(p.id));
    let history = lib.history.iter().find(|h| !h.is_folder).map(|h| NodeId::Group(Category::History, h.id.to_string()));
    let nodes: Vec<NodeId> = [
        Some(NodeId::All),
        playlist,
        history,
        Some(NodeId::Group(Category::Sets, "set1".into())),
        Some(NodeId::Browse),
    ]
    .into_iter()
    .flatten()
    .collect();
    // Each list in turn (drawn, one would seldom be the playlist).
    for (k, node) in nodes.into_iter().enumerate() {
        app.modal = None;
        exports_the_list_shown(&tc, &mut app, (dir.path(), &root), &lib, node, k);
    }

    // Any list's file is named for it: a playlist in a folder by its path, a session by its name.
    let (mut app, _store) = app_with(library());
    let lib = app.loaded.as_ref().unwrap().library.clone();
    for p in lib.playlists.iter().filter(|p| !p.is_folder) {
        let path = lib.playlist_path(p.id);
        let l = app.loaded.as_mut().unwrap();
        l.node = NodeId::Playlist(p.id);
        app.perform(Act::ExportList, None);
        let Some(Modal::Prompt(prompt)) = app.modal.take() else { panic!("{path} not exported") };
        assert_eq!(prompt.text, format!("~/Music/{}.m3u8", path.replace('/', " - ")));
    }
    for h in lib.history.iter().filter(|h| !h.is_folder) {
        app.loaded.as_mut().unwrap().node = NodeId::Group(Category::History, h.id.to_string());
        // (Shown, its group is read: that's where its name is.)
        app.refresh_visible();
        app.perform(Act::ExportList, None);
        let Some(Modal::Prompt(prompt)) = app.modal.take() else { panic!("{} not exported", h.name) };
        assert_eq!(prompt.text, format!("~/Music/{}.m3u8", h.name.replace('/', " - ")));
    }
}

/// [`an_export_writes_the_list_shown_where_asked`] for one list (the `k`th).
fn exports_the_list_shown(
    tc: &hegel::TestCase,
    app: &mut App,
    (dir, root): (&Path, &Path),
    lib: &locallibrary::model::Library,
    node: NodeId,
    k: usize,
) {
    use super::super::app::testing::press;
    use hegel::generators as gs;
    let l = app.loaded.as_mut().unwrap();
    l.reveal(&node);
    l.node = node.clone();
    app.refresh_visible();
    let shown: Vec<String> = app.visible.iter().map(|&i| lib.tracks[i].title.clone()).collect();
    app.perform(Act::ExportList, None);
    let Some(Modal::Prompt(p)) = &mut app.modal else {
        assert_eq!(node, NodeId::Browse, "{node:?} not exported: {}", last_message(app));
        assert!(last_message(app).starts_with("export writes a playlist, a set, a session or all tracks"));
        return;
    };
    assert!(p.text.starts_with("~/Music/") && p.text.ends_with(".m3u8"), "{}", p.text);
    let ext = tc.draw(gs::sampled_from(&["m3u8", "csv", "txt"][..]));
    let out = dir.join("out").join(format!("list{k}.{ext}"));
    let there = tc.draw(gs::booleans());
    if there {
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(&out, "mine").unwrap();
    }
    p.text = match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => " ".into(),
        1 => root.join("list.m3u8").display().to_string(),
        _ => out.display().to_string(),
    };
    let typed = p.text.clone();
    press(app, "\n");
    if typed.trim().is_empty() {
        return assert_eq!(last_message(app), "say where: a file name");
    }
    if typed.starts_with(&root.display().to_string()) {
        assert_eq!(last_message(app), "not onto the drive: somewhere on this computer");
        return assert!(!root.join("list.m3u8").exists());
    }
    if there {
        assert!(matches!(&app.modal, Some(Modal::Ask(q, _)) if q.ends_with("is there already. Replace it?")));
        if tc.draw(gs::booleans()) {
            press(app, "n");
            return assert_eq!(std::fs::read_to_string(&out).unwrap(), "mine", "kept");
        }
        press(app, "y");
    }
    wait(app, |a| a.job.is_none());
    let n = shown.len();
    assert_eq!(last_message(app), format!("wrote {}: {n} track{}", out.display(), if n == 1 { "" } else { "s" }));
    let written = std::fs::read_to_string(&out).unwrap();
    for title in &shown {
        assert!(written.contains(title.as_str()), "{title} isn't in {written}");
    }
}

/// Packing offers what there is to pack (the marked playlists, else the playlist shown or picked
/// in the tree; the crate's tracks from this drive; what went on last time), then asks onto which
/// drive (not this one, and only when one's plugged in) unless it was picked; the crate is saved
/// as a playlist first, named. A drive's other commands need a drive, a playlist to bring back
/// is guessed from the one shown, rekordbox XML of the marked playlists says how many, and a
/// session is exported by its name.
#[hegel::test(test_cases = 40, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn packing_and_the_drives_commands_offer_what_there_is(tc: hegel::TestCase) {
    use super::super::app::testing::{key, press};
    use hegel::generators as gs;
    let (mut app, _dir, root) = demo_app();
    let lib = app.loaded.as_ref().unwrap().library.clone();
    let marks: Vec<String> = lib
        .playlists
        .iter()
        .filter(|p| !p.is_folder)
        .map(|p| lib.playlist_path(p.id))
        .filter(|_| tc.draw(gs::booleans()))
        .collect();
    app.list_marks = marks.clone();
    let shown = lib.playlists.iter().find(|p| !p.is_folder).map(|p| p.id).filter(|_| tc.draw(gs::booleans()));
    let l = app.loaded.as_mut().unwrap();
    if let Some(id) = shown {
        l.reveal(&NodeId::Playlist(id));
    }
    l.node = shown.map_or(NodeId::All, NodeId::Playlist);
    let in_tree = tc.draw(gs::booleans());
    if in_tree {
        app.focus = Focus::Playlists;
        let row = shown.and_then(|id| app.loaded.as_ref().unwrap().tree_position(&NodeId::Playlist(id))).unwrap_or(0);
        app.playlist_state.select(Some(row));
    }
    let crated = tc.draw(gs::integers::<usize>().max_value(2));
    for t in lib.tracks.iter().take(crated) {
        app.crate_list.add(&root, "DEMO", [t.clone()]);
    }
    app.pack(None);
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no picker: {}", last_message(&app)) };
    let labels: Vec<String> = p.items.iter().map(|i| i.label.clone()).collect();
    let mut want = Vec::new();
    if !marks.is_empty() {
        want.push(format!("the marked playlists: {}", marks.join(", ")));
    } else if let Some(id) = shown {
        want.push(format!("the playlist {}", lib.playlist_path(id)));
    }
    let n = crated.min(lib.tracks.len());
    if n > 0 {
        want.push(format!("the crate's {n} track{} (saved as a playlist first)", if n == 1 { "" } else { "s" }));
    }
    want.push("what the drive was packed with last time".into());
    assert_eq!(labels, want);
    // Picked: onto which drive, when there's another (there's none plugged in here).
    let i = tc.draw(gs::integers::<usize>().max_value(labels.len() - 1));
    let Some(Modal::Pick(p)) = &mut app.modal else { unreachable!() };
    p.state.select(Some(i));
    key(&mut app, ratatui::crossterm::event::KeyCode::Enter);
    assert_eq!(last_message(&app), "plug in the drive to pack onto");
    // Onto a drive picked: the crate saved first, named; the rest planned.
    let other = root.with_file_name("OTHER");
    if labels[i].starts_with("the crate's") {
        app.pack_onto(root.clone(), other.clone(), PackWhat::Crate(vec!["/Contents/1.mp3".into()]));
        let Some(Modal::Prompt(p)) = &app.modal else { panic!("no prompt") };
        assert!(p.label.starts_with("Save the crate as (then pack it onto "), "{}", p.label);
    } else {
        app.modal = None;
        app.pack_what(root.clone(), Some(other), PackWhat::Again);
        assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Planning"));
        wait(&mut app, |a| a.job.is_none());
    }
    app.modal = None;

    // rekordbox XML of the marked playlists (those drawn, then on a drive with more, any of
    // those), and a playlist to bring back guessed from the tree.
    let xml_of = |app: &mut App, marks: &[String]| {
        app.list_marks = marks.to_vec();
        app.perform(Act::ExportXml, None);
        let Some(Modal::Prompt(p)) = app.modal.take() else { panic!("no prompt") };
        let n = marks.len();
        let want = if n == 0 {
            "rekordbox XML of the drive to".to_string()
        } else {
            format!("rekordbox XML of the {n} marked playlist{} to", if n == 1 { "" } else { "s" })
        };
        assert_eq!(p.label, want);
    };
    xml_of(&mut app, &marks);
    let (mut more, _more_dir) = app_with(library());
    let lists = more.loaded.as_ref().unwrap().library.clone();
    let lists: Vec<String> = lists.playlists.iter().map(|p| lists.playlist_path(p.id)).collect();
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(lists.len()));
    xml_of(&mut more, &lists[..n]);
    app.restore_playlist(root.clone());
    let Some(Modal::Prompt(p)) = &app.modal else { panic!("no prompt") };
    let guess = match (in_tree, shown) {
        (_, Some(id)) => lib.playlist_path(id),
        _ => String::new(),
    };
    if !in_tree || shown.is_some() {
        assert_eq!(p.text, guess);
    }
    app.modal = None;

    // A session exported by its name.
    if let Some(h) = lib.history.iter().find(|h| !h.is_folder) {
        let l = app.loaded.as_mut().unwrap();
        l.node = NodeId::Group(Category::History, h.id.to_string());
        app.perform(Act::ExportList, None);
        let Some(Modal::Prompt(p)) = &app.modal else { panic!("no prompt") };
        assert!(p.text.ends_with(&format!("/{}.m3u8", h.name.replace('/', " - "))), "{}", p.text);
        app.modal = None;
    }

    // With no drive: each says one's needed.
    let (mut none, _store) = blank(View::Library);
    for (act, says) in [
        (Act::Check, "no drive: plug one in, or open one in the library"),
        (Act::ExportList, "open a drive first"),
        (Act::ExportXml, "open a drive first"),
    ] {
        none.toast = None;
        none.perform(act, None);
        assert_eq!(last_message(&none), says);
    }
    none.view = View::Snapshots;
    none.history = Some(Ok(Vec::new()));
    press(&mut none, "\n");
    assert_eq!(last_message(&none), "open the drive to bring a playlist back to in the library first");
}

/// Each drive checked says which player models it was checked for and where they came from: as
/// given says nothing more, and remembered for its library, remembered, or none remembered each
/// say which. Checked for real, the demo drive's report is titled for it and starts by saying how
/// it went.
#[hegel::test(test_cases = 30)]
fn a_check_says_which_players_it_was_for(tc: hegel::TestCase) {
    use hegel::generators::{self as gs, Generator};
    const FROM: [PlayersFrom; 4] = [PlayersFrom::Given, PlayersFrom::Library, PlayersFrom::Default, PlayersFrom::None];
    let mut r = report(0, 0);
    // Every source once, then as drawn.
    r.players = (0..4 + tc.draw(gs::integers::<usize>().max_value(3)))
        .map(|i| DrivePlayers {
            drive: format!("D{i}"),
            models: ["CDJ-3000", "XDJ-AZ"][..tc.draw(gs::integers::<usize>().max_value(2))]
                .iter()
                .map(|m| m.to_string())
                .collect(),
            from: if i < 4 { FROM[i] } else { tc.draw(gs::sampled_from(&FROM[..]).print_as_debug()) },
            library: None,
        })
        .collect();
    let lines = check_lines(&r);
    for p in &r.players {
        let models = if p.models.is_empty() { "no player models".to_string() } else { p.models.join(", ") };
        let head = format!("{}: {models}", p.drive);
        let (tone, line) = lines.iter().find(|(_, l)| l.starts_with(&format!("{}: ", p.drive))).expect("said");
        assert_eq!(*tone, Tone::Faint);
        let more = &line[head.len()..];
        match p.from {
            PlayersFrom::Given => assert_eq!(more, "", "{line}"),
            PlayersFrom::Library => assert!(more.contains("remembered for its library"), "{line}"),
            PlayersFrom::Default => assert_eq!(more, " (remembered)", "{line}"),
            PlayersFrom::None => assert!(more.contains("players set remembers"), "{line}"),
        }
    }

    if tc.draw(gs::booleans()) {
        let (mut app, _dir, root) = demo_app();
        app.drive_act(Act::Check, root);
        assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Checking DEMO"));
        wait(&mut app, |a| a.job.is_none());
        let Some(Modal::Report(r)) = &app.modal else { panic!("no report: {}", last_message(&app)) };
        assert_eq!(r.title, "Check · DEMO");
        let verdict = &r.lines[0].1;
        assert!(
            verdict.starts_with("Ready") || verdict.starts_with("No problems") || verdict.contains("problem"),
            "{verdict}"
        );
        assert!(r.lines.iter().any(|(t, l)| *t == Tone::Heading && l == "DEMO"), "the drive's own part");
    }
}
