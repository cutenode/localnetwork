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
        let result = Ok(Box::new(Applied { operation: operation.into(), report }));
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

/// The header follows any run of progress: each phase is told as it starts, by name, and a file
/// in it as the phase and the file's name; it's too late to stop once the databases are being
/// written or files deleted (said with each word after), and a cancel reaches the job only
/// before then.
#[hegel::test(test_cases = 300)]
fn progress_is_told_and_a_cancel_stops_only_before_writing(tc: hegel::TestCase) {
    use hegel::generators as gs;
    const PHASES: [Phase; 9] = [
        Phase::Scan,
        Phase::Hash,
        Phase::Copy,
        Phase::Write,
        Phase::Delete,
        Phase::Verify,
        Phase::Format,
        Phase::Match,
        Phase::Listen,
    ];
    let (tx, rx) = channel();
    let (outer, inner) = (CancelToken::new(), CancelToken::new());
    let reporter = Reporter::new(tx, 7, outer.clone(), inner.clone());
    let (mut phase, mut committed, mut stopped) = (String::new(), false, false);
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(12)) {
        if tc.draw(gs::integers::<u8>().max_value(5)) == 0 {
            outer.cancel();
        }
        let p = tc.draw(gs::sampled_from(&PHASES[..]));
        let event = match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => Event::PhaseStart { phase: p, items: 0, bytes: 0 },
            1 => Event::Item {
                phase: p,
                path: format!("/Contents/{}/x.mp3", tc.draw(gs::integers::<u8>().max_value(3))),
                bytes: 0,
            },
            _ => Event::Bytes { phase: p, bytes: 10 },
        };
        reporter.on_event(&event);
        if let Event::PhaseStart { phase: p, .. } = &event {
            phase = phase_name(*p).to_string();
            committed |= matches!(p, Phase::Write | Phase::Delete);
        }
        stopped |= !committed && outer.is_cancelled();
        let told: Vec<(String, bool)> = (rx.try_iter())
            .map(|m| match m {
                Msg::Progress { job: 7, detail, committed } => (detail, committed),
                _ => panic!("not progress"),
            })
            .collect();
        assert!(told.len() <= 1, "{told:?}");
        match &event {
            Event::PhaseStart { .. } => assert_eq!(told, [(phase.clone(), committed)], "a phase is told as it starts"),
            Event::Item { .. } => {
                assert!(told.iter().all(|t| *t == (format!("{phase} · x.mp3"), committed)), "{told:?}")
            }
            _ => assert!(told.is_empty(), "{told:?}"),
        }
        assert_eq!(inner.is_cancelled(), stopped, "committed {committed}");
    }
}

/// What a write came to is said as such, whatever it was: what was saved in the store (or that
/// it was already), ejected or only unmounted, which drives synced or why they didn't, My Tags
/// changed, a set packed, a backup put back, changes written (and that u undoes them), with the
/// first warning; or that it was cancelled, the drive was busy or full, or what went wrong. What
/// made a backup is what u takes back.
#[hegel::test(test_cases = 300)]
fn a_write_is_said_as_what_it_came_to(tc: hegel::TestCase) {
    // Every kind of write, each case.
    for kind in 0..12u8 {
        said_as_what_it_came_to(&tc, kind);
    }
}

/// [`a_write_is_said_as_what_it_came_to`] for one kind of write.
fn said_as_what_it_came_to(tc: &hegel::TestCase, kind: u8) {
    use hegel::generators as gs;
    use serde_json::json;
    let (mut app, _dir) = app_with(library());
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    let drive = app.drive_label(&root);
    let backup = tc.draw(gs::booleans());
    let b = |report: &mut serde_json::Value| {
        if backup {
            report["backup"] = json!("3lzbackup");
        }
    };
    let undo = if backup { " · u to undo" } else { "" };
    let n = tc.draw(gs::integers::<u64>().max_value(3));
    let s = |n: u64| if n == 1 { "" } else { "s" };
    let (result, want, backed) = match kind {
        0 => {
            let up = tc.draw(gs::booleans());
            let report = json!({ "up_to_date": up, "library": "Main", "snapshot": { "short_id": "3lzabcdefgh" }, "imported_files": n, "imported_bytes": 2048 });
            let want = if up {
                format!("{drive} was saved in the store already (Main, 3lzabcde)")
            } else {
                format!("saved {drive} in the store: snapshot 3lzabcde of Main ({n} new file{}, 2.0 KB)", s(n))
            };
            (Ok(Applied { operation: "pull".into(), report }), want, false)
        }
        1 => {
            let offs: Vec<bool> = (0..=n).map(|_| tc.draw(gs::booleans())).collect();
            let report = json!(offs.iter().map(|o| json!({ "powered_off": o })).collect::<Vec<_>>());
            let want = if offs.iter().all(|o| *o) {
                format!("ejected {drive}: it can be unplugged")
            } else {
                format!("unmounted {drive}")
            };
            (Ok(Applied { operation: "eject".into(), report }), want, false)
        }
        2 => {
            let failed: Vec<bool> = (0..=n).map(|_| tc.draw(gs::booleans())).collect();
            let rows: Vec<_> = (failed.iter().enumerate())
                .map(|(i, f)| json!({ "name": format!("D{i}"), "error": if *f { json!({ "message": "it's full" }) } else { json!(null) } }))
                .collect();
            let said: Vec<String> =
                (failed.iter().enumerate()).filter(|(_, f)| **f).map(|(i, _)| format!("D{i}: it's full")).collect();
            let want = if said.is_empty() {
                format!("synced {} drive{}", n + 1, s(n + 1))
            } else {
                format!("sync: {}", said.join("; "))
            };
            (Ok(Applied { operation: "sync.group".into(), report: json!(rows) }), want, false)
        }
        3 => {
            let mut report = json!({ "tracks_changed": n });
            b(&mut report);
            (
                Ok(Applied { operation: "tags".into(), report }),
                format!("My Tags changed on {n} track{} on {drive}{undo}", s(n)),
                backup,
            )
        }
        4 => {
            let mut report = json!({ "copied": n, "copied_bytes": 1_500_000 });
            b(&mut report);
            (
                Ok(Applied { operation: "pack".into(), report }),
                format!("packed onto {drive}: {n} file{} copied (1.5 MB){undo}", s(n)),
                backup,
            )
        }
        5 => {
            let report = json!({ "created": "2026-10-03T21:15:09Z", "reason": "edit" });
            (
                Ok(Applied { operation: "undo".into(), report }),
                format!("undone on {drive}: put back the backup (edit, 2026-10-03 21:15)"),
                true,
            )
        }
        6 => {
            let warned = tc.draw(gs::booleans());
            let databases = tc.draw(gs::booleans());
            let mut report = json!({ "changes": n });
            if databases {
                report["databases"] = json!(["export.pdb", "exportLibrary.db"]);
            }
            if warned {
                report["warnings"] = json!(["a track was missing", "and another"]);
            }
            b(&mut report);
            let head = if databases {
                format!("{n} change{} written to export.pdb and exportLibrary.db on {drive}", s(n))
            } else {
                format!("edit: done on {drive}")
            };
            let warning = if warned { " · a track was missing" } else { "" };
            (Ok(Applied { operation: "edit".into(), report }), format!("{head}{undo}{warning}"), backup)
        }
        7 => (Err(Error::Cancelled), "cancelled: nothing was written".to_string(), false),
        8 => (
            Err(Error::Locked(root.clone())),
            format!("{drive} is busy: another locallibrary command is changing it. Try again when it's done."),
            false,
        ),
        9 => (
            Err(Error::NoSpace { path: root.clone(), needed: 10, available: 3 }),
            format!("not enough free space on {drive} for this change"),
            false,
        ),
        _ => {
            let e = Error::Invalid("the drive changed since it was planned".into());
            let said = e.to_string();
            (Err(e), said, false)
        }
    };
    let ok = result.is_ok();
    // (The job it was for, or one stopped since: said either way.)
    if tc.draw(gs::booleans()) {
        app.job = Some(super::super::edits::Job { id: 5, ..job(true, false) });
    }
    app.handle_msg(Msg::Applied { job: 5, root: root.clone(), result: result.map(Box::new) });
    assert_eq!(last_message(&app), want);
    assert!(app.job.is_none(), "the write is done");
    assert_eq!(app.last_backup.as_ref().map(|k| &k.root), backed.then_some(&root), "what u takes back");
    // Whatever happened, the drive is read again.
    assert!(app.loading.is_some() || !ok);
}

/// A confirmation takes no key for a moment after it shows (but esc); then y applies, n, q and esc
/// put it away writing nothing, and the arrows, j, k and the page keys scroll it, never above its
/// top.
#[hegel::test(test_cases = 200)]
fn a_confirmation_waits_then_scrolls_or_answers(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (mut app, _dir) = app_with(library());
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    let planned = Plan { operation: "edit", summary: vec!["a line".into(); 40], ..Default::default() };
    app.modal = Some(Modal::Confirm(Confirm {
        root,
        drive: "TEST".into(),
        plan: Box::new(planned),
        scroll: 0,
        shown: Instant::now(),
        after: None,
    }));
    let mut scroll: u16 = 0;
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(10)) {
        let settled = tc.draw(gs::booleans());
        if let Some(Modal::Confirm(c)) = &mut app.modal {
            c.shown = if settled { Instant::now() - Duration::from_secs(1) } else { Instant::now() };
        }
        let code = tc.draw(gs::sampled_from(
            &[
                KeyCode::Up,
                KeyCode::Char('k'),
                KeyCode::Down,
                KeyCode::Char('j'),
                KeyCode::PageUp,
                KeyCode::PageDown,
                KeyCode::Char('x'),
                KeyCode::Char('n'),
                KeyCode::Char('q'),
                KeyCode::Esc,
            ][..],
        ));
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
        if !settled && code != KeyCode::Esc {
            assert!(matches!(app.modal, Some(Modal::Confirm(_))), "{code:?} taken before it could be read");
            continue;
        }
        let by: i32 = match code {
            KeyCode::Up | KeyCode::Char('k') => -1,
            KeyCode::Down | KeyCode::Char('j') => 1,
            KeyCode::PageUp => -10,
            KeyCode::PageDown => 10,
            KeyCode::Char('x') => 0,
            _ => {
                assert!(app.modal.is_none() && app.job.is_none() && last_message(&app) == "nothing written");
                return;
            }
        };
        scroll = (scroll as i32 + by).max(0) as u16;
        let Some(Modal::Confirm(c)) = &app.modal else { panic!("{code:?} closed it") };
        assert_eq!(c.scroll, scroll, "{code:?}");
    }
}

/// A playlist path as typed: its parts with spaces about them and a slash before it read back as
/// the parts; a part left empty makes it no path.
#[hegel::test(test_cases = 300)]
fn typed_paths_read_back_as_their_parts(tc: hegel::TestCase) {
    use hegel::generators as gs;
    // (An empty first part would be a slash before it.)
    let mut parts: Vec<&str> = vec![tc.draw(gs::sampled_from(&["Sets", "Fri day", "日本"][..]))];
    parts.extend(tc.draw(gs::vecs(gs::sampled_from(&["Sets", "2026", "Fri day", "日本", ""][..])).max_size(3)));
    let pad = |tc: &hegel::TestCase| tc.draw(gs::sampled_from(&["", " ", "  "][..]));
    let typed: String = format!(
        "{}{}{}",
        pad(&tc),
        if tc.draw(gs::booleans()) { "/" } else { "" },
        parts.iter().map(|p| format!("{}{p}{}", pad(&tc), pad(&tc))).collect::<Vec<_>>().join("/")
    );
    let read = clean_path(&typed);
    if parts.contains(&"") {
        assert_eq!(read, None, "{typed:?}");
    } else {
        assert_eq!(read, Some(parts.join("/")), "{typed:?}");
        assert_eq!(leaf(read.as_deref().unwrap()), *parts.last().unwrap());
    }
}

/// What a prompt asks for, and whether what's typed is that.
#[derive(Debug, Clone, Copy)]
enum Asks {
    Path,
    Name,
    Tag,
    Rule,
    Restore,
}

/// Enter in a prompt takes what's typed when it's what the prompt asks for (a playlist's path,
/// a name without a slash, a My Tag as Category/Tag, a rule's path, a playlist to bring back)
/// and goes on to plan it; otherwise it says what's needed and stays open with the text as it was.
#[hegel::test(test_cases = 200)]
fn a_prompt_takes_only_what_it_asks_for(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (mut app, _dir) = app_with(library());
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    let tracks = vec!["/Contents/1.mp3".to_string()];
    let (purpose, asks) = match tc.draw(gs::integers::<u8>().max_value(7)) {
        0 => (PromptFor::NewPlaylist { with: tracks }, Asks::Path),
        1 => (PromptFor::NewFolder, Asks::Path),
        2 => (PromptFor::SaveCrate { tracks }, Asks::Path),
        3 => (PromptFor::Rename { playlist: "Sets/Friday".into() }, Asks::Name),
        4 => (PromptFor::NewTag { tracks }, Asks::Tag),
        5 => (PromptFor::SmartRule { filter: "bpm:>120".into(), sort: None }, Asks::Rule),
        6 => (PromptFor::RestorePlaylist, Asks::Restore),
        _ => (PromptFor::CrateForPack { tracks, target: root.clone() }, Asks::Path),
    };
    let text: String = tc
        .draw(gs::sampled_from(
            &["", " ", "Friday", "Sets/Friday", "Sets//x", "/", "Genre/Deep House", "a/b/c", " Peak /"][..],
        ))
        .into();
    let fits = match asks {
        Asks::Path | Asks::Rule => clean_path(&text).is_some(),
        Asks::Name => !text.trim().is_empty() && !text.contains('/'),
        Asks::Tag => {
            let tag = text.trim().trim_matches('/');
            tag.split('/').count() == 2 && tag.split('/').all(|p| !p.trim().is_empty())
        }
        Asks::Restore => !text.trim().trim_matches('/').is_empty(),
    };
    app.modal = Some(Modal::Prompt(Prompt { label: "Name it".into(), text: text.clone(), purpose, root }));
    press(&mut app, "\n");
    match &app.modal {
        Some(Modal::Prompt(p)) if !fits => {
            assert_eq!(p.text, text, "kept as typed");
            let hint = last_message(&app);
            let needs = match asks {
                Asks::Path => "give it a name, with any folders before it",
                Asks::Name => "give it a name (without /)",
                Asks::Tag => "a My Tag is Category/Tag",
                Asks::Rule => "give it a path, with any folders before it",
                Asks::Restore => "name the playlist or folder",
            };
            assert!(hint.starts_with(needs), "{asks:?} {text:?}: {hint}");
        }
        _ if !fits => panic!("{asks:?} took {text:?}"),
        other => {
            assert!(!matches!(other, Some(Modal::Prompt(_))), "{asks:?} didn't take {text:?}: {}", last_message(&app));
            assert!(app.job.is_some(), "{asks:?}: nothing under way for {text:?}");
        }
    }
}

/// The actions menu's items that need a name open a prompt for it, starting from what there is:
/// a new playlist or folder in the folder selected (or the shown playlist's), a rename from the
/// name, the crate saved as `Crate` there, what to bring back from the playlist or folder selected;
/// replaying the drive's edits is planned; an item that can't be done says why and stays.
#[hegel::test(test_cases = 200)]
fn the_menu_asks_for_names_from_where_it_is(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (mut app, _dir) = app_with(library());
    let l = app.loaded.as_mut().unwrap();
    l.reveal(&NodeId::Playlist(2));
    let rows = l.tree.len();
    if tc.draw(gs::booleans()) {
        app.focus = Focus::Playlists;
        app.playlist_state.select(Some(tc.draw(gs::integers::<usize>().max_value(rows - 1))));
    } else if tc.draw(gs::booleans()) {
        l.node = NodeId::Playlist(2);
        app.refresh_visible();
    }
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
        app.focus = Focus::Tracks;
        press(&mut app, "aj");
    }
    let t = app.target().unwrap();
    let key = tc.draw(gs::sampled_from(&["N", "F", "r", "w", "X", "b", "y"][..]));
    press(&mut app, "e");
    let unavailable = match &app.modal {
        Some(Modal::Menu(m)) => m.items.iter().find(|i| i.key == key).unwrap().unavailable.clone(),
        _ => panic!("no menu"),
    };
    press(&mut app, key);
    if let Some(why) = unavailable {
        assert_eq!(last_message(&app), why);
        assert!(matches!(app.modal, Some(Modal::Menu(_))), "it stays");
        return;
    }
    // Replaying the drive's recorded edits plans it.
    if key == "y" {
        return assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Planning"));
    }
    let in_folder = if t.folder.is_empty() { String::new() } else { format!("{}/", t.folder) };
    let prompt = match &app.modal {
        Some(Modal::Prompt(p)) => (p.label.clone(), p.text.clone()),
        Some(Modal::Ask(q, _)) => {
            assert_eq!(key, "X");
            assert!(q.contains("crate"), "{q}");
            press(&mut app, "y");
            assert!(app.crate_list.is_empty() && last_message(&app) == "crate cleared");
            return;
        }
        _ => panic!("{key}: no prompt ({})", last_message(&app)),
    };
    let leaf = t.node.as_ref().map(|(p, _)| p.rsplit('/').next().unwrap().to_string());
    let want = match key {
        "N" => ("New playlist".to_string(), in_folder),
        "F" => ("New folder".to_string(), in_folder),
        "r" => (format!("Rename {} to", leaf.clone().unwrap()), leaf.unwrap()),
        // Bringing back the playlist or folder selected (or shown), or one to be named.
        "b" => (
            "Bring back the playlist or folder".to_string(),
            t.node.as_ref().map(|(p, _)| p.clone()).unwrap_or_default(),
        ),
        _ => {
            let n = t.crated.len();
            (format!("Save the crate's {n} track{} as", if n == 1 { "" } else { "s" }), format!("{in_folder}Crate"))
        }
    };
    assert_eq!(prompt, want);
}

/// Picking a playlist to add tracks to: the moving and playback keys still work, others do
/// nothing; enter opens or closes a folder, asks again on a row that isn't a playlist, and on a
/// playlist plans adding the tracks to it, focus going back where it was; esc drops it, saying
/// nothing was added. A job under way makes a plan wait its turn.
#[hegel::test(test_cases = 200)]
fn picking_a_playlist_takes_a_playlist(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (mut app, _dir) = app_with(library());
    let busy = tc.draw(gs::booleans());
    if busy {
        app.job = Some(super::super::edits::Job { label: "Checking DNA".into(), ..job(false, false) });
    }
    app.track_state.select(Some(0));
    let from = if tc.draw(gs::booleans()) { Focus::Tracks } else { Focus::Details };
    app.focus = from;
    let target = app.target().unwrap();
    app.run_action(Action::AddToPlaylist, target);
    assert_eq!(app.focus, Focus::Playlists);
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(8)) {
        let rows = app.loaded.as_ref().unwrap().tree.len();
        let row = tc.draw(gs::integers::<usize>().max_value(rows - 1));
        app.playlist_state.select(Some(row));
        let node = app.loaded.as_ref().unwrap().tree[row].id.clone();
        let folder = app.loaded.as_ref().unwrap().tree[row].folder;
        let before = app.loaded.as_ref().unwrap().tree.len();
        match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => {
                press(&mut app, "\u{1b}");
                assert!(app.picking.is_none() && app.focus == from && last_message(&app) == "nothing added");
                return;
            }
            1 => {
                let k = tc.draw(gs::sampled_from(&["x", "m", "t", "1"][..]));
                press(&mut app, k);
                assert!(app.picking.is_some() && app.modal.is_none(), "{k} taken while picking");
            }
            2 => {
                press(&mut app, "j");
                assert!(app.picking.is_some());
                assert_eq!(app.playlist_state.selected(), Some((row + 1).min(rows - 1)), "j moves");
            }
            _ => {
                press(&mut app, "\n");
                match node {
                    _ if folder => {
                        assert_ne!(app.loaded.as_ref().unwrap().tree.len(), before, "{node:?} opened or closed")
                    }
                    NodeId::Playlist(_) => {
                        assert!(app.picking.is_none() && app.focus == from);
                        if busy {
                            assert_eq!(last_message(&app), "wait for checking dna to finish");
                        } else {
                            assert!(app.job.as_ref().is_some_and(|j| j.label == "Planning"), "adding them is planned");
                        }
                        return;
                    }
                    _ => assert_eq!(last_message(&app), "pick a playlist (esc to cancel)"),
                }
            }
        }
    }
}

/// The menu's pickers and actions, and what comes back from the background, each go where they
/// should: a rating or colour picked (by its digit, or the arrows and enter) is planned, a digit
/// there's no rating or colour for picks nothing; taking tracks out of the playlist shown and
/// sorting it on the drive are planned, the sort named in the menu; a My Tag reads the drive's
/// tags first; a question takes only y or n; while a job runs, another waits for it, saying so;
/// a dialog from the background waits for the one open to close; results for a job stopped since
/// change nothing; with no drive open, the menu and undo say one's needed.
#[hegel::test(test_cases = 40, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_menu_and_the_background_go_where_they_should(tc: hegel::TestCase) {
    use super::super::browse::SORT_FIELDS;
    use super::super::drives::Report;
    use hegel::generators as gs;
    let (mut app, _dir) = app_with(library());
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    let mut turns: Vec<u8> = (0..12).collect();
    for k in (1..turns.len()).rev() {
        turns.swap(k, tc.draw(gs::integers::<usize>().max_value(k)));
    }
    let planned = |app: &App| app.job.as_ref().is_some_and(|j| j.label == "Planning");
    for step in 0..tc.draw(gs::integers::<usize>().min_value(12).max_value(24)) {
        wait(&mut app, |a| a.job.is_none());
        (app.modal, app.focus, app.view) = (None, Focus::Tracks, View::Library);
        app.waiting.clear();
        match turns[step % turns.len()] {
            k @ (0 | 1) => {
                let colour = k == 1;
                press(&mut app, "e");
                press(&mut app, if colour { "c" } else { "0" });
                if !colour {
                    // (0 rates at once: open the picker with enter on its item instead.)
                    wait(&mut app, |a| a.job.is_none());
                    press(&mut app, "e");
                    let Some(Modal::Menu(m)) = &mut app.modal else { panic!("no menu") };
                    let i = m.items.iter().position(|i| i.action == Action::RatingMenu).unwrap();
                    m.state.select(Some(i));
                    key(&mut app, KeyCode::Enter);
                }
                let Some(Modal::Menu(m)) = &app.modal else { panic!("no picker: {}", last_message(&app)) };
                assert_eq!(m.sub, Some(if colour { Sub::Colour } else { Sub::Rating }));
                let max = if colour { 8 } else { 5 };
                let d = tc.draw(gs::integers::<u8>().max_value(9));
                if tc.draw(gs::booleans()) {
                    press(&mut app, &d.to_string());
                    if d > max {
                        assert!(matches!(app.modal, Some(Modal::Menu(_))), "{d} picks nothing");
                        continue;
                    }
                } else {
                    for _ in 0..d {
                        key(&mut app, KeyCode::Down);
                    }
                    key(&mut app, KeyCode::Enter);
                }
                assert!(app.modal.is_none() && planned(&app), "picked, planned");
            }
            2 => {
                // In the playlist shown: taking the track out of it, and sorting it on the drive.
                let l = app.loaded.as_mut().unwrap();
                l.node = NodeId::Playlist(2);
                app.refresh_visible();
                app.track_state.select(Some(0));
                let field = tc.draw(gs::sampled_from(&SORT_FIELDS[..]));
                let descending = tc.draw(gs::booleans());
                app.sort = Some(SortKey { field, descending });
                app.refresh_visible();
                press(&mut app, "e");
                let Some(Modal::Menu(m)) = &app.modal else { panic!("no menu") };
                let label = &m.items.iter().find(|i| i.action == Action::SortList).unwrap().label;
                assert_eq!(
                    *label,
                    format!("sort Friday by {} {} on the drive", sort_label(field), if descending { "↓" } else { "↑" })
                );
                press(&mut app, if tc.draw(gs::booleans()) { "s" } else { "x" });
                assert!(planned(&app), "{}", last_message(&app));
            }
            3 => {
                press(&mut app, "eT");
                assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Reading My Tags"));
            }
            4 => {
                app.modal =
                    Some(Modal::Ask("Sure?".into(), Box::new(|app: &mut App| app.toast("done it", Level::Info))));
                press(&mut app, "x");
                assert!(matches!(app.modal, Some(Modal::Ask(..))), "only y or n answers");
                press(&mut app, if tc.draw(gs::booleans()) { "n" } else { "q" });
                assert!(app.modal.is_none() && last_message(&app) != "done it");
            }
            5 => {
                app.job = Some(Job { label: "Checking DNA".into(), ..job(false, false) });
                match tc.draw(gs::integers::<u8>().max_value(2)) {
                    0 => app.plan_edits(root.clone(), vec![set_rating(&paths(), 3)]),
                    1 => app.task("Reading".into(), |_| Ok(Box::new(|_: &mut App| {}))),
                    _ => {
                        let plan = Box::new(Plan { operation: "edit", ..Default::default() });
                        let c = Confirm {
                            root: root.clone(),
                            drive: "TEST".into(),
                            plan,
                            scroll: 0,
                            shown: Instant::now() - Duration::from_secs(1),
                            after: None,
                        };
                        app.modal = Some(Modal::Confirm(c));
                        press(&mut app, "y");
                    }
                }
                assert_eq!(last_message(&app), "wait for checking dna to finish");
                app.job = None;
            }
            6 => {
                // A report done in the background while a prompt is open: it waits, then shows.
                app.modal = Some(Modal::Prompt(Prompt {
                    label: "Name it".into(),
                    text: String::new(),
                    purpose: PromptFor::NewFolder,
                    root: root.clone(),
                }));
                app.show(Modal::Report(Report { title: "Check".into(), lines: Vec::new(), scroll: 0 }));
                assert_eq!(last_message(&app), "done in the background: it shows once this is closed");
                assert!(matches!(app.modal, Some(Modal::Prompt(_))), "what's typed stays");
                press(&mut app, "\u{1b}");
                app.tick();
                assert!(matches!(app.modal, Some(Modal::Report(_))), "then it shows");
            }
            7 => {
                // What comes back for a job stopped since, or stopped itself, says nothing.
                app.toast = None;
                app.job = Some(Job { id: 5, ..job(false, false) });
                app.handle_msg(Msg::Done {
                    job: 6,
                    result: Ok(Box::new(|app: &mut App| app.toast("late", Level::Info))),
                });
                app.handle_msg(Msg::Planned { job: 6, root: root.clone(), result: Err(Error::Cancelled) });
                assert!(app.job.is_some() && app.toast.is_none(), "another job's");
                match tc.draw(gs::integers::<u8>().max_value(2)) {
                    0 => app.handle_msg(Msg::Done { job: 5, result: Err(Error::Cancelled) }),
                    1 => app.handle_msg(Msg::Planned { job: 5, root: root.clone(), result: Err(Error::Cancelled) }),
                    _ => {
                        app.view = View::Drives;
                        app.handle_msg(Msg::Applied { job: 5, root: root.clone(), result: Err(Error::Cancelled) });
                    }
                }
                assert!(app.job.is_none());
                assert!(app.toast.is_none() || last_message(&app) == "cancelled: nothing was written");
            }
            8 => {
                let (mut none, _store) = blank(View::Library);
                press(&mut none, "e");
                assert_eq!(last_message(&none), "open a drive first");
                none.toast = None;
                press(&mut none, "u");
                assert_eq!(last_message(&none), "open a drive first");
            }
            9 => {
                // A playlist to bring back from the snapshot picked.
                app.modal = Some(Modal::Prompt(Prompt {
                    label: "Bring back".into(),
                    text: "Sets/Friday".into(),
                    purpose: PromptFor::RestoreFrom { snapshot: "3lzabcdef".into() },
                    root: root.clone(),
                }));
                press(&mut app, "\n");
                assert!(planned(&app));
            }
            10 => {
                app.modal = Some(Modal::Prompt(Prompt {
                    label: "Rename".into(),
                    text: tc.draw(gs::sampled_from(&["Saturday", "Sat/urday", " "][..])).into(),
                    purpose: PromptFor::Rename { playlist: "Sets/Friday".into() },
                    root: root.clone(),
                }));
                let ok =
                    matches!(&app.modal, Some(Modal::Prompt(p)) if !p.text.trim().is_empty() && !p.text.contains('/'));
                press(&mut app, "\n");
                assert_eq!(planned(&app), ok);
            }
            11 if tc.draw(gs::booleans()) => {
                // In a picker, up goes up, never past the top; with the drive gone, a digit
                // says why it can't rate.
                press(&mut app, "ec");
                let ups = tc.draw(gs::integers::<usize>().max_value(3));
                press(&mut app, "jj");
                for _ in 0..ups {
                    press(&mut app, "k");
                }
                let Some(Modal::Menu(m)) = &app.modal else { panic!("no picker") };
                assert_eq!(m.sub_state.selected(), Some(2usize.saturating_sub(ups)));
                app.modal = None;
                app.loaded.as_mut().unwrap().stale = true;
                press(&mut app, "e3");
                assert_eq!(last_message(&app), "the drive was unplugged");
                assert!(matches!(app.modal, Some(Modal::Menu(_))), "it stays");
                app.loaded.as_mut().unwrap().stale = false;
            }
            _ => {
                press(&mut app, "e");
                let Some(Modal::Menu(m)) = &app.modal else { panic!("no menu") };
                let rows = m.items.len();
                for _ in 0..tc.draw(gs::integers::<usize>().max_value(rows + 2)) {
                    key(&mut app, KeyCode::Down);
                }
                let Some(Modal::Menu(m)) = &app.modal else { panic!("closed") };
                assert!(m.state.selected().is_some_and(|s| s < rows));
            }
        }
    }
}
