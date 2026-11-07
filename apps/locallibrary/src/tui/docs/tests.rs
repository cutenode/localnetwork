//! What `docs/player.md` shows of the player, made from the player: its key tables from `KEYMAP`,
//! and its screenshots (`docs/player/*.svg`) from the screens `screens.rs` checks, each cell a
//! coloured rectangle and monospace text. Each test fails when a file differs from what the code
//! makes. To update them:
//!
//! `LOCALLIBRARY_BLESS=1 cargo nextest run -p locallibrary-cli -E 'test(/tui::docs::tests::/)'`

use std::path::Path;

use chrono::{Local, TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::style::{Color, Modifier};
use uuid::Uuid;

use super::super::app::{App, View};
use super::super::keymap::{HelpSection, help};
use super::super::screens::{Fixture, fixture};
use super::super::{theme, ui};

const DOCS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/docs");
const RERUN: &str = "LOCALLIBRARY_BLESS=1 cargo nextest run -p locallibrary-cli -E 'test(/tui::docs::tests::/)'";
/// Comments Markdown drops: the website shows an HTML comment as text.
const KEYS_START: &str = "[//]: # (The key tables are generated from the keymap: see src/tui/docs/tests.rs.)";
const KEYS_END: &str = "[//]: # (End of the key tables.)";

const VIEWS: [View; 5] = View::ALL;
const SIZES: [(u16, u16); 2] = [(80, 24), (160, 45)];
/// A cell in the screenshots, and where its text sits in it.
const CELL: (usize, usize) = (9, 18);
const BASELINE: usize = 14;
/// What the theme leaves to the terminal, as a dark terminal draws it.
const BACKGROUND: &str = "#101014";
const FOREGROUND: &str = "#e6e6e6";

/// `path` holds `made`, or with `LOCALLIBRARY_BLESS=1` now does.
fn holds(path: &Path, made: &str) {
    if std::env::var_os("LOCALLIBRARY_BLESS").is_some_and(|v| v == "1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        return std::fs::write(path, made).unwrap();
    }
    let on_disk = std::fs::read_to_string(path).unwrap_or_default();
    assert!(on_disk == made, "{} isn't what the player makes: update it with\n  {RERUN}", path.display());
}

fn table((title, rows): &HelpSection) -> String {
    let cell = |s: &str| s.replace('|', "\\|");
    // A row with no keys goes on from the one above it.
    let keys = |k: &str| if k.is_empty() { String::new() } else { format!("`{}`", cell(k)) };
    let rows: String = rows.iter().map(|(k, what)| format!("| {} | {} |\n", keys(k), cell(what))).collect();
    format!("{title}:\n\n| Keys | |\n| --- | --- |\n{rows}\n")
}

fn key_tables() -> String {
    let view_keys =
        VIEWS.map(|v| format!("## Keys in {}\n\n{}", v.title(), help(v).0.iter().map(table).collect::<String>()));
    let everywhere = help(View::Library).1.iter().map(table).collect::<String>();
    format!("{KEYS_START}\n\n{}## Keys everywhere\n\n{everywhere}{KEYS_END}", view_keys.concat())
}

#[test]
fn the_key_tables_are_the_keymap() {
    let path = Path::new(DOCS).join("player.md");
    let doc = std::fs::read_to_string(&path).unwrap();
    let start = doc.find(KEYS_START).expect("the start of the key tables");
    let end = doc.find(KEYS_END).expect("the end of the key tables") + KEYS_END.len();
    holds(&path, &format!("{}{}{}", &doc[..start], key_tables(), &doc[end..]));
}

fn colour(c: Color, unset: &str) -> String {
    match c {
        Color::Reset => unset.to_string(),
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        other => panic!("{other:?} in a truecolor screenshot"),
    }
}

/// A cell's colours, turned round when it's in reverse video.
fn colours(c: &Cell) -> (String, String) {
    let (fg, bg) = (colour(c.fg, FOREGROUND), colour(c.bg, BACKGROUND));
    if c.modifier.contains(Modifier::REVERSED) { (bg, fg) } else { (fg, bg) }
}

fn escaped(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The attributes of a cell's text.
fn text_style(c: &Cell) -> String {
    let (fg, _) = colours(c);
    let on = |m| c.modifier.contains(m);
    let bold = if on(Modifier::BOLD) { " font-weight=\"bold\"" } else { "" };
    let italic = if on(Modifier::ITALIC) { " font-style=\"italic\"" } else { "" };
    let dim = if on(Modifier::DIM) { " opacity=\"0.6\"" } else { "" };
    let line = if on(Modifier::UNDERLINED) { " text-decoration=\"underline\"" } else { "" };
    format!("fill=\"{fg}\"{bold}{italic}{dim}{line}")
}

/// One row: a rectangle for each run of one background, and a text for each run of one style,
/// with every glyph placed on its cell whatever the font's widths.
fn row(buf: &Buffer, y: u16, svg: &mut String) {
    let cells: Vec<&Cell> = (0..buf.area.width).map(|x| &buf[(x, y)]).collect();
    let (w, h) = CELL;
    let top = y as usize * h;
    let mut at = 0;
    for run in cells.chunk_by(|a, b| colours(a).1 == colours(b).1) {
        let (x, fill) = (at * w, colours(run[0]).1);
        at += run.len();
        if fill != BACKGROUND {
            let width = run.len() * w;
            svg.push_str(&format!("<rect x=\"{x}\" y=\"{top}\" width=\"{width}\" height=\"{h}\" fill=\"{fill}\"/>\n"));
        }
    }
    let blank = |c: &Cell| c.symbol().trim().is_empty();
    at = 0;
    for run in cells.chunk_by(|a, b| blank(a) == blank(b) && text_style(a) == text_style(b)) {
        let from = at;
        at += run.len();
        if blank(run[0]) {
            continue;
        }
        let xs: Vec<String> = (from..at).map(|x| (x * w).to_string()).collect();
        let glyphs: String = run.iter().map(|c| c.symbol()).collect();
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" {}>{}</text>\n",
            xs.join(" "),
            top + BASELINE,
            text_style(run[0]),
            escaped(&glyphs)
        ));
    }
}

fn svg(buf: &Buffer) -> String {
    let (w, h) = (buf.area.width as usize * CELL.0, buf.area.height as usize * CELL.1);
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\" \
         font-family=\"ui-monospace, Menlo, Consolas, monospace\" font-size=\"14\">\n\
         <rect width=\"{w}\" height=\"{h}\" fill=\"{BACKGROUND}\"/>\n"
    );
    (0..buf.area.height).for_each(|y| row(buf, y, &mut svg));
    svg + "</svg>\n"
}

/// What differs from one run or machine to the next (where the drive is, its identity, the clock)
/// made the same: a shown time is local, so it's set from a local one.
fn hold_still(app: &mut App) {
    let when = Local.with_ymd_and_hms(2026, 10, 30, 21, 30, 0).unwrap().with_timezone(&Utc);
    let drive = &mut app.drives[0];
    drive.root = "/Volumes/DEMO".into();
    drive.id = Some(Uuid::from_u128(0x5eed_cafe << 96));
    drive.exported_at = Some(when);
    if let Some(e) = &mut drive.last_known {
        e.at = when;
    }
    let drive = drive.clone();
    let snapshots = app.status.iter_mut().flatten().flatten().flat_map(|d| {
        d.status.drive = drive.clone();
        d.status.head.as_mut()
    });
    let listed = app.history.iter_mut().flatten().flatten().flat_map(|(_, snapshots)| snapshots);
    for s in snapshots.chain(listed) {
        (s.created, s.short_id) = (when, "5eedcafe0".into());
    }
}

fn drawn(app: &mut App, (w, h): (u16, u16)) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| ui::draw(f, app)).unwrap();
    terminal.backend().buffer().clone()
}

#[test]
fn the_screenshots_are_the_screens() {
    theme::set_test_mode(Some(theme::Mode::Dark));
    theme::set_test_truecolor();
    let Fixture { _dir, mut app, .. } = fixture();
    hold_still(&mut app);
    for view in VIEWS {
        app.view = view;
        for size in SIZES {
            let name = format!("{}-{}x{}.svg", view.title().to_lowercase(), size.0, size.1);
            holds(&Path::new(DOCS).join("player").join(name), &svg(&drawn(&mut app, size)));
        }
    }
}
