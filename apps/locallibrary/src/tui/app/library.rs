//! The Library view's state: the tracks shown, searching and sorting, a track's details, marks and
//! the crate.

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::Path;
use std::time::{Duration, Instant};

use locallibrary::api;
use locallibrary::fields::TrackValues;
use locallibrary::model::normalize;
use locallibrary::query::{Filter, SortField};
use locallibrary::{Device, Store, Track};

use crate::tui::browse::{self, Category, NodeId};
use crate::tui::edits::{Action, Menu};

use super::*;

pub(super) fn plural_marks(n: usize) -> String {
    if n == 1 { "mark".into() } else { format!("{n} marks") }
}

impl App {
    /// Work out the track list: the list shown (or the whole drive, when searching it), narrowed
    /// by the search box, in the chosen order. Keeps the selected track selected if it's still
    /// there.
    pub fn refresh_visible(&mut self) {
        self.search_due = None;
        let query = self.query.trim().to_string();
        let search = browse::Search::parse(&query);
        let filter = (!search.rest.is_empty()).then(|| Filter::parse(&search.rest));
        let tags = filter.as_ref().is_some_and(|f| f.as_ref().is_ok_and(Filter::uses_tags));
        if tags {
            self.read_my_tags();
        }
        let fields = filter.as_ref().is_some_and(|f| f.as_ref().is_ok_and(Filter::uses_fields));
        if fields || self.sort.is_some_and(|k| matches!(k.field, SortField::Field(_))) {
            self.read_fields();
        }
        self.filter_note = None;
        let Some(l) = &mut self.loaded else {
            self.visible.clear();
            return;
        };
        let selected = self.track_state.selected().and_then(|s| self.visible.get(s)).copied();
        let query = query.as_str();
        let mut list =
            if self.search_all && !query.is_empty() { (0..l.library.tracks.len()).collect() } else { l.base() };
        match filter {
            Some(Err(e)) => {
                self.filter_error = Some(e.to_string());
                if self.searching && !self.visible.is_empty() {
                    return; // keep the last good result while the filter is being typed
                }
                let q = normalize(query);
                list.retain(|&i| l.haystack[i].contains(&q));
            }
            None => {
                self.filter_error = None;
                if search.has_words() {
                    list.retain(|&i| search.matches(&l.haystack[i]));
                }
            }
            Some(Ok(f)) => {
                self.filter_error = None;
                if search.has_words() {
                    list.retain(|&i| search.matches(&l.haystack[i]));
                }
                let lib = l.library.clone();
                // My Tags as `tracks --filter` reads them; until they're in, `tag:` matches nothing.
                let none = FxHashMap::default();
                let my_tags = match &l.my_tags {
                    MyTags::Read(t) => Some(t.clone()),
                    _ if !tags => None,
                    MyTags::Reading | MyTags::Unread => {
                        self.filter_note = Some("reading My Tags…".into());
                        None
                    }
                    MyTags::Failed(e) => {
                        self.filter_note = Some(format!("can't read My Tags: {e}"));
                        None
                    }
                };
                // What analyzers worked out, as `tracks --filter` reads it; until it's in, a
                // plugin's field matches nothing.
                let values = match &l.fields {
                    Fields::Read(v) => Some(v.clone()),
                    _ if !fields => None,
                    Fields::Reading | Fields::Unread => {
                        self.filter_note = Some("reading what analyzers worked out…".into());
                        None
                    }
                    Fields::Failed(e) => {
                        self.filter_note = Some(format!("can't read what analyzers worked out: {e}"));
                        None
                    }
                };
                let empty;
                let values = match (fields, values.as_deref()) {
                    (false, _) => None,
                    (true, Some(v)) => Some(v),
                    // Read with no values yet: what the fields are, to check the filter against.
                    (true, None) => {
                        let declared = crate::plugins::host().map(|p| p.fields()).unwrap_or_default();
                        empty = TrackValues { declared, ..TrackValues::default() };
                        Some(&empty)
                    }
                };
                // Refused as `tracks --filter` refuses it: a `playlist:` naming nothing would match
                // no track, and negated, every one.
                match api::filter_context(&lib, &f, tags.then(|| my_tags.as_deref().unwrap_or(&none)), values) {
                    Ok(ctx) => list.retain(|&i| f.matches(&lib.tracks[i], &ctx)),
                    Err(e) => {
                        self.filter_error = Some(e.to_string());
                        if self.searching && !self.visible.is_empty() {
                            return; // keep the last good result while the filter is being typed
                        }
                        list.clear();
                    }
                }
            }
        }
        if let Some(s) = self.sort {
            browse::sort_list(&mut list, l.rank(s.field), s.descending);
            // Tracks without a value to show for a plugin's field (none, or null) go last, either
            // way.
            if let SortField::Field(name) = s.field {
                let values = l.values();
                let id = |i: usize| l.library.tracks[i].id;
                list.sort_by_key(|&i| values.is_none_or(|v| v.shown(id(i), name.as_str()).is_none()));
            }
        }
        self.visible = list;
        let len = self.visible.len();
        let pos = selected.and_then(|i| self.visible.iter().position(|&v| v == i));
        let sel = pos.unwrap_or_else(|| self.track_state.selected().unwrap_or(0).min(len.saturating_sub(1)));
        self.track_state.select(if len == 0 { None } else { Some(sel) });
    }

    /// The search box changed. On a small library the list narrows at once; on a big one once
    /// typing pauses, so keys don't each wait for a pass over every track.
    pub(super) fn search_changed(&mut self) {
        let big = self.loaded.as_ref().is_some_and(|l| l.library.tracks.len() > TYPE_AHEAD);
        if big {
            self.search_due = Some(Instant::now() + Duration::from_millis(150));
        } else {
            self.refresh_visible();
        }
    }

    /// Narrow the list now if the search box is waiting to.
    pub(super) fn flush_search(&mut self) {
        if self.search_due.is_some() {
            self.refresh_visible();
        }
    }

    /// Group `c` again now there's more to group by (read in the background), keeping the tree's
    /// cursor on its row as rows come and go above it.
    pub(super) fn regroup(&mut self, c: Category) {
        let Some(l) = self.loaded.as_mut() else { return };
        let row = self.playlist_state.selected().and_then(|i| l.tree.get(i)).map(|n| n.id.clone());
        l.groups.remove(&c);
        l.rebuild_tree();
        if let Some(i) = row.and_then(|id| l.tree_position(&id)) {
            self.playlist_state.select(Some(i));
        }
    }

    /// Read the store's sets in the background, for Browse › Sets. Without a store there are
    /// none.
    pub(in crate::tui) fn read_sets(&mut self) {
        let Some(l) = &self.loaded else { return };
        let (store_root, generation) = (self.store_root.clone(), l.generation);
        let job = move || {
            let sets = match Store::open(&store_root) {
                Ok(store) => api::sets_list(&store, &api::SetsQuery::default()).map_err(|e| e.to_string()),
                Err(_) => Ok(Vec::new()),
            };
            Msg::Sets(generation, sets)
        };
        self.spawn_or(job, move |e| Msg::Sets(generation, Err(e)));
    }

    /// Read the open drive's My Tags in the background, the first time a filter needs them.
    pub(in crate::tui) fn read_my_tags(&mut self) {
        let Some(l) = self.loaded.as_mut().filter(|l| matches!(l.my_tags, MyTags::Unread)) else { return };
        l.my_tags = MyTags::Reading;
        let (root, generation) = (l.root().to_path_buf(), l.generation);
        let job = move || {
            let drive = api::DriveRef { drive: root.display().to_string() };
            Msg::MyTags(generation, api::tags_by_track(&drive).map_err(|e| e.to_string()))
        };
        self.spawn_or(job, move |e| Msg::MyTags(generation, Err(e)));
    }

    /// Read what the analyzers installed worked out for the open drive's tracks in the
    /// background, the first time a column, a filter or a sort needs it (again after analysing).
    pub(in crate::tui) fn read_fields(&mut self) {
        let Some(l) = self.loaded.as_mut().filter(|l| matches!(l.fields, Fields::Unread)) else { return };
        l.fields = Fields::Reading;
        let (root, library, generation) = (l.root().to_path_buf(), l.library.clone(), l.generation);
        let store_root = self.store_root.clone();
        let job = move || {
            let read = Device::open(&root).map(|device| {
                let installed = crate::plugins::installed(Store::open(&store_root).ok());
                Box::new(TrackValues::read(installed.source(), &device, &library))
            });
            Msg::Fields(generation, read.map_err(|e| e.to_string()))
        };
        self.spawn_or(job, move |e| Msg::Fields(generation, Err(e)));
    }

    /// The selected track, as an index into the library.
    pub fn selected_track(&self) -> Option<usize> {
        self.track_state.selected().and_then(|s| self.visible.get(s)).copied()
    }

    /// The tracks actions work on: the marked ones (those in view first, in list order), else the
    /// selected one.
    pub fn targets(&self) -> Vec<usize> {
        let Some(l) = &self.loaded else { return Vec::new() };
        if self.marks.is_empty() {
            return self.selected_track().into_iter().collect();
        }
        let mut seen = FxHashSet::default();
        let marked = |i: &usize| self.marks.contains(&l.library.tracks[*i].file_path);
        let mut out: Vec<usize> = self.visible.iter().copied().filter(marked).filter(|i| seen.insert(*i)).collect();
        out.extend((0..l.library.tracks.len()).filter(marked).filter(|i| seen.insert(*i)));
        out
    }

    /// The selected track as background work keeps it: the library load's generation, and its
    /// index there.
    pub fn selected_key(&self) -> Option<(u64, usize)> {
        Some((self.loaded.as_ref()?.generation, self.selected_track()?))
    }

    /// Whether the details shown are for the selected track.
    pub fn details_current(&self) -> bool {
        let (Some(l), Some(d)) = (&self.loaded, &self.details) else { return false };
        d.generation == l.generation && Some(d.index) == self.selected_track()
    }

    /// Whether looking up the selected track's details failed (the log says why).
    pub fn details_lookup_failed(&self) -> bool {
        let Some(l) = &self.loaded else { return false };
        self.details_failed.is_some() && self.details_failed == self.selected_track().map(|i| (l.generation, i))
    }

    /// The match selected in the details panel, as an index into the open library.
    pub fn selected_match(&self) -> Option<usize> {
        if !self.details_current() {
            return None;
        }
        let d = self.details.as_ref()?;
        d.matches.items.get(self.details_state.selected()?).map(|m| m.index)
    }

    /// Look up the selected track's matches once the selection has settled for a moment, so
    /// holding an arrow key doesn't start a search per row.
    pub(super) fn want_details(&mut self) {
        if !self.show_details || self.view != View::Library {
            return;
        }
        let (Some(l), Some(i)) = (&self.loaded, self.selected_track()) else { return };
        let generation = l.generation;
        if self.details_current() || [self.details_asked, self.details_failed].contains(&Some((generation, i))) {
            return;
        }
        match self.details_seen {
            Some((g, j, at)) if (g, j) == (generation, i) => {
                if at.elapsed() < Duration::from_millis(90) {
                    return;
                }
            }
            _ => {
                self.details_seen = Some((generation, i, Instant::now()));
                return;
            }
        }
        self.details_asked = Some((generation, i));
        let (lib, root) = (l.library.clone(), l.root().to_path_buf());
        let job = move || {
            let track = lib.tracks[i].clone();
            let playlists = playlists_of(&lib, &track);
            let matches = Matches::find(lib, root, &track, 60);
            Msg::Details(Box::new(Details { generation, index: i, matches, playlists }))
        };
        self.spawn_or(job, move |e| Msg::Failed(Work::Details(generation, i), e));
    }

    pub(super) fn toggle_mark(&mut self) {
        if self.focus == Focus::Playlists {
            return self.toggle_list_mark();
        }
        if self.focus != Focus::Tracks {
            return;
        }
        let Some(path) =
            self.selected_track().and_then(|i| Some(self.loaded.as_ref()?.library.tracks[i].file_path.clone()))
        else {
            return;
        };
        if !self.marks.remove(&path) {
            self.marks.insert(path);
        }
        self.move_selection(1);
    }

    /// `m` in the tree: mark the playlist or folder there (to pack it), or unmark it.
    fn toggle_list_mark(&mut self) {
        let Some(l) = &self.loaded else { return };
        let Some(NodeId::Playlist(id)) =
            self.playlist_state.selected().and_then(|i| l.tree.get(i)).map(|n| n.id.clone())
        else {
            return;
        };
        let path = l.playlist_path(id);
        match self.list_marks.iter().position(|p| *p == path) {
            Some(i) => {
                self.list_marks.remove(i);
            }
            None => self.list_marks.push(path),
        }
        self.move_selection(1);
    }

    /// Mark every track in view, or, with marks already, clear them.
    pub(super) fn mark_all(&mut self) {
        let Some(l) = &self.loaded else { return };
        if self.marks.is_empty() {
            self.marks.extend(self.visible.iter().map(|&i| l.library.tracks[i].file_path.clone()));
            let n = self.marks.len();
            self.toast(format!("{n} marked"), Level::Info);
        } else {
            self.marks.clear();
            self.toast("marks cleared", Level::Info);
        }
    }

    /// Add tracks on the drive at `root` (by file path) to the crate.
    pub(in crate::tui) fn add_to_crate(&mut self, root: &Path, paths: &[String]) {
        let Some(l) = self.loaded.as_ref().filter(|l| l.root() == root) else { return };
        let tracks: Vec<Track> =
            paths.iter().filter_map(|p| l.index_of(p)).map(|i| l.library.tracks[i].clone()).collect();
        let n = tracks.len();
        let drive = self.drive_label(root);
        let added = self.crate_list.add(root, &drive, tracks);
        let total = self.crate_list.len();
        let msg = match (added, n - added) {
            (0, _) => "already in the crate".to_string(),
            (a, 0) => format!("added {a} to the crate ({total} in all)"),
            (a, d) => format!("added {a} to the crate ({d} already there)"),
        };
        self.toast(msg, Level::Info);
        if self.crate_state.selected().is_none() && total > 0 {
            self.crate_state.select(Some(0));
        }
    }

    pub(super) fn shift_crate(&mut self, delta: isize) {
        if let Some(i) = self.crate_state.selected() {
            let to = self.crate_list.shift(i, delta);
            self.crate_state.select(Some(to));
        }
    }

    pub(super) fn remove_from_crate(&mut self) {
        let Some(i) = self.crate_state.selected() else { return };
        if self.crate_list.remove(i).is_some() {
            let len = self.crate_list.len();
            self.crate_state.select(if len == 0 { None } else { Some(i.min(len - 1)) });
        }
    }

    /// An action from a key rather than the menu: done if it can be, else say why not.
    pub(super) fn quick(&mut self, action: Action) {
        let Some(t) = self.target() else { return };
        let menu = Menu::new(t, None);
        match menu.items.iter().find(|i| i.action == action).and_then(|i| i.unavailable.clone()) {
            Some(why) => self.toast(why, Level::Info),
            None => self.run_action(action, menu.target),
        }
    }
}
