//! Which snapshots `store prune` keeps.
//!
//! Every pull adds a snapshot, so a store only grows. Pruning keeps, in each library:
//!
//! * its latest snapshot, and labelled ones;
//! * the state each registered drive (retired ones aside) was last seen holding: `push` overwrites
//!   a drive without `--force` only when its state is saved, `drive list` says how far behind a
//!   drive is from it, and a drive back from a set is told apart from a new export by the last
//!   state it shared with the other drive ([`crate::after_set`]);
//! * the newest snapshot holding each history list (what players recorded at a set), so
//!   `history --all` and the DJ profile keep every list the store has seen;
//! * what the policy asks for: the newest N, and the newest of each of the last N days and weeks
//!   that have snapshots (in local time);
//!
//! and removes the rest. [`plan`] says which go and why the others stay; [`apply`] removes
//! exactly those. What's removed keeps its parents ([`Store::forget_snapshots`]), so history
//! walks through it, and [`Store::gc`] then frees the files no snapshot left uses (it never
//! removes a snapshot of a library, so it keeps every snapshot the plan keeps). Which of two
//! drives is newer comes from the lineage each drive keeps, which pruning doesn't touch.

use chrono::{Datelike, Local};
use rustc_hash::{FxHashMap, FxHashSet};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::doctor::Databases;
use crate::hash::Hash;
use crate::paths;
use crate::plays;
use crate::progress::{Ctx, Event, Phase};
use crate::store::{SnapshotInfo, Store};
use crate::{Error, Result};

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct PruneOptions {
    /// Keep each library's newest N snapshots.
    #[serde(default)]
    pub keep_last: Option<usize>,
    /// Keep the newest snapshot of each of the last N days that have one.
    #[serde(default)]
    pub keep_daily: Option<usize>,
    /// Keep the newest snapshot of each of the last N weeks that have one.
    #[serde(default)]
    pub keep_weekly: Option<usize>,
    /// Only this library (name or id) [default: every library].
    #[serde(default)]
    pub library: Option<String>,
    /// Say what would go, and remove nothing (for callers that plan and prune in one go).
    #[serde(default)]
    pub dry_run: bool,
}

/// Why a snapshot stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KeepReason {
    /// The library's latest.
    Latest,
    Label,
    /// A drive was last seen holding it.
    Drive,
    /// The newest snapshot holding a history list.
    History,
    /// Its databases couldn't be read, so what history lists it holds isn't known.
    Unreadable,
    Last,
    Daily,
    Weekly,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct KeptSnapshot {
    pub snapshot: SnapshotInfo,
    pub reasons: Vec<KeepReason>,
    /// With `drive`: the drives last seen holding it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub drives: Vec<String>,
    /// With `history`: the history lists no newer kept snapshot holds.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LibraryPrune {
    pub library: String,
    /// Newest first.
    pub kept: Vec<KeptSnapshot>,
    /// Newest first.
    pub removed: Vec<SnapshotInfo>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PruneReport {
    pub libraries: Vec<LibraryPrune>,
    pub removed_snapshots: usize,
    /// Stored files removed: no snapshot left uses them. With `dry_run`, the ones that would
    /// be.
    pub removed_objects: usize,
    pub freed_bytes: u64,
    pub dry_run: bool,
    pub warnings: Vec<String>,
}

/// A snapshot and why it stays (no reasons: it goes).
struct Candidate {
    info: SnapshotInfo,
    reasons: Vec<KeepReason>,
    drives: Vec<String>,
    history: Vec<String>,
}

impl Candidate {
    fn keep(&mut self, reason: KeepReason) {
        if !self.reasons.contains(&reason) {
            self.reasons.push(reason);
        }
    }
}

/// Which snapshots the policy removes and why the rest stay, and the files that frees (as
/// [`PruneReport`] with `dry_run`). Nothing is changed.
pub fn plan(store: &Store, opts: &PruneOptions, ctx: &Ctx<'_>) -> Result<PruneReport> {
    if opts.keep_last.is_none() && opts.keep_daily.is_none() && opts.keep_weekly.is_none() {
        return Err(Error::invalid("say what to keep: the last N snapshots, or N days' or weeks'"));
    }
    let libraries = match &opts.library {
        Some(spec) => vec![
            store.find_library(spec.trim())?.ok_or_else(|| Error::NotFound(format!("library {:?}", spec.trim())))?,
        ],
        None => store.libraries()?,
    };
    let labels = store.labels_by_snapshot()?;
    let mut by_library: FxHashMap<uuid::Uuid, Vec<Candidate>> = FxHashMap::default();
    for id in store.snapshot_ids()? {
        let mut info = store.load_info(&id)?;
        info.labels = labels.get(&id).cloned().unwrap_or_default();
        let library = info.library_id;
        let c = Candidate { info, reasons: Vec::new(), drives: Vec::new(), history: Vec::new() };
        by_library.entry(library).or_default().push(c);
    }
    let mut drive_states: FxHashMap<Hash, Vec<String>> = FxHashMap::default();
    for d in store.drives()?.into_values().filter(|d| d.log.retired.is_none()) {
        if let Some(fp) = d.last_fingerprint {
            drive_states.entry(fp).or_default().push(d.name);
        }
    }

    let mut report = PruneReport {
        libraries: Vec::new(),
        removed_snapshots: 0,
        removed_objects: 0,
        freed_bytes: 0,
        dry_run: true,
        warnings: Vec::new(),
    };
    let mut doomed = FxHashSet::default();
    for lib in libraries {
        let mut snaps = by_library.remove(&lib.id).unwrap_or_default();
        snaps.sort_by(|a, b| b.info.created.cmp(&a.info.created).then(b.info.id.cmp(&a.info.id)));
        by_policy(&mut snaps, lib.head, &drive_states, opts);
        protect_history(store, &mut snaps, ctx, &mut report.warnings)?;
        let (kept, removed): (Vec<Candidate>, Vec<Candidate>) = snaps.into_iter().partition(|c| !c.reasons.is_empty());
        doomed.extend(removed.iter().map(|c| c.info.id));
        report.libraries.push(LibraryPrune {
            library: lib.name,
            kept: kept
                .into_iter()
                .map(|c| KeptSnapshot { snapshot: c.info, reasons: c.reasons, drives: c.drives, history: c.history })
                .collect(),
            removed: removed.into_iter().map(|c| c.info).collect(),
        });
    }
    report.removed_snapshots = doomed.len();
    if !doomed.is_empty() {
        let garbage = store.garbage(&doomed)?;
        report.removed_objects = garbage.objects.len();
        report.freed_bytes = garbage.objects.iter().map(|(_, size)| size).sum();
    }
    Ok(report)
}

/// Remove the snapshots `plan` lists as removed, then the files nothing uses any more.
pub fn apply(store: &Store, plan: &PruneReport) -> Result<PruneReport> {
    let _lock = store.lock()?;
    let doomed: Vec<Hash> = plan.libraries.iter().flat_map(|l| l.removed.iter().map(|s| s.id)).collect();
    let mut report = PruneReport { dry_run: false, removed_objects: 0, freed_bytes: 0, ..plan.clone() };
    if !doomed.is_empty() {
        store.forget_snapshots(&doomed)?;
        let gc = store.gc(false)?;
        (report.removed_objects, report.freed_bytes) = (gc.removed_objects, gc.freed_bytes);
    }
    Ok(report)
}

/// Mark what stays by the library's head, labels, drives' states and the policy. `snaps` is
/// newest first.
fn by_policy(
    snaps: &mut [Candidate],
    head: Option<Hash>,
    drive_states: &FxHashMap<Hash, Vec<String>>,
    opts: &PruneOptions,
) {
    for c in snaps.iter_mut() {
        if Some(c.info.id) == head {
            c.keep(KeepReason::Latest);
        }
        if !c.info.labels.is_empty() {
            c.keep(KeepReason::Label);
        }
        if let Some(drives) = drive_states.get(&c.info.fingerprint) {
            c.keep(KeepReason::Drive);
            c.drives = drives.clone();
        }
    }
    for c in snaps.iter_mut().take(opts.keep_last.unwrap_or(0)) {
        c.keep(KeepReason::Last);
    }
    let day = |s: &SnapshotInfo| s.created.with_timezone(&Local).date_naive();
    if let Some(n) = opts.keep_daily {
        keep_newest_per(snaps, n, KeepReason::Daily, |s| {
            let d = day(s);
            (d.year(), d.ordinal())
        });
    }
    if let Some(n) = opts.keep_weekly {
        keep_newest_per(snaps, n, KeepReason::Weekly, |s| {
            let w = day(s).iso_week();
            (w.year(), w.week())
        });
    }
}

/// Keep the newest snapshot in each of the newest `n` periods that have one.
fn keep_newest_per(
    snaps: &mut [Candidate],
    n: usize,
    reason: KeepReason,
    period: impl Fn(&SnapshotInfo) -> (i32, u32),
) {
    let mut seen = FxHashSet::default();
    for c in snaps.iter_mut() {
        let p = period(&c.info);
        if seen.contains(&p) {
            continue;
        }
        if seen.len() == n {
            break;
        }
        seen.insert(p);
        c.keep(reason);
    }
}

type ListId = (String, Vec<String>);

/// A snapshot's history lists, by path and identity; `None` when its databases can't be read.
type Lists = Option<Vec<(String, ListId)>>;

/// Keep the newest snapshot holding each history list no kept snapshot holds.
fn protect_history(store: &Store, snaps: &mut [Candidate], ctx: &Ctx<'_>, warnings: &mut Vec<String>) -> Result<()> {
    if snaps.iter().all(|c| !c.reasons.is_empty()) {
        return Ok(());
    }
    // Snapshots of one state share their database files: read each set once.
    let mut cache: FxHashMap<Vec<Option<Hash>>, Lists> = FxHashMap::default();
    let mut lists = |info: &SnapshotInfo, warnings: &mut Vec<String>| -> Result<Lists> {
        ctx.check()?;
        ctx.emit(Event::Item { phase: Phase::Scan, path: info.short_id.clone(), bytes: 0 });
        let snap = store.load_snapshot(&info.id)?;
        let key: Vec<Option<Hash>> = [paths::EXPORT_PDB, paths::LIBRARY_PLUS_DB, paths::LIBRARY_PLUS_WAL]
            .iter()
            .map(|k| snap.files.iter().find(|f| f.path == *k).and_then(|f| f.hash))
            .collect();
        if let Some(found) = cache.get(&key) {
            return Ok(found.clone());
        }
        let found = match Databases::from_snapshot(store, &snap) {
            Ok(dbs) => Some(plays::histories(&dbs).iter().map(|l| (l.path.clone(), l.identity())).collect()),
            Err(e) => {
                warnings.push(format!("snapshot {}: {e}; kept", info.short_id));
                None
            }
        };
        cache.insert(key, found.clone());
        Ok(found)
    };
    ctx.phase_start(Phase::Scan, snaps.len() as u64, 0);
    let mut safe: FxHashSet<ListId> = FxHashSet::default();
    for c in snaps.iter().filter(|c| !c.reasons.is_empty()) {
        safe.extend(lists(&c.info, warnings)?.into_iter().flatten().map(|(_, id)| id));
    }
    for c in snaps.iter_mut().filter(|c| c.reasons.is_empty()) {
        let Some(found) = lists(&c.info, warnings)? else {
            c.keep(KeepReason::Unreadable);
            continue;
        };
        let only: Vec<String> =
            found.iter().filter(|(_, id)| !safe.contains(id)).map(|(path, _)| path.clone()).collect();
        if !only.is_empty() {
            c.keep(KeepReason::History);
            c.history = only;
            safe.extend(found.into_iter().map(|(_, id)| id));
        }
    }
    ctx.phase_end(Phase::Scan);
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Duration, TimeZone, Utc};

    use super::*;
    use crate::model::LibrarySummary;

    fn candidate(n: u8, created: DateTime<Utc>) -> Candidate {
        let id = Hash::of(&[n]);
        Candidate {
            info: SnapshotInfo {
                id,
                short_id: n.to_string(),
                library_id: uuid::Uuid::nil(),
                parents: vec![],
                created,
                message: None,
                source: None,
                fingerprint: Hash::of(&[n, n]),
                summary: LibrarySummary::default(),
                files: 0,
                bytes: 0,
                labels: vec![],
                beside: false,
            },
            reasons: vec![],
            drives: vec![],
            history: vec![],
        }
    }

    /// Kept snapshots by number, newest first.
    fn kept(snaps: &[Candidate]) -> Vec<&str> {
        snaps.iter().filter(|c| !c.reasons.is_empty()).map(|c| c.info.short_id.as_str()).collect()
    }

    /// Two snapshots a day, at 10:00 and 14:00 local time, for 21 days: newest first.
    fn three_weeks() -> Vec<Candidate> {
        let start = Local.with_ymd_and_hms(2026, 3, 2, 10, 0, 0).unwrap().with_timezone(&Utc); // a Monday
        let mut v: Vec<Candidate> = (0..42u8)
            .map(|i| candidate(i, start + Duration::days(i64::from(i / 2)) + Duration::hours(4 * i64::from(i % 2))))
            .collect();
        v.reverse();
        v
    }

    #[test]
    fn last_daily_and_weekly_keep_the_newest_of_each() {
        let none = FxHashMap::default();
        let mut snaps = three_weeks();
        by_policy(&mut snaps, None, &none, &PruneOptions { keep_last: Some(3), ..Default::default() });
        assert_eq!(kept(&snaps), ["41", "40", "39"]);

        let mut snaps = three_weeks();
        by_policy(&mut snaps, None, &none, &PruneOptions { keep_daily: Some(3), ..Default::default() });
        assert_eq!(kept(&snaps), ["41", "39", "37"], "each day's 14:00");

        let mut snaps = three_weeks();
        by_policy(&mut snaps, None, &none, &PruneOptions { keep_weekly: Some(2), ..Default::default() });
        // Sunday 22 March, then Sunday 15 March.
        assert_eq!(kept(&snaps), ["41", "27"]);

        // Together, with the head (an older one), a label and a drive's state.
        let mut snaps = three_weeks();
        snaps[30].info.labels = vec!["before NYE".into()];
        let drives = FxHashMap::from_iter([(snaps[35].info.fingerprint, vec!["Backup".to_string()])]);
        let head = Some(snaps[1].info.id);
        let opts = PruneOptions { keep_last: Some(1), keep_daily: Some(2), keep_weekly: Some(3), ..Default::default() };
        by_policy(&mut snaps, head, &drives, &opts);
        assert_eq!(kept(&snaps), ["41", "40", "39", "27", "13", "11", "6"]);
        assert_eq!(snaps[0].reasons, [KeepReason::Last, KeepReason::Daily, KeepReason::Weekly]);
        assert_eq!(snaps[1].reasons, [KeepReason::Latest]);
        assert_eq!(snaps[30].reasons, [KeepReason::Label]);
        assert_eq!(
            (snaps[35].reasons.as_slice(), snaps[35].drives.as_slice()),
            (&[KeepReason::Drive][..], &["Backup".to_string()][..])
        );
    }
}
