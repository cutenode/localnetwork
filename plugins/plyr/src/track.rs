//! The track a set becomes on plyr.fm: titled after the set, its tracklist as the description and
//! tagged as a DJ mix, each within what plyr.fm's track record holds (`fm.plyr.track`'s lexicon,
//! whose lengths are UTF-8 bytes).

use loclib_plugin::{PublishedSet, TracklistEntry, fit_lines};

/// The longest title a track has.
pub(crate) const MAX_TITLE: usize = 256;
/// The longest description a track has.
pub(crate) const MAX_DESCRIPTION: usize = 5000;
/// What plyr.fm lists a set under. Its tags are lowercase, 50 bytes and 10 a track at most.
pub(crate) const TAGS: [&str; 1] = ["dj mix"];

/// A set's track, and what was cut to fit, in a line each.
#[derive(Debug, PartialEq)]
pub(crate) struct Track {
    pub(crate) title: String,
    /// Empty for a set without a tracklist.
    pub(crate) description: String,
    pub(crate) notes: Vec<String>,
}

/// `set`'s track: its title, else where and when it was played ("Warehouse, 2026-10-03"), and its
/// tracklist a line each. A tracklist too long for plyr.fm keeps the tracks that fit, saying how
/// many more there are.
pub(crate) fn of(set: &PublishedSet, tracklist: &[TracklistEntry]) -> Track {
    let mut title = set.name();
    let mut notes = Vec::new();
    if title.len() > MAX_TITLE {
        title.truncate(title.floor_char_boundary(MAX_TITLE));
        notes.push(format!("the title is cut to plyr.fm's {MAX_TITLE} bytes"));
    }
    let lines: Vec<String> = tracklist.iter().map(TracklistEntry::line).collect();
    let (kept, description) = fit_lines(&lines, MAX_DESCRIPTION, str::len);
    if kept < lines.len() {
        let all = lines.len();
        notes.push(format!(
            "the description lists {kept} of the {all} tracks, as many as plyr.fm's {MAX_DESCRIPTION} bytes hold"
        ));
    }
    Track { title, description, notes }
}
