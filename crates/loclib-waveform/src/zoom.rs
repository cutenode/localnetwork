//! The zoomed waveform: the track scrolling past a playhead fixed in the middle. Columns are laid
//! out from the track's start, so one stays the same as it scrolls and is only worked out once:
//! [`ZoomRing`] keeps them, and a frame copies what's in view and adds what's newly come in. What a
//! frame costs doesn't grow with the length of the track or the zoom: however many of the
//! waveform's columns fall in one of the screen's, fewer of those come in a second.

use loclib_model::analysis::{Beat, Cue};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::color::scale;
use crate::cues::{cue_label, put_cue};
use crate::wave::fill_of;
use crate::waveform::layer_heights;
use crate::{LOWER, Palette, Rgb, Wave, place_markers};

/// How close together beats can be, in columns, and each still get a tick.
const TICK_ROOM: f64 = 3.0;

/// How much brighter a waveform's colours are here than on the overview: this is the one read.
const LIFT: f32 = 1.12;

/// What a [`ZoomRing`] draws.
pub struct Zoom<'a> {
    pub wave: Wave<'a>,
    pub position_secs: f64,
    pub visible_secs: f64,
    pub beats: &'a [Beat],
    pub cues: &'a [Cue],
    /// Rows of cue markers under the ruler, for when they collide. At 0 they're on the ruler.
    pub cue_rows: u16,
    /// Number the bars along the ruler.
    pub bar_numbers: bool,
    pub palette: Palette<'a>,
}

/// What every kept column depends on, but for what [`ZoomRing::forget`] is for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shape {
    width: u16,
    rows: u16,
    cue_rows: u16,
    secs_per_col: f64,
    bar_numbers: bool,
}

/// A cue's marker, with its name and colour worked out once for all the frames that show it.
#[derive(Debug)]
struct Mark {
    col: i64,
    row: u16,
    cue: usize,
    label: String,
    colour: Rgb,
}

/// The columns kept, in a ring: a column's cells are at its number modulo the capacity.
#[derive(Debug, Default)]
pub struct ZoomRing {
    shape: Option<Shape>,
    /// The ruler and the waveform of each column, one column after another.
    cells: Vec<Cell>,
    capacity: i64,
    /// The columns held, in track order.
    from: i64,
    to: i64,
    /// Bar starts, in seconds.
    bars: Vec<f64>,
    /// Every how many bars there's a number.
    numbered: usize,
    marks: Vec<Mark>,
}

impl ZoomRing {
    /// What's kept was drawn from something else now: another track, theme or cues.
    pub fn forget(&mut self) {
        self.shape = None;
    }

    fn reset(&mut self, z: &Zoom<'_>, shape: Shape) {
        self.shape = Some(shape);
        self.capacity = i64::from(shape.width) * 2;
        self.cells.clear();
        self.cells.resize(self.capacity as usize * usize::from(shape.rows), Cell::EMPTY);
        (self.from, self.to) = (0, 0);
        self.bars.clear();
        self.bars.extend(z.beats.iter().filter(|b| b.beat_number == 1).map(|b| f64::from(b.time_ms) / 1000.0));
        // Room for a number and a space after a tick, between each one numbered.
        let digits = f64::from(self.bars.len().max(1).ilog10()) + 3.0;
        let gap = self.bars.windows(2).next().map_or(0.0, |w| w[1] - w[0]) / shape.secs_per_col;
        self.numbered =
            [1, 2, 4, 8, 16, 32, 64].into_iter().find(|n| f64::from(*n) * gap >= digits).unwrap_or(128) as usize;

        let p = &z.palette;
        let col = |ms: u32| (f64::from(ms) / 1000.0 / shape.secs_per_col).floor() as i64;
        let bpm = z.beats.first().map_or(0.0, |b| b.bpm);
        let placed = place_markers(z.cues, shape.cue_rows.max(1), i64::MAX, |c| Some(col(c.start_ms)));
        self.marks.clear();
        self.marks.extend(placed.iter().map(|m| Mark {
            col: m.x,
            row: m.row,
            cue: z.cues.iter().position(|c| std::ptr::eq(c, m.cue)).unwrap_or(0),
            label: cue_label(m.cue, bpm),
            colour: p.cue_color(m.cue),
        }));
        self.marks.sort_by_key(|m| (m.row, m.col));
    }

    /// Have columns `left..left + width` kept.
    fn fill(&mut self, z: &Zoom<'_>, left: i64, width: i64) {
        if left >= self.to || left + width <= self.from {
            (self.from, self.to) = (left, left);
        }
        while self.from > left {
            self.from -= 1;
            self.draw_column(z, self.from);
            self.to = self.to.min(self.from + self.capacity);
        }
        while self.to < left + width {
            self.draw_column(z, self.to);
            self.to += 1;
            self.from = self.from.max(self.to - self.capacity);
        }
    }

    fn draw_column(&mut self, z: &Zoom<'_>, column: i64) {
        let Some(shape) = self.shape else { return };
        let (p, paint) = (&z.palette, z.palette.paint);
        let rows = usize::from(shape.rows);
        let at = column.rem_euclid(self.capacity) as usize * rows;
        let s = shape.secs_per_col;
        let (t0, t1) = (column as f64 * s, (column + 1) as f64 * s);
        let col_of = |ms: u32| (f64::from(ms) / 1000.0 / s).floor() as i64;
        let (bars, numbered) = (&self.bars, self.numbered);
        let cells = &mut self.cells[at..at + rows];
        cells.fill(Cell::EMPTY);
        let (ruler, wave_cells) = cells.split_at_mut(1);
        let ruler = &mut ruler[0];

        // The ruler: the bar or beat in this column, or a number after a bar's tick.
        let first = z.beats.partition_point(|b| f64::from(b.time_ms) / 1000.0 < t0);
        let in_column = z.beats[first..].iter().take_while(|b| f64::from(b.time_ms) / 1000.0 < t1);
        let beat_room = z.beats.first().map_or(0.0, |b| 60.0 / b.bpm.max(1.0)) / s;
        let bar = in_column.clone().find(|b| b.beat_number == 1);
        if bar.is_some() {
            p.put_in(ruler, "╻", paint(p.accent), None);
        } else if in_column.clone().next().is_some() && beat_room >= TICK_ROOM {
            p.put_in(ruler, "╷", paint(p.dim), None);
        } else if shape.bar_numbers {
            // The last bar to start, and how many columns after its tick this one is.
            let index = bars.partition_point(|at| *at < t0).saturating_sub(1);
            let after = bars.get(index).map_or(0, |at| column - (at / s).floor() as i64);
            let number = index + 1;
            if number.is_multiple_of(numbered) {
                let text = number.to_string();
                let digit = usize::try_from(after - 1).ok().and_then(|i| text.as_bytes().get(i));
                if let Some(d) = digit {
                    ruler.set_char(char::from(*d)).set_fg(paint(p.dim));
                }
            }
        }

        // Loops: a tint behind the waveform, unless colours can't be trusted to shade.
        let tint = z.cues.iter().filter(|_| !p.flat).find_map(|c| {
            let end = c.end_ms?;
            (col_of(c.start_ms)..=col_of(end)).contains(&column).then(|| paint(p.fade(p.cue_color(c), 0.22)))
        });
        if let Some(tint) = tint {
            for cell in wave_cells.iter_mut() {
                cell.set_bg(tint);
            }
        }

        // The waveform, mirrored about the line between two rows.
        let wave_rows = shape.rows - 1;
        let up_rows = wave_rows / 2;
        let down_rows = wave_rows - up_rows;
        let rate = z.wave.columns_per_sec().unwrap_or(150.0);
        if let Some(stack) = z.wave.stack(t0 * rate, t1 * rate) {
            let colors = stack.map(|l| paint(scale(p.wave_colour(l.color), LIFT)));
            let (up, down) = wave_cells.split_at_mut(usize::from(up_rows));
            let heights = layer_heights(p, &stack, f32::from(up_rows));
            for (k, cell) in up.iter_mut().rev().enumerate() {
                p.put_rising(cell, fill_of(heights, k as u16), &colors);
            }
            let heights = layer_heights(p, &stack, f32::from(down_rows));
            for (k, cell) in down.iter_mut().enumerate() {
                p.put_falling(cell, fill_of(heights, k as u16), &colors);
            }
        }

        // Lines through the waveform: a hot cue's over a memory cue's, which is marked underneath
        // too. The end of a loop is a thinner line on the ruler.
        let mut lines: [Option<&Cue>; 2] = [None, None];
        for c in z.cues {
            if col_of(c.start_ms) == column {
                lines[usize::from(c.hot_cue.is_some())] = Some(c);
            }
            if c.end_ms.is_some_and(|end| col_of(end) == column) {
                p.put_in(ruler, "╷", paint(p.cue_color(c)), None);
            }
        }
        for c in lines.into_iter().flatten() {
            let colour = paint(p.cue_color(c));
            for cell in wave_cells.iter_mut() {
                // Over a full cell the line keeps the waveform's colour either side of it.
                if cell.symbol() == LOWER[8] {
                    let under = cell.fg;
                    cell.set_bg(under);
                }
                cell.set_symbol("┃").set_fg(colour);
                cell.modifier.remove(Modifier::REVERSED);
            }
            if c.hot_cue.is_none() {
                wave_cells[usize::from(wave_rows) - 1].set_symbol("▲");
            }
        }
    }

    /// Draw at `area`, from the top: the ruler, the cue rows and the waveform.
    pub fn draw(&mut self, z: &Zoom<'_>, area: Rect, buf: &mut Buffer) {
        if area.height < 3 || area.width < 8 {
            return;
        }
        let cue_rows = z.cue_rows.min(area.height - 3);
        let rows = area.height - cue_rows;
        let secs_per_col = z.visible_secs / f64::from(area.width);
        let shape = Shape { width: area.width, rows, cue_rows, secs_per_col, bar_numbers: z.bar_numbers };
        if self.shape != Some(shape) {
            self.reset(z, shape);
        }
        let (width, centre) = (i64::from(area.width), area.width / 2);
        let head = (z.position_secs / secs_per_col).floor() as i64;
        let left = head - i64::from(centre);
        self.fill(z, left, width);

        for y in 0..rows {
            let to_y = area.y + if y == 0 { 0 } else { y + cue_rows };
            for x in 0..area.width {
                let column = (left + i64::from(x)).rem_euclid(self.capacity) as usize;
                if let Some(cell) = buf.cell_mut((area.x + x, to_y)) {
                    *cell = self.cells[column * usize::from(rows) + usize::from(y)].clone();
                }
            }
        }
        self.draw_marks(z, area, left, cue_rows, buf);

        // The playhead, last: nothing's drawn over it.
        let (p, paint) = (&z.palette, z.palette.paint);
        for y in area.y + 1 + cue_rows..area.bottom() {
            let Some(cell) = buf.cell_mut((area.x + centre, y)) else { continue };
            if cell.symbol() == " " {
                cell.set_symbol("│").set_fg(paint(p.fg));
            } else if p.plain {
                // One colour for everything: a line cut out of the waveform.
                cell.set_symbol("│").modifier.toggle(Modifier::REVERSED);
            } else {
                cell.set_fg(paint(p.playhead));
            }
            if cell.bg != Color::Reset {
                cell.set_bg(paint(p.playhead));
            }
        }
        p.put(buf, area.x + centre, area.y, "▼", paint(p.fg), None);
    }

    /// The cues in view: each marker, then its name up to the next marker of the row.
    fn draw_marks(&self, z: &Zoom<'_>, area: Rect, left: i64, cue_rows: u16, buf: &mut Buffer) {
        let p = &z.palette;
        let width = i64::from(area.width);
        let in_view = |m: &Mark| (0..width).contains(&(m.col - left));
        let top = area.y + u16::from(cue_rows > 0);
        for (i, m) in self.marks.iter().enumerate().filter(|(_, m)| in_view(m)) {
            let x = (m.col - left) as u16;
            let y = top + m.row.min(cue_rows.saturating_sub(1));
            put_cue(p, buf, area.x + x, y, &z.cues[m.cue], false);
            // A cell after the marker and one before the next: a name that won't fit between them
            // isn't drawn cut. One running off the screen is, as it scrolls in.
            let next = self.marks[i + 1..].iter().find(|n| n.row == m.row && n.col > m.col).filter(|n| in_view(n));
            let room = (next.map_or(width, |n| n.col - left - 1) - i64::from(x) - 2).max(0);
            if !m.label.is_empty() && (next.is_none() || m.label.chars().count() as i64 <= room) {
                let style = Style::new().fg((p.paint)(m.colour));
                buf.set_stringn(area.x + x + 2, y, &m.label, room as usize, style);
            }
        }
    }
}
