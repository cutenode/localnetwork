//! Building a set from sessions and/or a recording.
//!
//! Sessions give the tracks players logged, one session after another in the order they're
//! given: players have no clock, so nothing says which came first, and the DJ puts that right
//! afterwards. A recording ([`MixReport`]) beats them: it has start times, catches tracks played
//! for less than the minute a player needs to log one, and leaves out what was only heard in
//! headphones. When a set has both, the recording's tracklist is the set's, and the logged tracks
//! it doesn't hear are pointed out (likely previews).

use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use rustc_hash::FxHashSet;

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
pub(crate) mod tests {
    use super::*;
    use crate::mix::{MixInfo, Play};
    use crate::sets::Window;
    use loclib_model::test_util::track;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    pub(crate) fn entry(n: u32) -> SetEntry {
        let t = track(n, &format!("Artist {n}"), &format!("Track {n}"), &format!("/Contents/{n}.mp3"));
        SetEntry::from_track(&t, EntryOrigin::Session, None)
    }

    pub(crate) fn session(id: &str, tracks: &[u32], window: Window) -> Session {
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

    pub(crate) fn report(plays: &[(u32, f64, f64)]) -> MixReport {
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
        // Between 11:00 and 13:00 on this computer's clock, so on the day `d` in any time zone.
        let on = |d: &str| {
            let local = |h| day(d).and_hms_opt(h, 0, 0).unwrap().and_local_timezone(Local).unwrap().with_timezone(&Utc);
            Window { after: Some(local(11)), before: Some(local(13)), exact: false }
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
        assert_eq!(d, day("2026-10-03"));
        assert!(note.unwrap().contains("the day locallibrary first saw it"));

        let unknown = session("dddd", &[4], Window::default());
        let (d, note) = date(None, None, &[&unknown], today);
        assert_eq!(d, today);
        assert_eq!(note.as_deref(), Some("dated today (2026-10-06): give the day it was played"));
    }

    use hegel::TestCase;
    use hegel::generators as gs;

    /// Sessions of a few tracks each: imported logs starting within hours of each other, with
    /// their tracks' times, and history lists (players have no clock), which may still carry
    /// start times that mean nothing without a log's own.
    #[hegel::test_helper]
    fn some_sessions(tc: &TestCase) -> Vec<Session> {
        let night = at("2026-10-03T21:00:00Z");
        (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)))
            .map(|i| {
                let tracks = tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(9)).max_size(4));
                let window = match tc.draw(gs::booleans()) {
                    true => {
                        let start =
                            night + Duration::minutes(tc.draw(gs::integers::<i64>().min_value(0).max_value(300)));
                        Window { after: Some(start), before: None, exact: true }
                    }
                    false => Window::default(),
                };
                let mut s = session(&format!("s{i}"), &tracks, window);
                for e in &mut s.tracks {
                    e.start_seconds = tc.draw(gs::booleans()).then(|| tc.draw(gs::integers::<u32>().max_value(7200)));
                }
                s
            })
            .collect()
    }

    /// Sessions follow one another in the order given, each track kept as logged and marked with
    /// its session. Only a timed log's tracks are timed: the first timed play is at 0, and any
    /// two are as far apart as they were played. Appending sessions to a tracklist built from
    /// others gives what building from all of them at once does.
    #[hegel::test(test_cases = 300)]
    fn tracks_from_sessions_are_timed_from_the_first_play(tc: TestCase) {
        let sessions = some_sessions(&tc);
        let refs: Vec<&Session> = sessions.iter().collect();
        let built = from_sessions(&refs);
        let logged: Vec<(&Session, &SetEntry)> =
            sessions.iter().flat_map(|s| s.tracks.iter().map(move |e| (s, e))).collect();
        assert_eq!(built.len(), logged.len());
        // A timed log's window opens as its first track starts; its tracks' times count from there.
        let played: Vec<Option<DateTime<Utc>>> =
            logged.iter().map(|(s, e)| Some(s.window.after? + Duration::seconds(e.start_seconds?.into()))).collect();
        for ((b, (s, e)), at) in built.iter().zip(&logged).zip(&played) {
            let kept =
                SetEntry { start_seconds: e.start_seconds, origin: e.origin, session: e.session.clone(), ..b.clone() };
            assert_eq!(&kept, *e);
            assert_eq!((b.origin, b.session.as_deref()), (EntryOrigin::Session, Some(s.id.as_str())));
            assert_eq!(b.start_seconds.is_some(), at.is_some(), "{b:?}");
        }
        let timed: Vec<(i64, DateTime<Utc>)> =
            built.iter().zip(&played).filter_map(|(b, at)| Some((i64::from(b.start_seconds?), (*at)?))).collect();
        if let Some(&(first, t0)) = timed.iter().min_by_key(|(start, _)| *start) {
            assert_eq!(first, 0);
            for &(start, at) in &timed {
                assert_eq!(start, (at - t0).num_seconds());
            }
        }

        let split = tc.draw(gs::integers::<usize>().max_value(refs.len()));
        let (existing, added) = refs.split_at(split);
        let mut tracks = from_sessions(existing);
        append_sessions(&mut tracks, existing, added);
        assert_eq!(tracks, built);
    }

    /// A recording's tracklist is its plays by start, each at the whole second it starts, a
    /// tentative one noted; a track the DJ had unnamed stays unnamed; and each track the set had
    /// (by file, else by name, once each) that the recording doesn't hear is noted, a logged one
    /// as a likely preview.
    #[hegel::test(test_cases = 300)]
    fn a_recording_is_the_tracklist_and_notes_what_it_doesnt_hear(tc: TestCase) {
        let plays: Vec<(u32, f64, f64)> = (0..tc.draw(gs::integers::<usize>().max_value(5)))
            .map(|_| {
                let n = tc.draw(gs::integers::<u32>().min_value(1).max_value(6));
                let start = f64::from(tc.draw(gs::integers::<i32>().min_value(-30).max_value(4000))) / 4.0;
                (n, start, tc.draw(gs::sampled_from(vec![0.2, 0.49, 0.5, 0.9])))
            })
            .collect();
        let before: Vec<SetEntry> = (0..tc.draw(gs::integers::<usize>().max_value(5)))
            .map(|_| {
                let mut e = match tc.draw(gs::booleans()) {
                    true => entry(tc.draw(gs::integers::<u32>().min_value(1).max_value(8))),
                    false => SetEntry::added(tc.draw(gs::sampled_from(vec!["Someone - Dubplate", "ID"]))),
                };
                e.unnamed = tc.draw(gs::booleans());
                e.origin = if tc.draw(gs::booleans()) { EntryOrigin::Session } else { EntryOrigin::Added };
                e
            })
            .collect();
        let built = with_recording(&before, &report(&plays));

        let mut by_start = plays.clone();
        by_start.sort_by(|a, b| a.1.total_cmp(&b.1));
        let unnamed: Vec<Option<String>> =
            before.iter().filter(|e| e.unnamed && e.file.is_some()).map(|e| e.file.clone()).collect();
        let want: Vec<(String, Option<u32>, bool)> = by_start
            .iter()
            .map(|&(n, start, _)| {
                let file = Some(format!("/Contents/{n}.mp3"));
                (format!("Track {n}"), Some(start.max(0.0) as u32), unnamed.contains(&file))
            })
            .collect();
        let got: Vec<(String, Option<u32>, bool)> =
            built.tracks.iter().map(|e| (e.title.clone(), e.start_seconds, e.unnamed)).collect();
        assert_eq!(got, want);
        assert!(built.tracks.iter().all(|e| e.origin == EntryOrigin::Recording));

        // Tentative matches come first, by their place in the tracklist.
        let places: Vec<usize> = by_start.iter().enumerate().filter(|(_, p)| p.2 < 0.5).map(|(i, _)| i + 1).collect();
        assert!(built.notes.len() >= places.len(), "{:?}", built.notes);
        let (tentative, unheard) = built.notes.split_at(places.len());
        for (note, &i) in tentative.iter().zip(&places) {
            assert!(note.starts_with(&format!("{i}. {}: ", built.tracks[i - 1].name())), "{note}");
        }
        // Then each track the set had that isn't heard, the first time it comes up. Every track
        // here has a name of its own, so a name stands for its file.
        let heard: Vec<Option<String>> = built.tracks.iter().map(SetEntry::file_key).collect();
        let mut missed: Vec<&SetEntry> = Vec::new();
        for e in &before {
            if (e.file.is_none() || !heard.contains(&e.file_key())) && !missed.iter().any(|m| m.name() == e.name()) {
                missed.push(e);
            }
        }
        assert_eq!(unheard.len(), missed.len(), "{unheard:?}");
        for (note, e) in unheard.iter().zip(&missed) {
            assert!(note.starts_with(&format!("{}: ", e.name())), "{note}");
            assert_eq!(note.contains("logged"), e.origin == EntryOrigin::Session, "{note}");
        }
        // Each is told once, so a set listing its tracks twice is told the same.
        let twice: Vec<SetEntry> = before.iter().chain(&before).cloned().collect();
        assert_eq!(with_recording(&twice, &report(&plays)).notes, built.notes);
    }

    /// A set's day is the one given, else the recording's (when its tags give a whole day), else
    /// the day every session's window falls on; failing those, the first day one of them was seen
    /// (with a note), or today (with a note).
    #[hegel::test(test_cases = 300)]
    fn a_set_is_dated_by_what_knows_best_whatever_it_has(tc: TestCase) {
        let today = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
        let day = |d: u32| NaiveDate::from_ymd_opt(2026, 10, d).unwrap();
        let local = |d: u32, h: u32| {
            day(d).and_hms_opt(h, 0, 0).unwrap().and_local_timezone(Local).earliest().unwrap().with_timezone(&Utc)
        };
        let given = tc.draw(gs::booleans()).then(|| day(tc.draw(gs::integers::<u32>().min_value(1).max_value(5))));
        // A tag as given, and the day it gives.
        let (tag, tagged) = tc.draw(gs::sampled_from(vec![
            (None, None),
            (Some("2026-09-12T23:10:00"), Some((9, 12))),
            (Some("2026-09-13"), Some((9, 13))),
            (Some("2026"), None),
            (Some("12/09/2026"), None),
        ]));
        let recording = tc.draw(gs::booleans()).then(|| {
            let mut r = report(&[]);
            r.mix.date = tag.map(String::from);
            r
        });
        // Windows from one local day to another (the same one or a later one), unknown, or a
        // log's own, each with the day it falls on and the day it was first seen, when known.
        let n = tc.draw(gs::integers::<usize>().max_value(3));
        let (sessions, days): (Vec<Session>, Vec<_>) = (0..n)
            .map(|i| {
                let from = tc.draw(gs::integers::<u32>().min_value(1).max_value(4));
                let (window, on, seen) = match tc.draw(gs::integers::<u8>().max_value(2)) {
                    0 => (Window::default(), None, None),
                    1 => {
                        let to = from + tc.draw(gs::integers::<u32>().max_value(1));
                        let window = Window { after: Some(local(from, 11)), before: Some(local(to, 13)), exact: false };
                        (window, (to == from).then_some(from), Some(to))
                    }
                    // A night past midnight is the night it started.
                    _ => {
                        let window =
                            Window { after: Some(local(from, 22)), before: Some(local(from + 1, 3)), exact: true };
                        (window, Some(from), Some(from + 1))
                    }
                };
                (session(&format!("s{i}"), &[1], window), (on, seen))
            })
            .unzip();
        let refs: Vec<&Session> = sessions.iter().collect();
        let (d, note) = date(given, recording.as_ref(), &refs, today);
        let tagged = recording.as_ref().and(tagged).map(|(m, d)| NaiveDate::from_ymd_opt(2026, m, d).unwrap());
        if let Some(known) = given.or(tagged) {
            assert_eq!((d, note), (known, None));
            return;
        }
        match days.iter().map(|(on, _)| *on).collect::<Option<FxHashSet<u32>>>() {
            Some(on) if on.len() == 1 => assert_eq!((d, note), (day(on.into_iter().next().unwrap()), None)),
            _ => {
                let seen = days.iter().filter_map(|(_, seen)| seen.map(day)).min().unwrap_or(today);
                assert_eq!(d, seen);
                assert!(note.is_some_and(|n| n.contains(&seen.to_string()) && n.contains("give the day")));
            }
        }
    }
}
