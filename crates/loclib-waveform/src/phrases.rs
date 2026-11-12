use ratatui::buffer::Buffer;
use ratatui::buffer::Cell;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::widgets::Widget;

use crate::{Palette, Rgb};

/// rekordbox's phrase analysis across the whole track, one row: each phrase a block of its
/// colour, dimmed once played, with its name where the name fits whole, and the playhead in white.
pub struct PhraseStrip<'a> {
    /// (start ms, end ms, name, colour), in order.
    pub phrases: &'a [(f64, f64, String, Rgb)],
    pub duration_ms: f64,
    /// 0-1.
    pub progress: f32,
    pub palette: Palette<'a>,
}

impl Palette<'_> {
    /// The playhead on a cell of a phrase: the cell turned its colour, or on a letter of a name,
    /// the letter in its colour and underlined, so the name still reads.
    pub fn head_on_a_phrase(&self, cell: &mut Cell) {
        let paper = (self.paint)(self.playhead);
        if cell.symbol() != " " {
            cell.modifier.insert(Modifier::UNDERLINED);
            if !self.flat {
                cell.set_fg(paper);
            }
            return;
        }
        if self.flat {
            cell.modifier.toggle(Modifier::REVERSED);
            cell.set_fg(paper);
        } else {
            cell.set_fg((self.paint)(self.ink_on(self.playhead))).set_bg(paper);
        }
    }
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
        for (start, end, name, color) in self.phrases {
            let (a, b) = (x_of(*start), x_of(*end).max(x_of(*start) + 1).min(area.width));
            for x in a..b {
                let Some(cell) = buf.cell_mut((area.x + x, y)) else { continue };
                // With colours that can't shade, reverse video reads whatever the scheme's are, and
                // what's played takes the dim attribute.
                if p.flat {
                    p.put_in(cell, " ", paint(*color), None);
                    cell.modifier.insert(Modifier::REVERSED);
                    if x < head {
                        cell.modifier.insert(Modifier::DIM);
                    }
                    continue;
                }
                let c = if x < head { p.fade(*color, 0.45) } else { *color };
                // A fainter first cell keeps neighbouring phrases of the same colour apart.
                let c = if x == a { p.fade(c, 0.7) } else { c };
                p.put_in(cell, " ", paint(p.ink), Some(paint(c)));
            }
            // "Verse 1", or "Verse" with no room for the number, or the colour alone: a cut name
            // reads as noise.
            let room = b.saturating_sub(a + 1) as usize;
            let word = name.split(' ').next().unwrap_or(name);
            let Some(shown) = [name.as_str(), word].into_iter().find(|n| n.chars().count() <= room) else { continue };
            let ink = paint(if p.flat { *color } else { p.ink_on(*color) });
            for (i, ch) in shown.chars().enumerate() {
                if let Some(cell) = buf.cell_mut((area.x + a + 1 + i as u16, y)) {
                    cell.set_char(ch).set_fg(ink);
                }
            }
        }
        if let Some(cell) = buf.cell_mut((area.x + head, y)) {
            p.head_on_a_phrase(cell);
        }
    }
}
