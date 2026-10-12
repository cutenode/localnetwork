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
mod tests;
