use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;

use super::super::app::testing::*;
use super::*;

#[test]
fn clips_and_fits() {
    assert_eq!(clip("Friday", 10), "Friday");
    assert_eq!(clip("Friday night", 7), "Friday…");
    let spans = vec![Span::raw("abc"), Span::raw("defgh"), Span::raw("ij")];
    let out: String = fit(spans, 6).iter().map(|s| s.content.to_string()).collect();
    assert_eq!(out, "abcde…");
    assert_eq!(scroll_window(0, Some(12), 10, 40), 3);
    assert_eq!(scroll_window(20, Some(5), 10, 40), 5);
    assert_eq!(scroll_window(35, Some(39), 10, 40), 30);
}

/// The player's times read as the Mixes view's do: to the nearest second.
#[test]
fn times_are_to_the_nearest_second() {
    assert_eq!(clock(59.6), "1:00");
    assert_eq!(clock(3599.5), "1:00:00");
    assert_eq!(clock(-1.0), "0:00");
}

/// A line cut short fits its columns however wide its characters (CJK and emoji take two),
/// keeps as much as fits, and says it was cut.
#[hegel::test(test_cases = 500)]
fn clip_counts_columns(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use unicode_width::UnicodeWidthStr;
    let s: String =
        tc.draw(hegel::one_of!(gs::text().max_size(40), gs::text().alphabet("ab 日本😹\u{301}\u{fe0f}").max_size(40),));
    let max = tc.draw(gs::integers::<usize>().max_value(30));
    let cut = clip(&s, max);
    assert!(cut.width() <= max, "{cut:?} is {} columns wide, more than {max}", cut.width());
    if s.width() <= max {
        assert_eq!(cut, s);
    } else if max > 0 {
        let kept = cut.strip_suffix('…').expect("an ellipsis");
        assert!(s.starts_with(kept));
        if let Some(c) = s[kept.len()..].chars().next() {
            assert!(format!("{kept}{c}").width() >= max, "{c:?} would have fit too");
        }
    }
}

/// The help's descriptions wrap to their column however wide their characters.
#[test]
fn help_wraps_to_its_columns() {
    let sections: Vec<super::super::keymap::HelpSection> =
        vec![("Keys", vec![("x", "日本の 夜明け とともに 始まる 長い 長い セット です")])];
    for line in help_lines(&sections, 40) {
        assert!(line.width() <= 40, "{line:?} is {} columns wide", line.width());
    }
}

#[test]
fn sort_column_shows_even_when_narrow() {
    let k = |field| Some(SortKey { field, descending: true });
    assert_eq!(columns(80, None, false, false), [Col::Title, Col::Artist, Col::Bpm, Col::Key, Col::Time]);
    assert_eq!(columns(80, None, false, true), [Col::Title, Col::Artist, Col::Tags, Col::Bpm, Col::Key, Col::Time]);
    assert_eq!(
        columns(80, k(SortField::Added), false, false),
        [Col::Title, Col::Artist, Col::Added, Col::Bpm, Col::Key, Col::Time]
    );
    assert_eq!(columns(80, k(SortField::Bpm), false, false).len(), 5);
    assert!(!columns(200, None, false, false).contains(&Col::Rating), "ratings hidden unless asked for");
    assert_eq!(columns(150, k(SortField::Rating), false, false).len(), 8, "sorted by: shown");
    assert_eq!(columns(80, None, true, false), [Col::Title, Col::Artist, Col::Rating, Col::Bpm, Col::Key, Col::Time]);
    assert_eq!(columns(80, k(SortField::Rating), true, false).len(), 6, "already shown");
}

#[test]
fn keys_that_mix_show_green() {
    let (mut app, _dir) = app_with(library());
    app.focus = Focus::Tracks;
    // The key column's colour on the row that shows `title`.
    let key_colour = |app: &mut App, title: &str| {
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buf = terminal.backend().buffer();
        let key_x = app.areas.columns.iter().find(|(f, _)| *f == SortField::Key).unwrap().1.x + 1;
        let y = (0..20)
            .find(|&y| (0..100).map(|x| buf[(x, y)].symbol()).collect::<String>().contains(title))
            .unwrap_or_else(|| panic!("{title} not on screen"));
        buf[(key_x, y)].fg
    };
    let green = theme::rgb3(theme::GREEN);
    app.visible = (0..4).collect();
    let row = |app: &App, title: &str| {
        let l = app.loaded.as_ref().unwrap();
        app.visible.iter().position(|&i| l.library.tracks[i].title == title).unwrap()
    };
    // Never Grow Old (Am) selected: 8A is the same key, 3B doesn't mix, and its own row
    // stays as it was.
    app.track_state.select(Some(row(&app, "Never Grow Old")));
    assert_eq!(key_colour(&mut app, "Around the World"), green);
    assert_ne!(key_colour(&mut app, "Tell You No Lie"), green);
    assert_ne!(key_colour(&mut app, "Never Grow Old"), green);
    // Tell You No Lie (3B) selected: nothing else mixes.
    app.track_state.select(Some(row(&app, "Tell You No Lie")));
    assert_ne!(key_colour(&mut app, "Around the World"), green);
    assert_ne!(key_colour(&mut app, "Never Grow Old"), green);
}

/// Without colours nothing is coloured, and what colour said is still there; on a light
/// terminal the text goes dark.
#[test]
fn plain_and_light_terminals() {
    let (mut app, _dir) = app_with(library());
    app.focus = Focus::Tracks;
    // Never Grow Old (Am) selected: the 8A tracks mix with it.
    app.track_state.select(Some(1));
    let draw = |app: &mut App| {
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 30)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        terminal.backend().buffer().clone()
    };
    let row_of = |buf: &ratatui::buffer::Buffer, needle: &str| {
        (0..buf.area.height)
            .find(|&y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().contains(needle))
            .unwrap_or_else(|| panic!("{needle} not on screen"))
    };
    let line =
        |buf: &ratatui::buffer::Buffer, y: u16| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>();

    theme::set_test_mode(Some(theme::Mode::Plain));
    let buf = draw(&mut app);
    theme::set_test_mode(None);
    assert!(buf.content.iter().all(|c| c.fg == Color::Reset && c.bg == Color::Reset), "a colour got through");
    let y = row_of(&buf, "Never Grow Old");
    assert!((0..buf.area.width).any(|x| buf[(x, y)].modifier.contains(Modifier::REVERSED)), "the selection");
    let around = line(&buf, row_of(&buf, "Around the World"));
    assert!(around.contains("⇄8A"), "keys that mix are marked: {around}");
    assert!(around.contains("6"), "track colours by number: {around}");

    theme::set_test_mode(Some(theme::Mode::Light));
    let light = draw(&mut app);
    theme::set_test_mode(None);
    let dark = draw(&mut app);
    let y = row_of(&light, "Never Grow Old");
    let x = (0..light.area.width).find(|&x| light[(x, y)].symbol() == "N").unwrap();
    assert_ne!(light[(x, y)].fg, dark[(x, y)].fg);
}

#[test]
fn help_fits_80_columns_and_scrolls() {
    let (mut app, _dir) = app_with(library());
    app.help = true;
    app.help_scroll = 999;
    let s = screen(&mut app, 80, 24);
    assert!(app.help_scroll > 0 && app.help_scroll < 999, "clamped to the end: {}", app.help_scroll);
    assert!(s.contains("Every command is still available"), "{s}");
}

#[test]
fn draws_every_pane_and_dialog_at_any_size() {
    let (mut app, _dir) = app_with(library());
    app.show_details = true;
    app.show_crate = true;
    app.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    for (w, h) in [(80, 24), (100, 30), (130, 40), (200, 50), (40, 12), (20, 6)] {
        for focus in [Focus::Tracks, Focus::Playlists, Focus::Details, Focus::Crate] {
            app.focus = focus;
            screen(&mut app, w, h);
        }
        app.on_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        screen(&mut app, w, h);
        app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        screen(&mut app, w, h);
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.help = true;
        screen(&mut app, w, h);
        app.help = false;
    }
    // Full screen, with a track playing (made by hand: tests have no audio).
    let l = app.loaded.as_ref().unwrap();
    let track = l.library.tracks[1].clone();
    let matches = super::super::app::Matches::find(l.library.clone(), l.root().to_path_buf(), &track, 40);
    assert_eq!(matches.items.len(), 2, "8A and Am are the same key");
    app.now = Some(super::super::app::NowPlaying {
        id: 0,
        key: super::super::keys::Camelot::parse(&track.key),
        drive: "TEST".into(),
        root: l.root().to_path_buf(),
        analysis: Default::default(),
        art: None,
        duration: std::time::Duration::from_secs(300),
        phrases: Vec::new(),
        matches: Some(matches),
        playlists: vec!["Sets › Friday".into()],
        loading: false,
        opening: false,
        advancing: false,
        track,
    });
    app.fullscreen = true;
    for (w, h) in [(80, 24), (160, 45), (30, 10)] {
        let s = screen(&mut app, w, h);
        if w == 160 {
            assert!(s.contains("Around the World") && s.contains("in 1 playlist"), "{s}");
        }
        if std::env::var_os("SHOW_SCREENS").is_some() && w == 160 {
            println!("{s}");
        }
    }
    app.fullscreen = false;
    if std::env::var_os("SHOW_SCREENS").is_some() {
        for (w, h) in [(80, 24), (160, 45)] {
            app.focus = Focus::Tracks;
            println!("{}", screen(&mut app, w, h));
            app.focus = Focus::Playlists;
            println!("{}", screen(&mut app, w, h));
            app.on_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
            println!("{}", screen(&mut app, w, h));
            app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            app.help = true;
            println!("{}", screen(&mut app, w, h));
            app.help = false;
        }
    }
}

fn ago_by(secs: u64) -> std::time::Instant {
    std::time::Instant::now().checked_sub(std::time::Duration::from_secs(secs)).unwrap()
}

/// The screen `draw` draws, and the whole seconds old each of `ats` was while it drew: the
/// screen shows each one's age as one of them.
fn drawn(ats: &[std::time::Instant], draw: impl FnOnce() -> String) -> (String, Vec<std::ops::RangeInclusive<u64>>) {
    let before: Vec<u64> = ats.iter().map(|at| at.elapsed().as_secs()).collect();
    let s = draw();
    (s, ats.iter().zip(before).map(|(at, b)| b..=at.elapsed().as_secs()).collect())
}

/// Work under way shows in the header: a job with its time, what it's doing and how to
/// stop it, or what's being read.
#[test]
fn the_header_says_what_is_under_way() {
    let (mut app, _dir) = blank(View::Library);
    let started = ago_by(5);
    app.job = Some(super::super::edits::Job {
        label: "Checking DNA".into(),
        detail: "3 of 40 tracks".into(),
        started,
        ..job(false, false)
    });
    let head = |app: &mut App, w| screen(app, w, 12).lines().next().unwrap().to_string();
    let (line, secs) = drawn(&[started], || head(&mut app, 160));
    let said = |s| format!("Checking DNA {s}s 3 of 40 tracks  esc cancel");
    assert!(secs[0].clone().any(|s| line.contains(&said(s))), "{line}");
    // Narrow, the job's progress is cut short and the rest left out.
    let line = head(&mut app, 100);
    assert!(line.contains("Checking") && !line.contains("help"), "{line}");
    app.job.as_mut().unwrap().committed = true;
    assert!(!head(&mut app, 160).contains("esc cancel"), "too late to stop");

    app.job = None;
    app.loading = Some("Reading DNA…".into());
    assert!(head(&mut app, 160).contains("Reading DNA…"));
    app.loading = None;
    app.profile.loading = true;
    assert!(head(&mut app, 160).contains("Building profile…"));
    app.profile.loading = false;
    app.status_loading = true;
    let line = head(&mut app, 160);
    assert!("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏".chars().any(|c| line.contains(c)), "a spinner: {line}");
}

/// The footer is the search box while searching, with what the filter made of it, and says
/// what to do while picking a playlist.
#[test]
fn the_footer_follows_the_search_and_picking() {
    let (mut app, _dir) = app_with(library());
    let foot = |app: &mut App| screen(app, 120, 20).lines().last().unwrap().to_string();
    app.searching = true;
    app.query = "bpm:>".into();
    app.filter_error = Some("bpm: needs a number".into());
    assert!(foot(&mut app).contains("/ bpm:>▏") && foot(&mut app).contains("bpm: needs a number"));
    (app.filter_error, app.filter_note) = (None, Some("4 tracks in 2 keys".into()));
    assert!(foot(&mut app).contains("4 tracks in 2 keys"));
    app.searching = false;

    app.track_state.select(Some(0));
    let mut target = app.target().expect("a track selected");
    target.what = "Around the World".into();
    app.picking = Some(super::super::edits::Pick { target, focus: Focus::Tracks });
    assert!(foot(&mut app).contains("Add Around the World to which playlist?"));
    let s = screen(&mut app, 120, 20);
    assert!(s.contains("Pick a playlist"), "{s}");
}

/// An error waits above the latest message until it's dismissed; the log lists every message
/// with how long ago it came, and says when there's none.
#[test]
fn messages_wait_and_the_log_keeps_them() {
    let (mut app, _dir) = blank(View::Library);
    app.show_log = true;
    assert!(screen(&mut app, 100, 20).contains("Nothing to report yet."));
    app.show_log = false;

    app.held = Some("couldn't write to DNA".into());
    app.toast("saved", Level::Info);
    let s = screen(&mut app, 100, 20);
    assert!(s.contains("✖ couldn't write to DNA") && s.contains("● saved  esc hides"), "{s}");

    app.log.clear();
    let ats = [ago_by(30), std::time::Instant::now()];
    for (msg, at) in ["half a minute ago", "now"].into_iter().zip(ats) {
        app.log.push_back((msg.into(), Level::Info, at));
    }
    app.show_log = true;
    let (s, ages) = drawn(&ats, || screen(&mut app, 100, 20));
    let secs = std::time::Duration::from_secs;
    for age in ages {
        assert!(age.clone().any(|a| s.contains(&ago(secs(a)))), "{age:?}\n{s}");
    }
    let said: Vec<String> = [0, 30, 180, 7200].map(|a| ago(secs(a))).into();
    assert_eq!(said, ["just now", "30 s ago", "3 min ago", "2 h ago"]);
}

/// A message too wide for the screen breaks between words, and inside a word (a path) too
/// long for a line.
#[test]
fn long_messages_wrap_inside_long_words() {
    assert_eq!(wrap_text("copy /Volumes/DNA/Contents to here", 12), ["copy", "/Volumes/DNA", "/Contents to", "here"]);
    assert_eq!(compact(999), "999");
    assert_eq!(compact(1234), "1.2k");
    assert_eq!(compact(12_345), "12k");
}

/// The Drives view: nothing plugged in, then a drive with everything the store and its
/// logbook know about it.
#[test]
fn the_drives_view_says_what_is_known_of_a_drive() {
    use chrono::TimeZone;
    use locallibrary::api::{DriveStatus, FleetDrive};
    use locallibrary::compare::Relation;
    use locallibrary::fleet::{CheckEntry, DriveLog, Problem, Role};

    let (mut app, dir) = blank(View::Drives);
    assert!(screen(&mut app, 120, 40).contains("No rekordbox drives mounted."));

    let root = dir.path().join("DNA");
    std::fs::create_dir_all(&root).unwrap();
    let at = chrono::Utc.with_ymd_and_hms(2026, 10, 4, 21, 30, 0).unwrap();
    let mut info = locallibrary::Device::open(&root).unwrap().info();
    info.summary = Some(locallibrary::model::LibrarySummary {
        tracks: 120,
        playlists: 8,
        folders: 2,
        artists: 90,
        audio_bytes: 2_000_000_000,
        total_duration_secs: 30_000,
        tracks_without_artwork: 4,
    });
    (info.exported_at, info.has_library_plus) = (Some(at), true);
    (info.total_space, info.free_space) = (Some(16_000_000_000), Some(4_000_000_000));
    info.settings_files = vec!["MYSETTING.DAT".into()];
    info.lineage_len = 1;
    info.pending = Some(locallibrary::device::PendingOp {
        op: locallibrary::device::LineageOp::Sync,
        started: at,
        description: "sync from BACKUP".into(),
        backup: None,
        player: None,
    });
    let check = |ok, summary: &str| Some(CheckEntry { at, ok, summary: summary.into() });
    let log = DriveLog {
        writes: 3,
        bytes_written: 1_200_000_000,
        clean_ejects: 2,
        last_scrub: check(true, "all 120 tracks read"),
        last_fs_check: check(false, "2 errors"),
        problems: vec![Problem { at, kind: "write_failed".into(), detail: "disk full".into() }],
        ..Default::default()
    };
    let fleet = |relation, error: Option<&str>, library: Option<&str>| FleetDrive {
        status: DriveStatus {
            drive: info.clone(),
            library: library.map(Into::into),
            relation,
            head: None,
            back_from_set: None,
            settings_drift: 2,
            error: error.map(Into::into),
        },
        role: Some(Role::Set),
        read_only: true,
        low_space: true,
        log: Some(log.clone()),
        same_identity_as: vec!["/Volumes/DNA 1".into()],
        exported_over_edits: 0,
    };
    app.drives = vec![info.clone()];
    app.drive_state.select(Some(0));
    app.status_loading = true;
    let s = screen(&mut app, 140, 60);
    for said in [
        "120  ·  90 artists",
        "4 tracks (locallibrary art auto)",
        &local_time(at, "%Y-%m-%d %H:%M").to_string(),
        "OneLibrary (exportLibrary.db) present",
        "75% used",
        "checking…",
        "MYSETTING.DAT",
        "1 state recorded",
        "⚠ interrupted: sync from BACKUP",
    ] {
        assert!(s.contains(said), "{said}\n{s}");
    }

    app.status_loading = false;
    assert!(screen(&mut app, 140, 60).contains("press R to check"));
    app.status = Some(Err("the store can't be read".into()));
    assert!(screen(&mut app, 140, 60).contains("the store can't be read"));
    for (relation, error, library, says) in [
        (Some(Relation::Identical), None, Some("Main"), "✓ up to date"),
        (Some(Relation::RightNewer), None, Some("Main"), "has changes not in the store"),
        (Some(Relation::LeftNewer), None, Some("Main"), "behind the store"),
        (Some(Relation::Diverged), None, Some("Main"), "diverged from the store"),
        (Some(Relation::Unrelated), None, Some("Main"), "? unrelated"),
        (None, None, Some("Main"), "library has no snapshots"),
        (None, None, None, "not in the store (e, then save a snapshot)"),
        (None, Some("bad export"), None, "error: bad export"),
    ] {
        app.status = Some(Ok(vec![fleet(relation, error, library)]));
        let s = screen(&mut app, 140, 60);
        assert!(s.contains(says), "{says}\n{s}");
    }
    app.status = Some(Ok(vec![fleet(Some(Relation::Identical), None, Some("Main"))]));
    let s = screen(&mut app, 140, 60);
    for said in [
        "library     Main",
        "2 setting(s) differ from the profile",
        "role        set",
        "3 times, 1.2 GB",
        "2 times cleanly",
        &format!("✓ {}  all 120 tracks read", local_time(at, "%Y-%m-%d")),
        &format!("✖ {}  2 errors", local_time(at, "%Y-%m-%d")),
        &format!("✖ {} write failed: disk full", local_time(at, "%Y-%m-%d")),
        "write-protected",
        "nearly full",
        "a copy of it (or it of) /Volumes/DNA 1",
    ] {
        assert!(s.contains(said), "{said}\n{s}");
    }
}

/// The Drives view lists each library with its drives under it, the drives of none last. A
/// library's row says how its clones stand together; its pane lists them (those not plugged
/// in too), where each stands, and what can be done to all of them.
#[test]
fn the_drives_view_lists_libraries_with_their_clones() {
    use chrono::TimeZone;
    use locallibrary::fleet::{DriveLog, KnownDrive};
    use locallibrary::hash::Hash;
    use locallibrary::store::SnapshotInfo;

    let (mut app, dir) = blank(View::Drives);
    let home = Uuid::new_v4();
    let drive = |name: &str, library_id| {
        let root = dir.path().join(name);
        std::fs::create_dir_all(&root).unwrap();
        DeviceInfo { has_export: true, library_id, ..locallibrary::Device::open(&root).unwrap().info() }
    };
    let (loose, sd, main) = (drive("LOOSE", None), drive("SD", Some(home)), drive("REKORDBOX", Some(home)));
    app.drives = vec![loose.clone(), sd.clone(), main.clone()];
    app.library_names = [(home, "Home".to_string())].into_iter().collect();
    app.drive_state.select(Some(0));
    let s = screen(&mut app, 140, 30);
    let at = |s: &str, t: &str| s.find(t).unwrap_or_else(|| panic!("{t}\n{s}"));
    assert!(at(&s, "Home") < at(&s, "  ● SD") && at(&s, "  ● SD") < at(&s, "  ● REKORDBOX"), "{s}");
    assert!(at(&s, "  ● REKORDBOX") < at(&s, "No library") && at(&s, "No library") < at(&s, "  ● LOOSE"), "{s}");

    // Up from the drive of none: the row of the drives of none, then the library's drives,
    // then the library.
    let up = |app: &mut App| app.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    up(&mut app);
    assert!(screen(&mut app, 140, 30).contains("These drives aren't in the store yet."));
    up(&mut app);
    up(&mut app);
    assert_eq!(app.drive_state.selected(), Some(1), "SD");
    up(&mut app);
    let s = screen(&mut app, 140, 30);
    for said in ["2 of 2 clones plugged in", "● REKORDBOX", "● SD", "press R to check"] {
        assert!(s.contains(said), "{said}\n{s}");
    }
    app.status_loading = true;
    assert!(screen(&mut app, 140, 30).contains("checking…"));

    // What the store says of the library and its clones.
    let at_time = chrono::Utc.with_ymd_and_hms(2026, 10, 8, 12, 0, 0).unwrap();
    let head = SnapshotInfo {
        id: Hash::of(b"head"),
        short_id: "3f9a2c".into(),
        library_id: home,
        parents: Vec::new(),
        created: at_time,
        message: None,
        source: None,
        fingerprint: Hash::of(b"library"),
        summary: Default::default(),
        files: 0,
        bytes: 0,
        labels: Vec::new(),
        beside: false,
    };
    let spare = KnownDrive {
        id: Uuid::new_v4(),
        name: "Spare".into(),
        role: Some(Role::Backup),
        library: Some("Home".into()),
        mounted: None,
        last_mount: dir.path().join("Spare"),
        last_seen: at_time,
        capacity: None,
        standing: Standing::Behind { snapshots: 2, held: "1a2b3c".into() },
        log: DriveLog::default(),
    };
    app.fleet_libraries = vec![FleetLibrary {
        id: home,
        name: "Home".into(),
        head: Some(head),
        mounted: vec![main.root.clone(), sd.root.clone()],
        offline: vec![spare],
        identical: Some(false),
        unsaved: true,
    }];
    let clone = |d: &DeviceInfo, role, relation, back_from_set| {
        let mut f = fleet(d.clone(), Some("Home"));
        (f.role, f.status.relation, f.status.back_from_set) = (Some(role), Some(relation), back_from_set);
        f
    };
    app.status = Some(Ok(vec![
        clone(&main, Role::Primary, Relation::Identical, None),
        clone(&sd, Role::Set, Relation::RightNewer, Some(2)),
    ]));
    app.status_loading = false;
    let s = screen(&mut app, 140, 30);
    for said in [
        "2 of 3",
        "not identical · unsaved",
        "2 of 3 clones plugged in",
        "● REKORDBOX  primary  ✓ up to date",
        "● SD         set      ▲ back from a set: 2 new sessions",
        "○ Spare      backup   not plugged in · 2 snapshots behind",
        &format!("latest      3f9a2c  {}", local_time(at_time, "%Y-%m-%d %H:%M")),
        "browse its newest clone in the library",
    ] {
        assert!(s.contains(said), "{said}\n{s}");
    }
    for (w, h) in [(80, 24), (40, 12), (20, 6)] {
        screen(&mut app, w, h);
    }

    // Its actions are the library's, on all of its clones plugged in, not the drive's selected
    // before.
    assert_eq!(app.action_drive(), None);
    app.on_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no menu") };
    let labels: Vec<&str> = p.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(
        (p.title.as_str(), labels),
        ("Library · Home", vec!["sync its clones from the newest", "check its clones for a set", "eject its clones"])
    );
    app.modal = None;
    app.perform(super::super::keymap::Act::SetPrep, None);
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Checking SD and 1 more"));
    wait(&mut app, |a| a.job.is_none());

    // Enter shows the library: read from its newest clone (here, empty folders: it says why
    // it can't be).
    app.modal = None;
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!((app.view, app.loading.as_deref()), (View::Library, Some("Reading Home…")));
    wait(&mut app, |a| a.loading.is_none());
}

/// The Snapshots view while the store is read, when it can't be, and when it has none.
#[test]
fn the_snapshots_view_says_when_there_are_none() {
    let (mut app, _dir) = blank(View::Snapshots);
    assert!(screen(&mut app, 100, 20).contains("loading…"));
    app.history = Some(Err("no store here".into()));
    assert!(screen(&mut app, 100, 20).contains("no store here"));
    app.history = Some(Ok(Vec::new()));
    assert!(screen(&mut app, 100, 20).contains("The store has no snapshots yet."));
}

/// The test library's first track playing half a minute in, opened (made by hand: tests
/// have no audio), with everything rekordbox's analysis and tags can say about it.
fn playing_in_full() -> (App, tempfile::TempDir) {
    use locallibrary::anlz::{Mood, Phrase, PhraseKind, SongStructure, TrackAnalysis};

    use super::super::app::PhraseSpan;

    let phrase = |kind, start_beat, end_beat| Phrase { kind, start_beat, end_beat, fill_beat: None };
    // Two beats a second, the last few a touch faster.
    let mut beats = grid(0, 120.0, 600);
    beats[590..].iter_mut().for_each(|b| b.bpm = 121.0);
    let analysis = TrackAnalysis {
        beats,
        cues: vec![cue(Some(0), 10_000, None), cue(Some(1), 60_000, None), cue(None, 90_000, Some(94_000))],
        preview: Some(wave(150 * 300)),
        detail: Some(wave(150 * 300)),
        structure: Some(SongStructure {
            mood: Mood::High,
            phrases: vec![phrase(PhraseKind::Intro, 1, 65), phrase(PhraseKind::Chorus, 65, 600)],
        }),
    };
    let (mut app, dir) = playing(analysis);
    let now = app.now.as_mut().unwrap();
    let track = &mut now.track;
    (track.label, track.year, track.comment) = ("Delsin".into(), 2016, " a warm-up favourite ".into());
    (track.composer, track.isrc, track.play_count) = ("Michael Hall".into(), "GBXYZ1600001".into(), 3);
    (track.sample_depth, track.file_size, track.bitrate, track.sample_rate) = (24, 9_000_000, 320, 44100);
    now.key = super::super::keys::Camelot::parse(&track.key);
    now.phrases = vec![
        PhraseSpan { kind: PhraseKind::Intro, start_ms: 0.0, end_ms: 32_000.0, bars: 16 },
        PhraseSpan { kind: PhraseKind::Chorus, start_ms: 32_000.0, end_ms: 300_000.0, bars: 134 },
    ];
    let mut png = Vec::new();
    image::RgbImage::from_pixel(8, 8, image::Rgb([200, 40, 90]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    now.art = loclib_waveform::Art::decode(&png);
    now.playlists = (1..=6).map(|n| format!("Sets › Friday {n}")).collect();
    now.opening = false;
    app.resume = Some(std::time::Duration::from_secs(30));
    (app, dir)
}

#[test]
fn the_full_screen_player_says_all_it_knows_of_the_track() {
    let (mut app, _dir) = playing_in_full();
    app.fullscreen = true;
    let s = screen(&mut app, 160, 45);
    for said in [
        "INTRO",
        "phrase next Chorus in 0:02  ·  high mood",
        "Delsin · 2016",
        "24-bit · 9.0 MB · TEST",
        "played 3×",
        "“a warm-up favourite”",
        "composer Michael Hall · ISRC GBXYZ1600001",
        "next B 0:30",
        "600 beats · 150 bars · 120.0–121.0 BPM",
        "2 hot cues (1 loops) · 1 memory",
        "2 phrases ▮▮",
        "open this drive to see suggestions",
        "+ 3 more",
    ] {
        assert!(s.contains(said), "{said}\n{s}");
    }
    // In the last phrase there's no next one.
    app.resume = Some(std::time::Duration::from_secs(200));
    assert!(screen(&mut app, 160, 45).contains("phrase last"));
}

/// The compact player draws whichever visualiser is picked, the deck from the track's
/// detail waveform.
#[test]
fn every_visualiser_draws_in_the_player() {
    let (mut app, _dir) = blank(View::Library);
    app.vis = Vis::Deck;
    assert!(screen(&mut app, 120, 30).contains("deck: play a track"));
    app.audio_error = Some("no output device".into());
    assert!(screen(&mut app, 120, 30).contains("No audio output: no output device"));

    let (mut app, _dir) = playing_in_full();
    (app.repeat, app.deck.auto_cue) = (Repeat::One, true);
    for vis in [Vis::Bars, Vis::Mirror, Vis::Scope, Vis::Deck] {
        app.vis = vis;
        let s = screen(&mut app, 120, 30);
        assert!(s.contains(&format!(" v {} ", vis.name())), "{s}");
        assert!(s.contains("↻ one") && s.contains("auto cue"), "{s}");
    }
    // The deck: the hot cue it passed, with the waveform round the playhead.
    assert!(screen(&mut app, 120, 30).contains('█'));
}

/// The track list's title says what's searched and how it's sorted, the columns follow, and
/// an empty list says why.
#[test]
fn the_track_list_says_how_its_searched_and_sorted() {
    let (mut app, _dir) = app_with(library());
    app.focus = Focus::Tracks;
    app.show_ratings = true;
    app.sort = Some(SortKey { field: SortField::Added, descending: true });
    (app.query, app.search_all) = ("around".into(), true);
    app.visible = vec![0];
    let s = screen(&mut app, 200, 30);
    for said in ["/ around whole drive", " Added ↓ ", "Added↓", "Genre", "2026-09-30", "House", "★★★★★"] {
        assert!(s.contains(said), "{said}\n{s}");
    }
    app.visible.clear();
    assert!(screen(&mut app, 200, 30).contains("Nothing matches the search."));
    app.query.clear();
    assert!(screen(&mut app, 200, 30).contains("Nothing in this list."));

    // Unplugged, the drive's lists say so.
    app.loaded.as_mut().unwrap().stale = true;
    app.focus = Focus::Playlists;
    let s = screen(&mut app, 200, 30);
    assert_eq!(s.matches("unplugged").count(), 2, "{s}");
}

/// The crate says how to fill it, and names the drive of a track from another one.
#[test]
fn the_crate_says_how_to_fill_it() {
    let (mut app, _dir) = app_with(library());
    app.show_crate = true;
    app.focus = Focus::Crate;
    assert!(screen(&mut app, 160, 30).contains("The crate is empty."));
    let track = track(9, "DJ", "Hm");
    app.crate_list.items.push(super::super::crate_list::CrateTrack {
        root: "/Volumes/BACKUP".into(),
        drive: "BACKUP".into(),
        track,
    });
    let s = screen(&mut app, 160, 30);
    assert!(s.contains("Hm  DJ · BACKUP"), "{s}");
}

/// The selected track's panel: nothing selected, then a track whose file is missing, with
/// the playlists it's in once they're read.
#[test]
fn the_track_panel_says_what_is_wrong_with_a_track() {
    let (mut app, _dir) = app_with(library());
    app.show_details = true;
    app.focus = Focus::Details;
    app.track_state.select(None);
    assert!(screen(&mut app, 160, 40).contains("No track selected."));
    app.track_state.select(Some(0));
    // The missing files and the details are read in the background.
    wait(&mut app, |a| a.details_current() && a.loaded.as_ref().unwrap().problems.is_missing(0));
    let s = screen(&mut app, 160, 40);
    assert!(s.contains("✖ the audio file isn't on the drive"), "{s}");
}

/// A question waits for a yes or no; the rating picker opened by the pointer opens beside
/// it; a list with nothing matching what's typed says so; a check's problems are marked.
#[test]
fn dialogs_draw_what_they_ask() {
    use super::super::drives::{Report, Tone};
    use super::super::edits::{Menu, Sub};

    let (mut app, _dir) = app_with(library());
    app.modal = Some(Modal::Ask("DNA.xml is there already. Replace it?".into(), Box::new(|_: &mut App| {})));
    let s = screen(&mut app, 100, 24);
    assert!(s.contains("Sure?") && s.contains("Replace it?") && s.contains("y yes  n no"), "{s}");

    app.track_state.select(Some(0));
    let mut menu = Menu::new(app.target().unwrap(), Some((70, 2)));
    menu.sub = Some(Sub::Rating);
    app.modal = Some(Modal::Menu(menu));
    let s = screen(&mut app, 100, 24);
    assert!(
        s.contains("Rating · Daft Punk - Around the World") && s.contains("no rating") && s.contains("★★★★★"),
        "{s}"
    );
    // By the pointer, kept on screen: clicked at column 70, it's moved left to fit.
    let row = s.lines().find(|l| l.contains("Rating ·")).unwrap();
    assert_eq!(row.chars().position(|c| c == '╭'), Some(50), "{row}");

    app.modal = None;
    app.on_key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
    for c in "zzzzqq".chars() {
        app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no palette") };
    let empty = p.empty;
    let s = screen(&mut app, 100, 24);
    assert!(s.contains(empty) && s.contains("0 of"), "{s}");

    let lines = vec![(Tone::Problem, "2 tracks won't load".to_string()), (Tone::Ok, "the export reads".into())];
    app.modal = Some(Modal::Report(Report { title: "Check · DNA".into(), lines, scroll: 0 }));
    let s = screen(&mut app, 100, 24);
    assert!(s.contains("✖ 2 tracks won't load") && s.contains("✓ the export reads"), "{s}");
}

/// The outputs list the system's default first, by name when it has one, and mark the one
/// playing.
#[test]
fn outputs_name_the_default() {
    let (mut app, _dir) = blank(View::Library);
    app.deck.outputs = Some(super::super::deck::Outputs {
        names: vec!["MacBook Speakers".into(), "DJM-A9".into()],
        default: Some("MacBook Speakers".into()),
        state: ListState::default().with_selected(Some(0)),
    });
    let s = screen(&mut app, 100, 24);
    assert!(s.contains("the system's default (MacBook Speakers)") && s.contains("DJM-A9"), "{s}");
}

/// An edit's confirmation lists every playlist and track it changes, cut short where
/// there are many.
#[test]
fn an_edits_confirmation_lists_what_changes() {
    use locallibrary::edit::{EditOptions, EditPlan, PlaylistChange};
    use locallibrary::model::TrackRef;
    use locallibrary::op::Planned;

    use super::super::edits::set_rating;

    let (dir, root) = demo_drive();
    let device = locallibrary::Device::open(&root).unwrap();
    let path = device.library().unwrap().tracks[0].file_path.clone();
    let req = locallibrary::api::EditRequest {
        drive: root.display().to_string(),
        edits: vec![set_rating(&[path], 4)],
        options: EditOptions::default(),
    };
    let mut planned: Planned<EditPlan> = locallibrary::api::edit_plan(&req, &locallibrary::Ctx::silent()).unwrap();
    let p = &mut planned.plan;
    p.playlists_created.push("Sets/Sunday".into());
    p.playlists_deleted.push("Old".into());
    let change = |path: &str, added, removed, reordered| PlaylistChange {
        path: path.into(),
        before: 10,
        after: 10 + added - removed,
        added,
        removed,
        reordered,
    };
    p.playlists_changed =
        vec![change("Warm-up", 0, 0, true), change("Peak", 2, 1, false), change("Closing", 0, 0, false)];
    p.tracks_removed =
        (1..=7).map(|id| TrackRef { id, name: format!("Track {id}"), file_path: String::new() }).collect();
    (p.files, p.bytes) = (vec!["/Contents/1.mp3".into()], 9_000_000);
    p.history_removed = 1;
    p.warnings.push("the drive is nearly full".into());

    let mut app = App::blank(dir.path().join("store"), None);
    let s = confirmation_screen(&mut app, &root, Box::new(planned));
    for said in [
        "+ Sets/Sunday",
        "− Old",
        "~ Warm-up  reordered, 10 → 10 tracks",
        "~ Peak  +2 −1, 10 → 11 tracks",
        "~ Closing  entries changed",
        "− 7 tracks removed, 1 files, 9.0 MB",
        "Track 5",
        "and 2 more",
        "− 1 history list removed",
        "⚠ the drive is nearly full",
    ] {
        assert!(s.contains(said), "{said}\n{s}");
    }
}

/// The screen asking for a yes to `plan`, on the drive at `root`.
fn confirmation_screen(app: &mut App, root: &std::path::Path, plan: Box<dyn locallibrary::op::AnyPlanned>) -> String {
    app.modal = Some(Modal::Confirm(super::super::edits::Confirm {
        root: root.to_path_buf(),
        drive: "DEMO".into(),
        plan,
        scroll: 0,
        shown: std::time::Instant::now(),
        after: None,
    }));
    screen(app, 120, 50)
}

/// Undoing says how the playlists, tracks and files go back, and which recorded edits come
/// off the journal; a plan that deletes something says so.
#[test]
fn confirmations_say_what_goes_back_and_what_is_lost() {
    use locallibrary::diff::{Move, PlaylistChanges};
    use locallibrary::model::TrackRef;

    let (dir, root) = demo_drive();
    let device = locallibrary::Device::open(&root).unwrap();
    let path = device.library().unwrap().tracks[0].file_path.clone();
    let req = locallibrary::api::EditRequest {
        drive: root.display().to_string(),
        edits: vec![super::super::edits::set_rating(&[path], 4)],
        options: Default::default(),
    };
    locallibrary::api::edit_apply(&req, &locallibrary::Ctx::silent()).unwrap();
    let undo = locallibrary::api::UndoRequest { drive: Some(root.display().to_string()), ..Default::default() };
    let mut planned = locallibrary::api::undo_plan(&undo).unwrap();
    let track = |id| TrackRef { id, name: format!("Track {id}"), file_path: String::new() };
    let u = &mut planned.plan;
    u.diff.added_playlists.push("Sets/Sunday".into());
    u.diff.removed_playlists.push("Old".into());
    u.diff.changed_playlists.push(PlaylistChanges {
        path: "Peak".into(),
        added: Vec::new(),
        removed: Vec::new(),
        moved: vec![Move { track: track(1), from: 1, to: 2 }],
    });
    u.files = (0..6)
        .map(|i| locallibrary::api::FileChange { path: format!("/f{i}"), change: "replaced", bytes: 1000 })
        .collect();
    let entry = u.journal.first().cloned().expect("the edit was recorded");
    u.journal.push(entry);
    planned.warnings.push("the drive changed since".into());

    let mut app = App::blank(dir.path().join("store"), None);
    let s = confirmation_screen(&mut app, &root, Box::new(planned));
    for said in [
        "+ Sets/Sunday",
        "− Old",
        "~ Peak  1 moved",
        "6 files put back (6.0 KB)",
        "and 2 more",
        "2 recorded edits come off the journal",
        "⚠ the drive changed since",
    ] {
        assert!(s.contains(said), "{said}\n{s}");
    }

    // Any other plan: its summary, its warnings, and whether it deletes anything.
    let store = locallibrary::Store::open_or_init(dir.path().join("store")).unwrap();
    let pull = locallibrary::api::PullRequest {
        drive: root.display().to_string(),
        library: Some("Demo".into()),
        new_library: true,
        ..Default::default()
    };
    let mut planned = locallibrary::api::pull_plan(store.root(), &pull, &locallibrary::Ctx::silent()).unwrap();
    planned.warnings.push("the drive is nearly full".into());
    planned.destructive = true;
    let s = confirmation_screen(&mut app, &root, Box::new(planned));
    assert!(s.contains("⚠ the drive is nearly full"), "{s}");
    assert!(s.contains("This deletes or replaces what's there now."), "{s}");
}

/// Spans fitted to a width take no more columns than that, start as the line does, and are
/// the whole line when it fits.
#[hegel::test(test_cases = 300)]
fn fitted_spans_keep_to_their_width(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use unicode_width::UnicodeWidthStr;
    let parts: Vec<String> = tc.draw(gs::vecs(gs::text().alphabet("ab 日😹").max_size(6)).max_size(5));
    let max = tc.draw(gs::integers::<usize>().max_value(20));
    let whole: String = parts.concat();
    let out: String =
        fit(parts.into_iter().map(Span::raw).collect(), max).iter().map(|s| s.content.to_string()).collect();
    assert!(out.width() <= max, "{out:?} is wider than {max}");
    if whole.width() <= max {
        assert_eq!(out, whole);
    } else {
        assert!(whole.starts_with(out.trim_end_matches('…')), "{out:?} of {whole:?}");
    }
}

/// A list scrolled to show its selection does: the selected row is in the window, and the
/// window doesn't run past the end of the list. A window of no rows (a pane too short for
/// any) starts no later than the selection, and never past the end of the list.
#[hegel::test(test_cases = 300)]
fn the_selection_is_always_in_the_window(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let len = tc.draw(gs::integers::<usize>().max_value(100));
    // No rows often: the drives list and the profile's sections have none under 4 rows.
    let page = tc.draw(hegel::one_of!(gs::just(0), gs::integers::<usize>().max_value(40)));
    let offset = tc.draw(gs::integers::<usize>().max_value(120));
    let sel = tc.draw(gs::booleans()).then(|| tc.draw(gs::integers::<usize>().max_value(len.saturating_sub(1))));
    let top = scroll_window(offset, sel, page, len);
    assert!(top <= len.saturating_sub(page), "{top} leaves rows of {len} empty below a page of {page}");
    if let Some(s) = sel.filter(|_| len > 0) {
        assert!(top <= s && s < top + page.max(1), "{s} isn't in {top}..{}", top + page);
    }
}

/// Every list and table scrolled down to its selection on a short screen comes back up once
/// it fits again: drawn short, then tall, its first row and the one selected show. (The
/// Mixes view's and the profile's have their own, in `mix.rs` and `profile.rs`.)
#[hegel::test(test_cases = 100)]
fn lists_come_back_up_once_they_fit(tc: hegel::TestCase) {
    use std::sync::Arc;

    use hegel::generators as gs;
    use locallibrary::hash::Hash;
    use locallibrary::store::{LibraryRecord, SnapshotInfo};
    use locallibrary::{Device, Library};

    use super::super::app::{Details, Mix};
    use super::super::crate_list::CrateTrack;
    use super::super::deck::Outputs;
    use super::super::keymap::Act;
    use super::super::palette::{Item, Picker, Then};

    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(16));
    let sel = tc.draw(gs::integers::<usize>().max_value(n - 1));
    let (mut app, dir) = app_with(library());
    let named = |what: &str| (0..n).map(|i| format!("{what} {i:02}")).collect::<Vec<_>>();
    let (rows, picked) = match tc.draw(gs::integers::<u8>().max_value(6)) {
        0 => {
            let rows = named("Drive");
            app.drives = (rows.iter())
                .map(|name| {
                    let root = dir.path().join(name);
                    std::fs::create_dir_all(&root).unwrap();
                    DeviceInfo { name: Some(name.clone()), ..Device::open(&root).unwrap().info() }
                })
                .collect();
            app.focus = Focus::Drives;
            app.drive_state.select(Some(sel));
            (rows, sel)
        }
        1 => {
            let rows = named("Crated");
            app.crate_list.items = (rows.iter().zip(100..))
                .map(|(title, id)| CrateTrack {
                    root: dir.path().into(),
                    drive: "TEST".into(),
                    track: track(id, "DJ", title),
                })
                .collect();
            (app.show_crate, app.show_details, app.focus) = (true, false, Focus::Crate);
            app.crate_state.select(Some(sel));
            (rows, sel)
        }
        2 => {
            let rows = named("Match");
            let tracks = (rows.iter().zip(1..)).map(|(title, id)| track(id, "DJ", title)).collect();
            let key = Camelot::parse("8A").unwrap();
            let items = (0..n).map(|index| Mix { index, key, relation: KeyRelation::Same, tempo_gap: 0.0 }).collect();
            let matches =
                Matches { library: Arc::new(Library { tracks, ..Default::default() }), root: dir.path().into(), items };
            let generation = app.loaded.as_ref().unwrap().generation;
            let index = app.selected_track().unwrap();
            app.details = Some(Details { generation, index, matches, playlists: Vec::new() });
            (app.show_details, app.show_crate, app.focus) = (true, false, Focus::Details);
            app.details_state.select(Some(sel));
            (rows, sel)
        }
        3 => {
            let rows = named("snap");
            let at = chrono::Utc::now();
            let snapshot = |short_id: &String| SnapshotInfo {
                id: Hash::of(short_id.as_bytes()),
                short_id: short_id.clone(),
                library_id: Uuid::nil(),
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
            let record = LibraryRecord { id: Uuid::nil(), name: "Demo".into(), head: None, created: at, updated: at };
            app.history = Some(Ok(vec![(record, rows.iter().map(snapshot).collect())]));
            app.view = View::Snapshots;
            app.history_state.select(Some(sel));
            (rows, sel)
        }
        4 => {
            let rows = named("Command");
            let items = rows.iter().map(|r| Item::new(r, "")).collect();
            let mut p = Picker::new("Pick", items, Then::Act(vec![Act::Help; n]));
            p.state.select(Some(sel));
            app.modal = Some(Modal::Pick(p));
            (rows, sel)
        }
        5 => {
            let names = named("Output");
            app.deck.outputs = Some(Outputs {
                names: names.clone(),
                default: None,
                state: ListState::default().with_selected(Some(sel)),
            });
            ([vec!["the system's default".to_string()], names].concat(), sel)
        }
        _ => {
            app.on_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
            let Some(Modal::Menu(m)) = &mut app.modal else { panic!("no menu") };
            let picked = sel.min(m.items.len() - 1);
            m.state.select(Some(picked));
            (m.items.iter().map(|i| clip(&i.label, 41)).collect(), picked)
        }
    };
    let short = tc.draw(gs::integers::<u16>().min_value(1).max_value(40));
    screen(&mut app, 200, short);
    let s = screen(&mut app, 200, 70);
    for row in [&rows[0], &rows[picked]] {
        assert!(s.contains(row.as_str()), "{row:?} isn't shown after drawing {short} rows high:\n{s}");
    }
}

/// How long ago something was reads back as a span that holds it: under five seconds is just
/// now, then seconds, minutes and hours, each the whole number passed.
#[hegel::test(test_cases = 300)]
fn ages_read_back_as_the_time_passed(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let secs = tc.draw(gs::integers::<u64>().max_value(400_000));
    let said = ago(std::time::Duration::from_secs(secs));
    if said == "just now" {
        return assert!(secs < 5);
    }
    let (n, unit) = said.strip_suffix(" ago").and_then(|s| s.split_once(' ')).expect(&said);
    let n: u64 = n.parse().unwrap();
    let per = match unit {
        "s" => 1,
        "min" => 60,
        "h" => 3600,
        _ => panic!("{said}"),
    };
    assert!(n * per <= secs && secs < (n + 1) * per, "{secs} s said as {said}");
    assert!(secs >= 5 && (per == 1) == (secs < 60) && (per == 3600) == (secs >= 3600), "{said} for {secs} s");
}

/// A count said short reads back as near what it is: as it is under a thousand, to a tenth of a
/// thousand below ten thousand, then to the thousand, never over.
#[hegel::test(test_cases = 300)]
fn counts_said_short_read_back(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let n = tc.draw(gs::integers::<usize>().max_value(2_000_000));
    let said = compact(n);
    match said.strip_suffix('k') {
        None => assert_eq!(said.parse::<usize>().unwrap(), n),
        Some(k) => {
            let back = (k.parse::<f64>().unwrap() * 1000.0).round() as usize;
            let near = if n < 10_000 { back.abs_diff(n) <= 50 } else { back <= n && n - back < 1000 };
            assert!(n >= 1000 && near, "{n} as {said}");
        }
    }
}

/// The track list's columns: title and artist first, album and genre as the width allows, My
/// Tags and ratings when asked for, the sorted column when it wouldn't show otherwise, and BPM,
/// key and time last; each once, each named apart.
#[hegel::test(test_cases = 300)]
fn the_columns_are_the_ones_that_fit_and_the_sort(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let width = tc.draw(gs::integers::<u16>().max_value(220));
    let field = tc.draw(gs::optional(gs::sampled_from(&EVERY_FIELD[..])));
    let sort = field.map(|field| SortKey { field, descending: tc.draw(gs::booleans()) });
    let (ratings, tags) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    let cols = columns(width, sort, ratings, tags);
    assert_eq!(cols[..2], [Col::Title, Col::Artist]);
    assert_eq!(cols[cols.len() - 3..], [Col::Bpm, Col::Key, Col::Time]);
    assert_eq!(cols.contains(&Col::Album), width >= 110 || field == Some(SortField::Album));
    assert_eq!(cols.contains(&Col::Genre), width >= 140 || field == Some(SortField::Genre));
    assert_eq!(cols.contains(&Col::Tags), tags);
    assert_eq!(cols.contains(&Col::Rating), ratings || field == Some(SortField::Rating));
    if let Some(c) = field.and_then(Col::of) {
        assert!(cols.contains(&c), "sorted by {c:?}, not shown");
        assert_eq!(c.field(), field);
    }
    for (i, c) in cols.iter().enumerate() {
        assert!(!cols[i + 1..].contains(c), "{c:?} twice");
        assert!(cols[i + 1..].iter().all(|d| d.name() != c.name()), "{c:?} named as another");
    }
}

/// The demo drive, with an edit made on it (so there's a backup to undo), for confirmations.
fn edited_drive() -> &'static (tempfile::TempDir, std::path::PathBuf, String) {
    static DRIVE: std::sync::OnceLock<(tempfile::TempDir, std::path::PathBuf, String)> = std::sync::OnceLock::new();
    DRIVE.get_or_init(|| {
        let (dir, root) = demo_drive();
        let path = locallibrary::Device::open(&root).unwrap().library().unwrap().tracks[0].file_path.clone();
        let req = locallibrary::api::EditRequest {
            drive: root.display().to_string(),
            edits: vec![super::super::edits::set_rating(std::slice::from_ref(&path), 4)],
            options: Default::default(),
        };
        locallibrary::api::edit_apply(&req, &locallibrary::Ctx::silent()).unwrap();
        (dir, root, path)
    })
}

/// Playlist names a plan can touch.
const LISTS: &[&str] = &["Sets/Sunday", "Old", "Warm-up", "Peak", "Closing"];

/// A confirmation lists everything its plan changes: an edit's playlists made, deleted and
/// changed (with how), its tracks changed and removed (the first five by name, then how many
/// more), the history lists it drops and its warnings; an undo how playlists and tracks go back,
/// the files it puts back (the first four, then how many more) and the recorded edits that come
/// off the journal; any other plan its summary and warnings, and whether it deletes.
#[hegel::test(test_cases = 40, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_confirmation_lists_all_its_plan_does(tc: hegel::TestCase) {
    // An edit, an undo and another plan, in each case.
    for kind in 0..3u8 {
        a_confirmation_lists(&tc, kind);
    }
}

/// [`a_confirmation_lists_all_its_plan_does`] for an edit (0), an undo (1) or another plan.
fn a_confirmation_lists(tc: &hegel::TestCase, kind: u8) {
    use hegel::generators as gs;
    use locallibrary::diff::{Entry, Move, PlaylistChanges};
    use locallibrary::edit::{EditOptions, PlaylistChange};
    use locallibrary::model::TrackRef;

    let (dir, root, path) = edited_drive();
    let names = |tc: &hegel::TestCase| -> Vec<String> {
        LISTS.iter().filter(|_| tc.draw(gs::booleans())).map(|s| s.to_string()).collect()
    };
    let track = |id| TrackRef { id, name: format!("Track {id}"), file_path: String::new() };
    let warnings: Vec<String> =
        (0..tc.draw(gs::integers::<usize>().max_value(2))).map(|i| format!("warning {i}")).collect();
    let mut app = App::blank(dir.path().join("store"), None);
    let mut want: Vec<String> = warnings.iter().map(|w| format!("⚠ {w}")).collect();
    let mut not = Vec::new();
    let plan: Box<dyn locallibrary::op::AnyPlanned> = match kind {
        0 => {
            let req = locallibrary::api::EditRequest {
                drive: root.display().to_string(),
                edits: vec![super::super::edits::set_rating(std::slice::from_ref(path), 2)],
                options: EditOptions::default(),
            };
            let mut planned = locallibrary::api::edit_plan(&req, &locallibrary::Ctx::silent()).unwrap();
            let p = &mut planned.plan;
            p.playlists_created = names(tc);
            p.playlists_deleted = names(tc);
            p.playlists_changed = (names(tc).into_iter())
                .map(|path| {
                    let (added, removed) =
                        (tc.draw(gs::integers::<usize>().max_value(2)), tc.draw(gs::integers::<usize>().max_value(2)));
                    PlaylistChange {
                        path,
                        before: 10,
                        after: 10 + added - removed,
                        added,
                        removed,
                        reordered: tc.draw(gs::booleans()),
                    }
                })
                .collect();
            p.tracks_changed = tc.draw(gs::integers::<usize>().max_value(3));
            p.tracks_removed = (1..=tc.draw(gs::integers::<u32>().max_value(8))).map(track).collect();
            if !p.tracks_removed.is_empty() && tc.draw(gs::booleans()) {
                (p.files, p.bytes) = (vec!["/Contents/1.mp3".into()], 9_000_000);
            }
            p.history_removed = tc.draw(gs::integers::<usize>().max_value(2));
            p.warnings = warnings;
            want.extend(p.playlists_created.iter().map(|l| format!("+ {l}")));
            want.extend(p.playlists_deleted.iter().map(|l| format!("− {l}")));
            for c in &p.playlists_changed {
                let how = match (c.reordered, c.added, c.removed) {
                    (true, ..) => "reordered".to_string(),
                    (_, 0, 0) => "entries changed".into(),
                    (_, a, 0) => format!("+{a}"),
                    (_, 0, r) => format!("−{r}"),
                    (_, a, r) => format!("+{a} −{r}"),
                };
                want.push(format!("~ {}  {how}, {} → {} tracks", c.path, c.before, c.after));
            }
            if p.tracks_changed > 0 {
                want.push(format!("~ {} changed", plural(p.tracks_changed, "track")));
            }
            let removed = p.tracks_removed.len();
            if removed > 0 {
                let files = if p.files.is_empty() { String::new() } else { ", 1 files, 9.0 MB".into() };
                want.push(format!("− {} removed{files}", plural(removed, "track")));
                want.extend(p.tracks_removed.iter().take(5).map(|t| t.name.clone()));
                if removed > 5 {
                    want.push(format!("and {} more", removed - 5));
                    not.push(format!("Track {}", removed));
                }
            }
            if p.history_removed > 0 {
                want.push(format!("− {} removed", plural(p.history_removed, "history list")));
            }
            want.push("The drive is backed up first: u takes this back.".into());
            Box::new(planned)
        }
        1 => {
            let undo = locallibrary::api::UndoRequest { drive: Some(root.display().to_string()), ..Default::default() };
            let mut planned = locallibrary::api::undo_plan(&undo).unwrap();
            let u = &mut planned.plan;
            u.diff.added_playlists = names(tc);
            u.diff.removed_playlists = names(tc);
            u.diff.changed_playlists = (names(tc).into_iter())
                .map(|path| {
                    let entries =
                        |n: usize| (0..n).map(|i| Entry { track: track(i as u32), position: i + 1 }).collect();
                    let moved = (0..tc.draw(gs::integers::<usize>().max_value(2)))
                        .map(|i| Move { track: track(i as u32), from: 1, to: 2 })
                        .collect();
                    let (a, r) =
                        (tc.draw(gs::integers::<usize>().max_value(2)), tc.draw(gs::integers::<usize>().max_value(2)));
                    PlaylistChanges { path, added: entries(a), removed: entries(r), moved }
                })
                .collect();
            u.files = (0..tc.draw(gs::integers::<usize>().max_value(6)))
                .map(|i| locallibrary::api::FileChange { path: format!("/f{i}"), change: "replaced", bytes: 1000 })
                .collect();
            let entry = u.journal.first().cloned().expect("the edit was recorded");
            u.journal = vec![entry; tc.draw(gs::integers::<usize>().max_value(2))];
            planned.warnings = warnings;
            let u = &planned.plan;
            want.extend(u.diff.added_playlists.iter().map(|l| format!("+ {l}")));
            want.extend(u.diff.removed_playlists.iter().map(|l| format!("− {l}")));
            for c in &u.diff.changed_playlists {
                let mut parts = Vec::new();
                parts.extend((!c.added.is_empty()).then(|| format!("+{}", c.added.len())));
                parts.extend((!c.removed.is_empty()).then(|| format!("−{}", c.removed.len())));
                parts.extend((!c.moved.is_empty()).then(|| format!("{} moved", c.moved.len())));
                want.push(format!("~ {}  {}", c.path, parts.join(" ")).trim_end().to_string());
            }
            if u.diff.is_empty() {
                want.push("playlists and tracks stay as they are".into());
            }
            want.push(format!("{} put back ({})", plural(u.files.len(), "file"), bytes(u.files.len() as u64 * 1000)));
            want.extend(u.files.iter().take(4).map(|f| format!("replaced {}", f.path)));
            if u.files.len() > 4 {
                want.push(format!("and {} more", u.files.len() - 4));
            }
            match u.journal.len() {
                0 => not.push("off the journal".into()),
                1 => want.push("1 recorded edit comes off the journal".into()),
                n => want.push(format!("{n} recorded edits come off the journal")),
            }
            Box::new(planned)
        }
        _ => {
            let store = locallibrary::Store::open_or_init(dir.path().join("store")).unwrap();
            let pull = locallibrary::api::PullRequest {
                drive: root.display().to_string(),
                library: Some("Demo".into()),
                new_library: true,
                ..Default::default()
            };
            let mut planned = locallibrary::api::pull_plan(store.root(), &pull, &locallibrary::Ctx::silent()).unwrap();
            planned.warnings = warnings;
            planned.destructive = tc.draw(gs::booleans());
            want.extend(planned.summary.iter().map(|s| format!("• {s}")));
            let deletes = "This deletes or replaces what's there now.".to_string();
            if planned.destructive {
                want.push(deletes)
            } else {
                not.push(deletes)
            }
            Box::new(planned)
        }
    };
    let s = confirmation_screen(&mut app, root, plan);
    for said in &want {
        assert!(s.contains(said.as_str()), "{said:?} isn't said:\n{s}");
    }
    for said in &not {
        assert!(!s.contains(said.as_str()), "{said:?} is said:\n{s}");
    }
    // At any size it draws, its lines kept to the screen.
    let (w, h) = (
        tc.draw(gs::integers::<u16>().min_value(1).max_value(200)),
        tc.draw(gs::integers::<u16>().min_value(1).max_value(60)),
    );
    assert_eq!(screen(&mut app, w, h).lines().count(), h as usize);
}

/// The library's screen says what's going on, whatever else is: a playlist being picked (in the
/// footer and over the tree), the drive unplugged, an offer to replay edits, why the search
/// doesn't read or what it waits for, the sort, a job's progress and a set's tracks that aren't on
/// the drive (or that the sets are being read); with no drive, whether one is being read; and the
/// selected track's details, with why there are no matches.
#[hegel::test(test_cases = 100)]
fn the_library_says_what_is_going_on(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use locallibrary::sets::{EntryOrigin, SetEntry};

    use super::super::app::{Msg, ReplayOffer};
    use super::super::browse::{Category, NodeId};
    use super::super::edits::Pick;

    let mut want: Vec<String> = Vec::new();
    if tc.draw(gs::booleans()) {
        // No drive: being read, or none.
        let (mut app, _dir) = blank(View::Library);
        let reading = tc.draw(gs::booleans());
        app.loading = reading.then(|| "Reading DNA…".into());
        let s = screen(&mut app, 160, 40);
        let said = if reading { "Reading library…" } else { "No rekordbox drive open." };
        assert!(s.contains(said) && s.contains("no drive open"), "{s}");
        return;
    }
    let mut lib = library();
    lib.tracks.extend(any_tracks(&tc, 3).into_iter().map(|mut t| {
        t.id += 10;
        t.file_path = format!("/Contents/{}.mp3", t.id);
        t
    }));
    let (mut app, _dir) = app_with(lib);
    let generation = app.loaded.as_ref().unwrap().generation;
    let picking = tc.draw(gs::booleans());
    let searching = !picking && tc.draw(gs::booleans());
    if picking {
        app.track_state.select(Some(0));
        let target = app.target().unwrap();
        want.push(format!("Add {} to which playlist?", target.what));
        want.push("Pick a playlist".into());
        app.picking = Some(Pick { target, focus: Focus::Tracks });
    }
    if tc.draw(gs::booleans()) {
        app.loaded.as_mut().unwrap().stale = true;
        want.push("unplugged".into());
    }
    if tc.draw(gs::booleans()) {
        let edits = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
        let root = app.loaded.as_ref().unwrap().root().to_path_buf();
        app.replay_offer = Some(ReplayOffer { root, drive: "DNA".into(), edits });
        let edits = if edits == 1 { "1 edit".to_string() } else { format!("{edits} edits") };
        want.push(format!("rekordbox exported over {edits} on DNA. Replay?"));
    }
    if let Some(field) = tc.draw(gs::optional(gs::sampled_from(&EVERY_FIELD[..]))) {
        let descending = tc.draw(gs::booleans());
        app.sort = Some(SortKey { field, descending });
        app.refresh_visible();
        want.push(format!(" {} {} ", sort_label(field), if descending { "↓" } else { "↑" }));
    }
    if tc.draw(gs::booleans()) {
        let started = std::time::Instant::now() - std::time::Duration::from_secs(3);
        app.job = Some(super::super::edits::Job { detail: "3 of 40 tracks".into(), started, ..job(false, false) });
        want.push("3 of 40 tracks".into());
    }
    // Browse open, a category's values under it.
    if tc.draw(gs::booleans()) {
        let l = app.loaded.as_mut().unwrap();
        l.toggle(&NodeId::Browse);
        l.toggle(&NodeId::Category(Category::Artist));
        let label = l.group(Category::Artist, "daft punk").unwrap().label.clone();
        want.push(format!("· {label}"));
    }
    // Nothing playing, the last run's queue ready to carry on with.
    if !picking && tc.draw(gs::booleans()) {
        let l = app.loaded.as_ref().unwrap();
        let (library, root) = (l.library.clone(), l.root().to_path_buf());
        app.queue =
            super::super::app::Queue { library, items: vec![0], root, drive: "TEST".into(), order: vec![0], pos: 0 };
        app.resume = Some(std::time::Duration::from_secs(95));
        // (Wrapped to the player's width.)
        want.extend(["space carries on with".to_string(), "1:35".into()]);
    }
    // A set shown: some of its tracks played from another drive, the sets read or being read.
    if tc.draw(gs::booleans()) {
        let lib = app.loaded.as_ref().unwrap().library.clone();
        let on_drive = tc.draw(gs::integers::<usize>().max_value(2));
        let mut entries: Vec<SetEntry> =
            lib.tracks.iter().take(on_drive).map(|t| SetEntry::from_track(t, EntryOrigin::Session, None)).collect();
        entries.push(SetEntry { file: Some("/Contents/elsewhere.mp3".into()), ..SetEntry::added("Elsewhere") });
        let read = tc.draw(gs::booleans());
        if read {
            app.handle_msg(Msg::Sets(generation, Ok(vec![set("set1", "2026-10-03", Some("Friday"), None, entries)])));
        }
        app.loaded.as_mut().unwrap().node = NodeId::Group(Category::Sets, "set1".into());
        app.refresh_visible();
        want.push(match (read, on_drive) {
            (false, _) => "Reading the sets…".into(),
            (true, 0) => "None of its tracks are on this drive.".into(),
            (true, _) => "· 1 not on this drive".into(),
        });
    } else if tc.draw(gs::booleans()) {
        // A list with nothing in it (a folder, shown as a list).
        app.loaded.as_mut().unwrap().node = NodeId::Playlist(1);
        app.refresh_visible();
        if !searching {
            want.push("Nothing in this list.".into());
        }
    }
    if searching {
        app.searching = true;
        match tc.draw(gs::booleans()) {
            true => app.filter_error = Some("bpm: needs a number".into()),
            false => app.filter_note = Some("reading My Tags…".into()),
        }
        want.push(app.filter_error.clone().or(app.filter_note.clone()).unwrap());
    }
    // The selected track's details: found (with the playlists it's in), its lookup failed, or
    // not done yet.
    if !picking && tc.draw(gs::booleans()) {
        app.show_details = true;
        app.track_state.select(Some(tc.draw(gs::integers::<usize>().max_value(app.visible.len().saturating_sub(1)))));
        if let Some(i) = app.selected_track() {
            let how = tc.draw(gs::integers::<u8>().max_value(2));
            match how {
                0 => {
                    app.handle_msg(Msg::Failed(super::super::app::Work::Details(generation, i), "it fell over".into()))
                }
                1 => {
                    // Its key: one the wheel knows, one it does not, or none. With no matches, that says why.
                    let key = tc.draw(gs::sampled_from(&["8A", "not a key", ""][..]));
                    let l = app.loaded.as_mut().unwrap();
                    std::sync::Arc::make_mut(&mut l.library).tracks[i].key = key.into();
                    let matches =
                        Matches { library: l.library.clone(), root: l.root().to_path_buf(), items: Vec::new() };
                    let playlists = vec!["Sets › Friday".to_string()];
                    app.details = Some(super::super::app::Details { generation, index: i, matches, playlists });
                    want.push("in 1 playlist".into());
                    want.push(
                        if key == "8A" { "nothing in a compatible key and tempo" } else { "no key to match" }.into(),
                    );
                    if key == "not a key" {
                        want.push(key.into());
                    }
                }
                _ => {}
            }
            let t = &app.loaded.as_ref().unwrap().library.tracks[i];
            if t.year > 0 {
                want.push(t.year.to_string());
            }
            if how != 1 {
                want.push(if how == 0 { "couldn't look for matches (! says why)" } else { "finding matches…" }.into());
            }
        }
    }
    // A question, a report or the outputs over it all, or a name being typed.
    let over = tc.draw(gs::integers::<u8>().max_value(4));
    if over > 0 {
        want.retain(|w| {
            !w.starts_with("Add ")
                && !w.starts_with("rekordbox exported")
                && !w.starts_with("bpm:")
                && !w.starts_with("reading My")
        });
    }
    want.extend(put_over(&mut app, over));
    let s = screen(&mut app, 240, 50);
    for said in &want {
        assert!(s.contains(said.as_str()), "{said:?} isn't said:\n{s}");
    }
    let (w, h) = (
        tc.draw(gs::integers::<u16>().min_value(1).max_value(240)),
        tc.draw(gs::integers::<u16>().min_value(1).max_value(60)),
    );
    assert_eq!(screen(&mut app, w, h).lines().count(), h as usize);
    // Each of the others over it says what it says too (as drawn, one would seldom come up).
    for other in (1..=4).filter(|&o| o != over) {
        (app.modal, app.deck.outputs) = (None, None);
        let says = put_over(&mut app, other);
        let s = screen(&mut app, 240, 50);
        for said in &says {
            assert!(s.contains(said.as_str()), "{said:?} isn't said:\n{s}");
        }
    }
}

/// [`the_library_says_what_is_going_on`]'s overlay `over` (none for 0), put over the screen;
/// what it says.
fn put_over(app: &mut App, over: u8) -> Vec<String> {
    match over {
        1 => {
            app.modal = Some(Modal::Ask("DNA.xml is there already. Replace it?".into(), Box::new(|_: &mut App| {})));
            vec!["Sure?".to_string(), "Replace it?".into(), "y yes  n no".into()]
        }
        2 => {
            use super::super::drives::{Report, Tone};
            let lines = vec![
                (Tone::Ok, "the export reads".to_string()),
                (Tone::Warning, "1 warning".into()),
                (Tone::Problem, "2 tracks won't load".into()),
            ];
            app.modal = Some(Modal::Report(Report { title: "Check · DNA".into(), lines, scroll: 0 }));
            vec!["✓ the export reads".to_string(), "⚠ 1 warning".into(), "✖ 2 tracks won't load".into()]
        }
        3 => {
            app.modal = None;
            app.deck.outputs = Some(super::super::deck::Outputs {
                names: vec!["MacBook Speakers".into(), "DJM-A9".into()],
                default: Some("MacBook Speakers".into()),
                state: ListState::default().with_selected(Some(0)),
            });
            vec!["the system's default (MacBook Speakers)".to_string(), "DJM-A9".into()]
        }
        // A name being typed, in the footer.
        4 => {
            let root = std::path::PathBuf::from("/Volumes/DNA");
            let purpose = super::super::edits::PromptFor::RestorePlaylist;
            app.modal = Some(Modal::Prompt(super::super::edits::Prompt {
                label: "Bring back the playlist or folder".into(),
                text: "Sets/Friday".into(),
                purpose,
                root,
            }));
            vec![" Bring back the playlist or folder: Sets/Friday".to_string(), "enter ok · esc cancel".into()]
        }
        _ => Vec::new(),
    }
}

/// The Drives view says what's known of a drive, whatever it is: where it stands against the
/// store (and how far a set brought it on), an error reading it, the players' library on it, a
/// copy of it plugged in too, its last checks and problems, an operation left half done; checking,
/// it says so, and what reading the store says when it can't. A library's row lists its clones
/// plugged in and not (each with where it stands), and the store's latest snapshot of it. The
/// Snapshots view says when they're being read, can't be, or there are none.
#[hegel::test(test_cases = 30, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_drives_view_says_what_is_known(tc: hegel::TestCase) {
    use chrono::TimeZone;
    use hegel::generators as gs;
    use locallibrary::fleet::{CheckEntry, DriveLog, KnownDrive, Problem};
    use locallibrary::hash::Hash;
    use locallibrary::store::SnapshotInfo;

    let at = chrono::Utc.with_ymd_and_hms(2026, 10, 4, 21, 30, 0).unwrap();
    let (mut app, dir) = blank(View::Snapshots);
    let history = tc.draw(gs::integers::<u8>().max_value(2));
    app.history = match history {
        0 => None,
        1 => Some(Err("no store here".into())),
        _ => Some(Ok(Vec::new())),
    };
    let said = ["loading…", "no store here", "The store has no snapshots yet."][history as usize];
    let s = screen(&mut app, 120, 30);
    assert!(s.contains(said), "{said}\n{s}");

    app.view = View::Drives;
    let home = Uuid::from_u128(7);
    app.library_names = [(home, "Home".to_string())].into_iter().collect();
    let root = dir.path().join("DNA");
    std::fs::create_dir_all(&root).unwrap();
    let mut info = DeviceInfo {
        name: Some("DNA".into()),
        library_id: Some(home),
        has_export: true,
        ..locallibrary::Device::open(&root).unwrap().info()
    };
    info.has_library_plus = tc.draw(gs::booleans());
    info.summary = Some(locallibrary::model::LibrarySummary {
        tracks: 120,
        playlists: 8,
        folders: 2,
        artists: 90,
        audio_bytes: 2_000_000_000,
        total_duration_secs: 30_000,
        tracks_without_artwork: 0,
    });
    // (Its space made up: the machine's varies.)
    (info.total_space, info.free_space) = (Some(16_000_000_000), Some(4_000_000_000));
    let pending = tc.draw(gs::booleans());
    if pending {
        info.pending = Some(locallibrary::device::PendingOp {
            op: locallibrary::device::LineageOp::Sync,
            started: at,
            description: "sync from BACKUP".into(),
            backup: None,
            player: None,
        });
    }
    app.drives = vec![info.clone()];
    app.select_drive_row(DriveRow::Drive(0));
    let relation = tc.draw(gs::sampled_from(
        &[
            None,
            Some(Relation::Identical),
            Some(Relation::RightNewer),
            Some(Relation::LeftNewer),
            Some(Relation::Diverged),
            Some(Relation::Unrelated),
        ][..],
    ));
    let library = tc.draw(gs::booleans()).then_some("Home");
    let back = tc.draw(gs::sampled_from(&[None, Some(0usize), Some(1), Some(3)][..]));
    let error = tc.draw(gs::booleans()).then_some("bad export");
    let copy = tc.draw(gs::booleans());
    let (scrub, problems) = (tc.draw(gs::optional(gs::booleans())), tc.draw(gs::booleans()));
    let log = DriveLog {
        last_scrub: scrub.map(|ok| CheckEntry { at, ok, summary: "all 120 tracks read".into() }),
        problems: if problems {
            vec![Problem { at, kind: "write_failed".into(), detail: "disk full".into() }]
        } else {
            Vec::new()
        },
        ..Default::default()
    };
    let mut fd = fleet(info.clone(), library);
    (fd.status.relation, fd.status.back_from_set, fd.status.error) = (relation, back, error.map(Into::into));
    (fd.log, fd.same_identity_as) = (Some(log), if copy { vec!["/Volumes/DNA 1".into()] } else { Vec::new() });
    let status = tc.draw(gs::integers::<u8>().max_value(2));
    app.status_loading = status == 0;
    app.status = match status {
        0 => None,
        1 => Some(Err("the store can't be read".into())),
        _ => Some(Ok(vec![fd])),
    };
    let s = screen(&mut app, 160, 60);
    let mut want: Vec<String> = Vec::new();
    if info.has_library_plus {
        want.push("OneLibrary (exportLibrary.db) present".into());
    }
    if pending {
        want.push("⚠ interrupted: sync from BACKUP".into());
    }
    match status {
        0 => want.push("checking…".into()),
        1 => want.push("the store can't be read".into()),
        _ => {
            want.push(match (error, relation) {
                (Some(e), _) => format!("error: {e}"),
                (None, None) if library.is_none() => "not in the store".into(),
                (None, None) => "library has no snapshots".into(),
                (None, Some(Relation::Identical)) => "up to date".into(),
                (None, Some(Relation::RightNewer)) => match back {
                    Some(0) => "back from a set".into(),
                    Some(n) => format!("back from a set: {}", plural(n, "new session")),
                    None => "has changes not in the store".into(),
                },
                (None, Some(Relation::LeftNewer)) => "behind the store".into(),
                (None, Some(Relation::Diverged)) => "diverged from the store".into(),
                (None, Some(other)) => format!("{other:?}").to_lowercase(),
            });
            if copy {
                want.push("a copy of it (or it of) /Volumes/DNA 1".into());
            }
            if let Some(ok) = scrub {
                want.push(format!(
                    "{} {}  all 120 tracks read",
                    if ok { "✓" } else { "✖" },
                    local_time(at, "%Y-%m-%d")
                ));
            }
            if problems {
                want.push(format!("✖ {} write failed: disk full", local_time(at, "%Y-%m-%d")));
            }
        }
    }
    for w in &want {
        assert!(s.contains(w.as_str()), "{w:?}\n{s}");
    }
    // Wherever it stands against the store (as drawn, an error or the status unread would hide
    // it), that's said, and what to do about it.
    let kept = (app.status.take(), app.status_loading);
    for (relation, back, says) in [
        (Relation::Identical, None, "✓ up to date"),
        (Relation::RightNewer, Some(2), "▲ back from a set: 2 new sessions"),
        (Relation::RightNewer, None, "▲ has changes not in the store"),
        (Relation::LeftNewer, None, "▼ behind the store"),
        (Relation::Diverged, None, "⑂ diverged from the store"),
        (Relation::Unrelated, None, "? unrelated"),
    ] {
        let mut fd = fleet(info.clone(), Some("Home"));
        (fd.status.relation, fd.status.back_from_set) = (Some(relation), back);
        (app.status, app.status_loading) = (Some(Ok(vec![fd])), false);
        app.select_drive_row(DriveRow::Drive(0));
        let s = screen(&mut app, 160, 60);
        assert!(s.contains(says), "{says:?}\n{s}");
    }
    // A drive of no library in the store: its row says how to start one.
    (app.status, app.status_loading) = (Some(Ok(vec![fleet(info.clone(), None)])), false);
    app.select_drive_row(DriveRow::Unlinked);
    let s = screen(&mut app, 160, 60);
    assert!(s.contains("These drives aren't in the store yet."), "{s}");
    (app.status, app.status_loading) = kept;
    app.select_drive_row(DriveRow::Drive(0));

    // The library's row, with a clone not plugged in.
    let standing = tc.draw(gs::sampled_from(&[0u8, 1, 2, 3][..]));
    let spare = KnownDrive {
        id: Uuid::new_v4(),
        name: "Spare".into(),
        role: None,
        library: Some("Home".into()),
        mounted: None,
        last_mount: dir.path().join("Spare"),
        last_seen: at,
        capacity: None,
        standing: match standing {
            0 => Standing::Behind { snapshots: 2, held: "1a2b3c".into() },
            1 => Standing::UpToDate,
            2 => Standing::Unsaved,
            _ => Standing::Unknown,
        },
        log: DriveLog::default(),
    };
    let head = tc.draw(gs::booleans()).then(|| SnapshotInfo {
        id: Hash::of(b"head"),
        short_id: "3f9a2c".into(),
        library_id: home,
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
    });
    let known = tc.draw(gs::booleans());
    let identical = tc.draw(gs::optional(gs::booleans()));
    let unsaved = tc.draw(gs::booleans());
    app.fleet_libraries = if known {
        vec![FleetLibrary {
            id: home,
            name: "Home".into(),
            head: head.clone(),
            mounted: vec![root.clone()],
            offline: vec![spare],
            identical,
            unsaved,
        }]
    } else {
        Vec::new()
    };
    app.select_drive_row(DriveRow::Library(home));
    let s = screen(&mut app, 160, 60);
    let mut want: Vec<String> = vec!["● DNA".into()];
    if known {
        want.push("○ Spare".into());
        want.push(
            ["· 2 snapshots behind", "· up to date", "· had changes not in the store", "not plugged in"]
                [standing as usize]
                .into(),
        );
        if let Some(h) = &head {
            want.push(format!("latest      {}  {}", h.short_id, local_time(h.created, "%Y-%m-%d %H:%M")));
        }
        if let Some(same) = identical {
            want.push(if same { "identical".into() } else { "not identical".into() });
        }
        want.push(match (&head, unsaved) {
            (None, _) => "no snapshots yet".into(),
            (Some(_), true) => "unsaved".into(),
            (Some(_), false) => "saved".into(),
        });
    } else if status == 0 {
        want.push("checking…".into());
    }
    for w in &want {
        assert!(s.contains(w.as_str()), "{w:?}\n{s}");
    }
    for (w, h) in [(80, 24), (40, 12)] {
        assert_eq!(screen(&mut app, w, h).lines().count(), h as usize);
    }
}

/// Plugins' columns go before tempo, key and time, in the order asked for; one shown already as
/// the sort's stays where it is.
#[test]
fn plugins_columns_go_before_tempo_key_and_time() {
    let (bpm, key) = (FieldName::new("sound.bpm"), FieldName::new("sound.key"));
    let base = columns(80, None, false, false);
    assert_eq!(
        with_fields(base.clone(), &[bpm, key]),
        [Col::Title, Col::Artist, Col::Field(bpm), Col::Field(key), Col::Bpm, Col::Key, Col::Time]
    );
    assert_eq!(with_fields(base.clone(), &[]), base);
    let sorted = columns(80, Some(SortKey { field: SortField::Field(key), descending: false }), false, false);
    assert_eq!(sorted[2], Col::Field(key), "the sort column, not otherwise shown");
    assert_eq!(with_fields(sorted, &[bpm, key]).iter().filter(|c| **c == Col::Field(key)).count(), 1);
}

/// A plugin's value as its column shows it: numbers to two places at most, a list joined.
#[test]
fn plugins_values_show_shortly() {
    let text = |v: FieldValue| field_cell(&v).to_string();
    assert_eq!(text(FieldValue::Number(121.0)), "121");
    assert_eq!(text(FieldValue::Number(121.256)), "121.26");
    assert_eq!(text(FieldValue::Number(-8.5)), "-8.5");
    assert_eq!(text(FieldValue::Integer(4000)), "4000");
    assert_eq!(text(FieldValue::Boolean(true)), "yes");
    assert_eq!(text(FieldValue::Strings(vec!["deep".into(), "warm".into()])), "deep, warm");
}

/// With the My Tags column shown and the drive's tags read, each track's row lists its tags by
/// their own names (not the categories they're under), in the order the drive has them; a track
/// without any shows none.
#[hegel::test(test_cases = 50)]
fn the_my_tags_column_lists_each_tracks_tags(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use std::sync::Arc;
    let (mut app, _dir) = app_with(library());
    app.show_tags_column = true;
    let names = ["Genre/Deep", "Vibe/Moody", "Peak", "Situation/Warm Up"];
    let l = app.loaded.as_mut().unwrap();
    let tracks = l.library.tracks.clone();
    let mut tags = rustc_hash::FxHashMap::default();
    let mut want = Vec::new();
    for t in &tracks {
        let mine: Vec<String> = names.iter().filter(|_| tc.draw(gs::booleans())).map(|n| n.to_string()).collect();
        want.push(mine.iter().map(|n| n.rsplit('/').next().unwrap().to_string()).collect::<Vec<_>>());
        tags.insert(locallibrary::paths::file_key(&t.file_path), mine);
    }
    l.my_tags = super::super::app::MyTags::Read(Arc::new(tags));
    let s = screen(&mut app, 200, 30);
    // The rows under the column headers, in the order the list shows the tracks.
    let rows: Vec<&str> = s.lines().skip_while(|l| !l.contains("My Tags")).skip(1).take(app.visible.len()).collect();
    assert_eq!(rows.len(), app.visible.len(), "{s}");
    for (&i, row) in app.visible.iter().zip(rows) {
        let (title, leaves) = (&tracks[i].title, &want[i]);
        assert!(row.contains(title.as_str()), "{title} isn't on {row}\n{s}");
        if leaves.is_empty() {
            for n in names {
                assert!(!row.contains(n.rsplit('/').next().unwrap()), "{title} has no tags: {row}");
            }
        } else {
            assert!(row.contains(&leaves.join(", ")), "{title} has {leaves:?}: {row}");
        }
        assert!(!row.contains("Genre/") && !row.contains("Vibe/"), "{row}");
    }
}
