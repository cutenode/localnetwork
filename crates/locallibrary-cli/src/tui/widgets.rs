//! Custom widgets: spectrum bars, oscilloscope, rekordbox waveforms, cover art.

use locallibrary::anlz::{Beat, Cue, WaveColumn, Waveform};
use locallibrary::model::TrackColor;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::symbols::Marker;
use ratatui::widgets::Widget;
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine};

use super::theme::{self, rgb3};

/// Partial blocks, filled from the bottom, in eighths.
const LOWER: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

fn put(buf: &mut Buffer, x: u16, y: u16, symbol: &str, fg: Color, bg: Option<Color>) {
    if let Some(cell) = buf.cell_mut((x, y)) {
        cell.set_symbol(symbol).set_fg(fg);
        if let Some(bg) = bg {
            cell.set_bg(bg);
            // Without colours, a coloured cell (a cue's letter, a loop) shows in reverse video.
            if theme::plain() {
                cell.modifier.insert(ratatui::style::Modifier::REVERSED);
            }
        }
    }
}

/// How many spectrum bands fit in `width` columns, and how wide each bar and gap is.
pub fn bar_layout(width: u16, mirror: bool) -> (usize, u16, u16) {
    let (bar, gap) = if width >= 48 { (2, 1) } else { (1, 0) };
    let mut n = ((width + gap) / (bar + gap)) as usize;
    if mirror {
        n /= 2;
    }
    (n.max(1), bar, gap)
}

/// Vertical bars with peak caps, coloured along a gradient by height. `mirror` puts the bass in
/// the middle and spreads the highs out to both sides.
pub struct Spectrum<'a> {
    pub bands: &'a [f32],
    pub peaks: &'a [(f32, f32)],
    pub mirror: bool,
    /// A colour washed over the bars (a cue just passed) and how strongly, 0-1.
    pub tint: Option<([u8; 3], f32)>,
}

impl Widget for Spectrum<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() || self.bands.is_empty() {
            return;
        }
        let (_, bar, gap) = bar_layout(area.width, self.mirror);
        let n = self.bands.len();
        // Bars in display order (index into bands).
        let order: Vec<usize> = if self.mirror { (0..n).rev().chain(0..n).collect() } else { (0..n).collect() };
        let used = order.len() as u16 * (bar + gap) - gap;
        let left = area.x + area.width.saturating_sub(used) / 2;
        let h = area.height as f32;
        for (slot, &band) in order.iter().enumerate() {
            let x0 = left + slot as u16 * (bar + gap);
            if x0 + bar > area.right() {
                break;
            }
            let level = self.bands[band] * h;
            let peak = self.peaks.get(band).map_or(0.0, |p| p.0) * h;
            for row in 0..area.height {
                let y = area.bottom() - 1 - row;
                let fill = ((level - row as f32) * 8.0).round().clamp(0.0, 8.0) as usize;
                let mut color = theme::gradient(&theme::SPECTRUM, (row as f32 + 0.5) / h);
                if let Some((tint, t)) = self.tint {
                    color = theme::mix(color, tint, t * 0.7);
                }
                let (symbol, fg) = if fill > 0 {
                    (LOWER[fill], rgb3(color))
                } else if peak > level + 0.25 && peak.floor() as u16 == row && peak > 0.05 {
                    (if peak.fract() < 0.5 { "▁" } else { "▔" }, rgb3(theme::mix(color, [255, 255, 255], 0.35)))
                } else {
                    continue;
                };
                for dx in 0..bar {
                    put(buf, x0 + dx, y, symbol, fg, None);
                }
            }
        }
    }
}

/// Oscilloscope trace in braille, scaled to fill the height.
pub struct Scope<'a> {
    pub samples: &'a [f32],
    pub tint: Option<([u8; 3], f32)>,
}

impl Widget for Scope<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() || self.samples.len() < 2 {
            return;
        }
        let points = (area.width as usize * 2).min(self.samples.len());
        let step = self.samples.len() as f64 / points as f64;
        // Auto-gain, so quiet passages stay readable (up to 6x; silence stays flat).
        let peak = self.samples.iter().fold(0.0f32, |m, s| m.max(s.abs())) as f64;
        let gain = if peak > 1e-4 { (0.92 / peak).clamp(1.0, 6.0) } else { 1.0 };
        let at = |i: usize| self.samples[((i as f64 * step) as usize).min(self.samples.len() - 1)] as f64 * gain;
        Canvas::default()
            .marker(Marker::Braille)
            .x_bounds([0.0, points as f64])
            .y_bounds([-1.0, 1.0])
            .paint(|ctx| {
                ctx.draw(&CanvasLine::new(0.0, 0.0, points as f64, 0.0, rgb3(theme::FAINT)));
                for i in 1..points {
                    let (a, b) = (at(i - 1).clamp(-1.0, 1.0), at(i).clamp(-1.0, 1.0));
                    let t = (a.abs().max(b.abs()) as f32).powf(0.7);
                    let mut color = theme::gradient(&[theme::CYAN, theme::VIOLET, theme::PINK], t);
                    if let Some((tint, k)) = self.tint {
                        color = theme::mix(color, tint, k * 0.7);
                    }
                    ctx.draw(&CanvasLine::new((i - 1) as f64, a, i as f64, b, rgb3(color)));
                }
            })
            .render(area, buf);
    }
}

/// The loudest waveform column in `[from, to)`, or the one at `from` when the range is empty.
fn column(w: &Waveform, from: f64, to: f64) -> Option<WaveColumn> {
    if from < 0.0 || from >= w.columns.len() as f64 {
        return None;
    }
    let a = from as usize;
    let b = (to.ceil() as usize).clamp(a + 1, w.columns.len());
    w.columns[a..b].iter().copied().max_by(|x, y| x.back.height.total_cmp(&y.back.height))
}

/// The colour a cue is shown in: its own (a hot cue's, or a memory cue's from rekordbox's colour
/// table), else rekordbox's default for its kind.
pub fn cue_color(c: &Cue) -> [u8; 3] {
    match (c.color.or_else(|| TrackColor::from_id(c.color_id).map(theme::track_color)), c.hot_cue, c.end_ms) {
        (Some(rgb), _, _) => rgb,
        (None, _, Some(_)) => theme::LOOP,
        (None, Some(_), None) => theme::HOT_CUE,
        (None, None, None) => theme::MEMORY_CUE,
    }
}

/// `A`-`H` (and on) for hot cues.
pub fn cue_letter(c: &Cue) -> Option<char> {
    c.hot_cue.filter(|h| *h < 26).map(|h| (b'A' + h) as char)
}

/// What a cue is called on screen: its comment, plus a loop's length in beats as rekordbox stored
/// it, or worked out from the tempo (`loop in ↻8`).
pub fn cue_label(c: &Cue, bpm: f64) -> String {
    let beats = match (c.end_ms, c.loop_beats) {
        (Some(_), Some((n, d))) => Some(if n % d == 0 { format!("↻{}", n / d) } else { format!("↻{n}/{d}") }),
        (Some(end), None) if bpm > 0.0 => {
            let b = (end - c.start_ms) as f64 / 60_000.0 * bpm;
            Some(if b >= 0.9 { format!("↻{:.0}", b.round()) } else { format!("↻1/{:.0}", (1.0 / b).round()) })
        }
        (Some(_), None) => Some("↻".to_string()),
        (None, _) => None,
    };
    match (c.comment.trim(), beats) {
        ("", Some(b)) => b,
        ("", None) => String::new(),
        (name, Some(b)) => format!("{name} {b}"),
        (name, None) => name.to_string(),
    }
}

/// Marker for a cue in a single cell: the hot cue's letter on its colour, or a triangle.
fn put_cue(buf: &mut Buffer, x: u16, y: u16, c: &Cue, dim: bool) {
    let color = cue_color(c);
    let color = if dim { theme::scale(color, 0.55) } else { color };
    match cue_letter(c) {
        Some(letter) => put(buf, x, y, &letter.to_string(), rgb3([12, 12, 16]), Some(rgb3(color))),
        None => put(buf, x, y, "▼", rgb3(color), None),
    }
}

/// Write cue names along row `y`, each starting at its marker and cut short before the next one.
/// `marks` are (column, label, colour), in any order.
fn put_labels(buf: &mut Buffer, area: Rect, y: u16, mut marks: Vec<(u16, String, [u8; 3])>) {
    marks.retain(|(_, l, _)| !l.is_empty());
    marks.sort_by_key(|(x, _, _)| *x);
    let mut free = 0u16;
    for i in 0..marks.len() {
        let (x, label, color) = &marks[i];
        if *x < free {
            continue;
        }
        let next = marks[i + 1..].iter().map(|(nx, _, _)| *nx).find(|nx| nx > x).unwrap_or(area.width);
        let room = next.saturating_sub(*x + 1) as usize;
        if room < 2 {
            continue;
        }
        let (end, _) = buf.set_stringn(area.x + x, y, label, room, theme::fg(*color));
        free = end - area.x + 1;
    }
}

/// Whole-track overview: a row of cue markers above a bottom-anchored waveform; the part already
/// played is dimmed, with the playhead in white.
pub struct Overview<'a> {
    pub wave: Option<&'a Waveform>,
    /// 0-1.
    pub progress: f32,
    pub cues: &'a [Cue],
    pub duration_ms: f64,
}

impl Widget for Overview<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 2 || area.width < 4 {
            return;
        }
        let marker_y = area.y;
        let wave_area = Rect { y: area.y + 1, height: area.height - 1, ..area };
        let w = area.width as f64;
        let head = ((self.progress.clamp(0.0, 1.0) as f64 * w) as u16).min(area.width - 1);

        if self.duration_ms > 0.0 {
            let x_of = |ms: u32| ((ms as f64 / self.duration_ms) * w) as u16;
            for c in self.cues {
                if let Some(end) = c.end_ms {
                    for x in x_of(c.start_ms)..=x_of(end).min(area.width - 1) {
                        put(buf, area.x + x, marker_y, "▁", rgb3(theme::scale(cue_color(c), 0.8)), None);
                    }
                }
            }
            // Memory cues first, so a hot cue at the same spot stays visible.
            let mut cues: Vec<&Cue> = self.cues.iter().collect();
            cues.sort_by_key(|c| c.hot_cue.is_some());
            for c in cues {
                let x = x_of(c.start_ms);
                if x < area.width {
                    put_cue(buf, area.x + x, marker_y, c, x < head);
                }
            }
        }

        let Some(wave) = self.wave.filter(|w| !w.columns.is_empty()) else {
            // No analysis: a plain progress rail.
            let y = wave_area.y + wave_area.height / 2;
            for x in 0..area.width {
                let (sym, c) = if x < head { ("━", theme::PINK) } else { ("─", theme::FAINT) };
                put(buf, area.x + x, y, sym, rgb3(c), None);
            }
            put(buf, area.x + head, y, "●", rgb3(theme::FG), None);
            return;
        };

        let n = wave.columns.len() as f64;
        let h = wave_area.height as f32;
        for x in 0..area.width {
            let col = column(wave, x as f64 * n / w, (x + 1) as f64 * n / w).unwrap_or_default();
            let played = x < head;
            let shade = |c: [u8; 3]| {
                if x == head {
                    [255, 255, 255]
                } else if played {
                    theme::scale(c, 0.5)
                } else {
                    theme::scale(c, 1.25)
                }
            };
            let (back, front) = (col.back.height * h, col.front.height * h);
            for row in 0..wave_area.height {
                let y = wave_area.bottom() - 1 - row;
                let r = row as f32;
                let back_fill = ((back - r) * 8.0).round().clamp(0.0, 8.0) as usize;
                let front_fill = ((front - r) * 8.0).round().clamp(0.0, 8.0) as usize;
                if back_fill == 0 {
                    if x == head {
                        put(buf, area.x + x, y, "│", rgb3(theme::FG), None);
                    }
                    continue;
                }
                if front_fill == 8 {
                    put(buf, area.x + x, y, "█", rgb3(shade(col.front.color)), None);
                } else if front_fill > 0 && back_fill == 8 {
                    // Front layer fills the bottom of the cell, the back layer the rest.
                    put(
                        buf,
                        area.x + x,
                        y,
                        LOWER[front_fill],
                        rgb3(shade(col.front.color)),
                        Some(rgb3(shade(col.back.color))),
                    );
                } else {
                    put(buf, area.x + x, y, LOWER[back_fill], rgb3(shade(col.back.color)), None);
                }
            }
        }
    }
}

/// Scrolling detail waveform centred on the playhead, mirrored, with beat ticks above it
/// (bar starts brighter) and cue markers.
pub struct Detail<'a> {
    pub wave: &'a Waveform,
    pub position_secs: f64,
    pub visible_secs: f64,
    pub beats: &'a [Beat],
    pub cues: &'a [Cue],
}

impl Widget for Detail<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 3 || area.width < 8 {
            return;
        }
        let cps = self.wave.columns_per_sec.unwrap_or(150.0);
        let w = area.width as f64;
        let start = self.position_secs - self.visible_secs / 2.0;
        let secs_per_col = self.visible_secs / w;
        let centre = area.width / 2;
        let x_of = |secs: f64| ((secs - start) / secs_per_col).floor();

        let tick_y = area.y;
        for b in self.beats {
            let x = x_of(b.time_ms as f64 / 1000.0);
            if (0.0..w).contains(&x) {
                let (sym, c) = if b.beat_number == 1 { ("╻", theme::PINK) } else { ("╷", theme::DIM) };
                put(buf, area.x + x as u16, tick_y, sym, rgb3(c), None);
            }
        }
        let wave_area = Rect { y: area.y + 1, height: area.height - 1, ..area };
        let in_view = |x: f64| (0.0..w).contains(&x);
        // Loops: a wash of the loop's colour behind the waveform for their length.
        for c in self.cues {
            let Some(end) = c.end_ms else { continue };
            let (a, b) = (x_of(c.start_ms as f64 / 1000.0).max(0.0), x_of(end as f64 / 1000.0).min(w - 1.0));
            if b < 0.0 || a >= w {
                continue;
            }
            let wash = rgb3(theme::scale(cue_color(c), 0.22));
            for x in a as u16..=b as u16 {
                for y in wave_area.y..wave_area.bottom() {
                    if let Some(cell) = buf.cell_mut((area.x + x, y)) {
                        cell.set_bg(wash);
                    }
                }
            }
        }

        let up_rows = wave_area.height / 2;
        let down_rows = wave_area.height - up_rows;
        let centre_y = wave_area.y + up_rows;
        for x in 0..area.width {
            let t0 = start + x as f64 * secs_per_col;
            let Some(col) = column(self.wave, t0 * cps, (t0 + secs_per_col) * cps) else { continue };
            let color = if x == centre { [255, 255, 255] } else { col.back.color };
            let past = x < centre;
            let color = if past { theme::scale(color, 0.7) } else { color };
            let a = col.back.height;
            // Upper half grows up from the centre line, in eighths.
            let up = a * up_rows as f32;
            for k in 0..up_rows {
                let fill = ((up - k as f32) * 8.0).round().clamp(0.0, 8.0) as usize;
                if fill > 0 {
                    put(buf, area.x + x, centre_y - 1 - k, LOWER[fill], rgb3(color), None);
                }
            }
            // Lower half: a dimmer reflection, in half cells.
            let down = a * down_rows as f32;
            let reflection = theme::scale(color, 0.55);
            for k in 0..down_rows {
                let f = down - k as f32;
                let sym = if f >= 0.75 {
                    "█"
                } else if f >= 0.25 {
                    "▀"
                } else {
                    continue;
                };
                put(buf, area.x + x, centre_y + k, sym, rgb3(reflection), None);
            }
            if x == centre && a == 0.0 {
                put(buf, area.x + x, centre_y, "│", rgb3(theme::FG), None);
            }
        }

        // Cue lines through the waveform (memory cues first, hot cues over them), flags above
        // with their names, and memory cues marked underneath too.
        let mut cues: Vec<&Cue> = self.cues.iter().collect();
        cues.sort_by_key(|c| c.hot_cue.is_some());
        let mut labels = Vec::new();
        for c in &cues {
            let x = x_of(c.start_ms as f64 / 1000.0);
            if !in_view(x) {
                continue;
            }
            let x = x as u16;
            let color = cue_color(c);
            for y in wave_area.y..wave_area.bottom() {
                if let Some(cell) = buf.cell_mut((area.x + x, y)) {
                    if cell.symbol() == " " {
                        cell.set_symbol("│");
                    }
                    cell.set_fg(rgb3(color));
                }
            }
            if c.hot_cue.is_none() {
                put(buf, area.x + x, wave_area.bottom() - 1, "▲", rgb3(color), None);
            }
            put_cue(buf, area.x + x, tick_y, c, false);
            labels.push((x + 1, cue_label(c, self.beats.first().map_or(0.0, |b| b.bpm)), color));
        }
        // Loop ends get a thinner line.
        for c in self.cues {
            if let Some(end) = c.end_ms {
                let x = x_of(end as f64 / 1000.0);
                if in_view(x) {
                    put(buf, area.x + x as u16, tick_y, "╷", rgb3(cue_color(c)), None);
                }
            }
        }
        put_labels(buf, area, tick_y, labels);
        put(buf, area.x + centre, tick_y, "▼", rgb3(theme::FG), None);
    }
}

/// The stretch of track around the playhead with its cues: hot cues as lettered chips, memory cues
/// as diamonds, loops as bars over their length, and (given two rows) their names underneath.
/// Shown under the spectrum and scope visualisers.
pub struct CueLane<'a> {
    pub cues: &'a [Cue],
    pub position_secs: f64,
    pub visible_secs: f64,
    pub bpm: f64,
}

impl Widget for CueLane<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 8 || area.height == 0 {
            return;
        }
        let w = area.width as f64;
        let start = self.position_secs - self.visible_secs / 2.0;
        let secs_per_col = self.visible_secs / w;
        let centre = area.width / 2;
        let x_of = |ms: u32| ((ms as f64 / 1000.0 - start) / secs_per_col).floor();
        let y = area.y;
        let past = |x: u16| x < centre;
        let shade = |c: [u8; 3], x: u16| if past(x) { theme::scale(c, 0.5) } else { c };

        for x in 0..area.width {
            let c = if past(x) { theme::FAINT } else { theme::mix(theme::FAINT, theme::DIM, 0.5) };
            put(buf, area.x + x, y, "─", rgb3(c), None);
        }
        for c in self.cues {
            let Some(end) = c.end_ms else { continue };
            let (a, b) = (x_of(c.start_ms), x_of(end));
            if b < 0.0 || a >= w {
                continue;
            }
            for x in a.max(0.0) as u16..=(b.min(w - 1.0) as u16) {
                put(buf, area.x + x, y, "━", rgb3(shade(cue_color(c), x)), None);
            }
        }
        put(buf, area.x + centre, y, "┃", rgb3(theme::FG), None);
        let mut cues: Vec<&Cue> = self.cues.iter().collect();
        cues.sort_by_key(|c| c.hot_cue.is_some());
        let mut labels = Vec::new();
        for c in cues {
            let x = x_of(c.start_ms);
            if !(0.0..w).contains(&x) {
                continue;
            }
            let x = x as u16;
            let color = shade(cue_color(c), x);
            match cue_letter(c) {
                Some(l) => put(buf, area.x + x, y, &l.to_string(), rgb3([12, 12, 16]), Some(rgb3(color))),
                None => put(buf, area.x + x, y, "◆", rgb3(color), None),
            }
            labels.push((x, cue_label(c, self.bpm), color));
        }
        if area.height >= 2 {
            put_labels(buf, area, y + 1, labels);
        }
    }
}

/// rekordbox's phrase analysis across the whole track, one row: each phrase a block of its
/// colour with its name, dimmed once played, and the playhead in white.
pub struct PhraseStrip<'a> {
    /// (start ms, end ms, name, colour), in order.
    pub phrases: &'a [(f64, f64, String, [u8; 3])],
    pub duration_ms: f64,
    pub progress: f32,
}

impl Widget for PhraseStrip<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 4 || area.height == 0 || self.duration_ms <= 0.0 {
            return;
        }
        let w = area.width as f64;
        let x_of = |ms: f64| ((ms / self.duration_ms) * w).round().clamp(0.0, w) as u16;
        let head = ((self.progress.clamp(0.0, 1.0) as f64 * w) as u16).min(area.width - 1);
        let y = area.y;
        let ink = rgb3([16, 16, 22]);
        for (start, end, name, color) in self.phrases {
            let (a, b) = (x_of(*start), x_of(*end).max(x_of(*start) + 1).min(area.width));
            for x in a..b {
                let c = if x < head { theme::scale(*color, 0.45) } else { *color };
                // A darker first cell keeps neighbouring phrases of the same colour apart.
                let c = if x == a { theme::scale(c, 0.7) } else { c };
                put(buf, area.x + x, y, " ", ink, Some(rgb3(c)));
            }
            let room = b.saturating_sub(a + 1) as usize;
            if room >= 2 {
                for (i, ch) in name.chars().take(room).enumerate() {
                    if let Some(cell) = buf.cell_mut((area.x + a + 1 + i as u16, y)) {
                        cell.set_char(ch).set_fg(ink);
                    }
                }
            }
        }
        if let Some(cell) = buf.cell_mut((area.x + head, y)) {
            cell.set_symbol("┃").set_fg(rgb3([255, 255, 255]));
        }
    }
}

/// An image in half blocks: two pixels per cell, top in the foreground, bottom in the background.
/// `None` pixels are transparent.
pub struct Pixels<'a> {
    pub width: u16,
    pub pixels: &'a [Option<[u8; 3]>],
}

impl Widget for Pixels<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let w = self.width as usize;
        // Pictures need colours; and they're drawn as they are, on a light terminal too.
        if w == 0 || theme::plain() {
            return;
        }
        let rows = self.pixels.len() / w / 2;
        for cy in 0..(rows as u16).min(area.height) {
            for cx in 0..self.width.min(area.width) {
                let top = self.pixels[cy as usize * 2 * w + cx as usize];
                let bottom = self.pixels[(cy as usize * 2 + 1) * w + cx as usize];
                let (x, y) = (area.x + cx, area.y + cy);
                match (top, bottom) {
                    (Some(t), Some(b)) => put(buf, x, y, "▀", theme::image(t), Some(theme::image(b))),
                    (Some(t), None) => put(buf, x, y, "▀", theme::image(t), None),
                    (None, Some(b)) => put(buf, x, y, "▄", theme::image(b), None),
                    (None, None) => {}
                }
            }
        }
    }
}

/// Half-block pixels, row-major, two rows per cell.
type PixelBuf = Vec<Option<[u8; 3]>>;

/// Cover art, decoded once and resampled to whatever cell size it is drawn at.
pub struct Art {
    image: image::RgbImage,
    cache: Option<(u16, u16, PixelBuf)>,
}

impl Art {
    pub fn decode(bytes: &[u8]) -> Option<Art> {
        let image = image::load_from_memory(bytes).ok()?.to_rgb8();
        Some(Art { image, cache: None })
    }

    /// Pixels for a `cols` x `rows` cell area (two pixels per cell vertically).
    pub fn pixels(&mut self, cols: u16, rows: u16) -> &[Option<[u8; 3]>] {
        if self.cache.as_ref().is_none_or(|(c, r, _)| (*c, *r) != (cols, rows)) {
            let resized = image::imageops::resize(
                &self.image,
                cols as u32,
                rows as u32 * 2,
                image::imageops::FilterType::Lanczos3,
            );
            let px = resized.pixels().map(|p| Some(p.0)).collect();
            self.cache = Some((cols, rows, px));
        }
        &self.cache.as_ref().unwrap().2
    }
}

/// A spinning record, for tracks without artwork. `angle` turns the light streak (radians).
pub fn vinyl(cols: u16, rows: u16, angle: f32, label: [u8; 3]) -> PixelBuf {
    let (w, h) = (cols as usize, rows as usize * 2);
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let radius = cx.min(cy);
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = ((x as f32 + 0.5 - cx) / radius, (y as f32 + 0.5 - cy) / radius);
            let r = (dx * dx + dy * dy).sqrt();
            let px = if !(0.06..=1.0).contains(&r) {
                None // outside, or the spindle hole
            } else if r < 0.36 {
                // Label, lit from the top left.
                Some(theme::scale(label, 1.0 - 0.35 * ((dy - dx) * 0.5 + 0.5).clamp(0.0, 1.0)))
            } else if r > 0.95 {
                Some([74, 74, 90]) // rim
            } else {
                // Grooves, plus a highlight that sweeps round as the record turns.
                let groove = if ((r * 40.0) as i32) % 2 == 0 { 1.0 } else { 0.78 };
                let theta = dy.atan2(dx) - angle;
                let streak = theta.cos().abs().powi(10) * (1.1 - r) * 1.4;
                Some(theme::mix(theme::scale(theme::VINYL, groove), [170, 170, 196], streak))
            };
            out.push(px);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use locallibrary::anlz::{WaveLayer, WaveformStyle};

    use super::super::app::testing::wave;
    use super::*;

    fn rendered(buf: &Buffer) -> String {
        buf.content.iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn spectrum_fills_from_the_bottom() {
        let area = Rect::new(0, 0, 10, 4);
        let mut buf = Buffer::empty(area);
        Spectrum { bands: &[1.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], peaks: &[], mirror: false, tint: None }
            .render(area, &mut buf);
        // Full bar in column 0, half bar in column 1.
        assert_eq!(buf[(0, 0)].symbol(), "█");
        assert_eq!(buf[(0, 3)].symbol(), "█");
        assert_eq!(buf[(1, 0)].symbol(), " ");
        assert_eq!(buf[(1, 3)].symbol(), "█");
        assert_eq!(buf[(2, 3)].symbol(), " ");
    }

    #[test]
    fn waveforms_render_without_panicking_at_any_size() {
        let w = wave(1500);
        let cues = [cue(Some(0), 1000, None, "")];
        let beats = [Beat { beat_number: 1, bpm: 128.0, time_ms: 500 }];
        for (width, height) in [(1, 1), (4, 2), (80, 4), (200, 12)] {
            let area = Rect::new(2, 1, width, height);
            let mut buf = Buffer::empty(Rect::new(0, 0, width + 4, height + 2));
            Overview { wave: Some(&w), progress: 0.5, cues: &cues, duration_ms: 10_000.0 }.render(area, &mut buf);
            Overview { wave: None, progress: 1.0, cues: &cues, duration_ms: 0.0 }.render(area, &mut buf);
            Detail { wave: &w, position_secs: 0.2, visible_secs: 8.0, beats: &beats, cues: &cues }
                .render(area, &mut buf);
            Detail { wave: &w, position_secs: 9999.0, visible_secs: 8.0, beats: &beats, cues: &cues }
                .render(area, &mut buf);
            let px = vinyl(width, height, 1.0, [255, 0, 0]);
            Pixels { width, pixels: &px }.render(area, &mut buf);
        }
    }

    #[test]
    fn overview_marks_hot_cues_with_letters() {
        let w = wave(100);
        let cues = [cue(Some(1), 5000, None, "")];
        let area = Rect::new(0, 0, 20, 3);
        let mut buf = Buffer::empty(area);
        Overview { wave: Some(&w), progress: 0.0, cues: &cues, duration_ms: 10_000.0 }.render(area, &mut buf);
        assert_eq!(buf[(10, 0)].symbol(), "B");
        assert!(rendered(&buf).contains('█'));
    }

    /// A cue (see [`testing::cue`](super::super::app::testing::cue)) named `comment`.
    fn cue(hot: Option<u8>, start_ms: u32, end_ms: Option<u32>, comment: &str) -> Cue {
        Cue { comment: comment.into(), ..super::super::app::testing::cue(hot, start_ms, end_ms) }
    }

    #[test]
    fn cue_labels_name_loops_by_length() {
        assert_eq!(cue_label(&cue(Some(1), 70_764, Some(74_574), "loop in"), 126.0), "loop in ↻8");
        assert_eq!(cue_label(&cue(None, 0, Some(238), ""), 126.0), "↻1/2");
        assert_eq!(cue_label(&cue(None, 288, None, "1.1Bars"), 126.0), "1.1Bars");
        assert_eq!(cue_label(&cue(Some(0), 288, None, ""), 126.0), "");
        // The length rekordbox stored wins over the tempo (wrong where the grid changes speed).
        let stored = |beats| Cue { loop_beats: Some(beats), ..cue(None, 0, Some(1000), "") };
        assert_eq!(
            (cue_label(&stored((16, 1)), 126.0), cue_label(&stored((3, 2)), 126.0)),
            ("↻16".into(), "↻3/2".into())
        );
    }

    #[test]
    fn memory_cues_take_their_colour_from_the_table() {
        let red = Cue { color_id: 2, ..cue(None, 0, None, "") };
        assert_eq!(cue_color(&red), theme::track_color(TrackColor::Red));
        assert_eq!(cue_color(&cue(None, 0, None, "")), theme::MEMORY_CUE);
        let hot = Cue { color: Some([1, 2, 3]), color_id: 2, ..cue(Some(0), 0, None, "") };
        assert_eq!(cue_color(&hot), [1, 2, 3], "a hot cue's own colour first");
    }

    #[test]
    fn cue_lane_places_cues_around_the_playhead() {
        // 20 s across 40 columns, playhead at 10 s in column 20.
        let cues = [
            cue(Some(0), 2_000, None, "Drop"),
            cue(None, 15_000, None, ""),
            cue(None, 16_000, Some(18_000), ""),
            cue(Some(1), 30_000, None, "out of view"),
        ];
        let area = Rect::new(0, 0, 40, 2);
        let mut buf = Buffer::empty(area);
        CueLane { cues: &cues, position_secs: 10.0, visible_secs: 20.0, bpm: 120.0 }.render(area, &mut buf);
        assert_eq!(buf[(4, 0)].symbol(), "A");
        assert_eq!(buf[(20, 0)].symbol(), "┃");
        assert_eq!(buf[(30, 0)].symbol(), "◆");
        assert_eq!(buf[(32, 0)].symbol(), "◆");
        assert_eq!(buf[(34, 0)].symbol(), "━");
        assert_eq!(buf[(37, 0)].symbol(), "─");
        let labels: String = (0..40).map(|x| buf[(x, 1)].symbol().to_string()).collect();
        assert!(labels.starts_with("    Drop"), "{labels:?}");
        assert!(labels.contains("↻4"), "{labels:?}");
        assert!(!rendered(&buf).contains('B'));
    }

    #[test]
    fn deck_draws_cue_lines_and_shades_loops() {
        let w = wave(150 * 20);
        let cues = [cue(Some(2), 9_000, None, "Vox"), cue(None, 11_000, Some(13_000), "")];
        let area = Rect::new(0, 0, 40, 6);
        let mut buf = Buffer::empty(area);
        Detail { wave: &w, position_secs: 10.0, visible_secs: 8.0, beats: &[], cues: &cues }.render(area, &mut buf);
        // 0.2 s per column from 6 s: the hot cue at 9 s is column 15, the loop spans 25-35.
        assert_eq!(buf[(15, 0)].symbol(), "C");
        assert_eq!(buf[(16, 0)].symbol(), "V");
        let cue_fg = rgb3(theme::HOT_CUE);
        assert!((1..6).all(|y| buf[(15, y)].fg == cue_fg));
        let wash = rgb3(theme::scale(theme::LOOP, 0.22));
        assert!((1..6).all(|y| buf[(30, y)].bg == wash));
        assert_ne!(buf[(20, 3)].bg, wash);
        assert_eq!(buf[(25, 5)].symbol(), "▲");
    }

    /// A band that has fallen leaves its peak cap behind for a while, and a cue just passed
    /// tints the bars.
    #[test]
    fn spectrum_peaks_hang_above_the_bars() {
        let area = Rect::new(0, 0, 1, 4);
        let draw = |tint| {
            let mut buf = Buffer::empty(area);
            Spectrum { bands: &[0.25], peaks: &[(0.75, 0.0)], mirror: false, tint }.render(area, &mut buf);
            buf
        };
        let buf = draw(None);
        let column: Vec<&str> = (0..4).map(|y| buf[(0, y)].symbol()).collect();
        assert_eq!(column, ["▁", " ", " ", "█"], "the cap three quarters up, the bar a quarter");
        assert_ne!(draw(Some((theme::RED, 1.0)))[(0, 3)].fg, buf[(0, 3)].fg);
    }

    /// A cue just passed tints the scope too.
    #[test]
    fn scope_is_tinted_by_a_cue_just_passed() {
        let area = Rect::new(0, 0, 10, 4);
        let samples: Vec<f32> = (0..40).map(|i| (i as f32 / 3.0).sin()).collect();
        let draw = |tint| {
            let mut buf = Buffer::empty(area);
            Scope { samples: &samples, tint }.render(area, &mut buf);
            buf.content.iter().map(|c| c.fg).collect::<Vec<_>>()
        };
        assert_ne!(draw(None), draw(Some((theme::RED, 1.0))));
    }

    /// Where the front layer ends inside a cell, the cell shows it in front of the back layer.
    #[test]
    fn overview_layers_the_front_over_the_back() {
        let col = WaveColumn {
            back: WaveLayer { height: 1.0, color: [200, 50, 50] },
            front: WaveLayer { height: 0.3, color: [255, 255, 255] },
        };
        let w = Waveform { style: WaveformStyle::Rgb, columns_per_sec: None, columns: vec![col; 10] };
        let area = Rect::new(0, 0, 10, 3);
        let mut buf = Buffer::empty(area);
        Overview { wave: Some(&w), progress: 0.0, cues: &[], duration_ms: 10_000.0 }.render(area, &mut buf);
        // Two rows of waveform: the front fills just over half of the bottom one.
        let (bottom, top) = (&buf[(5, 2)], &buf[(5, 1)]);
        assert_eq!((bottom.symbol(), top.symbol()), ("▅", "█"));
        assert_ne!(bottom.bg, ratatui::style::Color::Reset, "the back layer behind it");
    }
}
