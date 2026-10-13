use super::*;
use crate::sets::build::tests::{report as mix_report, session};
use chrono::{DateTime, TimeZone, Utc};
use hegel::TestCase;
use hegel::generators as gs;

fn hour(h: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 10, 20, 0, 0).unwrap() + TimeDelta::hours(h)
}

/// Sessions of up to 5 tracks: lists a pull saved (a few pulls, so some were saved together),
/// lists only on a drive (recorded since its last pull), and imported logs with their own
/// times; some in a set already, some dismissed.
#[hegel::test_helper]
fn some_sessions(tc: &TestCase) -> Vec<Session> {
    let maybe_hour = |hours: Vec<Option<i64>>| tc.draw(gs::sampled_from(hours)).map(hour);
    (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(6)))
        .map(|i| {
            let tracks = tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(6)).max_size(5));
            let mut s = session(&format!("s{i}"), &tracks, Window::default());
            let drive = tc.draw(gs::sampled_from(vec!["Main", "Backup"])).to_string();
            match tc.draw(gs::integers::<u8>().max_value(2)) {
                0 => {
                    s.first_seen_on = Some(drive);
                    let before = maybe_hour(vec![Some(12), Some(24), Some(30), Some(60)]);
                    let after = maybe_hour(vec![None, Some(0), Some(6), Some(18)]);
                    s.window = Window { after, before, exact: false };
                }
                1 => {
                    s.drives = vec![drive];
                    s.window = Window {
                        after: maybe_hour(vec![None, Some(20), Some(28)]),
                        before: Some(hour(34)),
                        exact: false,
                    };
                }
                _ => {
                    let start = tc.draw(gs::integers::<i64>().min_value(0).max_value(40));
                    let end = start + tc.draw(gs::integers::<i64>().min_value(0).max_value(6));
                    s.window = Window { after: Some(hour(start)), before: Some(hour(end)), exact: true };
                }
            }
            s.sets = (tc.draw(gs::integers::<u8>().max_value(4)) == 0)
                .then(|| "3m2abcdefgh22".to_string())
                .into_iter()
                .collect();
            s.dismissed = tc.draw(gs::integers::<u8>().max_value(4)) == 0;
            s
        })
        .collect()
}

/// Stores to suggest from, made once (writing one takes a while): with no recording, and with
/// a recording of tracks 1 to 3, or 2 to 6.
fn stores() -> &'static [(Vec<u32>, Store, tempfile::TempDir); 3] {
    static STORES: std::sync::OnceLock<[(Vec<u32>, Store, tempfile::TempDir); 3]> = std::sync::OnceLock::new();
    STORES.get_or_init(|| {
        [vec![], vec![1, 2, 3], vec![2, 3, 4, 5, 6]].map(|heard| {
            let dir = tempfile::tempdir().unwrap();
            let store = Store::open_or_init(dir.path().join("store")).unwrap();
            if !heard.is_empty() {
                let plays: Vec<(u32, f64, f64)> = heard.iter().map(|&n| (n, f64::from(n) * 300.0, 0.9)).collect();
                report::save(&store, &mix_report(&plays)).unwrap();
            }
            (heard, store, dir)
        })
    })
}

/// What the sessions' windows span: unknown at an end where any of them is.
fn spanned(group: &[&Session]) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    let after = match group.iter().all(|s| s.window.after.is_some()) {
        true => group.iter().filter_map(|s| s.window.after).min(),
        false => None,
    };
    let before = match group.iter().all(|s| s.window.before.is_some()) {
        true => group.iter().filter_map(|s| s.window.before).max(),
        false => None,
    };
    (after, before)
}

/// Whether the sessions surely were one night: imported logs always are; else what they span
/// is known at both ends and falls on one local day or within 36 hours.
fn is_one_night(group: &[&Session]) -> bool {
    let day = |t: DateTime<Utc>| t.with_timezone(&chrono::Local).date_naive();
    match spanned(group) {
        (Some(_), _) if group.iter().all(|s| s.window.exact) => true,
        (Some(a), Some(b)) => day(a) == day(b) || b - a <= TimeDelta::hours(36),
        _ => false,
    }
}

/// Candidates are made of the sessions in no set and not dismissed, each in one at most:
/// every such session of 3 tracks or more is in one, and every candidate has one. Sessions
/// that go together (both only on drives, which nothing here tells apart, or both saved or
/// imported, by the same pull or with windows that overlap) are in the same candidate unless
/// their night can't be told, when each is its own; only then does a candidate say so. Each
/// is scored for its different tracks, a recording hearing 3 or more of them and drives on
/// the same night, is dated by the imported logs' times or the time its windows share, and
/// they come newest first.
#[hegel::test(test_cases = 150)]
fn suggestions_group_what_goes_together_and_score_the_evidence(tc: TestCase) {
    let (heard, store, _) = &stores()[tc.draw(gs::integers::<usize>().max_value(2))];
    let sessions = some_sessions(&tc);
    let got = suggest(Some(store), &sessions, &[]).unwrap();

    let by_id = |id: &String| sessions.iter().find(|s| &s.id == id).unwrap();
    let is_open = |s: &Session| s.sets.is_empty() && !s.dismissed;
    let candidate_of = |s: &Session| got.iter().position(|c| c.sessions.contains(&s.id));
    let mut seen = Vec::new();
    for c in &got {
        for id in &c.sessions {
            assert!(is_open(by_id(id)), "{id} is in a set or dismissed");
            assert!(!seen.contains(id), "{id} is in two candidates");
            seen.push(id.clone());
        }
        assert!(c.sessions.iter().any(|id| by_id(id).tracks.len() >= MIN_TRACKS));
    }
    for s in sessions.iter().filter(|s| is_open(s) && s.tracks.len() >= MIN_TRACKS) {
        assert!(candidate_of(s).is_some(), "{} isn't suggested", s.id);
    }
    for w in got.windows(2) {
        assert!((w[0].window.before, w[0].window.after) >= (w[1].window.before, w[1].window.after), "newest first");
    }

    let unsaved = |s: &Session| s.first_seen_on.is_none() && !s.window.exact;
    let overlaps = |a: &Window, b: &Window| match (a.after, a.before, b.after, b.before) {
        (Some(a0), Some(a1), Some(b0), Some(b1)) => a0 < b1 && b0 < a1,
        _ => false,
    };
    let together = |a: &Session, b: &Session| match (unsaved(a), unsaved(b)) {
        (true, true) => true,
        (false, false) => {
            let same_pull =
                a.first_seen_on.is_some() && a.first_seen_on == b.first_seen_on && a.window.before == b.window.before;
            same_pull || overlaps(&a.window, &b.window)
        }
        _ => false,
    };
    for a in &sessions {
        for b in &sessions {
            let (Some(ca), Some(cb)) = (candidate_of(a), candidate_of(b)) else { continue };
            if together(a, b) && ca != cb {
                assert!(
                    got[ca].sessions.len() == 1 && got[cb].sessions.len() == 1,
                    "{} and {} go together",
                    a.id,
                    b.id
                );
            }
        }
    }

    for c in &got {
        let group: Vec<&Session> = c.sessions.iter().map(by_id).collect();
        let one_night = is_one_night(&group);
        assert!(one_night || group.len() == 1, "{:?} can't be told to be one night", c.sessions);
        assert_eq!(c.reasons.contains(&WHICH_NIGHT.to_string()), !one_night);

        let files: FxHashSet<String> = group.iter().flat_map(|s| &s.tracks).filter_map(|e| e.file_key()).collect();
        assert_eq!(c.tracks, files.len());
        let recorded: FxHashSet<String> = heard.iter().map(|n| format!("contents/{n}.mp3")).collect();
        let heard_here = files.intersection(&recorded).count();
        let drives: FxHashSet<&String> =
            group.iter().filter_map(|s| s.first_seen_on.as_ref().or(s.drives.first())).collect();
        let score = files.len().min(40) as u32
            + if heard_here >= MIN_TRACKS { 30 } else { 0 }
            + if drives.len() > 1 { 10 } else { 0 };
        assert_eq!(c.score, score, "{:?}", c.reasons);
        let tracks = match files.len() {
            1 => "1 track: at least a minute of play".to_string(),
            n => format!("{n} tracks: at least {n} minutes of play"),
        };
        assert_eq!(c.reasons[0], tracks);
        let recording = format!("the recording Friday has {heard_here} of its tracks");
        assert_eq!(c.reasons.contains(&recording), heard_here >= MIN_TRACKS);
        let several = format!("{} drives on the same night", drives.len());
        assert_eq!(c.reasons.contains(&several), drives.len() > 1);

        let exact: Vec<&Window> = group.iter().map(|s| &s.window).filter(|w| w.exact).collect();
        let window = match exact.is_empty() {
            false => Window {
                after: exact.iter().filter_map(|w| w.after).min(),
                before: exact.iter().filter_map(|w| w.before).max(),
                exact: true,
            },
            true => {
                let after = group.iter().filter_map(|s| s.window.after).max();
                let before = group.iter().filter_map(|s| s.window.before).min();
                match after.zip(before) {
                    Some((a, b)) if a >= b => {
                        let (after, before) = spanned(&group);
                        Window { after, before, exact: false }
                    }
                    _ => Window { after, before, exact: false },
                }
            }
        };
        assert_eq!(c.window, window);
    }
}
