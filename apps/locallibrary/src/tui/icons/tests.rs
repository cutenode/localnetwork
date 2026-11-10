use std::path::Path;

use hegel::TestCase;
use hegel::generators as gs;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::Rect;
use rustc_hash::FxHashMap;
use unicode_width::UnicodeWidthStr;

use super::super::app::testing::{blank, cue, grid, key, playing, wave};
use super::super::app::{App, View};
use super::super::screens::{Fixture, fixture};
use super::super::ui;
use super::*;

fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let vars: FxHashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    move |k| vars.get(k).cloned()
}

#[test]
fn each_terminal_gets_what_it_can_draw() {
    let set = |vars: &[(&str, &str)]| detect(env(vars));
    assert_eq!(set(&[("TERM", "xterm-256color"), ("LANG", "en_US.UTF-8")]), IconSet::Unicode);
    assert_eq!(set(&[]), IconSet::Unicode, "nothing to go on: Windows");
    assert_eq!(set(&[("TERM", "linux"), ("LANG", "en_US.UTF-8")]), IconSet::Ascii);
    assert_eq!(set(&[("TERM", "vt100"), ("LANG", "en_US.UTF-8")]), IconSet::Ascii);
    assert_eq!(set(&[("TERM", "xterm"), ("LANG", "C")]), IconSet::Ascii);
    assert_eq!(set(&[("TERM", "xterm"), ("LANG", "en_GB.ISO-8859-1")]), IconSet::Ascii);
    assert_eq!(set(&[("TERM", "xterm"), ("LANG", "C"), ("LC_ALL", "en_GB.utf8")]), IconSet::Unicode);
    assert_eq!(set(&[("TERM", "xterm"), ("LANG", "en_GB.UTF-8"), ("LC_CTYPE", "C")]), IconSet::Ascii);
    assert_eq!(set(&[("TERM", "xterm"), ("LC_ALL", ""), ("LANG", "C.UTF-8")]), IconSet::Unicode);
    assert_eq!(set(&[("TERM", "xterm-ghostty"), ("TERM_PROGRAM", "ghostty")]), IconSet::Nerd);
    assert_eq!(set(&[("TERM", "xterm-256color"), ("TERM_PROGRAM", "WezTerm")]), IconSet::Nerd);
    assert_eq!(set(&[("TERM", "xterm-kitty")]), IconSet::Nerd);
    assert_eq!(set(&[("TERM", "xterm"), ("KITTY_WINDOW_ID", "1")]), IconSet::Nerd);
    assert_eq!(set(&[("TERM", "xterm-256color"), ("TERM_PROGRAM", "iTerm.app")]), IconSet::Unicode);
}

/// The table's columns hold what they say: ASCII that's only ASCII, and the same width across, so a
/// set never moves anything on the screen.
#[test]
fn the_tables_columns_line_up() {
    for (i, icon) in ALL.iter().enumerate() {
        let [nerd, unicode, ascii] = icon.alone;
        assert!(ascii.is_ascii(), "{i}: {ascii}");
        assert_ne!(ascii_of(unicode.chars().next().unwrap()), '?', "{i}: what a widget or a key's text draws");
        assert_eq!((nerd.width(), ascii.width()), (unicode.width(), unicode.width()), "{i}: {unicode}");
        for set in IconSet::ALL {
            let at = set as usize;
            assert_eq!(icon.spaced[at], format!("{} ", icon.alone[at]));
            assert_eq!(icon.padded[at], format!(" {} ", icon.alone[at]));
        }
    }
}

#[hegel::test(test_cases = 50)]
fn prop_a_set_names_itself_and_picks_its_column(tc: TestCase) {
    let set = tc.draw(gs::sampled_from(&IconSet::ALL[..]));
    assert_eq!(IconSet::parse(set.name()), Some(set));
    assert_eq!(IconSet::parse(&format!(" {} ", set.name().to_uppercase())), Some(set));
    assert_eq!(IconSet::ALL[set as usize], set);
    let other = tc.draw(gs::text().max_size(6));
    assert_eq!(
        IconSet::parse(&other).is_some(),
        ["nerd", "unicode", "ascii"].contains(&other.trim().to_ascii_lowercase().as_str())
    );
    set_test_set(Some(set));
    let icon = tc.draw(gs::sampled_from(ALL.iter().collect::<Vec<_>>()));
    let [nerd, unicode, ascii] = icon.alone;
    let expected = match set {
        IconSet::Nerd => nerd,
        IconSet::Unicode => unicode,
        IconSet::Ascii => ascii,
    };
    assert_eq!(icon.get(), expected);
    assert_eq!(spinner().len(), if set == IconSet::Ascii { 4 } else { 10 });
    assert_eq!(super::ascii(), set == IconSet::Ascii);
    set_test_set(None);
}

/// A locale that doesn't say UTF-8 and the consoles are ASCII whatever else is set; and a locale
/// saying UTF-8 elsewhere never gets ASCII.
#[hegel::test(test_cases = 200)]
fn prop_the_terminal_and_locale_decide_a_set(tc: TestCase) {
    let term = tc.draw(gs::sampled_from(&["", "xterm", "linux", "vt220", "xterm-kitty"][..]));
    let locale = tc.draw(gs::sampled_from(&["", "C", "en_US.UTF-8", "de_DE.utf8", "POSIX"][..]));
    let which = tc.draw(gs::sampled_from(&["LC_ALL", "LC_CTYPE", "LANG"][..]));
    let program = tc.draw(gs::sampled_from(&["", "ghostty", "Apple_Terminal"][..]));
    let vars: Vec<(&str, &str)> = [("TERM", term), (which, locale), ("TERM_PROGRAM", program)]
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .collect();
    let ascii = matches!(locale, "C" | "POSIX") || term == "linux" || term == "vt220";
    let expected = match (ascii, term, program) {
        (true, ..) => IconSet::Ascii,
        (_, "xterm-kitty", _) | (_, _, "ghostty") => IconSet::Nerd,
        _ => IconSet::Unicode,
    };
    assert_eq!(detect(env(&vars)), expected, "{vars:?}");
}

/// Any text folds to ASCII, a cell at a time: nothing moves, and what has a near match isn't `?`.
#[hegel::test(test_cases = 200)]
fn prop_a_frame_folds_to_ascii_cell_for_cell(tc: TestCase) {
    let line = tc.draw(gs::text().max_size(24));
    let mut buf = Buffer::empty(Rect::new(0, 0, 30, 1));
    buf.set_stringn(0, 0, &line, 30, ratatui::style::Style::new());
    let before: Vec<bool> = buf.content.iter().map(|c| c.symbol().is_ascii()).collect();
    let unchanged: Vec<String> = buf.content.iter().map(|c| c.symbol().to_string()).collect();
    fold(&mut buf);
    for (i, cell) in buf.content.iter().enumerate() {
        assert!(cell.symbol().is_ascii(), "{:?} in {line:?}", cell.symbol());
        assert_eq!(cell.symbol().len(), 1, "one character");
        if before[i] {
            assert_eq!(cell.symbol(), unchanged[i], "{line:?}");
        }
    }
}

/// Everything the player and its widgets draw has a near match: a `?` would mean a glyph nobody
/// mapped.
#[hegel::test(test_cases = 100)]
fn prop_what_the_player_draws_folds_to_a_near_match(tc: TestCase) {
    const DRAWN: &str =
        "─━–−·•▔│┃╷╻╵╹‖▏▍┌┐└┘├┤┬┴┼╭╮╯╰┏┓┗┛▁▂▃▄▅▆▇█▉▊▋▌▐▮▀░▒▓⠋⠙\u{2800}●◆★⟳♫○□■✓✖✕✗⚠⊘↻◉⑂…≈↔⇄←‹→›▶▸↑▲↓▼▾“”‘’×±½";
    let line = tc.draw(gs::text().alphabet(DRAWN).min_size(1).max_size(24));
    let mut buf = Buffer::empty(Rect::new(0, 0, 30, 1));
    buf.set_stringn(0, 0, &line, 30, ratatui::style::Style::new());
    fold(&mut buf);
    let folded: String = buf.content.iter().map(|c| c.symbol()).collect();
    assert!(folded.is_ascii() && !folded.contains('?'), "{line:?} as {folded:?}");
}

/// With ASCII, nothing on a screen is anything else, in any view, however it was chosen.
#[hegel::test(test_cases = 10)]
fn prop_any_screen_in_ascii_is_ascii(tc: TestCase) {
    let view = tc.draw(gs::sampled_from(&View::ALL[..]));
    let (mut app, _dir) = blank(view);
    set_test_set(Some(IconSet::Ascii));
    all_ascii(&mut app, view.title());
    set_test_set(None);
}

#[test]
fn lines_and_blocks_stay_lines_and_blocks() {
    let folded = |s: &str| s.chars().map(ascii_of).collect::<String>();
    assert_eq!(folded("╭──╮│╰──╯"), "+--+|+--+");
    assert_eq!(folded("▁▂▃▄▅▆▇█"), "___===##");
    assert_eq!(folded("↑↓←→‹›"), "^v<>{}".replace('{', "<").replace('}', ">"));
    assert_eq!(folded("…·–×“”‘’½±"), "~--x\"\"''/+");
    assert_eq!(folded("⠋\u{2800}"), "* ");
    assert_eq!(folded("日本"), "??");
}

fn all_ascii(app: &mut App, what: &str) {
    for (w, h) in [(80, 24), (160, 45)] {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| ui::draw(f, app)).unwrap();
        let screen: String = terminal.backend().buffer().content.iter().map(|c| c.symbol()).collect();
        let left: String = screen.chars().filter(|c| !c.is_ascii()).collect();
        assert!(left.is_empty(), "{what} at {w}x{h} has {left:?} left:\n{screen}");
    }
}

/// With ASCII there's nothing else on any screen, the waveform's blocks and the frames included.
#[test]
fn ascii_leaves_no_other_glyph_on_any_screen() {
    set_test_set(Some(IconSet::Ascii));
    let Fixture { _dir, mut app, .. } = fixture();
    for view in View::ALL {
        app.view = view;
        all_ascii(&mut app, view.title());
    }
    app.view = View::Library;
    key(&mut app, KeyCode::Char('?'));
    all_ascii(&mut app, "the help");
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char(':'));
    all_ascii(&mut app, "the palette");

    let analysis = locallibrary::anlz::TrackAnalysis {
        beats: grid(0, 128.0, 400),
        cues: vec![cue(Some(0), 4_000, None), cue(None, 60_000, None)],
        preview: Some(wave(400)),
        detail: Some(wave(4_000)),
        ..Default::default()
    };
    let (mut app, _dir) = playing(analysis);
    all_ascii(&mut app, "playing");
    app.deck_layout = true;
    all_ascii(&mut app, "the deck");
    set_test_set(None);
}

/// Every symbol of the table comes from the table: a literal one in the player would stay as it
/// is whatever the set, on the screens the test above doesn't reach.
#[test]
fn no_icon_is_left_outside_the_table() {
    fn sources(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                sources(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui"), &mut files);
    let test_file = |p: &Path| {
        let name = p.file_name().unwrap().to_str().unwrap();
        let in_tests = p.components().any(|c| c.as_os_str() == "tests");
        in_tests || ["tests.rs", "props.rs", "testing.rs", "screens.rs", "icons.rs"].contains(&name)
    };
    // Text that shows a key's own symbol, not a symbol drawn for something.
    let keymap = |p: &Path| p.ends_with("keymap.rs");
    let glyphs: Vec<&str> = ALL.iter().map(|i| i.alone[IconSet::Unicode as usize]).collect();
    for file in files.iter().filter(|f| !test_file(f) && !keymap(f)) {
        let source = std::fs::read_to_string(file).unwrap();
        for line in source.lines().filter(|l| !l.trim_start().starts_with("//")) {
            let found = glyphs.iter().find(|g| line.contains(**g));
            assert!(found.is_none(), "{found:?} in {}: {line}", file.display());
        }
    }
}
