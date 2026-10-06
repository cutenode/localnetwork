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
        let by_path: std::collections::HashMap<&str, &Track> =
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
mod tests {
    use super::super::browse::tests::{library, track};
    use super::*;

    #[test]
    fn adds_once_removes_and_reorders() {
        let root = Path::new("/Volumes/A");
        let mut list = Crate::default();
        assert_eq!(list.add(root, "A", [track(1, "a", "One"), track(2, "b", "Two"), track(1, "a", "One")]), 2);
        assert_eq!(list.add(root, "A", [track(2, "b", "Two"), track(3, "c", "Three")]), 1);
        // The same file on another drive is another track.
        assert_eq!(list.add(Path::new("/Volumes/B"), "B", [track(1, "a", "One")]), 1);
        let ids = |t: &Crate| t.items.iter().map(|x| x.track.id).collect::<Vec<_>>();
        assert_eq!(ids(&list), [1, 2, 3, 1]);
        assert_eq!(list.shift(0, 1), 1);
        assert_eq!(ids(&list), [2, 1, 3, 1]);
        assert_eq!(list.shift(1, -5), 0, "stops at the top");
        assert_eq!(list.shift(0, 9), 3, "and at the bottom");
        assert_eq!(ids(&list), [2, 3, 1, 1]);
        assert_eq!(list.remove(1).map(|t| t.track.id), Some(3));
        assert!(list.remove(9).is_none());
        assert_eq!(list.duration_secs(), 900);
        list.clear();
        assert!(list.is_empty());
    }

    #[test]
    fn refresh_takes_new_details_and_drops_missing_tracks() {
        let root = Path::new("/Volumes/A");
        let mut lib = library();
        let mut list = Crate::default();
        list.add(root, "A", lib.tracks.iter().take(2).cloned());
        list.add(Path::new("/Volumes/B"), "B", [track(7, "x", "Elsewhere")]);
        lib.tracks[0].rating = 1;
        lib.tracks.remove(1);
        list.refresh(root, &lib);
        assert_eq!(list.len(), 2);
        assert_eq!(list.items[0].track.rating, 1);
        assert_eq!(list.items[1].track.title, "Elsewhere");
    }
}
