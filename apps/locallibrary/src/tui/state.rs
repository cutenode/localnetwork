//! What the player remembers between runs (the drive and list it was showing, the view, sort,
//! volume, visualiser and panels, its theme, colours and icons, quantize, the output, the queue:
//! its picks, which have played and the list it plays on with, and where it was), in `tui.json` in the store, or beside the store's folder until there is one. Best
//! effort: a missing or unreadable file just means starting fresh, an unreadable one is left as it
//! is for the rest of the run, and nothing is ever written to a drive. Of it, the choices that are the user's preferences (the theme, colours and icons,
//! the columns, the visualiser, playback modes, how keys and waveforms look, the footer's hints and
//! the palette's recent commands) go in `settings export`; see `locallibrary::settings::transfer`.

use std::path::{Path, PathBuf};

use locallibrary::Track;
use locallibrary::query::SortKey;
use locallibrary::store::{PLAYER_FILE as FILE, player_dir, player_file};
use serde::{Deserialize, Serialize};

use super::browse::SavedNode;

/// The most of the queue kept: a list played from a big library can hold every track on it.
pub const QUEUE_KEPT: usize = 1000;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    /// The drive that was open.
    pub root: Option<PathBuf>,
    /// The list it was showing.
    pub node: Option<SavedNode>,
    /// The view (`Library`, `Drives`...).
    pub view: Option<String>,
    pub sort: Option<SortKey>,
    pub search_all: bool,
    pub volume: Option<f32>,
    /// The visualiser's name (`spectrum`, `deck`...).
    pub vis: Option<String>,
    pub details: bool,
    /// The crate panel.
    pub crate_panel: bool,
    /// The track list's rating column.
    pub ratings: bool,
    /// The track list's My Tags column.
    pub tags_column: bool,
    /// The track list's columns of plugins' fields (`sound.bpm`), in order.
    pub field_columns: Vec<String>,
    /// The theme it draws in, a plugin's (`<plugin>.<name>`): the default without one.
    pub theme: Option<String>,
    /// The colour tier asked for (`truecolor`, `256`, `16`): what the terminal says without one.
    pub color: Option<String>,
    /// The icons asked for (`nerd`, `unicode`, `ascii`): what suits the terminal without one.
    pub icons: Option<String>,
    pub shuffle: bool,
    /// `off`, `all` or `one`.
    pub repeat: Option<String>,
    /// Quantize (on unless turned off).
    pub quantize: Option<bool>,
    /// The output device picked (`None`: the system's default).
    pub output: Option<String>,
    /// Single play: stop after each track.
    pub single: bool,
    /// Auto cue: start tracks at their first memory cue or beat.
    pub auto_cue: bool,
    /// The Library view shows the deck.
    pub deck_layout: bool,
    /// How keys are written (`camelot`, `open-key`, `musical`).
    pub notation: Option<String>,
    /// The colours keys come in.
    pub key_palette: Option<String>,
    /// How waveforms are coloured (`blue`, `rgb`, `3band`): as the drive says without one.
    pub bands: Option<String>,
    /// Seconds before a track's end the deck warns of it, 0 for not at all.
    pub end_warning: Option<u16>,
    /// Cover art (on unless turned off).
    pub art: Option<bool>,
    /// Cover art in half blocks, whatever graphics the terminal seems to have.
    pub art_halfblocks: bool,
    /// How much the footer says (`full`, `compact`, `off`): all of it without one.
    pub hints: Option<String>,
    /// With no hints, the footer still says `? help` (on unless turned off).
    pub help_hint: Option<bool>,
    /// The palette's commands run lately, newest first, by name.
    pub recent_commands: Vec<String>,
    /// The version that last showed what a first run shows once.
    pub intro_seen: Option<String>,
    /// Recorded sets opened in the Mixes view, newest first.
    pub mixes: Vec<PathBuf>,
    /// Folders of recorded sets opened in the Mixes view, newest first.
    pub mix_folders: Vec<PathBuf>,
    /// The crate, which is the queue's picks.
    pub crate_list: Vec<SavedCrateTrack>,
    /// The pick playing, or ready to play, and how far into it (as the queue's `at_ms`).
    pub crate_playing: Option<SavedCrateTrack>,
    pub crate_at_ms: u64,
    /// Fill: when the queue runs out, the best match goes on the end.
    pub fill: bool,
    pub queue: Option<SavedQueue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedCrateTrack {
    pub root: PathBuf,
    pub drive: String,
    pub track: Track,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub played: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub filled: bool,
}

/// The list the queue plays on with after its picks: its tracks by file path in the order they
/// play, the one it was on, and how far into that one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedQueue {
    pub root: PathBuf,
    pub drive: String,
    /// What the list is called.
    pub name: String,
    pub tracks: Vec<String>,
    pub pos: usize,
    pub at_ms: u64,
}

/// Read the saved state (the store's, else the one kept beside it): the defaults when there's
/// none, `None` when there's one that can't be read or parsed (a newer build's, say), which the
/// player then runs without and doesn't save over.
pub fn load(store_root: &Path) -> Option<Saved> {
    let Some(file) = player_file(store_root, FILE) else { return Some(Saved::default()) };
    match std::fs::read(file) {
        Ok(bytes) => serde_json::from_slice(&bytes).ok(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(Saved::default()),
        Err(_) => None,
    }
}

/// Save the state into the store when there is one. The store refuses to set itself up in a
/// directory that holds anything else, so the player mustn't be the first to create it: until
/// then the state goes beside it. Never inside the drive that was open.
pub fn save(store_root: &Path, saved: &Saved) {
    let Some(dir) = player_dir(store_root) else { return };
    // Compared resolved: the drive's root comes resolved (macOS's /var is /private/var), the
    // store's folder as it was given.
    let resolved = locallibrary::fsutil::resolved;
    if saved.root.as_deref().is_some_and(|r| resolved(&dir).starts_with(resolved(r))) {
        return;
    }
    // One that can't be read right now keeps what it holds.
    let file = dir.join(FILE);
    if std::fs::File::open(&file).is_err_and(|e| e.kind() != std::io::ErrorKind::NotFound) {
        return;
    }
    if let Ok(json) = serde_json::to_vec_pretty(saved)
        && std::fs::create_dir_all(&dir).is_ok()
    {
        let _ = locallibrary::fsutil::write_atomic(&file, &json);
    }
}

#[cfg(test)]
mod tests;
