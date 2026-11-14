//! Drives and libraries: finding what's mounted, reading a drive, and showing what's read.

use std::path::PathBuf;
use std::sync::PoisonError;

use locallibrary::api;
use locallibrary::store::{LibraryRecord, SnapshotInfo};
use locallibrary::watch::VolumeWatcher;
use locallibrary::{Ctx, Device, Store, paths};
use ratatui::widgets::TableState;

use crate::tui::browse::{self, Category, NodeId, Problems, SavedNode};

use super::*;

/// Every library in the store with its snapshots, newest first.
pub type History = Vec<(LibraryRecord, Vec<SnapshotInfo>)>;

/// rekordbox exported over edits made to the open drive: replaying them is offered (`y`).
pub struct ReplayOffer {
    pub root: PathBuf,
    pub drive: String,
    /// The recorded edits (journal entries).
    pub edits: usize,
}

/// The view to come back to when the open drive is read again (after an edit, say).
struct ViewKeep {
    node: SavedNode,
    expanded: Vec<SavedNode>,
    tree_row: Option<SavedNode>,
    track: Option<String>,
    row: usize,
    offset: usize,
}

impl App {
    /// Full drive details (this reads every drive's database).
    /// Open the drive a path or name means, straight away: from the command line, or typed.
    pub(in crate::tui) fn open_drive(&mut self, spec: &str) -> Result<(), String> {
        let root = api::resolve_drive(spec).map_err(|e| e.to_string())?.root().to_path_buf();
        if !api::drives_mounted().iter().any(|m| m.root == root) {
            self.extra_roots.push(root.clone());
        }
        self.load_library(root);
        Ok(())
    }

    /// The first read of the drive list is under way: until it answers, nothing can be said about
    /// what's plugged in.
    pub fn scanning(&self) -> bool {
        self.drives_in_flight && !self.listed
    }

    pub(in crate::tui) fn refresh_drives(&mut self) {
        if self.offline {
            return;
        }
        if self.drives_in_flight {
            self.drives_again = true;
            return;
        }
        self.drives_in_flight = true;
        let extra = self.extra_roots.clone();
        let watcher = self.watcher.clone();
        let job = move || {
            // Watching starts before the list is read: a drive mounted while it's read shows
            // as a change at the next poll.
            watcher.lock().unwrap_or_else(PoisonError::into_inner).get_or_insert_with(VolumeWatcher::new);
            let mut list = api::list_drives(&api::ListDrives { all: false });
            for root in extra {
                if !list.iter().any(|d| d.root == root)
                    && let Ok(info) = api::drive_info(&api::DriveRef { drive: root.display().to_string() })
                {
                    list.push(info);
                }
            }
            Msg::Drives(list)
        };
        self.spawn_or(job, |e| Msg::Failed(Work::Drives, e));
    }

    pub(in crate::tui) fn refresh_status(&mut self) {
        if self.status_loading || self.offline {
            return;
        }
        self.status_loading = true;
        let root = self.store_root.clone();
        let job = move || {
            let r = Store::open(&root)
                .map_err(|_| "no store yet: saving a snapshot of a drive (e) makes one".to_string())
                .and_then(|s| {
                    let req = api::FleetRequest::default();
                    api::fleet_status(Some(&s), &req, &Ctx::silent()).map_err(|e| e.to_string())
                });
            Msg::Status(r)
        };
        self.spawn_or(job, |e| Msg::Status(Err(e)));
    }

    pub(super) fn refresh_history(&mut self) {
        let root = self.store_root.clone();
        let job = move || {
            let r = (|| -> Result<_, String> {
                let store = Store::open(&root)
                    .map_err(|_| "no store yet: `locallibrary pull <drive>` creates one".to_string())?;
                let mut out = Vec::new();
                for lib in store.libraries().map_err(|e| e.to_string())? {
                    let req = api::HistoryRequest { library: lib.name.clone(), limit: Some(200) };
                    let snaps = api::history(&store, &req).map_err(|e| e.to_string())?;
                    out.push((lib, snaps));
                }
                Ok(out)
            })();
            Msg::History(r)
        };
        self.spawn_or(job, |e| Msg::History(Err(e)));
    }

    /// Read a drive's library in the background. Reading the open drive again keeps the view.
    /// Opening one drive and then another shows the other, even if the first takes longer.
    pub(in crate::tui) fn load_library(&mut self, root: PathBuf) {
        let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.read_from(name, root.clone(), move || root);
    }

    /// Read `name` in the background from the drive `pick` answers (`first` when picking fails),
    /// and show it.
    pub(in crate::tui) fn read_from(
        &mut self,
        name: String,
        first: PathBuf,
        pick: impl FnOnce() -> PathBuf + Send + 'static,
    ) {
        self.loading = Some(format!("Reading {name}…"));
        self.load += 1;
        let load = self.load;
        let job = move || {
            let root = pick();
            let drive = api::DriveRef { drive: root.display().to_string() };
            let r = api::drive_library(&drive)
                .and_then(|view| Ok(Box::new(Loaded::prepare(Device::open_export(&root)?, view.library))));
            Msg::Library(load, root, r.map_err(|e| e.to_string()))
        };
        self.spawn_or(job, move |e| Msg::Library(load, first, Err(e)));
    }

    /// Look for tracks whose audio file is missing, in the background.
    fn check_files(&mut self) {
        let Some(l) = &self.loaded else { return };
        let (lib, root, generation) = (l.library.clone(), l.root().to_path_buf(), l.generation);
        self.spawn(move || {
            let missing = match Device::open(&root) {
                Ok(d) => lib
                    .tracks
                    .iter()
                    .map(|t| !d.checked_path(&paths::key(&t.file_path)).is_ok_and(|p| p.is_file()))
                    .collect(),
                Err(_) => Vec::new(),
            };
            Msg::Missing(generation, missing)
        });
    }

    /// The open drive went away, or came back.
    pub(super) fn check_present(&mut self, present: Option<(PathBuf, bool)>) {
        let Some((root, here)) = present else { return };
        let Some(l) = self.loaded.as_mut().filter(|l| l.root() == root) else { return };
        if !here && !l.stale {
            l.stale = true;
            self.picking = None;
            let name = self.drive_label(&root);
            self.toast(format!("{name} was unplugged: what's shown may be out of date"), Level::Error);
        } else if here && l.stale && self.loading.is_none() {
            self.load_library(root);
        }
    }

    /// Show a freshly read library. The drive that was already open keeps its view (list,
    /// folders, selection, marks); another drive starts at All tracks, or where the last run
    /// left it.
    pub(super) fn install(&mut self, root: PathBuf, mut new: Loaded) {
        let keep = self.loaded.as_ref().filter(|l| l.root() == new.root()).map(|l| self.view_keep(l));
        // The store's sets go with every drive: the ones read last stand in until they're read
        // again, so a set shown doesn't empty meanwhile.
        new.sets = self.loaded.as_mut().and_then(|l| l.sets.take());
        self.crate_list.refresh(new.root(), &new.library);
        self.queue_changed();
        self.loaded = Some(new);
        self.first_open();
        self.details = None;
        self.details_asked = None;
        match keep {
            Some(k) => self.restore_view(k),
            None => {
                self.query.clear();
                self.marks.clear();
                let restore = self.restore.take().filter(|(r, _)| *r == root).map(|(_, n)| n);
                let Some(l) = &mut self.loaded else { return };
                if let Some(id) = restore
                    .and_then(|n| l.find_node(&n))
                    .filter(|id| !matches!(id, NodeId::Browse | NodeId::Category(_)))
                {
                    l.reveal(&id);
                    l.node = id;
                }
                self.playlist_state.select(l.tree_position(&l.node).or(Some(0)));
                self.track_state = TableState::default().with_selected(Some(0));
                self.visible.clear();
                self.refresh_visible();
                if let Some(i) = self.drives.iter().position(|d| d.root == root) {
                    self.drive_state.select(Some(i));
                }
            }
        }
        self.restore_queue(&root);
        self.check_files();
        self.index_library();
        self.read_sets();
        self.check_replay();
        if self.show_tags_column {
            self.read_my_tags();
        }
        if !self.field_columns.is_empty() {
            self.read_fields();
        }
    }

    /// Whether rekordbox exported over edits recorded on the open drive, in the background: it
    /// exported after locallibrary last wrote to the drive (or saw it; see `Device::exported_at`),
    /// and replaying the recorded edits would change something. Then replaying them is offered.
    fn check_replay(&mut self) {
        if self.replay_offer.as_ref().is_some_and(|o| self.loaded.as_ref().is_none_or(|l| l.root() != o.root)) {
            self.replay_offer = None;
        }
        let Some(l) = self.loaded.as_ref() else { return };
        let (root, generation) = (l.root().to_path_buf(), l.generation);
        if l.stale || self.replay_dismissed.contains(&root) {
            self.replay_checked = generation;
            return;
        }
        self.spawn(move || {
            let drive = api::DriveRef { drive: root.display().to_string() };
            let edits = api::edits_list(&drive).map_or(0, |e| e.len());
            // Written since: later than the state locallibrary recorded last (allowing for FAT's
            // two-second times).
            let written_since = || {
                api::drive_info(&drive).is_ok_and(|i| match (i.exported_at, &i.last_known) {
                    (Some(at), Some(known)) => at > known.at + chrono::Duration::seconds(3),
                    (Some(_), None) => true,
                    (None, _) => false,
                })
            };
            let changes =
                edits > 0 && written_since() && api::edits_replay_plan(&drive, &Ctx::silent()).is_ok_and(|p| !p.noop);
            Msg::Replay(generation, if changes { edits } else { 0 })
        });
    }

    /// `y` with the offer up: replay the edits (shows the plan first).
    pub(super) fn take_replay_offer(&mut self) {
        let Some(o) = self.replay_offer.take() else { return };
        let drive = o.root.display().to_string();
        self.plan(o.root, move |ctx| Ok(Box::new(api::edits_replay_plan(&api::DriveRef { drive }, ctx)?)));
    }

    /// Work out every sort's ranks and Browse's groups in the background.
    fn index_library(&mut self) {
        let Some(l) = &self.loaded else { return };
        let (lib, generation, by_id) = (l.library.clone(), l.generation, l.by_id.clone());
        self.spawn(move || {
            let ranks = browse::SORT_FIELDS.iter().map(|&f| (f, browse::ranks(&lib, f))).collect();
            // Problems waits for the file check, and is quick; My Tags and Sets wait for their reads.
            let later = |c: &Category| matches!(c, Category::Problems | Category::MyTags | Category::Sets);
            let groups = (Category::ALL.into_iter().filter(|c| !later(c)))
                .map(|c| (c, browse::groups(&lib, c, &by_id, &Problems::default(), None, &[])))
                .collect();
            Msg::Indexed(generation, Box::new(Index { ranks, groups }))
        });
    }

    fn view_keep(&self, l: &Loaded) -> ViewKeep {
        let selected = self.track_state.selected();
        ViewKeep {
            node: l.save_node(&l.node),
            expanded: l.expanded.iter().map(|id| l.save_node(id)).collect(),
            tree_row: self.playlist_state.selected().and_then(|i| l.tree.get(i)).map(|n| l.save_node(&n.id)),
            track: selected.and_then(|s| self.visible.get(s)).map(|&i| l.library.tracks[i].file_path.clone()),
            row: selected.unwrap_or(0),
            offset: self.track_state.offset(),
        }
    }

    fn restore_view(&mut self, k: ViewKeep) {
        let Some(l) = &mut self.loaded else { return };
        for s in &k.expanded {
            if let Some(id) = l.find_node(s) {
                l.expanded.insert(id);
            }
        }
        l.node = l.find_node(&k.node).unwrap_or(NodeId::All);
        l.rebuild_tree();
        let row = k.tree_row.and_then(|s| l.find_node(&s)).and_then(|id| l.tree_position(&id));
        let rows = l.tree.len();
        self.playlist_state.select(Some(row.unwrap_or(self.playlist_state.selected().unwrap_or(0).min(rows - 1))));
        self.marks.retain(|p| l.index_of(p).is_some());
        let target = k.track.and_then(|p| l.index_of(&p));
        self.visible.clear();
        self.refresh_visible();
        let pos = target.and_then(|i| self.visible.iter().position(|&v| v == i));
        if !self.visible.is_empty() {
            self.track_state.select(Some(pos.unwrap_or(k.row.min(self.visible.len() - 1))));
            *self.track_state.offset_mut() = k.offset;
        }
    }
}
