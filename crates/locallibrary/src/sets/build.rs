//! Building a set from sessions and/or a recording.
//!
//! Sessions give the tracks players logged, one session after another in the order they're
//! given: players have no clock, so nothing says which came first, and the DJ puts that right
//! afterwards. A recording ([`MixReport`]) beats them: it has start times, catches tracks played
//! for less than the minute a player needs to log one, and leaves out what was only heard in
//! headphones. When a set has both, the recording's tracklist is the set's, and the logged tracks
//! it doesn't hear are pointed out (likely previews).

use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use rustc_hash::{FxHashMap, FxHashSet};

use super::{EntryOrigin, MusicBrainzIds, Session, SetEntry};
use crate::device::Device;
use crate::fields::roles;
use crate::hash::Hash;
use crate::mix::{MixReport, Play};
use crate::paths::FileClass;
use crate::store::Store;

/// A tracklist, and what's worth telling the DJ about it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Tracklist {
    pub tracks: Vec<SetEntry>,
    pub notes: Vec<String>,
}

/// The tracks `sessions` logged, one session after another in the order given. Tracks of a timed
/// log (an imported one) count their start from the first timed play of them all.
pub(crate) fn from_sessions(sessions: &[&Session]) -> Vec<SetEntry> {
    let origin = first_play(sessions);
    sessions.iter().flat_map(|s| entries(s, origin)).collect()
}

/// `tracks` (built from the sessions `existing`) with the tracks of `added` after them. Start
/// times keep counting from the first timed play of all the sessions, so when an added log
/// started earlier, the times already there move on by the difference.
pub(crate) fn append_sessions(tracks: &mut Vec<SetEntry>, existing: &[&Session], added: &[&Session]) {
    let all: Vec<&Session> = existing.iter().chain(added).copied().collect();
    let origin = first_play(&all);
    if let (Some(old), Some(new)) = (first_play(existing), origin) {
        let later = (old - new).num_seconds().max(0) as u32;
        for start in tracks.iter_mut().filter_map(|e| e.start_seconds.as_mut()) {
            *start += later;
        }
    }
    tracks.extend(added.iter().flat_map(|s| entries(s, origin)));
}

/// When the first timed play of `sessions` started.
fn first_play(sessions: &[&Session]) -> Option<DateTime<Utc>> {
    sessions.iter().flat_map(|s| s.tracks.iter().filter_map(|e| played_at(s, e))).min()
}

/// When `e` started, for a timed log: its window opens as the log's first track starts, and its
/// tracks' start times count from there.
fn played_at(s: &Session, e: &SetEntry) -> Option<DateTime<Utc>> {
    Some(s.window.after? + Duration::seconds(e.start_seconds?.into()))
}

fn entries(s: &Session, origin: Option<DateTime<Utc>>) -> impl Iterator<Item = SetEntry> + '_ {
    s.tracks.iter().map(move |e| SetEntry {
        start_seconds: origin.zip(played_at(s, e)).map(|(o, at)| (at - o).num_seconds().max(0) as u32),
        origin: EntryOrigin::Session,
        session: Some(s.id.clone()),
        ..e.clone()
    })
}

/// The tracks heard in `report`, in the order they start, each at the second it starts. A
/// tentative match goes in too, with a note to check it.
pub(crate) fn from_recording(report: &MixReport) -> Tracklist {
    let mut plays: Vec<&Play> = report.plays.iter().collect();
    plays.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut out = Tracklist::default();
    for (i, p) in plays.into_iter().enumerate() {
        let mut e = SetEntry::from_track(&p.track, EntryOrigin::Recording, None);
        e.start_seconds = Some(p.start_second());
        if p.is_tentative() {
            out.notes.push(format!(
                "{}. {}: a tentative match ({:.0}%), check it",
                i + 1,
                e.name(),
                p.confidence * 100.0
            ));
        }
        out.tracks.push(e);
    }
    out
}

/// Fill in the MusicBrainz IDs of the `tracks` that have none yet from their files, on the first
/// of `drives` that holds each: the file's tags first, and what they lack from the analyzers
/// chosen for [`MusicBrainzIds::ROLES`] (`sets publish` runs them), as they kept it in `store`
/// for the file, when it was worked out for the track as the set has it. A file no drive holds is
/// read from the store's copy ([`kept_audio`]): its tags only, since analyzers keep what they
/// worked out by the file on a drive. rekordbox keeps no MusicBrainz IDs, so only files have them.
pub(crate) fn read_musicbrainz<'e>(
    store: &Store,
    tracks: impl IntoIterator<Item = &'e mut SetEntry>,
    drives: &[Device],
) {
    let roles = roles::reader(store, &MusicBrainzIds::ROLES);
    let mut kept = None;
    for e in tracks.into_iter().filter(|e| e.musicbrainz.is_empty()) {
        let Some(key) = e.file.as_deref().map(crate::paths::key) else { continue };
        let Some(path) = drives.iter().filter_map(|d| d.checked_path(&key).ok()).find(|p| p.is_file()) else {
            let kept = kept.get_or_insert_with(|| kept_audio(store));
            if let Some(h) = kept.get(&crate::paths::fold(&key)) {
                e.musicbrainz = MusicBrainzIds::read(&store.object_path(h));
            }
            continue;
        };
        let heard = roles.as_ref().map(|read| MusicBrainzIds::from_roles(&read(&path), e)).unwrap_or_default();
        e.musicbrainz = MusicBrainzIds::read(&path).or(heard);
    }
}

/// The audio files the store keeps, by folded key ([`crate::paths::fold`], as drives compare
/// paths): the hash of its copy, as each library's latest snapshot has it. A path in several
/// libraries is the one changed most recently's.
fn kept_audio(store: &Store) -> FxHashMap<String, Hash> {
    let mut libraries = store.libraries().unwrap_or_default();
    libraries.sort_by_key(|l| std::cmp::Reverse(l.updated));
    let mut out = FxHashMap::default();
    for snapshot in libraries.iter().filter_map(|l| store.load_snapshot(&l.head?).ok()) {
        for f in snapshot.files.into_iter().filter(|f| f.class == FileClass::Audio) {
            if let Some(h) = f.hash {
                out.entry(crate::paths::fold(&f.path)).or_insert(h);
            }
        }
    }
    out
}

/// [`from_recording`], noting which of `before` (the set's tracks so far, or its sessions')
/// the recording doesn't hear. Tracks the DJ marked unnamed stay unnamed.
pub(crate) fn with_recording(before: &[SetEntry], report: &MixReport) -> Tracklist {
    let mut out = from_recording(report);
    let unnamed: FxHashSet<String> = before.iter().filter(|e| e.unnamed).filter_map(SetEntry::file_key).collect();
    for e in &mut out.tracks {
        e.unnamed = e.file_key().is_some_and(|k| unnamed.contains(&k));
    }
    let notes = not_heard(before, &out.tracks);
    out.notes.extend(notes);
    out
}

/// A note for each track of `logged` (by file, once each) that isn't among `heard`. Logged tracks
/// a recording doesn't hear were most likely previewed in headphones.
pub(crate) fn not_heard(logged: &[SetEntry], heard: &[SetEntry]) -> Vec<String> {
    let heard: FxHashSet<String> = heard.iter().filter_map(SetEntry::file_key).collect();
    let mut told = FxHashSet::default();
    let mut out = Vec::new();
    for e in logged {
        let key = e.file_key();
        if key.as_ref().is_some_and(|k| heard.contains(k)) || !told.insert(key.unwrap_or_else(|| e.name())) {
            continue;
        }
        out.push(match e.origin {
            EntryOrigin::Session => format!("{}: logged but not heard in the recording (previews?)", e.name()),
            _ => format!("{}: not heard in the recording", e.name()),
        });
    }
    out
}

/// The day a set was played: `given`, else the recording's own date, else the day all the
/// sessions' windows fall on. Failing those, the day locallibrary first saw one of the sessions
/// (the set was played before then), or `today`, with a note asking for the real one.
pub(crate) fn date(
    given: Option<NaiveDate>,
    recording: Option<&MixReport>,
    sessions: &[&Session],
    today: NaiveDate,
) -> (NaiveDate, Option<String>) {
    if let Some(d) = given.or_else(|| recording.and_then(recorded_on)) {
        return (d, None);
    }
    let days: Vec<Option<NaiveDate>> = sessions.iter().map(|s| s.window.day()).collect();
    if let Some(Some(d)) = days.first()
        && days.iter().all(|x| x == &Some(*d))
    {
        return (*d, None);
    }
    match sessions.iter().filter_map(|s| s.window.before).min() {
        Some(seen) => {
            let d = seen.with_timezone(&Local).date_naive();
            (d, Some(format!("dated {d}, the day locallibrary first saw it: give the day it was played")))
        }
        None => (today, Some(format!("dated today ({today}): give the day it was played"))),
    }
}

/// The day the recording's tags give, when they give a whole one (`2026-10-03`, or a time on it).
fn recorded_on(r: &MixReport) -> Option<NaiveDate> {
    let date = r.mix.date.as_deref()?.trim();
    NaiveDate::parse_from_str(date.get(..10)?, "%Y-%m-%d").ok()
}

#[cfg(test)]
pub(crate) mod tests;
