//! What a set's video says on YouTube: its title, and its tracklist as the description, a line a
//! track, timed so YouTube makes each a chapter. Each is within what YouTube's `videos` resource
//! takes: a title of 100 characters, a description of 5,000 bytes, neither with `<` or `>`.
//! YouTube makes chapters of a description whose first time is `00:00` and that has at least
//! three, in order, each at least ten seconds long; otherwise it makes none, so a time that would
//! break that is left off its line.

use loclib_plugin::{PublishedSet, TracklistEntry, fit_lines};

/// The longest title a video has, in characters.
pub(crate) const MAX_TITLE: usize = 100;
/// The longest description, in bytes.
pub(crate) const MAX_DESCRIPTION: usize = 5000;
/// The shortest chapter, in seconds.
pub(crate) const MIN_CHAPTER: u32 = 10;
/// How many chapters a description needs before YouTube shows them.
pub(crate) const MIN_CHAPTERS: usize = 3;

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

/// `text` as YouTube takes it: without `<` or `>`.
fn plain(text: &str) -> String {
    text.replace(['<', '>'], "")
}

/// `set`'s details for a recording `length` seconds long: its title, else where and when it was
/// played ("Warehouse, 2026-10-03"), and its tracklist a line each (see [`chapters`]). A
/// tracklist too long for YouTube keeps the tracks that fit, saying how many more there are.
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
    let starts = chapters(tracklist, length);
    let lines: Vec<String> = tracklist.iter().zip(&starts).map(|(e, at)| line(e, *at)).collect();
    let (kept, description) = fit_lines(&lines, MAX_DESCRIPTION, str::len);
    if kept < lines.len() {
        notes.push(format!(
            "the description lists {kept} of the {} tracks, as many as YouTube's {MAX_DESCRIPTION} bytes hold",
            lines.len()
        ));
    }
    let chapters = starts[..kept].iter().flatten().count();
    // The first track starts at 0 once any has a time.
    let any_timed = tracklist.iter().any(|e| e.start_seconds.is_some());
    let timed =
        tracklist.iter().take(kept).enumerate().filter(|(i, e)| e.start_seconds.is_some() || *i == 0 && any_timed);
    let untimed = timed.count() - chapters;
    if untimed > 0 {
        notes.push(format!(
            "left the time off {untimed} of the tracks: YouTube's chapters are in order, each at least \
             {MIN_CHAPTER} seconds long"
        ));
    }
    if (1..MIN_CHAPTERS).contains(&chapters) {
        notes.push(format!("YouTube shows chapters once there are {MIN_CHAPTERS}, and the description has {chapters}"));
    }
    Details { title, description, chapters, notes }
}

/// Where each entry's chapter starts, if it has one: once any entry has a time, the first starts
/// at 0 (the recording starts with it), and each later one with a time has one when it starts at
/// least [`MIN_CHAPTER`] after the chapter before and before the recording's last
/// [`MIN_CHAPTER`] seconds.
pub(crate) fn chapters(tracklist: &[TracklistEntry], length: u32) -> Vec<Option<u32>> {
    let timed = tracklist.iter().any(|e| e.start_seconds.is_some());
    let mut last: Option<u32> = None;
    let mut starts = Vec::with_capacity(tracklist.len());
    for (i, entry) in tracklist.iter().enumerate() {
        let start = if i == 0 && timed { Some(0) } else { entry.start_seconds };
        let after_last = |s: u32| last.is_none_or(|l| s >= l.saturating_add(MIN_CHAPTER));
        let start = start.filter(|&s| after_last(s) && s.saturating_add(MIN_CHAPTER) <= length);
        last = start.or(last);
        starts.push(start);
    }
    starts
}

/// A tracklist's line: `05:30 Artist - Title` when it starts a chapter at `at`, else without a
/// time, as YouTube takes it. A track published unnamed is `ID - ID`, as DJs write one.
pub(crate) fn line(entry: &TracklistEntry, at: Option<u32>) -> String {
    let (artist, title) = (entry.artist.as_deref().map(plain), entry.title.as_deref().map(plain));
    TracklistEntry { artist, title, ..entry.clone() }.line_at(at)
}
