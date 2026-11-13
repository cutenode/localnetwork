//! Cover art, none of it on the UI thread: the playing track's picture is made for the size it's
//! shown at, and a track's colour chip read when its row is on screen, each on a worker. A frame
//! only draws what's come back, so a library of thousands costs what's on screen.
//!
//! The picture goes out as the terminal's own graphics where it has them, chosen once from what's
//! known of the terminal by then: nothing's asked of it for this, so it costs the player's start
//! nothing. Everywhere else, half blocks are the floor.

use std::path::{Path, PathBuf};

use locallibrary::{Device, paths};
use loclib_waveform::{Art, PixelBuf, Rgb};
use ratatui::crossterm::terminal::window_size;
use ratatui::layout::Size;
use ratatui_image::Resize;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use rustc_hash::FxHashMap;

use super::app::{App, Msg};
use super::theme;

/// The chips kept: more than a screenful for a long scroll, in what a few bytes each comes to.
const CHIPS_KEPT: usize = 4096;
/// How many covers are read for chips at once: drives are slow, and more would race the scrolling.
const CHIPS_AT_ONCE: usize = 2;

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
    /// The drive the chips are from: artwork ids mean nothing on another.
    root: PathBuf,
    /// By artwork id, each cover's two main colours once read (none: it couldn't be).
    chips: FxHashMap<u32, Option<[Rgb; 2]>>,
    /// Rows on screen last frame with no chip yet, and their covers' paths.
    wanted: Vec<(u32, String)>,
    /// Chips being read.
    reading: usize,
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

    /// A chip from this drive: `None` until it's read. What's not read yet is asked for by the next
    /// tick, for only the rows that are still on screen by then.
    pub fn chip(&mut self, root: &Path, id: u32, path: &str) -> Option<[Rgb; 2]> {
        if self.root != root {
            self.root = root.to_path_buf();
            self.chips.clear();
        }
        match self.chips.get(&id) {
            Some(chip) => *chip,
            None => {
                self.wanted.push((id, path.to_string()));
                None
            }
        }
    }

    /// The frame's rows start over.
    pub fn new_frame(&mut self) {
        self.wanted.clear();
    }

    /// How many rows are still waiting for a chip, or being read.
    pub fn busy(&self) -> bool {
        self.reading > 0 || !self.wanted.is_empty()
    }
}

/// A cover's two main colours, from the thumbnail on the drive.
fn read_chip(root: &Path, path: &str) -> Option<[Rgb; 2]> {
    let file = Device::open(root).ok()?.checked_path(&paths::key(path)).ok()?;
    Some(Art::decode(&std::fs::read(file).ok()?)?.main_colours())
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
        let image = image::DynamicImage::ImageRgb8(art.image().clone());
        p.new_protocol(image, Size::new(cols, rows), Resize::Fit(None)).ok()
    });
    graphics.map_or_else(|| Picture::Pixels(art.resized(cols, rows)), |p| Picture::Graphics(Box::new(p)))
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

    /// Ask for the chips the last frame's rows are waiting for, a few at a time.
    pub(super) fn want_chips(&mut self) {
        if self.covers.chips.len() >= CHIPS_KEPT {
            self.covers.chips.clear();
        }
        while self.covers.reading < CHIPS_AT_ONCE {
            let Some((id, path)) = self.covers.wanted.pop() else { break };
            if self.covers.chips.contains_key(&id) {
                continue;
            }
            self.covers.chips.insert(id, None);
            self.covers.reading += 1;
            let root = self.covers.root.clone();
            let failed = root.clone();
            self.spawn_or(
                move || {
                    let chip = read_chip(&root, &path);
                    Msg::Chip(root, id, chip)
                },
                move |_| Msg::Chip(failed, id, None),
            );
        }
    }

    /// A chip's back: kept unless the list has moved to another drive, whose ids mean something else.
    pub(super) fn chip_read(&mut self, root: &Path, id: u32, chip: Option<[Rgb; 2]>) {
        self.covers.reading -= 1;
        if root == self.covers.root {
            self.covers.chips.insert(id, chip);
        }
    }
}

#[cfg(test)]
mod tests;
