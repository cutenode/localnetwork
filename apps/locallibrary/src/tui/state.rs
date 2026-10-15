//! What the player remembers between runs (the drive and list it was showing, the view, sort,
//! volume, visualiser and panels, quantize, the output, the crate, and the queue and where it was
//! in it), in `tui.json` in the store, or beside the store's folder until there is one. Best
//! effort: a missing or unreadable file just means starting fresh, and nothing is ever written to
//! a drive.

use std::path::{Path, PathBuf};

use locallibrary::Track;
use locallibrary::query::SortKey;
use serde::{Deserialize, Serialize};

use super::browse::SavedNode;

const FILE: &str = "tui.json";

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
    /// The crate panel (`tags` before the crate was called that).
    #[serde(alias = "tags")]
    pub crate_panel: bool,
    /// The track list's rating column.
    pub ratings: bool,
    /// The track list's My Tags column.
    pub tags_column: bool,
    /// The track list's columns of plugins' fields (`sound.bpm`), in order.
    pub field_columns: Vec<String>,
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
    /// Recorded sets opened in the Mixes view, newest first.
    pub mixes: Vec<PathBuf>,
    /// Folders of recorded sets opened in the Mixes view, newest first.
    pub mix_folders: Vec<PathBuf>,
    /// The crate (`tag_list` before it was called that).
    #[serde(alias = "tag_list")]
    pub crate_list: Vec<SavedCrateTrack>,
    pub queue: Option<SavedQueue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedCrateTrack {
    pub root: PathBuf,
    pub drive: String,
    pub track: Track,
}

/// The play queue: its tracks by file path in the order they play, the one it was on, and how
/// far into that one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedQueue {
    pub root: PathBuf,
    pub drive: String,
    pub tracks: Vec<String>,
    pub pos: usize,
    pub at_ms: u64,
}

/// Where the state goes while there's no store: a folder beside the store's
/// (`locallibrary-player` next to `locallibrary` in the platform's data folder by default).
fn fallback(store_root: &Path) -> Option<PathBuf> {
    let name = store_root.file_name()?.to_string_lossy();
    Some(store_root.with_file_name(format!("{name}-player")))
}

fn has_store(store_root: &Path) -> bool {
    store_root.join("store.json").is_file()
}

/// Read the saved state (the store's, else the one kept beside it); anything wrong with it
/// means the defaults.
pub fn load(store_root: &Path) -> Saved {
    let dir = if has_store(store_root) && store_root.join(FILE).is_file() {
        Some(store_root.to_path_buf())
    } else {
        fallback(store_root)
    };
    dir.and_then(|d| std::fs::read(d.join(FILE)).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Save the state into the store when there is one. The store refuses to set itself up in a
/// directory that holds anything else, so the player mustn't be the first to create it: until
/// then the state goes beside it. Never inside the drive that was open.
pub fn save(store_root: &Path, saved: &Saved) {
    let Some(dir) = (if has_store(store_root) { Some(store_root.to_path_buf()) } else { fallback(store_root) }) else {
        return;
    };
    if saved.root.as_deref().is_some_and(|r| dir.starts_with(r)) {
        return;
    }
    if let Ok(json) = serde_json::to_vec_pretty(saved)
        && std::fs::create_dir_all(&dir).is_ok()
    {
        let _ = locallibrary::fsutil::write_atomic(&dir.join(FILE), &json);
    }
}

#[cfg(test)]
mod tests;
