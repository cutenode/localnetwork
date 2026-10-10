use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::color::scale;
use crate::{Palette, Rgb};

/// rekordbox's phrase analysis across the whole track, one row: each phrase a block of its
/// colour with its name, dimmed once played, and the playhead in white.
pub struct PhraseStrip<'a> {
    /// (start ms, end ms, name, colour), in order.
    pub phrases: &'a [(f64, f64, String, Rgb)],
    pub duration_ms: f64,
    /// 0-1.
    pub progress: f32,
    pub palette: Palette,
}

impl Widget for PhraseStrip<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 4 || area.height == 0 || self.duration_ms <= 0.0 {
            return;
        }
        let (p, paint) = (&self.palette, self.palette.paint);
        let w = area.width as f64;
        let x_of = |ms: f64| ((ms / self.duration_ms) * w).round().clamp(0.0, w) as u16;
        let head = ((self.progress.clamp(0.0, 1.0) as f64 * w) as u16).min(area.width - 1);
        let y = area.y;
        let ink = paint([16, 16, 22]);
        for (start, end, name, color) in self.phrases {
            let (a, b) = (x_of(*start), x_of(*end).max(x_of(*start) + 1).min(area.width));
            for x in a..b {
                let c = if x < head { scale(*color, 0.45) } else { *color };
                // A darker first cell keeps neighbouring phrases of the same colour apart.
                let c = if x == a { scale(c, 0.7) } else { c };
                p.put(buf, area.x + x, y, " ", ink, Some(paint(c)));
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
            cell.set_symbol("┃").set_fg(paint([255, 255, 255]));
        }
    }
}
