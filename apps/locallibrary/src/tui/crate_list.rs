//! The crate, which is the queue: as on a CDJ (its TAG LIST), tracks picked while browsing. What's
//! picked plays in order after the playing track, before the rest of the list `enter` was pressed
//! in (`Queue`, "next from"). Played tracks stay, greyed, until they're taken out. Kept between
//! runs, saved to the drive as a playlist and packed onto a drive.

use std::path::{Path, PathBuf};

use locallibrary::{Library, Track};

#[derive(Clone)]
pub struct CrateTrack {
    /// The drive the track is on.
    pub root: PathBuf,
    pub drive: String,
    pub track: Track,
    /// It's played (or is playing): greyed, and not played again unless it's queued again.
    pub played: bool,
    /// Fill put it there, from what mixes into the track before it.
    pub filled: bool,
}

impl CrateTrack {
    pub fn new(root: &Path, drive: &str, track: Track) -> CrateTrack {
        CrateTrack { root: root.to_path_buf(), drive: drive.to_string(), track, played: false, filled: false }
    }

    pub fn is(&self, root: &Path, file_path: &str) -> bool {
        self.root == root && self.track.file_path == file_path
    }
}

#[derive(Default)]
pub struct Crate {
    pub items: Vec<CrateTrack>,
    /// The pick playing (or ready to play), when it's one of these rather than the list's. Kept
    /// as it was, so the track plays on when it's taken out of the crate.
    pub playing: Option<CrateTrack>,
}

impl Crate {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn contains(&self, root: &Path, file_path: &str) -> bool {
        self.position(root, file_path).is_some()
    }

    pub fn position(&self, root: &Path, file_path: &str) -> Option<usize> {
        self.items.iter().position(|t| t.is(root, file_path))
    }

    /// Whether entry `i` is the pick playing.
    pub fn is_playing(&self, i: usize) -> bool {
        let (Some(p), Some(t)) = (&self.playing, self.items.get(i)) else { return false };
        t.is(&p.root, &p.track.file_path)
    }

    /// The picks still to play, in the order they play.
    pub fn up_next(&self) -> impl Iterator<Item = &CrateTrack> {
        self.items.iter().filter(|t| !t.played)
    }

    /// Add tracks from the drive at `root` to the end, skipping ones still to play. One that's
    /// played is queued again, at the end. Returns how many were added.
    pub fn add(&mut self, root: &Path, drive: &str, tracks: impl IntoIterator<Item = Track>) -> usize {
        let mut added = 0;
        for track in tracks {
            match self.position(root, &track.file_path) {
                Some(i) if !self.items[i].played => continue,
                Some(i) => drop(self.items.remove(i)),
                None => {}
            }
            self.items.push(CrateTrack::new(root, drive, track));
            added += 1;
        }
        added
    }

    /// Put tracks from the drive at `root` at the front of those still to play, as a block in
    /// their order: they play next. Ones already listed move there. Returns how many.
    pub fn play_next(&mut self, root: &Path, drive: &str, tracks: impl IntoIterator<Item = Track>) -> usize {
        let mut block: Vec<CrateTrack> = Vec::new();
        for track in tracks {
            if block.iter().any(|t| t.is(root, &track.file_path)) {
                continue;
            }
            if let Some(i) = self.position(root, &track.file_path) {
                self.items.remove(i);
            }
            block.push(CrateTrack::new(root, drive, track));
        }
        let n = block.len();
        let at = self.items.iter().position(|t| !t.played).unwrap_or(self.items.len());
        self.items.splice(at..at, block);
        n
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

    /// Take every track out (the one playing plays on).
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// How long the picks still to play last.
    pub fn up_next_secs(&self) -> u64 {
        self.up_next().map(|t| t.track.duration_secs as u64).sum()
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
        if let Some(p) = self.playing.as_mut().filter(|p| p.root == root)
            && let Some(t) = by_path.get(p.track.file_path.as_str())
        {
            p.track = (*t).clone();
        }
    }
}

#[cfg(test)]
mod tests;
