//! The track a set becomes on plyr.fm: titled after the set, its tracklist as the description and
//! tagged as a DJ mix, each within what plyr.fm's track record holds (`fm.plyr.track`'s lexicon,
//! whose lengths are UTF-8 bytes).

use loclib_plugin::{PublishedSet, TracklistEntry};

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
    let given = |s: &Option<String>| s.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let place = given(&set.venue).or_else(|| given(&set.city)).unwrap_or_else(|| "DJ set".into());
    let mut title = given(&set.title).unwrap_or_else(|| format!("{place}, {}", set.date));
    let mut notes = Vec::new();
    if title.len() > MAX_TITLE {
        title.truncate(title.floor_char_boundary(MAX_TITLE));
        notes.push(format!("the title is cut to plyr.fm's {MAX_TITLE} bytes"));
    }
    let lines: Vec<String> = tracklist.iter().map(line).collect();
    let mut description = lines.join("\n");
    if description.len() > MAX_DESCRIPTION {
        let (kept, more) = fitting(&lines);
        let all = lines.len();
        notes.push(format!(
            "the description lists {kept} of the {all} tracks, as many as plyr.fm's {MAX_DESCRIPTION} bytes hold"
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
            used += l.len() + 1;
            used + more(i + 1).len() <= MAX_DESCRIPTION
        })
        .count();
    (kept, more(kept))
}

/// A tracklist's line: `05:30 Artist - Title`, `1:02:03` past an hour, without a time when it's
/// not known. A track published unnamed is `ID - ID`, as DJs write one.
pub(crate) fn line(entry: &TracklistEntry) -> String {
    let named = |s: &Option<String>| s.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("ID").to_string();
    let name = format!("{} - {}", named(&entry.artist), named(&entry.title));
    match entry.start_seconds {
        Some(s) if s >= 3600 => format!("{}:{:02}:{:02} {name}", s / 3600, s / 60 % 60, s % 60),
        Some(s) => format!("{:02}:{:02} {name}", s / 60, s % 60),
        None => name,
    }
}
