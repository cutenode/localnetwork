//! Building a set from sessions and/or a recording.
//!
//! Sessions give the tracks players logged, one session after another in the order they're
//! given: players have no clock, so nothing says which came first, and the DJ puts that right
//! afterwards. A recording ([`MixReport`]) beats them: it has start times, catches tracks played
//! for less than the minute a player needs to log one, and leaves out what was only heard in
//! headphones. When a set has both, the recording's tracklist is the set's, and the logged tracks
//! it doesn't hear are pointed out (likely previews).

use std::collections::HashSet;

use chrono::{DateTime, Duration, Local, NaiveDate, Utc};

use super::{EntryOrigin, Session, SetEntry};
use crate::mix::{MixReport, Play};

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
        e.start_seconds = Some(p.start.max(0.0).floor() as u32);
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

/// [`from_recording`], noting which of `before` (the set's tracks so far, or its sessions')
/// the recording doesn't hear. Tracks the DJ marked unnamed stay unnamed.
pub(crate) fn with_recording(before: &[SetEntry], report: &MixReport) -> Tracklist {
    let mut out = from_recording(report);
    let unnamed: HashSet<String> = before.iter().filter(|e| e.unnamed).filter_map(SetEntry::file_key).collect();
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
    let heard: HashSet<String> = heard.iter().filter_map(SetEntry::file_key).collect();
    let mut told = HashSet::new();
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
mod tests {
    use super::*;
    use crate::mix::{MixInfo, Play};
    use crate::model::tests::track;
    use crate::sets::Window;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn entry(n: u32) -> SetEntry {
        let t = track(n, &format!("Artist {n}"), &format!("Track {n}"), &format!("/Contents/{n}.mp3"));
        SetEntry::from_track(&t, EntryOrigin::Session, None)
    }

    fn session(id: &str, tracks: &[u32], window: Window) -> Session {
        Session {
            id: id.into(),
            name: "HISTORY 001".into(),
            path: "HISTORY 001".into(),
            source: "device_library".into(),
            drives: Vec::new(),
            first_seen_on: None,
            window,
            tracks: tracks.iter().map(|&n| entry(n)).collect(),
            missing: 0,
            sets: Vec::new(),
            dismissed: false,
        }
    }

    /// An imported log that started at `start` and played `tracks` at these seconds from it.
    fn timed(id: &str, start: &str, tracks: &[(u32, u32)]) -> Session {
        let mut s = session(id, &[], Window { after: Some(at(start)), before: None, exact: true });
        s.tracks = tracks.iter().map(|&(n, secs)| SetEntry { start_seconds: Some(secs), ..entry(n) }).collect();
        s
    }

    fn report(plays: &[(u32, f64, f64)]) -> MixReport {
        let play = |&(n, start, confidence): &(u32, f64, f64)| Play {
            track: track(n, &format!("Artist {n}"), &format!("Track {n}"), &format!("/Contents/{n}.mp3")),
            start,
            end: start + 200.0,
            track_start: 0.0,
            track_end: 200.0,
            tempo: 1.0,
            played_bpm: None,
            key_lock: None,
            semitones: 0.0,
            key: None,
            played_key: None,
            confidence,
            votes: 100,
            presence: Vec::new(),
            presence_step: 2.0,
            jumps: 0,
            alternates: Vec::new(),
        };
        MixReport {
            mix: MixInfo {
                path: "/mixes/friday.wav".into(),
                kind: "audio",
                title: "Friday".into(),
                artist: None,
                genre: None,
                date: None,
                recorder: None,
                duration_secs: 3600.0,
                sample_rate: 44100,
                channels: 2,
                labels: Vec::new(),
                cover: None,
            },
            drive: "DNA".into(),
            drive_id: None,
            library: Default::default(),
            plays: plays.iter().map(play).collect(),
            transitions: Vec::new(),
            unidentified: Vec::new(),
            coverage: 0.9,
            waveform: Default::default(),
            tempo_range: 0.16,
            analysed_at: Utc::now(),
            elapsed_secs: 1.0,
        }
    }

    fn names(tracks: &[SetEntry]) -> Vec<String> {
        tracks.iter().map(|e| e.title.clone()).collect()
    }

    #[test]
    fn sessions_follow_one_another_in_the_order_given() {
        let (a, b) = (session("aaaa", &[1, 2], Window::default()), session("bbbb", &[3], Window::default()));
        let tracks = from_sessions(&[&b, &a]);
        assert_eq!(names(&tracks), ["Track 3", "Track 1", "Track 2"]);
        assert_eq!(tracks[0].session.as_deref(), Some("bbbb"));
        assert!(tracks.iter().all(|e| e.origin == EntryOrigin::Session && e.start_seconds.is_none()));
    }

    #[test]
    fn timed_logs_count_from_the_first_play_of_the_set() {
        let late = timed("late", "2026-10-03T22:00:00Z", &[(3, 0), (4, 300)]);
        let early = timed("early", "2026-10-03T21:00:00Z", &[(1, 60), (2, 400)]);
        let untimed = session("cccc", &[5], Window::default());
        let tracks = from_sessions(&[&late, &early, &untimed]);
        let starts: Vec<Option<u32>> = tracks.iter().map(|e| e.start_seconds).collect();
        assert_eq!(starts, [Some(3540), Some(3840), Some(0), Some(340), None]);

        // Appending the earlier log to a set built from the later one moves its times on.
        let mut tracks = from_sessions(&[&late]);
        assert_eq!(tracks[0].start_seconds, Some(0));
        append_sessions(&mut tracks, &[&late], &[&early]);
        let starts: Vec<Option<u32>> = tracks.iter().map(|e| e.start_seconds).collect();
        assert_eq!(starts, [Some(3540), Some(3840), Some(0), Some(340)]);
    }

    #[test]
    fn a_recording_lists_its_plays_by_start_and_notes_tentative_ones() {
        let r = report(&[(2, 300.9, 0.9), (1, 4.2, 0.3)]);
        let built = from_recording(&r);
        assert_eq!(names(&built.tracks), ["Track 1", "Track 2"]);
        assert_eq!(built.tracks.iter().map(|e| e.start_seconds).collect::<Vec<_>>(), [Some(4), Some(300)]);
        assert!(built.tracks.iter().all(|e| e.origin == EntryOrigin::Recording));
        assert_eq!(built.notes, ["1. Artist 1 - Track 1: a tentative match (30%), check it"]);
    }

    #[test]
    fn the_recording_beats_the_sessions() {
        let mut logged = from_sessions(&[&session("aaaa", &[1, 9, 2, 9], Window::default())]);
        logged[2].unnamed = true;
        let built = with_recording(&logged, &report(&[(1, 0.0, 0.9), (3, 200.0, 0.9), (2, 400.0, 0.9)]));
        assert_eq!(names(&built.tracks), ["Track 1", "Track 3", "Track 2"], "heard but not logged goes in");
        assert_eq!(built.notes, ["Artist 9 - Track 9: logged but not heard in the recording (previews?)"]);
        assert!(built.tracks[2].unnamed, "an unnamed track stays unnamed");
        assert!(!built.tracks[0].unnamed);
        let typed = SetEntry::added("Someone - Dubplate");
        assert_eq!(not_heard(&[typed], &built.tracks), ["Someone - Dubplate: not heard in the recording"]);
    }

    #[test]
    fn a_set_is_dated_by_what_knows_best() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
        let day = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        let on = |d: &str| Window {
            after: Some(at(&format!("{d}T11:00:00Z"))),
            before: Some(at(&format!("{d}T13:00:00Z"))),
            exact: false,
        };
        let (a, b) = (session("aaaa", &[1], on("2026-10-03")), session("bbbb", &[2], on("2026-10-03")));
        let mut r = report(&[]);
        r.mix.date = Some("2026-09-12T23:10:00".into());

        assert_eq!(date(Some(day("2026-01-01")), Some(&r), &[&a], today), (day("2026-01-01"), None));
        assert_eq!(date(None, Some(&r), &[&a], today), (day("2026-09-12"), None));
        r.mix.date = Some("2026".into());
        assert_eq!(date(None, Some(&r), &[&a, &b], today), (day("2026-10-03"), None), "a year alone isn't a day");

        // Played before the first of them was seen.
        let later = session("cccc", &[3], on("2026-10-05"));
        let (d, note) = date(None, None, &[&later, &a], today);
        assert_eq!(d, at("2026-10-03T13:00:00Z").with_timezone(&Local).date_naive());
        assert!(note.unwrap().contains("the day locallibrary first saw it"));

        let unknown = session("dddd", &[4], Window::default());
        let (d, note) = date(None, None, &[&unknown], today);
        assert_eq!(d, today);
        assert_eq!(note.as_deref(), Some("dated today (2026-10-06): give the day it was played"));
    }
}
