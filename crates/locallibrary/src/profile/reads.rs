//! What the profile reads in a track's analysis file (its phrases' mood, how many cues of each
//! kind it has), kept in `<store>/cache/profile` so the next profile reads none of the files
//! again: a stored file by its contents, a file on a drive by its path, size and modification
//! time.

use serde::{Deserialize, Serialize};

use super::{CueCounts, Files, Origin};
use crate::anlz::{self, Mood};
use crate::audio_cache::{Cache, Kind};
use crate::progress::WarningLimiter;

pub(super) struct Reads;

/// What one analysis file says.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Read {
    /// From its phrases (an `.EXT` has them).
    pub mood: Option<Mood>,
    /// `None` when its cues can't be read.
    pub cues: Option<CueCounts>,
}

impl Kind for Reads {
    const DIR: &'static str = "profile";
    const EXT: &'static str = "json";
    const WHAT: &'static str = "an analysis file's cues and mood";
    /// Bump when [`Read::of`] makes something else of the same file.
    const VERSION: u32 = 1;
    type Value = Read;

    fn encode(read: &Read) -> Vec<u8> {
        serde_json::to_vec(read).unwrap_or_default()
    }

    fn decode(bytes: &[u8]) -> Option<Read> {
        serde_json::from_slice(bytes).ok()
    }
}

impl Read {
    pub(super) fn of(bytes: &[u8]) -> Read {
        Read {
            mood: anlz::song_structure(bytes).ok().flatten().map(|s| s.mood),
            cues: anlz::cues(bytes).ok().map(|cues| CueCounts {
                hot: cues.iter().filter(|c| c.hot_cue.is_some()).count(),
                memory: cues.iter().filter(|c| c.hot_cue.is_none() && c.end_ms.is_none()).count(),
                loops: cues.iter().filter(|c| c.end_ms.is_some()).count(),
            }),
        }
    }
}

impl Files<'_> {
    /// The analysis file `key` of `origin` read ([`Read::of`]), and whether that came from the
    /// cache; `None` when there's no such file.
    pub(super) fn analysis(&self, origin: Origin, key: &str, warn: &mut WarningLimiter) -> Option<(Read, bool)> {
        let cache_key = self.cache.as_ref().and_then(|_| self.content_key(origin, key));
        if let (Some(cache), Some(k)) = (&self.cache, &cache_key)
            && let Some(read) = cache.get(k)
        {
            return Some((read, true));
        }
        let read = Read::of(&self.read(origin, key)?);
        if let (Some(cache), Some(k)) = (&self.cache, &cache_key)
            && let Err(e) = cache.put(k, &read)
        {
            warn.warn("uncached analysis files", format!("couldn't cache {}: {e}", Reads::WHAT));
        }
        Some((read, false))
    }

    /// What names the file's contents: a stored file's hash, or a drive file's path, size and
    /// modification time.
    fn content_key(&self, origin: Origin, key: &str) -> Option<String> {
        let content = match origin {
            Origin::Drive(i) => {
                let path = self.drives[i].checked_path(key).ok()?;
                let meta = std::fs::metadata(&path).ok()?;
                let mtime = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
                format!("{}\u{1f}{}\u{1f}{mtime}", path.display(), meta.len())
            }
            Origin::Snapshot(i) => self.snapshots[i].get(&key.to_lowercase())?.to_hex(),
        };
        Some(Cache::<Reads>::key_of(&content))
    }
}
