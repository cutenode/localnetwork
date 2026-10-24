use locallibrary::anlz::{Beat, Mood, Phrase, SongStructure};
use locallibrary::query::QueryContext;
use locallibrary::sets::{EntryOrigin, SetBook, SetEntry};

use super::testing::*;
use super::*;

#[test]
fn phrases_are_placed_with_the_beat_grid() {
    // 120 BPM: a beat every 500 ms from 0.
    let beats = (0..64).map(|i| Beat { beat_number: (i % 4 + 1) as u16, bpm: 120.0, time_ms: i * 500 }).collect();
    let phrase = |kind, start_beat, end_beat| Phrase { kind, start_beat, end_beat, fill_beat: None };
    let analysis = TrackAnalysis {
        beats,
        structure: Some(SongStructure {
            mood: Mood::High,
            phrases: vec![phrase(PhraseKind::Intro, 1, 33), phrase(PhraseKind::Up, 33, 97)],
        }),
        ..Default::default()
    };
    let spans = phrase_spans(&analysis, Duration::from_secs(40));
    assert_eq!(spans.len(), 2);
    assert_eq!((spans[0].start_ms, spans[0].end_ms, spans[0].bars), (0.0, 16_000.0, 8));
    // Runs past the end of the grid: ends with the track.
    assert_eq!((spans[1].start_ms, spans[1].end_ms, spans[1].bars), (16_000.0, 40_000.0, 16));
}

fn titles(app: &App) -> Vec<String> {
    let l = app.loaded.as_ref().unwrap();
    app.visible.iter().map(|&i| l.library.tracks[i].title.clone()).collect()
}

fn select_tree(app: &mut App, id: NodeId) {
    let row = app.loaded.as_ref().unwrap().tree_position(&id).unwrap();
    app.focus = Focus::Playlists;
    app.playlist_state.select(Some(row));
    key(app, KeyCode::Enter);
}

#[test]
fn search_filters_and_sorts() {
    let (mut app, _dir) = app_with(library());
    assert_eq!(titles(&app), ["Around the World", "Never Grow Old", "Tell You No Lie", "Around the World"]);
    press(&mut app, "/floor");
    assert_eq!(titles(&app), ["Never Grow Old", "Tell You No Lie"]);
    // The filter language.
    app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    press(&mut app, "bpm:>=124");
    assert_eq!(titles(&app), ["Never Grow Old", "Tell You No Lie"]);
    press(&mut app, "-");
    assert!(app.filter_error.is_some());
    assert_eq!(titles(&app).len(), 2, "half-typed, the last good result stays");
    key(&mut app, KeyCode::Backspace);
    press(&mut app, " -genre:house key:8A");
    assert_eq!(titles(&app), ["Never Grow Old"]);
    press(&mut app, "\n");
    assert!(!app.searching && app.filter_error.is_none());
    // Sorting: b cycles, B turns it round; the selected track stays selected.
    key(&mut app, KeyCode::Esc);
    assert!(app.query.is_empty());
    app.sort = Some(SortKey { field: SortField::Bpm, descending: false });
    app.track_state.select(Some(0));
    app.refresh_visible();
    assert_eq!(titles(&app), ["Around the World", "Around the World", "Tell You No Lie", "Never Grow Old"]);
    press(&mut app, "B");
    assert_eq!(titles(&app)[0], "Never Grow Old");
    assert_eq!(app.selected_track(), Some(0), "still on the track it was on");
    press(&mut app, "b");
    assert_eq!(app.sort.map(|k| k.field), Some(SortField::Key));
}

#[test]
fn search_can_cover_the_whole_drive() {
    let (mut app, _dir) = app_with(library());
    select_tree(&mut app, NodeId::Playlist(1));
    select_tree(&mut app, NodeId::Playlist(2));
    assert_eq!(titles(&app), ["Never Grow Old", "Around the World", "Tell You No Lie"]);
    press(&mut app, "/around");
    assert_eq!(titles(&app).len(), 1);
    press(&mut app, "\t");
    assert!(app.search_all);
    assert_eq!(titles(&app).len(), 2, "both copies, from anywhere on the drive");
}

/// Plain words search what `tracks --filter` searches (comments and paths too), the same way.
#[test]
fn search_reads_like_the_filter_language() {
    let mut lib = library();
    lib.tracks[0].comment = "Peak time weapon".into();
    let (mut app, _dir) = app_with(lib.clone());
    let search = |app: &mut App, q: &str| {
        press(app, "/");
        app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        press(app, q);
    };
    let ctx = QueryContext::new(&lib);
    for q in [
        "weapon",
        "copy/1",
        "\"never grow\"",
        "lie|weapon",
        "\"punk around\"",
        "8a house",
        "floor 3b",
        "-techno",
        "floor -lie bpm:>120",
        "\"never grow\" -genre:house artist:floor|daft",
    ] {
        search(&mut app, q);
        let f = Filter::parse(q).unwrap();
        let cli: Vec<String> = f.select(&ctx).iter().map(|t| t.title.clone()).collect();
        assert_eq!(titles(&app), cli, "{q}");
    }
    // The CLI refuses a term with nothing to look for; the search box must cope with one too.
    assert!(Filter::parse("|").is_err());
    search(&mut app, "|");
    search(&mut app, "weapon");
    assert_eq!(titles(&app), ["Around the World"], "in the comment");
    search(&mut app, "\"punk around\"");
    assert!(app.visible.is_empty(), "a word doesn't run from the artist into the title");
}

#[test]
fn browses_by_category() {
    let (mut app, _dir) = app_with(library());
    select_tree(&mut app, NodeId::Browse);
    select_tree(&mut app, NodeId::Category(Category::Bpm));
    select_tree(&mut app, NodeId::Group(Category::Bpm, "125".into()));
    assert_eq!(app.focus, Focus::Tracks);
    assert_eq!(titles(&app), ["Never Grow Old"]);
    assert_eq!(app.loaded.as_ref().unwrap().playlist_name(), "BPM › 125–129");
    select_tree(&mut app, NodeId::Category(Category::History));
    select_tree(&mut app, NodeId::Group(Category::History, "9".into()));
    assert_eq!(titles(&app), ["Tell You No Lie", "Never Grow Old"], "in the order they were played");
}

/// Sets in the store at `store_root`: Friday (track 3, one typed in by hand, track 2), an
/// older one at Corsica Studios (track 1), and one with nothing on the drive.
fn save_sets(store_root: &Path, lib: &Library) {
    let store = Store::open_or_init(store_root).unwrap();
    let entry = |i: usize| SetEntry::from_track(&lib.tracks[i], EntryOrigin::Session, None);
    let friday = vec![entry(2), SetEntry::added("Unknown - ID"), entry(1)];
    let sets = vec![
        set("3lzcorsica222", "2026-09-12", None, Some("Corsica Studios"), vec![entry(0)]),
        set("3m2friday2222", "2026-10-03", Some("Friday at the Warehouse"), Some("Warehouse"), friday),
        set("3lxradio22222", "2026-08-01", Some("Radio"), None, vec![SetEntry::added("Unknown - ID")]),
    ];
    SetBook { sets, ..Default::default() }.save(&store).unwrap();
}

/// Browse › Sets: the store's sets, each with its tracks on the drive in the order played and
/// a count of the rest; played and marked (to save as a playlist) like any list. Without a
/// store there's nothing to browse.
#[test]
fn browses_the_sets_in_the_store() {
    let lib = library();
    let (mut app, _dir) = app_with(lib.clone());
    read_sets(&mut app);
    select_tree(&mut app, NodeId::Browse);
    let l = app.loaded.as_ref().unwrap();
    assert!(l.tree_position(&NodeId::Category(Category::Sets)).is_none(), "no store, no Sets");
    assert!(app.toast.is_none(), "and nothing to say about it");

    // The sets come in above the playlists: the tree's cursor stays where it was.
    save_sets(&app.store_root.clone(), &lib);
    let folder = l.tree_position(&NodeId::Playlist(1));
    app.playlist_state.select(folder);
    app.read_sets();
    read_sets(&mut app);
    let l = app.loaded.as_ref().unwrap();
    assert_eq!(app.playlist_state.selected(), l.tree_position(&NodeId::Playlist(1)));
    assert_ne!(app.playlist_state.selected(), folder);

    select_tree(&mut app, NodeId::Category(Category::Sets));
    let l = app.loaded.as_ref().unwrap();
    let names: Vec<&str> = l.tree.iter().filter(|n| n.depth == 2).map(|n| n.name.as_str()).collect();
    assert_eq!(
        names,
        ["2026-10-03 Friday at the Warehouse", "2026-09-12 Corsica Studios", "2026-08-01 Radio"],
        "newest first"
    );
    select_tree(&mut app, NodeId::Group(Category::Sets, "3lxradio22222".into()));
    let screen = super::super::ui::screen(&mut app, 120, 30);
    assert!(screen.contains("None of its tracks are on this drive."), "{screen}");
    select_tree(&mut app, NodeId::Group(Category::Sets, "3m2friday2222".into()));
    assert_eq!(titles(&app), ["Tell You No Lie", "Never Grow Old"], "in the order played");
    let l = app.loaded.as_ref().unwrap();
    assert_eq!((l.playlist_name().as_str(), l.left_out()), ("Sets › 2026-10-03 Friday at the Warehouse", 1));
    let screen = super::super::ui::screen(&mut app, 120, 30);
    assert!(screen.contains("Friday at the Warehouse 2 · 1 not on this drive"), "{screen}");

    // Marked to save as a playlist, and played, in the set's order.
    press(&mut app, "M");
    let tracks = app.target().unwrap().tracks;
    assert_eq!(tracks, ["/Contents/3.mp3", "/Contents/2.mp3"]);
    app.play_selected();
    assert_eq!(app.queue.items, [2, 1]);

    // A session remembered from before they were called that still opens.
    let saved: SavedNode = serde_json::from_str(r#"{"kind":"group","category":"history","key":"9"}"#).unwrap();
    let l = app.loaded.as_ref().unwrap();
    assert_eq!(l.node_title(&l.find_node(&saved).unwrap()), "Sessions › HISTORY 001");
}

#[test]
fn marks_and_the_crate() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "mm");
    assert_eq!(app.marks.len(), 2);
    assert_eq!(app.track_state.selected(), Some(2), "marking moves down");
    assert_eq!(app.targets(), [0, 1]);
    press(&mut app, "a");
    assert_eq!(app.crate_list.len(), 2);
    press(&mut app, "a");
    assert_eq!(app.crate_list.len(), 2, "no repeats");
    press(&mut app, "M");
    assert!(app.marks.is_empty());
    assert_eq!(app.targets(), [2], "the selected track when nothing is marked");
    press(&mut app, "a");
    press(&mut app, "t");
    assert!(app.show_crate);
    key(&mut app, KeyCode::Tab);
    assert_eq!(app.focus, Focus::Crate);
    app.crate_state.select(Some(2));
    press(&mut app, "K");
    let ids = |app: &App| app.crate_list.items.iter().map(|t| t.track.id).collect::<Vec<_>>();
    assert_eq!(ids(&app), [1, 3, 2]);
    assert_eq!(app.crate_state.selected(), Some(1));
    press(&mut app, "\u{7f}");
    assert_eq!(ids(&app), [1, 2]);
    press(&mut app, "X");
    assert!(matches!(app.modal, Some(Modal::Ask(..))));
    press(&mut app, "y");
    assert!(app.crate_list.is_empty());
}

#[test]
fn edits_go_through_a_plan_and_a_confirmation() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "e");
    assert!(matches!(app.modal, Some(Modal::Menu(_))));
    // A rating straight from the menu: planning starts in the background.
    press(&mut app, "5");
    assert!(app.modal.is_none());
    assert!(app.job.as_ref().is_some_and(|j| !j.writing));
    // The test drive has no export, so the plan fails and says why.
    let t = Instant::now();
    while app.job.is_some() && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(5));
        app.tick();
    }
    assert!(app.job.is_none());
    assert!(
        app.toast.as_ref().is_some_and(|(m, level, _)| *level == Level::Error && m.contains("not a rekordbox export"))
    );
    // Removing needs a playlist in view.
    press(&mut app, "\u{7f}");
    assert!(app.job.is_none());
    assert!(app.toast.as_ref().is_some_and(|(m, ..)| m == "the list shown isn't a playlist"));
}

#[test]
fn quitting_waits_for_a_write_to_finish() {
    let (mut app, _dir) = app_with(library());
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    let job = |writing| Job {
        id: 7,
        label: if writing { "Writing TEST".into() } else { "Planning".into() },
        detail: String::new(),
        cancel: locallibrary::CancelToken::new(),
        writing,
        committed: writing,
        started: Instant::now(),
        touched: Vec::new(),
        after: None,
    };
    // Planning only reads the drive: quitting stops it.
    app.job = Some(job(false));
    press(&mut app, "q");
    assert!(app.quit && app.job.is_none());

    // Writing: q, ctrl-c and a signal all wait for the write.
    app.quit = false;
    app.job = Some(job(true));
    press(&mut app, "q");
    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    app.request_quit();
    assert!(!app.quit && app.quit_after_write);
    assert!(app.toast.as_ref().is_some_and(|(m, ..)| m.contains("quitting when it's done")));
    press(&mut app, "\u{1b}");
    assert!(!app.quit_after_write && app.job.is_some(), "esc stays, and the write goes on");

    // The write ends while waiting at exit (the screen gone, say): then it quits.
    press(&mut app, "q");
    let tx = app.sender();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        let _ = tx.send(Msg::Applied { job: 7, root, result: Err(Error::Cancelled) });
    });
    app.finish_write();
    assert!(app.job.is_none() && app.quit);
    assert_eq!(app.exit_message.as_deref(), Some("cancelled: nothing was written"));
}

/// `m` in the tree marks playlists and folders (to pack them, or for XML of them); they're
/// ticked there and offered first when packing; esc twice clears them with the track marks.
#[test]
fn marks_playlists_in_the_tree() {
    let (mut app, _dir) = app_with(library());
    let row = |app: &App, id| app.loaded.as_ref().unwrap().tree_position(&NodeId::Playlist(id)).unwrap();
    app.focus = Focus::Playlists;
    app.playlist_state.select(Some(row(&app, 1)));
    press(&mut app, "m");
    assert_eq!(app.list_marks, ["Sets"]);
    let s = super::super::ui::screen(&mut app, 100, 30);
    assert!(s.contains("✓ Sets"), "{s}");
    app.perform(Act::Pack, None);
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no choice: {:?}", app.toast) };
    assert_eq!(p.items[0].label, "the marked playlists: Sets");
    press(&mut app, "\u{1b}");
    press(&mut app, "\u{1b}\u{1b}");
    assert!(app.list_marks.is_empty());
}

#[test]
fn adding_to_a_playlist_picks_it_in_the_tree() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "ep");
    assert!(app.picking.is_some());
    assert_eq!(app.focus, Focus::Playlists);
    // Keys other than moving about do nothing while picking; esc gives up.
    press(&mut app, "i");
    assert!(!app.show_details);
    press(&mut app, "\u{1b}");
    assert!(app.picking.is_none());
    assert_eq!(app.focus, Focus::Tracks);
}

#[test]
fn reading_the_drive_again_keeps_the_view() {
    let (mut app, dir) = app_with(library());
    select_tree(&mut app, NodeId::Playlist(1));
    select_tree(&mut app, NodeId::Playlist(2));
    app.sort = Some(SortKey { field: SortField::Title, descending: false });
    app.refresh_visible();
    app.track_state.select(Some(2));
    let selected = app.selected_track().map(|i| app.loaded.as_ref().unwrap().library.tracks[i].file_path.clone());
    press(&mut app, "m");
    // The drive comes back with new ids and another track in the playlist, as after an edit.
    let mut lib = library();
    for t in &mut lib.tracks {
        t.id += 100;
    }
    lib.playlists[1].track_ids = vec![104, 102, 101, 103];
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    app.install(root, Loaded::prepare(Device::open(dir.path()).unwrap(), lib));
    let l = app.loaded.as_ref().unwrap();
    assert_eq!(l.playlist_name(), "Sets › Friday");
    assert_eq!(titles(&app), ["Around the World", "Around the World", "Never Grow Old", "Tell You No Lie"]);
    let now = app.selected_track().map(|i| l.library.tracks[i].file_path.clone());
    assert_eq!(now, selected);
    assert_eq!(app.marks.len(), 1);
    assert!(app.loaded.as_ref().unwrap().tree.iter().any(|n| n.name == "Friday"), "the folder is still open");
}

#[test]
fn the_drive_opened_last_is_shown_whichever_read_finishes_last() {
    let (mut app, dir) = app_with(library());
    let (a, b) = (dir.path().join("A"), dir.path().join("B"));
    for r in [&a, &b] {
        std::fs::create_dir_all(r).unwrap();
    }
    app.load_library(a.clone());
    let first = app.load;
    app.load_library(b.clone());
    let read = |root: &Path| {
        let mut lib = library();
        lib.tracks.truncate(1);
        Ok(Box::new(Loaded::prepare(Device::open(root).unwrap(), lib)))
    };
    // B (opened last) comes back first; A's read finishing afterwards changes nothing.
    let b_root = Device::open(&b).unwrap().root().to_path_buf();
    app.handle_msg(Msg::Library(app.load, b_root.clone(), read(&b)));
    app.handle_msg(Msg::Library(first, a.clone(), read(&a)));
    assert_eq!(app.loaded.as_ref().unwrap().root(), b_root);
    assert!(app.loading.is_none());
    // Nor does an error from a read nobody waits for any more.
    app.toast = None;
    app.handle_msg(Msg::Library(first, a, Err("gone".into())));
    assert!(app.toast.is_none());
}

#[test]
fn notices_unplugging_and_missing_files() {
    let (mut app, _dir) = app_with(library());
    let (root, generation) = {
        let l = app.loaded.as_ref().unwrap();
        (l.root().to_path_buf(), l.generation)
    };
    app.handle_msg(Msg::Missing(generation, vec![false, true, false, false]));
    let l = app.loaded.as_ref().unwrap();
    assert!(l.problems.is_missing(1));
    assert!(l.problems.has_copies(0) && l.problems.has_copies(3));
    app.handle_msg(Msg::Missing(generation + 99, vec![true; 4]));
    assert!(!app.loaded.as_ref().unwrap().problems.is_missing(0), "results for another load are dropped");
    app.handle_msg(Msg::Volumes(Vec::new(), Some((root, false))));
    assert!(app.loaded.as_ref().unwrap().stale);
    press(&mut app, "e");
    let Some(Modal::Menu(m)) = &app.modal else { panic!("no menu") };
    assert!(m.items.iter().find(|i| i.action == Action::RatingMenu).unwrap().unavailable.is_some());
}

#[test]
fn tag_filters_read_the_drives_my_tags() {
    let (_dir, root) = demo_drive();
    let mut device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    let tagged = lib.tracks[1].clone();
    let edit = locallibrary::tags::Edit::Add { tag: "Genre/Techno".into(), tracks: vec![tagged.id] };
    let plan = locallibrary::tags::plan(&device, &edit).unwrap();
    locallibrary::tags::apply(&plan, &mut device, &Ctx::silent()).unwrap();
    let mut app = App::blank(root.join("store"), None);
    app.install(root.clone(), Loaded::prepare(Device::open_export(&root).unwrap(), lib));
    press(&mut app, "/tag:techno");
    assert!(app.visible.is_empty(), "nothing until My Tags are read");
    assert_eq!(app.filter_note.as_deref(), Some("reading My Tags…"));
    wait(&mut app, |a| a.filter_note.is_none());
    assert_eq!(titles(&app), std::slice::from_ref(&tagged.title));
    press(&mut app, " -artist:nobody");
    assert_eq!(titles(&app), [tagged.title], "read once, kept for the next filter");
}

/// Regression: a `playlist:` naming nothing matched no track, and negated every track, with
/// nothing said.
#[test]
fn a_filter_naming_no_playlist_says_so() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "/-playlist:friday");
    assert_eq!((app.filter_error.as_deref(), app.visible.len()), (None, 1), "the copy of track 1 isn't in it");
    press(&mut app, "x");
    let e = app.filter_error.clone().unwrap_or_default();
    assert!(e.contains("playlist \"fridayx\""), "{e}");
}

/// Wait for the plan, check the confirmation, say yes, and wait for the drive to be read again:
/// what the write said.
fn confirm(app: &mut App, check: impl Fn(&locallibrary::edit::EditPlan)) -> String {
    wait(app, |a| a.job.is_none());
    let Some(Modal::Confirm(c)) = &app.modal else { panic!("no confirmation: {:?}", app.toast.as_ref().map(|t| &t.0)) };
    check(c.edit().expect("an edit plan"));
    if std::env::var_os("SHOW_SCREENS").is_some() {
        println!("{}", super::super::ui::screen(app, 100, 30));
    }
    let generation = app.loaded.as_ref().unwrap().generation;
    just_shown(app);
    press(app, "y");
    assert!(matches!(app.modal, Some(Modal::Confirm(_))), "keys right as it appears are ignored");
    settle(app);
    press(app, "\n");
    assert!(app.job.is_none(), "enter doesn't write");
    press(app, "y");
    assert!(app.job.as_ref().is_some_and(|j| j.writing));
    let said = written(app);
    assert_ne!(app.loaded.as_ref().unwrap().generation, generation, "read again");
    assert!(said.contains("written to export.pdb"), "{said}");
    said
}

#[test]
fn edits_a_real_drive_and_keeps_the_view() {
    let (_dir, root) = demo_drive();
    let device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    let mut app = App::blank(root.join("store"), None);
    app.install(root.clone(), Loaded::prepare(device, lib));
    let track =
        |app: &App| app.selected_track().map(|i| app.loaded.as_ref().unwrap().library.tracks[i].clone()).unwrap();

    // A rating for the selected track, straight from the menu.
    app.track_state.select(Some(1));
    let title = track(&app).title;
    press(&mut app, "e4");
    confirm(&mut app, |p| assert_eq!(p.tracks_changed, 1));
    assert_eq!(track(&app).title, title, "still on the same track");
    assert_eq!(track(&app).rating, 4);

    // A new playlist holding both tracks, named in the prompt.
    press(&mut app, "Men");
    assert!(matches!(app.modal, Some(Modal::Prompt(_))));
    press(&mut app, "Sets/Warmup\n");
    confirm(&mut app, |p| assert_eq!(p.playlists_created, ["Sets/", "Sets/Warmup"]));
    let lib = app.loaded.as_ref().unwrap().library.clone();
    let warmup = lib.find_playlist("Sets/Warmup").unwrap();
    assert_eq!(warmup.track_ids.len(), 2);
    assert_eq!(app.marks.len(), 2, "the marks survive");

    // Open it, sort it by BPM and write that order to the drive.
    press(&mut app, "\u{1b}");
    assert_eq!(app.marks.len(), 2, "one esc only says what a second does");
    press(&mut app, "\u{1b}");
    assert!(app.marks.is_empty());
    let l = app.loaded.as_mut().unwrap();
    l.reveal(&NodeId::Playlist(warmup.id));
    select_tree(&mut app, NodeId::Playlist(warmup.id));
    app.sort = Some(SortKey { field: SortField::Bpm, descending: false });
    app.refresh_visible();
    press(&mut app, "es");
    confirm(&mut app, |p| assert!(p.playlists_changed[0].reordered));
    let l = app.loaded.as_ref().unwrap();
    assert_eq!(l.playlist_name(), "Sets › Warmup", "the playlist is still open");
    let warmup = &l.library.find_playlist("Sets/Warmup").unwrap().track_ids;
    let bpms: Vec<f64> = l.library.tracks_by_id(warmup).iter().map(|t| t.bpm).collect();
    assert_eq!(bpms, [120.0, 128.0]);

    // Remove the selected track from it with del.
    press(&mut app, "\u{7f}");
    confirm(&mut app, |p| assert_eq!(p.playlists_changed[0].removed, 1));
    assert_eq!(app.visible.len(), 1);

    // Add it back by picking the playlist in the tree.
    app.loaded.as_mut().unwrap().node = NodeId::All;
    app.refresh_visible();
    app.track_state.select(Some(0));
    press(&mut app, "ep");
    let row = app.loaded.as_ref().unwrap().tree.iter().position(|n| n.name == "Warmup").unwrap();
    app.playlist_state.select(Some(row));
    press(&mut app, "\n");
    wait(&mut app, |a| a.job.is_none());
    match &app.modal {
        Some(Modal::Confirm(_)) => {
            confirm(&mut app, |p| assert_eq!(p.playlists_changed[0].path, "Sets/Warmup"));
        }
        _ => assert!(app.toast.as_ref().is_some_and(|t| t.0.starts_with("nothing to change"))),
    }

    // Another process writing the drive: the lock error is a message, not a crash.
    let lock = Device::open(&root).unwrap().lock().unwrap();
    press(&mut app, "e5");
    wait(&mut app, |a| a.job.is_none());
    if let Some(Modal::Confirm(c)) = &mut app.modal {
        c.shown -= Duration::from_secs(1);
    }
    press(&mut app, "y");
    wait(&mut app, |a| a.job.is_none() && a.loading.is_none());
    let msg = app.toast.as_ref().map(|t| t.0.clone()).unwrap_or_default();
    assert!(msg.contains("busy"), "{msg}");
    drop(lock);
}

/// `u` plans putting the last backup back, shows it, and takes the change back on `y`; the
/// message after a change says so.
#[test]
fn u_undoes_the_last_change() {
    let (_dir, root) = demo_drive();
    let device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    let mut app = App::blank(root.join("store"), None);
    app.install(root.clone(), Loaded::prepare(device, lib));
    app.track_state.select(Some(1));
    let rating = |app: &App| app.selected_track().map(|i| app.loaded.as_ref().unwrap().library.tracks[i].rating);
    let before = rating(&app);
    press(&mut app, "e4");
    let said = confirm(&mut app, |p| assert_eq!(p.tracks_changed, 1));
    assert_eq!(rating(&app), Some(4));
    assert!(said.ends_with("u to undo"), "{said}");

    press(&mut app, "u");
    wait(&mut app, |a| a.job.is_none());
    let Some(Modal::Confirm(c)) = &mut app.modal else { panic!("no confirmation: {:?}", app.toast) };
    let u = c.undo().expect("an undo plan");
    assert!(u.backup.reason.contains("edit"), "{}", u.backup.reason);
    assert_eq!(u.diff.changed_tracks.len(), 1);
    c.shown -= Duration::from_secs(1);
    let screen = super::super::ui::screen(&mut app, 100, 30);
    assert!(screen.contains("put back the backup (before") && screen.contains("1 track as they were"), "{screen}");
    let generation = app.loaded.as_ref().unwrap().generation;
    press(&mut app, "y");
    let said = written(&mut app);
    assert_ne!(app.loaded.as_ref().unwrap().generation, generation, "read again");
    assert_eq!(rating(&app), before);
    assert!(said.starts_with("undone on"), "{said}");

    // In the Drives view, u is for the selected drive; nothing left to undo says so.
    app.drives = vec![Device::open(&root).unwrap().info()];
    app.drive_state.select(Some(0));
    press(&mut app, "2u");
    wait(&mut app, |a| a.job.is_none());
    match &app.modal {
        Some(Modal::Confirm(c)) => assert!(c.undo().is_some()),
        _ => assert!(app.toast.as_ref().is_some_and(|t| t.0.contains("undo")), "{:?}", app.toast),
    }
}

/// From the Drives view: pull a drive into the store (then nothing more to pull), check it,
/// and refuse to eject a folder (it's no volume).
#[test]
fn the_drives_view_pulls_checks_and_ejects() {
    let (dir, root) = demo_drive();
    let device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    let mut app = App::blank(dir.path().join("store"), None);
    app.install(root.clone(), Loaded::prepare(device, lib));
    app.drives = vec![Device::open(&root).unwrap().info()];
    app.drive_state.select(Some(0));
    press(&mut app, "2");
    assert_eq!(app.view, View::Drives);

    // e lists what can be done; pull is the first.
    press(&mut app, "e");
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no menu") };
    assert!(p.title.starts_with("Drive · "));
    assert!(p.items[0].label.starts_with("save a snapshot"));
    press(&mut app, "\n");
    let (op, summary) = confirming(&mut app);
    assert_eq!(op, "pull");
    assert!(summary.contains("starting the library"), "{summary}");
    assert!(!dir.path().join("store").exists(), "planning made no store");
    let said = say_yes(&mut app);
    assert!(said.starts_with("saved DEMO in the store"), "{said}");
    let store = Store::open(dir.path().join("store")).unwrap();
    assert_eq!(store.libraries().unwrap().len(), 1);
    // The status the Drives view reads (made here: tests read no other drives).
    let library = store.libraries().unwrap()[0].name.clone();
    app.status = Some(Ok(vec![fleet(Device::open(&root).unwrap().info(), Some(&library))]));

    // Saved: nothing more to pull.
    app.perform(Act::Pull, None);
    wait(&mut app, |a| a.job.is_none());
    assert!(app.modal.is_none());
    assert!(last_message(&app).contains("saved in the store already"), "{}", last_message(&app));

    // A check: its results in a panel.
    app.perform(Act::Check, None);
    assert!(app.job.as_ref().is_some_and(|j| j.label == "Checking DEMO"));
    wait(&mut app, |a| a.job.is_none());
    let Some(Modal::Report(r)) = &app.modal else { panic!("no report: {}", last_message(&app)) };
    assert_eq!(r.title, "Check · DEMO");
    assert!(r.lines.iter().any(|(_, l)| l == "DEMO"), "{:?}", r.lines);
    press(&mut app, "x");
    assert!(app.modal.is_none(), "any other key closes it");

    // Eject: the test's drive is a folder, with nothing to eject.
    app.perform(Act::Eject, None);
    wait(&mut app, |a| a.job.is_none());
    assert!(app.modal.is_none());
    assert!(last_message(&app).contains("nothing to eject"), "{}", last_message(&app));
}

/// Packing the crate for a set: saved as a playlist on the drive first, then packed onto
/// another drive, picked from the list.
#[test]
fn packs_the_crate_onto_another_drive() {
    let (dir, root) = demo_drive();
    let device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    let mut app = App::blank(dir.path().join("store"), None);
    app.install(root.clone(), Loaded::prepare(device, lib));
    let target = dir.path().join("SET");
    std::fs::create_dir_all(&target).unwrap();
    let target = Device::open(&target).unwrap().root().to_path_buf();
    app.drives = vec![Device::open(&root).unwrap().info(), Device::open(&target).unwrap().info()];
    press(&mut app, "Ma");
    assert_eq!(app.crate_list.len(), 2);

    app.perform(Act::Pack, None);
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no choice") };
    let at = p.items.iter().position(|i| i.label.starts_with("the crate's 2 tracks")).expect("the crate");
    let Some(Modal::Pick(p)) = &mut app.modal else { panic!() };
    p.state.select(Some(at));
    press(&mut app, "\n");
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no drives to pick") };
    assert_eq!(p.items.len(), 1, "the other drive");
    assert_eq!(p.items[0].label, "SET");
    press(&mut app, "\n");
    assert!(matches!(&app.modal, Some(Modal::Prompt(p)) if p.text.starts_with("Sets/")));
    app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    press(&mut app, "Sets/Friday\n");
    let (op, _) = confirming(&mut app);
    assert_eq!(op, "edit");
    say_yes(&mut app);
    // Then the pack, planned once the playlist is there.
    let (op, summary) = confirming(&mut app);
    assert_eq!(op, "pack");
    assert!(summary.contains("Sets/Friday") && summary.contains("SET"), "{summary}");
    let said = say_yes(&mut app);
    assert!(said.starts_with("packed onto SET"), "{said}");
    let packed = Device::open_export(&target).unwrap().library().unwrap();
    assert!(packed.find_playlist("Sets/Friday").is_some_and(|p| p.track_ids.len() == 2));
    assert_eq!(packed.tracks.len(), 2);
}

/// Bringing a playlist back from a snapshot: named, then the snapshot picked.
#[test]
fn brings_back_a_playlist_from_a_snapshot() {
    let (dir, root) = demo_drive();
    let store_root = dir.path().join("store");
    // A playlist, saved in the store, then deleted from the drive.
    let edit = |edits| {
        let req = api::EditRequest { drive: root.display().to_string(), edits, options: Default::default() };
        api::edit_apply(&req, &Ctx::silent()).unwrap();
    };
    let lib = Device::open_export(&root).unwrap().library().unwrap();
    let paths: Vec<String> = lib.tracks.iter().map(|t| t.file_path.clone()).collect();
    edit(super::super::edits::create("Warmup", false, &paths));
    let store = Store::open_or_init(&store_root).unwrap();
    let pull = api::PullRequest { drive: root.display().to_string(), ..Default::default() };
    api::pull(&store, &pull, &Ctx::silent()).unwrap();
    edit(vec![super::super::edits::delete("Warmup")]);

    let device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    assert!(lib.find_playlist("Warmup").is_none());
    let mut app = App::blank(store_root, None);
    app.install(root.clone(), Loaded::prepare(device, lib));
    app.drives = vec![Device::open(&root).unwrap().info()];
    app.perform(Act::RestorePlaylist, None);
    assert!(matches!(&app.modal, Some(Modal::Prompt(_))));
    press(&mut app, "Warmup\n");
    wait(&mut app, |a| a.job.is_none());
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no snapshots: {}", last_message(&app)) };
    assert_eq!(p.items[0].label, "the newest snapshot that has it");
    assert_eq!(p.items.len(), 2, "and the one snapshot");
    press(&mut app, "\n");
    let (op, summary) = confirming(&mut app);
    assert_eq!(op, "playlist.restore");
    assert!(summary.starts_with("bring back Warmup"), "{summary}");
    say_yes(&mut app);
    let l = app.loaded.as_ref().unwrap();
    assert_eq!(l.library.find_playlist("Warmup").map(|p| p.track_ids.len()), Some(2));
}

/// After rekordbox exports over edits made here, the player offers to replay them: `y`
/// shows the plan, esc puts the offer away for the run.
#[test]
fn offers_to_replay_edits_rekordbox_exported_over() {
    let (_dir, root) = demo_drive();
    let pdb = root.join(paths::EXPORT_PDB);
    let exported = std::fs::read(&pdb).unwrap();
    let device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    let mut app = App::blank(root.join("store"), None);
    app.install(root.clone(), Loaded::prepare(device, lib));
    let generation = |app: &App| app.loaded.as_ref().unwrap().generation;
    let g = generation(&app);
    wait(&mut app, |a| a.replay_checked == g);
    assert!(app.replay_offer.is_none(), "nothing recorded yet");

    // An edit made here: recorded, and on the drive, so there's nothing to replay.
    app.track_state.select(Some(1));
    let rating = |app: &App| app.selected_track().map(|i| app.loaded.as_ref().unwrap().library.tracks[i].rating);
    press(&mut app, "e4");
    confirm(&mut app, |_| {});
    let g = generation(&app);
    wait(&mut app, |a| a.replay_checked == g);
    assert!(app.replay_offer.is_none(), "the edit is on the drive");

    // rekordbox exports again (the database as it was): the edit is gone, and offered back.
    let export_again = |app: &mut App| {
        std::fs::write(&pdb, &exported).unwrap();
        // A while after the edit, as an export would be.
        let later = std::time::SystemTime::now() + Duration::from_secs(10);
        std::fs::File::options().write(true).open(&pdb).unwrap().set_modified(later).unwrap();
        app.load_library(root.clone());
        wait(app, |a| a.loading.is_none());
        let g = generation(app);
        wait(app, |a| a.replay_checked == g);
    };
    export_again(&mut app);
    assert_ne!(rating(&app), Some(4));
    let o = app.replay_offer.as_ref().expect("an offer");
    assert_eq!((o.edits, o.drive.as_str()), (1, "DEMO"));
    app.toast = None;
    let s = super::super::ui::screen(&mut app, 100, 30);
    assert!(s.contains("rekordbox exported over 1 edit on DEMO. Replay?"), "{s}");
    assert!(s.lines().last().unwrap().contains("y replay edits"), "{s}");
    press(&mut app, "y");
    let (op, _) = confirming(&mut app);
    assert_eq!(op, "edits.replay");
    say_yes(&mut app);
    assert_eq!(rating(&app), Some(4), "made again");
    let g = generation(&app);
    wait(&mut app, |a| a.replay_checked == g);
    assert!(app.replay_offer.is_none());

    // Put away with esc: not offered again this run, though it's in the palette.
    export_again(&mut app);
    assert!(app.replay_offer.is_some());
    press(&mut app, "\u{1b}");
    assert!(app.replay_offer.is_none());
    export_again(&mut app);
    assert!(app.replay_offer.is_none());
    press(&mut app, "y");
    assert!(app.job.is_none() && app.modal.is_none(), "y is only for the offer");
    // The actions menu replays too.
    press(&mut app, "ey");
    let (op, _) = confirming(&mut app);
    assert_eq!(op, "edits.replay");
}

/// My Tags: a new tag given to the selected track (through the tags plan), shown in the
/// column and under Browse › My Tags.
#[test]
fn gives_tracks_a_my_tag_and_shows_them() {
    let (_dir, root) = demo_drive();
    let device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    let mut app = App::blank(root.join("store"), None);
    app.install(root.clone(), Loaded::prepare(device, lib));
    app.track_state.select(Some(0));
    press(&mut app, "#");
    assert!(app.show_tags_column);
    wait(&mut app, |a| matches!(a.loaded.as_ref().unwrap().my_tags, MyTags::Read(_)));

    press(&mut app, "T");
    wait(&mut app, |a| a.job.is_none());
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no tags: {}", last_message(&app)) };
    assert_eq!(p.title, "My Tag for 1 track");
    assert_eq!(p.items[0].label, "a new tag…");
    assert_eq!(p.selected(), Some(0));
    press(&mut app, "\n");
    assert!(matches!(&app.modal, Some(Modal::Prompt(p)) if p.text == "Genre/"));
    press(&mut app, "Deep\n");
    let (op, _) = confirming(&mut app);
    assert_eq!(op, "tags");
    let said = say_yes(&mut app);
    assert!(said.starts_with("My Tags changed on 1 track"), "{said}");
    wait(&mut app, |a| matches!(a.loaded.as_ref().unwrap().my_tags, MyTags::Read(_)));
    let s = super::super::ui::screen(&mut app, 140, 30);
    assert!(s.contains("My Tags") && s.contains("Deep"), "{s}");

    // Browse › My Tags › Genre › Deep holds it.
    select_tree(&mut app, NodeId::Browse);
    select_tree(&mut app, NodeId::Category(Category::MyTags));
    select_tree(&mut app, NodeId::Group(Category::MyTags, "Genre/Deep".into()));
    assert_eq!(app.visible.len(), 1);
    assert_eq!(app.loaded.as_ref().unwrap().playlist_name(), "My Tags › Genre › Deep");
}

/// A rule-based playlist from the search box's filter: saved in the store, then filled on
/// the drive once confirmed; the rules are listed in a panel.
#[test]
fn makes_a_smart_playlist_from_the_search() {
    let (dir, root) = demo_drive();
    let device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    let mut app = App::blank(dir.path().join("store"), None);
    app.install(root.clone(), Loaded::prepare(device, lib));
    app.perform(Act::SmartNew, None);
    assert!(app.modal.is_none() && last_message(&app).starts_with("search first"));
    press(&mut app, "/bpm:>=125\n");
    assert_eq!(app.visible.len(), 1);
    app.perform(Act::SmartNew, None);
    assert!(matches!(&app.modal, Some(Modal::Prompt(p)) if p.text == "Smart/bpm:>=125"));
    app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    press(&mut app, "Smart/Fast\n");
    let (op, _) = confirming(&mut app);
    assert_eq!(op, "smart.write");
    say_yes(&mut app);
    let l = app.loaded.as_ref().unwrap();
    assert_eq!(l.library.find_playlist("Smart/Fast").map(|p| p.track_ids.len()), Some(1));

    app.perform(Act::SmartList, None);
    wait(&mut app, |a| a.job.is_none());
    let Some(Modal::Report(r)) = &app.modal else { panic!("no rules: {}", last_message(&app)) };
    assert!(r.lines.iter().any(|(_, l)| l == "Smart/Fast"), "{:?}", r.lines);
    assert!(r.lines.iter().any(|(_, l)| l.starts_with("bpm:>=125")), "{:?}", r.lines);
    // Filling them again changes nothing.
    press(&mut app, "x");
    app.perform(Act::SmartApply, None);
    wait(&mut app, |a| a.job.is_none());
    assert!(app.modal.is_none() && last_message(&app).starts_with("nothing to change"), "{}", last_message(&app));
}

/// The list shown and rekordbox XML, written where asked (never onto the drive).
#[test]
fn exports_the_list_shown_and_rekordbox_xml() {
    let (dir, root) = demo_drive();
    let device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    let mut app = App::blank(dir.path().join("store"), None);
    app.install(root.clone(), Loaded::prepare(device, lib));
    let out = dir.path().join("out");
    let type_path = |app: &mut App, path: &Path| {
        app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        press(app, &format!("{}\n", path.display()));
        wait(app, |a| a.job.is_none());
    };
    app.perform(Act::ExportList, None);
    assert!(matches!(&app.modal, Some(Modal::Prompt(p)) if p.text.ends_with("DEMO.m3u8")));
    type_path(&mut app, &out.join("all.csv"));
    assert!(last_message(&app).ends_with("2 tracks"), "{}", last_message(&app));
    let csv = std::fs::read_to_string(out.join("all.csv")).unwrap();
    assert!(csv.contains("Demo Track 1"), "{csv}");

    app.perform(Act::ExportXml, None);
    type_path(&mut app, &out.join("demo.xml"));
    assert!(last_message(&app).contains("2 tracks"), "{}", last_message(&app));
    assert!(std::fs::read_to_string(out.join("demo.xml")).unwrap().contains("DJ_PLAYLISTS"));

    // A file that's there is replaced only once that's asked about.
    std::fs::write(out.join("demo.xml"), "mine").unwrap();
    app.perform(Act::ExportXml, None);
    type_path(&mut app, &out.join("demo.xml"));
    assert!(matches!(&app.modal, Some(Modal::Ask(q, _)) if q.contains("Replace it?")));
    press(&mut app, "n");
    assert_eq!(std::fs::read_to_string(out.join("demo.xml")).unwrap(), "mine");
    app.perform(Act::ExportXml, None);
    type_path(&mut app, &out.join("demo.xml"));
    press(&mut app, "y");
    wait(&mut app, |a| a.job.is_none());
    assert!(std::fs::read_to_string(out.join("demo.xml")).unwrap().contains("DJ_PLAYLISTS"));

    // Not onto the drive itself.
    app.perform(Act::ExportXml, None);
    type_path(&mut app, &root.join("x.xml"));
    assert!(last_message(&app).starts_with("not onto the drive"));
    assert!(!root.join("x.xml").exists());
}

/// The set shown last run, on starting: said to be reading until the sets are in. Read again
/// (after an edit, say), it stays as it was meanwhile.
#[test]
fn a_set_shown_stays_shown_while_the_sets_are_read() {
    let lib = library();
    let dir = tempfile::tempdir().unwrap();
    save_sets(&dir.path().join("store"), &lib);
    let mut app = App::blank(dir.path().join("store"), None);
    let device = Device::open(dir.path()).unwrap();
    let root = device.root().to_path_buf();
    let friday = SavedNode::Group { category: Category::Sets, key: "3m2friday2222".into() };
    app.restore = Some((root.clone(), friday));
    app.install(root.clone(), Loaded::prepare(device, lib.clone()));
    assert!(app.visible.is_empty() && app.animating());
    let screen = super::super::ui::screen(&mut app, 120, 30);
    assert!(screen.contains("Reading the sets…"), "{screen}");
    read_sets(&mut app);
    assert_eq!(titles(&app), ["Tell You No Lie", "Never Grow Old"]);
    assert!(!app.animating());
    let l = app.loaded.as_ref().unwrap();
    let shown = l.tree_position(&NodeId::Group(Category::Sets, "3m2friday2222".into()));
    assert!(shown.is_some() && app.playlist_state.selected() == shown, "the tree's cursor on it");
    app.install(root.clone(), Loaded::prepare(Device::open(&root).unwrap(), lib));
    assert_eq!(titles(&app), ["Tell You No Lie", "Never Grow Old"]);
}

/// A set as it is on the drive: its tracks there, in the order played, written the way a
/// playlist is.
#[test]
fn exports_a_set() {
    let (dir, root) = demo_drive();
    let device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let entry = |i: usize| SetEntry::from_track(&lib.tracks[i], EntryOrigin::Session, None);
    let elsewhere = SetEntry { file: Some("/Contents/elsewhere.mp3".into()), ..entry(0) };
    let tracks = vec![entry(1), elsewhere, entry(0)];
    let friday = set("3m2friday2222", "2026-10-03", Some("Friday at the Warehouse"), None, tracks);
    SetBook { sets: vec![friday], ..Default::default() }.save(&store).unwrap();
    let mut app = App::blank(store.root().to_path_buf(), None);
    app.install(root.clone(), Loaded::prepare(device, lib.clone()));
    read_sets(&mut app);
    select_tree(&mut app, NodeId::Browse);
    select_tree(&mut app, NodeId::Category(Category::Sets));
    select_tree(&mut app, NodeId::Group(Category::Sets, "3m2friday2222".into()));

    app.perform(Act::ExportList, None);
    let Some(Modal::Prompt(p)) = &app.modal else { panic!("no prompt: {}", last_message(&app)) };
    assert!(p.text.ends_with("/2026-10-03 Friday at the Warehouse.m3u8"), "{}", p.text);
    let out = dir.path().join("out").join("friday.m3u8");
    app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    press(&mut app, &format!("{}\n", out.display()));
    wait(&mut app, |a| a.job.is_none());
    assert!(last_message(&app).ends_with(": 2 tracks"), "{}", last_message(&app));
    let m3u8 = std::fs::read_to_string(&out).unwrap();
    let (first, second) = (&lib.tracks[1], &lib.tracks[0]);
    let at = |t: &Track| m3u8.find(&format!(",{} - {}\n", t.artist, t.title)).unwrap();
    assert!(m3u8.starts_with("#EXTM3U\n#PLAYLIST:2026-10-03 Friday at the Warehouse\n"), "{m3u8}");
    assert!(at(first) < at(second), "in the order played: {m3u8}");
    assert!(!m3u8.contains("elsewhere"), "{m3u8}");
}

#[test]
fn errors_stay_until_esc_and_every_message_is_kept() {
    let (mut app, _dir) = app_with(library());
    app.toast("can't read it", Level::Error);
    app.toast("added 1 to the crate", Level::Info);
    assert_eq!(app.held.as_deref(), Some("can't read it"), "the error stays above the newer message");
    // Info messages go after a while; errors don't.
    app.toast.as_mut().unwrap().2 -= Duration::from_secs(10);
    app.tick();
    assert!(app.toast.is_none() && app.held.is_some());
    app.toast("worse", Level::Error);
    // As old as the info message that went, no older: an `Instant` can't go back past the
    // machine's boot, which a CI runner may have done minutes ago.
    app.toast.as_mut().unwrap().2 -= Duration::from_secs(10);
    app.tick();
    assert!(app.toast.is_some());
    // The first esc puts errors away (and does nothing else).
    press(&mut app, "m");
    press(&mut app, "\u{1b}");
    assert!(app.toast.is_none() && app.held.is_none());
    assert_eq!(app.marks.len(), 1);
    // The log has everything, and ! shows it until another key.
    assert_eq!(app.log.len(), 3);
    press(&mut app, "!");
    assert!(app.show_log);
    let s = super::super::ui::screen(&mut app, 80, 24);
    assert!(s.contains("Messages") && s.contains("can't read it") && s.contains("worse"), "{s}");
    press(&mut app, "x");
    assert!(!app.show_log);
    // A long message wraps, and shows above the search box while it's open.
    let long = format!("{} end", "word ".repeat(30));
    app.toast(long, Level::Info);
    press(&mut app, "/");
    let s = super::super::ui::screen(&mut app, 60, 20);
    let rows: Vec<&str> = s.lines().collect();
    assert!(rows[19].starts_with(" / "), "{s}");
    assert!(rows[18].contains("end") && rows[17].contains("word"), "{s}");
}

#[test]
fn remembers_the_crate_queue_view_and_modes() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    std::fs::create_dir(dir.path().join("drive")).unwrap();
    let mut app = App::with_library(&dir.path().join("drive"), library());
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    // The app's store is inside the test drive (not saved there), so save beside another.
    app.store_root = store.clone();
    press(&mut app, "ma");
    app.shuffle = true;
    app.repeat = Repeat::All;
    app.view = View::Mixes;
    let lib = app.loaded.as_ref().unwrap().library.clone();
    app.queue = Queue {
        library: lib.clone(),
        items: (0..4).collect(),
        root: root.clone(),
        drive: "TEST".into(),
        order: vec![2, 0, 1, 3],
        pos: 1,
    };
    app.resume = Some(Duration::from_secs(95));
    app.save_state();

    let saved = state::load(&store);
    assert_eq!(saved.view.as_deref(), Some("Mixes"));
    assert!(saved.shuffle && saved.repeat.as_deref() == Some("all"));
    assert_eq!(saved.crate_list.len(), 1);
    let q = saved.queue.as_ref().unwrap();
    assert_eq!((q.pos, q.at_ms), (1, 90_000), "where it was, to the last 15 seconds");
    assert_eq!(q.tracks[0], lib.tracks[2].file_path, "in the order it plays");

    // The next run: the crate straight away, the queue once its drive is read.
    let mut next = App::blank(store, None);
    next.apply_saved(&saved, true);
    assert_eq!(next.crate_list.len(), 1);
    assert!(next.shuffle && next.repeat == Repeat::All);
    assert!(next.queue.is_empty());
    let device = Device::open(&root).unwrap();
    next.install(root.clone(), Loaded::prepare(device, library()));
    assert_eq!(next.queue.len(), 4);
    let (t, at) = next.resumable().unwrap();
    assert_eq!((t.title.as_str(), at), ("Around the World", Duration::from_secs(90)));
    assert!(super::super::ui::screen(&mut next, 100, 30).contains("carries on with"));
    // Moving on in the queue instead forgets where that track was.
    press(&mut next, "n");
    assert!(next.resume.is_none() && next.resumable().is_none());
}

/// A folder of sets opened in the Mixes view lists the recordings in it and its subfolders
/// (not hidden ones, nor other files), is listed again on the next run with what's new in it,
/// and `del` forgets it.
#[test]
fn mixes_lists_a_folder_of_sets_and_remembers_it() {
    let dir = tempfile::tempdir().unwrap();
    let sets = dir.path().join("mixes");
    let files = [
        "tachycardia/Tierney Cyren.mp3",
        "tachycardia/._Tierney Cyren.mp3",
        "one shot/techno/Untitled.mp3",
        "loose.WAV",
        "notes.txt",
        ".old/x.wav",
    ];
    for f in files {
        let p = sets.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "x").unwrap();
    }
    let listed = |app: &mut App| {
        let until = Instant::now() + Duration::from_secs(5);
        while let Ok(msg) = app.rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
            let done = matches!(msg, Msg::MixFolders(..));
            app.handle_msg(msg);
            if done {
                break;
            }
        }
    };
    let shown = |app: &App| app.mix.sources.iter().map(|s| format!("{} · {}", s.name, s.place)).collect::<Vec<_>>();

    let store = dir.path().join("store");
    let mut app = App::blank(store.clone(), None);
    app.view = View::Mixes;
    press(&mut app, &format!("O{}\n", sets.display()));
    listed(&mut app);
    let techno = Path::new("one shot").join("techno");
    assert_eq!(
        shown(&app),
        [
            "loose.WAV · mixes".into(),
            format!("Untitled.mp3 · {}", techno.display()),
            "Tierney Cyren.mp3 · tachycardia".into()
        ]
    );
    assert!(app.toast.as_ref().is_some_and(|(m, ..)| m.starts_with("3 recordings in")), "{:?}", app.toast);
    app.save_state();

    std::fs::write(sets.join("tachycardia/encore.flac"), "x").unwrap();
    let mut next = App::blank(store.clone(), None);
    next.apply_saved(&state::load(&store), false);
    next.view = View::Mixes;
    // Drawing the view lists the folders.
    super::super::ui::screen(&mut next, 100, 30);
    listed(&mut next);
    assert_eq!(next.mix.sources.len(), 4, "{:?}", shown(&next));
    assert!(super::super::ui::screen(&mut next, 100, 30).contains("encore.flac"));

    press(&mut next, "\u{7f}");
    assert!(next.mix.sources.is_empty() && next.mix.folders.is_empty(), "{:?}", shown(&next));
}

/// A library the size of a big collection, made up: `n` tracks with a few thousand artists,
/// albums and genres, every key and tempo; 500 folders two deep holding ten playlists of a
/// hundred tracks each; and twenty history lists.
fn big_library(n: u32) -> Library {
    use locallibrary::Playlist;
    let keys = ["1A", "2A", "3B", "8A", "8B", "Am", "F#m", "12B", "Ebm", "C"];
    let tracks = (1..=n)
        .map(|id| {
            let mut t = track(id, &format!("Artist {}", id % 4000), &format!("Title {id}"));
            t.album = format!("Album {}", id % 6000);
            t.genre = format!("Genre {}", id % 120);
            t.label = format!("Label {}", id % 900);
            t.comment = if id % 7 == 0 { format!("comment {id}") } else { String::new() };
            t.key = keys[id as usize % keys.len()].into();
            t.bpm = 90.0 + (id % 800) as f64 / 10.0;
            t.rating = (id % 6) as u8;
            t.play_count = (id % 13) as u16;
            t.date_added = format!("20{:02}-{:02}-01", 10 + id % 16, 1 + id % 12);
            t.file_path = format!("/Contents/Artist {}/Album {}/{id}.mp3", id % 4000, id % 6000);
            t
        })
        .collect();
    let mut playlists = Vec::new();
    let mut id = 0;
    let mut next = || {
        id += 1;
        id
    };
    let node = |id, parent, name: String, folder, track_ids| Playlist {
        id,
        parent_id: parent,
        name,
        is_folder: folder,
        sort_order: id,
        track_ids,
    };
    for f in 0..50 {
        let top = next();
        playlists.push(node(top, None, format!("Folder {f}"), true, Vec::new()));
        for g in 0..9 {
            let sub = next();
            playlists.push(node(sub, Some(top), format!("Sub {g}"), true, Vec::new()));
            for p in 0..10 {
                let start = (f * 9 + g) * 10 + p;
                let ids = (0..100).map(|k| (start * 37 + k * 101) % n + 1).collect();
                playlists.push(node(next(), Some(sub), format!("List {p}"), false, ids));
            }
        }
    }
    let history =
        (0..20).map(|h| node(100_000 + h, None, format!("HISTORY {h:03}"), false, (1..60).collect())).collect();
    Library { tracks, playlists, history_playlists: 20, history }
}

/// What runs on the UI thread stays quick with 50,000 tracks and 5,000 playlists (the bound
/// is for a debug build, many times what each takes); the rest runs in the background.
#[test]
fn a_big_library_stays_quick() {
    let dir = tempfile::tempdir().unwrap();
    // Reading and indexing the drive happen in the background: they only have to finish.
    let loaded = Loaded::prepare(Device::open(dir.path()).unwrap(), big_library(50_000));
    let root = loaded.root().to_path_buf();
    let mut app = App::blank(dir.path().join("store"), None);
    let check = |what: &str, t: Instant| {
        let took = t.elapsed();
        assert!(took < Duration::from_secs(1), "{what} took {took:?}");
    };
    let t = Instant::now();
    app.install(root.clone(), loaded);
    check("showing the library", t);
    assert_eq!(app.visible.len(), 50_000);

    // Sort ranks and Browse's groups come from the background (other messages are left: the
    // drive here is an empty folder, which the player would take for unplugged).
    while app.loaded.as_ref().unwrap().ranks.len() < browse::SORT_FIELDS.len() {
        if let msg @ Msg::Indexed(..) = app.rx.recv_timeout(Duration::from_secs(60)).expect("indexed") {
            app.handle_msg(msg);
        }
    }
    let t = Instant::now();
    let l = app.loaded.as_mut().unwrap();
    l.expanded.insert(NodeId::Browse);
    l.expanded.extend(Category::ALL.map(NodeId::Category));
    let folders: Vec<NodeId> =
        l.library.playlists.iter().filter(|p| p.is_folder).map(|p| NodeId::Playlist(p.id)).collect();
    l.expanded.extend(folders);
    l.rebuild_tree();
    check("opening every folder and category", t);
    assert!(app.loaded.as_ref().unwrap().tree.len() > 5_000);

    for field in browse::SORT_FIELDS {
        let t = Instant::now();
        app.sort = Some(SortKey { field, descending: false });
        app.refresh_visible();
        check(&format!("sorting by {field:?}"), t);
    }

    // Typing waits for a pause; each search is then one pass over the tracks.
    let t = Instant::now();
    press(&mut app, "/artist 12");
    check("typing", t);
    assert!(app.search_due.is_some(), "not searched on every key");
    let t = Instant::now();
    app.flush_search();
    check("a search", t);
    assert!(!app.visible.is_empty() && app.visible.len() < 50_000);
    app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    press(&mut app, "bpm:120-128 key:8A~ -genre:\"genre 1\" title");
    let t = Instant::now();
    press(&mut app, "\n");
    check("a filter", t);
    assert!(!app.visible.is_empty());

    // Playing queues the whole list without copying it.
    press(&mut app, "\u{1b}");
    assert_eq!(app.visible.len(), 50_000);
    let t = Instant::now();
    app.play_selected();
    check("queueing every track", t);
    assert_eq!(app.queue.len(), 50_000);

    // Finding a list again after the drive is read again.
    let l = app.loaded.as_ref().unwrap();
    let deep = l.library.playlists.iter().rev().find(|p| !p.is_folder).unwrap();
    let saved = l.save_node(&NodeId::Playlist(deep.id));
    let t = Instant::now();
    assert_eq!(l.find_node(&saved), Some(NodeId::Playlist(deep.id)));
    check("finding a playlist by its path", t);
}

#[test]
fn tracks_open_in_the_background() {
    let (mut app, _dir) = app_with(library());
    let l = app.loaded.as_ref().unwrap();
    let (track, root) = (l.library.tracks[0].clone(), l.root().to_path_buf());
    app.now = Some(NowPlaying {
        id: 5,
        key: None,
        drive: "TEST".into(),
        root,
        analysis: TrackAnalysis::default(),
        from_plugins: None,
        art: None,
        duration: Duration::from_secs(300),
        phrases: Vec::new(),
        matches: None,
        playlists: Vec::new(),
        loading: true,
        opening: true,
        advancing: false,
        track,
    });
    // A seek before the audio is in waits for it.
    app.seek_to(Duration::from_secs(30));
    assert_eq!(app.pending_seek, Some(Duration::from_secs(30)));
    // What comes back for a track started before this one is dropped.
    app.toast = None;
    app.handle_msg(Msg::Opened(4, Err("gone".into())));
    assert!(app.now.is_some() && app.toast.is_none());
    app.handle_msg(Msg::Opened(5, Err("can't decode it".into())));
    assert!(app.now.is_none());
    assert_eq!(app.toast.as_ref().map(|t| t.0.as_str()), Some("Daft Punk - Around the World: can't decode it"));
}

/// A file opened in the background plays, from where it was asked to start. (No device:
/// what plays is read from a mixer.)
#[test]
fn plays_a_file_opened_in_the_background() {
    let dir = tempfile::tempdir().unwrap();
    // Two seconds counting up: 16-bit mono WAV at 8 kHz.
    let ramp: Vec<i16> = (0..16_000).collect();
    let path = dir.path().join("set.wav");
    std::fs::write(&path, super::super::audio::tests::wav_of(8000, &ramp)).unwrap();
    let (mut audio, mut out) = Audio::offline(8000);
    audio.set_volume(1.0);
    let mut app = App::blank(dir.path().join("store"), Some(audio));
    let mut track = track(1, "DJ", "Set");
    track.duration_secs = 2;
    assert!(app.play_file(&path, track, Duration::from_millis(500)));
    assert!(app.now.as_ref().is_some_and(|n| n.opening));
    wait(&mut app, |a| a.now.as_ref().is_some_and(|n| !n.opening));
    assert!(app.audio.as_ref().unwrap().is_loaded());
    assert_eq!(app.now.as_ref().unwrap().duration, Duration::from_secs(2));
    assert!(app.pending_seek.is_none() && app.position() >= Duration::from_millis(400), "{:?}", app.position());
    assert_eq!((out.next().unwrap() * 32768.0).round(), 4000.0, "from half a second in");
}

/// Near the end of a track the next is opened and queued behind it: the player goes straight
/// on to it, and so does what it shows. One at another sample rate starts as the playing one
/// ends. Skipping or changing repeat drops what was opened. (No device: what plays is read
/// from a mixer.)
#[test]
fn plays_the_queue_without_gaps() {
    let (_dir, mut app, mut out) = playing_wavs(&[8000, 8000, 16000]);
    wait(&mut app, |a| a.now.as_ref().is_some_and(|n| !n.opening));
    // Under 30 seconds from the end: the next one's opened, and queued.
    wait(&mut app, |a| a.upcoming.as_ref().is_some_and(|u| u.queued && !u.now.loading));
    assert_eq!(app.upcoming.as_ref().unwrap().pos, 1);
    let played: Vec<f32> = (&mut out).take(4400).collect();
    let level = |s: f32| (s * 32768.0).round();
    assert_eq!((level(played[0]), level(played[3999]), level(played[4000])), (1000.0, 1000.0, 2000.0), "no gap");
    app.tick();
    let showing = |app: &App| app.now.as_ref().map(|n| (app.queue.pos, n.track.title.clone(), n.opening));
    assert_eq!(showing(&app), Some((1, "Track 2".into(), false)));
    assert!((ms(50)..ms(52)).contains(&app.position()), "{:?}", app.position());

    // Track 3 is at 16 kHz: opened, but kept to start once track 2 ends.
    wait(&mut app, |a| a.upcoming.as_ref().is_some_and(|u| u.file.is_some() && !u.queued));
    (&mut out).take(3700).for_each(drop);
    wait(&mut app, |a| a.queue.pos == 2);
    assert_eq!(showing(&app), Some((2, "Track 3".into(), false)));
    assert!(app.audio.as_ref().is_some_and(|a| a.is_loaded()));

    // Repeat all: track 1 comes next (at 8 kHz: kept, not queued). Repeat one: track 3
    // again, queued.
    press(&mut app, "r");
    assert!(app.upcoming.is_none());
    wait(&mut app, |a| a.upcoming.as_ref().is_some_and(|u| u.pos == 0 && u.file.is_some()));
    press(&mut app, "r");
    wait(&mut app, |a| a.upcoming.as_ref().is_some_and(|u| u.pos == 2 && u.queued));
    // Skipping starts the next track afresh.
    press(&mut app, "n");
    assert!(app.upcoming.is_none() && app.now.as_ref().is_some_and(|n| n.opening));
}

/// Ejecting a drive stops playing from it, the track opened ahead too; another drive's
/// track plays on.
#[test]
fn ejecting_lets_go_of_the_drive() {
    let (dir, mut app, _out) = playing_wavs(&[8000, 8000]);
    wait(&mut app, |a| a.now.as_ref().is_some_and(|n| !n.opening));
    app.let_go_of(Path::new("/Volumes/ELSEWHERE"));
    assert!(app.now.is_some(), "not this drive's");
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    app.let_go_of(&root);
    assert!(app.now.is_none() && app.upcoming.is_none());
    assert!(!app.audio.as_ref().unwrap().is_loaded());
    drop(dir);
}

/// Space while a track is still opening (a slow drive) isn't lost: the track opens paused, or
/// playing after a second press.
#[test]
fn space_while_opening_says_how_the_track_starts() {
    let (_dir, mut app, _out) = playing_wavs(&[8000, 8000]);
    press(&mut app, " ");
    assert!(app.now.as_ref().is_some_and(|n| n.opening) && !app.playback().1, "shown paused");
    opened(&mut app);
    assert!(app.audio.as_ref().unwrap().is_loaded() && !app.is_playing());
    press(&mut app, "n");
    press(&mut app, "  ");
    opened(&mut app);
    assert!(app.is_playing());
}

/// Single play: nothing's opened ahead, and at the end of a track the player stops, with the
/// next one ready for space.
/// Full screen, the best match is picked as the matches arrive; ↑↓ pick another and enter
/// queues it to play next, marked in the list. S turns autoplay off there too.
#[test]
fn full_screen_queues_a_match_to_play_next() {
    let (_dir, mut app, _out) = playing_wavs(&[8000, 8000, 8000]);
    wait(&mut app, |a| a.now.as_ref().is_some_and(|n| !n.loading));
    let mix = |index| Mix { index, key: Camelot::parse("8A").unwrap(), relation: Relation::Same, tempo_gap: 0.0 };
    let (library, root) = (app.queue.library.clone(), app.queue.root.clone());
    let id = app.now.as_ref().unwrap().id;
    let info =
        TrackInfo { matches: Some(Matches { library, root, items: vec![mix(2), mix(1)] }), ..Default::default() };
    app.handle_msg(Msg::TrackInfo(id, Box::new(info)));
    app.fullscreen = true;
    assert_eq!(app.next_pick.selected(), Some(0));
    press(&mut app, "j\n");
    assert_eq!(last_message(&app), "next: DJ - Track 2");
    press(&mut app, "k\n");
    let order: Vec<&str> = app.queue.order.iter().map(|&i| app.queue.track(i).title.as_str()).collect();
    assert_eq!(order, ["Track 1", "Track 3", "Track 2", "Track 2", "Track 3"]);
    let screen = super::super::ui::screen(&mut app, 200, 50);
    assert!(screen.contains("Track 3 next"), "{screen}");
    let hints = super::super::keymap::hints(&app, 200);
    assert!(hints.iter().any(|h| h.1 == "play next") && hints.iter().any(|h| h.1 == "autoplay"), "{hints:?}");
    press(&mut app, "S");
    assert_eq!(last_message(&app), "autoplay off: stops after each track");
    press(&mut app, "n");
    assert_eq!(app.now.as_ref().unwrap().track.title, "Track 3");
}

#[test]
fn single_play_stops_after_each_track() {
    let (_dir, mut app, mut out) = playing_wavs(&[8000, 8000]);
    press(&mut app, "S");
    wait(&mut app, |a| a.now.as_ref().is_some_and(|n| !n.opening));
    app.tick();
    assert!(app.upcoming.is_none());
    (&mut out).take(4100).for_each(drop);
    wait(&mut app, |a| a.now.is_none());
    let (t, at) = app.resumable().expect("ready");
    assert_eq!((t.title.as_str(), at, app.queue.pos), ("Track 2", Duration::ZERO, 1));
    assert!(super::super::ui::screen(&mut app, 120, 40).contains("single"));
    // Space plays it.
    press(&mut app, " ");
    assert!(app.now.as_ref().is_some_and(|n| n.track.title == "Track 2"));
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn title(app: &App) -> Option<String> {
    app.now.as_ref().map(|n| n.track.title.clone())
}

/// The output going away (unplugged headphones): it's let go of, with the track's file, and
/// the track waits where it was; space plays on through the output there is now.
#[test]
fn losing_the_output_lets_go_and_plays_on_through_another() {
    let (_dir, mut app, _out) = playing_wavs(&[8000, 8000]);
    app.outputs = super::super::audio::tests::OUTPUTS;
    opened(&mut app);
    app.seek_to(ms(100));
    app.audio.as_ref().unwrap().stream_failed("the device is gone", true);
    app.tick();
    assert!(app.audio.is_none(), "let go of");
    assert!(app.toast.as_ref().is_some_and(|t| t.0.contains("the device is gone")), "{:?}", app.toast);
    assert_eq!((title(&app), app.position(), app.is_playing()), (Some("Track 1".into()), ms(100), false));
    // A seek meanwhile moves where it waits.
    app.seek_to(ms(200));
    assert_eq!(app.position(), ms(200));
    press(&mut app, " ");
    assert!(app.audio.is_some(), "opened again");
    opened(&mut app);
    assert_eq!((title(&app), app.position(), app.is_playing()), (Some("Track 1".into()), ms(200), true));
    // A glitch is only said.
    app.audio.as_ref().unwrap().stream_failed("underrun", false);
    app.tick();
    assert!(app.audio.is_some() && app.is_playing());
}

/// A key pressed as the player goes on to the next track, before a tick has caught up with
/// it, acts on the track playing: a seek isn't lost on the one that ended.
#[test]
fn a_seek_as_the_next_track_starts_moves_that_one() {
    let (_dir, mut app, mut out) = playing_wavs(&[8000, 8000]);
    wait(&mut app, |a| a.upcoming.as_ref().is_some_and(|u| u.queued));
    // Track 1 plays out (half a second at 8 kHz) and track 2 starts, with no tick since.
    (&mut out).take(4100).for_each(drop);
    key(&mut app, KeyCode::Right);
    app.tick();
    assert_eq!((title(&app), app.position()), (Some("Track 2".into()), ms(250)));
}

/// Picking another output, the track carries on as it was: paused or playing, where it was
/// (or was going, while it opens), with its loop; a recorded set too, and the queue stays.
#[test]
fn switching_outputs_keeps_the_track_as_it_was() {
    use super::super::deck::{Loop, set_loop};
    let (dir, mut app, _out) = playing_wavs(&[8000, 8000]);
    app.outputs = super::super::audio::tests::OUTPUTS;
    let switch = |app: &mut App, down: usize| {
        press(app, "d");
        outputs_listed(app);
        for _ in 0..down {
            key(app, KeyCode::Down);
        }
        press(app, "\n");
    };
    opened(&mut app);
    let looped = Loop { start: 100.0, end: 300.0, on: true };
    set_loop(&mut app, Some(looped));
    app.seek_to(ms(150));
    press(&mut app, " ");
    switch(&mut app, 1);
    assert_eq!(app.audio.as_ref().and_then(|a| a.device()), Some("Phones"));
    opened(&mut app);
    let state = |app: &App| (title(app), app.position(), app.is_playing(), app.deck.looped);
    assert_eq!(state(&app), (Some("Track 1".into()), ms(150), false, Some(looped)));

    // Switched while the next track opens: it opens playing, from where it was to start.
    press(&mut app, "n");
    app.seek_to(ms(120));
    switch(&mut app, 0);
    opened(&mut app);
    assert_eq!(state(&app), (Some("Track 2".into()), ms(120), true, None));

    // A recorded set, paused: still paused, and the queue is still there for afterwards.
    let path = dir.path().join("Contents/1.wav");
    let mut set = track(9, "DJ", "Set");
    set.file_path = path.display().to_string();
    assert!(app.play_file(&path, set, ms(50)));
    opened(&mut app);
    press(&mut app, " ");
    switch(&mut app, 1);
    opened(&mut app);
    assert_eq!((title(&app), app.position(), app.is_playing()), (Some("Set".into()), ms(50), false));
    assert_eq!((app.queue.len(), app.queue.pos), (2, 1));
}

/// A file with no audio in it (only a header) plays nothing and ends at once: it's said, and
/// with repeat one it isn't opened again and again.
#[test]
fn a_track_with_no_audio_is_skipped_not_replayed() {
    let (_dir, mut app, mut out) = playing_files(&[(8000, 0)]);
    app.repeat = Repeat::One;
    // Played through an output, which keeps asking for samples.
    let mut play = |app: &mut App| {
        (&mut out).take(100).for_each(drop);
        std::thread::sleep(Duration::from_millis(5));
        app.tick();
    };
    let t = Instant::now();
    while app.now.as_ref().is_some_and(|n| n.opening) && t.elapsed() < Duration::from_secs(10) {
        play(&mut app);
    }
    while app.now.is_some() && t.elapsed() < Duration::from_secs(10) {
        play(&mut app);
    }
    assert!(last_message(&app).ends_with("there's no audio in it"), "{}", last_message(&app));
    for _ in 0..20 {
        play(&mut app);
    }
    assert!(app.now.is_none(), "not opened again");
}

/// A track that breaks while it plays (playing it panicked on the audio thread) is said, as
/// one that won't open is.
#[test]
fn a_track_that_breaks_while_playing_is_said() {
    let (_dir, mut app, mut out) = playing_wavs(&[8000]);
    opened(&mut app);
    app.audio.as_ref().unwrap().break_playing("a bad frame");
    (&mut out).take(100).for_each(drop);
    wait(&mut app, |a| a.now.is_none());
    let said = last_message(&app);
    assert!(said.contains("Track 1: a bad frame"), "{said}");
}

/// Holding `n` asks for one track after another: only the last is opened (the drive isn't
/// read for the others).
#[test]
fn starting_tracks_quickly_opens_only_the_last() {
    let (_dir, mut app, _out) = playing_wavs(&[8000; 6]);
    let opener = app.opener_idle();
    let busy = opener.lock().unwrap();
    // The first track may have been opened before that: only what the keys ask for counts.
    while app.rx.try_recv().is_ok() {}
    press(&mut app, "nnnnn");
    drop(busy);
    let last = app.now.as_ref().unwrap().id;
    let mut opened = Vec::new();
    while let Ok(msg) = app.rx.recv_timeout(Duration::from_secs(5)) {
        if let Msg::Opened(id, _) = msg {
            opened.push(id);
        }
        if opened.contains(&last) {
            break;
        }
    }
    assert_eq!(opened, [last]);
}

/// Tracks asked for one after another while one is being opened: only the last started is
/// read (the next track, opened ahead, isn't dropped by it), and a panic in one is sent as
/// its failure.
#[test]
fn the_opener_reads_only_the_latest_start() {
    let (tx, rx) = channel();
    let mut opener = Opener::default();
    let read = Arc::new(Mutex::new(Vec::new()));
    let busy = opener.idle.clone();
    let held = busy.lock().unwrap();
    for (id, starts) in [(1, true), (2, true), (3, false), (4, true), (5, false)] {
        let read = read.clone();
        opener.open(
            &tx,
            id,
            starts,
            Box::new(move |o: &Opening| {
                read.lock().unwrap().push(id);
                o.send(Msg::Note(id.to_string(), Level::Info));
            }),
        );
    }
    opener.open(&tx, 6, false, Box::new(|_: &Opening| panic!("no such file")));
    drop(held);
    let mut said = Vec::new();
    while said.len() < 4 {
        match rx.recv_timeout(Duration::from_secs(5)).expect("sent") {
            Msg::Note(id, _) => said.push(id),
            Msg::Opened(6, Err(e)) => said.push(e),
            Msg::TrackInfo(6, _) => said.push("info".into()),
            _ => {}
        }
    }
    assert_eq!(*read.lock().unwrap(), [4, 5]);
    assert_eq!(said[..2], ["4", "5"]);
    assert!(said[2].contains("no such file") && said[3] == "info", "{said:?}");
}

/// A panic on one of the library's own threads (a scoped worker) is said as it was, rather
/// than as the worker that started it re-raises it (`probe worker panicked`).
#[test]
fn a_panic_on_a_library_thread_is_said_as_it_was() {
    let said = caught(|| {
        // What the panic hook notes, on each thread.
        std::thread::scope(|s| s.spawn(|| note("no sample rate (probe.rs:1)".into())).join().unwrap());
        note("probe worker panicked".into());
        panic!("probe worker panicked");
    })
    .unwrap_err();
    assert!(said.ends_with(": no sample rate (probe.rs:1)"), "{said}");
    let said = caught(|| panic!("on its own")).unwrap_err();
    assert!(said.ends_with(": on its own"), "the last one isn't said again: {said}");
}

/// A worker that panics (a bug, or a file the library trips over) is said, and what waits
/// for it stops waiting: here, the job the plan was for.
#[test]
fn a_worker_that_panics_says_so_and_lets_go() {
    let (mut app, _dir) = app_with(library());
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    app.plan(root, |_| panic!("the plan fell over"));
    assert!(app.job.is_some());
    wait(&mut app, |a| a.job.is_none());
    assert!(last_message(&app).contains("the plan fell over"), "{}", last_message(&app));
    app.perform(Act::Check, None);
    assert!(app.job.is_some(), "the next job can start");
}

/// The volumes are polled once at a time: a mount whose `stat` hangs doesn't pile them up.
#[test]
fn one_volume_poll_at_a_time() {
    let (mut app, _dir) = app_with(library());
    app.volumes_in_flight = true;
    let due = Instant::now() - Duration::from_secs(5);
    app.last_volume_poll = due;
    app.tick();
    assert_eq!(app.last_volume_poll, due, "not polled again");
    app.handle_msg(Msg::Volumes(Vec::new(), None));
    app.tick();
    assert!(app.volumes_in_flight && app.last_volume_poll > due);
}

/// A poll of the volumes that keeps failing is said once, not every second, until one works.
#[test]
fn a_failing_volume_poll_is_said_once() {
    let (mut app, _dir) = app_with(library());
    let said = |app: &App| app.log.iter().filter(|(m, ..)| m == "the watcher fell over").count();
    for _ in 0..3 {
        app.handle_msg(Msg::Failed(Work::Volumes, "the watcher fell over".into()));
    }
    assert_eq!(said(&app), 1);
    app.handle_msg(Msg::Volumes(Vec::new(), None));
    app.handle_msg(Msg::Failed(Work::Volumes, "the watcher fell over".into()));
    assert_eq!(said(&app), 2);
}

/// A details lookup that failed (it panicked on the track) isn't started again while that
/// track stays selected: the panel says so, and the screen stops redrawing at animation
/// rate. Another track is looked up as usual.
#[test]
fn a_failed_details_lookup_is_not_asked_again() {
    let (mut app, _dir) = app_with(library());
    app.show_details = true;
    let generation = app.loaded.as_ref().unwrap().generation;
    let settled = |app: &mut App, row: usize| {
        app.track_state.select(Some(row));
        let i = app.selected_track().unwrap();
        app.details_seen = Some((generation, i, Instant::now() - Duration::from_secs(1)));
        i
    };
    let i = settled(&mut app, 0);
    app.want_details();
    assert_eq!(app.details_asked, Some((generation, i)));
    app.handle_msg(Msg::Failed(Work::Details(generation, i), "it fell over".into()));
    app.want_details();
    assert_eq!(app.details_asked, None, "not asked again");
    assert!(!app.animating());
    let s = super::super::ui::screen(&mut app, 140, 30);
    assert!(s.contains("couldn't look for matches"), "{s}");

    let j = settled(&mut app, 1);
    app.want_details();
    assert_eq!(app.details_asked, Some((generation, j)));
}

/// A plan (or a task's results) ready while another dialog is open waits for it to close:
/// what's typed there stays there, and the confirmation still ignores keys at first.
#[test]
fn a_plan_waits_for_the_dialog_open() {
    let (_dir, root) = demo_drive();
    let device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    let mut app = App::blank(root.join("store"), None);
    app.install(root.clone(), Loaded::prepare(device, lib));
    app.track_state.select(Some(1));
    press(&mut app, "e4");
    press(&mut app, "eN");
    wait(&mut app, |a| a.job.is_none());
    assert!(matches!(app.modal, Some(Modal::Prompt(_))), "the prompt stays");
    press(&mut app, "Friday y");
    let Some(Modal::Prompt(p)) = &app.modal else { panic!("no prompt") };
    assert!(p.text.ends_with("Friday y"), "{}", p.text);
    press(&mut app, "\u{1b}");
    app.tick();
    assert!(matches!(app.modal, Some(Modal::Confirm(_))), "then the plan");
    just_shown(&mut app);
    press(&mut app, "y");
    assert!(matches!(app.modal, Some(Modal::Confirm(_))) && app.job.is_none(), "keys right as it appears are ignored");

    // Behind a search being typed too.
    press(&mut app, "\u{1b}");
    press(&mut app, "e5/demo");
    wait(&mut app, |a| a.job.is_none());
    assert!(app.searching && app.modal.is_none() && app.query == "demo");
    press(&mut app, "\n");
    app.tick();
    assert!(matches!(app.modal, Some(Modal::Confirm(_))));
}

/// The Drives view's menu acts on the drive it was opened for, whichever is selected when
/// an item is chosen, and refuses once it's unplugged. Its plans say what they do.
#[test]
fn the_drive_menu_acts_on_its_own_drive() {
    let (dir, root) = demo_drive();
    let other = dir.path().join("OTHER");
    std::fs::create_dir_all(&other).unwrap();
    let device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    let mut app = App::blank(dir.path().join("store"), None);
    app.install(root.clone(), Loaded::prepare(device, lib));
    let (a, b) = (Device::open(&root).unwrap().info(), Device::open(&other).unwrap().info());
    app.drives = vec![a, b.clone()];
    app.drive_state.select(Some(0));
    press(&mut app, "2e");
    app.drive_state.select(Some(1));
    press(&mut app, "\n");
    let (op, _) = confirming(&mut app);
    let Some(Modal::Confirm(c)) = &app.modal else { panic!() };
    assert_eq!((op.as_str(), c.root.as_path()), ("pull", root.as_path()));
    let s = super::super::ui::screen(&mut app, 100, 30);
    assert!(s.contains("Save DEMO in the store?") && s.contains("y save") && s.contains("DEMO: save it"), "{s}");
    press(&mut app, "\u{1b}");

    app.drive_state.select(Some(0));
    press(&mut app, "e");
    app.handle_msg(Msg::Drives(vec![b]));
    press(&mut app, "\n");
    assert!(app.job.is_none() && app.modal.is_none());
    assert_eq!(last_message(&app), "DEMO isn't plugged in any more");
}

/// The drive menu and `u` know their drive by its identity: another drive mounted at its path
/// (two sticks with one volume name) isn't taken for it, and the drive mounted elsewhere is
/// followed there.
#[test]
fn the_drive_menu_and_u_know_their_drive_by_identity() {
    let (dir, root) = demo_drive();
    let (_moved_dir, moved) = demo_drive();
    let device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    let mut app = App::blank(dir.path().join("store"), None);
    app.install(root.clone(), Loaded::prepare(device, lib));
    let ours = DeviceInfo { id: Some(uuid::Uuid::new_v4()), ..Device::open(&root).unwrap().info() };
    let stranger = DeviceInfo { id: Some(uuid::Uuid::new_v4()), ..ours.clone() };
    app.drives = vec![ours.clone()];
    app.drive_state.select(Some(0));
    press(&mut app, "2e");
    app.handle_msg(Msg::Drives(vec![stranger.clone()]));
    press(&mut app, "\n");
    assert!(app.job.is_none() && app.modal.is_none());
    assert_eq!(last_message(&app), "DEMO isn't plugged in any more");

    let elsewhere = DeviceInfo { root: moved.clone(), ..ours.clone() };
    app.handle_msg(Msg::Drives(vec![ours.clone()]));
    press(&mut app, "e");
    app.handle_msg(Msg::Drives(vec![elsewhere.clone()]));
    press(&mut app, "\n");
    let (op, _) = confirming(&mut app);
    let Some(Modal::Confirm(c)) = &app.modal else { panic!() };
    assert_eq!((op.as_str(), c.root.as_path()), ("pull", moved.as_path()));
    press(&mut app, "\u{1b}");

    app.last_backup = Some(app.drive_at(moved.clone()));
    app.handle_msg(Msg::Drives(vec![ours]));
    assert_eq!(app.last_backup.as_ref().map(|k| k.root.as_path()), Some(root.as_path()), "followed");
    app.handle_msg(Msg::Drives(vec![stranger]));
    assert_eq!(app.last_backup, None, "not the stranger");
}

/// The Drives view says which drive `u` undoes on: the one changed last this run, else the
/// one selected. The drive menu's own undo (of its drive) shows the `u` key only when they're
/// the same.
#[test]
fn u_says_which_drive_it_undoes_on() {
    let (dir, root) = demo_drive();
    let other = dir.path().join("OTHER");
    std::fs::create_dir_all(&other).unwrap();
    let mut app = App::blank(dir.path().join("store"), None);
    let named = |root: &Path, name: &str| DeviceInfo { name: Some(name.into()), ..Device::open(root).unwrap().info() };
    app.drives = vec![named(&root, "Main"), named(&other, "Set")];
    press(&mut app, "2");
    app.drive_state.select(Some(0));
    let undo_key = |app: &mut App| {
        press(app, "e");
        let Some(Modal::Pick(p)) = &app.modal else { panic!("no menu") };
        let key = p.items.iter().find(|i| i.label == "undo the last change").unwrap().detail.clone();
        press(app, "\u{1b}");
        key
    };
    let s = super::super::ui::screen(&mut app, 140, 40);
    assert!(s.contains("undo the last change to Main"), "{s}");
    assert_eq!(undo_key(&mut app), "u");

    app.last_backup = Some(app.drive_at(app.drives[1].root.clone()));
    let s = super::super::ui::screen(&mut app, 140, 40);
    assert!(s.contains("undo the last change to Set"), "{s}");
    assert_eq!(undo_key(&mut app), "", "u undoes on Set");
}

/// After packing onto another drive from the library, `u` undoes the pack, on that drive
/// (not the one open, where the crate was saved as a playlist first). Once that drive has
/// gone, `u` undoes on the open one again.
#[test]
fn u_after_packing_undoes_on_the_drive_packed_onto() {
    let (dir, root) = demo_drive();
    let (_set_dir, target) = demo_drive();
    let device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    let mut app = App::blank(dir.path().join("store"), None);
    app.install(root.clone(), Loaded::prepare(device, lib));
    app.drives = vec![Device::open(&root).unwrap().info(), Device::open(&target).unwrap().info()];
    press(&mut app, "Ma");
    app.perform(Act::Pack, None);
    let Some(Modal::Pick(p)) = &mut app.modal else { panic!("no choice") };
    let at = p.items.iter().position(|i| i.label.starts_with("the crate's")).expect("the crate");
    p.state.select(Some(at));
    press(&mut app, "\n\n");
    press(&mut app, "\n");
    say_yes(&mut app);
    let (op, _) = confirming(&mut app);
    assert_eq!(op, "pack");
    let said = say_yes(&mut app);
    assert!(said.ends_with("u to undo"), "{said}");
    press(&mut app, "u");
    let (op, _) = confirming(&mut app);
    let Some(Modal::Confirm(c)) = &app.modal else { panic!() };
    assert_eq!((op.as_str(), c.root.as_path()), ("undo", target.as_path()));

    press(&mut app, "\u{1b}");
    app.handle_msg(Msg::Drives(vec![Device::open(&root).unwrap().info()]));
    press(&mut app, "u");
    let (op, _) = confirming(&mut app);
    let Some(Modal::Confirm(c)) = &app.modal else { panic!() };
    assert_eq!((op.as_str(), c.root.as_path()), ("undo", root.as_path()));
}

#[test]
fn line_editing() {
    let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
    let mut s = String::new();
    for c in "new folder ".chars() {
        edit_line(&mut s, key(KeyCode::Char(c)));
    }
    edit_line(&mut s, KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
    assert_eq!(s, "new ");
    assert!(edit_line(&mut s, key(KeyCode::Backspace)));
    assert_eq!(s, "new");
    assert!(edit_line(&mut s, KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)));
    assert!(!edit_line(&mut s, key(KeyCode::Backspace)));
}

#[test]
fn drives_plugged_in_while_running() {
    let (mut app, _dir) = app_with(library());
    // Reads of the drive list are asked for, never made (one is "in flight" when it matters).
    app.offline = false;
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = (Device::open(a.path()).unwrap().info(), Device::open(b.path()).unwrap().info());
    let toast = |app: &mut App| app.toast.take().map(|t| t.0);
    toast(&mut app);

    // The first list is what was there at startup: nothing to announce.
    app.handle_msg(Msg::Drives(vec![a.clone()]));
    assert_eq!(toast(&mut app), None);
    assert!(app.listed);
    // A poll finding nothing mounted or unmounted reads nothing again, nor does a drive
    // changing.
    let mounted = |d: &DeviceInfo| locallibrary::watch::MountedDrive {
        root: d.root.clone(),
        id: None,
        name: d.volume_name.clone(),
        has_export: false,
    };
    app.handle_msg(Msg::Volumes(Vec::new(), None));
    let changed = WatchEvent::Changed { drive: mounted(&a), changed: vec![locallibrary::watch::DriveChange::State] };
    app.handle_msg(Msg::Volumes(vec![changed], None));
    assert!(!app.drives_in_flight);

    app.handle_msg(Msg::Drives(vec![a.clone(), b.clone()]));
    assert_eq!(toast(&mut app), Some(format!("{} plugged in", app.drive_label(&b.root))));
    app.handle_msg(Msg::Drives(vec![a.clone()]));
    assert_eq!(toast(&mut app), None, "unplugging isn't announced here");

    // Asked for while a read runs: read again when it's done, rather than dropped. A drive
    // plugged in asks for one.
    app.drives_in_flight = true;
    app.handle_msg(Msg::Volumes(vec![WatchEvent::Attached(mounted(&b))], None));
    assert!(app.drives_again);
    app.drives_again = false;
    app.refresh_drives();
    assert!(app.drives_again);
}

/// What an input box shows of a typed line fits its columns, is the end of the line, and is
/// as much of it as fits (Bombadil found the cursor pushed out of the box by wide text).
#[hegel::test(test_cases = 500)]
fn typed_tail_fits_and_keeps_the_end(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use unicode_width::UnicodeWidthStr;
    let text: String =
        tc.draw(hegel::one_of!(gs::text().max_size(40), gs::text().alphabet("ab 日本😹\u{301}\u{fe0f}").max_size(40),));
    let columns = tc.draw(gs::integers::<usize>().max_value(30));
    let shown = typed_tail(&text, columns);
    assert!(text.ends_with(shown));
    assert!(shown.width() <= columns, "{shown:?} is {} columns wide, more than {columns}", shown.width());
    if let Some((i, c)) = text[..text.len() - shown.len()].char_indices().next_back() {
        assert!(text[i..].width() > columns, "{c:?} would have fit too");
    }
}

fn selected_title(app: &App) -> Option<String> {
    app.selected_track().map(|i| app.loaded.as_ref().unwrap().library.tracks[i].title.clone())
}

/// A click selects the row under it, in whichever pane, and a second click there plays it; a
/// column header sorts by it; the wheel scrolls the pane it's over; the tabs switch views.
#[test]
fn clicks_select_play_sort_and_switch_views() {
    let (mut app, _dir) = app_with(library());
    let row = spot(&mut app, "Tell You No Lie");
    mouse(&mut app, LEFT, row);
    assert_eq!(selected_title(&app).as_deref(), Some("Tell You No Lie"));
    // Clicked again: played, with the list queued from there (there's no output here to
    // play it through, which it says).
    quick_again(&mut app);
    mouse(&mut app, LEFT, row);
    assert_eq!((&app.queue.items, app.queue.pos), (&app.visible, 2));
    assert!(last_message(&app).starts_with("can't play"), "{}", last_message(&app));

    point(&mut app, LEFT, "BPM");
    assert_eq!(app.sort, Some(SortKey { field: SortField::Bpm, descending: false }));
    assert_eq!(titles(&app), ["Around the World", "Around the World", "Tell You No Lie", "Never Grow Old"]);
    assert_eq!(selected_title(&app).as_deref(), Some("Tell You No Lie"), "still selected");

    point(&mut app, MouseEventKind::ScrollDown, "♫ All tracks");
    assert_eq!((app.focus, app.playlist_state.selected()), (Focus::Playlists, Some(2)));
    point(&mut app, MouseEventKind::ScrollUp, "Tell You No Lie");
    assert_eq!((app.focus, selected_title(&app).as_deref()), (Focus::Tracks, Some("Around the World")));

    point(&mut app, LEFT, "2 Drives");
    assert_eq!(app.view, View::Drives);
    point(&mut app, LEFT, "1 Library");
    assert_eq!(app.view, View::Library);
}

/// A second click is one on the row the first was on: a click where another view's row was just
/// clicked only selects, and so does a third click on a row.
#[test]
fn a_second_click_is_on_the_same_row() {
    let (mut app, _dir) = app_with(library());
    let info = Device::open(app.loaded.as_ref().unwrap().root()).unwrap().info();
    app.drives = (0..6).map(|i| DeviceInfo { name: Some(format!("D{i}")), ..info.clone() }).collect();
    let sets = spot(&mut app, "Sets");
    press(&mut app, "2");
    super::super::ui::screen(&mut app, 120, 30);
    app.drive_state.select(None);
    mouse(&mut app, LEFT, sets);
    assert!(app.drive_row(&app.drive_rows()).is_some(), "a drive's row clicked");
    press(&mut app, "1");
    super::super::ui::screen(&mut app, 120, 30);
    let tree = |app: &App| app.loaded.as_ref().unwrap().tree.len();
    let rows = tree(&app);
    quick_again(&mut app);
    mouse(&mut app, LEFT, sets);
    let l = app.loaded.as_ref().unwrap();
    let selected = app.playlist_state.selected().map(|i| l.tree[i].name.as_str());
    assert_eq!((app.focus, selected), (Focus::Playlists, Some("Sets")));
    assert_eq!(tree(&app), rows, "the folder left as it was");
    quick_again(&mut app);
    mouse(&mut app, LEFT, sets);
    assert_ne!(tree(&app), rows, "the folder opened or closed");
    mouse(&mut app, LEFT, sets);
    assert_ne!(tree(&app), rows, "a third click only selects");
}

/// A right-click opens the actions menu on what's under the pointer. Its items, and the
/// rating picker's, are clicked; a click outside closes it.
#[test]
fn right_click_opens_the_menu_and_its_items_are_clicked() {
    let (mut app, _dir) = app_with(library());
    point(&mut app, MouseEventKind::Down(MouseButton::Right), "Never Grow Old");
    let Some(Modal::Menu(m)) = &app.modal else { panic!("no menu") };
    assert_eq!(m.target.tracks, ["/Contents/2.mp3"]);
    point(&mut app, LEFT, "add to the crate");
    assert!(app.modal.is_none());
    assert_eq!(app.crate_list.items.iter().map(|t| t.track.title.as_str()).collect::<Vec<_>>(), ["Never Grow Old"]);

    press(&mut app, "e");
    mouse(&mut app, MouseEventKind::Down(MouseButton::Right), (0, 0));
    assert!(app.modal.is_some(), "only the left button closes it");
    mouse(&mut app, LEFT, (0, 0));
    assert!(app.modal.is_none());

    press(&mut app, "e");
    point(&mut app, LEFT, "set the rating");
    assert!(matches!(&app.modal, Some(Modal::Menu(m)) if m.sub.is_some()));
    point(&mut app, LEFT, "no rating");
    assert!(app.modal.is_none());
    assert!(app.job.as_ref().is_some_and(|j| !j.writing), "planning it");
    wait(&mut app, |a| a.job.is_none());
}

/// The help and the messages scroll with the keys and the wheel, and any other key or a click
/// closes them.
#[test]
fn the_help_and_the_messages_scroll_then_close() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "?");
    press(&mut app, "jj ");
    key(&mut app, KeyCode::PageUp);
    key(&mut app, KeyCode::Down);
    press(&mut app, "k");
    assert_eq!((app.help, app.help_scroll), (true, 2));
    mouse(&mut app, MouseEventKind::ScrollDown, (5, 5));
    mouse(&mut app, MouseEventKind::ScrollUp, (5, 5));
    mouse(&mut app, MouseEventKind::ScrollUp, (5, 5));
    assert_eq!(app.help_scroll, 0);
    mouse(&mut app, LEFT, (5, 5));
    assert!(!app.help);
    press(&mut app, "?x");
    assert!(!app.help && app.view == View::Library, "the key only closes it");

    press(&mut app, "!");
    key(&mut app, KeyCode::PageDown);
    press(&mut app, "j");
    key(&mut app, KeyCode::Up);
    assert_eq!((app.show_log, app.log_scroll), (true, 10));
    mouse(&mut app, MouseEventKind::ScrollDown, (5, 5));
    press(&mut app, "k ");
    key(&mut app, KeyCode::PageUp);
    assert_eq!(app.log_scroll, 12);
    mouse(&mut app, MouseEventKind::ScrollUp, (5, 5));
    assert_eq!(app.log_scroll, 9);
    mouse(&mut app, LEFT, (5, 5));
    assert!(!app.show_log);
    press(&mut app, "!q");
    assert!(!app.show_log && !app.quit);
}

/// In the search box: the arrows move through what it finds, enter keeps a filter that
/// doesn't read and says why, and esc (or going to another view) clears it. Sorted again, a
/// filter that can't be met shows nothing.
#[test]
fn the_search_box_moves_keeps_and_clears() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "/around");
    key(&mut app, KeyCode::Down);
    assert_eq!((app.searching, app.track_state.selected()), (true, Some(1)));
    key(&mut app, KeyCode::Up);
    assert_eq!(app.track_state.selected(), Some(0));
    key(&mut app, KeyCode::Esc);
    assert!(!app.searching && app.query.is_empty() && app.visible.len() == 4);

    // A click on another view's tab puts it away, as esc does: keys typed there are that view's.
    press(&mut app, "/around");
    point(&mut app, LEFT, "2 Drives");
    assert!(!app.searching && app.query.is_empty() && app.visible.len() == 4);
    press(&mut app, "1");
    assert_eq!(app.view, View::Library);

    press(&mut app, "/playlist:nothing\n");
    assert!(!app.searching);
    assert!(last_message(&app).contains("playlist \"nothing\""), "{}", last_message(&app));
    press(&mut app, "b");
    assert!(app.visible.is_empty() && app.track_state.selected().is_none());

    // With no drive open there's nothing to search.
    let (mut no_drive, _store) = blank(View::Library);
    press(&mut no_drive, "/x");
    assert!(no_drive.visible.is_empty());
}

/// While a playlist is being picked in the tree, the mouse works only there: clicks and the
/// wheel elsewhere don't take the focus, a double click opens a folder or picks a playlist,
/// and enter on anything else says what to pick.
#[test]
fn picking_a_playlist_with_the_mouse_stays_in_the_tree() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "ep");
    assert!(app.picking.is_some());
    point(&mut app, LEFT, "Never Grow Old");
    point(&mut app, MouseEventKind::ScrollDown, "Never Grow Old");
    point(&mut app, LEFT, "2 Drives");
    assert_eq!((app.focus, app.view), (Focus::Playlists, View::Library));
    assert_eq!(app.playlist_state.selected(), Some(2), "the wheel moved the tree");

    press(&mut app, "kk\n");
    assert_eq!(last_message(&app), "pick a playlist (esc to cancel)");
    let sets = spot(&mut app, "Sets");
    mouse(&mut app, LEFT, sets);
    quick_again(&mut app);
    mouse(&mut app, LEFT, sets);
    let friday = spot(&mut app, "Friday");
    mouse(&mut app, LEFT, friday);
    quick_again(&mut app);
    mouse(&mut app, LEFT, friday);
    assert!(app.picking.is_none() && app.focus == Focus::Tracks);
    assert!(app.job.is_some(), "adding to Friday is planned");
    wait(&mut app, |a| a.job.is_none());
}

/// With no drive open last time here, the drive list arriving opens the first library, read
/// from its lead clone; a library's row selected goes once none of its drives is listed.
#[test]
fn the_first_library_opens_when_listed() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let home = store.create_library("Home").unwrap().id;
    let mut app = App::blank(dir.path().join("store"), None);
    let drive = |name: &str, library_id| {
        let root = dir.path().join(name);
        std::fs::create_dir_all(&root).unwrap();
        DeviceInfo { has_export: true, library_id, ..Device::open(&root).unwrap().info() }
    };
    let loose = drive("A", None);
    app.handle_msg(Msg::Drives(vec![loose.clone(), drive("MAIN", Some(home))]));
    assert_eq!(app.loading.as_deref(), Some("Reading Home…"));
    app.drive_header = Some(DriveRow::Library(home));
    wait(&mut app, |a| a.loading.is_none());
    app.handle_msg(Msg::Drives(vec![loose]));
    assert_eq!(app.drive_header, None);
}

/// The Drives view: a click selects a drive, a second opens it; one with no export says so.
/// Enter on a drive with one reads it and shows it in the library.
#[test]
fn drives_are_picked_with_the_mouse_and_opened() {
    let (_dir, root) = demo_drive();
    let empty = tempfile::tempdir().unwrap();
    let mut app = App::blank(root.join("store"), None);
    let named = |r: &Path, name: &str| DeviceInfo { name: Some(name.into()), ..Device::open(r).unwrap().info() };
    app.drives = vec![named(&root, "Main"), named(empty.path(), "Blank")];
    press(&mut app, "2");
    // Each drive takes two lines: its name, and where it's mounted.
    let blank = spot(&mut app, "Blank");
    mouse(&mut app, LEFT, blank);
    assert_eq!(app.drive_state.selected(), Some(1));
    let (_, main) = spot(&mut app, "Main");
    mouse(&mut app, LEFT, (4, main + 1));
    assert_eq!(app.drive_state.selected(), Some(0), "the line under its name");
    mouse(&mut app, LEFT, blank);
    quick_again(&mut app);
    mouse(&mut app, LEFT, blank);
    assert_eq!((last_message(&app), app.view), ("no rekordbox export on this drive", View::Drives));

    key(&mut app, KeyCode::Up);
    press(&mut app, "\n");
    assert_eq!((app.view, app.focus), (View::Library, Focus::Playlists));
    assert!(app.loading.is_some());
    wait(&mut app, |a| a.loading.is_none());
    assert_eq!(app.loaded.as_ref().map(|l| l.root()), Some(root.as_path()));
    assert_eq!(app.drive_state.selected(), Some(0));
}

/// Clicking the track's overview seeks there; dragging along it too.
#[test]
fn the_overview_seeks_where_clicked_or_dragged() {
    let (mut app, _dir) = playing(TrackAnalysis::default());
    super::super::ui::screen(&mut app, 120, 30);
    let r = app.areas.overview;
    let at = |app: &App| app.pending_seek.map(|d| d.as_secs());
    mouse(&mut app, LEFT, (r.x + r.width / 2, r.y));
    assert!(at(&app).is_some_and(|s| (145..=155).contains(&s)), "half way through five minutes: {:?}", at(&app));
    app.last_seek -= Duration::from_secs(1);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), (r.x, r.y));
    assert!(at(&app).is_some_and(|s| s < 5), "the start: {:?}", at(&app));
    let start = app.pending_seek;
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), (r.x + r.width / 2, 0));
    assert_eq!(app.pending_seek, start, "dragging elsewhere doesn't seek");
    // The outputs listed: a click closes them.
    press(&mut app, "d");
    mouse(&mut app, MouseEventKind::ScrollDown, (r.x, r.y));
    assert!(app.deck.outputs.is_some());
    mouse(&mut app, LEFT, (r.x, r.y));
    assert!(app.deck.outputs.is_none());
}

/// A queue of the test library's tracks on its drive, at `pos`.
fn queued(app: &mut App, pos: usize) {
    let l = app.loaded.as_ref().unwrap();
    app.queue = Queue {
        library: l.library.clone(),
        items: (0..4).collect(),
        root: l.root().to_path_buf(),
        drive: "TEST".into(),
        order: (0..4).collect(),
        pos,
    };
}

/// Shuffle puts the track playing first and the rest in a new order, and back in the list's
/// order when it's turned off; repeat goes off, all, one. The volume, visualiser, full screen
/// and the deck's zoom keep within their bounds.
#[test]
fn playback_modes_and_bounds() {
    let (mut app, _dir) = app_with(library());
    queued(&mut app, 2);
    press(&mut app, "s");
    let mut order = app.queue.order.clone();
    assert_eq!((order[0], app.queue.pos, app.shuffle), (2, 0, true));
    order.sort();
    assert_eq!(order, [0, 1, 2, 3]);
    press(&mut app, "s");
    assert_eq!((app.queue.order.clone(), app.queue.pos), (vec![0, 1, 2, 3], 2));
    assert_eq!(last_message(&app), "shuffle off");
    let repeats: Vec<Repeat> = (0..3)
        .map(|_| {
            press(&mut app, "r");
            app.repeat
        })
        .collect();
    assert_eq!(repeats, [Repeat::All, Repeat::One, Repeat::Off]);

    press(&mut app, &"+".repeat(20));
    assert_eq!(app.volume, 1.5);
    press(&mut app, &"-".repeat(40));
    assert_eq!(app.volume, 0.0);
    press(&mut app, "vvv");
    assert_eq!(app.vis, Vis::Deck);
    press(&mut app, "v");
    assert_eq!(app.vis, Vis::Bars);
    press(&mut app, &"]".repeat(10));
    assert_eq!(app.deck_secs, 1.5);
    press(&mut app, &"[".repeat(20));
    assert_eq!(app.deck_secs, 60.0);
    press(&mut app, "f");
    assert!(app.fullscreen);
    press(&mut app, "\u{1b}");
    assert!(!app.fullscreen, "esc leaves full screen");
}

/// `o` goes to the playing track: selected in the library, showing all tracks when the list
/// shown doesn't have it. On another drive it says so; a recorded set is in Mixes.
#[test]
fn o_goes_to_the_playing_track() {
    let (mut app, _dir) = playing(TrackAnalysis::default());
    queued(&mut app, 0);
    press(&mut app, "/lie\n2");
    press(&mut app, "o");
    assert_eq!((app.view, app.focus, app.query.as_str()), (View::Library, Focus::Tracks, ""));
    assert_eq!(app.selected_track(), Some(0));
    app.track_state.select(Some(2));
    press(&mut app, "o");
    assert_eq!(app.selected_track(), Some(0), "in the list shown");

    let elsewhere = PathBuf::from("/Volumes/ELSEWHERE");
    app.queue.root = elsewhere.clone();
    app.now.as_mut().unwrap().root = elsewhere;
    press(&mut app, "o");
    assert_eq!(last_message(&app), "the track is on another drive");
    app.queue = Queue::default();
    press(&mut app, "o");
    assert_eq!(app.view, View::Mixes, "a recorded set");
}

/// `c` and `C` jump to the next and previous cue; past the last there are no more.
#[test]
fn c_jumps_from_cue_to_cue() {
    let cues = vec![cue(Some(0), 1_000, None), cue(None, 64_000, None)];
    let (mut app, _dir) = playing(TrackAnalysis { cues, ..Default::default() });
    press(&mut app, "c");
    assert_eq!(app.pending_seek, Some(Duration::from_secs(1)));
    // (While the track opens it stays at the start: where it is is set here.)
    app.resume = Some(Duration::from_secs(70));
    press(&mut app, "C");
    assert_eq!(app.pending_seek, Some(Duration::from_secs(64)));
    press(&mut app, "c");
    assert_eq!(last_message(&app), "no more cues");
    assert_eq!(app.pending_seek, Some(Duration::from_secs(64)));
}

/// Enter in the crate plays it from the entry selected, only the entries on that entry's
/// drive (saying so); enter in the details panel plays the matches.
#[test]
fn plays_the_crate_and_the_matches() {
    let (mut app, _dir) = app_with(library());
    let other = track(9, "Elsewhere", "Not Here");
    app.crate_list.add(Path::new("/Volumes/OTHER"), "OTHER", [other]);
    press(&mut app, "jja");
    press(&mut app, "kka");
    press(&mut app, "t");
    app.focus = Focus::Crate;
    app.crate_state.select(Some(2));
    press(&mut app, "\n");
    let queued: Vec<&str> = (0..app.queue.len()).map(|i| app.queue.track(i).title.as_str()).collect();
    assert_eq!((queued, app.queue.pos), (vec!["Tell You No Lie", "Around the World"], 1));
    let said = format!("playing the crate's tracks on {}", app.queue.drive);
    assert!(app.log.iter().any(|(m, ..)| *m == said), "{:?}", app.log);

    app.focus = Focus::Tracks;
    press(&mut app, "ki");
    wait(&mut app, |a| a.details_current());
    press(&mut app, "\t");
    assert_eq!(app.focus, Focus::Details);
    // The actions menu there is for the match selected.
    press(&mut app, "e");
    let Some(Modal::Menu(m)) = &app.modal else { panic!("no menu") };
    let first = app.details.as_ref().unwrap().matches.items[0].index;
    assert_eq!(m.target.tracks, [app.loaded.as_ref().unwrap().library.tracks[first].file_path.clone()]);
    press(&mut app, "\u{1b}");
    let d = app.details.as_ref().unwrap();
    let matches: Vec<usize> = d.matches.items.iter().map(|m| m.index).collect();
    assert!(!matches.is_empty());
    assert_eq!(app.selected_match(), Some(matches[0]));
    press(&mut app, "\n");
    assert_eq!(app.queue.items, matches);
    // Hiding the panel takes the focus back to the tracks.
    press(&mut app, "i");
    assert_eq!(app.focus, Focus::Tracks);
}

/// The snapshots read again keep the one selected while it's there, the last when the list
/// got shorter, and none of none; the first once there are some again.
#[test]
fn the_snapshot_selected_is_one_there_is() {
    use locallibrary::hash::Hash;
    use locallibrary::store::{LibraryRecord, SnapshotInfo};
    let at = chrono::Utc::now();
    let snapshot = |i: usize| SnapshotInfo {
        id: Hash::of(&[i as u8]),
        short_id: format!("snap{i:02}"),
        library_id: uuid::Uuid::nil(),
        parents: Vec::new(),
        created: at,
        message: None,
        source: None,
        fingerprint: Hash::of(b"library"),
        summary: Default::default(),
        files: 0,
        bytes: 0,
        labels: Vec::new(),
        beside: false,
    };
    let record = LibraryRecord { id: uuid::Uuid::nil(), name: "Demo".into(), head: None, created: at, updated: at };
    let history = |n: usize| Msg::History(Ok(vec![(record.clone(), (0..n).map(snapshot).collect())]));
    let (mut app, _dir) = app_with(library());
    press(&mut app, "3");
    app.handle_msg(history(4));
    assert_eq!(app.history_state.selected(), Some(0));
    press(&mut app, "jj");
    app.handle_msg(history(4));
    assert_eq!(app.history_state.selected(), Some(2), "still there");
    app.handle_msg(history(2));
    assert_eq!(app.history_state.selected(), Some(1), "the last");
    assert!(super::super::ui::screen(&mut app, 120, 30).contains("snap01"));
    app.handle_msg(history(0));
    assert_eq!(app.history_state.selected(), None);
    assert!(super::super::ui::screen(&mut app, 120, 30).contains("The store has no snapshots yet."));
    app.handle_msg(history(3));
    assert_eq!(app.history_state.selected(), Some(0));
}

/// What comes back from the background for a read nobody waits for any more changes
/// nothing; what does is shown, and a failure is said.
#[test]
fn background_results_are_shown_or_said() {
    let (mut app, _dir) = app_with(library());
    let generation = app.loaded.as_ref().unwrap().generation;
    app.handle_msg(Msg::Status(Ok(Default::default())));
    assert!(matches!(app.status, Some(Ok(ref v)) if v.is_empty()) && !app.status_loading);
    app.history_state.select(None);
    app.handle_msg(Msg::History(Err("no store yet".into())));
    assert!(matches!(app.history, Some(Err(_))));
    assert_eq!(app.history_state.selected(), None, "no snapshot to select");
    app.handle_msg(Msg::Note("a report was written".into(), Level::Info));
    assert_eq!(last_message(&app), "a report was written");
    app.handle_msg(Msg::Library(app.load, PathBuf::from("/Volumes/GONE"), Err("can't read it".into())));
    assert_eq!(last_message(&app), "can't read it");

    // My Tags that can't be read: a tag filter says so, and matches nothing.
    app.handle_msg(Msg::MyTags(generation, Err("no tags here".into())));
    press(&mut app, "/tag:deep");
    assert_eq!(app.filter_note.as_deref(), Some("can't read My Tags: no tags here"));
    assert!(app.visible.is_empty());
    press(&mut app, "\u{1b}");
    // Sets that can't be read: said, and Browse has none.
    app.handle_msg(Msg::Sets(generation, Err("a broken set book".into())));
    assert_eq!(last_message(&app), "can't read the sets: a broken set book");

    // Another load's index is dropped; a details lookup for a track no longer asked about too.
    let index = Index { ranks: vec![(SortField::Title, vec![0; 4])], groups: FxHashMap::default() };
    app.handle_msg(Msg::Indexed(generation + 1, Box::new(index)));
    assert!(app.loaded.as_ref().unwrap().ranks.is_empty());
    let l = app.loaded.as_ref().unwrap();
    let matches = Matches { library: l.library.clone(), root: l.root().to_path_buf(), items: Vec::new() };
    let details = Details { generation, index: 1, matches, playlists: Vec::new() };
    app.handle_msg(Msg::Details(Box::new(details)));
    assert!(app.details.is_none());
}

/// The drive list arriving with no drive open opens the one open last time (else the first
/// with an export); a read of the list asked for while one ran is made once that one fails.
#[test]
fn the_drive_open_last_time_opens_when_listed() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::blank(dir.path().join("store"), None);
    let drive = |name: &str| {
        let root = dir.path().join(name);
        std::fs::create_dir_all(&root).unwrap();
        DeviceInfo { has_export: true, ..Device::open(&root).unwrap().info() }
    };
    let (a, b) = (drive("A"), drive("B"));
    app.preferred_root = Some(b.root.clone());
    app.status = Some(Ok(Vec::new()));
    app.drives_again = true;
    app.handle_msg(Msg::Drives(vec![a, b]));
    assert!(!app.drives_again, "read again as asked");
    assert_eq!((app.loading.as_deref(), app.drive_state.selected()), (Some("Reading B…"), Some(1)));
    // (An empty folder, read as a drive: it says why it can't be.)
    wait(&mut app, |a| a.loading.is_none());
    assert!(app.loaded.is_none() && !last_message(&app).is_empty());

    app.drives_in_flight = true;
    app.drives_again = true;
    app.handle_msg(Msg::Failed(Work::Drives, "the list fell over".into()));
    assert!(!app.drives_in_flight && !app.drives_again);
    assert_eq!(last_message(&app), "the list fell over");
}

/// The open drive unplugged is said, and reading it is refused; plugged back in, it's read
/// again.
#[test]
fn an_unplugged_drive_is_read_again_when_it_comes_back() {
    let (mut app, _dir) = app_with(library());
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    app.handle_msg(Msg::Volumes(Vec::new(), Some((root.clone(), false))));
    assert!(last_message(&app).ends_with("was unplugged: what's shown may be out of date"));
    press(&mut app, "\n");
    assert_eq!(last_message(&app), "the drive was unplugged");
    assert!(app.now.is_none() && app.queue.is_empty());
    app.handle_msg(Msg::Volumes(Vec::new(), Some((root, true))));
    assert!(app.loading.is_some());
}

/// `R` reads again what the view shows: the drive in the library; the store's snapshots,
/// which without a store say how to make one.
#[test]
fn r_reads_the_view_again() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "R");
    assert!(app.loading.is_some());
    press(&mut app, "3");
    wait(&mut app, |a| a.history.is_some());
    let Some(Err(e)) = &app.history else { panic!("snapshots without a store") };
    assert!(e.starts_with("no store yet"), "{e}");
    app.history = None;
    press(&mut app, "R");
    wait(&mut app, |a| a.history.is_some());
    assert_eq!(app.history_rows(), 0);
    // (The Drives view's status isn't read in tests: it would look at the machine's drives.)
    press(&mut app, "2R");
    assert!(app.status.is_none() && !app.status_loading);
}

/// The store's snapshots in the Snapshots view: moved through with the keys and the mouse.
#[test]
fn the_snapshots_view_lists_the_store() {
    let (dir, root) = demo_drive();
    let store_root = dir.path().join("store");
    let store = Store::open_or_init(&store_root).unwrap();
    let pull = |store: &Store| {
        let req = api::PullRequest { drive: root.display().to_string(), ..Default::default() };
        api::pull(store, &req, &Ctx::silent()).unwrap();
    };
    pull(&store);
    let edit = super::super::edits::create("Warmup", false, &[]);
    let req = api::EditRequest { drive: root.display().to_string(), edits: edit, options: Default::default() };
    api::edit_apply(&req, &Ctx::silent()).unwrap();
    pull(&store);
    let mut app = App::blank(store_root, None);
    press(&mut app, "3\n");
    assert_eq!(last_message(&app), "open the drive to bring a playlist back to in the library first");
    let device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    app.install(root.clone(), Loaded::prepare(device, lib));
    wait(&mut app, |a| a.history.is_some());
    assert_eq!(app.history_rows(), 2);
    press(&mut app, "G");
    assert_eq!(app.history_state.selected(), Some(1));
    press(&mut app, "g");
    assert_eq!(app.history_state.selected(), Some(0));
    let Some(Ok(libs)) = &app.history else { panic!("no snapshots") };
    let ids: Vec<String> = libs[0].1.iter().map(|s| s.short_id[..8].to_string()).collect();
    let second = spot_in(&mut app, &ids[1], |a| a.areas.history);
    mouse(&mut app, LEFT, second);
    assert_eq!(app.history_state.selected(), Some(1));
    mouse(&mut app, MouseEventKind::ScrollUp, second);
    assert_eq!(app.history_state.selected(), Some(0));
    // Enter brings back a playlist from the snapshot selected: named, then planned (here, it's
    // on the drive as it is in the snapshot).
    press(&mut app, "\n");
    let Some(Modal::Prompt(p)) = &app.modal else { panic!("no prompt: {}", last_message(&app)) };
    assert_eq!(p.label, format!("Bring back from {} the playlist or folder", ids[0]));
    press(&mut app, "Warmup\n");
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(last_message(&app), "nothing to change");
}

/// Paging moves a screenful of tracks (as many rows as the list shows), in the search box
/// too, else ten; home and end go to either end. Tab goes round the panes and shift-tab back.
#[test]
fn paging_and_moving_between_panes() {
    let mut lib = library();
    let more = (5..40).map(|id| track(id, "Artist", &format!("Track {id}")));
    lib.tracks.extend(more);
    let (mut app, _dir) = app_with(lib);
    super::super::ui::screen(&mut app, 120, 30);
    let page = app.areas.tracks.height as usize - 3;
    key(&mut app, KeyCode::PageDown);
    assert_eq!(app.track_state.selected(), Some(page));
    key(&mut app, KeyCode::End);
    assert_eq!(app.track_state.selected(), Some(38));
    key(&mut app, KeyCode::PageUp);
    assert_eq!(app.track_state.selected(), Some(38 - page));
    key(&mut app, KeyCode::Home);
    assert_eq!(app.track_state.selected(), Some(0));
    // The same in the search box.
    press(&mut app, "/");
    key(&mut app, KeyCode::PageDown);
    assert_eq!((app.searching, app.track_state.selected()), (true, Some(page)));
    key(&mut app, KeyCode::PageUp);
    assert_eq!(app.track_state.selected(), Some(0));
    key(&mut app, KeyCode::Esc);

    press(&mut app, "it");
    let back = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
    let panes: Vec<Focus> = (0..5)
        .map(|_| {
            app.on_key(back);
            app.focus
        })
        .collect();
    assert_eq!(panes, [Focus::Playlists, Focus::Drives, Focus::Crate, Focus::Details, Focus::Tracks]);
    app.focus = Focus::Playlists;
    key(&mut app, KeyCode::PageDown);
    assert_eq!(app.playlist_state.selected(), Some(2), "ten down, as far as the tree goes");
    // The crate's panel hidden while it has the focus: the focus goes back to the tracks.
    app.focus = Focus::Crate;
    press(&mut app, "t");
    assert_eq!(app.focus, Focus::Tracks);
}

/// `del` in the tree deletes the playlist selected there (planned first); on a list that
/// isn't a playlist it says what to select. In the crate, `w` says when there's nothing from
/// this drive to save.
#[test]
fn del_in_the_tree_and_saving_an_empty_crate() {
    let (mut app, _dir) = app_with(library());
    app.focus = Focus::Playlists;
    press(&mut app, "\u{7f}");
    assert_eq!(last_message(&app), "select a playlist or folder");
    assert!(app.job.is_none());
    let sets = app.loaded.as_ref().unwrap().tree_position(&NodeId::Playlist(1));
    app.playlist_state.select(sets);
    press(&mut app, "\u{7f}");
    assert!(app.job.is_some(), "deleting Sets is planned");
    wait(&mut app, |a| a.job.is_none());

    press(&mut app, "t");
    app.focus = Focus::Crate;
    press(&mut app, "w");
    assert_eq!(last_message(&app), "the crate has nothing from this drive");
}

/// `p` goes back a track, and on the first restarts it; `n` past the last stops, with repeat
/// off. Space with nothing playing plays the selected track; with no output it says why.
#[test]
fn moving_back_and_past_the_end_of_the_queue() {
    let (_dir, mut app, _out) = playing_wavs(&[8000, 8000]);
    opened(&mut app);
    press(&mut app, "n");
    assert_eq!(app.queue.pos, 1);
    press(&mut app, "p");
    assert_eq!((app.queue.pos, title(&app)), (0, Some("Track 1".into())));
    opened(&mut app);
    app.seek_to(ms(300));
    press(&mut app, "p");
    assert_eq!(app.queue.pos, 0, "the first track restarts");
    assert!(app.position() < ms(100), "{:?}", app.position());
    press(&mut app, "nn");
    assert!(app.now.is_none(), "stopped at the end");
    // The volume goes to the output playing.
    press(&mut app, "-");
    assert_eq!((app.volume, app.audio.as_ref().unwrap().volume()), (0.75, 0.75));

    // Nothing playing: space plays the selected track.
    app.track_state.select(Some(1));
    press(&mut app, " ");
    assert_eq!((app.queue.pos, title(&app)), (1, Some("Track 2".into())));

    let (mut silent, _dir) = app_with(library());
    press(&mut silent, " ");
    assert!(last_message(&silent).starts_with("can't play"), "{}", last_message(&silent));
    let track = silent.loaded.as_ref().unwrap().library.tracks[0].clone();
    assert!(!silent.play_file(Path::new("/nowhere/set.wav"), track, Duration::ZERO));
}

/// Space carries on where the last run left off: the track it was on, from where it was.
#[test]
fn space_carries_on_where_the_last_run_left_off() {
    let (_dir, mut app, _out) = playing_wavs(&[8000, 8000]);
    press(&mut app, "x");
    assert!(app.now.is_none());
    app.queue.pos = 1;
    app.resume = Some(ms(200));
    press(&mut app, " ");
    assert_eq!(title(&app), Some("Track 2".into()));
    opened(&mut app);
    assert_eq!(app.position(), ms(200));
}

/// Shuffled, playing a list starts with the track picked and plays the rest in another order.
#[test]
fn shuffled_play_starts_with_the_track_picked() {
    let (_dir, mut app, _out) = playing_wavs(&[8000; 5]);
    app.shuffle = true;
    app.track_state.select(Some(3));
    press(&mut app, "\n");
    assert_eq!((app.queue.order[0], app.queue.pos, title(&app)), (3, 0, Some("Track 4".into())));
    let mut order = app.queue.order.clone();
    order.sort();
    assert_eq!(order, [0, 1, 2, 3, 4]);
}

/// A track that won't open, come to by moving through the queue, is passed over (and said).
#[test]
fn a_track_that_wont_open_is_passed_over() {
    let (dir, mut app, _out) = playing_wavs(&[8000, 8000, 8000]);
    std::fs::remove_file(dir.path().join("Contents/2.wav")).unwrap();
    opened(&mut app);
    press(&mut app, "n");
    wait(&mut app, |a| a.queue.pos == 2);
    opened(&mut app);
    assert_eq!(title(&app), Some("Track 3".into()));
    assert!(app.log.iter().any(|(m, ..)| m.starts_with("DJ - Track 2: ")), "{:?}", app.log);
}

/// Single play turned on while the next track is queued behind the playing one takes it off
/// the queue: the player stops at the end of the track.
#[test]
fn single_play_takes_back_the_track_queued() {
    let (_dir, mut app, mut out) = playing_wavs(&[8000, 8000]);
    wait(&mut app, |a| a.upcoming.as_ref().is_some_and(|u| u.queued));
    press(&mut app, "S");
    assert!(app.upcoming.is_none());
    (&mut out).take(4100).for_each(drop);
    wait(&mut app, |a| a.now.is_none());
    assert_eq!(app.queue.pos, 1, "track 2 ready for space");
}

/// What the player shows and how it plays is remembered for the next run, and saved every
/// couple of seconds as it changes, not only on the way out.
#[test]
fn remembers_the_players_settings_and_saves_them_as_they_change() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let mut app = App::blank(store.clone(), None);
    press(&mut app, "vv*#itQSA");
    app.volume = 0.35;
    app.deck.output = Some("Phones".into());
    app.search_all = true;
    // Not saved until it's been a couple of seconds since the last look.
    app.last_save_check = just_now();
    app.tick();
    assert_eq!(state::load(&store), Saved::default());
    app.last_save_check = Instant::now() - Duration::from_secs(3);
    app.tick();
    let saved = state::load(&store);
    assert_eq!(saved.vis.as_deref(), Some("scope"));

    let mut next = App::blank(store, None);
    next.apply_saved(&saved, false);
    let shown =
        |a: &App| (a.vis, a.show_ratings, a.show_tags_column, a.show_details, a.show_crate, a.search_all, a.volume);
    assert_eq!(shown(&next), (Vis::Scope, true, true, true, true, true, 0.35));
    let deck = |a: &App| (a.deck.quantize, a.deck.single, a.deck.auto_cue, a.deck.output.clone());
    assert_eq!(deck(&next), (false, true, true, Some("Phones".into())));
    assert_eq!(View::from_title("History"), Some(View::Snapshots), "what the Snapshots view was called");
}

/// The queue the last run had waits for its drive; it isn't put back over a track playing,
/// nor when none of its tracks are on the drive any more.
#[test]
fn the_last_runs_queue_waits_for_its_drive() {
    let (mut app, _dir) = app_with(library());
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    let saved = |tracks: Vec<&str>| SavedQueue {
        root: root.clone(),
        drive: "TEST".into(),
        tracks: tracks.into_iter().map(String::from).collect(),
        pos: 0,
        at_ms: 30_000,
    };
    app.restore_queue = Some(saved(vec!["/Contents/gone.mp3"]));
    app.restore_queue(&root);
    assert!(app.queue.is_empty() && app.resume.is_none(), "none of it is on the drive");
    app.restore_queue = Some(saved(vec!["/Contents/2.mp3"]));
    app.restore_queue(Path::new("/Volumes/OTHER"));
    assert!(app.restore_queue.is_some(), "waits for its own drive");
    app.now = playing(TrackAnalysis::default()).0.now.take();
    app.restore_queue(&root);
    assert!(app.queue.is_empty(), "not over a track playing");
}

/// Marks and the crate: `m` does nothing where there's nothing to mark, marks and unmarks a
/// playlist in the tree; adding what's partly in the crate says how much was new; `X` with
/// nothing selected in an empty crate does nothing.
#[test]
fn marking_and_the_crate_at_their_edges() {
    let (mut app, _dir) = app_with(library());
    app.focus = Focus::Details;
    press(&mut app, "m");
    assert!(app.marks.is_empty());
    app.focus = Focus::Tracks;
    press(&mut app, "/no such track\nm");
    assert!(app.marks.is_empty(), "nothing selected");
    press(&mut app, "\u{1b}");
    app.focus = Focus::Playlists;
    press(&mut app, "m");
    assert!(app.list_marks.is_empty(), "All tracks isn't a playlist");
    let sets = app.loaded.as_ref().unwrap().tree_position(&NodeId::Playlist(1));
    app.playlist_state.select(sets);
    press(&mut app, "m");
    app.playlist_state.select(sets);
    press(&mut app, "m");
    assert!(app.list_marks.is_empty(), "marked and unmarked");

    app.focus = Focus::Tracks;
    press(&mut app, "a");
    press(&mut app, "kmm");
    press(&mut app, "a");
    assert_eq!(last_message(&app), "added 1 to the crate (1 already there)");
    press(&mut app, "a");
    assert_eq!(last_message(&app), "already in the crate");
    // In the crate: keys move through it; one taken out leaves the one after it selected.
    press(&mut app, "t");
    app.focus = Focus::Crate;
    app.crate_state.select(Some(0));
    press(&mut app, "j");
    assert_eq!(app.crate_state.selected(), Some(1));
    press(&mut app, "\u{7f}\u{7f}\u{7f}");
    assert!(app.crate_list.is_empty() && app.crate_state.selected().is_none());

    // With nothing playing, shuffle only says it's on.
    press(&mut app, "s");
    assert!(app.shuffle && app.queue.order.is_empty());
    // The details panel with nothing looked up has nothing to move through, or to delete.
    press(&mut app, "i");
    app.focus = Focus::Details;
    press(&mut app, "j\u{7f}");
    assert!(app.job.is_none());
    assert_eq!(app.details_state.selected(), None);
    assert!(app.selected_match().is_none());
    press(&mut app, "\n");
    assert!(app.queue.is_empty(), "no match to play");
}

/// A click in the crate selects its row there; a click on a pane's border selects nothing;
/// a click while a prompt is open leaves it open; a click below a menu's last item leaves
/// the menu open.
#[test]
fn clicks_in_the_crate_on_borders_and_on_dialogs() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "ajajat");
    app.crate_state.select(Some(0));
    let row = spot_in(&mut app, "Tell You No Lie", |a| a.areas.crate_panel);
    mouse(&mut app, LEFT, row);
    assert_eq!((app.focus, app.crate_state.selected()), (Focus::Crate, Some(2)));
    let tracks = app.areas.tracks;
    let selected = app.track_state.selected();
    mouse(&mut app, LEFT, (tracks.x + 5, tracks.y));
    assert_eq!((app.focus, app.track_state.selected()), (Focus::Tracks, selected));

    press(&mut app, "e");
    super::super::ui::screen(&mut app, 120, 30);
    let menu = app.areas.menu;
    mouse(&mut app, LEFT, (menu.x + 3, menu.y + menu.height - 1));
    assert!(matches!(app.modal, Some(Modal::Menu(_))), "below the last item");
    press(&mut app, "c");
    super::super::ui::screen(&mut app, 120, 30);
    let menu = app.areas.menu;
    mouse(&mut app, LEFT, (menu.x + 3, menu.y + menu.height - 1));
    assert!(matches!(&app.modal, Some(Modal::Menu(m)) if m.sub.is_some()));
    press(&mut app, "\u{1b}q");
    press(&mut app, "eN");
    mouse(&mut app, LEFT, (1, 1));
    assert!(matches!(app.modal, Some(Modal::Prompt(_))));
}

/// In the Profile and Mixes views a click on a tab still switches views; in the library,
/// enter on the drive list opens the drive selected (one with no export says so).
#[test]
fn tabs_from_every_view_and_enter_on_the_drive_list() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "5");
    point(&mut app, LEFT, "4 Profile");
    assert_eq!(app.view, View::Profile);
    point(&mut app, LEFT, "1 Library");
    assert_eq!(app.view, View::Library);
    let empty = tempfile::tempdir().unwrap();
    app.drives = vec![Device::open(empty.path()).unwrap().info()];
    app.drive_state.select(Some(0));
    app.focus = Focus::Drives;
    press(&mut app, "\n");
    assert_eq!(last_message(&app), "no rekordbox export on this drive");
    // Tab does nothing in the Drives view, nor del.
    press(&mut app, "2\t\u{7f}");
    assert_eq!((app.view, app.focus), (View::Drives, Focus::Drives));
}

/// The playing track's artwork is read from the drive once it's open.
#[test]
fn the_playing_tracks_artwork_is_read_from_the_drive() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("Contents")).unwrap();
    std::fs::write(dir.path().join("Contents/1.wav"), super::super::audio::tests::wav(8000, 4000, 1000)).unwrap();
    let art = dir.path().join("PIONEER/Artwork/00001/a1.png");
    std::fs::create_dir_all(art.parent().unwrap()).unwrap();
    image::RgbImage::from_pixel(4, 4, image::Rgb([200, 30, 90])).save(&art).unwrap();
    let mut t = track(1, "DJ", "Track 1");
    t.file_path = "/Contents/1.wav".into();
    t.artwork_path = Some("/PIONEER/Artwork/00001/a1.png".into());
    let mut app = App::with_library(dir.path(), Library { tracks: vec![t], ..Default::default() });
    let (audio, _out) = Audio::offline(8000);
    app.audio = Some(audio);
    press(&mut app, "\n");
    wait(&mut app, |a| a.now.as_ref().is_some_and(|n| !n.loading));
    assert!(app.now.as_ref().unwrap().art.is_some());
}

/// Asked to play through an output that's gone, it plays through the default and says so.
#[test]
fn an_output_thats_gone_plays_through_the_default() {
    let (_dir, mut app, _out) = playing_wavs(&[8000]);
    app.audio = None;
    app.outputs = super::super::audio::tests::OUTPUTS;
    app.deck.output = Some("Speakers".into());
    press(&mut app, "\n");
    assert!(app.audio.is_some());
    assert_eq!(last_message(&app), "no output called Speakers: playing through the default output");
}

/// A recorded set played on its own stops at its end; the queue stays for afterwards.
#[test]
fn a_recorded_set_stops_at_its_end() {
    let (dir, mut app, mut out) = playing_wavs(&[8000, 8000]);
    opened(&mut app);
    let mut set = track(9, "DJ", "Set");
    set.duration_secs = 0;
    let path = dir.path().join("Contents/2.wav");
    assert!(app.play_file(&path, set, Duration::ZERO));
    opened(&mut app);
    (&mut out).take(4100).for_each(drop);
    wait(&mut app, |a| a.now.is_none());
    assert_eq!((app.queue.len(), app.queue.pos), (2, 0));
}

/// Crossing a cue as it plays flashes the visualiser in the cue's colour.
#[test]
fn crossing_a_cue_flashes_its_colour() {
    let (_dir, mut app, mut out) = playing_wavs(&[8000]);
    opened(&mut app);
    let mut cue = cue(Some(0), 100, None);
    cue.color = Some([1, 2, 3]);
    app.now.as_mut().unwrap().analysis.cues = vec![cue];
    app.tick();
    assert!(app.flash.is_none());
    (&mut out).take(1600).for_each(drop);
    app.tick();
    // (Just crossed, however long the test took since.)
    let Some(flash) = &mut app.flash else { panic!("no flash") };
    flash.1 = just_now();
    assert_eq!(app.flash().map(|(c, _)| c), Some([1, 2, 3]));
    // It fades: gone a second later.
    app.flash.as_mut().unwrap().1 = Instant::now() - Duration::from_secs(1);
    assert!(app.flash().is_none());
}

/// The messages log keeps the last 200.
#[test]
fn the_log_keeps_the_last_messages() {
    let (mut app, _dir) = app_with(library());
    for i in 0..=LOG {
        app.toast(format!("message {i}"), Level::Info);
    }
    assert_eq!(app.log.len(), LOG);
    assert_eq!(app.log.front().map(|m| m.0.as_str()), Some("message 1"));
}

/// On a big library the search box narrows the list once typing pauses (at the next tick
/// after then), not on every key.
#[test]
fn a_big_librarys_search_waits_for_typing_to_pause() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::with_library(dir.path(), big_library(TYPE_AHEAD as u32 + 1));
    press(&mut app, "/title 4999");
    assert_eq!(app.visible.len(), TYPE_AHEAD + 1, "not yet");
    app.search_due = Some(Instant::now());
    app.tick();
    assert_eq!(titles(&app), ["Title 4999"]);
}

/// An offer to replay edits on a drive that isn't open any more goes away.
#[test]
fn a_replay_offer_goes_with_its_drive() {
    let (mut app, dir) = app_with(library());
    let other = PathBuf::from("/Volumes/OTHER");
    app.replay_offer = Some(ReplayOffer { root: other, drive: "OTHER".into(), edits: 2 });
    let device = Device::open(dir.path()).unwrap();
    app.install(device.root().to_path_buf(), Loaded::prepare(device, library()));
    assert!(app.replay_offer.is_none());
}

/// In the Mixes view the keys for moving go through its recordings, as the wheel does
/// outside its own panes.
#[test]
fn the_mixes_view_moves_through_its_recordings() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "5");
    let recording = |i: usize| super::super::mix::Source {
        path: PathBuf::from(format!("/sets/{i}.wav")),
        name: format!("{i}.wav"),
        place: "sets".into(),
        duration_secs: None,
        when: None,
        folder: None,
    };
    app.mix.sources = (0..15).map(recording).collect();
    app.mix.list.select(Some(0));
    let selected = |app: &App| app.mix.list.selected();
    press(&mut app, "G");
    assert_eq!(selected(&app), Some(14));
    press(&mut app, "g");
    key(&mut app, KeyCode::PageDown);
    assert_eq!(selected(&app), Some(10));
    mouse(&mut app, MouseEventKind::ScrollDown, (0, 0));
    assert_eq!(selected(&app), Some(13));
}

/// What the player remembers (the list shown, the view, the sort, the panels and columns,
/// playback's modes and volume, the crate, the queue), however it was set, reads back as it
/// was saved and is what the next run starts with.
#[hegel::test(test_cases = 50)]
fn what_is_remembered_is_what_the_next_run_starts_with(tc: hegel::TestCase) {
    use hegel::generators as gs;
    const KEYS: &[&str] =
        &["b", "B", "i", "t", "*", "#", "s", "r", "v", "Q", "S", "A", "+", "-", "a", "j", "\n", "2", "3", "1"];
    let (mut app, _dir) = app_with(library());
    // A store off the drive: nothing is saved inside the drive that was open.
    let home = tempfile::tempdir().unwrap();
    app.store_root = home.path().join("store");
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(20)) {
        press(&mut app, tc.draw(gs::sampled_from(KEYS)));
    }
    // Plugins' columns, shown from the palette.
    let shown = ["sound.bpm", "sound.key"].map(locallibrary::query::FieldName::new);
    app.field_columns = shown[..tc.draw(gs::integers::<usize>().max_value(shown.len()))].to_vec();
    let saved = app.saved_state();
    state::save(&app.store_root, &saved);
    let read = state::load(&app.store_root);
    assert_eq!(read, saved);
    let mut next = App::blank(app.store_root.clone(), None);
    next.apply_saved(&read, true);
    if let Some(v) = read.view.as_deref().and_then(View::from_title) {
        next.view = v;
    }
    assert_eq!(next.preferred_root.as_ref(), saved.root.as_ref(), "the drive to open first");
    assert_eq!(next.restore.as_ref().map(|r| &r.1), saved.node.as_ref(), "the list to show on it");
    // Before its drive is read, the next run has all the rest.
    assert_eq!(next.saved_state(), Saved { root: None, node: None, ..saved });
}

/// A track with any analysis (a beat grid, hot and memory cues and loops anywhere, phrases
/// that may run past the grid, waveforms) plays on the deck: its phrases start on their
/// beats and end after, a hot cue pad goes to just where the cue is (a loop's plays it), and
/// the deck draws at any size in every visualiser, full screen or not, as its keys jump,
/// loop and zoom about.
#[hegel::test(test_cases = 15, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_deck_plays_any_analysis(tc: hegel::TestCase) {
    use super::super::ui::screen;
    use hegel::generators as gs;
    let n = tc.draw(gs::integers::<u32>().max_value(64));
    let beats =
        grid(tc.draw(gs::integers::<u32>().max_value(2_000)), tc.draw(gs::sampled_from(&[90.0, 124.0, 174.0][..])), n);
    let cues: Vec<locallibrary::anlz::Cue> = (0..tc.draw(gs::integers::<usize>().max_value(6)))
        .map(|_| {
            let start = tc.draw(gs::integers::<u32>().max_value(310_000));
            let end = tc.draw(gs::optional(gs::integers::<u32>().min_value(1).max_value(20_000))).map(|d| start + d);
            cue(tc.draw(gs::optional(gs::integers::<u8>().max_value(7))), start, end)
        })
        .collect();
    let kinds = [PhraseKind::Intro, PhraseKind::Up, PhraseKind::Down, PhraseKind::Chorus, PhraseKind::Outro];
    // As the reader leaves them (each ends after it starts, though maybe past the grid), and
    // as it never does: on beat 0, or ending where or before it starts.
    let phrases = (0..tc.draw(gs::integers::<usize>().max_value(4)))
        .map(|_| {
            let start_beat = tc.draw(gs::integers::<u16>().max_value(n as u16 + 4));
            let end_beat = match tc.draw(gs::booleans()) {
                true => start_beat + tc.draw(gs::integers::<u16>().min_value(1).max_value(40)),
                false => tc.draw(gs::integers::<u16>().max_value(start_beat)),
            };
            Phrase { kind: tc.draw(gs::sampled_from(&kinds[..])), start_beat, end_beat, fill_beat: None }
        })
        .collect();
    let analysis = TrackAnalysis {
        beats,
        cues,
        preview: tc.draw(gs::booleans()).then(|| wave(400)),
        detail: tc.draw(gs::booleans()).then(|| wave(3_000)),
        structure: tc.draw(gs::booleans()).then_some(SongStructure { mood: Mood::Mid, phrases }),
    };
    let (mut app, _dir) = playing(analysis.clone());
    let now = app.now.as_mut().unwrap();
    now.phrases = phrase_spans(&analysis, now.duration);
    // Only those that start on the grid and end after they start are drawn.
    let placed: Vec<&Phrase> = (analysis.structure.iter().flat_map(|s| &s.phrases))
        .filter(|p| p.start_beat > 0 && (p.start_beat as usize) <= n as usize && p.end_beat > p.start_beat)
        .collect();
    assert_eq!(now.phrases.len(), placed.len(), "{:?}", analysis.structure);
    for (p, span) in placed.iter().zip(&now.phrases) {
        assert_eq!(span.start_ms, analysis.beats[p.start_beat as usize - 1].time_ms as f64);
        assert!(span.start_ms < span.end_ms, "{p:?} at {} to {}", span.start_ms, span.end_ms);
    }
    now.opening = tc.draw(gs::booleans());
    let end = now.duration - Duration::from_millis(250);
    const KEYS: &[&str] = &[
        "1", "2", "3", "4", "5", "6", "7", "8", "c", "C", "N", "P", ",", ".", "<", ">", "{", "}", "l", "(", ")", "Q",
        "v", "f", "[", "]", "'",
    ];
    press(&mut app, "'");
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(12)) {
        let key = tc.draw(gs::sampled_from(KEYS));
        app.pending_seek = None;
        press(&mut app, key);
        let slot = key.parse::<u8>().ok().filter(|_| app.pads_on()).map(|d| d - 1);
        let hot = slot.and_then(|s| analysis.cues.iter().find(|c| c.hot_cue == Some(s)));
        if let (Some(c), true) = (hot, app.now.as_ref().is_some_and(|n| n.opening)) {
            assert_eq!(app.pending_seek, Some(Duration::from_millis(c.start_ms as u64).min(end)), "pad {key}");
            if let Some(e) = c.end_ms {
                let l = app.deck.looped.unwrap();
                assert_eq!((l.start, l.end, l.on), (c.start_ms as f64, e as f64, true), "pad {key}'s loop");
            }
        }
        if let Some(at) = app.pending_seek {
            assert!(at <= end, "a jump to {at:?}, past {end:?}");
        }
        if let Some(l) = app.deck.looped {
            assert!(l.start < l.end, "{l:?}");
        }
        let (w, h) = (
            tc.draw(gs::integers::<u16>().min_value(1).max_value(200)),
            tc.draw(gs::integers::<u16>().min_value(1).max_value(60)),
        );
        let s = screen(&mut app, w, h);
        if w >= 100 && h >= 30 && app.view == View::Library {
            assert!(s.contains("Around the World") && s.contains("BPM"), "the deck at {w}x{h}:\n{s}");
        }
    }
}

/// What a track's drive lacks of its analysis comes from what a plugin's analyzer kept, read with
/// the rest in the background, and the deck says so under the track.
#[test]
fn a_plugins_analysis_fills_in_what_the_drive_lacks() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let heard = "\n[[contributes.analyzers]]\nname = \"heard\"\ntitle = \"Heard\"\nversion = \"1\"\n\
                 fields = { grid = \"beats\", wave = \"waveform\" }\n";
    let manifest = std::fs::read_to_string(dir.path().join("echo.toml")).unwrap();
    std::fs::write(dir.path().join("echo.toml"), manifest + heard).unwrap();
    let device = Device::open_export(&root).unwrap();
    let t = device.library().unwrap().tracks[0].clone();
    // The drive has no analysis for it: rekordbox's files are gone.
    let dat = device.path(&paths::key(&t.analyze_path));
    for ext in ["DAT", "EXT", "2EX"] {
        std::fs::remove_file(dat.with_extension(ext)).unwrap();
    }
    let grid: Vec<Beat> =
        (0..8).map(|i| Beat { beat_number: i % 4 + 1, bpm: 120.0, time_ms: 500 * u32::from(i) }).collect();
    let heard = serde_json::json!({
        "grid": grid,
        "wave": { "blob": "w.bands", "columns_per_sec": 150.0 },
        "blobs": { "w.bands": [10, 20, 30, 40, 50, 60] },
    });
    std::fs::write(device.path(&paths::key(&t.file_path)), heard.to_string()).unwrap();
    let store_root = dir.path().join("store");
    let store = Store::open_or_init(&store_root).unwrap();
    let plugins = crate::plugins::host().unwrap();
    plugins.analyze("echo", "heard", &root.display().to_string(), None, &store, &Ctx::silent()).unwrap();

    let info = track_info(&root, &t, None, &store_root);
    let said = "grid and waveform from echo.heard, not on the drive";
    assert_eq!(info.from_plugins.as_deref(), Some(said));
    assert_eq!(info.analysis.beats, grid);
    let detail = info.analysis.detail.as_ref().unwrap();
    assert_eq!((detail.style, detail.columns.len()), (anlz::WaveformStyle::ThreeBand, 2));
    // Another track, which rekordbox analysed, is all the drive's.
    let other = device.library().unwrap().tracks[1].clone();
    assert_eq!(track_info(&root, &other, None, &store_root).from_plugins, None);

    let (mut app, _played) = playing(info.analysis);
    app.now.as_mut().unwrap().from_plugins = info.from_plugins;
    let s = crate::tui::ui::screen(&mut app, 160, 45);
    assert!(s.contains(said), "{s}");
}
