//! Deciding which of two exports of a library is newer.
//!
//! The strongest signal is *lineage*: every drive locallibrary touches records the fingerprints
//! of the library states it has held (see [`crate::device`]). If drive A's history contains drive
//! B's current state, B is simply behind A. If both moved on from a shared state, they diverged.
//!
//! For drives that have never been seen before, the decision falls back to evidence from the
//! exports themselves, each reported so a user (or UI) can judge:
//!
//! * track set containment (one library is a superset of the other),
//! * `export.pdb`'s internal save counter (meaningful when one drive was copied from the other),
//! * when the database (`export.pdb`, or `exportLibrary.db` on OneLibrary-only drives) was last
//!   written,
//! * the most recent "date added" among tracks.
//!
//! Tracks are matched by file path, which rekordbox keeps stable across exports of one library.
//! If the libraries barely overlap they are reported as *unrelated*, which is the cue to merge
//! rather than sync.

use chrono::{DateTime, Utc};
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::Serialize;
use uuid::Uuid;

use crate::device::{Device, LineageEntry, LineageOp};
use crate::doctor::Databases;
use crate::hash::Hash;
use crate::local_time;
use crate::model::{Library, LibraryDiff};
use crate::paths;
use crate::pdb;
use crate::progress::Ctx;
use crate::store::{Snapshot, Store};
use crate::{Error, Result};

/// One side of a comparison: a drive or a stored snapshot.
#[derive(Debug, Clone)]
pub struct Side {
    pub label: String,
    pub id: Option<Uuid>,
    pub library_id: Option<Uuid>,
    pub fingerprint: Option<Hash>,
    /// Other fingerprints of the same state: lineage recorded by earlier versions of
    /// locallibrary may hold these (see [`crate::device::Scanned::legacy_fingerprints`]), and a
    /// drive synced with `--keep-extra` also goes by the state it was synced to (see
    /// [`crate::device::StateAlias`]).
    pub aliases: Vec<Hash>,
    pub lineage: Vec<LineageEntry>,
    pub library: Library,
    pub sequence: Option<u32>,
    pub exported_at: Option<DateTime<Utc>>,
    pub pending: bool,
    /// A sync or push onto the drive was cut off: it holds part of the library being copied.
    pub mid_copy: bool,
    /// The drive this one was packed from (see [`crate::pack`]): it holds some of that drive's
    /// playlists, not its library.
    pub packed_from: Option<Uuid>,
}

/// A file of `snapshot` from the store: where it is and its bytes (`None`: the snapshot lacks it).
pub(crate) fn snapshot_object(
    store: &Store,
    snapshot: &Snapshot,
    key: &str,
) -> Result<Option<(std::path::PathBuf, Vec<u8>)>> {
    let Some(entry) = snapshot.files.iter().find(|f| f.path == key) else { return Ok(None) };
    let hash = entry.hash.ok_or_else(|| Error::invalid(format!("{} has no hash", entry.path)))?;
    let path = store.object_path(&hash);
    let bytes = std::fs::read(&path).map_err(|e| Error::Io { path: path.clone(), source: e })?;
    Ok(Some((path, bytes)))
}

impl Side {
    pub fn from_device(device: &Device, ctx: &Ctx<'_>) -> Result<Side> {
        let library = device.library()?;
        let scanned = device.scan_fingerprinted(Some(&library), ctx)?;
        Ok(Side {
            label: device.name(),
            id: device.id(),
            library_id: device.state().and_then(|s| s.library_id),
            fingerprint: scanned.fingerprint,
            aliases: scanned.legacy_fingerprints.into_iter().chain(scanned.alias).collect(),
            lineage: device.state().map(|s| s.lineage.clone()).unwrap_or_default(),
            library,
            sequence: device.pdb_header().map(|h| h.sequence),
            exported_at: device.exported_at(),
            pending: device.state().is_some_and(|s| s.pending.is_some()),
            mid_copy: device
                .state()
                .and_then(|s| s.pending.as_ref())
                .is_some_and(|p| matches!(p.op, LineageOp::Sync | LineageOp::Push)),
            packed_from: crate::pack::record(device).ok().flatten().and_then(|r| r.source),
        })
    }

    pub fn from_snapshot(store: &Store, snapshot: &Snapshot, label: impl Into<String>) -> Result<Side> {
        // Like a drive: export.pdb when the snapshot has one, otherwise OneLibrary.
        let (library, sequence) = match snapshot_object(store, snapshot, paths::EXPORT_PDB)? {
            Some((path, bytes)) => {
                let sequence = pdb::read_header(&bytes).map(|h| h.sequence);
                (pdb::library_from_database(&pdb::parse_database(bytes, &path)?), sequence)
            }
            None => (Databases::from_snapshot(store, snapshot)?.primary().clone(), None),
        };
        Ok(Side {
            label: label.into(),
            id: None,
            library_id: Some(snapshot.library_id),
            fingerprint: Some(snapshot.fingerprint),
            aliases: Vec::new(),
            lineage: snapshot.lineage.clone(),
            library,
            sequence,
            exported_at: Some(snapshot.created),
            pending: false,
            mid_copy: false,
            packed_from: None,
        })
    }

    /// Every fingerprint this side's state goes by.
    fn fingerprints(&self) -> impl Iterator<Item = Hash> + '_ {
        self.fingerprint.into_iter().chain(self.aliases.iter().copied())
    }

    /// Whether this side's lineage holds `other`'s current state.
    fn knows(&self, other: &Side) -> bool {
        other.fingerprints().any(|fp| self.lineage.iter().any(|e| e.fingerprint == fp))
    }

    /// Whether both hold the same state.
    fn same_state(&self, other: &Side) -> bool {
        self.fingerprint.is_some() && self.fingerprints().any(|a| other.fingerprints().any(|b| a == b))
    }

    /// Changed since locallibrary last recorded it (e.g. a fresh rekordbox export).
    fn dirty(&self) -> bool {
        match (self.lineage.last(), self.fingerprint) {
            (Some(head), Some(_)) => !self.fingerprints().any(|fp| fp == head.fingerprint),
            _ => false,
        }
    }

    fn summary(&self) -> SideSummary {
        SideSummary {
            label: self.label.clone(),
            id: self.id,
            library_id: self.library_id,
            fingerprint: self.fingerprint,
            tracks: self.library.tracks.len(),
            playlists: self.library.playlists.iter().filter(|p| !p.is_folder).count(),
            sequence: self.sequence,
            exported_at: self.exported_at,
            changed_since_last_sync: self.dirty(),
            interrupted_operation: self.pending,
            last_synced: self.lineage.last().map(|e| e.at),
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SideSummary {
    pub label: String,
    pub id: Option<Uuid>,
    pub library_id: Option<Uuid>,
    pub fingerprint: Option<Hash>,
    pub tracks: usize,
    pub playlists: usize,
    pub sequence: Option<u32>,
    pub exported_at: Option<DateTime<Utc>>,
    pub changed_since_last_sync: bool,
    pub interrupted_operation: bool,
    pub last_synced: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    /// Same content.
    Identical,
    LeftNewer,
    RightNewer,
    /// Same library, but both sides changed independently.
    Diverged,
    /// Different libraries (little overlap): merge rather than sync.
    Unrelated,
    /// Same library, no way to tell which is newer.
    Undetermined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Favors {
    Left,
    Right,
    Neither,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Evidence {
    pub signal: &'static str,
    pub favors: Favors,
    pub detail: String,
    #[serde(skip)]
    weight: u32,
}

/// What to do about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Suggestion {
    Nothing,
    UpdateRight,
    UpdateLeft,
    /// Diverged: pick a side explicitly, or merge.
    ChooseOrMerge,
    Merge,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Comparison {
    pub relation: Relation,
    pub confidence: Confidence,
    pub suggestion: Suggestion,
    pub left: SideSummary,
    pub right: SideSummary,
    pub evidence: Vec<Evidence>,
    /// Shared tracks / tracks in the smaller library.
    pub overlap: f64,
    /// From the older side to the newer one (left -> right when undetermined).
    pub diff: LibraryDiff,
}

impl Comparison {
    /// `true` if the comparison says `left` should be copied over `right`.
    pub fn left_is_newer(&self) -> bool {
        self.relation == Relation::LeftNewer
    }

    /// Why which side is newer can't be told, when it can't: the evidence that made it
    /// [`Relation::Undetermined`], which [`compare`] gives last.
    pub fn undetermined(&self) -> Option<&str> {
        match self.relation {
            Relation::Undetermined => self.evidence.last().map(|e| e.detail.as_str()),
            _ => None,
        }
    }
}

pub fn compare_devices(left: &Device, right: &Device, ctx: &Ctx<'_>) -> Result<Comparison> {
    let mut c = compare(&Side::from_device(left, ctx)?, &Side::from_device(right, ctx)?);
    // A side holds export.pdb's library when the drive has it, and OneLibrary players write only
    // to OneLibrary: its differences count too.
    let (l, r) = (Databases::read(left).ok(), Databases::read(right).ok());
    if let (Some(l), Some(r)) = (l.as_ref().and_then(Databases::beside), r.as_ref().and_then(Databases::beside)) {
        crate::diff::add_library_diff(&mut c.diff, older_to_newer(c.relation, l, r));
    }
    Ok(c)
}

/// How `left` and `right` differ, from the older to the newer as `relation` has them.
fn older_to_newer(relation: Relation, left: &Library, right: &Library) -> LibraryDiff {
    match relation {
        Relation::LeftNewer => LibraryDiff::between(right, left),
        _ => LibraryDiff::between(left, right),
    }
}

pub fn compare(left: &Side, right: &Side) -> Comparison {
    let mut evidence = Vec::new();
    let overlap = track_overlap(&left.library, &right.library);

    let (mut relation, mut confidence) = decide(left, right, overlap, &mut evidence);
    // What a drive an operation was cut off on holds may be half written (a sync or push cut off
    // on it is the older side, see `decide`): it isn't copied over the other unless asked to.
    let newer = match relation {
        Relation::LeftNewer => Some(left),
        Relation::RightNewer => Some(right),
        _ => None,
    };
    if let Some(side) = newer.filter(|s| s.pending && !s.mid_copy) {
        evidence.push(self::evidence(
            "interrupted",
            Favors::Neither,
            format!("an operation on {} didn't finish: say which way to copy", side.label),
            0,
        ));
        (relation, confidence) = (Relation::Undetermined, Confidence::Low);
    }
    let suggestion = match relation {
        Relation::Identical => Suggestion::Nothing,
        Relation::LeftNewer => Suggestion::UpdateRight,
        Relation::RightNewer => Suggestion::UpdateLeft,
        Relation::Diverged | Relation::Undetermined => Suggestion::ChooseOrMerge,
        Relation::Unrelated => Suggestion::Merge,
    };
    let diff = older_to_newer(relation, &left.library, &right.library);
    Comparison {
        relation,
        confidence,
        suggestion,
        left: left.summary(),
        right: right.summary(),
        evidence,
        overlap,
        diff,
    }
}

fn decide(left: &Side, right: &Side, overlap: f64, ev: &mut Vec<Evidence>) -> (Relation, Confidence) {
    if left.same_state(right) {
        ev.push(evidence("content", Favors::Neither, "both hold exactly the same library files", 0));
        return (Relation::Identical, Confidence::High);
    }
    // A sync or push cut off on one drive left it part way between its old library and the one
    // being copied onto it: it holds no state of its own, and syncing onto it again finishes
    // the job. (Its old library was being replaced anyway; nothing only it had is lost.)
    if left.mid_copy != right.mid_copy {
        let (newer, behind) = if right.mid_copy { (left, right) } else { (right, left) };
        ev.push(evidence(
            "interrupted",
            if right.mid_copy { Favors::Left } else { Favors::Right },
            format!("a sync onto {} didn't finish; syncing from {} again finishes it", behind.label, newer.label),
            0,
        ));
        return (if right.mid_copy { Relation::LeftNewer } else { Relation::RightNewer }, Confidence::High);
    }
    if left.pending || right.pending {
        ev.push(evidence(
            "interrupted",
            if left.pending { Favors::Right } else { Favors::Left },
            "a previous locallibrary operation on one side did not finish",
            0,
        ));
    }
    // A drive packed from the other holds some of its playlists: a different library, which
    // a sync would replace with the whole one.
    for (packed, from) in [(right, left), (left, right)] {
        if packed.packed_from.is_some() && packed.packed_from == from.id {
            ev.push(evidence("pack", Favors::Neither, format!("{} was packed from {}", packed.label, from.label), 0));
            return (Relation::Unrelated, Confidence::High);
        }
    }

    // 1. Lineage.
    let l_has_r = left.knows(right);
    let r_has_l = right.knows(left);
    match (l_has_r, r_has_l) {
        (true, false) => {
            ev.push(evidence(
                "lineage",
                Favors::Left,
                format!("{} previously held {}'s current state", left.label, right.label),
                0,
            ));
            return (Relation::LeftNewer, Confidence::High);
        }
        (false, true) => {
            ev.push(evidence(
                "lineage",
                Favors::Right,
                format!("{} previously held {}'s current state", right.label, left.label),
                0,
            ));
            return (Relation::RightNewer, Confidence::High);
        }
        _ => {}
    }
    if let (Some(a), Some(b)) = (left.library_id, right.library_id)
        && a != b
    {
        ev.push(evidence("library", Favors::Neither, "the drives are linked to different libraries in the store", 0));
        return (Relation::Unrelated, Confidence::High);
    }
    let r_fps: FxHashSet<Hash> = right.lineage.iter().map(|e| e.fingerprint).collect();
    if let Some(common) = left.lineage.iter().rev().find(|e| r_fps.contains(&e.fingerprint)) {
        ev.push(evidence(
            "lineage",
            Favors::Neither,
            format!("both changed since they were last in sync ({})", local_time(common.at, "%Y-%m-%d %H:%M")),
            0,
        ));
        return (Relation::Diverged, Confidence::High);
    }

    // 2. Heuristics.
    if overlap < 0.5 {
        ev.push(evidence("overlap", Favors::Neither, format!("only {:.0}% of tracks are shared", overlap * 100.0), 0));
        return (Relation::Unrelated, if overlap < 0.2 { Confidence::High } else { Confidence::Medium });
    }

    let lp: FxHashSet<String> = left.library.tracks.iter().map(|t| paths::key(&t.file_path)).collect();
    let rp: FxHashSet<String> = right.library.tracks.iter().map(|t| paths::key(&t.file_path)).collect();
    let only_l = lp.difference(&rp).count();
    let only_r = rp.difference(&lp).count();
    match (only_l, only_r) {
        (0, 0) => ev.push(evidence("tracks", Favors::Neither, "same set of tracks", 0)),
        (n, 0) => ev.push(evidence(
            "tracks",
            Favors::Left,
            format!("{} has {n} track(s) the other lacks and none missing", left.label),
            3,
        )),
        (0, n) => ev.push(evidence(
            "tracks",
            Favors::Right,
            format!("{} has {n} track(s) the other lacks and none missing", right.label),
            3,
        )),
        (a, b) => {
            ev.push(evidence(
                "tracks",
                Favors::Neither,
                format!(
                    "each side has tracks the other lacks ({a} only on {}, {b} only on {})",
                    left.label, right.label
                ),
                0,
            ));
            return (Relation::Diverged, Confidence::Medium);
        }
    }

    if let (Some(a), Some(b)) = (left.sequence, right.sequence)
        && a != b
    {
        let f = if a > b { Favors::Left } else { Favors::Right };
        ev.push(evidence("pdb_sequence", f, format!("export.pdb save counter {a} vs {b}"), 2));
    }
    if let (Some(a), Some(b)) = (left.exported_at, right.exported_at) {
        let secs = (a - b).num_seconds();
        if secs.abs() > 60 {
            let f = if secs > 0 { Favors::Left } else { Favors::Right };
            ev.push(evidence(
                "exported_at",
                f,
                format!("exported {} vs {}", local_time(a, "%Y-%m-%d %H:%M"), local_time(b, "%Y-%m-%d %H:%M")),
                1,
            ));
        }
    }
    let newest = |l: &Library| l.tracks.iter().map(|t| t.date_added.clone()).filter(|d| !d.is_empty()).max();
    if let (Some(a), Some(b)) = (newest(&left.library), newest(&right.library))
        && a != b
    {
        let f = if a > b { Favors::Left } else { Favors::Right };
        ev.push(evidence("date_added", f, format!("most recently added track {a} vs {b}"), 2));
    }

    let score = |side: Favors| ev.iter().filter(|e| e.favors == side).map(|e| e.weight).sum::<u32>();
    let (l, r) = (score(Favors::Left), score(Favors::Right));
    match l.cmp(&r) {
        std::cmp::Ordering::Equal => {
            ev.push(evidence(
                "tie",
                Favors::Neither,
                format!(
                    "the evidence doesn't tell whether {} or {} is newer: say which way to copy",
                    left.label, right.label
                ),
                0,
            ));
            (Relation::Undetermined, Confidence::Low)
        }
        ord => {
            let (winner, loser_score, total) =
                if ord.is_gt() { (Relation::LeftNewer, r, l) } else { (Relation::RightNewer, l, r) };
            // Copying a side with fewer tracks over one with more deletes tracks: not on this
            // evidence alone.
            let (newer, older) = if ord.is_gt() { (left, right) } else { (right, left) };
            if newer.library.tracks.len() < older.library.tracks.len() {
                ev.push(evidence(
                    "tracks",
                    Favors::Neither,
                    format!("{} looks newer, but has fewer tracks: say which way to copy", newer.label),
                    0,
                ));
                return (Relation::Undetermined, Confidence::Low);
            }
            // Holding more tracks isn't enough on its own either (a newer export may have removed
            // some): another signal has to agree, with none against.
            let confidence = if loser_score == 0 && total >= 4 { Confidence::Medium } else { Confidence::Low };
            (winner, confidence)
        }
    }
}

fn evidence(signal: &'static str, favors: Favors, detail: impl Into<String>, weight: u32) -> Evidence {
    Evidence { signal, favors, detail: detail.into(), weight }
}

/// Fraction of the smaller library's tracks (by path) also present in the other.
pub fn track_overlap(a: &Library, b: &Library) -> f64 {
    let pa: FxHashSet<String> = a.tracks.iter().map(|t| paths::key(&t.file_path)).collect();
    let pb: FxHashSet<String> = b.tracks.iter().map(|t| paths::key(&t.file_path)).collect();
    let smaller = pa.len().min(pb.len());
    if smaller == 0 {
        return if pa.len() == pb.len() { 1.0 } else { 0.0 };
    }
    pa.intersection(&pb).count() as f64 / smaller as f64
}

#[cfg(test)]
mod tests;
