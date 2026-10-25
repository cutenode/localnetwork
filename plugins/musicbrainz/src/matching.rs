//! Which of the recordings an ISRC is a track is, and which of that recording's releases and
//! artists are its. The recording and the artists are only what MusicBrainz's answer leaves no
//! doubt about; the release is the earliest of the recording's releases named as the track's
//! album, one of its editions. Titles and names match as loclib-tracklist's `fold` has them (but
//! for case, accents, punctuation and spacing), lengths to the slack an imported tracklist's are.

use loclib_model::Track;
use loclib_model::model::artist_credits;
use loclib_tracklist::{DURATION_SLACK, fold};
use rustc_hash::FxHashSet;
use uuid::Uuid;

use crate::lookup::{NameCredit, Recording};

/// A track's MusicBrainz IDs: none at all unless its recording is known.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Ids {
    pub(crate) recording: Option<Uuid>,
    pub(crate) release: Option<Uuid>,
    /// One for each name [`artist_credits`] splits its artist into, in that order, or none.
    pub(crate) artists: Vec<Uuid>,
}

/// The IDs `track` has by the `recordings` its ISRC is.
pub(crate) fn ids(track: &Track, recordings: &[Recording]) -> Ids {
    let Some(r) = recording(track, recordings) else { return Ids::default() };
    Ids { recording: Some(r.id), release: release(track, r), artists: artists(track, r) }
}

fn same(a: &str, b: &str) -> bool {
    fold(a) == fold(b)
}

/// The one of `recordings` `track` is: the only one with its title and its length (which it
/// needs, to tell several apart). Even an only one has to have its length, or without one its
/// title: an extended mix or an edit can carry the original's ISRC.
fn recording<'r>(track: &Track, recordings: &'r [Recording]) -> Option<&'r Recording> {
    // Whether `r` is as long as the track, when both lengths are known.
    let near = |r: &Recording| {
        let secs = r.length.filter(|_| track.duration_secs > 0).map(|ms| (ms + 500) / 1000)?;
        Some(secs.abs_diff(track.duration_secs.into()) <= DURATION_SLACK.into())
    };
    let titled = |r: &Recording| same(&r.title, &track.title);
    if let [one] = recordings {
        return match near(one) {
            Some(near) => near.then_some(one),
            None => titled(one).then_some(one),
        };
    }
    let mut fit = recordings.iter().filter(|r| titled(r) && near(r) == Some(true));
    match (fit.next(), fit.next()) {
        (Some(r), None) => Some(r),
        _ => None,
    }
}

/// The release of `r` named as `track`'s album: the earliest when several are (its editions,
/// any of which is the album; the earliest is the one first out), the undated after the dated
/// (`2018` before `2018-03-09`), the first listed of those out the same day.
fn release(track: &Track, r: &Recording) -> Option<Uuid> {
    if track.album.trim().is_empty() {
        return None;
    }
    let named = r.releases.release.iter().filter(|x| same(&x.title, &track.album));
    named.min_by_key(|x| x.date.as_deref().filter(|d| !d.is_empty()).map_or((1, ""), |d| (0, d))).map(|x| x.id)
}

/// The artist of `r` each name `track`'s artist credits is (by the artist's name or the one it's
/// credited under): none unless every name is one artist, and each a different one, so every ID
/// goes with its name.
fn artists(track: &Track, r: &Recording) -> Vec<Uuid> {
    let credited = |name: &String| -> Option<Uuid> {
        let is = |c: &&NameCredit| same(&c.artist.name, name) || c.name.as_deref().is_some_and(|n| same(n, name));
        let mut found = r.credit.names.iter().filter(is).map(|c| c.artist.id);
        let id = found.next()?;
        found.all(|other| other == id).then_some(id)
    };
    let ids: Vec<Uuid> = artist_credits(&track.artist).iter().map(credited).collect::<Option<_>>().unwrap_or_default();
    let distinct = ids.iter().collect::<FxHashSet<_>>().len() == ids.len();
    if distinct { ids } else { Vec::new() }
}
