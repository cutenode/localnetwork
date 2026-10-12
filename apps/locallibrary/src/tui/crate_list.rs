//! The crate: as on a CDJ (its TAG LIST), tracks picked while browsing, kept for the session,
//! played as a list of their own and saved to the drive as a playlist. Separate from the play
//! queue, which Enter rebuilds from whatever list it's pressed in.

use std::path::{Path, PathBuf};

use locallibrary::{Library, Track};

pub struct CrateTrack {
    /// The drive the track is on.
    pub root: PathBuf,
    pub drive: String,
    pub track: Track,
}

#[derive(Default)]
pub struct Crate {
    pub items: Vec<CrateTrack>,
}

impl Crate {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn contains(&self, root: &Path, file_path: &str) -> bool {
        self.items.iter().any(|t| t.root == root && t.track.file_path == file_path)
    }

    /// Add tracks from the drive at `root` to the end, skipping ones already listed. Returns how
    /// many were added.
    pub fn add(&mut self, root: &Path, drive: &str, tracks: impl IntoIterator<Item = Track>) -> usize {
        let before = self.items.len();
        for track in tracks {
            if !self.contains(root, &track.file_path) {
                self.items.push(CrateTrack { root: root.to_path_buf(), drive: drive.to_string(), track });
            }
        }
        self.items.len() - before
    }

    pub fn remove(&mut self, i: usize) -> Option<CrateTrack> {
        (i < self.items.len()).then(|| self.items.remove(i))
    }

    /// Move entry `i` up (negative) or down by `delta` places, stopping at the ends. Returns
    /// where it ended up.
    pub fn shift(&mut self, i: usize, delta: isize) -> usize {
        if i >= self.items.len() {
            return i;
        }
        let to = (i as isize + delta).clamp(0, self.items.len() as isize - 1) as usize;
        let item = self.items.remove(i);
        self.items.insert(to, item);
        to
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn duration_secs(&self) -> u64 {
        self.items.iter().map(|t| t.track.duration_secs as u64).sum()
    }

    /// The drive at `root` was read again (after an edit, say): take the tracks' new details,
    /// and drop the ones it no longer has.
    pub fn refresh(&mut self, root: &Path, lib: &Library) {
        let by_path: rustc_hash::FxHashMap<&str, &Track> =
            lib.tracks.iter().map(|t| (t.file_path.as_str(), t)).collect();
        self.items.retain_mut(|item| {
            if item.root != root {
                return true;
            }
            match by_path.get(item.track.file_path.as_str()) {
                Some(t) => {
                    item.track = (*t).clone();
                    true
                }
                None => false,
            }
        });
    }
}

#[cfg(test)]
mod tests;
