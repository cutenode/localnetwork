//! Writes of rekordbox's collection (`master.db`) made from here, and what the store keeps of them
//! (ADR 0007):
//!
//! ```text
//! <store>/collection/
//!   staged.json    edits waiting to be written, oldest first: rekordbox or its agent was running
//!   written.json   the latest writes, each with what it replaced, for undo
//!   backups/       master.db as it was right before each write (the latest BACKUPS_KEPT)
//!   lock           held while a write is made and these files are read and written back
//! ```
//!
//! Every edit is staged first and then written with whatever else is waiting, in the order made,
//! as one write ([`flush`]): over a fresh backup, only while rekordbox and its agent are closed
//! (see [`masterdb::write`]). While either runs the edits wait, and the player writes them once
//! both have quit. Cloud Library Sync being on (or looking so) refuses the write and drops the edit:
//! what a synced write has to change hasn't been worked out.
//!
//! Undo moves forward ([`undo_plan`]): it writes what an edit replaced as a new change with new
//! stamps, so `localUpdateCount` only rises. A track whose cues changed in rekordbox since is left
//! as it is, with a warning. An edit still waiting is just dropped. Restoring a backup whole is
//! only for a damaged file (`locallibrary collection` doesn't offer it).

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cue_edit::collection::{CollectionCueEdit, apply_in};
use crate::fsutil;
use crate::op::{Operation, Planned, Touched};
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result};
use loclib_rekordbox_db::masterdb;
#[cfg(test)]
use loclib_rekordbox_db::masterdb::files::Running;
use loclib_rekordbox_db::masterdb::write::{self as mw, CueEntry, CuesBefore, Edit, Target};

/// How many backups of `master.db` the store keeps: each write makes one.
pub const BACKUPS_KEPT: usize = 10;

/// How many writes `written.json` remembers, for undo.
const WRITTEN_KEPT: usize = 50;

/// An edit waiting to be written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Staged {
    pub id: String,
    pub at: DateTime<Utc>,
    /// The `master.db` it's for.
    pub db: PathBuf,
    pub change: Change,
}

/// What a staged edit does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    Cues(CollectionCueEdit),
    /// Put back what the write `write` (a [`Written::id`]) replaced.
    Undo {
        write: String,
        what: String,
    },
}

impl Change {
    pub fn describe(&self) -> String {
        match self {
            Change::Cues(c) => c.describe(),
            Change::Undo { what, .. } => format!("undo: {what}"),
        }
    }
}

/// A write that was made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Written {
    pub id: String,
    pub at: DateTime<Utc>,
    pub db: PathBuf,
    /// What it did, in a few words.
    pub what: String,
    /// The backup of `master.db` taken right before it.
    pub backup: PathBuf,
    /// `localUpdateCount` before and after.
    pub count: (i64, i64),
    /// What each track's cues were before it.
    pub before: Vec<CuesBefore>,
    /// Each track's cues right after it, by `djmdContent.ID`: an undo leaves a track whose cues
    /// differ from these alone (rekordbox changed them since).
    pub after: Vec<(String, Vec<CueEntry>)>,
    /// The write this one undid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undoes: Option<String>,
    /// It was undone since.
    #[serde(default)]
    pub undone: bool,
    /// A repair of the cue lists, which has nothing to undo.
    #[serde(default)]
    pub repair: bool,
}

/// What came of writing the collection.
#[derive(Debug, Clone, Default, PartialEq, Serialize, JsonSchema)]
pub struct CollectionWrite {
    /// Edits written now.
    pub written: usize,
    /// Edits still waiting to be written.
    pub waiting: usize,
    /// Why they're waiting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// `localUpdateCount` before and after the write.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<(i64, i64)>,
    /// The backup taken right before it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup: Option<PathBuf>,
    /// Edits (or tracks of them) passed over: they no longer applied.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

fn folder(store: &Store) -> PathBuf {
    store.root().join("collection")
}

fn staged_path(store: &Store) -> PathBuf {
    folder(store).join("staged.json")
}

fn written_path(store: &Store) -> PathBuf {
    folder(store).join("written.json")
}

fn lock(store: &Store) -> Result<fsutil::FileLock> {
    let dir = folder(store);
    std::fs::create_dir_all(&dir).map_err(|e| Error::Io { path: dir.clone(), source: e })?;
    fsutil::FileLock::wait(&dir.join("lock"))
}

fn all_staged(store: &Store) -> Result<Vec<Staged>> {
    Ok(fsutil::read_json_opt(&staged_path(store))?.unwrap_or_default())
}

fn all_written(store: &Store) -> Result<Vec<Written>> {
    Ok(fsutil::read_json_opt(&written_path(store))?.unwrap_or_default())
}

/// The edits of the `master.db` at `db` waiting to be written, oldest first.
pub fn staged(store: &Store, db: &Path) -> Result<Vec<Staged>> {
    Ok(all_staged(store)?.into_iter().filter(|s| s.db == db).collect())
}

/// The writes of the `master.db` at `db` made from here, oldest first (the latest few).
pub fn written(store: &Store, db: &Path) -> Result<Vec<Written>> {
    Ok(all_written(store)?.into_iter().filter(|w| w.db == db).collect())
}

/// The store a collection write keeps its backups and staged edits in: `store`, else the one the
/// CLI named ([`crate::api::store_root`]).
pub(crate) fn store_for(store: Option<&Store>) -> Result<Store> {
    match store {
        Some(s) => Ok(s.clone()),
        None => {
            let root = crate::api::store_root().ok_or_else(|| {
                Error::invalid("writing rekordbox's collection needs locallibrary's store, for its backups")
            })?;
            Store::open_or_init(root)
        }
    }
}

/// What a test says of rekordbox running, and its agent's log.
#[cfg(test)]
type TestTarget = (fn() -> Running, Option<PathBuf>);

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_TARGET: std::cell::RefCell<Option<TestTarget>> =
        const { std::cell::RefCell::new(None) };
}

/// The write target for the `master.db` at `db`: backed up into the store, rekordbox and its agent
/// as they run on this computer, the agent's log where rekordbox keeps it.
fn target(store: &Store, db: &Path, key: &str) -> Target {
    let here = Target::this_computer(PathBuf::new());
    #[allow(unused_mut, reason = "tests change it")]
    let mut t = Target {
        master_db: db.to_path_buf(),
        key: key.to_string(),
        backups: folder(store).join("backups"),
        agent_log: here.and_then(|h| h.agent_log),
        work: None,
        running: masterdb::files::running_now,
    };
    #[cfg(test)]
    {
        TEST_TARGET.with(|o| {
            if let Some((running, log)) = o.borrow().clone() {
                t.running = running;
                t.agent_log = log;
            }
        });
        // No test of this crate writes rekordbox's own collection: the write path's own guard is
        // built only into that crate's tests.
        let own = masterdb::default_path().filter(|_| std::env::var_os(masterdb::PATH_ENV).is_none());
        assert!(own.as_deref() != Some(db), "a test tried to write rekordbox's own collection");
        let home = directories::BaseDirs::new().map(|b| b.home_dir().join("Library/Pioneer"));
        assert!(!home.is_some_and(|h| db.starts_with(h)), "a test tried to write {}", db.display());
    }
    t
}

/// Keep `change` to be written to `db`, after those waiting already. Answers its id.
fn stage(store: &Store, db: &Path, change: Change) -> Result<String> {
    let _lock = lock(store)?;
    let mut all = all_staged(store)?;
    let id = uuid::Uuid::new_v4().to_string();
    all.push(Staged { id: id.clone(), at: Utc::now(), db: db.to_path_buf(), change });
    fsutil::write_json_atomic(&staged_path(store), &all)?;
    Ok(id)
}

/// Drop the staged edit `id`; whether it was there.
fn unstage(store: &Store, id: &str) -> Result<bool> {
    let _lock = lock(store)?;
    let mut all = all_staged(store)?;
    let n = all.len();
    all.retain(|s| s.id != id);
    fsutil::write_json_atomic(&staged_path(store), &all)?;
    Ok(all.len() < n)
}

/// One staged edit made inside a write: what it replaced, and the cues it left.
struct Made {
    before: Vec<CuesBefore>,
    after: Vec<(String, Vec<CueEntry>)>,
}

fn make(
    e: &mut Edit<'_>,
    collection: &masterdb::Collection,
    change: &Change,
    written: &[Written],
    warnings: &mut Vec<String>,
) -> Result<Made> {
    let before = match change {
        Change::Cues(edit) => apply_in(e, collection, edit, warnings)?,
        Change::Undo { write, what } => {
            let Some(w) = written.iter().find(|w| &w.id == write) else {
                warnings.push(format!("undo: {what}: that write isn't remembered any more"));
                return Ok(Made { before: Vec::new(), after: Vec::new() });
            };
            let mut out = Vec::new();
            for b in &w.before {
                let left = w.after.iter().find(|(id, _)| *id == b.track_id).map(|(_, c)| c);
                let now = e.cues(&b.track_id)?;
                if left.is_some_and(|l| *l != now) {
                    warnings.push(format!(
                        "undo: {what}: track {}'s cues changed since (in rekordbox?), so they're left as they are",
                        b.track_id
                    ));
                    continue;
                }
                out.push(e.restore_cues(b)?);
            }
            out
        }
    };
    let mut after = Vec::new();
    for b in &before {
        after.push((b.track_id.clone(), e.cues(&b.track_id)?));
    }
    Ok(Made { before, after })
}

/// Write every edit of `db` waiting in the store, in the order made, as one write. While rekordbox
/// or its agent runs (or `master.db` changed meanwhile) they stay waiting, and the answer says
/// why; any other refusal (Cloud Library Sync) is an error, and they stay too.
pub fn flush(store: &Store, db: &Path, key: &str, ctx: &Ctx<'_>) -> Result<CollectionWrite> {
    let _lock = lock(store)?;
    let waiting: Vec<Staged> = all_staged(store)?.into_iter().filter(|s| s.db == db).collect();
    if waiting.is_empty() {
        return Ok(CollectionWrite::default());
    }
    ctx.check()?;
    let target = target(store, db, key);
    // rekordbox running is told before the collection is read, which takes a while.
    let running = (target.running)();
    if running.rekordbox || running.agent {
        let why = match running.rekordbox {
            true => loclib_rekordbox_db::Refusal::RekordboxRunning,
            false => loclib_rekordbox_db::Refusal::AgentRunning,
        };
        return Ok(CollectionWrite { waiting: waiting.len(), why: Some(why.to_string()), ..Default::default() });
    }
    let collection = masterdb::open(db, key)?;
    let mut earlier = all_written(store)?;
    let mut warnings = Vec::new();
    let result = mw::write(&target, |e| {
        let mut made = Vec::new();
        for s in &waiting {
            let m = make(e, &collection, &s.change, &earlier, &mut warnings)
                .map_err(|err| loclib_rekordbox_db::Error::Invalid(err.to_string()))?;
            made.push(m);
        }
        Ok(made)
    });
    let w = match result {
        Ok(w) => w,
        Err(loclib_rekordbox_db::Error::Refused(r)) if r.waits() => {
            return Ok(CollectionWrite { waiting: waiting.len(), why: Some(r.to_string()), ..Default::default() });
        }
        Err(e) => return Err(e.into()),
    };
    // Remembered, then no longer waiting: a failure between the two would write them again, which
    // for whole cue lists comes to the same.
    for (s, m) in waiting.iter().zip(w.value) {
        let undoes = match &s.change {
            Change::Undo { write, .. } => Some(write.clone()),
            Change::Cues(_) => None,
        };
        if let Some(u) = &undoes
            && let Some(old) = earlier.iter_mut().find(|e| &e.id == u)
        {
            old.undone = true;
        }
        earlier.push(Written {
            id: s.id.clone(),
            at: Utc::now(),
            db: db.to_path_buf(),
            what: s.change.describe(),
            backup: w.backup.clone(),
            count: w.count,
            before: m.before,
            after: m.after,
            undoes,
            undone: false,
            repair: false,
        });
    }
    remember(store, earlier)?;
    let mut all = all_staged(store)?;
    all.retain(|s| !waiting.iter().any(|w| w.id == s.id));
    fsutil::write_json_atomic(&staged_path(store), &all)?;
    prune_backups(store);
    Ok(CollectionWrite {
        written: waiting.len(),
        waiting: all.iter().filter(|s| s.db == db).count(),
        why: None,
        count: Some(w.count),
        backup: Some(w.backup),
        warnings,
    })
}

fn remember(store: &Store, mut written: Vec<Written>) -> Result<()> {
    let extra = written.len().saturating_sub(WRITTEN_KEPT);
    written.drain(..extra);
    fsutil::write_json_atomic(&written_path(store), &written)
}

/// Keeps the latest [`BACKUPS_KEPT`] backups (their names sort by time).
fn prune_backups(store: &Store) {
    let dir = folder(store).join("backups");
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("master.") && n.ends_with(".db"))
        .collect();
    names.sort();
    let extra = names.len().saturating_sub(BACKUPS_KEPT);
    for n in &names[..extra] {
        let _ = std::fs::remove_file(dir.join(n));
    }
}

/// Stage `change` and write what's waiting: the edit (and any waiting before it) is written now,
/// or waits for rekordbox and its agent to quit. Refused otherwise (Cloud Library Sync, a damaged
/// file), the edit is dropped.
pub(crate) fn make_now(store: &Store, db: &Path, key: &str, change: Change, ctx: &Ctx<'_>) -> Result<CollectionWrite> {
    // An edit that would wait for rekordbox to quit is refused now if Cloud Library Sync is on:
    // the write would be refused then anyway, and the deck would show it as made meanwhile.
    let t = target(store, db, key);
    let running = (t.running)();
    if (running.rekordbox || running.agent)
        && let Some(why) = mw::sync_on_at(db, key, t.agent_log.as_deref())?
    {
        return Err(loclib_rekordbox_db::Error::Refused(loclib_rekordbox_db::Refusal::CloudSync(why)).into());
    }
    let id = stage(store, db, change)?;
    match flush(store, db, key, ctx) {
        Ok(w) => Ok(w),
        Err(e) => {
            unstage(store, &id)?;
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Undo
// ---------------------------------------------------------------------------------------------

/// Taking back the latest edit of the collection: a waiting one is dropped, a written one put
/// back as a new write.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CollectionUndoPlan {
    pub db: PathBuf,
    /// What's taken back.
    pub what: String,
    /// It's waiting, not written: it's only dropped.
    pub waiting: bool,
    /// The staged edit or write it takes back.
    pub id: String,
    #[serde(skip)]
    key: String,
}

/// What undoing came to.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct CollectionUndoReport {
    /// A waiting edit was dropped.
    pub dropped: bool,
    pub write: CollectionWrite,
}

/// Plan taking back the latest edit of the collection at `db` made from here (see the module
/// docs); a no-op when there's none.
pub fn undo_plan(store: &Store, db: &Path, key: &str) -> Result<Planned<CollectionUndoPlan>> {
    let last_staged = staged(store, db)?.pop();
    let target = match &last_staged {
        Some(s) => Some((s.id.clone(), s.change.describe(), true)),
        None => written(store, db)?
            .into_iter()
            .rev()
            .find(|w| !w.undone && w.undoes.is_none() && !w.repair)
            .map(|w| (w.id, w.what, false)),
    };
    let noop = target.is_none();
    let (id, what, waiting) = target.unwrap_or_default();
    let summary = match (noop, waiting) {
        (true, _) => "nothing to undo in rekordbox's collection".to_string(),
        (false, true) => format!("drop {what} (waiting, not written yet)"),
        (false, false) => format!("put back the cues as they were before {what}, as a new change"),
    };
    let plan = CollectionUndoPlan { db: db.to_path_buf(), what, waiting, id, key: key.to_string() };
    Ok(Planned::new("collection.undo", plan, ()).line(summary).noop(noop).touches(Touched::path(
        db,
        !waiting,
        Vec::new(),
    )))
}

impl Operation for CollectionUndoPlan {
    type Request = ();
    type Report = CollectionUndoReport;

    fn apply(planned: &Planned<Self>, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<CollectionUndoReport> {
        let p = &planned.plan;
        let store = store_for(store)?;
        if p.id.is_empty() {
            return Ok(CollectionUndoReport::default());
        }
        if p.waiting {
            if !unstage(&store, &p.id)? {
                return Err(Error::stale("rekordbox's collection", "the edit was written meanwhile"));
            }
            let waiting = staged(&store, &p.db)?.len();
            let write = CollectionWrite { waiting, ..Default::default() };
            return Ok(CollectionUndoReport { dropped: true, write });
        }
        let change = Change::Undo { write: p.id.clone(), what: p.what.clone() };
        Ok(CollectionUndoReport { dropped: false, write: make_now(&store, &p.db, &p.key, change, ctx)? })
    }
}

// ---------------------------------------------------------------------------------------------
// Repair
// ---------------------------------------------------------------------------------------------

/// Making the tracks' `contentCue` lists agree with their `djmdCue` rows (see
/// [`Edit::repair_cue_lists`]): rekordbox syncs and reads the lists, and scripts that wrote cue rows
/// alone left them disagreeing.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CollectionRepairPlan {
    pub db: PathBuf,
    /// The tracks whose lists are rewritten, as `ID  Artist - Title`.
    pub tracks: Vec<String>,
    /// Tracks with cues and no list get one too.
    pub create_missing: bool,
    #[serde(skip)]
    key: String,
}

/// Plan the repair of the collection at `db`.
pub fn repair_plan(db: &Path, key: &str, create_missing: bool) -> Result<Planned<CollectionRepairPlan>> {
    let ids = mw::cue_list_mismatches(db, key, create_missing)?;
    let names = match ids.is_empty() {
        true => Default::default(),
        false => {
            let c = masterdb::open(db, key)?;
            let by_content: rustc_hash::FxHashMap<&str, u32> =
                c.content_ids.iter().map(|(n, id)| (id.as_str(), *n)).collect();
            ids.iter()
                .map(|id| {
                    let name = by_content.get(id.as_str()).and_then(|n| c.library.track(*n)).map(|t| t.display_name());
                    format!("{id}  {}", name.unwrap_or_default())
                })
                .collect::<Vec<_>>()
        }
    };
    let noop = names.is_empty();
    let line = format!("rewrite the cue lists (contentCue) of {} track(s) from their cue rows", names.len());
    let plan = CollectionRepairPlan { db: db.to_path_buf(), tracks: names, create_missing, key: key.to_string() };
    Ok(Planned::new("collection.repair", plan, ()).line(line).noop(noop).touches(Touched::path(db, true, Vec::new())))
}

impl Operation for CollectionRepairPlan {
    type Request = ();
    type Report = CollectionWrite;

    /// Written now or not at all: refused while rekordbox or its agent runs.
    fn apply(planned: &Planned<Self>, store: Option<&Store>, _: &Ctx<'_>) -> Result<CollectionWrite> {
        let p = &planned.plan;
        let store = store_for(store)?;
        let _lock = lock(&store)?;
        let w = mw::write(&target(&store, &p.db, &p.key), |e| e.repair_cue_lists(p.create_missing))?;
        let mut written = all_written(&store)?;
        written.push(Written {
            id: uuid::Uuid::new_v4().to_string(),
            at: Utc::now(),
            db: p.db.clone(),
            what: format!("repair the cue lists of {} track(s)", w.value.len()),
            backup: w.backup.clone(),
            count: w.count,
            before: w.value.clone(),
            after: Vec::new(),
            undoes: None,
            undone: false,
            repair: true,
        });
        remember(&store, written)?;
        prune_backups(&store);
        Ok(CollectionWrite {
            written: w.value.len(),
            count: Some(w.count),
            backup: Some(w.backup),
            ..Default::default()
        })
    }
}

/// Plan writing what's waiting for the collection at `db` now.
pub fn apply_staged_plan(store: &Store, db: &Path, key: &str) -> Result<Planned<CollectionFlushPlan>> {
    let waiting: Vec<String> = staged(store, db)?.iter().map(|s| s.change.describe()).collect();
    let noop = waiting.is_empty();
    let plan = CollectionFlushPlan { db: db.to_path_buf(), edits: waiting.clone(), key: key.to_string() };
    Ok(Planned::new("collection.apply", plan, ()).summary(waiting).noop(noop).touches(Touched::path(
        db,
        true,
        Vec::new(),
    )))
}

/// Writing the edits waiting for the collection.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CollectionFlushPlan {
    pub db: PathBuf,
    /// The edits waiting, oldest first.
    pub edits: Vec<String>,
    #[serde(skip)]
    key: String,
}

impl Operation for CollectionFlushPlan {
    type Request = ();
    type Report = CollectionWrite;

    fn apply(planned: &Planned<Self>, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<CollectionWrite> {
        flush(&store_for(store)?, &planned.plan.db, &planned.plan.key, ctx)
    }
}

#[cfg(test)]
mod tests;
