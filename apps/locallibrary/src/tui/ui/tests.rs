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
