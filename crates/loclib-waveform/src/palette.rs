use loclib_model::TrackColor;
use loclib_model::analysis::Cue;
use loclib_model::theme::{self, Theme};
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

/// An 8-bit sRGB colour, as rekordbox stores them.
pub use loclib_model::tokens::Rgb;

/// The colours the widgets draw in, a theme's ([`Palette::of`]), and how a colour becomes what
/// the terminal draws. Every widget takes one; `Palette::default()` is [`Palette::DARK`].
#[derive(Debug, Clone, Copy)]
pub struct Palette<'a> {
    /// The playhead.
    pub fg: Rgb,
    /// Beat ticks, and (mixed with `faint`) the cue lane's rail ahead of the playhead.
    pub dim: Rgb,
    /// Rails, and the scope's centre line.
    pub faint: Rgb,
    /// The played part of the progress rail, and bar starts among the beat ticks.
    pub accent: Rgb,
    /// The spectrum's bars by height, bottom to top: two stops or more.
    pub spectrum: &'a [Rgb],
    /// The scope's trace by loudness, quiet to loud: two stops or more.
    pub scope: &'a [Rgb],
    /// A hot cue without a colour of its own.
    pub hot_cue: Rgb,
    /// A memory cue without a colour of its own.
    pub memory_cue: Rgb,
    /// A loop without a colour of its own.
    pub loop_cue: Rgb,
    /// The colours a memory cue's or loop's `color_id` names (rekordbox's track colours), pink
    /// to purple.
    pub track_colors: [Rgb; 8],
    /// The playhead's line.
    pub playhead: Rgb,
    /// How a colour is drawn: where a light terminal's version of it, or the nearest of 256
    /// colours, comes in.
    pub paint: fn(Rgb) -> Color,
    /// How a picture's colour is drawn (cover art, the record): as it is, whatever the background.
    pub image: fn(Rgb) -> Color,
    /// No colours (NO_COLOR): coloured cells (a cue's letter, a loop) show in reverse video, and
    /// pictures aren't drawn.
    pub plain: bool,
}

impl Default for Palette<'_> {
    fn default() -> Self {
        Palette::DARK
    }
}

impl Palette<'static> {
    /// [`Theme::DEFAULT`]'s palette for a dark terminal in 24-bit colour.
    pub const DARK: Palette<'static> = {
        let t = Theme::DEFAULT;
        Palette {
            fg: t.text,
            dim: t.dim,
            faint: t.faint,
            accent: t.accent,
            spectrum: theme::SPECTRUM,
            scope: theme::SCOPE,
            hot_cue: t.hot_cue,
            memory_cue: t.memory_cue,
            loop_cue: t.loop_cue,
            track_colors: t.track_colors,
            playhead: t.playhead,
            paint: |[r, g, b]| Color::Rgb(r, g, b),
            image: |[r, g, b]| Color::Rgb(r, g, b),
            plain: false,
        }
    };
}

impl<'a> Palette<'a> {
    /// `theme`'s colours, drawn as they are in 24-bit colour (as [`Palette::DARK`] is).
    pub fn of(theme: &'a Theme) -> Palette<'a> {
        Palette {
            fg: theme.text,
            dim: theme.dim,
            faint: theme.faint,
            accent: theme.accent,
            spectrum: &theme.spectrum,
            scope: &theme.scope,
            hot_cue: theme.hot_cue,
            memory_cue: theme.memory_cue,
            loop_cue: theme.loop_cue,
            track_colors: theme.track_colors,
            playhead: theme.playhead,
            ..Palette::DARK
        }
    }

    /// The colour rekordbox's track colour `c` is drawn in.
    pub fn track_color(&self, c: TrackColor) -> Rgb {
        self.track_colors[usize::from(c.id() - 1)]
    }

    /// The colour a cue is shown in: its own (a hot cue's, or a memory cue's from rekordbox's
    /// colour table), else the palette's for its kind.
    pub fn cue_color(&self, c: &Cue) -> Rgb {
        match (c.color.or_else(|| TrackColor::from_id(c.color_id).map(|t| self.track_color(t))), c.hot_cue, c.end_ms) {
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
