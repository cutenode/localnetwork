//! A set's tracks as chapters of its recording, so podcast apps and players show the track playing
//! and skip between them: Podcasting 2.0 JSON chapters (`sets export --format chapters`), and
//! chapters written into the file `sets publish --upload` sends.

use schemars::JsonSchema;
use serde::Serialize;

use super::export::clock;
use super::{Set, SetEntry};

/// A track of a set as a chapter of its recording.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Chapter {
    /// Seconds into the recording.
    pub start: u32,
    /// The track as it's shown: `Artist - Title`, or `ID - ID`.
    pub title: String,
}

/// `set`'s tracks as chapters, in the order they play: one for each track with a start time, and
/// before the recording ends when its `length` (in seconds) is known. `title` names the track at a
/// position (from 1). The notes say which tracks get no chapter, and why.
pub fn chapters(
    set: &Set,
    length: Option<u64>,
    title: impl Fn(usize, &SetEntry) -> String,
) -> (Vec<Chapter>, Vec<String>) {
    let mut timed: Vec<(usize, u32, &SetEntry)> =
        (1..).zip(&set.tracks).filter_map(|(n, e)| Some((n, e.start_seconds?, e))).collect();
    // A stable sort: of tracks that start together, the first in the set is the chapter.
    timed.sort_by_key(|&(_, start, _)| start);
    let untimed: Vec<String> =
        (1..).zip(&set.tracks).filter(|(_, e)| e.start_seconds.is_none()).map(|(n, _)| n.to_string()).collect();
    let mut notes = match untimed.as_slice() {
        [] => vec![],
        all if all.len() == set.tracks.len() => vec!["no track has a start time, so there are no chapters".into()],
        [one] => vec![format!("track {one} has no start time, so no chapter")],
        many => vec![format!("tracks {} have no start time, so no chapters", many.join(", "))],
    };
    let mut out = Vec::new();
    let mut last: Option<(usize, u32)> = None;
    for (n, start, e) in timed {
        if length.is_some_and(|l| u64::from(start) >= l) {
            notes.push(format!("track {n} starts at {}, after the recording ends, so no chapter", clock(start)));
        } else if let Some((m, _)) = last.filter(|&(_, s)| s == start) {
            notes.push(format!("track {n} starts when track {m} does ({}), so no chapter of its own", clock(start)));
        } else {
            out.push(Chapter { start, title: title(n, e) });
            last = Some((n, start));
        }
    }
    (out, notes)
}

/// Each chapter with where it ends, in seconds: where the next one starts, and the last where the
/// recording does, `length` seconds in.
pub fn ends(chapters: &[Chapter], length: u64) -> impl Iterator<Item = (&Chapter, u64)> {
    let next = chapters.iter().skip(1).map(|c| u64::from(c.start)).chain([length]);
    chapters.iter().zip(next)
}

#[cfg(test)]
mod tests;
