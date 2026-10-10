use loclib_model::TrackColor;
use loclib_model::analysis::Cue;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

/// An 8-bit sRGB colour, as rekordbox stores them.
pub type Rgb = [u8; 3];

/// The colours the widgets draw in, and how a colour becomes what the terminal draws. Every
/// widget takes one; `Palette::default()` is [`Palette::DARK`].
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// The playhead.
    pub fg: Rgb,
    /// Beat ticks, and (mixed with `faint`) the cue lane's rail ahead of the playhead.
    pub dim: Rgb,
    /// Rails, and the scope's centre line.
    pub faint: Rgb,
    /// The played part of the progress rail, and bar starts among the beat ticks.
    pub accent: Rgb,
    /// The spectrum's bars by height, bottom to top: two stops or more.
    pub spectrum: &'static [Rgb],
    /// The scope's trace by loudness, quiet to loud: two stops or more.
    pub scope: &'static [Rgb],
    /// A hot cue without a colour of its own.
    pub hot_cue: Rgb,
    /// A memory cue without a colour of its own.
    pub memory_cue: Rgb,
    /// A loop without a colour of its own.
    pub loop_cue: Rgb,
    /// The colour a memory cue's or loop's `color_id` names (rekordbox's track colours).
    pub track_color: fn(TrackColor) -> Rgb,
    /// How a colour is drawn: where a light terminal's version of it, or the nearest of 256
    /// colours, comes in.
    pub paint: fn(Rgb) -> Color,
    /// How a picture's colour is drawn (cover art, the record): as it is, whatever the background.
    pub image: fn(Rgb) -> Color,
    /// No colours (NO_COLOR): coloured cells (a cue's letter, a loop) show in reverse video, and
    /// pictures aren't drawn.
    pub plain: bool,
}

impl Default for Palette {
    fn default() -> Self {
        Palette::DARK
    }
}

impl Palette {
    /// A palette for a dark terminal in 24-bit colour.
    pub const DARK: Palette = Palette {
        fg: [226, 226, 236],
        dim: [120, 120, 142],
        faint: [72, 72, 92],
        accent: [255, 92, 160],
        spectrum: &[[64, 140, 255], [158, 112, 255], [255, 92, 160], [255, 196, 86]],
        scope: &[[92, 214, 255], [158, 112, 255], [255, 92, 160]],
        hot_cue: [40, 226, 20],
        memory_cue: [232, 48, 48],
        loop_cue: [255, 140, 30],
        track_color: |c| match c {
            TrackColor::Pink => [248, 82, 201],
            TrackColor::Red => [232, 48, 48],
            TrackColor::Orange => [255, 140, 30],
            TrackColor::Yellow => [240, 220, 40],
            TrackColor::Green => [60, 200, 80],
            TrackColor::Aqua => [40, 200, 220],
            TrackColor::Blue => [48, 96, 240],
            TrackColor::Purple => [150, 72, 230],
        },
        paint: |[r, g, b]| Color::Rgb(r, g, b),
        image: |[r, g, b]| Color::Rgb(r, g, b),
        plain: false,
    };

    /// The colour a cue is shown in: its own (a hot cue's, or a memory cue's from rekordbox's
    /// colour table), else the palette's for its kind.
    pub fn cue_color(&self, c: &Cue) -> Rgb {
        match (c.color.or_else(|| TrackColor::from_id(c.color_id).map(self.track_color)), c.hot_cue, c.end_ms) {
            (Some(rgb), _, _) => rgb,
            (None, _, Some(_)) => self.loop_cue,
            (None, Some(_), None) => self.hot_cue,
            (None, None, None) => self.memory_cue,
        }
    }

    pub(crate) fn put(&self, buf: &mut Buffer, x: u16, y: u16, symbol: &str, fg: Color, bg: Option<Color>) {
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_symbol(symbol).set_fg(fg);
            if let Some(bg) = bg {
                cell.set_bg(bg);
                // Without colours, a coloured cell (a cue's letter, a loop) shows in reverse video.
                if self.plain {
                    cell.modifier.insert(Modifier::REVERSED);
                }
            }
        }
    }
}
