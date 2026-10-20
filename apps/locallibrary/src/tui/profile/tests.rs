use ratatui::crossterm::event::KeyCode;

use super::super::app::View;
use super::super::app::testing::*;
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
        for i in 0..Section::ALL.len() {
            t.select_section(i);
            let screen = render(&mut t, w, h);
            if w >= 80 {
                assert!(screen.contains(Section::ALL[i].title()), "{}:\n{screen}", Section::ALL[i].title());
            }
        }
        t.document = true;
        let screen = render(&mut t, w, h);
        if w >= 80 {
            assert!(screen.contains("\"version\": 1"), "{screen}");
        }
        t.document = false;
    }
    t.select_section(Section::ALL.iter().position(|s| *s == Section::Tempo).unwrap());
    let screen = render(&mut t, 120, 40);
    assert!(screen.contains("124–128 BPM"), "{screen}");
}

/// Without declared sets, the view counts sessions and calls them that.
#[test]
fn sessions_are_called_sessions() {
    let mut t = fixture_tab_with(&[&[1, 2, 3, 4, 5, 6], &[9, 10, 1, 2, 12, 7]]);
    t.select_section(Section::ALL.iter().position(|s| *s == Section::Sets).unwrap());
    let screen = render(&mut t, 160, 30);
    assert!(screen.contains("no sets declared: 12 plays in 2 sessions"), "{screen}");
    assert!(
        screen.contains("Session shape") && screen.contains("2 sessions · 6 tracks in a typical session"),
        "{screen}"
    );
    assert!(screen.contains("First seen") && screen.contains("HISTORY 001"), "{screen}");
    assert!(screen.contains("only on this computer") && !screen.contains("Set shape"), "{screen}");
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
    assert_eq!(app.profile.section(), Section::Sets);
    assert!(published(&app).contains(&Section::Sets));
    press(&mut app, "i");
    assert!(!published(&app).contains(&Section::Sets));
    let saved = locallibrary::profile::ProfilePrefs::load(&Store::open(store.path()).unwrap()).unwrap();
    assert!(!saved.published().contains(&Section::Sets), "choices are saved as they're made");

    // Into the artist list, and hide the top artist.
    let artists = Section::ALL.iter().position(|s| *s == Section::Artists).unwrap();
    app.profile.select_section(artists);
    key(&mut app, KeyCode::Tab);
    assert_eq!(app.profile.pane, Pane::Detail);
    press(&mut app, "h");
    let Some(Ok(v)) = &app.profile.view else { panic!() };
    assert_eq!(v.prefs.hidden_artists, ["[a]pendics.shuffle"]);
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.profile.pane, Pane::Sections);

    press(&mut app, "#");
    let Some(Ok(v)) = &app.profile.view else { panic!() };
    assert_eq!(v.document.precision, Precision::Exact);
    press(&mut app, "d");
    assert!(!app.profile.document, "d picks the output, as everywhere");
    app.deck.outputs = None;
    press(&mut app, "D");
    assert!(app.profile.document);
    key(&mut app, KeyCode::Esc);
    assert!(!app.profile.document);

    // Keys the view doesn't use still work.
    press(&mut app, "1");
    assert_eq!(app.view, View::Library);
    press(&mut app, "4");
    assert_eq!(app.view, View::Profile);
    press(&mut app, "q");
    assert!(app.quit);
}

/// Signed out, the account bar names the key that signs in: `L` (`l` loops the track).
#[cfg(feature = "account")]
#[test]
fn the_account_bar_says_which_key_signs_in() {
    let line: String = Account::default().line(200).spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(line.contains("  L sign in with your Atmosphere account"), "{line}");
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
fn signing_in_takes_a_handle_and_signing_out_two_presses() {
    use super::super::app::View;

    let store = tempfile::tempdir().unwrap();
    let mut app = App::blank(store.path().to_path_buf(), None);
    app.profile = fixture_tab();
    app.view = View::Profile;
    assert!(render(&mut app.profile, 120, 30).contains("Not signed in"));

    press(&mut app, "L");
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
    assert!(screen.contains("sign in again by"), "{screen}");
    press(&mut app, "L");
    assert!(app.toast.as_ref().is_some_and(|t| t.0.contains("signed in as @dj.example.com: press L again")));
    press(&mut app, "L");
    assert!(app.profile.account.busy(), "the second press signs out");

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
    let lines = columns(&[Some(0.0), Some(1.0), None], 0.0, 2, 2, |_| theme::CYAN);
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
    use locallibrary::profile::{
        ArcSection, DecadeCount, EnergySection, MixingSection, PrepSection, SetInfo, SignatureTransition, StyleSection,
        StyleValue, Tag,
    };
    let mut t = fixture_tab_with(&[&[1, 2, 3, 4, 5, 6], &[9, 10, 1, 2, 12, 7]]);
    let Some(Ok(v)) = &mut t.view else { panic!("no profile") };
    let r = &mut v.report;
    r.basis = Basis::Plays;
    r.summary.tags = vec![Tag { id: "harmonic", label: "Mixes in key".into(), why: "77% of 30 transitions".into() }];
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
    r.arc = ArcSection {
        sets: 2,
        opening_bpm: Some(122.0),
        closing_bpm: Some(127.5),
        builds: 0.6,
        steady: 0.3,
        winds_down: 0.1,
        curve: vec![Some(-2.0), Some(-1.0), None, Some(1.5)],
    };
    r.mixing = MixingSection {
        transitions: 40,
        keyed: 30,
        same_key: 10,
        adjacent: 8,
        relative: 3,
        energy_boost: 2,
        clash: 7,
        harmonic_share: Some(0.77),
        timed: 30,
        median_step_pct: Some(1.2),
        within_3pct: Some(0.8),
        within_6pct: Some(0.95),
        half_double: 1,
        genred: 20,
        genre_change_share: Some(0.35),
    };
    r.energy = Some(EnergySection { tracks: 12, high: 0.5, mid: 0.3, low: 0.2, curve: vec![Some(0.2), Some(1.0)] });
    r.era.tracks = 10;
    (r.era.median_year, r.era.oldest, r.era.newest, r.era.recent_share) =
        (Some(2019), Some(1998), Some(2024), Some(0.3));
    r.era.decades = vec![DecadeCount { decade: 1990, count: 2 }, DecadeCount { decade: 2010, count: 8 }];
    r.signature.transitions =
        vec![SignatureTransition { from: "Ferrule - Night Bus".into(), to: "Ferrule - Halogen".into(), sets: 3 }];
    r.prep = Some(PrepSection {
        tracks: 12,
        hot_cue_share: 0.75,
        hot_cues_per_track: Some(3.5),
        memory_cue_share: 0.5,
        loop_share: 0.1,
    });
    (r.collection.played_share, r.collection.duplicate_files) = (Some(0.4), 3);
    r.style = Some(StyleSection {
        source: "MYSETTING.DAT on DNA".into(),
        values: vec![StyleValue {
            key: "quantize".into(),
            label: "Quantize".into(),
            value: "on".into(),
            value_label: "On".into(),
        }],
    });
    v.prefs.headline = Some("Warm-up specialist".into());
    let played: Vec<String> = r.signature.played.iter().take(2).map(|p| p.name.clone()).collect();
    (v.prefs.pinned, v.prefs.hidden) = (vec![played[0].clone()], vec![played[1].clone()]);
    t
}

/// The screen with section `s` selected.
fn showing(t: &mut ProfileTab, s: Section) -> String {
    t.select_section(Section::ALL.iter().position(|x| *x == s).unwrap());
    render(t, 140, 40)
}

#[test]
fn every_section_says_what_its_numbers_show() {
    let mut t = full_tab();
    for (s, says) in [
        (
            Section::Summary,
            &["Warm-up specialist  (your headline)", "Your headline: Warm-up specialist", "77% of 30 transitions"][..],
        ),
        (
            Section::Sets,
            &[
                "1 only on a drive (pull archives them) · 2 shorter sessions left out",
                "on a drive",
                "122 → 128",
                "01234567",
            ],
        ),
        (Section::Arc, &["122.0 → 127.5 BPM", "averaged over 2 sets", "builds", "through a set: -2.0 to +1.5"]),
        (
            Section::Mixing,
            &[
                "77% of 30 transitions between keyed tracks are harmonic",
                "clash 7",
                "median 1.2% · 80% within 3% · 95% within 6% · 1 at half or double time",
                "35% of transitions change genre",
            ],
        ),
        (Section::Energy, &["mood, 12 tracks", "high", "50%", "Mood through a set"]),
        (Section::Era, &["median 2019", "(1998–2024) · 30% released the year played", "1990s", "2010s"]),
        (Section::Signature, &["Moves played again and again", " 3× Ferrule - Night Bus → Ferrule - Halogen"]),
        (Section::Prep, &["Cues across 12 tracks", "75% · 3.5 each", "memory cues"]),
        (Section::Collection, &["Played in a set", "40%", "3 more files are copies of these"]),
        (Section::Style, &["Quantize", "On", "From MYSETTING.DAT on DNA"]),
    ] {
        let screen = showing(&mut t, s);
        for said in says {
            assert!(screen.contains(said), "{s:?}: {said}\n{screen}");
        }
    }
    // The pinned track is starred, the hidden one struck out.
    let Some(Ok(v)) = &t.view else { panic!("no profile") };
    let (pinned, hidden) = (v.prefs.pinned[0].clone(), v.prefs.hidden[0].clone());
    let screen = showing(&mut t, Section::Signature);
    let row = |name: &str| screen.lines().find(|l| l.contains(name)).unwrap_or_else(|| panic!("{name}\n{screen}"));
    assert!(row(&pinned).contains('★') && row(&hidden).contains('⊘'), "{screen}");
}

/// A section without the data it needs says what it needs.
#[test]
fn empty_sections_say_what_they_need() {
    let mut t = fixture_tab();
    let Some(Ok(v)) = &mut t.view else { panic!("no profile") };
    let r = &mut v.report;
    (r.tempo.core, r.keys.tracks, r.era.tracks, r.energy, r.prep, r.style) = (None, 0, 0, None, None, None);
    r.genres.top.clear();
    for (s, says) in [
        (Section::Tempo, "No BPMs: tracks need rekordbox's analysis."),
        (Section::Arc, "Needs sets of 6 or more tracks with BPMs."),
        (Section::Mixing, "Needs sets."),
        (Section::Keys, "No keys: tracks need rekordbox's analysis."),
        (Section::Energy, "No phrase analysis read"),
        (Section::Genres, "None tagged."),
        (Section::Era, "No release years tagged."),
        (Section::Prep, "No cues read"),
        (Section::Style, "No My Settings on these drives"),
        (Section::Sets, "No sets declared, and no sessions with enough tracks."),
        (Section::Summary, "No trait stands out yet."),
    ] {
        let screen = showing(&mut t, s);
        assert!(screen.contains(says), "{s:?}: {says}\n{screen}");
    }
}

/// The section list sums up how the sets run.
#[test]
fn the_arc_is_summed_up_in_a_word() {
    let mut t = full_tab();
    let Some(Ok(v)) = &mut t.view else { panic!("no profile") };
    let r = &mut v.report;
    let mut word = |builds, steady, winds_down| {
        (r.arc.builds, r.arc.steady, r.arc.winds_down) = (builds, steady, winds_down);
        stat(r, Section::Arc)
    };
    assert_eq!(word(0.6, 0.2, 0.2), "builds");
    assert_eq!(word(0.2, 0.6, 0.2), "steady");
    assert_eq!(word(0.2, 0.2, 0.6), "winds down");
    assert_eq!(word(0.4, 0.3, 0.3), "varies");
}

/// The app's Profile view showing `t`, its store in a temporary folder.
fn profile_app(t: ProfileTab) -> (App, tempfile::TempDir) {
    let (mut app, dir) = blank(View::Profile);
    app.profile = t;
    (app, dir)
}

/// `m` pins a signature track and `h` hides it, each pressed again undoing it; `H` hides
/// its artist. Outside a list, they say where to go.
#[test]
fn signature_tracks_are_pinned_and_hidden() {
    let (mut app, _store) = profile_app(fixture_tab_with(&[&[1, 2, 3, 4, 5, 6], &[9, 10, 1, 2, 12, 7]]));
    press(&mut app, "m");
    assert_eq!(last_message(&app), "select a track under Signature tracks (tab) to pin it");
    press(&mut app, "h");
    assert!(
        last_message(&app).starts_with("select a track under Signature tracks, or an artist"),
        "{}",
        last_message(&app)
    );

    app.profile.select_section(Section::ALL.iter().position(|s| *s == Section::Signature).unwrap());
    press(&mut app, "\t");
    let Some(Ok(v)) = &app.profile.view else { panic!("no profile") };
    let track = v.report.signature.played[0].clone();
    for (key, said) in [
        ('m', format!("pinned {}", track.name)),
        ('m', format!("unpinned {}", track.name)),
        ('h', format!("{} won't be named", track.name)),
        ('h', format!("{} can be named again", track.name)),
        ('H', format!("{} won't be named", track.artist)),
        ('H', format!("{} can be named again", track.artist)),
    ] {
        press(&mut app, &key.to_string());
        assert_eq!(last_message(&app), said);
    }
    let Some(Ok(v)) = &app.profile.view else { panic!("no profile") };
    assert!(v.prefs.pinned.is_empty() && v.prefs.hidden.is_empty() && v.prefs.hidden_artists.is_empty());

    // Among artists, m pins nothing.
    press(&mut app, "\u{1b}");
    app.profile.select_section(Section::ALL.iter().position(|s| *s == Section::Artists).unwrap());
    press(&mut app, "\t");
    press(&mut app, "m");
    assert_eq!(last_message(&app), "select a track under Signature tracks (tab) to pin it");
}

/// `w` writes the document into the store folder, made there if there's none.
#[test]
fn w_writes_the_document_to_the_store() {
    let (mut app, _dir) = profile_app(fixture_tab());
    press(&mut app, "w");
    let path = app.store_root.join("profile-document.json");
    assert_eq!(last_message(&app), format!("wrote {}", path.display()));
    let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(doc["version"], 1);

    let (tx, _rx) = std::sync::mpsc::channel();
    app.profile = ProfileTab::new(tx);
    press(&mut app, "w");
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
    press(&mut app, "i");
    assert!(app.toast.as_ref().is_some_and(|t| t.1 == Level::Error), "{:?}", app.toast);
    press(&mut app, "w");
    assert!(app.toast.as_ref().is_some_and(|t| t.1 == Level::Error), "{:?}", app.toast);
    let Some(Ok(v)) = &app.profile.view else { panic!("no profile") };
    assert_eq!(v.published, before);
}

/// `a` builds the profile from the open drive alone, in the background, and the header
/// says how far it's got.
#[test]
fn a_builds_the_profile_from_the_open_drive() {
    let dir = tempfile::tempdir().unwrap();
    let pdb = dir.path().join("PIONEER/rekordbox/export.pdb");
    std::fs::create_dir_all(pdb.parent().unwrap()).unwrap();
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/rekordbox-pdb/tests/data/bigger-export.pdb");
    std::fs::copy(fixture, &pdb).unwrap();
    let (tx, _rx) = std::sync::mpsc::channel();
    let (mut app, _store) = profile_app(ProfileTab::new(tx));
    press(&mut app, "a");
    assert_eq!(last_message(&app), "open a drive in the library first");

    let mut app = App::with_library(dir.path(), library());
    app.view = View::Profile;
    press(&mut app, "a");
    let Scope::Drive(_, name) = app.profile.scope.clone() else { panic!("{:?}", app.profile.scope) };
    assert_eq!(last_message(&app), format!("profile from {name} only"));
    assert!(app.profile.loading);
    // The header says so while it builds. Counters of the test's own: the build's thread keeps
    // the ones it was given, and counting there can't overwrite these.
    app.profile.progress = Arc::new([AtomicU64::new(3), AtomicU64::new(12)]);
    let s = screen(&mut app, 140, 30);
    assert!(s.contains("Building profile… 3/12 tracks"), "{s}");
    // A build started before this one is stopped, and what it sends back ignored.
    let old = app.profile.generation;
    press(&mut app, "R");
    app.profile.loaded(old, Err("stale".into()));
    assert!(app.profile.loading && app.profile.view.is_none());

    wait(&mut app, |a| !a.profile.loading);
    assert!(matches!(&app.profile.view, Some(Ok(v)) if v.report.collection.tracks == 12), "{:?}", app.toast);
    let s = screen(&mut app, 140, 30);
    assert!(s.contains(&format!("Profile · {name} only")), "{s}");
}

/// The pointer picks a section, a click picks the row of its list under it, and the wheel
/// moves through either.
#[test]
fn the_mouse_picks_sections_and_rows() {
    let (mut app, _dir) = profile_app(fixture_tab_with(&[&[1, 2, 3, 4, 5, 6], &[9, 10, 1, 2, 12, 7]]));
    let signature = spot_in(&mut app, "Signature", |a| a.profile.sections_area);
    mouse(&mut app, LEFT, signature);
    assert_eq!(app.profile.section(), Section::Signature);

    // The row showing the third track.
    let Some(Ok(v)) = &app.profile.view else { panic!("no profile") };
    let third = v.report.signature.played[2].name.clone();
    let row = spot_in(&mut app, &third, |a| a.profile.detail_area);
    mouse(&mut app, LEFT, row);
    assert_eq!((app.profile.pane, app.profile.rows.selected()), (Pane::Detail, Some(2)));
    mouse(&mut app, MouseEventKind::ScrollDown, row);
    assert_eq!(app.profile.rows.selected(), Some(5));
    mouse(&mut app, MouseEventKind::ScrollUp, signature);
    let i = Section::ALL.iter().position(|s| *s == Section::Signature).unwrap();
    assert_eq!((app.profile.pane, app.profile.section()), (Pane::Sections, Section::ALL[i - 3]));
    let detail = app.profile.detail_area;
    // The document scrolls with the wheel.
    press(&mut app, "D");
    mouse(&mut app, MouseEventKind::ScrollDown, (detail.x + 4, detail.y + 6));
    assert_eq!(app.profile.scroll, 3);
    // Outside the view, the pointer does what it does anywhere.
    mouse(&mut app, MouseEventKind::ScrollDown, (0, 0));
    assert_eq!(app.profile.scroll, 3);
}

/// A full profile (every section filled in, some tracks pinned or hidden) draws at any size,
/// whichever section, pane, row or scroll, and with room, lists every section by its title,
/// the selected one's numbers beside them; the selected row stays among the section's rows.
#[hegel::test(test_cases = 20, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_full_profile_draws_whole_at_any_size(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut t = full_tab();
    let basis = t.report().unwrap().basis;
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(8)) {
        match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 => t.select_section(tc.draw(gs::integers::<usize>().max_value(Section::ALL.len() - 1))),
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
            for s in Section::ALL {
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
    let name = |x: &profile::Share| x.name.clone();
    let tables: Vec<(Section, Vec<String>)> = [
        (Section::Sets, r.sets.list.iter().map(|s| s.name.clone()).collect::<Vec<_>>()),
        (Section::Signature, r.signature.played.iter().map(|p| p.name.clone()).collect()),
        (Section::Genres, r.genres.top.iter().map(name).collect()),
        (Section::Labels, r.labels.top.iter().map(name).collect()),
        (Section::Artists, r.artists.top.iter().map(name).collect()),
    ]
    .into_iter()
    .filter(|(_, names)| !names.is_empty())
    .collect();
    let (section, names) = tables[tc.draw(gs::integers::<usize>().max_value(tables.len() - 1))].clone();
    t.select_section(Section::ALL.iter().position(|s| *s == section).unwrap());
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

/// Each section says what its numbers show, whatever they are: the arc's opening and closing
/// tempo, how it runs (summed up in a word in the list) and its curve; the moods and theirs; the
/// era's decades; the cues; how much of the collection was played and its copies; and, without
/// the data, what it needs; how the sets are mixed: in key, by tempo steps, across genres, each
/// share said as unknown when it is. The header says what the numbers come from and how they're
/// rounded.
#[hegel::test(test_cases = 25, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn every_section_says_what_any_numbers_show(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use locallibrary::profile::{DecadeCount, EnergySection, PrepSection};
    let mut v = full_view();
    let r = &mut v.report;
    r.basis = tc.draw(gs::sampled_from(&[Basis::Plays, Basis::Sessions, Basis::Collection][..]));
    let shares = &[0.0, 0.2, 0.3, 0.5, 0.6, 1.0][..];
    r.arc.sets = tc.draw(gs::integers::<usize>().max_value(3));
    (r.arc.opening_bpm, r.arc.closing_bpm) = (Some(122.0), Some(tc.draw(gs::sampled_from(&[118.25, 127.5][..]))));
    (r.arc.builds, r.arc.steady, r.arc.winds_down) =
        (tc.draw(gs::sampled_from(shares)), tc.draw(gs::sampled_from(shares)), tc.draw(gs::sampled_from(shares)));
    r.arc.curve = tc.draw(gs::vecs(gs::optional(gs::sampled_from(&[-2.0, 0.5, 1.5][..]))).max_size(4));
    r.energy = tc.draw(gs::booleans()).then(|| EnergySection {
        tracks: 12,
        high: tc.draw(gs::sampled_from(shares)),
        mid: 0.3,
        low: 0.2,
        curve: tc.draw(gs::vecs(gs::optional(gs::sampled_from(&[0.0, 0.5, 1.0][..]))).max_size(3)),
    });
    r.era.tracks = tc.draw(gs::sampled_from(&[0usize, 10][..]));
    r.era.decades = (0..tc.draw(gs::integers::<i32>().min_value(0).max_value(3)))
        .map(|i| DecadeCount { decade: 1990 + 10 * i, count: 1 + i as usize })
        .collect();
    r.prep = tc.draw(gs::booleans()).then(|| PrepSection {
        tracks: 12,
        hot_cue_share: tc.draw(gs::sampled_from(shares)),
        hot_cues_per_track: tc.draw(gs::optional(gs::sampled_from(&[1.0, 3.5][..]))),
        memory_cue_share: 0.5,
        loop_share: 0.1,
    });
    r.collection.played_share = tc.draw(gs::optional(gs::sampled_from(shares)));
    r.collection.duplicate_files = tc.draw(gs::integers::<usize>().max_value(3));
    let m = &mut r.mixing;
    m.transitions = tc.draw(gs::sampled_from(&[0usize, 40][..]));
    (m.keyed, m.timed, m.genred, m.half_double) = (
        tc.draw(gs::sampled_from(&[0usize, 30][..])),
        tc.draw(gs::sampled_from(&[0usize, 30][..])),
        tc.draw(gs::sampled_from(&[0usize, 20][..])),
        tc.draw(gs::integers::<usize>().max_value(2)),
    );
    m.harmonic_share = tc.draw(gs::optional(gs::sampled_from(shares)));
    (m.within_3pct, m.within_6pct, m.genre_change_share) = (
        tc.draw(gs::optional(gs::sampled_from(shares))),
        tc.draw(gs::optional(gs::sampled_from(shares))),
        tc.draw(gs::optional(gs::sampled_from(shares))),
    );
    v.prefs.precision = if tc.draw(gs::booleans()) { Precision::Exact } else { Precision::Rounded };
    // However the sets run, the list sums it up in a word.
    let mut arc = v.report.clone();
    for (b, st, w, word) in
        [(0.6, 0.3, 0.1, "builds"), (0.2, 0.6, 0.2, "steady"), (0.2, 0.2, 0.6, "winds down"), (0.4, 0.3, 0.3, "varies")]
    {
        (arc.arc.sets, arc.arc.builds, arc.arc.steady, arc.arc.winds_down) = (2, b, st, w);
        assert_eq!(stat(&arc, Section::Arc), word);
    }
    // And without the tempos, keys or My Settings, those sections say what they need.
    let mut bare = tab_of(ProfileView { report: ProfileReport { style: None, ..v.report.clone() }, ..v.clone() });
    if let Some(Ok(b)) = &mut bare.view {
        (b.report.tempo.core, b.report.keys.tracks) = (None, 0);
    }
    for (section, says) in [
        (Section::Tempo, "No BPMs: tracks need rekordbox's analysis."),
        (Section::Keys, "No keys: tracks need rekordbox's analysis."),
        (Section::Style, "No My Settings on these drives"),
    ] {
        assert!(showing(&mut bare, section).contains(says), "{section:?}");
    }
    let r = v.report.clone();
    let exact = v.prefs.precision == Precision::Exact;
    let mut t = tab_of(v);
    let unit = r.basis.unit();
    let units = |n: usize| plural(n, unit);
    for (i, s) in Section::ALL.iter().enumerate() {
        t.select_section(i);
        let screen = render(&mut t, 160, 50);
        let mut want: Vec<String> = Vec::new();
        match s {
            Section::Summary => {
                want.push(if exact { "exact".into() } else { "rounded".into() });
                want.push(match r.basis {
                    Basis::Plays => format!("{} in {}", plural(r.sets.plays, "play"), plural(r.sets.count, "set")),
                    Basis::Sessions => format!(
                        "no sets declared: {} in {}",
                        plural(r.sets.plays, "play"),
                        plural(r.sets.count, "session")
                    ),
                    Basis::Collection => "no sets: from the collection".into(),
                });
            }
            Section::Arc if r.arc.sets == 0 => want.push(format!("Needs {unit}s of 6 or more tracks with BPMs.")),
            Section::Arc => {
                want.push(format!("122.0 → {:.1} BPM", r.arc.closing_bpm.unwrap()));
                want.push(format!("averaged over {}", units(r.arc.sets)));
                let word = match (r.arc.builds, r.arc.steady, r.arc.winds_down) {
                    (b, ..) if b >= 0.5 => "builds",
                    (_, st, _) if st >= 0.5 => "steady",
                    (_, _, w) if w >= 0.5 => "winds down",
                    _ => "varies",
                };
                assert_eq!(stat(&r, *s), word);
                let curve: Vec<f64> = r.arc.curve.iter().flatten().copied().collect();
                if !curve.is_empty() {
                    let (lo, hi) =
                        curve.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), x| (l.min(*x), h.max(*x)));
                    want.push(format!("through a {unit}: {lo:+.1} to {hi:+.1}"));
                }
            }
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
            Section::Prep => match &r.prep {
                None => want.push("No cues read".into()),
                Some(p) => {
                    want.push("Cues across 12 tracks".into());
                    let each = p.hot_cues_per_track.map(|n| format!(" · {n:.1} each")).unwrap_or_default();
                    want.push(format!("{}{each}", pct(p.hot_cue_share)));
                }
            },
            Section::Mixing if r.mixing.transitions == 0 => want.push(format!("Needs {unit}s.")),
            Section::Mixing => {
                let m = &r.mixing;
                // A share unknown is a dash.
                let share = |x: Option<f64>| x.map_or_else(|| "–".to_string(), pct);
                if m.keyed > 0 {
                    want.push(format!(
                        "{} of {} transitions between keyed tracks are harmonic",
                        share(m.harmonic_share),
                        m.keyed
                    ));
                }
                if m.timed > 0 {
                    let half = if m.half_double > 0 {
                        format!(" · {} at half or double time", m.half_double)
                    } else {
                        String::new()
                    };
                    want.push(format!(
                        "Tempo steps: median 1.2% · {} within 3% · {} within 6%{half}",
                        share(m.within_3pct),
                        share(m.within_6pct)
                    ));
                }
                if m.genred > 0 {
                    want.push(format!("{} of transitions change genre", share(m.genre_change_share)));
                }
            }
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
/// would keep them: m pins and unpins a signature track (not one that's hidden, which says so),
/// h hides one (unpinning it) and names it again, H does
/// that for an artist (a signature track's, or one under Artists), i puts the section in the
/// document or leaves it out, and the numbers go exact or rounded; each says what it did, and
/// keys used where they mean nothing say where to go.
#[hegel::test(test_cases = 30, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_djs_choices_are_kept_as_made(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use std::collections::BTreeSet;
    let (mut app, _dir) = profile_app(tab_of(full_view()));
    let at = |s: Section| Section::ALL.iter().position(|x| *x == s).unwrap();
    let (mut pinned, mut hidden, mut artists) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    let mut published: BTreeSet<Section> = match &app.profile.view {
        Some(Ok(v)) => v.published.iter().copied().collect(),
        _ => unreachable!(),
    };
    let mut exact = false;
    const SECTIONS: [Section; 3] = [Section::Signature, Section::Artists, Section::Tempo];
    const ACTS: [Act; 5] =
        [Act::ProfilePin, Act::ProfileHide, Act::ProfileHideArtist, Act::ProfileInclude, Act::ProfileRounding];
    // Every act on a row of every section first (as drawn, they'd seldom meet), then as drawn.
    for step in 0..15 + tc.draw(gs::integers::<usize>().max_value(10)) {
        let section = if step < 15 { SECTIONS[step / 5] } else { tc.draw(gs::sampled_from(&SECTIONS[..])) };
        app.profile.select_section(at(section));
        let detail = (step < 15 || tc.draw(gs::booleans())) && app.profile.detail_len() > 0;
        app.profile.pane = if detail { Pane::Detail } else { Pane::Sections };
        if detail {
            let n = app.profile.detail_len();
            app.profile.rows.select(Some(tc.draw(gs::integers::<usize>().max_value(n - 1))));
        }
        let Some(Ok(v)) = &app.profile.view else { unreachable!() };
        let row = app.profile.rows.selected().filter(|_| detail);
        let track =
            row.and_then(|i| v.report.signature.played.get(i)).filter(|_| section == Section::Signature).cloned();
        let artist = match section {
            Section::Signature => track.as_ref().map(|t| t.artist.clone()),
            Section::Artists => row.and_then(|i| v.report.artists.top.get(i)).map(|a| a.name.clone()),
            _ => None,
        };
        let act = if step < 15 { ACTS[step % 5] } else { tc.draw(gs::sampled_from(&ACTS[..])) };
        // Once, or (as often, to take it back) twice.
        for _ in 0..1 + usize::from(tc.draw(gs::booleans())) {
            app.perform(act, None);
            let said = last_message(&app).to_string();
            let toggle =
                |set: &mut BTreeSet<String>, x: &str| if set.remove(x) { false } else { set.insert(x.to_string()) };
            match act {
                Act::ProfilePin => match &track {
                    // A hidden track isn't pinned: it's to be named again first.
                    Some(t) if hidden.contains(&t.name) => {
                        assert!(said.ends_with("is hidden; unhide it before pinning it"), "{said}");
                    }
                    Some(t) => {
                        let on = toggle(&mut pinned, &t.name);
                        assert_eq!(
                            said,
                            if on { format!("pinned {}", t.name) } else { format!("unpinned {}", t.name) }
                        );
                    }
                    None => assert_eq!(said, "select a track under Signature tracks (tab) to pin it"),
                },
                Act::ProfileHide | Act::ProfileHideArtist => match (&track, &artist) {
                    (Some(t), _) if act == Act::ProfileHide => {
                        let on = toggle(&mut hidden, &t.name);
                        // Hidden, it's unpinned.
                        if on {
                            pinned.remove(&t.name);
                        }
                        assert_eq!(
                            said,
                            if on {
                                format!("{} won't be named", t.name)
                            } else {
                                format!("{} can be named again", t.name)
                            }
                        );
                    }
                    (_, Some(a)) if act == Act::ProfileHideArtist || track.is_none() => {
                        let on = toggle(&mut artists, &locallibrary::model::normalize(a));
                        assert_eq!(
                            said,
                            if on { format!("{a} won't be named") } else { format!("{a} can be named again") }
                        );
                    }
                    _ => assert!(said.starts_with("select a track under Signature tracks, or an artist"), "{said}"),
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
            assert_eq!(p.pinned.iter().cloned().collect::<BTreeSet<_>>(), pinned);
            assert_eq!(p.hidden.iter().cloned().collect::<BTreeSet<_>>(), hidden);
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
/// (never past its ends); a click picks a section, or a row of its list; scoped to the open drive
/// (only with one open) and back to everything, it's built again; w writes the document (once
/// there's one to write); a build replaced since changes nothing; a choice that can't be saved
/// says why.
#[hegel::test(test_cases = 20, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_profile_view_answers_to_its_keys_and_the_mouse(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (mut app, dir) = profile_app(tab_of(full_view()));
    let signature = Section::ALL.iter().position(|s| *s == Section::Signature).unwrap();
    let mut turns: Vec<u8> = (0..8).collect();
    for k in (1..turns.len()).rev() {
        turns.swap(k, tc.draw(gs::integers::<usize>().max_value(k)));
    }
    for step in 0..tc.draw(gs::integers::<usize>().min_value(8).max_value(16)) {
        app.view = View::Profile;
        if !matches!(app.profile.view, Some(Ok(_))) {
            (app.profile.view, app.profile.loading) = (Some(Ok(full_view())), false);
        }
        match turns[step % turns.len()] {
            0 => {
                app.profile.select_section(signature);
                (app.profile.pane, app.profile.document) = (Pane::Sections, false);
                press(&mut app, "\t");
                assert_eq!(app.profile.pane, Pane::Detail);
                press(&mut app, "D");
                assert!(app.profile.document);
                assert!(super::escape(&mut app) && !app.profile.document, "esc closes the document");
                assert!(super::escape(&mut app) && app.profile.pane == Pane::Sections, "then the list");
                assert!(!super::escape(&mut app), "then it's the app's");
            }
            1 => {
                app.profile.select_section(signature);
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
                let i = tc.draw(gs::integers::<usize>().max_value(Section::ALL.len().min(r.height as usize - 2) - 1));
                mouse(&mut app, LEFT, (r.x + 2, r.y + 1 + i as u16));
                assert_eq!(
                    (app.profile.pane, app.profile.section()),
                    (Pane::Sections, Section::ALL[i + app.profile.sections.offset()])
                );
                app.profile.select_section(signature);
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
                // Scoped to the open drive, if there's one, and back.
                let drive = tc.draw(gs::booleans());
                if drive {
                    let lib = App::with_library(dir.path(), library());
                    app.loaded = lib.loaded;
                } else {
                    app.loaded = None;
                }
                app.profile.scope = Scope::Everything;
                app.perform(Act::ProfileScope, None);
                if !drive {
                    assert_eq!(last_message(&app), "open a drive in the library first");
                    continue;
                }
                assert!(last_message(&app).ends_with(" only") && app.profile.loading);
                let first = app.profile.cancel.clone().unwrap();
                app.perform(Act::ProfileScope, None);
                assert!(first.is_cancelled(), "the build replaced is stopped");
                assert_eq!(last_message(&app), "profile from every drive and the store");
                // (Its builds report to a tab of their own, which nothing reads.)
                app.profile.loading = false;
            }
            4 => {
                let none = tc.draw(gs::booleans());
                if none {
                    app.profile.view = None;
                }
                press(&mut app, "w");
                if none {
                    assert_eq!(last_message(&app), "nothing to export yet");
                } else {
                    assert_eq!(
                        last_message(&app),
                        format!("wrote {}", app.store_root.join("profile-document.json").display())
                    );
                }
            }
            5 => {
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
            6 => {
                // The store's folder a file: a choice can't be saved, nor the document written.
                let store = std::mem::replace(&mut app.store_root, dir.path().join("a file"));
                std::fs::write(&app.store_root, "not a folder").unwrap();
                press(&mut app, "i");
                assert!(app.toast.as_ref().is_some_and(|t| t.1 == Level::Error), "{:?}", app.toast);
                press(&mut app, "w");
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
