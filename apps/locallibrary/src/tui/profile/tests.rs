use ratatui::crossterm::event::KeyCode;

use super::super::app::View;
use super::super::app::testing::*;
#[cfg(feature = "account")]
use super::super::edits::Modal;
use super::super::ui::screen;
use super::*;

/// The view drawn at `w`×`h`, as text.
fn render(t: &mut ProfileTab, w: u16, h: u16) -> String {
    let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    term.draw(|f| draw_tab(f, t, f.area())).unwrap();
    let buf = term.backend().buffer().clone();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A tab holding the profile of the fixture export (twelve tracks, no history).
fn fixture_tab() -> ProfileTab {
    fixture_tab_with(&[])
}

/// [`fixture_tab`], with history lists of the export's tracks.
fn fixture_tab_with(history: &[&[u32]]) -> ProfileTab {
    let dir = tempfile::tempdir().unwrap();
    let pdb = dir.path().join("PIONEER/rekordbox/export.pdb");
    std::fs::create_dir_all(pdb.parent().unwrap()).unwrap();
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/rekordbox-pdb/tests/data/bigger-export.pdb");
    std::fs::copy(fixture, &pdb).unwrap();
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb).unwrap();
    for (i, tracks) in history.iter().enumerate() {
        let id = ed.create_history_playlist(&format!("HISTORY {:03}", i + 1)).unwrap();
        for t in *tracks {
            ed.add_to_history(id, *t).unwrap();
        }
    }
    ed.save(&pdb).unwrap();
    let req = ProfileRequest { drives: vec![dir.path().display().to_string()], ..Default::default() };
    let view = api::profile(None, &req, &Ctx::silent()).unwrap();
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut t = ProfileTab::new(tx);
    t.view = Some(Ok(view));
    t
}

#[test]
fn every_section_draws_at_any_size() {
    let mut t = fixture_tab();
    for (w, h) in [(160, 48), (80, 24), (40, 12), (10, 4)] {
        for (i, s) in SECTIONS.iter().enumerate() {
            t.select_section(i);
            let screen = render(&mut t, w, h);
            if w >= 80 {
                assert!(screen.contains(s.title()), "{}:\n{screen}", s.title());
            }
        }
        t.document = true;
        let screen = render(&mut t, w, h);
        if w >= 80 {
            assert!(screen.contains("\"version\": 2"), "{screen}");
        }
        t.document = false;
    }
}

/// The view lists only the sections that can be published, under an overview of the collection
/// titled just Profile: no generated headline, traits, sources or scope.
#[test]
fn only_what_can_be_published_is_listed() {
    assert_eq!(SECTIONS.to_vec(), Section::ALL.into_iter().filter(|s| s.publishable()).collect::<Vec<_>>());
    let mut t = fixture_tab();
    let screen = render(&mut t, 160, 40);
    for s in Section::ALL.into_iter().filter(|s| !s.publishable()) {
        assert!(!screen.contains(s.title()), "{s:?} is listed:\n{screen}");
    }
    let r = t.report().unwrap();
    let header = screen.lines().take(4).collect::<Vec<_>>().join("\n");
    assert!(header.contains("Profile") && !header.contains("Profile ·"), "{header}");
    assert!(header.contains(&format!("12 tracks · {} genres", r.genres.distinct)), "{header}");
    for gone in ["every drive", "the store", "traits", "document:", "rounded", "BPM"] {
        assert!(!header.contains(gone), "{gone}:\n{header}");
    }
}

/// Without declared sets, the view counts sessions and calls them that.
#[test]
fn sessions_are_called_sessions() {
    let mut t = fixture_tab_with(&[&[1, 2, 3, 4, 5, 6], &[9, 10, 1, 2, 12, 7]]);
    t.select_section(SECTIONS.iter().position(|s| *s == Section::Sets).unwrap());
    let screen = render(&mut t, 160, 30);
    assert!(screen.lines().nth(2).is_some_and(|l| l.contains(" · 2 sessions")), "{screen}");
    assert!(screen.contains("2 sessions · 6 tracks in a typical session"), "{screen}");
    assert!(screen.contains("First seen") && screen.contains("HISTORY 001"), "{screen}");
    assert!(screen.contains("only on this computer"), "{screen}");
    assert!(!screen.contains(" Sets ") && !screen.contains("2 sets"), "{screen}");
}

#[test]
fn keys_change_the_view_and_save_choices() {
    use super::super::app::View;

    let store = tempfile::tempdir().unwrap();
    let mut app = App::blank(store.path().to_path_buf(), None);
    app.profile = fixture_tab();
    app.view = View::Profile;
    let published = |app: &App| match &app.profile.view {
        Some(Ok(v)) => v.published.clone(),
        _ => panic!("no profile"),
    };

    key(&mut app, KeyCode::Down);
    assert_eq!(app.profile.section(), Section::Energy);
    key(&mut app, KeyCode::Up);
    assert_eq!(app.profile.section(), Section::Sets);
    assert!(published(&app).contains(&Section::Sets));
    app.perform(Act::ProfileInclude, None);
    assert!(!published(&app).contains(&Section::Sets));
    let saved = locallibrary::profile::ProfilePrefs::load(&Store::open(store.path()).unwrap()).unwrap();
    assert!(!saved.published().contains(&Section::Sets), "choices are saved as they're made");

    // Into the artist list, and hide the top artist.
    let artists = SECTIONS.iter().position(|s| *s == Section::Artists).unwrap();
    app.profile.select_section(artists);
    key(&mut app, KeyCode::Tab);
    assert_eq!(app.profile.pane, Pane::Detail);
    press(&mut app, "eh");
    let Some(Ok(v)) = &app.profile.view else { panic!() };
    assert_eq!(v.prefs.hidden_artists, ["[a]pendics.shuffle"]);
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.profile.pane, Pane::Sections);

    press(&mut app, "e#");
    let Some(Ok(v)) = &app.profile.view else { panic!() };
    assert_eq!(v.document.precision, Precision::Exact);
    app.perform(Act::ProfileDocument, None);
    assert!(app.profile.document);
    key(&mut app, KeyCode::Esc);
    assert!(!app.profile.document);

    // Keys the view doesn't use still work.
    press(&mut app, "1");
    assert_eq!(app.view, View::Library);
    press(&mut app, "5");
    assert_eq!(app.view, View::Profile);
    for _ in 0..4 {
        key(&mut app, KeyCode::Esc);
    }
    assert!(app.quit);
}

/// Signed out, the account bar names the key whose menu signs in: `e`.
#[cfg(feature = "account")]
#[test]
fn the_account_bar_says_which_key_signs_in() {
    let line: String = Account::default().line(200).spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(line.contains("  e sign in with your Atmosphere account"), "{line}");
}

/// A long, wide top genre (the user's own text) is cut to fit the Sections column, and the
/// section's name stays whole.
#[test]
fn a_long_top_genre_fits_the_sections_column() {
    let mut t = fixture_tab();
    let Some(Ok(v)) = &mut t.view else { panic!("no profile") };
    let name = "日本の夜明けとともに始まる長いジャンル".to_string();
    v.report.genres.top.insert(0, locallibrary::profile::Share { name, count: 1, share: 1.0 });
    let screen = render(&mut t, 120, 40);
    // A wide character's second column draws as a space.
    let row = screen.lines().find(|l| l.contains("日 本")).unwrap_or_else(|| panic!("{screen}"));
    assert!(row.contains("Genres") && row.contains('…'), "{row}");
}

/// Bars of wide characters fit their columns like any other (`fit` pads to exactly `width`).
#[test]
fn fit_counts_columns() {
    use unicode_width::UnicodeWidthStr;
    for s in ["Friday", "日本の夜明けとともに", "ab😹😹😹"] {
        for width in 0..12 {
            assert_eq!(fit(s, width).width(), width, "{s:?} in {width}");
        }
    }
}

#[cfg(feature = "account")]
#[test]
fn signing_in_takes_a_handle_and_signing_out_asks_first() {
    use super::super::app::View;

    let store = tempfile::tempdir().unwrap();
    let mut app = App::blank(store.path().to_path_buf(), None);
    app.profile = fixture_tab();
    app.view = View::Profile;
    assert!(render(&mut app.profile, 120, 30).contains("Not signed in"));

    app.perform(Act::ProfileAccount, None);
    press(&mut app, "dj.example.comq");
    assert!(!app.quit, "typing a handle doesn't run commands");
    key(&mut app, KeyCode::Backspace);
    let screen = render(&mut app.profile, 120, 30);
    assert!(screen.contains("Atmosphere handle or server: dj.example.com"), "{screen}");
    key(&mut app, KeyCode::Esc);
    assert!(!app.profile.account.typing());

    let account: locallibrary::api::AccountInfo = serde_json::from_value(serde_json::json!({
        "did": "did:plc:abc",
        "handle": "dj.example.com",
        "pds": "https://pds.example",
        "auth_server": "https://pds.example",
        "scopes": ["atproto"],
        "signed_in_at": "2026-10-05T12:00:00Z",
        "sign_in_again_by": "2099-10-19T12:00:00Z",
        "access_expires_at": null,
        "client": { "kind": "loopback" },
        "storage": "file"
    }))
    .unwrap();
    app.profile.account.info = Some(account);
    let screen = render(&mut app.profile, 140, 30);
    assert!(screen.contains("Atmosphere account @dj.example.com as the development client"), "{screen}");
    assert!(!screen.contains("sign in again"), "far off, when to sign in again isn't said:\n{screen}");
    // Within a couple of days, it is.
    let soon = chrono::Utc::now() + chrono::Duration::hours(30);
    app.profile.account.info.as_mut().unwrap().sign_in_again_by = soon;
    let screen = render(&mut app.profile, 140, 30);
    assert!(screen.contains("sign in again by") && screen.contains("(1 days)"), "{screen}");
    app.perform(Act::ProfileAccount, None);
    assert!(matches!(&app.modal, Some(Modal::Ask(q, _)) if q == "Sign out of @dj.example.com?"));
    press(&mut app, "y");
    assert!(app.profile.account.busy(), "yes signs out");

    // The sign-out thread's answer: nobody was signed in in this store folder.
    let done = super::super::account::AccountEvent::Done { message: "not signed in".into(), error: false };
    super::super::account::on_event(&mut app, done);
    assert!(app.profile.account.info.is_none() && !app.profile.account.busy());
}

#[test]
fn loading_and_errors_draw() {
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut t = ProfileTab::new(tx);
    t.loading = true;
    assert!(render(&mut t, 80, 10).contains("Building profile"));
    t.view = Some(Err("no store".into()));
    assert!(render(&mut t, 80, 10).contains("R tries again"));
}

#[test]
fn columns_stack_eighths() {
    let lines = columns(&[Some(0.0), Some(1.0), None], 0.0, 2, 2, |_| theme::current().info);
    assert_eq!(lines.len(), 2);
    let top: String = lines[0].spans.iter().map(|s| s.content.to_string()).collect();
    let bottom: String = lines[1].spans.iter().map(|s| s.content.to_string()).collect();
    assert_eq!(top, "  █   ");
    assert_eq!(bottom, "▁ █   ");
}

#[test]
fn json_lines_colour_keys_and_values() {
    let l = json_line("  \"headline\": \"Techno warm-ups\",");
    let text: String = l.spans.iter().map(|s| s.content.to_string()).collect();
    assert_eq!(text, "  \"headline\": \"Techno warm-ups\",");
    assert_eq!(json_line("}").spans.len(), 2);
}

/// [`fixture_tab_with`] two sessions, as a profile of declared sets, every section filled
/// in with made-up numbers.
fn full_tab() -> ProfileTab {
    use locallibrary::profile::{DecadeCount, EnergySection, SetInfo};
    let mut t = fixture_tab_with(&[&[1, 2, 3, 4, 5, 6], &[9, 10, 1, 2, 12, 7]]);
    let Some(Ok(v)) = &mut t.view else { panic!("no profile") };
    let r = &mut v.report;
    r.basis = Basis::Plays;
    (r.sets.not_archived, r.sets.too_short) = (1, 2);
    r.sets.list.push(SetInfo {
        id: "0123456789abcdef".into(),
        name: "Friday at the Bunker".into(),
        place: "DNA".into(),
        date: None,
        tracks: 6,
        opening_bpm: Some(122.0),
        closing_bpm: Some(128.0),
        harmonic_share: Some(0.5),
    });
    r.energy = Some(EnergySection { tracks: 12, high: 0.5, mid: 0.3, low: 0.2, curve: vec![Some(0.2), Some(1.0)] });
    r.era.tracks = 10;
    (r.era.median_year, r.era.oldest, r.era.newest, r.era.recent_share) =
        (Some(2019), Some(1998), Some(2024), Some(0.3));
    r.era.decades = vec![DecadeCount { decade: 1990, count: 2 }, DecadeCount { decade: 2010, count: 8 }];
    (r.collection.played_share, r.collection.duplicate_files) = (Some(0.4), 3);
    add_artists(r);
    t
}

/// Eight more artists under Artists (the fixture has one), so its list has rows to move through.
fn add_artists(r: &mut ProfileReport) {
    for i in 0..8 {
        let name = format!("Artist {i}");
        r.artists.top.push(locallibrary::profile::Share { name, count: 1, share: 0.05 });
    }
    r.artists.distinct = r.artists.top.len();
}

/// The screen with section `s` selected.
fn showing(t: &mut ProfileTab, s: Section) -> String {
    t.select_section(SECTIONS.iter().position(|x| *x == s).unwrap());
    render(t, 140, 40)
}

#[test]
fn every_section_says_what_its_numbers_show() {
    let mut t = full_tab();
    let r = t.report().unwrap();
    let overview = format!(
        "12 tracks · {} · {} · released 1998–2024 · 2 sets",
        plural(r.genres.distinct, "genre"),
        plural(r.artists.distinct, "artist")
    );
    for (s, says) in [
        (
            Section::Sets,
            &[
                "1 only on a drive (pull archives them) · 2 shorter sessions left out",
                "on a drive",
                "122 → 128",
                "01234567",
            ][..],
        ),
        (Section::Energy, &["mood, 12 tracks", "high", "50%", "Mood through a set"]),
        (Section::Era, &["median 2019", "(1998–2024) · 30% released this year or last", "1990s", "2010s"]),
        (Section::Collection, &["Played in a set", "40%", "3 more files are copies of these"]),
    ] {
        let screen = showing(&mut t, s);
        assert!(screen.contains(&overview), "{overview}\n{screen}");
        for said in says {
            assert!(screen.contains(said), "{s:?}: {said}\n{screen}");
        }
    }
}

/// A section without the data it needs says what it needs.
#[test]
fn empty_sections_say_what_they_need() {
    let mut t = fixture_tab();
    let Some(Ok(v)) = &mut t.view else { panic!("no profile") };
    let r = &mut v.report;
    (r.era.tracks, r.energy) = (0, None);
    r.genres.top.clear();
    for (s, says) in [
        (Section::Energy, "No phrase analysis read"),
        (Section::Genres, "None tagged."),
        (Section::Era, "No release years tagged."),
        (Section::Sets, "No sets declared, and no sessions with enough tracks."),
    ] {
        let screen = showing(&mut t, s);
        assert!(screen.contains(says), "{s:?}: {says}\n{screen}");
    }
}

/// The app's Profile view showing `t`, its store in a temporary folder.
fn profile_app(t: ProfileTab) -> (App, tempfile::TempDir) {
    let (mut app, dir) = blank(View::Profile);
    app.profile = t;
    (app, dir)
}

/// `h` in the actions menu hides the selected artist under Artists, chosen again names them again.
/// Outside that list, it says where to go.
#[test]
fn artists_are_hidden_and_named_again() {
    let (mut app, _store) = profile_app(fixture_tab());
    press(&mut app, "eh");
    assert_eq!(last_message(&app), "select an artist under Artists (tab) to hide them");

    app.profile.select_section(SECTIONS.iter().position(|s| *s == Section::Artists).unwrap());
    press(&mut app, "\t");
    let Some(Ok(v)) = &app.profile.view else { panic!("no profile") };
    let artist = v.report.artists.top[0].name.clone();
    for said in [format!("{artist} won't be named"), format!("{artist} can be named again")] {
        press(&mut app, "eh");
        assert_eq!(last_message(&app), said);
    }
    let Some(Ok(v)) = &app.profile.view else { panic!("no profile") };
    assert!(v.prefs.hidden_artists.is_empty());
}

/// Saving the document (from `:` or the actions menu) writes it into the store folder, made there if there's none.
#[test]
fn w_writes_the_document_to_the_store() {
    let (mut app, _dir) = profile_app(fixture_tab());
    app.perform(Act::ProfileWrite, None);
    let path = app.store_root.join("profile-document.json");
    assert_eq!(last_message(&app), format!("wrote {}", path.display()));
    let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(doc["version"], locallibrary::profile::DOCUMENT_VERSION);

    let (tx, _rx) = std::sync::mpsc::channel();
    app.profile = ProfileTab::new(tx);
    app.perform(Act::ProfileWrite, None);
    assert_eq!(last_message(&app), "nothing to export yet");
}

/// A choice that can't be saved (the store folder is a file) says why, and changes nothing.
#[test]
fn a_choice_that_cant_be_saved_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("store");
    std::fs::write(&file, "not a folder").unwrap();
    let mut app = App::blank(file, None);
    app.profile = fixture_tab();
    app.view = View::Profile;
    let before = match &app.profile.view {
        Some(Ok(v)) => v.published.clone(),
        _ => panic!("no profile"),
    };
    app.profile.select_section(SECTIONS.iter().position(|s| *s == Section::Genres).unwrap());
    app.perform(Act::ProfileInclude, None);
    assert!(app.toast.as_ref().is_some_and(|t| t.1 == Level::Error), "{:?}", app.toast);
    app.perform(Act::ProfileWrite, None);
    assert!(app.toast.as_ref().is_some_and(|t| t.1 == Level::Error), "{:?}", app.toast);
    let Some(Ok(v)) = &app.profile.view else { panic!("no profile") };
    assert_eq!(v.published, before);
}

/// Refreshing (f5) builds the profile in the background, and the header says how far it's got.
#[test]
fn r_builds_the_profile_in_the_background() {
    let dir = tempfile::tempdir().unwrap();
    let pdb = dir.path().join("PIONEER/rekordbox/export.pdb");
    std::fs::create_dir_all(pdb.parent().unwrap()).unwrap();
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/rekordbox-pdb/tests/data/bigger-export.pdb");
    std::fs::copy(fixture, &pdb).unwrap();
    let mut app = App::with_library(dir.path(), library());
    app.view = View::Profile;
    app.profile.only_drive = Some(dir.path().to_path_buf());
    key(&mut app, KeyCode::F(5));
    assert!(app.profile.loading);
    // The header says so while it builds. Counters of the test's own: the build's thread keeps
    // the ones it was given, and counting there can't overwrite these.
    app.profile.progress = Arc::new([AtomicU64::new(3), AtomicU64::new(12)]);
    let s = screen(&mut app, 140, 30);
    assert!(s.contains("Building profile… 3/12 tracks"), "{s}");
    // A build started before this one is stopped, and what it sends back ignored.
    let old = app.profile.generation;
    key(&mut app, KeyCode::F(5));
    app.profile.loaded(old, Err("stale".into()));
    assert!(app.profile.loading && app.profile.view.is_none());

    wait(&mut app, |a| !a.profile.loading);
    assert!(matches!(&app.profile.view, Some(Ok(v)) if v.report.collection.tracks == 12), "{:?}", app.toast);
    let s = screen(&mut app, 140, 30);
    assert!(s.contains("12 tracks ·"), "{s}");
}

/// The pointer picks a section, a click picks the row of its list under it, and the wheel
/// moves through either.
#[test]
fn the_mouse_picks_sections_and_rows() {
    let mut t = fixture_tab();
    let Some(Ok(v)) = &mut t.view else { panic!("no profile") };
    add_artists(&mut v.report);
    let (mut app, _dir) = profile_app(t);
    let artists = spot_in(&mut app, "Artists", |a| a.profile.sections_area);
    mouse(&mut app, LEFT, artists);
    assert_eq!(app.profile.section(), Section::Artists);

    // The row showing the third artist.
    let Some(Ok(v)) = &app.profile.view else { panic!("no profile") };
    let third = v.report.artists.top[2].name.clone();
    let row = spot_in(&mut app, &third, |a| a.profile.detail_area);
    mouse(&mut app, LEFT, row);
    assert_eq!((app.profile.pane, app.profile.rows.selected()), (Pane::Detail, Some(2)));
    mouse(&mut app, MouseEventKind::ScrollDown, row);
    assert_eq!(app.profile.rows.selected(), Some(5));
    mouse(&mut app, MouseEventKind::ScrollUp, artists);
    assert_eq!((app.profile.pane, app.profile.section()), (Pane::Sections, SECTIONS[0]));
    let detail = app.profile.detail_area;
    // The document scrolls with the wheel.
    app.perform(Act::ProfileDocument, None);
    mouse(&mut app, MouseEventKind::ScrollDown, (detail.x + 4, detail.y + 6));
    assert_eq!(app.profile.scroll, 3);
    // Outside the view, the pointer does what it does anywhere.
    mouse(&mut app, MouseEventKind::ScrollDown, (0, 0));
    assert_eq!(app.profile.scroll, 3);
}

/// A full profile (every section filled in) draws at any size,
/// whichever section, pane, row or scroll, and with room, lists every section by its title,
/// the selected one's numbers beside them; the selected row stays among the section's rows.
#[hegel::test(test_cases = 20, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_full_profile_draws_whole_at_any_size(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut t = full_tab();
    let basis = t.report().unwrap().basis;
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(8)) {
        match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 => t.select_section(tc.draw(gs::integers::<usize>().max_value(SECTIONS.len() - 1))),
            1 => t.pane = if t.pane == Pane::Sections && t.detail_len() > 0 { Pane::Detail } else { Pane::Sections },
            2 => t.document = !t.document,
            _ => t.move_by(tc.draw(gs::sampled_from(&[-10, -1, 1, 1, 10][..]))),
        }
        assert!(
            t.rows.selected().is_none_or(|r| r < t.detail_len()),
            "row {:?} of {}",
            t.rows.selected(),
            t.detail_len()
        );
        let (w, h) = (
            tc.draw(gs::integers::<u16>().min_value(1).max_value(200)),
            tc.draw(gs::integers::<u16>().min_value(1).max_value(60)),
        );
        // At any size, and at one with room for everything.
        render(&mut t, w, h);
        let screen = render(&mut t, 140, 45);
        if !t.document {
            for s in SECTIONS {
                assert!(screen.contains(s.title_in(basis)), "{s:?} isn't listed:\n{screen}");
            }
        }
    }
}

/// A section's table scrolled down to its selection on a short screen comes back up once it
/// fits again: drawn short, then tall, its first row and the one selected show.
#[hegel::test(test_cases = 50, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_tables_come_back_up_once_they_fit(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut t = full_tab();
    let r = t.report().unwrap();
    let name = |x: &locallibrary::profile::Share| x.name.clone();
    let tables: Vec<(Section, Vec<String>)> = [
        (Section::Sets, r.sets.list.iter().map(|s| s.name.clone()).collect::<Vec<_>>()),
        (Section::Genres, r.genres.top.iter().map(name).collect()),
        (Section::Artists, r.artists.top.iter().map(name).collect()),
    ]
    .into_iter()
    .filter(|(_, names)| !names.is_empty())
    .collect();
    let (section, names) = tc.draw(gs::sampled_from(&tables[..]));
    t.select_section(SECTIONS.iter().position(|s| *s == section).unwrap());
    let sel = tc.draw(gs::integers::<usize>().max_value(names.len() - 1));
    (t.pane, t.rows) = (Pane::Detail, TableState::default().with_selected(Some(sel)));
    render(&mut t, 140, tc.draw(gs::integers::<u16>().min_value(1).max_value(30)));
    let screen = render(&mut t, 140, 45);
    for i in [0, sel] {
        // As much as any column has room for.
        let name: String = names[i].chars().take(16).collect();
        assert!(screen.contains(&name), "row {i}, {name:?}, isn't shown:\n{screen}");
    }
}

/// The full profile (see [`full_tab`]), worked out once, with the DJ's choices as a store
/// without any has them.
fn full_view() -> ProfileView {
    static VIEW: std::sync::OnceLock<ProfileView> = std::sync::OnceLock::new();
    let mut v = VIEW
        .get_or_init(|| match full_tab().view {
            Some(Ok(v)) => v,
            _ => panic!("no profile"),
        })
        .clone();
    v.prefs = Default::default();
    v.published = v.prefs.published().into_iter().collect();
    v
}

fn tab_of(view: ProfileView) -> ProfileTab {
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut t = ProfileTab::new(tx);
    t.view = Some(Ok(view));
    t
}

/// Each section says what its numbers show, whatever they are: the moods and their curve; the
/// era's decades; how much of the collection was played and its copies; and, without the data,
/// what it needs. The header sums up the collection, and how many sets or sessions there are.
#[hegel::test(test_cases = 25, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn every_section_says_what_any_numbers_show(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use locallibrary::profile::{DecadeCount, EnergySection};
    let mut v = full_view();
    let r = &mut v.report;
    r.basis = tc.draw(gs::sampled_from(&[Basis::Plays, Basis::Sessions, Basis::Collection][..]));
    let shares = &[0.0, 0.2, 0.3, 0.5, 0.6, 1.0][..];
    r.energy = tc.draw(gs::booleans()).then(|| EnergySection {
        tracks: 12,
        high: tc.draw(gs::sampled_from(shares)),
        mid: 0.3,
        low: 0.2,
        curve: tc.draw(gs::vecs(gs::optional(gs::sampled_from(&[0.0, 0.5, 1.0][..]))).max_size(3)),
    });
    r.era.tracks = tc.draw(gs::sampled_from(&[0usize, 10][..]));
    (r.era.oldest, r.era.newest) =
        tc.draw(gs::sampled_from(&[(None, None), (Some(2001), Some(2001)), (Some(1998), Some(2024))][..]));
    r.era.decades = (0..tc.draw(gs::integers::<i32>().min_value(0).max_value(3)))
        .map(|i| DecadeCount { decade: 1990 + 10 * i, count: 1 + i as usize })
        .collect();
    r.sets.count = tc.draw(gs::integers::<usize>().max_value(3));
    r.collection.played_share = tc.draw(gs::optional(gs::sampled_from(shares)));
    r.collection.duplicate_files = tc.draw(gs::integers::<usize>().max_value(3));
    let r = v.report.clone();
    let mut t = tab_of(v);
    let unit = r.basis.unit();
    let mut overview =
        format!("12 tracks · {} · {}", plural(r.genres.distinct, "genre"), plural(r.artists.distinct, "artist"));
    match (r.era.oldest, r.era.newest) {
        (Some(a), Some(b)) if a != b => overview.push_str(&format!(" · released {a}–{b}")),
        (Some(a), _) => overview.push_str(&format!(" · released {a}")),
        _ => {}
    }
    if r.basis != Basis::Collection && r.sets.count > 0 {
        overview.push_str(&format!(" · {}", plural(r.sets.count, unit)));
    }
    for (i, s) in SECTIONS.iter().enumerate() {
        t.select_section(i);
        let screen = render(&mut t, 160, 50);
        let header = screen.lines().nth(2).unwrap_or_default();
        assert!(header.trim_start_matches('│').trim().trim_end_matches('│').trim() == overview, "{overview}\n{screen}");
        let mut want: Vec<String> = Vec::new();
        match s {
            Section::Energy => match &r.energy {
                None => want.push("No phrase analysis read".into()),
                Some(e) => {
                    want.push("mood, 12 tracks".into());
                    want.push(format!("{} high", pct(e.high)));
                    if e.curve.iter().flatten().count() > 1 {
                        want.push(format!("Mood through a {unit}"));
                    }
                }
            },
            Section::Era if r.era.tracks == 0 => want.push("No release years tagged.".into()),
            Section::Era => want.extend(r.era.decades.iter().map(|d| format!("{}s", d.decade))),
            Section::Collection => {
                if let Some(p) = r.collection.played_share {
                    want.push(format!("Played in a {unit}"));
                    want.push(pct(p));
                }
                let d = r.collection.duplicate_files;
                if d > 0 {
                    want.push(format!("{d} more files are copies of these"));
                }
            }
            _ => {}
        }
        for said in &want {
            assert!(screen.contains(said.as_str()), "{s:?}: {said:?}\n{screen}");
        }
    }
}

/// The DJ's choices, made from the view with its keys, are kept as a person keeping count
/// would keep them: h hides an artist under Artists and names them again, i puts the section in
/// the document or leaves it out, and the numbers go exact or rounded; each says what it did, and
/// h used where it means nothing says where to go.
#[hegel::test(test_cases = 30, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_djs_choices_are_kept_as_made(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use std::collections::BTreeSet;
    let (mut app, _dir) = profile_app(tab_of(full_view()));
    let at = |s: Section| SECTIONS.iter().position(|x| *x == s).unwrap();
    let mut artists = BTreeSet::new();
    let mut published: BTreeSet<Section> = match &app.profile.view {
        Some(Ok(v)) => v.published.iter().copied().collect(),
        _ => unreachable!(),
    };
    let mut exact = false;
    const SHOWN: [Section; 3] = [Section::Artists, Section::Genres, Section::Era];
    const ACTS: [Act; 3] = [Act::ProfileHideArtist, Act::ProfileInclude, Act::ProfileRounding];
    // Every act on a row of every section first (as drawn, they'd seldom meet), then as drawn.
    for step in 0..9 + tc.draw(gs::integers::<usize>().max_value(10)) {
        let section = if step < 9 { SHOWN[step / 3] } else { tc.draw(gs::sampled_from(&SHOWN[..])) };
        app.profile.select_section(at(section));
        let detail = (step < 9 || tc.draw(gs::booleans())) && app.profile.detail_len() > 0;
        app.profile.pane = if detail { Pane::Detail } else { Pane::Sections };
        if detail {
            let n = app.profile.detail_len();
            app.profile.rows.select(Some(tc.draw(gs::integers::<usize>().max_value(n - 1))));
        }
        let Some(Ok(v)) = &app.profile.view else { unreachable!() };
        let row = app.profile.rows.selected().filter(|_| detail);
        let artist = match section {
            Section::Artists => row.and_then(|i| v.report.artists.top.get(i)).map(|a| a.name.clone()),
            _ => None,
        };
        let act = if step < 9 { ACTS[step % 3] } else { tc.draw(gs::sampled_from(&ACTS[..])) };
        // Once, or (as often, to take it back) twice.
        for _ in 0..1 + usize::from(tc.draw(gs::booleans())) {
            app.perform(act, None);
            let said = last_message(&app).to_string();
            match act {
                Act::ProfileHideArtist => match &artist {
                    Some(a) => {
                        let key = locallibrary::model::normalize(a);
                        let on = if artists.remove(&key) { false } else { artists.insert(key) };
                        assert_eq!(
                            said,
                            if on { format!("{a} won't be named") } else { format!("{a} can be named again") }
                        );
                    }
                    None => assert_eq!(said, "select an artist under Artists (tab) to hide them"),
                },
                Act::ProfileInclude => {
                    let on = !published.remove(&section) && published.insert(section);
                    let title = section.title();
                    assert_eq!(
                        said,
                        if on {
                            format!("{title} in the document")
                        } else {
                            format!("{title} left out of the document")
                        }
                    );
                }
                _ => {
                    exact = !exact;
                    assert_eq!(said, format!("{} numbers in the document", if exact { "exact" } else { "rounded" }));
                }
            }
            let Some(Ok(v)) = &app.profile.view else { unreachable!() };
            let p = &v.prefs;
            assert_eq!(
                p.hidden_artists.iter().map(|a| locallibrary::model::normalize(a)).collect::<BTreeSet<_>>(),
                artists
            );
            assert_eq!(v.published.iter().copied().collect::<BTreeSet<_>>(), published);
            assert_eq!(p.precision == Precision::Exact, exact);
        }
    }
}

/// The Profile view answers to its keys and the mouse: the document opens and esc closes it,
/// then goes back from a section's list to the sections; the arrows move through the list shown
/// (never past its ends); a click picks a section, or a row of its list; saving writes the document (once
/// there's one to write); a build replaced since changes nothing; a choice that can't be saved
/// says why.
#[hegel::test(test_cases = 20, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_profile_view_answers_to_its_keys_and_the_mouse(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (mut app, dir) = profile_app(tab_of(full_view()));
    let artists = SECTIONS.iter().position(|s| *s == Section::Artists).unwrap();
    let turns: Vec<u8> = tc.draw(gs::permutations((0..7).collect::<Vec<_>>()));
    for step in 0..tc.draw(gs::integers::<usize>().min_value(7).max_value(14)) {
        app.view = View::Profile;
        if !matches!(app.profile.view, Some(Ok(_))) {
            (app.profile.view, app.profile.loading) = (Some(Ok(full_view())), false);
        }
        match turns[step % turns.len()] {
            0 => {
                app.profile.select_section(artists);
                (app.profile.pane, app.profile.document) = (Pane::Sections, false);
                press(&mut app, "\t");
                assert_eq!(app.profile.pane, Pane::Detail);
                app.perform(Act::ProfileDocument, None);
                assert!(app.profile.document);
                assert!(super::escape(&mut app) && !app.profile.document, "esc closes the document");
                assert!(super::escape(&mut app) && app.profile.pane == Pane::Sections, "then the list");
                assert!(!super::escape(&mut app), "then it's the app's");
            }
            1 => {
                app.profile.select_section(artists);
                app.profile.pane = Pane::Detail;
                let n = app.profile.detail_len();
                let start = tc.draw(gs::integers::<usize>().max_value(n - 1));
                app.profile.rows.select(Some(start));
                let by = tc.draw(gs::sampled_from(&[-10isize, -1, 1, 10, 10_000][..]));
                super::move_by(&mut app, by);
                assert_eq!(app.profile.rows.selected(), Some((start as isize + by).clamp(0, n as isize - 1) as usize));
            }
            2 => {
                screen(&mut app, 140, 45);
                let r = app.profile.sections_area;
                let i = tc.draw(gs::integers::<usize>().max_value(SECTIONS.len().min(r.height as usize - 2) - 1));
                mouse(&mut app, LEFT, (r.x + 2, r.y + 1 + i as u16));
                assert_eq!(
                    (app.profile.pane, app.profile.section()),
                    (Pane::Sections, SECTIONS[i + app.profile.sections.offset()])
                );
                app.profile.select_section(artists);
                screen(&mut app, 140, 45);
                let d = app.profile.detail_area;
                let k = tc.draw(gs::integers::<usize>().max_value(2));
                mouse(&mut app, LEFT, (d.x + 4, d.y + 6 + k as u16));
                assert_eq!(
                    (app.profile.pane, app.profile.rows.selected()),
                    (Pane::Detail, Some(k + app.profile.rows.offset()))
                );
            }
            3 => {
                let none = tc.draw(gs::booleans());
                if none {
                    app.profile.view = None;
                }
                app.perform(Act::ProfileWrite, None);
                if none {
                    assert_eq!(last_message(&app), "nothing to export yet");
                } else {
                    assert_eq!(
                        last_message(&app),
                        format!("wrote {}", app.store_root.join("profile-document.json").display())
                    );
                }
            }
            4 => {
                // What an older build sends back is dropped.
                let (generation, view) = (app.profile.generation, app.profile.view.take());
                app.profile.loading = true;
                app.profile.loaded(generation + 3, Err("stale".into()));
                assert!(app.profile.loading && app.profile.view.is_none());
                app.profile.loaded(generation, Err("it fell over".into()));
                assert!(!app.profile.loading && matches!(app.profile.view, Some(Err(_))));
                assert!(screen(&mut app, 120, 30).contains("it fell over"));
                app.profile.view = view;
            }
            5 => {
                // The store's folder a file: a choice can't be saved, nor the document written.
                let store = std::mem::replace(&mut app.store_root, dir.path().join("a file"));
                std::fs::write(&app.store_root, "not a folder").unwrap();
                app.profile.select_section(SECTIONS.iter().position(|s| *s == Section::Genres).unwrap());
                app.perform(Act::ProfileInclude, None);
                assert!(app.toast.as_ref().is_some_and(|t| t.1 == Level::Error), "{:?}", app.toast);
                app.perform(Act::ProfileWrite, None);
                assert!(app.toast.as_ref().is_some_and(|t| t.1 == Level::Error), "{:?}", app.toast);
                std::fs::remove_file(&app.store_root).unwrap();
                app.store_root = store;
            }
            _ => {
                let rows = screen(&mut app, tc.draw(gs::integers::<u16>().min_value(1).max_value(200)), 40);
                assert_eq!(rows.lines().count(), 40);
            }
        }
    }
}

/// With no key for the view's actions, the include line (and signed out, the account bar) names
/// them as `:` finds them, and `e` isn't drawn.
#[test]
fn with_no_actions_key_the_view_names_them_for_the_palette() {
    use super::super::keymap::ids;
    use locallibrary::keybinds::Keybinds;
    let store = tempfile::tempdir().unwrap();
    let mut app = App::blank(store.path().to_path_buf(), None);
    let mut binds = Keybinds::default();
    binds.keys.insert(ids::id(Act::PaneActions).unwrap().into_owned(), Vec::new());
    app.set_keybinds(binds);
    app.profile = fixture_tab();
    let screen = render(&mut app.profile, 160, 48);
    assert!(screen.contains("leave out: in document in :") && !screen.contains("leave out in e"), "{screen}");
    #[cfg(feature = "account")]
    assert!(screen.contains("  sign in with your Atmosphere account (") && !screen.contains("e sign in"), "{screen}");
    #[cfg(feature = "account")]
    assert!(screen.contains("alpha) in :"), "{screen}");
}
