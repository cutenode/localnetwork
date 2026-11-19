//! Cover art, none of it on the UI thread: the playing track's picture is made for the size it's
//! shown at, on a worker, and a frame only draws what's come back.
//!
//! The picture goes out as the terminal's own graphics where it has them, chosen once from what's
//! known of the terminal by then: nothing's asked of it for this, so it costs the player's start
//! nothing. Everywhere else, half blocks are the floor.

use image::imageops::FilterType;
use image::{DynamicImage, RgbImage, Rgba, RgbaImage};
use loclib_waveform::{Art, PixelBuf};
use ratatui::crossterm::terminal::window_size;
use ratatui::layout::Size;
use ratatui_image::Resize;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;

use super::app::{App, Msg};
use super::theme;

/// What a cover is drawn as.
pub enum Picture {
    /// Half blocks.
    Pixels(PixelBuf),
    /// The terminal's own graphics.
    Graphics(Box<Protocol>),
}

/// A cover made for a size in cells.
pub struct Cover {
    pub track: u64,
    pub size: (u16, u16),
    pub picture: Picture,
}

/// What's been read of cover art.
#[derive(Default)]
pub struct Covers {
    /// The cover the deck has.
    pub cover: Option<Cover>,
    /// The cover asked for last, so it's only asked for once.
    asked: Option<(u64, (u16, u16))>,
    /// How pictures are made, worked out the first time one is.
    picker: Option<Option<Picker>>,
}

impl Covers {
    /// Work out how pictures are made again, and make the cover again with it.
    pub fn forget_picker(&mut self) {
        (self.picker, self.cover, self.asked) = (None, None, None);
    }
}

/// How pictures reach this terminal, from what's known of it: `sixel` is whether it listed them in
/// its answer to the player's questions. Inside a multiplexer, half blocks: graphics would have to
/// be wrapped for it, and it would have to pass them on. `halfblocks` is the player's setting for a
/// terminal guessed wrong: half blocks whatever it says, unless `LOCALLIBRARY_ART` names graphics.
pub fn protocol(var: impl Fn(&str) -> Option<String>, sixel: bool, halfblocks: bool) -> Option<ProtocolType> {
    let named = var("LOCALLIBRARY_ART");
    let named = named.as_deref().map(str::trim);
    match named {
        Some("kitty") => return Some(ProtocolType::Kitty),
        Some("iterm2") => return Some(ProtocolType::Iterm2),
        Some("sixel") => return Some(ProtocolType::Sixel),
        Some("halfblocks") => return None,
        _ => {}
    }
    if halfblocks || var("TMUX").is_some() || var("STY").is_some() || var("ZELLIJ").is_some() {
        return None;
    }
    let program = var("TERM_PROGRAM");
    match program.as_deref() {
        Some("ghostty") => return Some(ProtocolType::Kitty),
        Some("iTerm.app" | "WezTerm") => return Some(ProtocolType::Iterm2),
        _ => {}
    }
    if var("KITTY_WINDOW_ID").is_some() || var("TERM").as_deref() == Some("xterm-kitty") {
        return Some(ProtocolType::Kitty);
    }
    sixel.then_some(ProtocolType::Sixel)
}

/// What makes pictures for this terminal, if it can show them: one that doesn't say how big its
/// cells are can't be given one the right size.
fn picker(sixel: bool, halfblocks: bool) -> Option<Picker> {
    let protocol = protocol(|k| std::env::var(k).ok(), sixel, halfblocks)?;
    let size = window_size().ok()?;
    let cell = (size.width.checked_div(size.columns)?, size.height.checked_div(size.rows)?);
    if cell.0 == 0 || cell.1 == 0 {
        return None;
    }
    // Its way of being told the size is deprecated for asking the terminal, which reads its answers
    // off stdin: here that's where the keys are. The version's pinned.
    #[expect(deprecated)]
    let mut picker = Picker::from_fontsize(cell.into());
    picker.set_protocol_type(protocol);
    Some(picker)
}

/// `art` as drawn at `size`.
fn picture(art: &Art, size: (u16, u16), picker: Option<&Picker>) -> Picture {
    let (cols, rows) = size;
    let graphics = picker.and_then(|p| {
        let font = p.font_size();
        let (image, resize) = fitted(art.image(), (font.width, font.height), size);
        p.new_protocol(image, Size::new(cols, rows), resize).ok()
    });
    graphics.map_or_else(|| Picture::Pixels(art.resized(cols, rows)), |p| Picture::Graphics(Box::new(p)))
}

/// `image` made ready to fill `size` cells of `font` pixels, and how ratatui-image is to fit it. A
/// big cover shrinks smoothly. A small one grows in whole pixels, so it stays crisp: where a whole
/// number of screen pixels to each of its own fills the same cells, it's grown by that here and
/// padded out to whole cells, so it's drawn as it is. Otherwise each screen pixel is still one of
/// the cover's, just not always the same number of them.
fn fitted(image: &RgbImage, font: (u16, u16), size: (u16, u16)) -> (DynamicImage, Resize) {
    let (fw, fh) = (u32::from(font.0).max(1), u32::from(font.1).max(1));
    let (cols, rows) = (u32::from(size.0), u32::from(size.1));
    let (bw, bh) = (cols * fw, rows * fh);
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 || w > bw || h > bh {
        return (DynamicImage::ImageRgb8(image.clone()), Resize::Fit(Some(FilterType::Triangle)));
    }
    let times = (bw / w).min(bh / h);
    let (tw, th) = (w * times, h * times);
    if tw.div_ceil(fw) < cols && th.div_ceil(fh) < rows {
        return (DynamicImage::ImageRgb8(image.clone()), Resize::Scale(Some(FilterType::Nearest)));
    }
    let mut grown = RgbaImage::new(tw.div_ceil(fw) * fw, th.div_ceil(fh) * fh);
    for (x, y, px) in image.enumerate_pixels() {
        let [r, g, b] = px.0;
        for (dx, dy) in (0..times).flat_map(|dx| (0..times).map(move |dy| (dx, dy))) {
            grown.put_pixel(x * times + dx, y * times + dy, Rgba([r, g, b, 255]));
        }
    }
    (DynamicImage::ImageRgba8(grown), Resize::Fit(None))
}

impl App {
    /// Whether cover art is drawn: it's asked for, and the terminal has more than symbols to show
    /// it with.
    pub fn shows_art(&self) -> bool {
        self.deck.art && !theme::flat() && !super::icons::ascii()
    }

    /// The playing track's cover at `size`, once it's made: a frame asks, and draws it when it's
    /// back.
    pub fn cover(&mut self, size: (u16, u16)) -> Option<&Picture> {
        let now = self.now.as_ref()?;
        let (track, art) = (now.id, now.art.clone()?);
        let current = self.covers.cover.as_ref().is_some_and(|c| c.track == track && c.size == size);
        if !current && self.covers.asked != Some((track, size)) {
            self.covers.asked = Some((track, size));
            let (sixel, halfblocks) = (self.sixel, self.deck.art_halfblocks);
            let picker = self.covers.picker.get_or_insert_with(|| picker(sixel, halfblocks)).clone();
            let job = move || {
                let picture = picture(&art, size, picker.as_ref());
                Msg::Cover(Box::new(Cover { track, size, picture }))
            };
            self.spawn(job);
        }
        let cover = self.covers.cover.as_ref().filter(|c| c.track == track && c.size == size)?;
        Some(&cover.picture)
    }
}

#[cfg(test)]
mod tests;
