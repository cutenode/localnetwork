//! The track a set becomes on SoundCloud: titled after the set, its tracklist as the description
//! and tagged as a DJ mix, each within what SoundCloud's upload form takes (its lengths are
//! characters).

use loclib_plugin::{PublishedSet, TracklistEntry, fit_lines};

/// The longest title a track has, in characters.
pub(crate) const MAX_TITLE: usize = 100;
/// The longest description a track has, in characters.
pub(crate) const MAX_DESCRIPTION: usize = 4000;
/// What SoundCloud lists a set under: tags are separated by spaces, so one of more words is
/// quoted.
pub(crate) const TAGS: &str = "\"dj mix\"";

/// A set's track, and what was cut to fit, in a line each.
#[derive(Debug, PartialEq)]
pub(crate) struct Track {
    pub(crate) title: String,
    /// Empty for a set without a tracklist.
    pub(crate) description: String,
    pub(crate) notes: Vec<String>,
}

/// `set`'s track: its title, else where and when it was played ("Warehouse, 2026-10-03"), and its
/// tracklist a line each. A tracklist too long for SoundCloud keeps the tracks that fit, saying
/// how many more there are.
pub(crate) fn of(set: &PublishedSet, tracklist: &[TracklistEntry]) -> Track {
    let mut title = set.name();
    let mut notes = Vec::new();
    if title.chars().count() > MAX_TITLE {
        title = title.chars().take(MAX_TITLE).collect::<String>().trim_end().into();
        notes.push(format!("the title is cut to SoundCloud's {MAX_TITLE} characters"));
    }
    let lines: Vec<String> = tracklist.iter().map(TracklistEntry::line).collect();
    let (kept, description) = fit_lines(&lines, MAX_DESCRIPTION, |l: &str| l.chars().count());
    if kept < lines.len() {
        let all = lines.len();
        notes.push(format!(
            "the description lists {kept} of the {all} tracks, as many as SoundCloud's {MAX_DESCRIPTION} characters hold"
        ));
    }
    Track { title, description, notes }
}
