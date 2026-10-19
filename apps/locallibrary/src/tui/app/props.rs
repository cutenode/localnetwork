//! Property tests of the app's own state, as the keys and the background change it: the tree
//! Browse adds to the sidebar, a line being typed, panics caught on workers, the help and the
//! log, quitting while a drive is written, what the background sends back, what's remembered
//! for the next run, and playing through the queue as outputs come and go.

use hegel::TestCase;
use hegel::generators::{self as gs, Generator};
use locallibrary::query::QueryContext;

use super::testing::*;
use super::*;

/// A key as a terminal sends it: `ctrl` held, or not.
fn key_of(code: KeyCode, ctrl: bool) -> KeyEvent {
    KeyEvent::new(code, if ctrl { KeyModifiers::CONTROL } else { KeyModifiers::NONE })
}

/// The kinds of step `0..n` in an order drawn for the case: a walk taking them in turn takes each
/// once in every `n` steps, however its draws lean.
#[hegel::test_helper]
fn turns(tc: &TestCase, n: u8) -> Vec<u8> {
    let mut kinds: Vec<u8> = (0..n).collect();
    for i in (1..kinds.len()).rev() {
        kinds.swap(i, tc.draw(gs::integers::<usize>().max_value(i)));
    }
    kinds
}

/// The test library's playlists and history over made-up tracks, shown on a drive.
#[hegel::test_helper]
fn browsing(tc: &TestCase) -> (App, tempfile::TempDir) {
    let mut lib = library();
    lib.tracks = any_tracks(tc, 8);
    let (mut app, dir) = app_with(lib);
    // The drive's My Tags read, or not yet.
    let l = app.loaded.as_mut().unwrap();
    if tc.draw(gs::booleans()) {
        let tags = l
            .library
            .tracks
            .iter()
            .map(|t| (paths::key(&t.file_path).to_lowercase(), vec!["Genre/House".to_string()]))
            .collect();
        l.my_tags = MyTags::Read(Arc::new(tags));
    }
    (app, dir)
}

/// Every node the tree can show: the lists, Browse, every category and every value of one.
fn every_node(l: &mut Loaded) -> Vec<NodeId> {
    let mut v = vec![NodeId::All, NodeId::Browse];
    v.extend(l.library.playlists.iter().map(|p| NodeId::Playlist(p.id)));
    for c in Category::ALL {
        v.push(NodeId::Category(c));
        l.ensure_groups(c);
        v.extend(l.groups[&c].iter().map(|g| NodeId::Group(c, g.key.clone())));
    }
    v
}

fn names(l: &Loaded) -> Vec<(String, usize, bool)> {
    l.tree.iter().map(|n| (n.name.clone(), n.depth, n.expanded)).collect()
}

/// Opening and closing folders, Browse and its categories, in any order, and revealing any
/// node: opening and closing one again leaves the tree as it was; a node revealed shows in the
/// tree; every node shown is remembered as itself, titled as its path or category and value, and
/// lists its own tracks.
#[hegel::test(test_cases = 100)]
fn the_tree_opens_reveals_and_remembers_any_node(tc: TestCase) {
    let (mut app, _dir) = browsing(&tc);
    let l = app.loaded.as_mut().unwrap();
    let nodes = every_node(l);
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(12)) {
        let id = tc.draw(gs::sampled_from(&nodes[..]).print_as_debug());
        if tc.draw(gs::booleans()) {
            let before = names(l);
            l.toggle(&id);
            l.toggle(&id);
            assert_eq!(names(l), before, "{id:?} opened and closed again");
            l.toggle(&id);
        } else {
            l.reveal(&id);
            let empty = match &id {
                NodeId::Category(c) => l.groups[c].is_empty(),
                _ => false,
            };
            assert!(empty || l.tree_position(&id).is_some(), "{id:?} revealed, not in the tree");
        }
        let shown: Vec<(NodeId, usize, String)> =
            l.tree.iter().map(|n| (n.id.clone(), n.count, n.name.clone())).collect();
        for (id, count, name) in shown {
            let found = l.find_node(&l.save_node(&id)).expect("remembered");
            match &id {
                NodeId::Playlist(id) => assert_eq!(l.playlist_path(*id), l.save_node(&found).path()),
                other => assert_eq!(&found, other),
            }
            let title = l.node_title(&id);
            match &id {
                NodeId::Group(c, key) => {
                    let g = l.grouped(*c, key).unwrap().clone();
                    assert_eq!(title, format!("{} › {}", c.name(), g.label.trim()));
                    l.node = id.clone();
                    assert_eq!((l.base(), l.left_out()), (g.tracks.clone(), g.left_out));
                    assert_eq!(l.group(*c, key).map(|g| g.tracks.len()), Some(count));
                }
                NodeId::Playlist(id) => assert_eq!(title, l.playlist_path(*id).replace('/', " › ")),
                NodeId::Category(c) => assert_eq!(title, c.name()),
                NodeId::All | NodeId::Browse => assert_eq!(title, name),
            }
        }
    }
}

impl SavedNode {
    fn path(&self) -> String {
        match self {
            SavedNode::Playlist { path } => path.clone(),
            _ => String::new(),
        }
    }
}

/// Editing a line with keys: what's typed goes on the end, backspace takes the last character,
/// ctrl-u everything and ctrl-w the last word (with the spaces after it), other keys nothing; a
/// key says it changed the line just when it did.
#[hegel::test(test_cases = 300)]
fn a_line_edits_as_its_keys_say(tc: TestCase) {
    let mut text: String =
        tc.draw(gs::vecs(gs::sampled_from(&['a', 'b', ' ', '日', '😹'][..])).max_size(8)).into_iter().collect();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(10)) {
        let ctrl = tc.draw(gs::booleans());
        let code = match tc.draw(gs::integers::<u8>().max_value(5)) {
            0 => KeyCode::Backspace,
            1 => KeyCode::Char('u'),
            2 => KeyCode::Char('w'),
            3 => KeyCode::Left,
            _ => KeyCode::Char(tc.draw(gs::sampled_from(&['x', ' ', 'é'][..]))),
        };
        let before = text.clone();
        let changed = edit_line(&mut text, key_of(code, ctrl));
        assert_eq!(changed, text != before, "{code:?} (ctrl {ctrl}) on {before:?}: {text:?}");
        match (code, ctrl) {
            (KeyCode::Backspace, _) => {
                assert_eq!(text, before.chars().take(before.chars().count().saturating_sub(1)).collect::<String>())
            }
            (KeyCode::Char('u'), true) => assert!(text.is_empty()),
            (KeyCode::Char('w'), true) => {
                let cut = &before[text.len()..];
                assert!(
                    before.starts_with(&text) && (text.is_empty() || text.ends_with(' ')),
                    "{before:?} to {text:?}"
                );
                assert!(!cut.trim_end().contains(' '), "more than a word: {cut:?}");
            }
            (KeyCode::Char(c), false) => assert_eq!(text, format!("{before}{c}")),
            _ => assert_eq!(text, before),
        }
    }
}

/// A panic caught (on a worker, say) is said as what went wrong: what a library thread said
/// meanwhile when one did, else its own message, with where it was when the player's hook saw it;
/// work that doesn't panic hands back what it made.
#[hegel::test(test_cases = 50)]
fn a_caught_panic_says_what_went_wrong(tc: TestCase) {
    static HOOK: std::sync::Once = std::sync::Once::new();
    // The player's hook, this the UI's thread: the workers' panics are noted.
    HOOK.call_once(super::super::on_panic);
    let msg = tc.draw(gs::sampled_from(&["no sample rate", "index out of bounds", "日本"][..]));
    let elsewhere = tc.draw(gs::optional(gs::sampled_from(&["probe worker said", "a scoped thread"][..])));
    let panics = tc.draw(gs::booleans());
    let r = std::thread::spawn(move || {
        caught(|| {
            if let Some(e) = elsewhere {
                std::thread::scope(|s| s.spawn(|| note(e.to_string())).join().unwrap());
            }
            if panics {
                panic!("{msg}");
            }
            7
        })
    })
    .join()
    .unwrap();
    match (panics, r) {
        (false, r) => assert_eq!(r, Ok(7)),
        (true, Ok(_)) => panic!("a panic not caught"),
        (true, Err(said)) => {
            let rest = said.strip_prefix("something went wrong (a bug, please report it): ").expect(&said);
            match elsewhere {
                Some(e) => assert_eq!(rest, e),
                None => assert!(rest.starts_with(msg) && rest.contains("props.rs:"), "{rest}"),
            }
        }
    }
}

/// The help and the log scroll with the arrows, j and k, the page keys, space and the wheel (never
/// above the top, nor once drawn past the end), and any other key or a click puts them away; each
/// opens at its top. The log
/// keeps the last 200 messages.
#[hegel::test(test_cases = 50)]
fn the_help_and_the_log_scroll_and_close(tc: TestCase) {
    let (mut app, _dir) = blank(View::Library);
    let n = tc.draw(gs::integers::<usize>().max_value(230));
    for i in 0..n {
        app.toast(format!("message {i}"), if i % 7 == 0 { Level::Error } else { Level::Info });
    }
    assert_eq!(app.log.len(), n.min(LOG));
    assert!(app.log.back().is_none_or(|(m, ..)| *m == format!("message {}", n - 1)));
    // Every key and click in turn (in an order drawn for the case), on the help and the log
    // by turns.
    let keys = [
        (KeyCode::Up, Some(-1)),
        (KeyCode::Char('k'), Some(-1)),
        (KeyCode::Down, Some(1)),
        (KeyCode::Char('j'), Some(1)),
        (KeyCode::PageUp, Some(-10)),
        (KeyCode::PageDown, Some(10)),
        (KeyCode::Char(' '), Some(10)),
        (KeyCode::Esc, None),
        (KeyCode::Char('x'), None),
    ];
    let clicks = [
        (MouseEventKind::ScrollUp, Some(-3)),
        (MouseEventKind::ScrollDown, Some(3)),
        (MouseEventKind::Moved, Some(0)),
        (MouseEventKind::Down(MouseButton::Left), None),
        (MouseEventKind::Down(MouseButton::Right), None),
    ];
    let order = turns(&tc, (keys.len() + clicks.len()) as u8);
    let (mut scroll, mut next_help): (Option<(bool, u16)>, bool) = (None, tc.draw(gs::booleans()));
    for step in 0..tc.draw(gs::integers::<usize>().min_value(14).max_value(40)) {
        let Some((help, at)) = scroll else {
            press(&mut app, if next_help { "?" } else { "!" });
            scroll = Some((next_help, 0));
            next_help = !next_help;
            continue;
        };
        let k = order[step % order.len()] as usize;
        let moved = if k < keys.len() {
            let (code, by) = keys[k];
            app.on_key(key_of(code, false));
            by
        } else {
            let (kind, by) = clicks[k - keys.len()];
            mouse(&mut app, kind, (1, 1));
            by
        };
        scroll = moved.map(|by: i32| (help, (at as i32 + by).max(0) as u16));
        let shown = if help { (app.help, app.help_scroll) } else { (app.show_log, app.log_scroll) };
        assert_eq!(shown, (scroll.is_some(), scroll.map_or(shown.1, |s| s.1)), "help {help}");
        assert!(!(app.help && app.show_log));
        // Drawn, it scrolls no further than its last line; not past what it was.
        let (w, h) = (tc.draw(gs::integers::<u16>().min_value(20).max_value(160)), 24);
        assert_eq!(super::super::ui::screen(&mut app, w, h).lines().count(), h as usize);
        let drawn = if help { app.help_scroll } else { app.log_scroll };
        if let Some((help, at)) = scroll {
            assert!(drawn <= at, "drawn scrolled to {drawn}, past {at}");
            scroll = Some((help, drawn));
        }
    }
    // However many came before, past 200 the oldest go first.
    let more = tc.draw(gs::integers::<usize>().min_value(1).max_value(20));
    let total = n.max(LOG) + more;
    for i in n..total {
        app.toast(format!("message {i}"), Level::Info);
    }
    assert_eq!(app.log.len(), LOG);
    assert_eq!(app.log.front().map(|(m, ..)| m.clone()), Some(format!("message {}", total - LOG)));
}

/// Quitting (q, ctrl-c, ctrl-q, or the terminal going) quits at once with nothing under way,
/// stops a plan (which only reads), but waits for a write to finish, saying so once; esc then
/// stays. Once the write is done, the player quits.
#[hegel::test(test_cases = 100)]
fn quitting_waits_for_a_write_and_only_a_write(tc: TestCase) {
    let (mut app, _dir) = app_with(library());
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    let writing = tc.draw(gs::optional(gs::booleans()));
    app.job = writing.map(|committed| job(true, committed));
    let planning = writing.is_none() && tc.draw(gs::booleans());
    if planning {
        app.job = Some(job(false, false));
    }
    let (mut waiting, mut armed) = (false, 0);
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(8)) {
        match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 => press(&mut app, "q"),
            1 => app.on_key(key_of(KeyCode::Char('c'), true)),
            2 => app.on_key(key_of(KeyCode::Char('q'), true)),
            3 => app.request_quit(),
            _ => {
                press(&mut app, "\u{1b}");
                if waiting {
                    waiting = false;
                    assert_eq!(last_message(&app), "staying: not quitting after the write");
                    continue;
                }
                match (writing, planning) {
                    (Some(true), _) => assert_eq!(last_message(&app), "writing the drive: too late to stop"),
                    (Some(false), _) => assert!(app.job.as_ref().is_some_and(|j| j.cancel.is_cancelled())),
                    (None, true) => {
                        assert!(app.job.is_none() && last_message(&app) == "cancelled");
                        return;
                    }
                    (None, false) => {}
                }
                continue;
            }
        }
        if writing.is_none() {
            assert!(app.quit && app.job.is_none(), "nothing written: quits at once");
            return;
        }
        assert!(!app.quit && app.quit_after_write, "waits for the write");
        if !waiting {
            assert_eq!(last_message(&app), "still writing the drive: quitting when it's done (esc to stay)");
            armed += 1;
        }
        waiting = true;
        let said = app.log.iter().filter(|(m, ..)| m.starts_with("still writing the drive")).count();
        assert_eq!(said, armed, "said once each time it's asked to wait");
    }
    if writing.is_some() {
        app.request_quit();
        let tx = app.sender();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Applied { job: 99, root, result: Err(Error::Cancelled) });
        });
        app.finish_write();
        assert!(app.job.is_none() && app.quit, "quits once written");
    }
}

/// How many of the log's messages say `what`.
fn said(app: &App, what: &str) -> usize {
    app.log.iter().filter(|(m, ..)| m.contains(what)).count()
}

/// A snapshot of the library `Demo`, numbered `i`.
fn snapshot(i: usize) -> SnapshotInfo {
    use locallibrary::hash::Hash;
    SnapshotInfo {
        id: Hash::of(&[i as u8]),
        short_id: format!("snap{i:02}"),
        library_id: Uuid::nil(),
        parents: Vec::new(),
        created: chrono::Utc::now(),
        message: None,
        source: None,
        fingerprint: Hash::of(b"library"),
        summary: Default::default(),
        files: 0,
        bytes: 0,
        labels: Vec::new(),
        beside: false,
    }
}

/// Whatever the background sends back, in any order: what's for another read (an older load,
/// another library load, a lookup nobody asked for) changes nothing; the rest is shown or said.
/// A failing poll of the volumes is said once until one works; the open drive going away marks
/// what's shown as out of date, said once, and coming back has it read again; a file check lists
/// the missing files under Problems; My Tags, sets and an offer to replay land on their drive.
#[hegel::test(test_cases = 30, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn background_results_land_where_they_belong(tc: TestCase) {
    let (mut app, _dir) = app_with(library());
    let (root, mut generation, n) = {
        let l = app.loaded.as_ref().unwrap();
        (l.root().to_path_buf(), l.generation, l.library.tracks.len())
    };
    // The volumes are polled only when a step asks (the clock would otherwise decide).
    let later = Instant::now() + Duration::from_secs(3600);
    app.last_volume_poll = later;
    let (mut failing, mut failures, mut unplugs, mut stale) = (false, 0, 0, false);
    let record = LibraryRecord {
        id: Uuid::nil(),
        name: "Demo".into(),
        head: None,
        created: chrono::Utc::now(),
        updated: chrono::Utc::now(),
    };
    let kinds = turns(&tc, 19);
    for step in 0..tc.draw(gs::integers::<usize>().min_value(19).max_value(40)) {
        let ours = tc.draw(gs::booleans());
        let g = if ours { generation } else { generation + 1 };
        let toast = app.toast.clone().map(|t| t.0);
        match kinds[step % kinds.len()] {
            0 => {
                app.handle_msg(Msg::Failed(Work::Volumes, "the watcher fell over".into()));
                failures += usize::from(!failing);
                failing = true;
                assert!(!app.volumes_in_flight);
            }
            1 => {
                // Unplugged, the open drive comes back; otherwise nothing of it, another drive
                // gone, the open drive gone, or still here.
                let present = match tc.draw(gs::integers::<u8>().max_value(3)) {
                    _ if stale => Some((root.clone(), true)),
                    0 => None,
                    1 => Some((PathBuf::from("/Volumes/ELSEWHERE"), false)),
                    k => Some((root.clone(), k == 3)),
                };
                // A drive plugged in, now and then: the list is read again (not here: the tests'
                // player looks at no drives).
                let events = match tc.draw(gs::booleans()) {
                    true => vec![WatchEvent::Attached(locallibrary::watch::MountedDrive {
                        root: "/Volumes/NEW".into(),
                        id: None,
                        name: "NEW".into(),
                        has_export: true,
                    })],
                    false => Vec::new(),
                };
                app.handle_msg(Msg::Volumes(events, present.clone()));
                failing = false;
                match present {
                    Some((r, false)) if r == root && !stale => (stale, unplugs) = (true, unplugs + 1),
                    Some((r, true)) if r == root && stale => {
                        assert!(app.loading.is_some(), "back: read again");
                        // (An empty folder, read as a drive: it says why it can't be.)
                        wait(&mut app, |a| a.loading.is_none());
                        assert!(!last_message(&app).is_empty());
                    }
                    _ => {}
                }
            }
            2 => {
                app.drives_in_flight = true;
                app.handle_msg(Msg::Failed(Work::Drives, "the list fell over".into()));
                assert!(!app.drives_in_flight && last_message(&app) == "the list fell over");
            }
            3 => {
                let i = tc.draw(gs::integers::<usize>().max_value(n - 1));
                app.details_asked = Some((g, i));
                app.handle_msg(Msg::Failed(Work::Details(g, i), "it fell over".into()));
                assert_eq!(app.details_asked, None);
                let selected = app.selected_track() == Some(i);
                assert_eq!(app.details_lookup_failed(), ours && selected);
            }
            4 => {
                let ok = tc.draw(gs::booleans());
                app.status_loading = true;
                app.handle_msg(Msg::Status(if ok { Ok(Default::default()) } else { Err("no store yet".into()) }));
                assert!(!app.status_loading && app.status.as_ref().is_some_and(|s| s.is_ok() == ok));
            }
            5 => {
                let before = app.history_state.selected();
                let r = match tc.draw(gs::optional(gs::integers::<usize>().max_value(4))) {
                    Some(k) => Ok(vec![(record.clone(), (0..k).map(snapshot).collect::<Vec<_>>())]),
                    None => Err("no store yet".to_string()),
                };
                let rows = r.as_ref().map_or(0, |l| l[0].1.len());
                app.handle_msg(Msg::History(r));
                let want = (rows > 0).then(|| before.unwrap_or(0).min(rows - 1));
                assert_eq!(app.history_state.selected(), want);
            }
            6 => {
                let len = if tc.draw(gs::booleans()) { n } else { n + 1 };
                let missing: Vec<bool> = (0..len).map(|_| tc.draw(gs::booleans())).collect();
                let before = app.loaded.as_ref().unwrap().problems.missing.clone();
                app.handle_msg(Msg::Missing(g, missing.clone()));
                let l = app.loaded.as_mut().unwrap();
                let applies = ours && len == n;
                assert_eq!(l.problems.missing, if applies { Some(missing.clone()) } else { before });
                if applies {
                    let listed = l.group(Category::Problems, "missing").map(|g| g.tracks.clone()).unwrap_or_default();
                    assert_eq!(listed, (0..n).filter(|&i| missing[i]).collect::<Vec<_>>());
                }
            }
            7 => {
                let r =
                    if tc.draw(gs::booleans()) { Ok(FxHashMap::default()) } else { Err("no tags here".to_string()) };
                let ok = r.is_ok();
                app.handle_msg(Msg::MyTags(g, r));
                let tags = &app.loaded.as_ref().unwrap().my_tags;
                if ours {
                    assert!(if ok { matches!(tags, MyTags::Read(_)) } else { matches!(tags, MyTags::Failed(_)) });
                }
            }
            8 => {
                let r = if tc.draw(gs::booleans()) { Ok(Vec::new()) } else { Err("a broken set book".to_string()) };
                let ok = r.is_ok();
                app.handle_msg(Msg::Sets(g, r));
                if ours {
                    assert!(app.loaded.as_ref().unwrap().sets.is_some());
                    if !ok {
                        assert_eq!(last_message(&app), "can't read the sets: a broken set book");
                    }
                }
            }
            9 => {
                let generation = app.loaded.as_ref().unwrap().generation;
                app.handle_msg(Msg::Library(app.load + 1, PathBuf::from("/Volumes/GONE"), Err("can't read it".into())));
                assert_eq!(app.loaded.as_ref().unwrap().generation, generation, "an older read, dropped");
                assert_eq!(app.toast.clone().map(|t| t.0), toast);
            }
            10 => {
                let edits = tc.draw(gs::integers::<usize>().max_value(2));
                let before = app.replay_offer.as_ref().map(|o| o.edits);
                app.handle_msg(Msg::Replay(g, edits));
                let offered = app.replay_offer.as_ref().map(|o| o.edits);
                assert_eq!(offered, if ours { (edits > 0).then_some(edits) } else { before });
            }
            11 => {
                let level = if tc.draw(gs::booleans()) { Level::Info } else { Level::Error };
                app.handle_msg(Msg::Note("a report was written".into(), level));
                assert_eq!(last_message(&app), "a report was written");
            }
            12 => {
                let i = tc.draw(gs::integers::<usize>().max_value(n - 1));
                let asked = tc.draw(gs::booleans());
                app.details_asked = asked.then_some((g, i));
                let l = app.loaded.as_ref().unwrap();
                let mix =
                    |index| Mix { index, key: Camelot::parse("8A").unwrap(), relation: Relation::Same, tempo_gap: 0.0 };
                let matches =
                    Matches { library: l.library.clone(), root: root.clone(), items: vec![mix(n - 1), mix(0)] };
                let had = app.details.as_ref().map(|d| d.index);
                app.handle_msg(Msg::Details(Box::new(Details {
                    generation: g,
                    index: i,
                    matches,
                    playlists: Vec::new(),
                })));
                assert_eq!(app.details.as_ref().map(|d| d.index), if asked { Some(i) } else { had });
                // The match picked, of the selected track's (when they're its).
                if !asked {
                    let l = app.loaded.as_ref().unwrap();
                    let matches =
                        Matches { library: l.library.clone(), root: root.clone(), items: vec![mix(n - 1), mix(0)] };
                    app.details_asked = Some((g, i));
                    app.handle_msg(Msg::Details(Box::new(Details {
                        generation: g,
                        index: i,
                        matches,
                        playlists: Vec::new(),
                    })));
                }
                app.track_state.select(app.visible.iter().position(|&v| v == i));
                app.details_state.select(Some(1));
                let current = app.details_current();
                assert_eq!(app.selected_match(), current.then_some(0), "the match picked");
            }
            14 => {
                // The volumes polled: the open drive (an empty folder) reads as gone.
                app.last_volume_poll = Instant::now() - Duration::from_secs(2);
                app.volumes_in_flight = false;
                app.tick();
                assert!(app.volumes_in_flight, "polled");
                app.last_volume_poll = later;
                wait(&mut app, |a| !a.volumes_in_flight);
                failing = false;
                if !stale {
                    (stale, unplugs) = (true, unplugs + 1);
                }
            }
            15 => {
                // An info message goes once it's been read; an error stays.
                let error = tc.draw(gs::booleans());
                app.toast("a passing note", if error { Level::Error } else { Level::Info });
                if let Some(t) = &mut app.toast {
                    t.2 = Instant::now() - Duration::from_secs(10);
                }
                app.tick();
                assert_eq!(app.toast.is_some(), error);
            }
            16 => {
                // The drive read again: what's open in the tree and the track selected stay.
                let l = app.loaded.as_mut().unwrap();
                l.toggle(&NodeId::Playlist(1));
                let lib = (*l.library).clone();
                let rows = app.visible.len();
                app.track_state.select((rows > 1).then_some(1));
                let (selected, tree) = (app.selected_track(), app.loaded.as_ref().unwrap().tree.len());
                let fresh = Loaded::prepare(Device::open(&root).unwrap(), lib);
                app.handle_msg(Msg::Library(app.load, root.clone(), Ok(Box::new(fresh))));
                let l = app.loaded.as_ref().unwrap();
                assert_eq!((app.selected_track(), l.tree.len()), (selected, tree), "the view kept");
                (stale, generation) = (false, l.generation);
            }
            17 => {
                // Enter on a track of a drive that's gone plays nothing.
                app.focus = Focus::Tracks;
                app.view = View::Library;
                press(&mut app, "\n");
                if stale {
                    assert_eq!(last_message(&app), "the drive was unplugged");
                }
                assert!(app.now.is_none());
            }
            18 => {
                // Asked for while a read of the drives is under way, another waits for it.
                app.offline = false;
                app.drives_in_flight = true;
                app.refresh_drives();
                app.offline = true;
                assert!(app.drives_again, "read again once that one's done");
                app.handle_msg(Msg::Failed(Work::Drives, "the list fell over".into()));
                assert!(!app.drives_again && !app.drives_in_flight);
            }
            _ => {
                let field = tc.draw(gs::sampled_from(&browse::SORT_FIELDS[..]).print_as_debug());
                let had = app.loaded.as_ref().unwrap().ranks.iter().any(|(f, _)| *f == field);
                let index = Index { ranks: vec![(field, vec![7; n])], groups: FxHashMap::default() };
                app.handle_msg(Msg::Indexed(g, Box::new(index)));
                let l = app.loaded.as_ref().unwrap();
                let kept = l.ranks.iter().find(|(f, _)| *f == field).map(|(_, r)| r[0]);
                assert_eq!(kept.is_some(), had || ours);
            }
        }
        assert_eq!(said(&app, "the watcher fell over"), failures, "a failing poll said once until one works");
        assert_eq!(said(&app, "was unplugged: what's shown may be out of date"), unplugs, "unplugging said once");
        assert_eq!(app.loaded.as_ref().unwrap().stale, stale);
        let (w, h) = (tc.draw(gs::integers::<u16>().min_value(1).max_value(160)), 30);
        assert_eq!(super::super::ui::screen(&mut app, w, h).lines().count(), h as usize);
    }
}

/// What's remembered is saved on the way out, and as it changes (only then); the next run on the
/// same drive carries on with the queue where it was: the tracks around the one it was on, in
/// play order, from that one, at the time it was at (to 15 seconds).
#[hegel::test(test_cases = 30)]
fn the_next_run_carries_on_with_the_queue(tc: TestCase) {
    let mut lib = library();
    lib.tracks = any_tracks(&tc, 6);
    let (mut app, _dir) = app_with(lib);
    let store = tempfile::tempdir().unwrap();
    app.store_root = store.path().join("store");
    let (library, root) = {
        let l = app.loaded.as_ref().unwrap();
        (l.library.clone(), l.root().to_path_buf())
    };
    let items: Vec<usize> =
        tc.draw(gs::vecs(gs::integers::<usize>().max_value(library.tracks.len().saturating_sub(1))).max_size(6));
    let items: Vec<usize> = items.into_iter().filter(|&i| i < library.tracks.len()).collect();
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.rotate_left(tc.draw(gs::integers::<usize>().max_value(items.len())).min(items.len()));
    let pos = tc.draw(gs::integers::<usize>().max_value(items.len().saturating_sub(1)));
    let at = Duration::from_millis(tc.draw(gs::integers::<u64>().max_value(400_000)));
    app.queue = Queue {
        library: library.clone(),
        items: items.clone(),
        root: root.clone(),
        drive: "TEST".into(),
        order: order.clone(),
        pos,
    };
    app.resume = Some(at);
    app.save_state();
    let read = state::load(&app.store_root);
    assert_eq!(read, app.saved_state(), "saved as it is");

    // Unchanged, nothing is written; changed, it is.
    let file = store.path().join("store-player/tui.json");
    std::fs::remove_file(&file).unwrap();
    app.last_save_check = Instant::now() - Duration::from_secs(3);
    app.tick();
    assert!(!file.exists(), "saved again with nothing changed");
    press(&mut app, "+");
    app.last_save_check = Instant::now() - Duration::from_secs(3);
    app.tick();
    assert_eq!(state::load(&app.store_root).volume, Some(app.volume));

    let mut next = App::blank(app.store_root.clone(), None);
    let mut saved = state::load(&app.store_root);
    // The next run: as it was left, already playing something else, or with the drive's tracks
    // gone since; with the My Tags column shown, or not.
    let (how, tags) = (tc.draw(gs::integers::<u8>().max_value(2)), tc.draw(gs::booleans()));
    if how == 2
        && let Some(q) = &mut saved.queue
    {
        q.tracks.iter_mut().for_each(|t| *t = format!("{t}.gone"));
    }
    saved.tags_column = tags;
    app.vis = tc.draw(gs::sampled_from(&[Vis::Bars, Vis::Mirror, Vis::Scope, Vis::Deck][..]).print_as_debug());
    app.save_state();
    assert_eq!(state::load(&app.store_root).vis.as_deref(), Some(app.vis.name()), "the visualiser kept");
    // Showing a set last time, or not.
    let a_set = tc.draw(gs::booleans());
    if a_set {
        saved.node = Some(SavedNode::Group { category: Category::Sets, key: "set1".into() });
    }
    next.apply_saved(&saved, true);
    // An offer to replay edits on another drive goes; one put away this run isn't looked for.
    next.replay_offer = Some(ReplayOffer { root: PathBuf::from("/Volumes/OTHER"), drive: "OTHER".into(), edits: 2 });
    let dismissed = tc.draw(gs::booleans());
    if dismissed {
        next.replay_dismissed.insert(root.clone());
    }
    if how == 1 {
        next.queue = Queue {
            library: library.clone(),
            items: vec![0],
            root: PathBuf::from("/Volumes/OTHER"),
            drive: "OTHER".into(),
            order: vec![0],
            pos: 0,
        };
    }
    next.install(root.clone(), Loaded::prepare(Device::open(&root).unwrap(), (*library).clone()));
    assert_eq!(matches!(next.loaded.as_ref().unwrap().my_tags, MyTags::Reading), tags, "the column's tags read");
    assert!(next.replay_offer.is_none(), "the other drive's offer went");
    let generation = next.loaded.as_ref().unwrap().generation;
    assert_eq!(next.replay_checked == generation, dismissed, "not looked for once put away");
    if a_set {
        // The set shown once the sets are read, the tree's cursor on it.
        let l = next.loaded.as_ref().unwrap();
        assert!(l.reading_set(), "reading the sets");
        let lib = l.library.clone();
        let entries = lib
            .tracks
            .iter()
            .take(2)
            .map(|t| locallibrary::sets::SetEntry::from_track(t, locallibrary::sets::EntryOrigin::Session, None))
            .collect();
        next.handle_msg(Msg::Sets(generation, Ok(vec![set("set1", "2026-10-03", Some("Friday"), None, entries)])));
        let l = next.loaded.as_ref().unwrap();
        assert_eq!(next.playlist_state.selected(), l.tree_position(&l.node), "on the set in the tree");
    }
    let played = |q: &Queue| q.order.iter().map(|&i| q.track(i).file_path.clone()).collect::<Vec<_>>();
    if how == 1 {
        assert_eq!(next.queue.root, PathBuf::from("/Volumes/OTHER"), "what's playing stays");
        return;
    }
    if items.is_empty() || how == 2 {
        assert!(next.queue.is_empty() && next.resumable().is_none());
        return;
    }
    // Played in order (with the same file twice, each place keeps its own).
    assert_eq!(played(&next.queue), played(&app.queue), "the queue, in play order");
    let (track, from) = next.resumable().expect("ready to carry on");
    assert_eq!(track.file_path, app.queue.track(order[pos]).file_path);
    assert_eq!(from.as_millis() as u64, at.as_millis() as u64 / 15_000 * 15_000);
}

/// The keys of playing through the queue, as the library takes them.
const PLAY_KEYS: &[&str] =
    &[" ", " ", "n", "n", "p", "x", "s", "r", "S", "A", "c", "f", "\n", "j", "k", "o", "<right>"];

/// Playing a queue of half-second tracks (at two sample rates; some with no audio in them, some
/// whose file has gone) with the player's keys as time passes, while its output goes away (and
/// space plays on through another), glitches, or a track breaks as it plays; with matches to
/// queue next full screen, the crate to play, a recorded set played on its own, and the drive let
/// go of: the queue stays whole and in play order, what plays is the queue's (or the set), and
/// each says what it does.
#[hegel::test(test_cases = 12, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_queue_plays_on_through_what_goes_wrong(tc: TestCase) {
    let n = tc.draw(gs::integers::<usize>().min_value(2).max_value(4));
    let files: Vec<(u32, usize)> = (0..n)
        .map(|_| {
            (tc.draw(gs::sampled_from(&[8000u32, 16000][..])), tc.draw(gs::sampled_from(&[4000usize, 4000, 0][..])))
        })
        .collect();
    let (_dir, mut app, mut out) = playing_files(&files);
    for i in 1..=n {
        if tc.draw(gs::integers::<u8>().max_value(5)) == 0 {
            let _ = std::fs::remove_file(app.queue.root.join(format!("Contents/{i}.wav")));
        }
    }
    app.outputs = super::super::audio::tests::OUTPUTS;
    // (The drive is a folder of files, which a poll of the volumes would take for unplugged.)
    app.last_volume_poll = Instant::now() + Duration::from_secs(3600);
    app.deck.output = tc.draw(gs::sampled_from(&[None, Some("Phones"), Some("Old Speakers")][..])).map(String::from);
    let root = app.queue.root.clone();
    // The plays broken on purpose.
    let mut broke: Vec<u64> = Vec::new();
    // Whether `out` is the playing output's (an output opened again plays to nowhere).
    let mut fed = true;
    let kinds = turns(&tc, 16);
    for step in 0..tc.draw(gs::integers::<usize>().min_value(16).max_value(40)) {
        let (len, pos) = (app.queue.len(), app.queue.pos);
        match kinds[step % kinds.len()] {
            15 => {
                // Holding n while a track is being opened: only the last one asked for is opened.
                if !app.playing_queue() {
                    continue;
                }
                let idle = app.opener_idle();
                let busy = idle.lock().unwrap();
                while app.rx.try_recv().is_ok() {}
                for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
                    press(&mut app, "n");
                }
                drop(busy);
                let Some(last) = app.now.as_ref().map(|n| n.id) else { continue };
                let t = Instant::now();
                let mut opened = Vec::new();
                while !opened.contains(&last) && t.elapsed() < Duration::from_secs(5) {
                    if let Ok(msg) = app.rx.recv_timeout(Duration::from_millis(20)) {
                        if let Msg::Opened(id, _) = &msg {
                            opened.push(*id);
                        }
                        app.handle_msg(msg);
                    }
                }
                assert!(
                    opened.iter().all(|id| *id == last || app.upcoming.as_ref().is_some_and(|u| u.now.id == *id)),
                    "{opened:?} opened, not only {last}"
                );
            }
            14 => {
                // The track plays out, the next opened ahead behind it: the player goes straight
                // on to that one.
                let ahead = |a: &App| a.upcoming.as_ref().is_some_and(|u| u.queued || u.file.is_some());
                // Played afresh from its start, through an output whose samples are taken here.
                let (mut audio, fresh) = Audio::offline(8000);
                audio.set_volume(1.0);
                (app.audio, out) = (Some(audio), fresh);
                fed = true;
                // One track at a time or on through the queue, round again or not; the first with
                // its artwork on the drive.
                app.deck.single = tc.draw(gs::booleans());
                app.repeat = tc.draw(gs::sampled_from(&[Repeat::Off, Repeat::All, Repeat::One][..]).print_as_debug());
                let mut lib = (*app.queue.library).clone();
                let art = app.queue.root.join("PIONEER/Artwork/1.png");
                std::fs::create_dir_all(art.parent().unwrap()).unwrap();
                let mut png = Vec::new();
                image::RgbImage::from_pixel(4, 4, image::Rgb([200, 40, 90]))
                    .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                    .unwrap();
                std::fs::write(&art, png).unwrap();
                // On through the queue, the first said (by rekordbox) to be ten minutes long, or not.
                let far = !app.deck.single && tc.draw(gs::booleans());
                if let Some(t) = lib.tracks.first_mut() {
                    t.artwork_path = Some("/PIONEER/Artwork/1.png".into());
                    t.duration_secs = if far { 600 } else { 0 };
                }
                let items: Vec<usize> = (0..lib.tracks.len()).collect();
                let at = app.queue.root.clone();
                app.play_tracks(Arc::new(lib), items, at, 0);
                if app.deck.single {
                    // Played out, it stops with the next one ready for space, from its start.
                    let t = Instant::now();
                    while app.now.as_ref().is_some_and(|n| n.opening || n.loading)
                        && t.elapsed() < Duration::from_secs(5)
                    {
                        app.tick();
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    let Some(now) = app.now.as_ref().filter(|n| n.track.id == 1 && files[0].1 > 0) else { continue };
                    assert!(now.art.is_some() || now.loading, "its artwork read from the drive");
                    let next = app.next_pos(true);
                    (&mut out).take(4100).for_each(drop);
                    app.tick();
                    if let Some(p) = next.filter(|_| app.now.is_none() && app.resume.is_some()) {
                        assert_eq!((app.queue.pos, app.resume), (p, Some(Duration::ZERO)), "ready for space");
                        // From wherever it was left (the last run's, say).
                        app.resume = Some(Duration::from_millis(100));
                        press(&mut app, " ");
                        assert!(app.now.is_some(), "space plays it");
                        assert_eq!(app.pending_seek, Some(Duration::from_millis(100)), "from where it was");
                    }
                    continue;
                }
                if !app.playing_queue() || app.next_pos(true).is_none() {
                    continue;
                }
                if far {
                    // Ten minutes from its end, nothing is opened ahead yet.
                    let t = Instant::now();
                    while app.now.as_ref().is_some_and(|n| n.opening) && t.elapsed() < Duration::from_secs(5) {
                        app.tick();
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    if app.now.as_ref().is_some_and(|n| n.track.id == 1 && !n.opening) {
                        app.tick();
                        assert!(app.upcoming.is_none(), "opened ahead ten minutes out");
                    }
                    continue;
                }
                let t = Instant::now();
                while app.now.is_some() && !ahead(&app) && t.elapsed() < Duration::from_secs(5) {
                    app.tick();
                    std::thread::sleep(Duration::from_millis(1));
                }
                if !ahead(&app) {
                    continue;
                }
                // A cue just ahead flashes as the playhead crosses it.
                if let Some(now) = app.now.as_ref().filter(|n| !n.opening) {
                    let ms = app.position().as_millis() as u32 + 20;
                    let analysis = TrackAnalysis { cues: vec![cue(Some(0), ms, None)], ..Default::default() };
                    let id = now.id;
                    app.handle_msg(Msg::TrackInfo(id, Box::new(TrackInfo { analysis, ..Default::default() })));
                    app.last_pos_ms = app.position_ms();
                    (&mut out).take(400).for_each(drop);
                    app.tick();
                }
                if !ahead(&app) {
                    continue;
                }
                // Both with audio in them.
                let up = app.upcoming.as_ref().unwrap();
                let (next, id) = (up.pos, up.now.id);
                let audible = |t: &Track| {
                    let i = t.id as usize - 1;
                    files[i].1 > 0 && app.queue.root.join(format!("Contents/{}.wav", t.id)).is_file()
                };
                let current = app.now.as_ref().unwrap().track.clone();
                let playing = app.now.as_ref().map(|n| n.id);
                let fine = audible(&current)
                    && audible(&up.now.track)
                    && app.is_playing()
                    && !playing.is_some_and(|p| broke.contains(&p));
                let left = Duration::from_millis(500).saturating_sub(app.position());
                let was = format!(
                    "pos {} of {:?}, at {:?}, upcoming queued {}, file {}, repeat {:?}, shuffle {}, opening {}",
                    app.queue.pos,
                    app.queue.order,
                    app.position(),
                    up.queued,
                    up.file.is_some(),
                    app.repeat,
                    app.shuffle,
                    app.now.as_ref().unwrap().opening
                );
                (&mut out).take((left.as_secs_f64() * 8000.0) as usize + 20).for_each(drop);
                app.tick();
                if fine {
                    let said: Vec<&str> = app.log.iter().rev().take(3).map(|(m, ..)| m.as_str()).collect();
                    assert_eq!(
                        (app.queue.pos, app.now.as_ref().map(|n| n.id)),
                        (next, Some(id)),
                        "on to the one opened ahead: {was}; {said:?}"
                    );
                }
            }
            // Time passes through the output the queue started on.
            0 | 1 => {
                let k = tc.draw(gs::sampled_from(&[300usize, 2100, 4100][..]));
                (&mut out).take(k).for_each(drop);
                std::thread::sleep(Duration::from_millis(2));
            }
            2 => {
                // (Once open: a track still opening may yet say it won't.)
                let Some(a) = app.audio.as_ref().filter(|_| app.now.as_ref().is_none_or(|n| !n.opening)) else {
                    continue;
                };
                a.stream_failed("the device is gone", true);
                fed = false;
                let (at, was) = (app.playback().0, app.now.as_ref().map(|n| n.id));
                app.tick();
                assert!(app.audio.is_none() && !app.is_playing(), "let go of");
                assert_eq!(app.now.as_ref().map(|n| n.id), was, "the track waits");
                if was.is_some() && !app.now.as_ref().unwrap().opening {
                    assert_eq!(Some(app.position()), at, "where it was");
                }
                assert!(said(&app, "the device is gone") > 0);
                // With no output to open now, the next can't play: it says why, and stops.
                if was.is_some() && tc.draw(gs::booleans()) {
                    app.outputs = super::super::audio::Outputs {
                        list: || (Vec::new(), None),
                        open: |_| anyhow::bail!("no outputs"),
                    };
                    // (At the end of the queue, with nothing to go round to, n just stops.)
                    let next = app.queue.pos + 1 < app.queue.order.len() || app.repeat != Repeat::Off;
                    let before = said(&app, "can't play: no outputs");
                    press(&mut app, "n");
                    assert!(app.now.is_none() && app.audio.is_none(), "stopped");
                    assert_eq!(said(&app, "can't play: no outputs") > before, next);
                    app.outputs = super::super::audio::tests::OUTPUTS;
                    continue;
                }
                // Space plays on through the output there is now, from where it was.
                if was.is_some() && tc.draw(gs::booleans()) {
                    press(&mut app, " ");
                    assert!(app.audio.is_some(), "an output opened again");
                    // Only an output not in the list falls back ("Phones" is there).
                    let fell_back = app.deck.output.as_deref() == Some("Old Speakers");
                    assert_eq!(said(&app, "playing through the default output") > 0, fell_back);
                }
            }
            3 => {
                let Some(a) = &app.audio else { continue };
                a.stream_failed("underrun", false);
                let before = said(&app, "audio: underrun");
                app.tick();
                assert!(app.audio.is_some() && said(&app, "audio: underrun") == before + 1, "a glitch is only said");
            }
            4 => {
                let Some(a) = &app.audio else { continue };
                a.break_playing("a bad frame");
                broke.extend(app.now.as_ref().map(|n| n.id));
            }
            5 => {
                // The playing track's matches arrive: the best is picked to play next.
                let Some(now) = &app.now else { continue };
                let k = tc.draw(gs::integers::<usize>().min_value(1).max_value(n));
                let items = (0..k)
                    .map(|i| Mix {
                        index: i,
                        key: Camelot::parse("8A").unwrap(),
                        relation: Relation::Same,
                        tempo_gap: 0.0,
                    })
                    .collect();
                let matches = Matches { library: app.queue.library.clone(), root: root.clone(), items };
                // With cues near the start, which the playhead flashes as it crosses.
                let analysis =
                    TrackAnalysis { cues: vec![cue(Some(0), 60, None), cue(None, 200, None)], ..Default::default() };
                let info = TrackInfo { matches: Some(matches), analysis, ..Default::default() };
                app.handle_msg(Msg::TrackInfo(now.id, Box::new(info)));
                assert_eq!(app.next_pick.selected(), Some(0), "the best match picked");
            }
            6 => {
                app.let_go_of(&root);
                assert!(app.now.is_none() && app.upcoming.is_none(), "nothing left open on the drive");
            }
            7 => {
                let set = root.join("Contents/1.wav");
                let mut t = track(9, "DJ", "Set");
                t.file_path = set.display().to_string();
                if app.play_file(&set, t, Duration::from_millis(tc.draw(gs::integers::<u64>().max_value(300)))) {
                    assert_eq!(app.now.as_ref().map(|n| n.drive.as_str()), Some("recorded set"));
                    assert!(!app.playing_queue());
                    // Played to its end (through the output it started on), it stops.
                    let t = Instant::now();
                    while app.now.as_ref().is_some_and(|n| n.opening) && t.elapsed() < Duration::from_secs(5) {
                        app.tick();
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    if fed && files[0].1 > 0 && app.is_playing() && app.now.is_some() {
                        (&mut out).take(9000).for_each(drop);
                        app.tick();
                        assert!(app.now.is_none(), "a set stops at its end");
                    }
                }
                assert_eq!((app.queue.len(), app.queue.pos), (len, pos), "the queue stays, for afterwards");
            }
            8 => {
                // The crate, played from one of its tracks: those on the drive, from that one.
                let paths: Vec<String> = app.queue.library.tracks.iter().map(|t| t.file_path.clone()).collect();
                app.crate_list.clear();
                app.add_to_crate(&root, &paths);
                let elsewhere = tc.draw(gs::booleans());
                let before = said(&app, "playing the crate's tracks on ");
                if elsewhere {
                    app.crate_list.add(Path::new("/Volumes/OTHER"), "OTHER", [track(7, "DJ", "Elsewhere")]);
                }
                let i = tc.draw(gs::integers::<usize>().max_value(paths.len() - 1));
                (app.show_crate, app.focus, app.fullscreen) = (true, Focus::Crate, false);
                (app.modal, app.picking, app.searching) = (None, None, false);
                app.crate_state.select(Some(i));
                let was = app.now.as_ref().map(|n| n.id);
                press(&mut app, "\n");
                app.focus = Focus::Tracks;
                if let Some(now) = app.now.as_ref().filter(|n| Some(n.id) != was) {
                    assert_eq!(now.track.file_path, paths[i], "the crate plays from the one picked");
                }
                // With tracks of another drive in it, it says which drive's it plays.
                assert_eq!(said(&app, "playing the crate's tracks on "), before + usize::from(elsewhere));
            }
            9 => wait(&mut app, |a| a.now.as_ref().is_none_or(|n| !n.opening)),
            10 => {
                // Full screen, the pick among the matches (clicked twice, or enter) plays next.
                app.fullscreen = true;
                // A key or click first catches up with what played (`follow_queue`): a track that
                // ended since goes on to the next, with matches of its own, so catch up before
                // giving the playing one its matches.
                app.tick();
                if let Some(now) = app.now.as_ref().filter(|n| n.matches.as_ref().is_none_or(|m| m.items.is_empty())) {
                    let items = vec![Mix {
                        index: n - 1,
                        key: Camelot::parse("8A").unwrap(),
                        relation: Relation::Same,
                        tempo_gap: 0.0,
                    }];
                    let matches = Matches { library: app.queue.library.clone(), root: root.clone(), items };
                    let id = now.id;
                    app.handle_msg(Msg::TrackInfo(
                        id,
                        Box::new(TrackInfo { matches: Some(matches), ..Default::default() }),
                    ));
                }
                let matched = app.now.as_ref().and_then(|n| n.matches.as_ref()).map_or(0, |m| m.items.len());
                let queued = app.playing_queue() && app.next_pick.selected().is_some_and(|p| p < matched);
                if tc.draw(gs::booleans()) {
                    press(&mut app, "\n");
                } else {
                    super::super::ui::screen(&mut app, 160, 40);
                    let r = app.areas.matches;
                    if r.is_empty() {
                        continue;
                    }
                    let at = (
                        r.x + 1,
                        r.y + app.next_pick.selected().unwrap_or(0).saturating_sub(app.next_pick.offset()) as u16,
                    );
                    mouse(&mut app, LEFT, at);
                    mouse(&mut app, LEFT, at);
                }
                if queued {
                    assert_eq!(app.queue.len(), len + 1, "the pick queued");
                    assert!(last_message(&app).starts_with("next: "));
                    assert_eq!(app.queue.order[app.queue.pos + 1], app.queue.len() - 1, "to play next");
                }
            }
            11 => {
                // Found from a search that hides it: the list goes back to all its tracks.
                press(&mut app, "/zzz\n");
                let playing = app.now.as_ref().filter(|_| app.playing_queue()).map(|n| n.track.file_path.clone());
                press(&mut app, "o");
                if let Some(p) = playing {
                    let l = app.loaded.as_ref().unwrap();
                    assert!(app.query.is_empty(), "the search cleared to show it");
                    assert_eq!(app.selected_track().map(|i| l.library.tracks[i].file_path.clone()), Some(p));
                }
                app.view = View::Library;
            }
            _ => {
                let key = tc.draw(gs::sampled_from(PLAY_KEYS));
                let picked = app.next_pick.selected();
                let matched = app.now.as_ref().and_then(|n| n.matches.as_ref()).map_or(0, |m| m.items.len());
                let queued = app.fullscreen && app.playing_queue() && picked.is_some_and(|p| p < matched);
                let set = app.now.is_some() && !app.playing_queue();
                if key == "<right>" {
                    app.on_key(key_of(KeyCode::Right, false));
                } else {
                    press(&mut app, key);
                }
                match key {
                    "x" => assert!(app.now.is_none(), "x stops"),
                    "\n" if queued => {
                        assert_eq!(app.queue.len(), len + 1, "full screen, enter queues the pick");
                        assert!(last_message(&app).starts_with("next: "));
                        let next = app.queue.order[app.queue.pos + 1];
                        assert_eq!(next, app.queue.len() - 1, "to play next");
                    }
                    "o" if set => assert_eq!(app.view, View::Mixes, "a set is shown with the mixes"),
                    "o" if app.now.is_some() && app.loaded.as_ref().is_some_and(|l| l.root() == root) => {
                        let now = app.now.as_ref().unwrap().track.file_path.clone();
                        let l = app.loaded.as_ref().unwrap();
                        assert_eq!(app.selected_track().map(|i| l.library.tracks[i].file_path.clone()), Some(now));
                    }
                    _ => {}
                }
                app.view = View::Library;
            }
        }
        app.tick();
        let q = &app.queue;
        let mut order = q.order.clone();
        order.sort_unstable();
        assert_eq!(order, (0..q.len()).collect::<Vec<_>>(), "the play order is the queue's tracks");
        assert!(q.is_empty() || q.pos < q.len());
        if let Some(now) = &app.now {
            let theirs = app.playing_queue() || now.drive == "recorded set" || app.queue.root != now.root;
            assert!(theirs, "{} plays, not the queue's", now.track.title);
        }
        if app.audio.is_none() {
            assert!(!app.is_playing());
        }
        let (w, h) = (tc.draw(gs::integers::<u16>().min_value(1).max_value(200)), 40);
        assert_eq!(super::super::ui::screen(&mut app, w, h).lines().count(), h as usize);
    }
}

/// What esc puts away, in the order it does.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Away {
    Waiting,
    FullScreen,
    Job,
    Errors,
    Replay,
    Search,
    Marks,
}

/// Esc puts away one thing at a time, in order: waiting to quit after a write, full screen, the
/// job under way, the errors shown, the offer to replay edits, what's typed in the search; then,
/// pressed twice, the marks (the first press says what the second does).
#[hegel::test(test_cases = 200)]
fn esc_puts_away_one_thing_at_a_time(tc: TestCase) {
    let (mut app, _dir) = app_with(library());
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    let mut left: Vec<Away> = Vec::new();
    let mut on = |what: Away| {
        let yes = tc.draw(gs::booleans());
        if yes {
            left.push(what);
        }
        yes
    };
    let waiting = on(Away::Waiting);
    app.fullscreen = on(Away::FullScreen);
    if on(Away::Job) {
        app.job = Some(job(false, false));
    }
    if waiting {
        // Waiting on a write that's too late to stop: esc after only says so.
        app.job = Some(job(true, true));
        app.quit_after_write = true;
    }
    if on(Away::Errors) {
        app.toast("it broke", Level::Error);
    }
    if on(Away::Replay) {
        app.replay_offer = Some(ReplayOffer { root: root.clone(), drive: "TEST".into(), edits: 2 });
    }
    let all = app.visible.len();
    if on(Away::Search) {
        app.query = "daft".into();
        app.refresh_visible();
    }
    let (marks, lists) = (tc.draw(gs::integers::<usize>().max_value(2)), tc.draw(gs::booleans()));
    let paths: Vec<String> = app.loaded.as_ref().unwrap().library.tracks.iter().map(|t| t.file_path.clone()).collect();
    app.marks.extend(paths.into_iter().take(marks));
    if lists {
        app.list_marks.push("Sets/Friday".into());
    }
    let marked = app.marks.len() + app.list_marks.len();
    if marked > 0 {
        left.push(Away::Marks);
    }
    if waiting {
        left.retain(|a| *a != Away::Job);
    }
    // Offered, y replays the edits (planned first) instead.
    if app.replay_offer.is_some() && app.job.is_none() && !app.fullscreen && tc.draw(gs::booleans()) {
        press(&mut app, "y");
        assert!(app.replay_offer.is_none() && app.job.as_ref().is_some_and(|j| j.label == "Planning"));
        return;
    }
    for what in left {
        press(&mut app, "\u{1b}");
        match what {
            Away::Waiting => {
                assert!(!app.quit_after_write && last_message(&app) == "staying: not quitting after the write");
                if app.fullscreen {
                    press(&mut app, "\u{1b}");
                    assert!(!app.fullscreen);
                }
                press(&mut app, "\u{1b}");
                assert_eq!(last_message(&app), "writing the drive: too late to stop", "the write goes on");
                return;
            }
            Away::FullScreen => assert!(!app.fullscreen),
            Away::Job => assert!(app.job.is_none() && last_message(&app) == "cancelled"),
            Away::Errors => assert!(app.held.is_none() && app.toast.as_ref().is_none_or(|t| t.1 == Level::Info)),
            Away::Replay => {
                assert!(app.replay_offer.is_none() && app.replay_dismissed.contains(&root));
                assert_eq!(last_message(&app), "not replaying: : and replay does it later");
            }
            Away::Search => assert!(app.query.is_empty() && app.visible.len() == all),
            Away::Marks => {
                let what = if marked == 1 { "mark".to_string() } else { format!("{marked} marks") };
                assert_eq!(last_message(&app), format!("esc again clears the {what}"));
                assert_eq!(app.marks.len() + app.list_marks.len(), marked, "not yet");
                press(&mut app, "\u{1b}");
                assert!(app.marks.is_empty() && app.list_marks.is_empty() && last_message(&app) == "marks cleared");
            }
        }
    }
}

/// The screen redraws at animation rate while anything is under way (a read, a search waiting
/// for typing to pause, the drives' status being read, a job, a lookup of the selected track's
/// matches, a track's files being read, a cue's flash) and rests once nothing is.
#[hegel::test(test_cases = 100)]
fn the_screen_moves_only_while_something_does(tc: TestCase) {
    let (mut app, _dir) = app_with(library());
    wait(&mut app, |a| a.loading.is_none());
    assert!(!app.animating(), "at rest");
    let conditions: [fn(&mut App, bool); 6] = [
        |a, on| a.loading = on.then(|| "Reading TEST…".into()),
        |a, on| a.search_due = on.then(|| Instant::now() + Duration::from_secs(60)),
        |a, on| a.status_loading = on,
        |a, on| a.job = on.then(|| job(false, false)),
        |a, on| a.details_asked = on.then_some((1, 0)),
        // The details shown, with the selected track's not looked up yet.
        |a, on| a.show_details = on,
    ];
    // Each sets it going on its own.
    for c in &conditions {
        c(&mut app, true);
        assert!(app.animating());
        c(&mut app, false);
        assert!(!app.animating(), "at rest again");
    }
    // And any of them together.
    let mut any = false;
    for c in &conditions {
        let on = tc.draw(gs::booleans());
        c(&mut app, on);
        any |= on;
    }
    // A cue's flash, unless the terminal asks for less motion.
    if tc.draw(gs::booleans()) {
        app.flash = Some(([255, 0, 0], Instant::now()));
        assert_eq!(app.flash().is_some(), !super::super::theme::reduced_motion());
    }
    assert_eq!(app.animating(), any || app.flash().is_some());
}

/// Words and fields that make a filter, as the search box reads them.
const FILTER_WORDS: &[&str] = &[
    "daft",
    "-house",
    "techno",
    "bpm:120-130",
    "rating:>=3",
    "tag:Genre/House",
    "playlist:Friday",
    "-playlist:Nope",
    "key:8A",
    "artist:floorplan",
    "(",
    "x|",
    "genre:\"deep house\"",
];

/// The search box shows what the library's own filter (`tracks --filter`) does for what's typed,
/// in the list's order, as it's typed: of the list shown, or with tab, the whole drive. A `tag:`
/// waits for the drive's My Tags (saying so), then matches those tagged, or says it can't read
/// them; a filter that doesn't read (or names a playlist there isn't) says why, and enter says so
/// too. The arrows and page keys move through the list as it's typed.
#[hegel::test(test_cases = 100)]
fn the_search_box_filters_as_the_library_does(tc: TestCase) {
    let words: Vec<&str> = tc.draw(gs::vecs(gs::sampled_from(FILTER_WORDS)).min_size(1).max_size(3));
    let query = words.join(" ");
    // With no drive open there's nothing to search.
    let (mut none, _store) = blank(View::Library);
    press(&mut none, &format!("/{query}"));
    assert!(none.visible.is_empty() && none.searching);
    let mut lib = library();
    lib.tracks = any_tracks(&tc, 8);
    let (mut app, _dir) = app_with(lib);
    let l = app.loaded.as_mut().unwrap();
    let tags: FxHashMap<String, Vec<String>> = (l.library.tracks.iter())
        .filter(|_| tc.draw(gs::booleans()))
        .map(|t| (paths::key(&t.file_path).to_lowercase(), vec!["Genre/House".to_string()]))
        .collect();
    l.my_tags = MyTags::Reading;
    // In a playlist, or all tracks.
    if tc.draw(gs::booleans()) {
        l.node = NodeId::Playlist(2);
        app.refresh_visible();
    }
    press(&mut app, "/");
    for c in query.chars() {
        press(&mut app, &c.to_string());
        if tc.draw(gs::integers::<u8>().max_value(5)) == 0 {
            let code = tc.draw(
                gs::sampled_from(&[KeyCode::Up, KeyCode::Down, KeyCode::PageUp, KeyCode::PageDown][..])
                    .print_as_debug(),
            );
            app.on_key(key_of(code, false));
            assert!(app.track_state.selected().is_none_or(|s| s < app.visible.len()));
        }
    }
    if tc.draw(gs::booleans()) {
        press(&mut app, "\t");
    }
    // Whichever way the drive's My Tags went.
    let (first, enter) = (tc.draw(gs::integers::<usize>().max_value(2)), tc.draw(gs::booleans()));
    for k in 0..3 {
        let state = (first + k) % 3;
        let l = app.loaded.as_mut().unwrap();
        l.my_tags = match state {
            0 => MyTags::Read(Arc::new(tags.clone())),
            1 => MyTags::Failed("no tags here".into()),
            _ => MyTags::Reading,
        };
        app.visible.clear();
        app.refresh_visible();
        searched_as_the_library_does(&mut app, &query, enter && k == 2);
    }
}

/// What the search box shows for `query` is what the library's filter does with it, and what's
/// said of it; `enter` keeps it, saying why it doesn't read when it doesn't.
fn searched_as_the_library_does(app: &mut App, query: &str, enter: bool) {
    let l = app.loaded.as_mut().unwrap();
    let lib = l.library.clone();
    let base = if app.search_all { (0..lib.tracks.len()).collect() } else { l.base() };
    let read = match &l.my_tags {
        MyTags::Read(t) => Some(t.clone()),
        _ => None,
    };
    let none = FxHashMap::default();
    let ctx = QueryContext::new(&lib).with_tags(read.as_deref().unwrap_or(&none));
    match Filter::parse(query).and_then(|f| f.check(&ctx).map(|()| f)) {
        Ok(f) => {
            let want: Vec<usize> = base.into_iter().filter(|&i| f.matches(&lib.tracks[i], &ctx)).collect();
            assert_eq!(app.visible, want, "{query:?}");
            assert_eq!(app.filter_error, None);
            let note = app.filter_note.clone();
            match &l.my_tags {
                _ if !f.uses_tags() => assert_eq!(note, None),
                MyTags::Read(_) => assert_eq!(note, None),
                MyTags::Failed(e) => assert_eq!(note, Some(format!("can't read My Tags: {e}"))),
                _ => assert_eq!(note.as_deref(), Some("reading My Tags…")),
            }
        }
        Err(e) => {
            assert!(app.filter_error.is_some(), "{query:?} reads, but shouldn't: {e}");
            if enter {
                press(app, "\n");
                assert_eq!(Some(last_message(app)), app.filter_error.as_deref(), "enter says why");
            }
        }
    }
}

/// On a big library, the search box waits for typing to pause before narrowing the list (enter
/// doesn't wait), then shows what it would have at once.
#[hegel::test(test_cases = 5, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_big_librarys_search_waits_for_a_pause(tc: TestCase) {
    let n = TYPE_AHEAD as u32 + 1;
    let tracks = (1..=n).map(|id| track(id, &format!("Artist {}", id % 50), &format!("Title {id}"))).collect();
    let (mut app, _dir) = app_with(Library { tracks, ..Default::default() });
    let word = format!("title {}", tc.draw(gs::integers::<u32>().min_value(1).max_value(n)));
    press(&mut app, "/");
    press(&mut app, &word);
    assert_eq!(app.visible.len(), n as usize, "not yet");
    if tc.draw(gs::booleans()) {
        app.search_due = Some(Instant::now());
        app.tick();
    } else {
        press(&mut app, "\n");
    }
    // As a small library's search shows it at once.
    let l = app.loaded.as_ref().unwrap();
    let search = browse::Search::parse(&word);
    let want: Vec<usize> =
        (0..n as usize).filter(|&i| search.matches(&browse::haystack(&l.library.tracks[i]))).collect();
    assert_eq!(app.visible, want, "{word}");
    assert!(app.search_due.is_none());
}

/// Full screen, the player says all it knows of the playing track: its label and year, its key
/// (as rekordbox stored it when that isn't one), its comment and credits; how many beats, bars,
/// cues and phrases it has, with the tempo's range; the phrase playing and the next (and the
/// mood); what mixes into it, or why nothing is listed; and the playlists it's in, the last line
/// saying how many more.
#[hegel::test(test_cases = 100)]
fn the_player_says_what_it_knows_of_the_track(tc: TestCase) {
    use locallibrary::anlz::{Mood, Phrase, SongStructure};
    let n = tc.draw(gs::integers::<u32>().max_value(64));
    let mut beats = grid(0, 120.0, n);
    if n > 8 && tc.draw(gs::booleans()) {
        // Speeding up halfway.
        for b in &mut beats[n as usize / 2..] {
            b.bpm = 126.0;
        }
    }
    let cues: Vec<_> = (0..tc.draw(gs::integers::<u32>().max_value(5)))
        .map(|i| {
            cue(
                tc.draw(gs::booleans()).then_some(i as u8),
                i * 4000,
                tc.draw(gs::booleans()).then_some(i * 4000 + 1000),
            )
        })
        .collect();
    let mood = tc.draw(gs::sampled_from(&[Mood::High, Mood::Mid, Mood::Low][..]).print_as_debug());
    let kinds = [PhraseKind::Intro, PhraseKind::Up, PhraseKind::Chorus, PhraseKind::Outro];
    let phrases: Vec<Phrase> = (0..tc.draw(gs::integers::<u16>().max_value(4)))
        .map(|i| Phrase { kind: kinds[i as usize], start_beat: 1 + 16 * i, end_beat: 17 + 16 * i, fill_beat: None })
        .collect();
    let structure = (!phrases.is_empty()).then_some(SongStructure { mood, phrases });
    let detail = tc.draw(gs::booleans()).then(|| wave(150 * 60));
    let analysis = TrackAnalysis { beats, cues, structure, detail, ..Default::default() };
    let (mut app, _dir) = playing(analysis.clone());
    // On a terminal with colours, or without (where how keys mix is marked).
    let plain = tc.draw(gs::booleans());
    super::super::theme::set_test_mode(plain.then_some(super::super::theme::Mode::Plain));
    let now = app.now.as_mut().unwrap();
    now.phrases = phrase_spans(&analysis, now.duration);
    let t = &mut now.track;
    t.label = tc.draw(gs::sampled_from(&["", "Warp"][..])).into();
    t.year = tc.draw(gs::sampled_from(&[0u16, 1997][..]));
    t.comment = tc.draw(gs::sampled_from(&["", "Peak time"][..])).into();
    t.composer = tc.draw(gs::sampled_from(&["", "Bangalter"][..])).into();
    t.key = tc.draw(gs::sampled_from(&["8A", "Db", "x", ""][..])).into();
    now.key = Camelot::parse(&now.track.key);
    now.loading = false;
    let library = app.loaded.as_ref().unwrap().library.clone();
    let k = tc.draw(gs::integers::<usize>().max_value(3));
    now.matches = tc.draw(gs::booleans()).then(|| Matches {
        library,
        root: PathBuf::new(),
        items: (1..=k)
            .map(|index| {
                let relation =
                    [Relation::Same, Relation::Adjacent, Relation::Relative, Relation::EnergyBoost][index % 4];
                Mix { index, key: Camelot::parse("8A").unwrap(), relation, tempo_gap: 0.0 }
            })
            .collect(),
    });
    now.playlists = (0..tc.draw(gs::integers::<usize>().max_value(6))).map(|i| format!("Sets › List {i}")).collect();
    // Its artwork, in any colour, or none.
    if tc.draw(gs::booleans()) {
        let c = tc.draw(gs::sampled_from(&[[200u8, 40, 90], [10, 10, 10]][..]));
        let mut png = Vec::new();
        image::RgbImage::from_pixel(8, 8, image::Rgb(c))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        now.art = Art::decode(&png);
        assert!(now.art.is_some());
    }
    let pos = tc.draw(gs::integers::<u64>().max_value(40_000));
    app.resume = Some(Duration::from_millis(pos));
    app.fullscreen = true;
    // In any of the visualisers, which v goes round.
    let vis = [Vis::Bars, Vis::Mirror, Vis::Scope, Vis::Deck];
    let turns = tc.draw(gs::integers::<usize>().max_value(5));
    for _ in 0..turns {
        press(&mut app, "v");
    }
    assert_eq!(app.vis, vis[turns % 4]);
    let s = super::super::ui::screen(&mut app, 220, 60);
    let now = app.now.as_ref().unwrap();
    let (t, a) = (&now.track, &now.analysis);
    let says = |what: &str| s.contains(what);
    assert!(says(&t.title) && says(&t.artist));
    assert_eq!(says("Warp"), !t.label.is_empty(), "the label\n{s}");
    assert_eq!(says("1997"), t.year > 0, "the year\n{s}");
    assert_eq!(says("“Peak time”"), !t.comment.is_empty(), "the comment\n{s}");
    assert_eq!(says("composer Bangalter"), !t.composer.is_empty(), "the credits\n{s}");
    match now.key {
        Some(k) => assert!(says(&format!(" {} {} ", k.code(), k.note())), "the key\n{s}"),
        None if !t.key.is_empty() => assert!(says(&format!(" {} ", t.key)), "the key as stored\n{s}"),
        None => {}
    }
    if !a.beats.is_empty() {
        let tempo = if a.beats.iter().any(|b| b.bpm != 120.0) { "120.0–126.0 BPM" } else { "steady tempo" };
        assert!(says(&format!("{} beats · {} bars · {tempo}", a.beats.len(), a.beats.len().div_ceil(4))), "{s}");
    }
    if !a.cues.is_empty() {
        let hot = a.cues.iter().filter(|c| c.hot_cue.is_some()).count();
        let loops = a.cues.iter().filter(|c| c.end_ms.is_some()).count();
        assert!(says(&format!("{hot} hot cues ({loops} loops) · {} memory", a.cues.len() - hot)), "{s}");
    }
    if !now.phrases.is_empty() {
        assert!(says(&format!("{} phrases", now.phrases.len())), "{s}");
    }
    if let Some((i, _, _)) = now.phrase_at(pos as f64) {
        let next = match now.phrases.get(i + 1) {
            Some(p) => format!("phrase next {} in", p.kind.name()),
            None => "phrase last".into(),
        };
        assert!(says(&next), "{next}\n{s}");
        assert!(says(&format!("{mood:?} mood").to_lowercase()), "{s}");
    }
    let listed = now.matches.as_ref().is_some_and(|m| !m.items.is_empty());
    let why = match (&now.key, &now.matches) {
        _ if listed => None,
        (None, _) => Some("no key to match"),
        (_, None) => Some("open this drive to see suggestions"),
        _ => Some("nothing in a compatible key and tempo"),
    };
    if let Some(why) = why {
        assert!(says(why), "{why}\n{s}");
    }
    // Without colours, each match's key says how it mixes.
    if plain && let Some(m) = now.matches.as_ref() {
        for x in &m.items {
            let mark = match x.relation {
                Relation::Same => "=",
                Relation::Adjacent => "±",
                Relation::Relative => "↔",
                Relation::EnergyBoost => "↑",
            };
            assert!(says(&format!("{mark}8A")), "{mark}\n{s}");
        }
    }
    let lists = now.playlists.len();
    if lists > 0 {
        assert!(says(&format!("in {lists} playlist")), "{s}");
        assert_eq!(says("more"), lists > 4, "{s}");
    }
    // At any size, kept to the screen.
    let (w, h) = (
        tc.draw(gs::integers::<u16>().min_value(1).max_value(220)),
        tc.draw(gs::integers::<u16>().min_value(1).max_value(60)),
    );
    assert_eq!(super::super::ui::screen(&mut app, w, h).lines().count(), h as usize);
    // Back in the library, the keys listed are matched against the playing track's (with no key
    // playing, against the selected track's, or none).
    let playing_key = app.now.as_ref().unwrap().key;
    app.fullscreen = false;
    let s = super::super::ui::screen(&mut app, 220, 40);
    let selected_key =
        app.selected_track().and_then(|i| Camelot::parse(&app.loaded.as_ref().unwrap().library.tracks[i].key));
    match playing_key.or(selected_key) {
        Some(k) => assert!(s.contains(&format!("⇄ {} ", k.code())), "{s}"),
        None => assert!(!s.contains("⇄"), "{s}"),
    }
    super::super::theme::set_test_mode(None);
}

/// A click does what it points at: a tab shows its view; a column's header sorts by it (then the
/// other way, then not); a row of the tracks, the tree or the crate selects it there, and a second
/// click on it plays it or opens it; the overview seeks to where it's clicked or dragged; a right
/// click opens the actions menu, whose rows are chosen by a click and which a click outside closes;
/// the outputs listed close at a click.
#[hegel::test(test_cases = 40)]
fn a_click_does_what_it_points_at(tc: TestCase) {
    let (mut app, _dir) = playing(TrackAnalysis::default());
    app.show_crate = true;
    let paths: Vec<String> = app.loaded.as_ref().unwrap().library.tracks.iter().map(|t| t.file_path.clone()).collect();
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    app.add_to_crate(&root, &paths);
    let draw = |app: &mut App| super::super::ui::screen(app, 160, 40);
    let mut info = Device::open(&root).unwrap().info();
    info.name = Some("TEST".into());
    app.drives = vec![info.clone(), DeviceInfo { root: root.join("OTHER"), name: Some("OTHER".into()), ..info }];
    let kinds = turns(&tc, 14);
    for step in 0..tc.draw(gs::integers::<usize>().min_value(14).max_value(28)) {
        (app.view, app.modal, app.deck.outputs, app.picking) = (View::Library, None, None, None);
        draw(&mut app);
        let a = &app.areas;
        let mid = |r: Rect| (r.x + r.width / 2, r.y + r.height / 2);
        match kinds[step % kinds.len()] {
            13 => {
                // The Snapshots view's rows, under its header.
                let record = LibraryRecord {
                    id: Uuid::nil(),
                    name: "Demo".into(),
                    head: None,
                    created: chrono::Utc::now(),
                    updated: chrono::Utc::now(),
                };
                app.history = Some(Ok(vec![(record, (0..4).map(snapshot).collect())]));
                app.view = View::Snapshots;
                draw(&mut app);
                let r = app.areas.history;
                let k = tc.draw(gs::integers::<usize>().max_value(3));
                mouse(&mut app, LEFT, (r.x + 3, r.y + 2 + k as u16));
                assert_eq!(app.history_state.selected(), Some(k + app.history_state.offset()));
            }
            9 => {
                // The Drives view's rows, two lines each.
                app.view = View::Drives;
                draw(&mut app);
                let r = app.areas.drive_list;
                let rows = app.drive_rows();
                let k = tc.draw(gs::integers::<usize>().max_value(rows.len() - 1));
                mouse(&mut app, LEFT, (r.x + 2, r.y + 1 + 2 * k as u16 + tc.draw(gs::integers::<u16>().max_value(1))));
                assert_eq!(app.drive_row(&rows), Some(k), "the row clicked");
                // R there reads the drives again (not here: the tests' player looks at none).
                press(&mut app, "R");
                assert!(!app.status_loading);
            }
            10 => {
                // Picking a playlist, a click on the tracks picks nothing.
                app.track_state.select(Some(0));
                let target = app.target().unwrap();
                app.picking = Some(Pick { target, focus: Focus::Tracks });
                app.focus = Focus::Playlists;
                let r = a.tracks;
                mouse(&mut app, LEFT, (r.x + 3, r.y + 2));
                assert_eq!(app.focus, Focus::Playlists, "still picking in the tree");
            }
            11 => {
                // Below the menu's rows (or its picker's), a click inside it leaves it open.
                app.track_state.select(Some(0));
                app.open_menu(None);
                if tc.draw(gs::booleans()) {
                    let Some(Modal::Menu(m)) = &mut app.modal else { panic!("no menu") };
                    m.sub = Some(super::super::edits::Sub::Rating);
                }
                draw(&mut app);
                let m = app.areas.menu;
                mouse(&mut app, LEFT, (m.x + 2, m.y + m.height - 1));
                assert!(matches!(app.modal, Some(Modal::Menu(_))), "still open");
                // A question takes no clicks.
                app.modal = Some(Modal::Ask("Sure?".into(), Box::new(|_: &mut App| {})));
                mouse(&mut app, LEFT, (1, 1));
                assert!(matches!(app.modal, Some(Modal::Ask(..))), "a question waits for y or n");
            }
            12 => {
                // A view by name; leaving the library puts the search away.
                app.searching = tc.draw(gs::booleans());
                app.query = if app.searching { "daft".into() } else { String::new() };
                let v = tc.draw(gs::sampled_from(&View::ALL[..]).print_as_debug());
                app.perform(Act::ShowView(v), None);
                assert_eq!(app.view, v);
                if v != View::Library {
                    assert!(!app.searching && app.query.is_empty(), "the search put away");
                }
                wait(&mut app, |a| a.job.is_none() && !a.profile.loading);
                app.searching = false;
            }
            0 => {
                let tabs = a.tabs.clone();
                let (v, r) = tabs[tc.draw(gs::integers::<usize>().max_value(tabs.len() - 1))];
                mouse(&mut app, LEFT, mid(r));
                assert_eq!(app.view, v, "the tab clicked");
                // (The Drives and Snapshots views read what they show from the store, here none.)
                wait(&mut app, |a| !a.status_loading && a.job.is_none());
            }
            1 => {
                let cols = a.columns.clone();
                let (field, r) = cols[tc.draw(gs::integers::<usize>().max_value(cols.len() - 1))];
                let before = app.sort;
                mouse(&mut app, LEFT, mid(r));
                assert_eq!(app.sort, browse::click(before, field));
            }
            2 => {
                let (r, offset, n) = (a.tracks, app.track_state.offset(), app.visible.len());
                let k = tc.draw(gs::integers::<usize>().max_value(n.saturating_sub(1).min(r.height as usize - 4)));
                let at = (r.x + 3, r.y + 2 + k as u16);
                mouse(&mut app, LEFT, at);
                assert_eq!((app.focus, app.track_state.selected()), (Focus::Tracks, Some(offset + k)));
                if tc.draw(gs::booleans()) {
                    mouse(&mut app, LEFT, at);
                    // Played (no output here, which is said).
                    assert!(last_message(&app).starts_with("can't play"), "{}", last_message(&app));
                }
            }
            3 => {
                let (r, offset, n) =
                    (a.playlists, app.playlist_state.offset(), app.loaded.as_ref().unwrap().tree.len());
                let k = tc.draw(gs::integers::<usize>().max_value((n - 1).min(r.height as usize - 3)));
                let at = (r.x + 3, r.y + 1 + k as u16);
                mouse(&mut app, LEFT, at);
                assert_eq!((app.focus, app.playlist_state.selected()), (Focus::Playlists, Some(offset + k)));
                if tc.draw(gs::booleans()) {
                    let (folder, rows) = {
                        let l = app.loaded.as_ref().unwrap();
                        (l.tree[offset + k].folder, l.tree.len())
                    };
                    mouse(&mut app, LEFT, at);
                    let l = app.loaded.as_ref().unwrap();
                    if folder {
                        assert_ne!(l.tree.len(), rows, "a folder opened or closed");
                    } else {
                        assert_eq!((app.focus, &l.node), (Focus::Tracks, &l.tree[offset + k].id), "the list opened");
                    }
                }
            }
            4 => {
                let (r, offset, n) = (a.crate_rows, app.crate_state.offset(), app.crate_list.len());
                if r.is_empty() || n == 0 {
                    continue;
                }
                let k = tc.draw(gs::integers::<usize>().max_value((n - 1).min(r.height as usize - 1)));
                mouse(&mut app, LEFT, (r.x + 2, r.y + k as u16));
                assert_eq!((app.focus, app.crate_state.selected()), (Focus::Crate, Some(offset + k)));
            }
            5 | 6 => {
                let r = a.overview;
                if r.is_empty() {
                    continue;
                }
                let x = tc.draw(gs::integers::<u16>().max_value(r.width - 1));
                if kinds[step % kinds.len()] == 6 {
                    app.last_seek = Instant::now() - Duration::from_secs(1);
                    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), (r.x + x, r.y));
                } else {
                    mouse(&mut app, LEFT, (r.x + x, r.y));
                }
                let d = app.now.as_ref().unwrap().duration;
                let want = d.mul_f64((x as f64 + 0.5) / r.width as f64).min(d - Duration::from_millis(250));
                let at = app.pending_seek.unwrap_or_default();
                assert!(at.abs_diff(want) < Duration::from_millis(2), "{at:?} for {want:?}");
            }
            7 => {
                let r = a.tracks;
                mouse(&mut app, MouseEventKind::Down(MouseButton::Right), (r.x + 3, r.y + 2));
                assert!(matches!(app.modal, Some(Modal::Menu(_))), "the menu opens");
                draw(&mut app);
                let m = app.areas.menu;
                let Some(Modal::Menu(menu)) = &app.modal else { unreachable!() };
                let i = tc.draw(gs::integers::<usize>().max_value(menu.items.len() - 1));
                let (action, why) = (menu.items[i].action, menu.items[i].unavailable.clone());
                if tc.draw(gs::integers::<u8>().max_value(3)) == 0 {
                    mouse(&mut app, LEFT, (m.x + m.width + 2, m.y));
                    assert!(app.modal.is_none(), "a click outside closes it");
                    continue;
                }
                mouse(&mut app, LEFT, (m.x + 2, m.y + 1 + i as u16));
                match (why, action) {
                    (Some(why), _) => assert_eq!(last_message(&app), why),
                    (None, Action::RatingMenu | Action::ColourMenu) => {
                        let Some(Modal::Menu(menu)) = &app.modal else { panic!("the picker closed") };
                        assert!(menu.sub.is_some());
                        draw(&mut app);
                        let m = app.areas.menu;
                        mouse(&mut app, LEFT, (m.x + 2, m.y + 1));
                        assert!(app.modal.is_none() && app.job.is_some(), "a rating or colour picked is planned");
                        wait(&mut app, |a| a.job.is_none());
                    }
                    _ => assert!(
                        !matches!(app.modal, Some(Modal::Menu(ref m)) if m.sub.is_none()),
                        "{action:?} wasn't chosen"
                    ),
                }
                app.picking = None;
                wait(&mut app, |a| a.job.is_none());
            }
            _ => {
                app.perform(Act::Outputs, None);
                assert!(app.deck.outputs.is_some());
                mouse(&mut app, LEFT, (1, 1));
                assert!(app.deck.outputs.is_none(), "a click closes the outputs");
            }
        }
    }
}

/// The marks and the crate answer to their keys, wherever focus is: m marks a track (or, in the
/// tree, a playlist or folder) or unmarks it and moves on, and does nothing elsewhere; a adds the
/// selected track to the crate once; del takes the crate's selected track out (the selection
/// staying in the crate), or in the tree, plans deleting the playlist; saving the crate asks what
/// to call it and clearing it asks first, each saying why not when the crate has nothing.
#[hegel::test(test_cases = 60)]
fn the_marks_and_the_crate_answer_to_their_keys(tc: TestCase) {
    use std::collections::BTreeSet;
    let (mut app, _dir) = app_with(library());
    app.show_crate = true;
    app.loaded.as_mut().unwrap().reveal(&NodeId::Playlist(2));
    let (mut marks, mut lists, mut crated) = (BTreeSet::new(), Vec::<String>::new(), Vec::<String>::new());
    let kinds = turns(&tc, 9);
    for step in 0..tc.draw(gs::integers::<usize>().min_value(9).max_value(18)) {
        wait(&mut app, |a| a.job.is_none());
        app.modal = None;
        let (n, rows) = (app.visible.len(), app.loaded.as_ref().unwrap().tree.len());
        match kinds[step % kinds.len()] {
            8 => {
                // K and J move the crate's selected track up and down it; t hides the crate (focus
                // leaving it) and shows it again.
                app.focus = Focus::Crate;
                if crated.is_empty() {
                    continue;
                }
                let k = tc.draw(gs::integers::<usize>().max_value(crated.len() - 1));
                app.crate_state.select(Some(k));
                let up = tc.draw(gs::booleans());
                press(&mut app, if up { "K" } else { "J" });
                let to = if up { k.saturating_sub(1) } else { (k + 1).min(crated.len() - 1) };
                let moved = crated.remove(k);
                crated.insert(to, moved);
                assert_eq!(app.crate_state.selected(), Some(to));
                press(&mut app, "t");
                assert!(!app.show_crate && app.focus == Focus::Tracks);
                press(&mut app, "t");
                assert!(app.show_crate);
            }
            0 => {
                app.focus = Focus::Tracks;
                let k = tc.draw(gs::integers::<usize>().max_value(n - 1));
                app.track_state.select(Some(k));
                let path = app.loaded.as_ref().unwrap().library.tracks[app.visible[k]].file_path.clone();
                press(&mut app, "m");
                if !marks.remove(&path) {
                    marks.insert(path);
                }
                assert_eq!(app.track_state.selected(), Some((k + 1).min(n - 1)), "on to the next");
            }
            1 => {
                app.focus = Focus::Playlists;
                let k = tc.draw(gs::integers::<usize>().max_value(rows - 1));
                app.playlist_state.select(Some(k));
                let l = app.loaded.as_ref().unwrap();
                let path = match l.tree[k].id {
                    NodeId::Playlist(id) => Some(l.playlist_path(id)),
                    _ => None,
                };
                press(&mut app, "m");
                if let Some(p) = path {
                    match lists.iter().position(|x| *x == p) {
                        Some(i) => drop(lists.remove(i)),
                        None => lists.push(p),
                    }
                    assert_eq!(app.playlist_state.selected(), Some((k + 1).min(rows - 1)));
                }
            }
            2 => {
                // Nothing selected in the tracks, m marks nothing either.
                app.focus = Focus::Tracks;
                let sel = app.track_state.selected();
                app.track_state.select(None);
                app.perform(Act::Mark, None);
                app.track_state.select(sel);
                app.focus = if tc.draw(gs::booleans()) { Focus::Drives } else { Focus::Details };
                let crate_sel = app.crate_state.selected();
                app.perform(Act::Mark, None);
                assert_eq!(app.crate_state.selected(), crate_sel, "m marks nothing here");
            }
            3 => {
                app.focus = Focus::Tracks;
                let k = tc.draw(gs::integers::<usize>().max_value(n - 1));
                app.track_state.select(Some(k));
                let path = app.loaded.as_ref().unwrap().library.tracks[app.visible[k]].file_path.clone();
                // (Marked tracks go in together: these have none.)
                app.marks.clear();
                marks.clear();
                press(&mut app, "a");
                if !crated.contains(&path) {
                    crated.push(path);
                }
            }
            4 => {
                app.focus = Focus::Crate;
                if crated.is_empty() {
                    continue;
                }
                let k = tc.draw(gs::integers::<usize>().max_value(crated.len() - 1));
                app.crate_state.select(Some(k));
                key(&mut app, KeyCode::Delete);
                crated.remove(k);
                assert_eq!(app.crate_state.selected(), (!crated.is_empty()).then(|| k.min(crated.len() - 1)));
            }
            5 => {
                app.focus = Focus::Playlists;
                let k = tc.draw(gs::integers::<usize>().max_value(rows - 1));
                app.playlist_state.select(Some(k));
                let playlist = matches!(app.loaded.as_ref().unwrap().tree[k].id, NodeId::Playlist(_));
                key(&mut app, KeyCode::Delete);
                if playlist {
                    assert!(
                        app.job.is_some() || last_message(&app) == "the drive was unplugged",
                        "deleting it is planned"
                    );
                } else {
                    assert_eq!(last_message(&app), "select a playlist or folder");
                }
            }
            6 => {
                app.focus = Focus::Tracks;
                app.perform(Act::CrateSave, None);
                match &app.modal {
                    Some(Modal::Prompt(p)) => {
                        let n = crated.len();
                        assert_eq!(p.label, format!("Save the crate's {n} track{} as", if n == 1 { "" } else { "s" }));
                    }
                    _ => assert_eq!((crated.len(), last_message(&app)), (0, "the crate has nothing from this drive")),
                }
            }
            _ => {
                app.focus = Focus::Tracks;
                app.perform(Act::CrateClear, None);
                if crated.is_empty() {
                    assert_eq!(last_message(&app), "the crate is empty");
                } else {
                    assert!(matches!(app.modal, Some(Modal::Ask(..))), "asks first");
                    press(&mut app, "y");
                    crated.clear();
                }
            }
        }
        assert_eq!(app.marks.iter().cloned().collect::<BTreeSet<_>>(), marks);
        assert_eq!(app.list_marks, lists);
        assert_eq!(app.crate_list.items.iter().map(|t| t.track.file_path.clone()).collect::<Vec<_>>(), crated);
    }
}

/// Tracks asked for one after another while one is being opened: only the last one started is
/// read, with any opened ahead after it (not those before it); one that panics is said as its
/// failure, with nothing read for it.
#[hegel::test(test_cases = 60)]
fn only_the_latest_start_is_opened(tc: TestCase) {
    let (tx, rx) = channel();
    let mut opener = Opener::default();
    let read = Arc::new(Mutex::new(Vec::new()));
    let busy = opener.idle.clone();
    let held = busy.lock().unwrap();
    let asks: Vec<(u64, bool, bool)> = (1..=tc.draw(gs::integers::<u64>().min_value(1).max_value(8)))
        .map(|id| (id, tc.draw(gs::booleans()), tc.draw(gs::booleans())))
        .collect();
    for &(id, starts, panics) in &asks {
        let read = read.clone();
        let job = move |o: &Opening| {
            read.lock().unwrap().push(id);
            assert!(!panics, "no such file {id}");
            o.send(Msg::Note(id.to_string(), Level::Info));
        };
        opener.open(&tx, id, starts, Box::new(job));
    }
    drop(held);
    // Each is for the start before it (or the first): wanted while that's the latest.
    let starts = asks.iter().filter(|a| a.1).count();
    let mut seen = 0;
    let wanted: Vec<(u64, bool)> = (asks.iter())
        .filter_map(|&(id, s, p)| {
            seen += usize::from(s);
            (seen == starts).then_some((id, p))
        })
        .collect();
    let mut said = Vec::new();
    let expected = wanted.iter().map(|(_, p)| if *p { 2 } else { 1 }).sum::<usize>();
    while said.len() < expected {
        match rx.recv_timeout(Duration::from_secs(5)).expect("sent") {
            Msg::Note(id, _) => said.push(format!("read {id}")),
            Msg::Opened(id, Err(e)) => {
                assert!(e.contains(&format!("no such file {id}")), "{e}");
                said.push(format!("failed {id}"));
            }
            Msg::TrackInfo(id, _) => said.push(format!("nothing for {id}")),
            _ => panic!("something else sent"),
        }
    }
    let want: Vec<String> = (wanted.iter())
        .flat_map(|&(id, p)| {
            if p { vec![format!("failed {id}"), format!("nothing for {id}")] } else { vec![format!("read {id}")] }
        })
        .collect();
    assert_eq!(said, want);
    assert_eq!(*read.lock().unwrap(), wanted.iter().map(|w| w.0).collect::<Vec<_>>(), "only those read");
    assert!(rx.recv_timeout(Duration::from_millis(10)).is_err(), "nothing more");
}

/// What the store holds shows where it's asked for: R in the Snapshots view lists every library's
/// snapshots, and enter on one asks which playlist to bring back from it; the drive list arriving
/// with no drive open opens the one open last time when it's there, else the library there is on
/// it (by name), a library row no longer listed isn't selected any more, and a read of the list
/// asked for meanwhile is made.
#[hegel::test(test_cases = 6, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_store_shows_in_the_drives_and_snapshots_views(tc: TestCase) {
    use super::super::drives::DriveRow;
    use super::super::screens::Fixture;
    let Fixture { _dir, mut app, .. } = super::super::props::walk_fixture();
    app.on_key(key_of(KeyCode::Char('3'), false));
    app.history = None;
    app.on_key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT));
    wait(&mut app, |a| a.history.is_some());
    let store = locallibrary::Store::open(&app.store_root).unwrap();
    let want: usize = (store.libraries().unwrap().iter())
        .map(|l| {
            let req = locallibrary::api::HistoryRequest { library: l.name.clone(), limit: Some(200) };
            locallibrary::api::history(&store, &req).unwrap().len()
        })
        .sum();
    assert_eq!(app.history_rows(), want, "every snapshot the store has");
    let row = tc.draw(gs::integers::<usize>().max_value(want - 1));
    app.history_state.select(Some(row));
    app.on_key(key_of(KeyCode::Enter, false));
    match &app.modal {
        Some(Modal::Prompt(p)) => assert!(
            p.label.starts_with("Bring back from ") && p.label.ends_with(" the playlist or folder"),
            "{}",
            p.label
        ),
        _ => panic!("no prompt: {}", last_message(&app)),
    }
    app.modal = None;

    // The drive list, with nothing open.
    let info = app.drives[0].clone();
    let info_root = info.root.clone();
    let library = info.library_id.filter(|id| store.libraries().unwrap().iter().any(|l| l.id == *id));
    (app.loaded, app.loading, app.view) = (None, None, View::Drives);
    let last = tc.draw(gs::booleans());
    app.preferred_root = last.then(|| info.root.clone());
    app.drive_header = Some(DriveRow::Library(uuid::Uuid::from_u128(99)));
    app.drives_again = tc.draw(gs::booleans());
    app.drives.clear();
    app.handle_msg(Msg::Drives(vec![info]));
    assert_eq!(app.drive_header, None, "a library no longer listed");
    assert!(!app.drives_again, "read again as asked");
    let name = match library {
        Some(id) if !last => app.library_names[&id].clone(),
        _ => info_root.file_name().unwrap().to_string_lossy().into_owned(),
    };
    assert_eq!(app.loading, Some(format!("Reading {name}…")));
    wait(&mut app, |a| a.loading.is_none());
    assert!(app.loaded.is_some(), "{}", last_message(&app));
}

/// Tab goes round the Library's panes (the details and the crate among them while shown), each
/// once before the first comes back, and shift-tab goes back the way it came; the Drives and
/// Snapshots views have one pane, so tab leaves them as they are.
#[hegel::test(test_cases = 40)]
fn tab_goes_round_the_panes_there_are(tc: TestCase) {
    let (mut app, _dir) = app_with(library());
    let back = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
    (app.show_details, app.show_crate) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    let panes = 3 + usize::from(app.show_details) + usize::from(app.show_crate);
    let start = app.focus;
    let mut seen = vec![start];
    for _ in 1..panes {
        app.on_key(key_of(KeyCode::Tab, false));
        assert!(!seen.contains(&app.focus), "{:?} again before {start:?}", app.focus);
        seen.push(app.focus);
    }
    app.on_key(key_of(KeyCode::Tab, false));
    assert_eq!(app.focus, start, "round again");
    let k = tc.draw(gs::integers::<usize>().max_value(2 * panes));
    for _ in 0..k {
        app.on_key(key_of(KeyCode::Tab, false));
    }
    for _ in 0..k {
        app.on_key(back);
    }
    assert_eq!(app.focus, start, "back the way it came");
    for view in [View::Drives, View::Snapshots] {
        app.view = view;
        let was = (app.focus, app.drive_state.selected(), app.history_state.selected());
        app.on_key(if tc.draw(gs::booleans()) { back } else { key_of(KeyCode::Tab, false) });
        assert_eq!((app.view, (app.focus, app.drive_state.selected(), app.history_state.selected())), (view, was));
    }
}

/// What the echo plugin's analyzer works out of each track (`echo.bytes`, its file's size), as the
/// app has read it.
fn bytes_of(app: &App, i: usize) -> Option<i64> {
    let l = app.loaded.as_ref().unwrap();
    let v = l.values()?.get(l.library.tracks[i].id, "echo.bytes")?;
    match v {
        locallibrary::fields::FieldValue::Integer(n) => Some(*n),
        other => panic!("{other:?} isn't a size"),
    }
}

/// A plugin's analyzer run from the palette on a drive whose tracks' files are of drawn sizes
/// (some gone): it says how many it analysed and how many failed. Its field's column, asked for
/// from the palette before or after, shows after the player's own columns and before tempo, key
/// and time, with each track's value. Sorted by it either way, the tracks go by their values,
/// those without one last; a filter naming it keeps exactly the tracks whose value matches. The
/// column is kept in what's saved for the next run, which shows it again, and once hidden it
/// isn't.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_analyzers_column_shows_sorts_filters_and_is_kept(tc: TestCase) {
    plugins_once();
    let drive = plugged();
    let mut app = app_on(&drive.0);
    let store = tempfile::tempdir().unwrap();
    app.store_root = store.path().join("store");
    let lib = app.loaded.as_ref().unwrap().library.clone();
    let sizes: Vec<Option<usize>> = lib
        .tracks
        .iter()
        .map(|_| tc.draw(gs::sampled_from(&[Some(0), Some(1), Some(500), Some(4000), None][..])))
        .collect();
    for (t, size) in lib.tracks.iter().zip(&sizes) {
        let file = drive.0.join(t.file_path.trim_start_matches('/'));
        match size {
            Some(n) => std::fs::write(file, "x".repeat(*n)).unwrap(),
            None => std::fs::remove_file(file).unwrap(),
        }
    }
    let column_first = tc.draw(gs::booleans());
    if column_first {
        press(&mut app, ":show the echo.bytes column\n");
    }
    press(&mut app, ":analyze the tracks: file\n");
    wait(&mut app, |a| a.job.is_none());
    let analyzed = sizes.iter().flatten().count();
    let failed = sizes.len() - analyzed;
    let mut said = format!("file size: {analyzed} track{} analyzed, 0 already", if analyzed == 1 { "" } else { "s" });
    if failed > 0 {
        said.push_str(&format!(", {failed} failed (! says why)"));
    }
    assert_eq!(last_message(&app), said);
    if !column_first {
        // Filtered by it before what's worked out is read: nothing matches until it's in, then
        // what does.
        press(&mut app, "/echo.bytes:>0");
        assert_eq!(app.filter_note.as_deref(), Some("reading what analyzers worked out…"));
        assert!(app.visible.is_empty());
        wait(&mut app, |a| matches!(a.loaded.as_ref().unwrap().fields, Fields::Read(_)));
        let mut shown = app.visible.clone();
        shown.sort();
        let over: Vec<usize> = (0..sizes.len()).filter(|&i| sizes[i].is_some_and(|n| n > 0)).collect();
        assert_eq!(shown, over);
        press(&mut app, "\u{1b}");
        press(&mut app, ":show the echo.bytes column\n");
    }
    let name = FieldName::new("echo.bytes");
    assert_eq!(app.field_columns, [name]);
    wait(&mut app, |a| matches!(a.loaded.as_ref().unwrap().fields, Fields::Read(_)));
    let want: Vec<Option<i64>> = sizes.iter().map(|s| s.map(|n| n as i64)).collect();
    assert_eq!((0..lib.tracks.len()).map(|i| bytes_of(&app, i)).collect::<Vec<_>>(), want);

    let s = super::super::ui::screen(&mut app, 120, 30);
    let header = s.lines().find(|l| l.contains("echo.bytes")).unwrap_or_else(|| panic!("no column:\n{s}"));
    let at = |what: &str| header.find(what).unwrap_or_else(|| panic!("no {what} in {header}"));
    assert!(at("Artist") < at("echo.bytes") && at("echo.bytes") < at("BPM") && at("BPM") < at("Time"), "{header}");
    for (t, n) in lib.tracks.iter().zip(&want) {
        let row = s.lines().find(|l| l.contains(t.title.as_str())).unwrap_or_else(|| panic!("no {:?}:\n{s}", t.title));
        if let Some(n) = n.filter(|n| *n >= 100) {
            assert!(row.contains(&n.to_string()), "{row}");
        }
    }

    // Sorted by a click on its heading, then again the other way.
    let heading = spot(&mut app, "echo.bytes");
    for descending in [false, true] {
        mouse(&mut app, LEFT, heading);
        assert_eq!(app.sort.map(|k| (k.field, k.descending)), Some((SortField::Field(name), descending)));
        let order: Vec<Option<i64>> = app.visible.iter().map(|&i| bytes_of(&app, i)).collect();
        let valued = order.iter().take_while(|v| v.is_some()).count();
        assert!(order[valued..].iter().all(Option::is_none), "without a value last: {order:?}");
        let mut sorted = order[..valued].to_vec();
        sorted.sort_by(|a, b| if descending { b.cmp(a) } else { a.cmp(b) });
        assert_eq!(order[..valued], sorted[..], "descending {descending}");
    }
    let over = tc.draw(gs::sampled_from(&[0i64, 1, 499, 1000][..]));
    press(&mut app, &format!("/echo.bytes:>{over}"));
    let mut shown = app.visible.clone();
    shown.sort();
    let matching: Vec<usize> = (0..lib.tracks.len()).filter(|&i| want[i].is_some_and(|n| n > over)).collect();
    assert_eq!((shown, app.filter_error.clone()), (matching, None), "echo.bytes:>{over}");
    press(&mut app, "\u{1b}");

    // The next run.
    app.save_state();
    let saved = state::load(&app.store_root);
    assert_eq!(saved.field_columns, ["echo.bytes"]);
    let mut next = App::blank(app.store_root.clone(), None);
    next.apply_saved(&saved, false);
    assert_eq!(next.field_columns, [name]);
    // The drive opened again: its values read for the column at once.
    let device = Device::open(&drive.0).unwrap();
    let root = device.root().to_path_buf();
    next.install(root, Loaded::prepare(device, (*lib).clone()));
    wait(&mut next, |a| matches!(a.loaded.as_ref().unwrap().fields, Fields::Read(_)));
    assert_eq!((0..lib.tracks.len()).map(|i| bytes_of(&next, i)).collect::<Vec<_>>(), want);
    press(&mut app, ":hide the echo.bytes column\n");
    app.save_state();
    assert!(app.field_columns.is_empty() && state::load(&app.store_root).field_columns.is_empty());
}

/// Plugins' fields of each kind, as an analyzer declares them.
const FIELD_KINDS: [(&str, loclib_plugin::FieldType); 5] = {
    use loclib_plugin::FieldType::*;
    [("p.count", Integer), ("p.level", Number), ("p.loud", Boolean), ("p.mood", String), ("p.tags", Strings)]
};

/// A value of the field of the kind `FIELD_KINDS[kind]`, as drawn for a track: often none, some
/// the same as others'.
#[hegel::test_helper]
fn any_field_value(tc: &TestCase, kind: usize) -> Option<locallibrary::fields::FieldValue> {
    use locallibrary::fields::FieldValue::*;
    if tc.draw(gs::integers::<u8>().max_value(3)) == 0 {
        return None;
    }
    let words = &["Deep", "deep", "日本", ""][..];
    Some(match kind {
        0 => Integer(tc.draw(gs::sampled_from(&[-3i64, 0, 7, 120][..]))),
        1 => Number(tc.draw(gs::sampled_from(&[0.5f64, 1.0, 121.456, -2.25][..]))),
        2 => Boolean(tc.draw(gs::booleans())),
        3 => String(tc.draw(gs::sampled_from(words)).into()),
        _ => Strings(tc.draw(gs::vecs(gs::sampled_from(words)).max_size(2)).into_iter().map(Into::into).collect()),
    })
}

/// A field's value as its column should show it: a number to two places at most (no zeros
/// after the point), a switch as yes or no, a list joined.
fn field_text(v: &locallibrary::fields::FieldValue) -> std::string::String {
    use locallibrary::fields::FieldValue::*;
    match v {
        Integer(n) => n.to_string(),
        Number(n) => format!("{n:.2}").trim_end_matches('0').trim_end_matches('.').to_string(),
        Boolean(b) => if *b { "yes" } else { "no" }.to_string(),
        String(s) => s.clone(),
        Strings(all) => all.join(", "),
    }
}

/// Plugins' fields of every kind, read for made-up tracks (some without a value), shown as
/// columns in the order asked for: drawn at any size without a panic, and at full width after the
/// player's own columns and before tempo, key and time, each row showing its track's value as it
/// should read (cut at the column's edge). Sorted by one either way with clicks on its heading,
/// the tracks go by its values as the library orders them, those without one last.
#[hegel::test(test_cases = 60)]
fn plugins_fields_of_every_kind_show_and_sort(tc: TestCase) {
    let mut lib = library();
    lib.tracks = any_tracks(&tc, 8);
    let ids: Vec<u32> = lib.tracks.iter().map(|t| t.id).collect();
    let (mut app, _dir) = app_with(lib);
    let mut tracks = FxHashMap::default();
    for id in ids {
        let fields: locallibrary::fields::Fields = (0..FIELD_KINDS.len())
            .filter_map(|k| any_field_value(&tc, k).map(|v| (FIELD_KINDS[k].0.to_string(), v)))
            .collect();
        if !fields.is_empty() {
            tracks.insert(id, fields);
        }
    }
    let declared = FIELD_KINDS.iter().map(|(n, t)| (n.to_string(), *t)).collect();
    app.loaded.as_mut().unwrap().fields = Fields::Read(Arc::new(TrackValues { declared, tracks }));
    let order: Vec<FieldName> =
        (0..FIELD_KINDS.len()).filter(|_| tc.draw(gs::booleans())).map(|k| FieldName::new(FIELD_KINDS[k].0)).collect();
    let mut order = order;
    let n = order.len();
    order.rotate_left(tc.draw(gs::integers::<usize>().max_value(n)).min(n));
    for &name in &order {
        app.toggle_field_column(name);
    }
    assert_eq!(app.field_columns, order);
    let (w, h) = (
        tc.draw(gs::integers::<u16>().min_value(1).max_value(220)),
        tc.draw(gs::integers::<u16>().min_value(1).max_value(60)),
    );
    assert_eq!(super::super::ui::screen(&mut app, w, h).lines().count(), h as usize);

    let s = super::super::ui::screen(&mut app, 220, 40);
    let rows: Vec<Vec<char>> = s.lines().map(|l| l.chars().collect()).collect();
    let text = |r: Rect, y: u16| -> std::string::String {
        rows[y as usize][r.x as usize..(r.x + r.width) as usize].iter().filter(|c| !c.is_whitespace()).collect()
    };
    let at = |field: SortField, app: &App| app.areas.columns.iter().find(|(f, _)| *f == field).map(|(_, r)| *r);
    let heading_x: Vec<u16> = order.iter().map(|&n| at(SortField::Field(n), &app).expect("a column").x).collect();
    assert!(heading_x.is_sorted(), "in the order asked for");
    let artist = at(SortField::Artist, &app).unwrap().x;
    let bpm = at(SortField::Bpm, &app).unwrap().x;
    assert!(heading_x.iter().all(|&x| artist < x && x < bpm), "between the player's own and tempo");
    let l = app.loaded.as_ref().unwrap();
    let values = l.values().unwrap().clone();
    for &name in &order {
        let r = at(SortField::Field(name), &app).unwrap();
        assert_eq!(text(r, r.y), name.as_str().chars().filter(|c| !c.is_whitespace()).collect::<std::string::String>());
        for (k, &i) in app.visible.iter().enumerate().take(30) {
            let want = values.get(l.library.tracks[i].id, name.as_str()).map(field_text).unwrap_or_default();
            let want: std::string::String = want.chars().filter(|c| !c.is_whitespace()).collect();
            let shown = text(r, r.y + 1 + k as u16);
            assert!(
                want.starts_with(&shown) && shown.is_empty() == want.is_empty(),
                "{name:?}: {shown:?} for {want:?}"
            );
        }
    }

    let Some(&name) = order.first() else { return };
    let r = at(SortField::Field(name), &app).unwrap();
    for descending in [false, true] {
        mouse(&mut app, LEFT, (r.x, r.y));
        assert_eq!(app.sort.map(|k| (k.field, k.descending)), Some((SortField::Field(name), descending)));
        let l = app.loaded.as_ref().unwrap();
        let got: Vec<Option<&locallibrary::fields::FieldValue>> =
            app.visible.iter().map(|&i| values.get(l.library.tracks[i].id, name.as_str())).collect();
        let valued = got.iter().take_while(|v| v.is_some()).count();
        assert!(got[valued..].iter().all(Option::is_none), "without a value last");
        for pair in got[..valued].windows(2) {
            let ord = locallibrary::query::compare_values(pair[0].unwrap(), pair[1].unwrap());
            let wrong = if descending { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
            assert_ne!(ord, wrong, "{pair:?}, descending {descending}");
        }
    }
}
