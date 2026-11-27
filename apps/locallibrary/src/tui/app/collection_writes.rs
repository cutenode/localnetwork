//! The player's side of writing rekordbox's collection (see `locallibrary::collection_write`):
//! cue edits made while rekordbox or its agent runs wait in the store, shown on the deck as if
//! written, and are written once both have quit, from here (looked at every few seconds while the
//! collection is open, and once it's opened, so at the next start too). `u` takes the latest one
//! back, as a new change. Cloud Library Sync being on turns writes off: the write is refused with
//! why, and the player says so (once).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::output::plural;
use locallibrary::Store;
use locallibrary::collection_write::{self, Change, CollectionWrite};
use locallibrary::cue_edit::collection::overlay;
use loclib_rekordbox_db::masterdb::{self, Collection};

use super::*;

/// How often the collection's waiting edits are looked at, and written once rekordbox has quit.
const LOOK_EVERY: Duration = Duration::from_secs(5);

/// What the player knows of writing the open collection.
#[derive(Debug, Default)]
pub struct CollectionWrites {
    last: Option<Instant>,
    /// A look is under way (or, in tests, none is wanted).
    pub(in crate::tui) in_flight: bool,
    /// Cue edits waiting to be written.
    pub waiting: usize,
    /// Why they wait (rekordbox or its agent runs).
    pub why: Option<String>,
    /// The last failure said, so it's said once.
    said: Option<String>,
    /// `master.db` as it was when a write was last refused for something other than rekordbox
    /// running (Cloud Library Sync, say): it isn't tried again until the file changes, as each try
    /// backs the whole collection up and decrypts it.
    pub(in crate::tui) refused_at: Option<super::drives::CollectionStamp>,
}

impl CollectionWrites {
    /// A new edit or undo was made: what's waiting is looked at again at once.
    pub(in crate::tui) fn changed(&mut self) {
        self.refused_at = None;
        self.last = None;
    }
}

/// What a look at the collection's waiting edits came to.
pub struct Looked {
    pub(in crate::tui) db: PathBuf,
    pub(in crate::tui) write: Result<CollectionWrite, String>,
}

/// The collection's cues with the edits waiting in the store over them: what the player shows.
pub(in crate::tui) fn with_waiting(c: &mut Collection, store_root: &Path) {
    let Ok(Some(store)) = Store::open_existing(store_root) else { return };
    let Ok(waiting) = collection_write::staged(&store, &c.path) else { return };
    let edits = waiting.iter().filter_map(|s| match &s.change {
        Change::Cues(e) => Some(e),
        Change::Undo { .. } => None,
    });
    overlay(&mut c.cues, &c.content_ids, edits);
}

impl App {
    /// Every tick: while the collection is open, every [`LOOK_EVERY`] (and at once when it's
    /// opened), in the background, write its waiting edits if rekordbox and its agent have quit,
    /// else count them.
    pub(in crate::tui) fn watch_collection_writes(&mut self) {
        let Some(db) = self.loaded.as_ref().and_then(|l| l.collection.clone()) else { return };
        let w = &mut self.collection_writes;
        if w.in_flight || w.last.is_some_and(|at| at.elapsed() < LOOK_EVERY) || self.job.is_some() {
            return;
        }
        if w.refused_at.is_some_and(|at| at == super::drives::CollectionStamp::of(&db)) {
            return;
        }
        w.last = Some(Instant::now());
        w.in_flight = true;
        let store_root = self.store_root.clone();
        let job = move || {
            let key = masterdb::resolve_key(None);
            let write = Store::open_existing(&store_root)
                .and_then(|s| match s {
                    Some(s) => collection_write::flush(&s, &db, &key, &locallibrary::Ctx::silent()),
                    None => Ok(CollectionWrite::default()),
                })
                .map_err(|e| e.to_string());
            Msg::CollectionWrites(Box::new(Looked { db, write }))
        };
        self.spawn_or(job, |e| Msg::Note(e, Level::Error));
    }

    /// What [`Self::watch_collection_writes`] found.
    pub(in crate::tui) fn looked_at_collection(&mut self, looked: Looked) {
        let w = &mut self.collection_writes;
        w.in_flight = false;
        let open = self.loaded.as_ref().and_then(|l| l.collection.as_ref()).is_some_and(|db| *db == looked.db);
        match looked.write {
            Ok(write) => {
                let w = &mut self.collection_writes;
                w.waiting = write.waiting;
                w.why = write.why.clone();
                w.said = None;
                w.refused_at = None;
                if write.written > 0 {
                    let mut msg =
                        format!("wrote {} to rekordbox's collection{UNDO_HINT}", plural(write.written, "cue edit"));
                    if let Some(x) = write.warnings.first() {
                        msg.push_str(&format!(" · {x}"));
                    }
                    self.toast(msg, Level::Info);
                    self.last_backup = Some(self.drive_at(super::drives::collection_root()));
                    if open {
                        self.load_collection();
                    }
                }
            }
            Err(e) => {
                self.collection_writes.refused_at = Some(super::drives::CollectionStamp::of(&looked.db));
                if self.collection_writes.said.as_deref() != Some(e.as_str()) {
                    self.toast(
                        format!("the cue edits waiting for rekordbox's collection weren't written: {e}"),
                        Level::Error,
                    );
                    self.collection_writes.said = Some(e);
                }
            }
        }
    }

    /// The line the status bar shows while cue edits wait, if any do.
    pub fn collection_waiting(&self) -> Option<String> {
        let w = &self.collection_writes;
        (w.waiting > 0 && self.loaded.as_ref().is_some_and(Loaded::is_collection)).then(|| {
            let why = match &w.why {
                Some(y) if y.starts_with("rekordbox's agent") => "rekordbox's agent to quit",
                _ => "rekordbox to quit",
            };
            format!("{} waiting for {why}", plural(w.waiting, "cue edit"))
        })
    }

    /// `u` on the collection: take back the latest cue edit made here, after showing what that does.
    pub(in crate::tui) fn undo_collection(&mut self) {
        let Some(db) = self.collection_db.clone() else { return };
        let store_root = self.store_root.clone();
        self.plan_collection(move |_| {
            let store = Store::open_or_init(&store_root)?;
            let req = locallibrary::api::CollectionRef { db: Some(db), key: None };
            Ok(Box::new(locallibrary::api::collection_undo_plan(&store, &req)?))
        });
    }
}

#[cfg(test)]
mod tests;
