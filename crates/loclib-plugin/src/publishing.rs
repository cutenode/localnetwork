//! What a set destination writes of a set: its name, and its tracklist a line a track, as much
//! of it as fits; as a YouTube description ([`youtube`]); and what Mixcloud's rules say of its
//! music ([`mixcloud`]).

pub mod mixcloud;
pub mod youtube;

use crate::{PublishedSet, TracklistEntry};

impl PublishedSet {
    /// Where it was played: its venue, else its city, on one line.
    pub fn place(&self) -> Option<String> {
        plain(self.venue.as_deref()).or_else(|| plain(self.city.as_deref()))
    }

    /// Its title, else where and when it was played ("Warehouse, 2026-10-03", "DJ set, ..." when
    /// it's not known where), on one line.
    pub fn name(&self) -> String {
        plain(self.title.as_deref()).unwrap_or_else(|| {
            let place = self.place().unwrap_or_else(|| "DJ set".into());
            format!("{place}, {}", plain(Some(self.date.as_str())).unwrap_or_default())
        })
    }
}

impl TracklistEntry {
    /// Its line in a tracklist: `05:30 Artist - Title`, `1:02:03` past an hour, without a time
    /// when it's not known. A track published unnamed is `ID - ID`, as DJs write one.
    pub fn line(&self) -> String {
        self.line_at(self.start_seconds)
    }

    /// Its line with the time `at` (a chapter's start, say) in place of its own: without a time
    /// for none.
    pub fn line_at(&self, at: Option<u32>) -> String {
        let named = |s: &Option<String>| plain(s.as_deref()).unwrap_or_else(|| "ID".into());
        let name = format!("{} - {}", named(&self.artist), named(&self.title));
        match at {
            Some(at) => format!("{} {name}", clock(at)),
            None => name,
        }
    }
}

/// `secs` as a tracklist's time: `05:30`, or past an hour `1:02:03`.
pub fn clock(secs: u32) -> String {
    match secs {
        3600.. => format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60),
        _ => format!("{:02}:{:02}", secs / 60, secs % 60),
    }
}

/// `lines` a line each within `room`, as `len` measures it (`str::len` for bytes): all of them
/// when they fit, else those that fit and a last line saying how many more there are. How many
/// it keeps, and the text.
pub fn fit_lines(lines: &[String], room: usize, len: impl Fn(&str) -> usize) -> (usize, String) {
    let all = lines.join("\n");
    if len(&all) <= room {
        return (lines.len(), all);
    }
    let more = |kept: usize| format!("+ {} more", lines.len() - kept);
    let mut used = 0;
    // Each line kept takes more room than the last line saves (a digit at most).
    let kept = lines
        .iter()
        .enumerate()
        .take_while(|(i, l)| {
            used += len(l) + 1;
            used + len(&more(i + 1)) <= room
        })
        .count();
    (kept, lines[..kept].iter().map(|l| format!("{l}\n")).collect::<String>() + &more(kept))
}

/// `text` on one line and trimmed, when there's anything left of it: a name is one line however
/// it was written.
fn plain(text: Option<&str>) -> Option<String> {
    let text: String = text?.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.into())
}

#[cfg(test)]
mod tests;
