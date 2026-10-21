use hegel::TestCase;
use hegel::generators as gs;
use ratatui::crossterm::event::KeyCode;
use serde_json::json;

use super::super::super::app::testing::{
    app_on, demo_drive, demo_drive_in, echo_plugin, key, mounted, plugins_once, wait,
};
use super::super::super::app::{Focus, Msg};
use super::super::super::ui::screen;
use super::*;

/// The echo plugin's manifest, with a panel (`said`) answering as its `panel` setting says (the
/// test plugin's `render_panel`), for the drives in `dir`.
fn said_plugin(dir: &Path) {
    echo_plugin(dir);
    mounted(dir);
    let file = dir.join("echo.toml");
    let more = "\n[[contributes.panels]]\nname = \"said\"\ntitle = \"As it says\"\n\n[[contributes.settings]]\nkey = \"panel\"\ntitle = \"What it shows\"\ntype = \"string\"\n";
    let manifest = std::fs::read_to_string(&file).unwrap() + more;
    std::fs::write(&file, manifest).unwrap();
}

/// The player on the drive at `root`, the tracks focused, the details open.
fn showing(root: &Path) -> App {
    let mut app = app_on(root);
    (app.focus, app.show_details) = (Focus::Tracks, true);
    app
}

/// The `said` panel shows `answer` (its setting), in the store of `app`.
fn saying(app: &App, answer: serde_json::Value) {
    let store = Store::open_or_init(&app.store_root).unwrap();
    let plugins = crate::plugins::host().unwrap();
    plugins.set_setting("echo", &store, "panel", Some(answer.to_string().into())).unwrap();
}

/// Asks for the selected track's panels as though the selection had settled on it.
fn settled(app: &mut App) {
    app.want_panels();
    if let Some((g, i, _)) = app.panels.seen {
        app.panels.seen = Some((g, i, Instant::now() - SETTLE));
    }
    app.want_panels();
}

fn answered(app: &App) -> bool {
    app.panels.shown.iter().all(|s| !matches!(s.state, State::Loading))
}

/// A track's panels are asked for once the selection settles, and drawn as they answer, at either
/// size: coming first, then what each shows. Moving on, the next track's are coming, never the
/// last one's, in the room those took.
#[test]
fn a_tracks_panels_come_once_the_selection_settles() {
    let (dir, root) = demo_drive();
    said_plugin(dir.path());
    let mut app = showing(&root);
    let tracks = app.loaded.as_ref().unwrap().library.tracks.clone();
    app.want_panels();
    assert_eq!(app.panels.track, None, "not before it settles");
    assert!(app.panels.waiting(app.selected_key()) && app.animating(), "drawn often until it does");
    let s = screen(&mut app, 160, 45);
    assert!(s.contains("About the track") && s.contains("loading…"), "{s}");
    settled(&mut app);
    wait(&mut app, answered);
    let first = app.selected_track().unwrap();
    // Too short for a panel's rows: none drawn.
    assert!(!screen(&mut app, 80, 24).contains("About the track"));
    for (w, h) in [(80, 45), (160, 45)] {
        let s = screen(&mut app, w, h);
        assert!(
            s.contains("About the track") && s.contains(&format!("title  {}", tracks[first].title)),
            "{w}x{h}\n{s}"
        );
        assert!(s.contains("As it says") && s.contains("nothing to show"), "{w}x{h}\n{s}");
    }
    let rows: Vec<u16> = app.panels.shown.iter().map(|s| s.rows).collect();
    key(&mut app, KeyCode::Down);
    let next = app.selected_track().unwrap();
    assert_ne!(next, first);
    let s = screen(&mut app, 160, 45);
    assert!(!s.contains(&format!("title  {}", tracks[first].title)) && s.contains("loading…"), "{s}");
    settled(&mut app);
    assert_eq!(app.panels.shown.iter().map(|s| s.rows).collect::<Vec<_>>(), rows, "in the room they took");
    wait(&mut app, answered);
    let s = screen(&mut app, 160, 45);
    assert!(s.contains(&format!("title  {}", tracks[next].title)), "{s}");
    assert!(!app.panels.waiting(app.selected_key()), "done");
}

/// A panel that fails says so, the messages why; one on a folder opened by path can't be asked.
#[test]
fn a_panel_that_cant_show_says_why() {
    let (dir, root) = demo_drive();
    said_plugin(dir.path());
    let mut app = showing(&root);
    saying(&app, json!({ "fail": "invalid" }));
    settled(&mut app);
    wait(&mut app, answered);
    let s = screen(&mut app, 160, 45);
    assert!(s.contains("couldn't show it (! says why)"), "{s}");
    assert!(app.log.iter().any(|(m, ..)| m.starts_with("As it says panel: ")));
    let mut app = showing(&root);
    app.extra_roots.push(app.loaded.as_ref().unwrap().root().to_path_buf());
    settled(&mut app);
    assert!(matches!(app.panels.shown[0].state, State::Unavailable(_)));
    assert!(screen(&mut app, 160, 45).contains("only on a plugged-in drive"));
}

/// A panel's picture is drawn in colour (without colours, what it shows is said), and its chart
/// in braille.
#[test]
fn a_panels_picture_and_chart_are_drawn() {
    let (dir, root) = demo_drive();
    said_plugin(dir.path());
    let mut app = showing(&root);
    let mut png = Vec::new();
    image::RgbImage::from_pixel(32, 32, image::Rgb([200, 40, 90]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let image = json!({ "kind": "image", "blob": "a.png", "alt": "the cover" });
    saying(&app, json!({ "blobs": { "a.png": png }, "view": image }));
    settled(&mut app);
    wait(&mut app, answered);
    let State::Ready(ready) = &app.panels.shown[1].state else { panic!("its picture") };
    assert!(ready.art.is_some());
    assert!(screen(&mut app, 160, 45).contains('▀'));
    theme::set_test_mode(Some(theme::Mode::Plain));
    assert!(screen(&mut app, 160, 45).contains("[image: the cover]"));
    theme::set_test_mode(None);

    let mut app = showing(&root);
    let points = json!([[0, 1], [1, 3], [2, 2]]);
    let chart = json!({ "kind": "chart", "style": "line", "series": [{ "name": "energy", "points": points }] });
    saying(&app, json!({ "view": chart }));
    settled(&mut app);
    wait(&mut app, answered);
    let s = screen(&mut app, 160, 45);
    assert!(s.chars().any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)), "{s}");
}

/// Whatever order the selection moves in and panels answer in (an answer for a track since left,
/// one for a track whose panels aren't asked for yet, one failing), what's drawn under the
/// selected track is its own panels' answers, or that they're coming: never another track's.
#[hegel::test(test_cases = 30, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_track_shows_only_its_own_panels(tc: TestCase) {
    let dir = plugins_once();
    let root = demo_drive_in(dir, &format!("D{}", tc.draw(gs::integers::<u32>())));
    let mut app = showing(&root);
    let generation = app.loaded.as_ref().unwrap().generation;
    let n = app.visible.len();
    let said = |k: usize| format!("answer for {k}");
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(12)) {
        match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => app.track_state.select(Some(tc.draw(gs::integers::<usize>().max_value(n - 1)))),
            1 => settled(&mut app),
            _ => {
                let track = app.visible[tc.draw(gs::integers::<usize>().max_value(n - 1))];
                let at = tc.draw(gs::integers::<usize>().max_value(1));
                let answer = match tc.draw(gs::booleans()) {
                    true => Ok(Box::new(Ready::of(&View::Text { title: None, text: said(track) }, &|_| None))),
                    false => Err("it failed".into()),
                };
                app.handle_msg(Msg::Panel((generation, track), at, answer));
            }
        }
        let s = screen(&mut app, 160, 45);
        let selected = app.selected_track().unwrap();
        for k in (0..app.loaded.as_ref().unwrap().library.tracks.len()).filter(|k| *k != selected) {
            assert!(!s.contains(&said(k)), "track {k}'s panel under track {selected}'s details:\n{s}");
        }
        if !app.panels.of_track((generation, selected)) {
            assert!(!s.contains("couldn't show it"), "another track's failure:\n{s}");
        }
    }
}
