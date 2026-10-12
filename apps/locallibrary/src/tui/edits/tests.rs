use std::sync::mpsc::channel;

use locallibrary::progress::Phase;
use ratatui::crossterm::event::KeyModifiers;

use super::super::app::testing::*;
use super::super::drives::Tone;
use super::*;

fn paths() -> Vec<String> {
    vec!["/Contents/a.mp3".into(), "/Contents/b.mp3".into()]
}

#[test]
fn builds_edits_naming_tracks_by_file() {
    match set_rating(&paths(), 4) {
        Edit::EditTracks { tracks, fields } => {
            assert_eq!(tracks.paths, paths());
            assert_eq!(fields, TrackFields { rating: Some(4), ..Default::default() });
        }
        e => panic!("{e:?}"),
    }
    match set_colour(&paths(), None) {
        Edit::EditTracks { fields, .. } => assert_eq!(fields.color.as_deref(), Some("none")),
        e => panic!("{e:?}"),
    }
    match set_colour(&paths(), Some(TrackColor::Aqua)) {
        Edit::EditTracks { fields, .. } => assert_eq!(fields.color_value().unwrap(), Some(Some(TrackColor::Aqua))),
        e => panic!("{e:?}"),
    }
    let e = create("Shows/Friday", false, &paths());
    assert_eq!(e.len(), 2);
    assert!(matches!(&e[0], Edit::CreatePlaylist { path, folder: false } if path == "Shows/Friday"));
    assert!(
        matches!(&e[1], Edit::AddTracks { playlist, tracks, .. } if playlist == "Shows/Friday" && tracks.paths.len() == 2)
    );
    assert_eq!(create("Shows", true, &[]).len(), 1);
    let by = SortKey { field: locallibrary::query::SortField::Key, descending: false };
    assert!(matches!(sort("Shows/Friday", by), Edit::SortPlaylist { by, .. } if by == "key"));
    assert!(
        matches!(remove_from("A", &paths()), Edit::RemoveTracks { tracks: Some(_), positions, .. } if positions.is_empty())
    );
}

#[test]
fn typed_paths() {
    assert_eq!(clean_path(" Shows / 2026/Friday "), Some("Shows/2026/Friday".into()));
    assert_eq!(clean_path("/Warmup"), Some("Warmup".into()));
    assert_eq!(clean_path("Shows/"), None);
    assert_eq!(clean_path("a//b"), None);
    assert_eq!(clean_path("  "), None);
}

#[test]
fn menu_says_why_actions_are_unavailable() {
    let t = Target { root: "/Volumes/A".into(), tracks: paths(), what: "2 marked tracks".into(), ..Default::default() };
    let items = menu_items(&t);
    let find = |items: &[MenuItem], a| items.iter().find(|i| i.action == a).map(|i| i.unavailable.clone()).unwrap();
    assert_eq!(find(&items, Action::RatingMenu), None);
    assert_eq!(find(&items, Action::RemoveFromList).as_deref(), Some("the list shown isn't a playlist"));
    assert!(find(&items, Action::Rename).is_some());
    assert_eq!(find(&items, Action::ClearCrate).as_deref(), Some("the crate is empty"));
    let t = Target {
        list: Some("Sets/Friday".into()),
        node: Some(("Sets/Friday".into(), false)),
        folder: "Sets".into(),
        sort: Some(SortKey { field: locallibrary::query::SortField::Bpm, descending: true }),
        ..t
    };
    let items = menu_items(&t);
    assert_eq!(find(&items, Action::RemoveFromList), None);
    assert_eq!(find(&items, Action::SortList), None);
    assert!(items.iter().any(|i| i.label == "sort Friday by BPM ↓ on the drive"));
    assert!(items.iter().any(|i| i.label == "new playlist in Sets…"));
    let t = Target { stale: true, ..t };
    assert!(menu_items(&t).iter().filter(|i| i.action != Action::CrateAdd).all(|i| i.unavailable.is_some()));
}

fn clear_line(app: &mut App) {
    app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
}

/// The menu open now: whether a picker is open in it, and the rows selected.
fn menu(app: &App) -> (Option<Sub>, Option<usize>, Option<usize>) {
    match &app.modal {
        Some(Modal::Menu(m)) => (m.sub, m.state.selected(), m.sub_state.selected()),
        _ => panic!("no menu: {}", last_message(app)),
    }
}

/// The menu moves with the arrows and j/k, opens what's selected with enter or right, and
/// says why an item can't be done (staying open). The rating and colour pickers move the
/// same way, take a digit, and go back to the menu with esc or left.
#[test]
fn the_menu_and_its_pickers_answer_to_keys() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "ej");
    key(&mut app, KeyCode::Down);
    press(&mut app, "k");
    assert_eq!(menu(&app).1, Some(1));
    key(&mut app, KeyCode::Up);
    key(&mut app, KeyCode::Up);
    assert_eq!(menu(&app).1, Some(0));
    press(&mut app, "x");
    assert_eq!(last_message(&app), "the list shown isn't a playlist");
    assert_eq!(menu(&app).1, Some(6), "still open, on that item");

    // The rating picker: opened with right, moved through, and left again.
    press(&mut app, "kkkkk");
    key(&mut app, KeyCode::Right);
    assert_eq!(menu(&app), (Some(Sub::Rating), Some(1), Some(0)));
    press(&mut app, "jj");
    key(&mut app, KeyCode::Down);
    press(&mut app, "k");
    assert_eq!(menu(&app).2, Some(2));
    key(&mut app, KeyCode::Up);
    press(&mut app, "9x");
    key(&mut app, KeyCode::Tab);
    assert_eq!(menu(&app), (Some(Sub::Rating), Some(1), Some(1)), "no 9 stars, nor anything else");
    key(&mut app, KeyCode::Left);
    key(&mut app, KeyCode::Tab);
    assert_eq!(menu(&app), (None, Some(1), Some(1)));

    // The colour picker: as far down as it goes; esc goes back, then closes the menu.
    press(&mut app, "c");
    assert_eq!(menu(&app).0, Some(Sub::Colour));
    for _ in 0..12 {
        key(&mut app, KeyCode::Down);
    }
    assert_eq!(menu(&app).2, Some(8));
    key(&mut app, KeyCode::Esc);
    assert_eq!(menu(&app).0, None);
    press(&mut app, "q");
    assert!(app.modal.is_none());

    // A colour by its digit, a rating chosen with enter: each planned.
    press(&mut app, "ec3");
    assert!(app.modal.is_none() && app.job.is_some());
    wait(&mut app, |a| a.job.is_none());
    press(&mut app, "e");
    key(&mut app, KeyCode::Enter);
    assert!(app.job.is_none(), "adding to the crate plans nothing");
    assert_eq!(app.crate_list.len(), 1);
    press(&mut app, "ej\nj\n");
    assert!(app.modal.is_none() && app.job.is_some(), "4 stars, planned");
    wait(&mut app, |a| a.job.is_none());
}

/// The prompts for a new folder, a rename and saving the crate: a name that won't do says
/// what would and stays; one that will is planned.
#[test]
fn prompts_say_what_a_name_needs() {
    let (mut app, _dir) = app_with(library());
    let prompt = |app: &App| match &app.modal {
        Some(Modal::Prompt(p)) => (p.label.clone(), p.text.clone()),
        _ => panic!("no prompt: {}", last_message(app)),
    };
    press(&mut app, "eF");
    assert_eq!(prompt(&app), ("New folder".into(), String::new()));
    press(&mut app, "\n");
    assert_eq!(last_message(&app), "give it a name, with any folders before it: Sets/2026/Friday");
    press(&mut app, "Crates\n");
    assert!(app.modal.is_none() && app.job.is_some());
    wait(&mut app, |a| a.job.is_none());

    // Friday, in the tree: renamed (a name, not a path), deleted, and the crate saved beside it.
    let l = app.loaded.as_mut().unwrap();
    l.reveal(&NodeId::Playlist(2));
    let friday = l.tree_position(&NodeId::Playlist(2));
    app.focus = Focus::Playlists;
    app.playlist_state.select(friday);
    press(&mut app, "er");
    assert_eq!(prompt(&app), ("Rename Friday to".into(), "Friday".into()));
    clear_line(&mut app);
    press(&mut app, "Fri/day\n");
    assert_eq!(last_message(&app), "give it a name (without /)");
    clear_line(&mut app);
    press(&mut app, "Saturday\n");
    assert!(app.modal.is_none() && app.job.is_some());
    wait(&mut app, |a| a.job.is_none());
    press(&mut app, "ed");
    assert!(app.modal.is_none() && app.job.is_some(), "deleting it is planned");
    wait(&mut app, |a| a.job.is_none());

    app.focus = Focus::Tracks;
    press(&mut app, "a");
    app.focus = Focus::Playlists;
    press(&mut app, "ew");
    assert_eq!(prompt(&app), ("Save the crate's 1 track as".into(), "Sets/Crate".into()));
    press(&mut app, "\n");
    assert!(app.modal.is_none() && app.job.is_some());
    wait(&mut app, |a| a.job.is_none());
    // Esc puts a prompt away.
    press(&mut app, "eN\u{1b}");
    assert!(app.modal.is_none() && app.job.is_none());
}

/// A new My Tag, a smart playlist's path and the playlist to bring back each say what they
/// need when it's missing, and stay open.
#[test]
fn prompts_for_tags_rules_and_restores_say_what_they_need() {
    let (mut app, _dir) = app_with(library());
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    let still_asking = |app: &App| assert!(matches!(app.modal, Some(Modal::Prompt(_))), "{}", last_message(app));
    app.tag_chosen(root, vec!["/Contents/1.mp3".into()], super::super::mytags::TagChoice::New);
    clear_line(&mut app);
    press(&mut app, "Deep\n");
    assert!(last_message(&app).starts_with("a My Tag is Category/Tag"));
    still_asking(&app);
    press(&mut app, "\u{1b}");

    press(&mut app, "/bpm:>120\n");
    app.perform(Act::SmartNew, None);
    clear_line(&mut app);
    press(&mut app, "\n");
    assert!(last_message(&app).starts_with("give it a path"));
    still_asking(&app);
    press(&mut app, "\u{1b}");

    press(&mut app, "eb \n");
    assert_eq!(last_message(&app), "name the playlist or folder: Sets/2026/Friday");
    still_asking(&app);
}

/// A report scrolls with the keys and closes with any other; a question waits for y or n.
#[test]
fn reports_scroll_and_questions_wait_for_an_answer() {
    let (mut app, _dir) = app_with(library());
    let lines = vec![(Tone::Text, "one".to_string())];
    app.modal = Some(Modal::Report(Report { title: "Check".into(), lines, scroll: 0 }));
    let scroll = |app: &App| match &app.modal {
        Some(Modal::Report(r)) => r.scroll,
        _ => panic!("closed"),
    };
    press(&mut app, "jj");
    key(&mut app, KeyCode::PageDown);
    press(&mut app, "k ");
    key(&mut app, KeyCode::PageUp);
    key(&mut app, KeyCode::Up);
    assert_eq!(scroll(&app), 10);
    press(&mut app, "x");
    assert!(app.modal.is_none());

    let ask = |app: &mut App| {
        app.modal = Some(Modal::Ask("Sure?".into(), Box::new(|app: &mut App| app.toast("done it", Level::Info))));
    };
    ask(&mut app);
    press(&mut app, "x");
    assert!(matches!(app.modal, Some(Modal::Ask(..))), "only y or n answers");
    press(&mut app, "n");
    assert!(app.modal.is_none() && app.toast.is_none());
    ask(&mut app);
    press(&mut app, "Y");
    assert_eq!(last_message(&app), "done it");
}

/// A plan scrolls while it's shown; n says nothing was written. Asked to write while
/// another job runs, it waits.
#[test]
fn a_confirmation_scrolls_and_waits_for_other_work() {
    let (_dir, root) = demo_drive();
    let mut app = app_on(&root);
    press(&mut app, "e4");
    assert_eq!(confirming(&mut app).0, "edit");
    settle(&mut app);
    press(&mut app, "jj");
    key(&mut app, KeyCode::PageDown);
    press(&mut app, "k");
    key(&mut app, KeyCode::PageUp);
    key(&mut app, KeyCode::Down);
    assert!(matches!(&app.modal, Some(Modal::Confirm(c)) if c.scroll == 2));
    app.job = Some(job(false, false));
    press(&mut app, "y");
    assert_eq!(last_message(&app), "wait for planning to finish");
    app.job = None;
    press(&mut app, "e4");
    settle(&mut app);
    press(&mut app, "n");
    assert!(app.modal.is_none() && app.job.is_none());
    assert_eq!(last_message(&app), "nothing written");
}

/// Work asked for while a job runs waits for it; nothing is planned for a drive that was
/// unplugged, or with no drive open. What comes back for a job that was cancelled is dropped,
/// and a cancelled plan says nothing.
#[test]
fn jobs_one_at_a_time_and_only_for_a_drive_that_is_there() {
    let (mut app, _dir) = app_with(library());
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    app.job = Some(job(false, false));
    press(&mut app, "u");
    assert_eq!(last_message(&app), "wait for planning to finish");
    app.task("Checking".into(), |_| Ok(Box::new(|_: &mut App| {})));
    assert_eq!(last_message(&app), "wait for planning to finish");

    app.handle_msg(Msg::Planned { job: 98, root: root.clone(), result: Err(Error::Cancelled) });
    assert!(app.job.is_some(), "another job's plan");
    app.toast = None;
    app.handle_msg(Msg::Planned { job: 99, root: root.clone(), result: Err(Error::Cancelled) });
    assert!(app.job.is_none() && app.toast.is_none());

    app.handle_msg(Msg::Volumes(Vec::new(), Some((root, false))));
    press(&mut app, "u");
    assert!(app.job.is_none());
    assert_eq!(last_message(&app), "the drive was unplugged");
    // A rating by its digit too: the menu stays, and says why.
    press(&mut app, "e3");
    assert!(matches!(app.modal, Some(Modal::Menu(_))) && app.job.is_none());
    assert_eq!(last_message(&app), "the drive was unplugged");

    let (mut no_drive, _store) = blank(View::Library);
    press(&mut no_drive, "u");
    assert_eq!(last_message(&no_drive), "open a drive first");
    press(&mut no_drive, "e");
    assert!(no_drive.modal.is_none());
    assert_eq!(last_message(&no_drive), "open a drive first");
}

/// A task that fails says what it was doing; one cancelled says nothing.
#[test]
fn a_failed_task_says_what_it_was_doing() {
    let (mut app, _dir) = app_with(library());
    app.task("Checking TEST".into(), |_| Err(Error::Invalid("no export here".into())));
    wait(&mut app, |a| a.job.is_none());
    let said = last_message(&app);
    assert!(said.starts_with("checking test: ") && said.ends_with("no export here"), "{said}");
    app.toast = None;
    app.task("Checking TEST".into(), |_| Err(Error::Cancelled));
    wait(&mut app, |a| a.job.is_none());
    assert!(app.toast.is_none());
}

/// Esc while a job runs: planning stops at once; a write stops only until the databases are
/// being written, and a stopped job's progress no longer shows.
#[test]
fn esc_stops_a_job_only_before_it_writes() {
    let (mut app, _dir) = app_with(library());
    app.job = Some(job(true, false));
    app.on_progress(99, "copying · 1.mp3".into(), false);
    press(&mut app, "\u{1b}");
    let j = app.job.as_ref().unwrap();
    assert!(j.cancel.is_cancelled() && j.detail == "cancelling…");
    app.on_progress(99, "copying · 2.mp3".into(), true);
    let j = app.job.as_ref().unwrap();
    assert_eq!((j.detail.as_str(), j.committed), ("cancelling…", true));

    app.job = Some(job(true, true));
    press(&mut app, "\u{1b}");
    assert_eq!(last_message(&app), "writing the drive: too late to stop");
    assert!(!app.job.as_ref().unwrap().cancel.is_cancelled());
    app.job = Some(job(false, false));
    press(&mut app, "\u{1b}");
    assert!(app.job.is_none());
    assert_eq!(last_message(&app), "cancelled");
}

/// What applying came to, as the footer says it.
#[test]
fn says_what_a_write_came_to() {
    let (mut app, _dir) = app_with(library());
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    let drive = app.drive_label(&root);
    let mut applied = |operation: &'static str, report: serde_json::Value| {
        let result = Ok(Applied { operation, report });
        app.handle_msg(Msg::Applied { job: 1, root: root.clone(), result });
        last_message(&app).to_string()
    };
    assert_eq!(
        applied("eject", serde_json::json!([{ "powered_off": true }])),
        format!("ejected {drive}: it can be unplugged")
    );
    assert_eq!(applied("eject", serde_json::json!([{ "powered_off": false }])), format!("unmounted {drive}"));
    let synced = serde_json::json!([{ "name": "A", "error": null }, { "name": "B", "error": null }]);
    assert_eq!(applied("sync.group", synced), "synced 2 drives");
    let failed =
        serde_json::json!([{ "name": "A", "error": null }, { "name": "B", "error": { "message": "it's full" } }]);
    assert_eq!(applied("sync.group", failed), "sync: B: it's full");
    let saved = serde_json::json!({ "up_to_date": true, "library": "Main", "snapshot": { "short_id": "3lzabcdefgh" } });
    assert_eq!(applied("pull", saved), format!("{drive} was saved in the store already (Main, 3lzabcde)"));
    let warned = serde_json::json!({ "warnings": ["a track was missing"] });
    assert_eq!(applied("smart.write", warned), format!("smart.write: done on {drive} · a track was missing"));

    let full = Error::NoSpace { path: root.clone(), needed: 10, available: 3 };
    app.handle_msg(Msg::Applied { job: 1, root, result: Err(full) });
    assert_eq!(last_message(&app), format!("not enough free space on {drive} for this change"));
}

/// The header follows a job's progress: each phase as it starts, then the files in it (not
/// more often than it can be read). Cancelling stops the job only until it writes.
#[test]
fn progress_is_told_and_cancelling_stops_only_before_writing() {
    let (tx, rx) = channel();
    let (outer, inner) = (CancelToken::new(), CancelToken::new());
    let reporter = Reporter::new(tx, 7, outer.clone(), inner.clone());
    let start = |phase| Event::PhaseStart { phase, items: 0, bytes: 0 };
    let told = || {
        rx.try_iter()
            .map(|m| match m {
                Msg::Progress { job: 7, detail, committed } => (detail, committed),
                _ => panic!("not progress"),
            })
            .collect::<Vec<_>>()
    };
    for phase in [Phase::Scan, Phase::Hash, Phase::Copy, Phase::Verify, Phase::Format, Phase::Match] {
        reporter.on_event(&start(phase));
    }
    let names: Vec<String> = told().into_iter().map(|(d, _)| d).collect();
    assert_eq!(names, ["scanning", "hashing", "copying", "verifying", "formatting", "working"]);
    // A file is told as the phase and its name (unless one was told a moment ago); what
    // else happens isn't.
    reporter.on_event(&Event::Item { phase: Phase::Copy, path: "/Contents/a/1.mp3".into(), bytes: 0 });
    reporter.on_event(&Event::Bytes { phase: Phase::Copy, bytes: 10 });
    assert!(told().iter().all(|(d, _)| d == "copying · 1.mp3"));

    outer.cancel();
    reporter.on_event(&Event::Bytes { phase: Phase::Copy, bytes: 10 });
    assert!(inner.is_cancelled(), "not writing yet: it stops");
    let (outer, inner) = (CancelToken::new(), CancelToken::new());
    let (tx, rx) = channel();
    let reporter = Reporter::new(tx, 7, outer.clone(), inner.clone());
    reporter.on_event(&start(Phase::Delete));
    outer.cancel();
    reporter.on_event(&start(Phase::Write));
    assert!(!inner.is_cancelled(), "writing: too late to stop");
    let committed: Vec<bool> = rx.try_iter().map(|m| matches!(m, Msg::Progress { committed: true, .. })).collect();
    assert_eq!(committed, [true, true]);
}

/// Packing the crate asks what to call the playlist it's saved as first; a name that won't
/// do says what would.
#[test]
fn packing_the_crate_needs_a_playlist_name() {
    let (mut app, _dir) = app_with(library());
    let other = tempfile::tempdir().unwrap();
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    let info = |r: &std::path::Path| locallibrary::Device::open(r).unwrap().info();
    app.drives = vec![info(&root), info(other.path())];
    press(&mut app, "a");
    app.perform(Act::Pack, None);
    press(&mut app, "crate\n\n");
    assert!(matches!(&app.modal, Some(Modal::Prompt(p)) if p.text.starts_with("Sets/")), "{}", last_message(&app));
    clear_line(&mut app);
    press(&mut app, "\n");
    assert_eq!(last_message(&app), "give it a name, with any folders before it: Sets/2026/Friday");
    assert!(matches!(app.modal, Some(Modal::Prompt(_))) && app.job.is_none());
}
