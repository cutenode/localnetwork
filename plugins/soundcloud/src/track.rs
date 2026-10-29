//! The track a set becomes on SoundCloud: titled after the set, its tracklist as the description
//! and tagged as a DJ mix, each within what SoundCloud's upload form takes (its lengths are
//! characters).

use loclib_plugin::{PublishedSet, TracklistEntry};

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

/// `text` on one line: a tracklist's line is one line however its names were written.
fn plain(text: &str) -> String {
    text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().into()
}

/// `set`'s track: its title, else where and when it was played ("Warehouse, 2026-10-03"), and its
/// tracklist a line each. A tracklist too long for SoundCloud keeps the tracks that fit, saying
/// how many more there are.
pub(crate) fn of(set: &PublishedSet, tracklist: &[TracklistEntry]) -> Track {
    let given = |s: &Option<String>| s.as_deref().map(plain).filter(|s| !s.is_empty());
    let place = given(&set.venue).or_else(|| given(&set.city)).unwrap_or_else(|| "DJ set".into());
    let mut title = given(&set.title).unwrap_or_else(|| format!("{place}, {}", plain(&set.date)));
    let mut notes = Vec::new();
    if title.chars().count() > MAX_TITLE {
        title = title.chars().take(MAX_TITLE).collect::<String>().trim_end().into();
        notes.push(format!("the title is cut to SoundCloud's {MAX_TITLE} characters"));
    }
    let lines: Vec<String> = tracklist.iter().map(line).collect();
    let mut description = lines.join("\n");
    if description.chars().count() > MAX_DESCRIPTION {
        let (kept, more) = fitting(&lines);
        let all = lines.len();
        notes.push(format!(
            "the description lists {kept} of the {all} tracks, as many as SoundCloud's {MAX_DESCRIPTION} characters hold"
        ));
        description = lines[..kept].iter().map(|l| format!("{l}\n")).collect::<String>() + &more;
    }
    Track { title, description, notes }
}

/// How many of `lines` fit in a description with a last line saying how many more there are,
/// and that line.
fn fitting(lines: &[String]) -> (usize, String) {
    let more = |kept: usize| format!("+ {} more", lines.len() - kept);
    let mut used = 0;
    // Each line kept takes more room than the last line saves (a digit at most).
    let kept = lines
        .iter()
        .enumerate()
        .take_while(|(i, l)| {
            used += l.chars().count() + 1;
            used + more(i + 1).len() <= MAX_DESCRIPTION
        })
        .count();
    (kept, more(kept))
}

/// A tracklist's line: `05:30 Artist - Title`, `1:02:03` past an hour, without a time when it's
/// not known. A track published unnamed is `ID - ID`, as DJs write one.
pub(crate) fn line(entry: &TracklistEntry) -> String {
    let named = |s: &Option<String>| s.as_deref().map(plain).filter(|s| !s.is_empty()).unwrap_or_else(|| "ID".into());
    let name = format!("{} - {}", named(&entry.artist), named(&entry.title));
    match entry.start_seconds {
        Some(s) if s >= 3600 => format!("{}:{:02}:{:02} {name}", s / 3600, s / 60 % 60, s % 60),
        Some(s) => format!("{:02}:{:02} {name}", s / 60, s % 60),
        None => name,
    }
}
