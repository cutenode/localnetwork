use loclib_model::analysis::{Beat, Cue, WaveColumn, Waveform};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::color::scale;
use crate::cues::{cue_label, put_cue, put_labels};
use crate::{LOWER, Palette, Rgb};

/// The loudest waveform column in `[from, to)`, or the one at `from` when the range is empty.
fn column(w: &Waveform, from: f64, to: f64) -> Option<WaveColumn> {
    if from < 0.0 || from >= w.columns.len() as f64 {
        return None;
    }
    let a = from as usize;
    let b = (to.ceil() as usize).clamp(a + 1, w.columns.len());
    w.columns[a..b].iter().copied().max_by(|x, y| x.back.height.total_cmp(&y.back.height))
}

/// Whole-track overview: a row of cue markers above a bottom-anchored waveform; the part already
/// played is dimmed, with the playhead in white.
pub struct Overview<'a> {
    /// `None` (no analysis) draws a plain progress rail.
    pub wave: Option<&'a Waveform>,
    /// 0-1.
    pub progress: f32,
    pub cues: &'a [Cue],
    pub duration_ms: f64,
    pub palette: Palette<'a>,
}

impl Widget for Overview<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 2 || area.width < 4 {
            return;
        }
        let (p, paint) = (&self.palette, self.palette.paint);
        let marker_y = area.y;
        let wave_area = Rect { y: area.y + 1, height: area.height - 1, ..area };
        let w = area.width as f64;
        let head = ((self.progress.clamp(0.0, 1.0) as f64 * w) as u16).min(area.width - 1);

        if self.duration_ms > 0.0 {
            let x_of = |ms: u32| ((ms as f64 / self.duration_ms) * w) as u16;
            for c in self.cues {
                if let Some(end) = c.end_ms {
                    for x in x_of(c.start_ms)..=x_of(end).min(area.width - 1) {
                        p.put(buf, area.x + x, marker_y, "▁", paint(scale(p.cue_color(c), 0.8)), None);
                    }
                }
            }
            // Memory cues first, so a hot cue at the same spot stays visible.
            let mut cues: Vec<&Cue> = self.cues.iter().collect();
            cues.sort_by_key(|c| c.hot_cue.is_some());
            for c in cues {
                let x = x_of(c.start_ms);
                if x < area.width {
                    put_cue(p, buf, area.x + x, marker_y, c, x < head);
                }
            }
        }

        let Some(wave) = self.wave.filter(|w| !w.columns.is_empty()) else {
            // No analysis: a plain progress rail.
            let y = wave_area.y + wave_area.height / 2;
            for x in 0..area.width {
                let (sym, c) = if x < head { ("━", p.accent) } else { ("─", p.faint) };
                p.put(buf, area.x + x, y, sym, paint(c), None);
            }
            p.put(buf, area.x + head, y, "●", paint(p.fg), None);
            return;
        };

        let n = wave.columns.len() as f64;
        let h = wave_area.height as f32;
        for x in 0..area.width {
            let col = column(wave, x as f64 * n / w, (x + 1) as f64 * n / w).unwrap_or_default();
            let played = x < head;
            let shade = |c: Rgb| {
                if x == head {
                    p.playhead
                } else if played {
                    scale(c, 0.5)
                } else {
                    scale(c, 1.25)
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
                        p.put(buf, area.x + x, y, "│", paint(p.fg), None);
                    }
                    continue;
                }
                if front_fill == 8 {
                    p.put(buf, area.x + x, y, "█", paint(shade(col.front.color)), None);
                } else if front_fill > 0 && back_fill == 8 {
                    // Front layer fills the bottom of the cell, the back layer the rest.
                    p.put(
                        buf,
                        area.x + x,
                        y,
                        LOWER[front_fill],
                        paint(shade(col.front.color)),
                        Some(paint(shade(col.back.color))),
                    );
                } else {
                    p.put(buf, area.x + x, y, LOWER[back_fill], paint(shade(col.back.color)), None);
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
    /// The beat grid; the first beat's tempo gives loops without a stored length theirs.
    pub beats: &'a [Beat],
    pub cues: &'a [Cue],
    pub palette: Palette<'a>,
}

impl Widget for Detail<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 3 || area.width < 8 {
            return;
        }
        let (p, paint) = (&self.palette, self.palette.paint);
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
                let (sym, c) = if b.beat_number == 1 { ("╻", p.accent) } else { ("╷", p.dim) };
                p.put(buf, area.x + x as u16, tick_y, sym, paint(c), None);
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
            let wash = paint(scale(p.cue_color(c), 0.22));
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
            let color = if x == centre { p.playhead } else { col.back.color };
            let past = x < centre;
            let color = if past { scale(color, 0.7) } else { color };
            let a = col.back.height;
            // Upper half grows up from the centre line, in eighths.
            let up = a * up_rows as f32;
            for k in 0..up_rows {
                let fill = ((up - k as f32) * 8.0).round().clamp(0.0, 8.0) as usize;
                if fill > 0 {
                    p.put(buf, area.x + x, centre_y - 1 - k, LOWER[fill], paint(color), None);
                }
            }
            // Lower half: a dimmer reflection, in half cells.
            let down = a * down_rows as f32;
            let reflection = scale(color, 0.55);
            for k in 0..down_rows {
                let f = down - k as f32;
                let sym = if f >= 0.75 {
                    "█"
                } else if f >= 0.25 {
                    "▀"
                } else {
                    continue;
                };
                p.put(buf, area.x + x, centre_y + k, sym, paint(reflection), None);
            }
            if x == centre && a == 0.0 {
                p.put(buf, area.x + x, centre_y, "│", paint(p.fg), None);
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
            let color = p.cue_color(c);
            for y in wave_area.y..wave_area.bottom() {
                if let Some(cell) = buf.cell_mut((area.x + x, y)) {
                    if cell.symbol() == " " {
                        cell.set_symbol("│");
                    }
                    cell.set_fg(paint(color));
                }
            }
            if c.hot_cue.is_none() {
                p.put(buf, area.x + x, wave_area.bottom() - 1, "▲", paint(color), None);
            }
            put_cue(p, buf, area.x + x, tick_y, c, false);
            labels.push((x + 1, cue_label(c, self.beats.first().map_or(0.0, |b| b.bpm)), color));
        }
        // Loop ends get a thinner line.
        for c in self.cues {
            if let Some(end) = c.end_ms {
                let x = x_of(end as f64 / 1000.0);
                if in_view(x) {
                    p.put(buf, area.x + x as u16, tick_y, "╷", paint(p.cue_color(c)), None);
                }
            }
        }
        put_labels(p, buf, area, tick_y, labels);
        p.put(buf, area.x + centre, tick_y, "▼", paint(p.fg), None);
    }
}
