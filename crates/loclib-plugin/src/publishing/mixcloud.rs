//! Mixcloud's rules for the music a show plays, which it licenses: over 15 minutes, at most 3
//! tracks from one release (2 in a row) and 4 by one artist (3 in a row), or only subscribers can
//! listen; under 15 minutes, no copyrighted music at all. The mixcloud plugin warns about them
//! when it publishes a set, and `sets export --format mixcloud` when it writes the tracklist.

use loclib_model::model::{artist_credits, normalize};

use crate::TracklistEntry;

/// The shortest show that may play copyrighted music, in seconds.
const MIN_LENGTH: u64 = 15 * 60;

/// What a show breaks a limit on tracks from one release or by one artist before.
const LIMITED: &str = "before it limits the show to subscribers";

/// A warning for each of Mixcloud's rules `tracklist` breaks. The show's length is to the end of
/// its last track when that has a start time (about the recording's), else its tracks' lengths
/// added up when each is known. A release is a track's album, and an artist each one its credit
/// names (see [`artist_credits`]), each told apart ignoring case; a track published unnamed has
/// neither.
pub fn warnings(tracklist: &[TracklistEntry]) -> Vec<String> {
    let length = match tracklist.last() {
        Some(TracklistEntry { start_seconds: Some(s), duration_seconds, .. }) => {
            Some(u64::from(*s) + u64::from(duration_seconds.unwrap_or(0)))
        }
        _ => tracklist.iter().map(|e| e.duration_seconds.map(u64::from)).sum(),
    };
    if let Some(length) = length.filter(|l| (1..MIN_LENGTH).contains(l)) {
        return vec![format!(
            "it's {} minutes long: Mixcloud doesn't allow copyrighted music in uploads under 15 minutes",
            length / 60
        )];
    }
    let releases: Vec<Vec<String>> =
        tracklist.iter().map(|e| e.album.iter().filter(|a| !a.trim().is_empty()).cloned().collect()).collect();
    let artists: Vec<Vec<String>> =
        tracklist.iter().map(|e| e.artist.as_deref().map(artist_credits).unwrap_or_default()).collect();
    let mut out = limits(&releases, |r| format!("from {r:?}"), 3, 2);
    out.extend(limits(&artists, |a| format!("by {a}"), 4, 3));
    out
}

/// A warning for each name (a release or an artist; `tracks` holds each track's) on more than
/// `most` tracks, and each run of more than `in_a_row` tracks one after another with it.
fn limits(tracks: &[Vec<String>], what: impl Fn(&str) -> String, most: usize, in_a_row: usize) -> Vec<String> {
    // Each name by its key, as first spelled: how many tracks have it, and where its run started.
    let mut names: Vec<(String, String, usize, Option<usize>)> = Vec::new();
    let mut runs = Vec::new();
    // One past the last track ends every run.
    for i in 0..=tracks.len() {
        let credits = tracks.get(i).map(Vec::as_slice).unwrap_or_default();
        let keys: Vec<String> = credits.iter().map(|c| normalize(c)).collect();
        for (key, name, _, run) in names.iter_mut() {
            if let Some(from) = run.take_if(|_| !keys.contains(key))
                && i - from > in_a_row
            {
                runs.push(format!(
                    "tracks {}-{i} are {}: Mixcloud allows {in_a_row} in a row {LIMITED}",
                    from + 1,
                    what(name)
                ));
            }
        }
        for (key, credit) in keys.into_iter().zip(credits) {
            match names.iter_mut().find(|n| n.0 == key) {
                Some((_, _, count, run)) => {
                    *count += 1;
                    run.get_or_insert(i);
                }
                None => names.push((key, credit.clone(), 1, Some(i))),
            }
        }
    }
    let mut out: Vec<String> = names
        .iter()
        .filter(|n| n.2 > most)
        .map(|(_, name, count, _)| format!("{count} tracks {}: Mixcloud allows {most} {LIMITED}", what(name)))
        .collect();
    out.extend(runs);
    out
}

#[cfg(test)]
mod tests;
