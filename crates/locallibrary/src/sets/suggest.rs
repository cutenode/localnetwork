//! Sets the sessions suggest: sessions from one night grouped, with the reasons they look like a
//! performance.

use chrono::TimeDelta;
use rustc_hash::{FxHashMap, FxHashSet};
use schemars::JsonSchema;
use serde::Serialize;

use crate::Result;
use crate::device::Device;
use crate::fleet::Role;
use crate::mix::report;
use crate::model::normalize;
use crate::plays;
use crate::sets::{Session, Window};
use crate::store::Store;

/// Sessions that look like one set.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Candidate {
    /// Their ids, newest first.
    pub sessions: Vec<String>,
    /// When it was played: the imported logs' times when there are some, else the time every
    /// session's window shares.
    pub window: Window,
    /// Different tracks the sessions logged.
    pub tracks: usize,
    /// How much it looks like a set, 0 to 100: up to 40 for its tracks, 20 for a drive taken to
    /// sets, 30 for a recording that has its tracks, 10 for several drives.
    pub score: u32,
    /// The evidence, in words.
    pub reasons: Vec<String>,
}

/// Sessions shorter than this are track checks: they join a night's group but never make one.
const MIN_TRACKS: usize = 3;

/// The longest sessions can span and still be one night: a gig, and the pull the day after.
const ONE_NIGHT: TimeDelta = TimeDelta::hours(36);

/// Said of a session whose window is too wide to tell which night it was.
const WHICH_NIGHT: &str = "locallibrary can't tell which night: pull after each gig, or run `watch --pull`";

/// The sets `sessions` suggest, newest first: the ones in no set that the DJ didn't dismiss,
/// grouped by night. Sessions saved in the store (or imported) go together when a pull first
/// saved them together or when their windows overlap, and sessions only on `drives` per library
/// (they were all recorded since the drives were last pulled), but only when what their windows
/// span fits one night: wider windows say nothing about which night each session was (a first
/// pull's lists could be from any time before it), so each long enough is a candidate of its own.
pub(crate) fn suggest(store: &Store, sessions: &[Session], drives: &[Device]) -> Result<Vec<Candidate>> {
    let open: Vec<&Session> = sessions.iter().filter(|s| s.sets.is_empty() && !s.dismissed).collect();
    let library: FxHashMap<String, String> = drives
        .iter()
        .map(|d| (d.name(), d.state().and_then(|s| s.library_id).map_or_else(|| d.name(), |l| l.to_string())))
        .collect();
    let unsaved = |s: &Session| s.first_seen_on.is_none() && !s.window.exact && !s.drives.is_empty();
    let together = |a: &Session, b: &Session| match (unsaved(a), unsaved(b)) {
        (true, true) => a.drives.iter().any(|x| b.drives.iter().any(|y| library.get(x) == library.get(y))),
        (false, false) => {
            let same_pull =
                a.first_seen_on.is_some() && (&a.first_seen_on, a.window.before) == (&b.first_seen_on, b.window.before);
            same_pull || overlap(&a.window, &b.window)
        }
        _ => false,
    };
    let mut parent: Vec<usize> = (0..open.len()).collect();
    for i in 0..open.len() {
        for j in 0..i {
            if together(open[i], open[j]) {
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                parent[a] = b;
            }
        }
    }
    // In the order of their newest sessions, so candidates from one window keep that order.
    let (mut groups, mut slot): (Vec<Vec<&Session>>, FxHashMap<usize, usize>) = (Vec::new(), FxHashMap::default());
    for (i, s) in open.iter().enumerate() {
        let g = *slot.entry(root(&mut parent, i)).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[g].push(s);
    }

    let set_drives: FxHashSet<String> =
        store.drives()?.into_values().filter(|r| r.role == Some(Role::Set)).map(|r| r.name).collect();
    // A report that can't be read is no evidence either way.
    let recordings: Vec<(String, FxHashSet<String>)> = report::list(store.root())?
        .into_iter()
        .filter_map(|r| report::load(store.root(), &r.id).ok())
        .map(|r| (r.mix.title, r.plays.iter().map(|p| plays::file_key(&p.track.file_path)).collect()))
        .collect();

    let nights = groups.into_iter().flat_map(|g| match one_night(&g) {
        true => vec![g],
        false => g.into_iter().map(|s| vec![s]).collect(),
    });
    let mut out: Vec<Candidate> = nights
        .filter(|g| g.iter().any(|s| s.tracks.len() >= MIN_TRACKS))
        .map(|g| candidate(&g, &set_drives, &recordings))
        .collect();
    out.sort_by_key(|c| std::cmp::Reverse((c.window.before, c.window.after)));
    Ok(out)
}

fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// Whether two windows known at both ends share a moment.
fn overlap(a: &Window, b: &Window) -> bool {
    matches!((a.after, a.before, b.after, b.before), (Some(a0), Some(a1), Some(b0), Some(b1)) if a0 < b1 && b0 < a1)
}

fn candidate(
    group: &[&Session],
    set_drives: &FxHashSet<String>,
    recordings: &[(String, FxHashSet<String>)],
) -> Candidate {
    let entries = group.iter().flat_map(|s| &s.tracks);
    let files: FxHashSet<String> = entries.clone().filter_map(|e| e.file_key()).collect();
    let tracks = entries.map(|e| e.file_key().unwrap_or_else(|| normalize(&e.name()))).collect::<FxHashSet<_>>().len();
    let window = combined(group);
    let mut score = tracks.min(40) as u32;
    // Players log a track once it has played for about a minute.
    let mut reasons = vec![match tracks {
        1 => "1 track: at least a minute of play".to_string(),
        n => format!("{n} tracks: at least {n} minutes of play"),
    }];
    // The drive each session was recorded on, as far as can be told: a list copied to another
    // drive is still one drive's.
    let drives: FxHashSet<&String> =
        group.iter().filter_map(|s| s.first_seen_on.as_ref().or(s.drives.first())).collect();
    if group.iter().flat_map(|s| s.drives.iter().chain(&s.first_seen_on)).any(|d| set_drives.contains(d)) {
        score += 20;
        reasons.push("from a drive you take to sets".into());
    }
    let heard =
        recordings.iter().map(|(title, keys)| (title, keys.intersection(&files).count())).max_by_key(|(_, n)| *n);
    if let Some((title, n)) = heard.filter(|(_, n)| *n >= MIN_TRACKS) {
        score += 30;
        reasons.push(format!("the recording {title} has {n} of its tracks"));
    }
    if drives.len() > 1 {
        score += 10;
        reasons.push(format!("{} drives on the same night", drives.len()));
    }
    if !one_night(group) {
        reasons.push(WHICH_NIGHT.into());
    }
    Candidate { sessions: group.iter().map(|s| s.id.clone()).collect(), window, tracks, score, reasons }
}

/// When sessions of one night were played: the imported logs' own times when there are some;
/// else the time every window shares, or what they span when they share none (a chain of windows
/// each overlapping the next).
fn combined(group: &[&Session]) -> Window {
    let exact: Vec<&Window> = group.iter().map(|s| &s.window).filter(|w| w.exact).collect();
    if !exact.is_empty() {
        let after = exact.iter().filter_map(|w| w.after).min();
        return Window { after, before: exact.iter().filter_map(|w| w.before).max(), exact: true };
    }
    let after = group.iter().filter_map(|s| s.window.after).max();
    let before = group.iter().filter_map(|s| s.window.before).min();
    if after.zip(before).is_none_or(|(a, b)| a < b) {
        return Window { after, before, exact: false };
    }
    span(group)
}

/// What the sessions' windows cover together, from the earliest start to the latest end: unknown
/// at an end where any of them is (`None` sorts first).
fn span(group: &[&Session]) -> Window {
    Window {
        after: group.iter().map(|s| s.window.after).min().flatten(),
        before: group.iter().map(|s| s.window.before).collect::<Option<Vec<_>>>().and_then(|b| b.into_iter().max()),
        exact: group.iter().all(|s| s.window.exact),
    }
}

/// Whether the sessions were surely played on one night: what their windows span is known at both
/// ends, and falls on one day or within [`ONE_NIGHT`] (imported logs' own times always do).
fn one_night(group: &[&Session]) -> bool {
    let w = span(group);
    w.day().is_some() || w.after.zip(w.before).is_some_and(|(a, b)| b - a <= ONE_NIGHT)
}

#[cfg(test)]
mod tests {
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
        let got = suggest(store, &sessions, &[]).unwrap();

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
                let same_pull = a.first_seen_on.is_some()
                    && a.first_seen_on == b.first_seen_on
                    && a.window.before == b.window.before;
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
}
