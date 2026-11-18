//! The player's widgets as Storybook shows them (apps/web/src/lib/tui): each drawn alone at a fixed
//! size, from the demo drive's second track, through crossterm into the bytes a terminal is sent.
//! A still is an `.ansi` file and one that moves an asciicast v2 `.cast`, each in the dark and the
//! light palette, and `stories.json` lists them with their sizes. The test fails when a file isn't
//! what the player draws, or is one no story makes, and when a widget in loclib-waveform has no
//! story. To update them:
//!
//! `LOCALLIBRARY_BLESS=1 cargo nextest run -p locallibrary-cli -E 'test(/tui::ui::tests::stories::/)'`

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use locallibrary::Device;
use locallibrary::anlz::{self, TrackWaves};
use loclib_waveform::{
    CueLane, Detail, Overview, PhraseStrip, Pixels, Scope, Spectrum, Wave, WaveOverview, band_gain, bar_layout,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::{Frame, Terminal, TerminalOptions, Viewport};
use serde_json::{Value, json};
use tempfile::TempDir;

use crate::tui::app::testing::{Written, blessing, cue, demo_drive, holds, playing};
use crate::tui::app::{NowPlaying, phrase_spans};
use crate::tui::icons::{self, IconSet};
use crate::tui::theme::{self, Mode, Tier};

use super::super::{beat_spans, phrase_chip};

const CAPTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../web/src/lib/tui/captures");
const WIDGETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../crates/loclib-waveform/src");
const RERUN: &str = "LOCALLIBRARY_BLESS=1 cargo nextest run -p locallibrary-cli -E 'test(/tui::ui::tests::stories::/)'";
const MODES: [(Mode, &str); 2] = [(Mode::Dark, "dark"), (Mode::Light, "light")];

/// Draws frame `n` of a story.
type Draw<'a> = Box<dyn Fn(&mut Frame, usize) + 'a>;

/// A widget in one state, or moving through several.
struct Story<'a> {
    /// Where its files go under `captures`, less the palette and extension: `<widget>/<state>`.
    id: &'static str,
    /// What draws it: the widget's type, or the function that makes its spans.
    widget: &'static str,
    cols: u16,
    rows: u16,
    /// Frames, and the seconds between them for one that moves.
    frames: usize,
    every: Option<f64>,
    draw: Draw<'a>,
}

impl<'a> Story<'a> {
    fn still(id: &'static str, widget: &'static str, (cols, rows): (u16, u16), draw: impl Fn(&mut Frame) + 'a) -> Self {
        Story { id, widget, cols, rows, frames: 1, every: None, draw: Box::new(move |f, _| draw(f)) }
    }

    fn moving(
        id: &'static str,
        widget: &'static str,
        (cols, rows): (u16, u16),
        (frames, every): (usize, f64),
        draw: impl Fn(&mut Frame, usize) + 'a,
    ) -> Self {
        Story { id, widget, cols, rows, frames, every: Some(every), draw: Box::new(draw) }
    }

    /// What a terminal its size is sent for each frame, in the palette drawn in now.
    fn written(&self) -> Vec<String> {
        let viewport = Viewport::Fixed(Rect::new(0, 0, self.cols, self.rows));
        let written = Written::default();
        let backend = CrosstermBackend::new(written.clone());
        let mut terminal = Terminal::with_options(backend, TerminalOptions { viewport }).unwrap();
        (0..self.frames)
            .map(|i| {
                terminal.draw(|f| (self.draw)(f, i)).unwrap();
                String::from_utf8(std::mem::take(&mut *written.0.lock().unwrap())).unwrap()
            })
            .collect()
    }

    /// Its file in the palette drawn in now: the bytes, or for one that moves, an asciicast v2
    /// recording of its frames.
    fn file(&self, page: &Value) -> (String, String) {
        let frames = self.written();
        let Some(every) = self.every else { return ("ansi".into(), frames.concat()) };
        let header = json!({ "version": 2, "width": self.cols, "height": self.rows, "theme": page });
        let events = frames.iter().enumerate().map(|(i, out)| json!([i as f64 * every, "o", out]).to_string() + "\n");
        ("cast".into(), format!("{header}\n{}", events.collect::<String>()))
    }
}

/// The demo drive's second track (two minutes at 120, with rekordbox's phrases), playing, with cues
/// added (it has none of its own): a hot cue at the first verse, the bridge and the outro, a memory
/// cue and an eight-beat loop.
struct Track {
    now: NowPlaying,
    waves: TrackWaves,
    _dir: TempDir,
    _drive: TempDir,
}

fn track() -> Track {
    let (drive, root) = demo_drive();
    let device = Device::open_export(&root).unwrap();
    let t = device.library().unwrap().tracks.into_iter().find(|t| t.title == "Demo Track 2").unwrap();
    let mut analysis = anlz::load(&device, &t);
    let at = |beat: usize| analysis.beats[beat - 1].time_ms;
    let named = |hot, beat, end: Option<usize>, name: &str| {
        let mut c = cue(hot, at(beat), end.map(at));
        c.comment = name.into();
        c
    };
    analysis.cues = vec![
        named(Some(0), 33, None, "Verse"),
        named(None, 61, None, ""),
        named(Some(1), 125, None, "Bridge"),
        named(None, 157, Some(165), ""),
        named(Some(2), 221, None, "Outro"),
    ];
    let duration = Duration::from_secs(u64::from(t.duration_secs));
    let phrases = phrase_spans(&analysis, duration);
    let waves = anlz::load_waves(&device, &t);
    let (mut app, dir) = playing(analysis);
    let mut now = app.now.take().unwrap();
    (now.track, now.duration, now.phrases) = (t, duration, phrases);
    Track { now, waves, _dir: dir, _drive: drive }
}

impl Track {
    /// Where beat `n` (from 1) of the grid is, in ms.
    fn beat(&self, n: usize) -> f64 {
        f64::from(self.now.analysis.beats[n - 1].time_ms)
    }

    fn duration_ms(&self) -> f64 {
        self.now.duration.as_secs_f64() * 1000.0
    }

    fn progress(&self, ms: f64) -> f32 {
        (ms / self.duration_ms()) as f32
    }

    /// The phrases as the overview's strip has them.
    fn phrases(&self) -> Vec<(f64, f64, String, [u8; 3])> {
        let named =
            self.now.phrases.iter().map(|p| (p.start_ms, p.end_ms, p.kind.name(), theme::current().phrase(p.kind)));
        named.collect()
    }
}

/// A spectrum's levels and peak caps across `n` bands: the bass loudest, a bump in the mids.
fn levels(n: usize) -> (Vec<f32>, Vec<(f32, f32)>) {
    let level = |i: usize| {
        let x = i as f32 / n as f32;
        let bump = if (0.4..0.6).contains(&x) { 0.25 } else { 0.0 };
        (0.95 - 0.8 * x + bump).min(1.0)
    };
    let levels: Vec<f32> = (0..n).map(level).collect();
    let peaks = levels.iter().map(|l| ((l + 0.1).min(1.0), 0.0)).collect();
    (levels, peaks)
}

/// A low tone with a quieter high one over it, as samples from -1 to 1. Parabolas, not `sin`, so
/// every platform makes the same numbers.
fn tone() -> Vec<f32> {
    let wave = |t: f32| {
        let x = t.rem_euclid(2.0);
        if x < 1.0 { 4.0 * x * (1.0 - x) } else { -4.0 * (x - 1.0) * (2.0 - x) }
    };
    (0..480).map(|i| 0.6 * wave(i as f32 / 60.0) + 0.25 * wave(i as f32 / 7.5)).collect()
}

/// A gradient across the accent and the playhead's colours, two pixels a cell.
fn picture((w, h): (u16, u16)) -> Vec<Option<[u8; 3]>> {
    let (from, to) = (theme::current().accent, theme::current().playhead);
    let (w, h) = (usize::from(w), usize::from(h) * 2);
    let at = |x: usize, y: usize| -> [u8; 3] {
        std::array::from_fn(|i| {
            ((usize::from(from[i]) * (w - x) + usize::from(to[i]) * x) * (h - y / 2) / (w * h)) as u8
        })
    };
    (0..h).flat_map(|y| (0..w).map(move |x| Some(at(x, y)))).collect()
}

fn stories(t: &Track) -> Vec<Story<'_>> {
    let now = &t.now;
    let cues = &now.analysis.cues;
    let duration_ms = t.duration_ms();
    let rgb = t.waves.rgb_preview.as_ref();
    let detail = t.waves.rgb_detail.as_ref().unwrap();
    let bands = t.waves.band_preview.as_ref().unwrap();
    let (intro, verse, bridge, outro) = (t.beat(9), t.beat(33), t.beat(141), t.beat(159));
    let overview = move |progress| Overview { wave: rgb, progress, cues, duration_ms, palette: theme::palette() };
    let detail = move |ms: f64| Detail {
        wave: detail,
        position_secs: ms / 1000.0,
        visible_secs: 8.0,
        beats: &now.analysis.beats,
        cues,
        palette: theme::palette(),
    };
    let phrases = move |f: &mut Frame, progress| {
        let phrases = t.phrases();
        f.render_widget(PhraseStrip { phrases: &phrases, duration_ms, progress, palette: theme::palette() }, f.area());
    };
    let spectrum = |f: &mut Frame, mirror| {
        let (levels, peaks) = levels(bar_layout(f.area().width, mirror).0);
        f.render_widget(
            Spectrum { bands: &levels, peaks: &peaks, mirror, tint: None, palette: theme::palette() },
            f.area(),
        );
    };
    let beat = |f: &mut Frame, ms| f.render_widget(Line::from(beat_spans(now, ms)), f.area());
    let phrase = |f: &mut Frame, ms| {
        f.render_widget(Line::from(phrase_chip(now, ms).map(Vec::from).unwrap_or_default()), f.area())
    };
    vec![
        Story::still("overview/start", "Overview", (80, 4), move |f| f.render_widget(overview(0.0), f.area())),
        Story::still("overview/bridge", "Overview", (80, 4), move |f| {
            f.render_widget(overview(t.progress(bridge)), f.area());
        }),
        Story::still("wave-overview/three-band", "WaveOverview", (80, 5), move |f| {
            let wave = Some(Wave::Bands { bands, columns_per_sec: None, gain: band_gain(bands) });
            let palette = theme::palette();
            let w = WaveOverview { wave, progress: t.progress(bridge), cues, duration_ms, cue_rows: 2, palette };
            f.render_widget(w, f.area());
        }),
        Story::still("detail/on-a-cue", "Detail", (80, 8), move |f| f.render_widget(detail(verse), f.area())),
        Story::still("detail/in-a-loop", "Detail", (80, 8), move |f| f.render_widget(detail(outro), f.area())),
        Story::still("cue-lane/bridge", "CueLane", (80, 2), move |f| {
            let lane = CueLane {
                cues,
                position_secs: bridge / 1000.0,
                visible_secs: 32.0,
                bpm: 120.0,
                palette: theme::palette(),
            };
            f.render_widget(lane, f.area());
        }),
        Story::still("phrase-strip/intro", "PhraseStrip", (80, 1), move |f| phrases(f, t.progress(intro))),
        Story::still("phrase-strip/bridge", "PhraseStrip", (80, 1), move |f| phrases(f, t.progress(bridge))),
        // A cell is 1.6 s of the track: the playhead crosses into the first verse a cell a frame.
        Story::moving("phrase-strip/into-the-verse", "PhraseStrip", (80, 1), (8, 0.4), move |f, i| {
            phrases(f, t.progress(verse - 6400.0 + i as f64 * 1600.0));
        }),
        Story::still("spectrum/bars", "Spectrum", (40, 8), move |f| spectrum(f, false)),
        Story::still("spectrum/mirrored", "Spectrum", (40, 8), move |f| spectrum(f, true)),
        Story::still("scope/tone", "Scope", (40, 6), |f| {
            f.render_widget(Scope { samples: &tone(), tint: None, palette: theme::palette() }, f.area());
        }),
        Story::still("pixels/gradient", "Pixels", (16, 8), |f| {
            let pixels = picture((16, 8));
            f.render_widget(Pixels { width: 16, pixels: &pixels, palette: theme::palette() }, f.area());
        }),
        Story::still("beat/one", "beat_spans", (10, 1), move |f| beat(f, t.beat(33))),
        Story::still("beat/two", "beat_spans", (10, 1), move |f| beat(f, t.beat(34))),
        Story::still("beat/three", "beat_spans", (10, 1), move |f| beat(f, t.beat(35))),
        Story::still("beat/four", "beat_spans", (10, 1), move |f| beat(f, t.beat(36))),
        // Two bars at the track's tempo.
        Story::moving("beat/two-bars", "beat_spans", (10, 1), (8, 0.5), move |f, i| beat(f, t.beat(33 + i))),
        Story::still("phrase/intro", "phrase_chip", (16, 1), move |f| phrase(f, intro)),
        Story::still("phrase/bridge", "phrase_chip", (16, 1), move |f| phrase(f, bridge)),
    ]
}

/// Draw in `mode`, in 24-bit colour, with the Unicode icons (a web font has no Nerd Font glyphs).
fn draw_in(mode: Option<Mode>) {
    theme::set_test_tier(mode.map(|_| Tier::Truecolor));
    theme::set_test_mode(mode);
    icons::set_test_set(mode.map(|_| IconSet::Unicode));
}

/// The colours a terminal leaves to the theme, in the palette drawn in now, as asciicast's theme
/// has them (its palette is the terminal's 16 colours, which nothing here draws in).
fn page() -> Value {
    let hex = |[r, g, b]: [u8; 3]| format!("#{r:02x}{g:02x}{b:02x}");
    let ansi = ["#000000", "#cd0000", "#00cd00", "#cdcd00", "#0000ee", "#cd00cd", "#00cdcd", "#e5e5e5"];
    json!({ "fg": hex(theme::current().text), "bg": hex(theme::page()), "palette": ansi.join(":") })
}

fn files_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let entries = entries.map(|e| e.unwrap().path());
    entries.flat_map(|p| if p.is_dir() { files_in(&p) } else { vec![p] }).collect()
}

/// The captures are the same on every platform, so this checks them where crossterm always writes
/// escape codes: on Windows it may call the console instead, when the one a test has won't say it
/// takes them.
#[test]
#[cfg_attr(windows, ignore = "crossterm may draw through the console API, not escape codes")]
fn the_stories_are_what_the_player_draws() {
    let t = track();
    let stories = stories(&t);
    let root = Path::new(CAPTURES);
    let mut made = BTreeSet::new();
    let mut pages = serde_json::Map::new();
    for (mode, name) in MODES {
        draw_in(Some(mode));
        let page = page();
        for s in &stories {
            let (ext, file) = s.file(&page);
            let path = root.join(format!("{}.{name}.{ext}", s.id));
            holds(&path, &file, RERUN);
            made.insert(path);
        }
        pages.insert(name.into(), page);
    }
    draw_in(None);
    let listed: serde_json::Map<_, _> = (stories.iter())
        .map(|s| {
            (s.id.into(), json!({ "widget": s.widget, "cols": s.cols, "rows": s.rows, "moves": s.every.is_some() }))
        })
        .collect();
    let manifest = root.join("stories.json");
    let json = serde_json::to_string_pretty(&json!({ "pages": pages, "stories": listed })).unwrap();
    holds(&manifest, &(json + "\n"), RERUN);
    made.insert(manifest);

    let stale: Vec<_> = files_in(root).into_iter().filter(|p| !made.contains(p)).collect();
    if blessing() {
        stale.iter().for_each(|p| std::fs::remove_file(p).unwrap());
        return;
    }
    assert!(stale.is_empty(), "no story makes {stale:?}: remove them with\n  {RERUN}");
}

/// Every widget loclib-waveform has, in a story of its own (as the web app's components are).
#[test]
fn every_widget_has_a_story() {
    let t = track();
    let storied: BTreeSet<&str> = stories(&t).iter().map(|s| s.widget).collect();
    let widgets: BTreeSet<String> = files_in(Path::new(WIDGETS))
        .iter()
        .flat_map(|p| std::fs::read_to_string(p).unwrap().lines().map(str::to_string).collect::<Vec<_>>())
        .filter_map(|line| {
            let (_, ty) = line.split_once("Widget for ").filter(|_| line.starts_with("impl"))?;
            Some(ty.split(|c: char| !c.is_alphanumeric() && c != '_').next()?.to_string())
        })
        .collect();
    assert!(widgets.len() >= 8, "found the widgets: {widgets:?}");
    let missing: Vec<_> = widgets.iter().filter(|w| !storied.contains(w.as_str())).collect();
    assert!(missing.is_empty(), "no story draws {missing:?}: add one to src/tui/ui/tests/stories.rs");
}
