use loclib_model::analysis::{Beat, Cue, WaveLayer, Waveform};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::widgets::Widget;

use crate::cues::{place_markers, put_cue};
use crate::wave::{Stack, fill_of};
use crate::{Marker, Palette, Wave, Zoom, ZoomRing};

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
        let Overview { wave, progress, cues, duration_ms, palette } = self;
        WaveOverview { wave: wave.map(Wave::Layers), progress, cues, duration_ms, cue_rows: 1, palette }
            .render(area, buf);
    }
}

/// [`Overview`] of any [`Wave`], with a second row of cue markers where they collide if `cue_rows`
/// allows it.
pub struct WaveOverview<'a> {
    pub wave: Option<Wave<'a>>,
    /// 0-1.
    pub progress: f32,
    pub cues: &'a [Cue],
    pub duration_ms: f64,
    pub cue_rows: u16,
    pub palette: Palette<'a>,
}

impl WaveOverview<'_> {
    fn markers(&self, area: Rect) -> Vec<Marker<'_>> {
        let w = f64::from(area.width);
        let rows = self.cue_rows.clamp(1, area.height.saturating_sub(1).max(1));
        place_markers(self.cues, rows, i64::from(area.width), |c| self.x_of(c.start_ms, w))
    }

    fn x_of(&self, ms: u32, w: f64) -> Option<i64> {
        (self.duration_ms > 0.0).then(|| ((f64::from(ms) / self.duration_ms) * w) as i64)
    }

    /// The rows `area` gives to cue markers, over the waveform.
    pub fn marker_rows(&self, area: Rect) -> u16 {
        self.markers(area).iter().map(|m| m.row + 1).max().unwrap_or(1)
    }
}

impl Widget for WaveOverview<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 2 || area.width < 4 {
            return;
        }
        let (p, paint) = (&self.palette, self.palette.paint);
        let w = area.width as f64;
        let head = ((self.progress.clamp(0.0, 1.0) as f64 * w) as u16).min(area.width - 1);
        let x_of = |ms: u32| self.x_of(ms, w);
        let markers = self.markers(area);
        let cue_rows = self.marker_rows(area);
        let wave_area = Rect { y: area.y + cue_rows, height: area.height - cue_rows, ..area };

        for c in self.cues {
            let (Some(end), Some(from)) = (c.end_ms, x_of(c.start_ms)) else { continue };
            let to = x_of(end).unwrap_or(from).min(i64::from(area.width) - 1);
            let tint = paint(p.fade(p.cue_color(c), 0.22));
            for x in from..=to {
                let x = area.x + x as u16;
                p.put(buf, x, area.y, "▁", paint(p.fade(p.cue_color(c), 0.8)), None);
                for y in wave_area.y..wave_area.bottom() {
                    if let Some(cell) = buf.cell_mut((x, y)).filter(|_| !p.flat) {
                        cell.set_bg(tint);
                    }
                }
            }
        }
        for m in &markers {
            put_cue(p, buf, area.x + m.x as u16, area.y + m.row, m.cue, m.x < i64::from(head));
        }

        let Some(wave) = self.wave.filter(|w| !w.is_empty()) else {
            // No analysis: a plain progress rail.
            let y = wave_area.y + wave_area.height / 2;
            for x in 0..area.width {
                let (sym, c) = if x < head { ("━", p.accent) } else { ("─", p.faint) };
                p.put(buf, area.x + x, y, sym, paint(c), None);
            }
            p.put(buf, area.x + head, y, "●", paint(p.fg), None);
            return;
        };

        let n = wave.len() as f64;
        let h = wave_area.height as f32;
        for x in 0..area.width {
            let stack = wave.stack(x as f64 * n / w, (x + 1) as f64 * n / w).unwrap_or_default();
            let played = x < head;
            // Once a column, not once a cell: every row of it is the same colours.
            let shade = |l: &WaveLayer| {
                paint(if x == head {
                    p.playhead
                } else if played {
                    p.fade(p.wave_colour(l.color), 0.5)
                } else {
                    p.wave_colour(l.color)
                })
            };
            let colors = [shade(&stack[0]), shade(&stack[1]), shade(&stack[2])];
            let heights = layer_heights(p, &stack, h);
            for row in 0..wave_area.height {
                let y = wave_area.bottom() - 1 - row;
                let Some(cell) = buf.cell_mut((area.x + x, y)) else { continue };
                let f = fill_of(heights, row);
                if f.fill == 0 {
                    if x == head {
                        cell.set_symbol("│").set_fg(paint(p.fg));
                    }
                    continue;
                }
                p.put_rising(cell, f, &colors);
                if played && p.flat {
                    cell.modifier.insert(Modifier::DIM);
                }
            }
        }
    }
}

/// A column's layers as heights in cells. Without colours there's one shape to see.
pub(crate) fn layer_heights(p: &Palette<'_>, stack: &Stack, cells: f32) -> [f32; 3] {
    let heights = stack.map(|l| l.height * cells);
    match p.plain {
        true => [heights.into_iter().fold(0.0, f32::max), 0.0, 0.0],
        false => heights,
    }
}

/// Scrolling detail waveform centred on the playhead, mirrored, with beat ticks above it
/// (bar starts brighter) and cue markers. Every column is worked out each time: [`ZoomRing`] keeps
/// them for what's drawn many times a second.
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
        let Detail { wave, position_secs, visible_secs, beats, cues, palette } = self;
        let wave = Wave::Layers(wave);
        let zoom = Zoom { wave, position_secs, visible_secs, beats, cues, cue_rows: 0, bar_numbers: false, palette };
        ZoomRing::default().draw(&zoom, area, buf);
    }
}
