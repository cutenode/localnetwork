use loclib_model::analysis::Cue;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Widget;

use crate::color::{mix, scale};
use crate::{Palette, Rgb};

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
pub(crate) fn put_cue(p: &Palette, buf: &mut Buffer, x: u16, y: u16, c: &Cue, dim: bool) {
    let (color, paint) = (p.cue_color(c), p.paint);
    let color = if dim { scale(color, 0.55) } else { color };
    match cue_letter(c) {
        Some(letter) => p.put(buf, x, y, &letter.to_string(), paint([12, 12, 16]), Some(paint(color))),
        None => p.put(buf, x, y, "▼", paint(color), None),
    }
}

/// Write cue names along row `y`, each starting at its marker and cut short before the next one.
/// `marks` are (column, label, colour), in any order.
pub(crate) fn put_labels(p: &Palette, buf: &mut Buffer, area: Rect, y: u16, mut marks: Vec<(u16, String, Rgb)>) {
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
        let (end, _) = buf.set_stringn(area.x + x, y, label, room, Style::new().fg((p.paint)(*color)));
        free = end - area.x + 1;
    }
}

/// The stretch of track around the playhead with its cues: hot cues as lettered chips, memory cues
/// as diamonds, loops as bars over their length, and (given two rows) their names underneath.
/// Made to go under a spectrum or scope.
pub struct CueLane<'a> {
    pub cues: &'a [Cue],
    pub position_secs: f64,
    pub visible_secs: f64,
    /// For a loop's length in beats, when rekordbox didn't store it.
    pub bpm: f64,
    pub palette: Palette,
}

impl Widget for CueLane<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 8 || area.height == 0 {
            return;
        }
        let (p, paint) = (&self.palette, self.palette.paint);
        let w = area.width as f64;
        let start = self.position_secs - self.visible_secs / 2.0;
        let secs_per_col = self.visible_secs / w;
        let centre = area.width / 2;
        let x_of = |ms: u32| ((ms as f64 / 1000.0 - start) / secs_per_col).floor();
        let y = area.y;
        let past = |x: u16| x < centre;
        let shade = |c: Rgb, x: u16| if past(x) { scale(c, 0.5) } else { c };

        for x in 0..area.width {
            let c = if past(x) { p.faint } else { mix(p.faint, p.dim, 0.5) };
            p.put(buf, area.x + x, y, "─", paint(c), None);
        }
        for c in self.cues {
            let Some(end) = c.end_ms else { continue };
            let (a, b) = (x_of(c.start_ms), x_of(end));
            if b < 0.0 || a >= w {
                continue;
            }
            for x in a.max(0.0) as u16..=(b.min(w - 1.0) as u16) {
                p.put(buf, area.x + x, y, "━", paint(shade(p.cue_color(c), x)), None);
            }
        }
        p.put(buf, area.x + centre, y, "┃", paint(p.fg), None);
        let mut cues: Vec<&Cue> = self.cues.iter().collect();
        cues.sort_by_key(|c| c.hot_cue.is_some());
        let mut labels = Vec::new();
        for c in cues {
            let x = x_of(c.start_ms);
            if !(0.0..w).contains(&x) {
                continue;
            }
            let x = x as u16;
            let color = shade(p.cue_color(c), x);
            match cue_letter(c) {
                Some(l) => p.put(buf, area.x + x, y, &l.to_string(), paint([12, 12, 16]), Some(paint(color))),
                None => p.put(buf, area.x + x, y, "◆", paint(color), None),
            }
            labels.push((x, cue_label(c, self.bpm), color));
        }
        if area.height >= 2 {
            put_labels(p, buf, area, y + 1, labels);
        }
    }
}
