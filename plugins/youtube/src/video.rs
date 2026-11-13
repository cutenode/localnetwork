//! What a set's video says on YouTube: its title, and its tracklist as the description (see
//! `loclib_plugin::youtube`), timed so YouTube makes each track a chapter. The title is within
//! what YouTube's `videos` resource takes: 100 characters, without `<` or `>`.

use loclib_plugin::youtube::{self, plain};
use loclib_plugin::{PublishedSet, TracklistEntry};

/// The longest title a video has, in characters.
pub(crate) const MAX_TITLE: usize = 100;

/// A set's title and description, and what was cut or left out, in a line each.
#[derive(Debug, PartialEq)]
pub(crate) struct Details {
    pub(crate) title: String,
    /// Empty for a set without a tracklist.
    pub(crate) description: String,
    /// How many of its lines are chapters.
    pub(crate) chapters: usize,
    pub(crate) notes: Vec<String>,
}

/// `set`'s details for a recording `length` seconds long: its title, else where and when it was
/// played ("Warehouse, 2026-10-03"), and its tracklist as YouTube's description of it.
pub(crate) fn of(set: &PublishedSet, tracklist: &[TracklistEntry], length: u32) -> Details {
    let mut title = PublishedSet {
        title: set.title.as_deref().map(plain),
        venue: set.venue.as_deref().map(plain),
        city: set.city.as_deref().map(plain),
        date: plain(&set.date),
        ..set.clone()
    }
    .name();
    let mut notes = Vec::new();
    if title.chars().count() > MAX_TITLE {
        title = title.chars().take(MAX_TITLE).collect::<String>().trim_end().into();
        notes.push(format!("the title is cut to YouTube's {MAX_TITLE} characters"));
    }
    let description = youtube::description(tracklist, length);
    notes.extend(description.notes);
    Details { title, description: description.text, chapters: description.chapters, notes }
}
