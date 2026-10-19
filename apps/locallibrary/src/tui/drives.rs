//! The Drives view's work: what can be done to a drive from here, each planned in the background
//! and confirmed in the same dialog as an edit (checking, which changes nothing, shows its
//! results in a panel instead): save it in the store, check it (or its whole group) for the
//! players it's going to, sync its group from the newest drive, pack a set onto it, bring back a
//! playlist from a snapshot, replay the edits a rekordbox export went over, undo the last change,
//! eject it. Every one of them is a library `*_plan` (or `check`) call: the player only asks
//! which drive, playlists or snapshot.

use std::path::{Path, PathBuf};

use locallibrary::api::{self, CheckRequest, DriveRef, EjectDrives, FleetDrive, PullRequest};
use locallibrary::check::{CheckOptions, CheckReport, Level as CheckLevel};
use locallibrary::export::{self, Format, Locator, PathStyle};
use locallibrary::local_time;
use locallibrary::pack::PackOptions;
use locallibrary::{Ctx, DeviceInfo, Error, Store, Track, fsutil};
use loclib_plugin::Where;
use uuid::Uuid;

use super::app::{App, Focus, Level, View};
use super::browse::{Category, NodeId};
use super::edits::{Modal, Prompt, PromptFor};
use super::keymap::{self, Act};
use super::palette::{Item, Picker, Then};

/// What happens once a confirmed change has been made: the next step of something that takes
/// two (saving the crate as a playlist, then packing it).
#[derive(Debug, Clone)]
pub enum After {
    /// Pack `playlist` from `source` onto `target`.
    Pack { source: PathBuf, target: PathBuf, playlist: String },
}

/// What goes onto a drive packed for a set.
#[derive(Debug, Clone)]
pub enum PackWhat {
    /// These playlists and folders, by path.
    Playlists(Vec<String>),
    /// The crate's tracks (from the source), saved there as a playlist first.
    Crate(Vec<String>),
    /// What the target was packed with last time.
    Again,
}

/// A panel of results (a check's), scrolled with the arrows and closed with any other key.
pub struct Report {
    pub title: String,
    pub lines: Vec<(Tone, String)>,
    pub scroll: u16,
}

/// How a line of a [`Report`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Heading,
    Ok,
    Warning,
    Problem,
    Text,
    Faint,
    /// A table's headings, and its rows: cut at the panel's edge rather than wrapped, so the
    /// columns stay aligned.
    Columns,
    Row,
}

/// The lines of a check's report: how it went, then each drive's items, problems and warnings
/// first, then the tracks the players won't load.
pub fn check_lines(r: &CheckReport) -> Vec<(Tone, String)> {
    let mut out = Vec::new();
    let verdict = match (r.problems, r.warnings) {
        (0, 0) => (Tone::Ok, "Ready: no problems, no warnings.".to_string()),
        (0, w) => (Tone::Warning, format!("No problems, {w} warning{}.", if w == 1 { "" } else { "s" })),
        (p, w) => (Tone::Problem, format!("{p} problem{}, {w} warning{}.", s(p), s(w))),
    };
    out.push(verdict);
    for p in &r.players {
        let models = if p.models.is_empty() { "no player models".to_string() } else { p.models.join(", ") };
        let from = match p.from {
            locallibrary::check::PlayersFrom::Given => "",
            locallibrary::check::PlayersFrom::Library => " (remembered for its library)",
            locallibrary::check::PlayersFrom::Default => " (remembered)",
            locallibrary::check::PlayersFrom::None => " (locallibrary players set remembers some)",
        };
        out.push((Tone::Faint, format!("{}: {models}{from}", p.drive)));
    }
    for drive in &r.drives {
        out.push((Tone::Text, String::new()));
        out.push((Tone::Heading, drive.clone()));
        let mut items: Vec<_> = r.items.iter().filter(|i| &i.drive == drive).collect();
        items.sort_by_key(|i| std::cmp::Reverse(i.level));
        for i in items {
            let tone = match i.level {
                CheckLevel::Ok => Tone::Ok,
                CheckLevel::Warning => Tone::Warning,
                CheckLevel::Problem => Tone::Problem,
            };
            out.push((tone, i.message.clone()));
        }
        for u in r.unloadable.iter().filter(|u| &u.drive == drive) {
            out.push((Tone::Text, format!("{} won't load on the {}:", u.message, u.player)));
            for t in u.tracks.iter().take(5) {
                out.push((Tone::Faint, format!("    {}", t.name)));
            }
            if u.tracks.len() > 5 {
                out.push((Tone::Faint, format!("    and {} more", u.tracks.len() - 5)));
            }
        }
    }
    out
}

fn s(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// A row of the Drives view: a library (its clones plugged in follow), the drives of no library
/// (they follow), or a drive (by its place in [`App::drives`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveRow {
    Library(Uuid),
    Unlinked,
    Drive(usize),
}

/// A drive remembered to act on later (the drive menu's, the one `u` undoes on): where it was
/// mounted, and its identity when it has one, so a drive mounted there since (two sticks with one
/// volume name mount at the same path) isn't taken for it.
#[derive(Debug, Clone, PartialEq)]
pub struct DriveKey {
    pub root: PathBuf,
    pub id: Option<Uuid>,
}

impl App {
    /// The drive mounted at `root`, to find again later.
    pub(super) fn drive_at(&self, root: PathBuf) -> DriveKey {
        let id = self.drives.iter().find(|d| d.root == root).and_then(|d| d.id);
        DriveKey { root, id }
    }

    /// Where the drive `key` is mounted now: by its identity, wherever that is (for a drive
    /// without one, at the same mount path). `None` once it's gone.
    pub(super) fn mounted(&self, key: &DriveKey) -> Option<PathBuf> {
        let found = self.drives.iter().find(|d| match key.id {
            Some(id) => d.id == Some(id),
            None => d.root == key.root,
        });
        found.map(|d| d.root.clone())
    }

    /// The mounted drive at `root` as the Drives view knows it (role, library, logbook).
    pub(super) fn fleet_drive(&self, root: &Path) -> Option<&FleetDrive> {
        match &self.status {
            Some(Ok(list)) => list.iter().find(|f| f.status.drive.root == root),
            _ => None,
        }
    }

    /// The library the drive at `root` belongs to, by name, when the store has it.
    fn library_of(&self, root: &Path) -> Option<String> {
        self.fleet_drive(root).and_then(|f| f.status.library.clone())
    }

    /// The mounted drives of the library the drive at `root` belongs to (it first): its group.
    fn group_of(&self, root: &Path) -> Vec<PathBuf> {
        let mut out = vec![root.to_path_buf()];
        if let (Some(lib), Some(Ok(list))) = (self.library_of(root), &self.status) {
            let others =
                list.iter().filter(|f| f.status.library.as_deref() == Some(&lib) && f.status.drive.root != root);
            out.extend(others.map(|f| f.status.drive.root.clone()));
        }
        out
    }

    /// The Drives view's row of a library (or of the drives of none) when that's what's selected.
    pub(super) fn header(&self) -> Option<DriveRow> {
        self.drive_header.filter(|_| self.view == View::Drives)
    }

    /// The library the store knows the drive `d` as a clone of.
    pub(super) fn library_id(&self, d: &DeviceInfo) -> Option<Uuid> {
        d.library_id.filter(|id| self.library_names.contains_key(id))
    }

    /// The Drives view's rows: each library by name with its drives under it, then the drives of
    /// none (under a row of their own when a library is above them).
    pub(super) fn drive_rows(&self) -> Vec<DriveRow> {
        let mut libraries: Vec<Uuid> = self.drives.iter().filter_map(|d| self.library_id(d)).collect();
        libraries.sort_by_key(|id| (self.library_names[id].to_lowercase(), *id));
        libraries.dedup();
        let drives_of = |id| (0..self.drives.len()).filter(move |&i| self.library_id(&self.drives[i]) == id);
        let mut rows = Vec::new();
        for &id in &libraries {
            rows.push(DriveRow::Library(id));
            rows.extend(drives_of(Some(id)).map(DriveRow::Drive));
        }
        let loose: Vec<DriveRow> = drives_of(None).map(DriveRow::Drive).collect();
        if !libraries.is_empty() && !loose.is_empty() {
            rows.push(DriveRow::Unlinked);
        }
        rows.extend(loose);
        rows
    }

    /// Where the selection is among `rows`: the library's row selected, else the selected drive's.
    pub(super) fn drive_row(&self, rows: &[DriveRow]) -> Option<usize> {
        let selected = self.drive_header.or(self.drive_state.selected().map(DriveRow::Drive))?;
        rows.iter().position(|r| *r == selected)
    }

    /// Select a row of the Drives view: a drive's is the drive the view's actions work on.
    pub(super) fn select_drive_row(&mut self, row: DriveRow) {
        match row {
            DriveRow::Drive(i) => {
                self.drive_header = None;
                self.drive_state.select(Some(i));
            }
            header => self.drive_header = Some(header),
        }
    }

    /// Move the Drives view's selection `delta` rows.
    pub(super) fn move_drive_row(&mut self, delta: isize) {
        let rows = self.drive_rows();
        let Some(last) = rows.len().checked_sub(1) else { return };
        let at = self.drive_row(&rows).unwrap_or(0) as isize;
        self.select_drive_row(rows[(at + delta).clamp(0, last as isize) as usize]);
    }

    /// The library's drives plugged in that have an export: its clones here.
    pub(super) fn clones(&self, id: Uuid) -> Vec<PathBuf> {
        let ours = |d: &&DeviceInfo| d.has_export && self.library_id(d) == Some(id);
        self.drives.iter().filter(ours).map(|d| d.root.clone()).collect()
    }

    /// Show the library `id` in the library view: its lead clone, as commands given the library
    /// find it (see [`api::resolve_drive`]: archive and retired drives aren't among them), found
    /// in the background; the first of its drives when there's none.
    pub(super) fn open_library(&mut self, id: Uuid) {
        let name = self.library_names.get(&id).cloned().unwrap_or_default();
        let Some(first) = self.clones(id).first().cloned() else {
            self.toast(format!("none of {name}'s drives has a rekordbox export"), Level::Info);
            return;
        };
        self.read_from(name, first.clone(), move || {
            api::resolve_drive(&id.to_string()).map_or(first, |d| d.root().to_path_buf())
        });
    }

    /// Enter on a library's row in the Drives view: show the library.
    pub(super) fn activate_header(&mut self) {
        let Some(DriveRow::Library(id)) = self.drive_header else {
            self.toast("select one of these drives", Level::Info);
            return;
        };
        self.open_library(id);
        self.view = View::Library;
        self.focus = Focus::Playlists;
    }

    /// `e` on a library's row: what can be done to all of its clones plugged in.
    fn library_menu(&mut self, id: Uuid) {
        let name = self.library_names.get(&id).cloned().unwrap_or_default();
        let items = [
            (Act::SyncGroup, "sync its clones from the newest"),
            (Act::SetPrep, "check its clones for a set"),
            (Act::Eject, "eject its clones"),
        ];
        let (acts, items): (Vec<Act>, Vec<Item>) =
            items.into_iter().map(|(a, label)| (a, Item::new(label, ""))).unzip();
        self.modal = Some(Modal::Pick(Picker::new(format!("Library · {name}"), items, Then::Library { id, acts })));
    }

    /// One of a library's actions (see [`Self::library_menu`]), on its clones plugged in.
    pub(super) fn library_act(&mut self, act: Act, id: Uuid) {
        let name = self.library_names.get(&id).cloned().unwrap_or_default();
        let clones = self.clones(id);
        let Some(first) = clones.first().cloned() else {
            self.toast(format!("none of {name}'s drives is plugged in any more"), Level::Info);
            return;
        };
        match act {
            Act::SyncGroup => self.sync_group(first),
            Act::SetPrep => self.check(clones),
            Act::Eject => self.eject(clones),
            _ => self.toast(format!("that's for one drive: select one of {name}'s"), Level::Info),
        }
    }

    /// The drive to act on, or a message saying there's none.
    fn acting_on(&mut self) -> Option<PathBuf> {
        let root = self.action_drive();
        if root.is_none() {
            self.toast("no drive: plug one in, or open one in the library", Level::Info);
        }
        root
    }

    /// One of a drive's actions (see [`Self::drive_menu`]), by a key: on the drive the keys act
    /// on.
    pub(super) fn drive_key(&mut self, act: Act) {
        if let Some(header) = self.header() {
            return match header {
                DriveRow::Library(id) => self.library_act(act, id),
                _ => self.toast("select one of these drives", Level::Info),
            };
        }
        if let Some(root) = self.acting_on() {
            self.drive_act(act, root);
        }
    }

    /// One of a drive's actions, on the drive at `root`.
    pub(super) fn drive_act(&mut self, act: Act, root: PathBuf) {
        match act {
            Act::Pull => self.pull(root),
            Act::Check => self.check(vec![root]),
            Act::SetPrep => self.check(self.group_of(&root)),
            Act::SyncGroup => self.sync_group(root),
            Act::Pack => self.pack(Some(root)),
            Act::RestorePlaylist => self.restore_playlist(root),
            Act::Replay => self.replay(root),
            Act::Undo => self.undo_on(root),
            Act::Eject => self.eject(vec![root]),
            Act::Plugin(plugin, name) => self.plugin_act(plugin, name, Some(root), None),
            Act::Analyze(plugin, analyzer) => self.analyze(plugin, analyzer, Some(root)),
            Act::Export(plugin, format) => self.plugin_export(plugin, format, Some(root)),
            act => self.perform(act, None),
        }
    }

    /// `e` in the Drives view: what can be done to the selected drive (plugins' commands and
    /// operations last). What's chosen is done to that drive (by its identity: see [`DriveKey`]),
    /// even if another is selected by then, or refused once it's been unplugged.
    pub(super) fn drive_menu(&mut self) {
        match self.header() {
            Some(DriveRow::Library(id)) => return self.library_menu(id),
            Some(_) => return self.toast("select one of these drives", Level::Info),
            None => {}
        }
        let Some(root) = self.acting_on() else { return };
        let name = self.drive_label(&root);
        let export = self.drives.iter().find(|d| d.root == root).is_none_or(|d| d.has_export);
        let no_export = (!export).then(|| "no rekordbox export on it".to_string());
        // `u` undoes on the drive changed last this run, which may be another.
        let undo_key = if self.undo_drive().as_ref() == Some(&root) { "u" } else { "" };
        let item = |act: Act, key: &str, why: &Option<String>| {
            (act, Item { label: keymap::name(act).into(), detail: key.into(), unavailable: why.clone() })
        };
        let items = vec![
            item(Act::Pull, "", &no_export),
            item(Act::Check, "", &no_export),
            item(Act::SetPrep, "", &no_export),
            item(Act::SyncGroup, "", &no_export),
            item(Act::Pack, "", &None),
            item(Act::RestorePlaylist, "", &no_export),
            item(Act::Replay, "", &no_export),
            item(Act::Undo, undo_key, &None),
            item(Act::Eject, "", &None),
        ];
        let (acts, items): (Vec<Act>, Vec<Item>) =
            items.into_iter().chain(self.plugin_items(Some(&root), &[Where::Drive])).unzip();
        let drive = self.drive_at(root);
        self.modal =
            Some(Modal::Pick(Picker::new(format!("Drive · {name}"), items, Then::Drive { drive, name, acts })));
    }

    /// Save the drive in the store (a snapshot of its library), once the plan is confirmed.
    pub(super) fn pull(&mut self, root: PathBuf) {
        let (drive, store_root) = (root.display().to_string(), self.store_root.clone());
        self.plan(root, move |ctx| {
            let req = PullRequest { drive, ..Default::default() };
            Ok(Box::new(api::pull_plan(&store_root, &req, ctx)?))
        });
    }

    /// Eject the drives (playback from them stops first), once the plan is confirmed.
    pub(super) fn eject(&mut self, roots: Vec<PathBuf>) {
        let Some(first) = roots.first().cloned() else { return };
        let drives = roots.iter().map(|r| r.display().to_string()).collect();
        let store_root = self.store_root.clone();
        self.plan(first, move |_| Ok(Box::new(api::drive_eject_plan(Some(&store_root), &EjectDrives { drives })?)));
    }

    /// Check the drives (several: the mounted drives of a library, as for a set) against the
    /// player models remembered for them, in the background; the results show in a panel.
    pub(super) fn check(&mut self, drives: Vec<PathBuf>) {
        let Some(name) = drives.first().map(|r| self.drive_label(r)) else { return };
        let label = match drives.len() {
            1 => format!("Checking {name}"),
            n => format!("Checking {name} and {} more", n - 1),
        };
        let store_root = self.store_root.clone();
        let specs: Vec<String> = drives.iter().map(|d| d.display().to_string()).collect();
        self.task(label, move |ctx| {
            let store = Store::open(&store_root).ok();
            let req = CheckRequest { drives: specs, options: CheckOptions::default() };
            let report = api::check(store.as_ref(), &req, ctx)?;
            let lines = check_lines(&report);
            let title = if report.drives.len() == 1 {
                format!("Check · {}", report.drives[0])
            } else {
                format!("Check · {}", report.drives.join(", "))
            };
            Ok(Box::new(move |app: &mut App| {
                app.modal = Some(Modal::Report(Report { title, lines, scroll: 0 }));
            }))
        });
    }

    /// Sync the drive's group (the mounted drives of its library) from the newest of them, once
    /// the plan is confirmed. When none is newest, planning says why.
    pub(super) fn sync_group(&mut self, root: PathBuf) {
        let name = self.drive_label(&root);
        let Some(library) = self.library_of(&root) else {
            let msg = if self.status.is_none() {
                "reading the drives' status: try again in a moment (the Drives view shows it)".to_string()
            } else {
                format!("{name} isn't in the store yet: save a snapshot of it first")
            };
            self.refresh_status();
            self.toast(msg, Level::Info);
            return;
        };
        if self.group_of(&root).len() < 2 {
            self.toast(
                format!("{name} is the only drive of {library} plugged in: plug in the others to sync them"),
                Level::Info,
            );
            return;
        }
        let store_root = self.store_root.clone();
        self.plan(root, move |ctx| {
            let store = Store::open(&store_root)?;
            let req = api::SyncSetRequest { library: Some(library), ..Default::default() };
            Ok(Box::new(api::sync_set_plan(&store, &req, ctx)?))
        });
    }

    /// Replay the edits recorded on the drive (after rekordbox exported over them), once the
    /// plan is confirmed.
    pub(super) fn replay(&mut self, root: PathBuf) {
        let drive = root.display().to_string();
        self.plan(root, move |ctx| Ok(Box::new(api::edits_replay_plan(&DriveRef { drive }, ctx)?)));
    }

    // -----------------------------------------------------------------------------------------
    // Packing a set onto a drive
    // -----------------------------------------------------------------------------------------

    /// Pack a set: from the drive open in the library, the marked playlists, the playlist shown
    /// or the crate, onto another drive (`onto`, unless it's that one, or one picked).
    pub(super) fn pack(&mut self, onto: Option<PathBuf>) {
        let Some(l) = self.loaded.as_ref().filter(|l| !l.stale) else {
            self.toast("open the drive to pack from in the library first", Level::Info);
            return;
        };
        let source = l.root().to_path_buf();
        let target = onto.filter(|r| *r != source);
        let mut choices = Vec::new();
        let mut items = Vec::new();
        if !self.list_marks.is_empty() {
            items.push(Item::new(format!("the marked playlists: {}", self.list_marks.join(", ")), ""));
            choices.push(PackWhat::Playlists(self.list_marks.clone()));
        }
        let shown = match (&l.node, self.focus) {
            (_, Focus::Playlists) => self.playlist_state.selected().and_then(|i| l.tree.get(i)).map(|n| n.id.clone()),
            (id, _) => Some(id.clone()),
        };
        if let Some(NodeId::Playlist(id)) = shown
            && self.list_marks.is_empty()
        {
            let path = l.playlist_path(id);
            items.push(Item::new(format!("the playlist {path}"), ""));
            choices.push(PackWhat::Playlists(vec![path]));
        }
        let crated: Vec<String> =
            (self.crate_list.items.iter()).filter(|t| t.root == source).map(|t| t.track.file_path.clone()).collect();
        if !crated.is_empty() {
            let n = crated.len();
            items.push(Item::new(format!("the crate's {n} track{} (saved as a playlist first)", s(n)), "t"));
            choices.push(PackWhat::Crate(crated));
        }
        items.push(Item::new("what the drive was packed with last time", ""));
        choices.push(PackWhat::Again);
        let title = format!("Pack from {} · what goes on", self.drive_label(&source));
        self.modal = Some(Modal::Pick(Picker::new(title, items, Then::PackWhat { source, target, choices })));
    }

    /// What to pack was picked: now the drive (unless it's known).
    pub(super) fn pack_what(&mut self, source: PathBuf, target: Option<PathBuf>, what: PackWhat) {
        if let Some(target) = target {
            return self.pack_onto(source, target, what);
        }
        let roots: Vec<PathBuf> = self.drives.iter().map(|d| d.root.clone()).filter(|r| *r != source).collect();
        if roots.is_empty() {
            self.toast("plug in the drive to pack onto", Level::Info);
            return;
        }
        let items = roots
            .iter()
            .map(|r| {
                let role = self.fleet_drive(r).and_then(|f| f.role).map(role_name).unwrap_or_default();
                Item::new(self.drive_label(r), role)
            })
            .collect();
        let title = "Pack onto which drive? (what's on it is replaced)".to_string();
        self.modal = Some(Modal::Pick(Picker::new(title, items, Then::PackOnto { source, what, roots })));
    }

    /// Pack `what` from `source` onto `target`: the crate is saved as a playlist on the source
    /// first (asked for a name), then the pack is planned.
    pub(super) fn pack_onto(&mut self, source: PathBuf, target: PathBuf, what: PackWhat) {
        match what {
            PackWhat::Crate(tracks) => {
                let today = chrono::Local::now().format("%Y-%m-%d");
                let label = format!("Save the crate as (then pack it onto {})", self.drive_label(&target));
                self.modal = Some(Modal::Prompt(Prompt {
                    label,
                    text: format!("Sets/{today}"),
                    purpose: PromptFor::CrateForPack { tracks, target },
                    root: source,
                }));
            }
            PackWhat::Playlists(playlists) => self.plan_pack(source, target, playlists),
            PackWhat::Again => self.plan_pack(source, target, Vec::new()),
        }
    }

    /// Plan packing `playlists` (none: the selection it was packed with) from `source` onto
    /// `target`.
    pub(super) fn plan_pack(&mut self, source: PathBuf, target: PathBuf, playlists: Vec<String>) {
        let req = api::PackRequest {
            source: source.display().to_string(),
            target: target.display().to_string(),
            options: PackOptions { playlists, ..Default::default() },
        };
        self.plan(target, move |ctx| Ok(Box::new(api::pack_plan(&req, ctx)?)));
    }

    // -----------------------------------------------------------------------------------------
    // Bringing back a playlist
    // -----------------------------------------------------------------------------------------

    /// Bring back a playlist or folder onto the drive: asks which (the one selected or shown to
    /// begin with), then from which snapshot.
    pub(super) fn restore_playlist(&mut self, root: PathBuf) {
        let guess = self
            .loaded
            .as_ref()
            .filter(|l| l.root() == root)
            .and_then(|l| {
                let id = match self.focus {
                    Focus::Playlists if self.view == View::Library => {
                        self.playlist_state.selected().and_then(|i| l.tree.get(i)).map(|n| n.id.clone())
                    }
                    _ => Some(l.node.clone()),
                };
                match id {
                    Some(NodeId::Playlist(p)) => Some(l.playlist_path(p)),
                    _ => None,
                }
            })
            .unwrap_or_default();
        self.modal = Some(Modal::Prompt(Prompt {
            label: "Bring back the playlist or folder".into(),
            text: guess,
            purpose: PromptFor::RestorePlaylist,
            root,
        }));
    }

    /// The playlist to bring back is named: list the snapshots of the drive's library to take
    /// it from (read in the background).
    pub(super) fn pick_snapshot(&mut self, root: PathBuf, playlist: String) {
        let library = self.drives.iter().find(|d| d.root == root).and_then(|d| d.library_id);
        let store_root = self.store_root.clone();
        self.task("Reading the snapshots".into(), move |_| {
            let store = Store::open(&store_root)
                .map_err(|_| Error::NotFound("a store: locallibrary pull saves the drive in one".into()))?;
            let lib = api::libraries_list(&store)?.into_iter().find(|l| Some(l.id) == library);
            let Some(lib) = lib else {
                return Err(Error::NotFound("the drive's library in the store: pull it first".into()));
            };
            let snaps = api::history(&store, &api::HistoryRequest { library: lib.name.clone(), limit: Some(100) })?;
            Ok(Box::new(move |app: &mut App| {
                let mut items = vec![Item::new("the newest snapshot that has it", "")];
                let mut ids = vec![None];
                for s in snaps {
                    let label = match (&s.message, s.labels.first()) {
                        (_, Some(l)) => format!("{}  {l}", local_time(s.created, "%Y-%m-%d %H:%M")),
                        (Some(m), None) => format!("{}  {m}", local_time(s.created, "%Y-%m-%d %H:%M")),
                        (None, None) => local_time(s.created, "%Y-%m-%d %H:%M").to_string(),
                    };
                    items.push(Item::new(label, s.short_id.clone()));
                    ids.push(Some(s.short_id));
                }
                let title = format!("Bring back {playlist} from {}", lib.name);
                let then = Then::Snapshot { root, playlist, snapshots: ids };
                app.modal = Some(Modal::Pick(Picker::new(title, items, then)));
            }))
        });
    }

    /// Plan bringing `playlist` back onto the drive at `root` from `snapshot` (the newest that
    /// has it when `None`).
    pub(super) fn plan_restore(&mut self, root: PathBuf, playlist: String, snapshot: Option<String>) {
        let store_root = self.store_root.clone();
        let req = api::PlaylistRestoreRequest { drive: root.display().to_string(), playlist, snapshot, backup: None };
        self.plan(root, move |ctx| {
            let store = Store::open(&store_root).ok();
            Ok(Box::new(api::playlist_restore_plan(store.as_ref(), &req, ctx)?))
        });
    }

    /// Enter in the Snapshots view: bring a playlist back from the selected snapshot onto the
    /// drive open in the library.
    pub(super) fn restore_from_selected_snapshot(&mut self) {
        let Some(root) = self.loaded.as_ref().map(|l| l.root().to_path_buf()) else {
            self.toast("open the drive to bring a playlist back to in the library first", Level::Info);
            return;
        };
        let Some(Ok(libs)) = &self.history else { return };
        let Some(snap) = libs.iter().flat_map(|(_, s)| s).nth(self.history_state.selected().unwrap_or(0)) else {
            return;
        };
        let id = snap.short_id.clone();
        self.modal = Some(Modal::Prompt(Prompt {
            label: format!("Bring back from {} the playlist or folder", &id[..8.min(id.len())]),
            text: String::new(),
            purpose: PromptFor::RestoreFrom { snapshot: id },
            root,
        }));
    }
}

impl App {
    // -----------------------------------------------------------------------------------------
    // Files on this computer
    // -----------------------------------------------------------------------------------------

    /// Write the list shown (a playlist or folder, a set, a session, or every track) to a file as
    /// M3U8, CSV or text: asks where first.
    pub(super) fn export_list(&mut self) {
        let Some(l) = &self.loaded else {
            self.toast("open a drive first", Level::Info);
            return;
        };
        let (list, name) = match &l.node {
            NodeId::All => (ExportOf::Everything, l.device.name()),
            NodeId::Playlist(id) => (ExportOf::Playlist(l.playlist_path(*id)), l.playlist_path(*id)),
            NodeId::Group(Category::History, _) => {
                let name = l.node_title(&l.node).rsplit(" › ").next().unwrap_or_default().to_string();
                (ExportOf::History(name.clone()), name)
            }
            NodeId::Group(Category::Sets, key) => {
                let set = l.grouped(Category::Sets, key);
                let name = set.map_or_else(|| key.clone(), |g| g.label.clone());
                let tracks =
                    set.map_or(Vec::new(), |g| g.tracks.iter().map(|&i| l.library.tracks[i].clone()).collect());
                (ExportOf::Set(name.clone(), tracks), name)
            }
            _ => {
                self.toast("export writes a playlist, a set, a session or all tracks: open one first", Level::Info);
                return;
            }
        };
        let file = name.replace('/', " - ");
        self.modal = Some(Modal::Prompt(Prompt {
            label: "Export to (.m3u8, .csv or .txt)".into(),
            text: format!("~/Music/{file}.m3u8"),
            purpose: PromptFor::ExportList { list },
            root: l.root().to_path_buf(),
        }));
    }

    /// Write rekordbox XML of the drive (just the marked playlists and their tracks, when any are
    /// marked), to take it back into rekordbox: asks where first.
    pub(super) fn export_xml(&mut self) {
        let Some(l) = &self.loaded else {
            self.toast("open a drive first", Level::Info);
            return;
        };
        let only = self.list_marks.clone();
        let label = match only.len() {
            0 => "rekordbox XML of the drive to".to_string(),
            n => format!("rekordbox XML of the {n} marked playlist{} to", s(n)),
        };
        let name = l.device.name();
        self.modal = Some(Modal::Prompt(Prompt {
            label,
            text: format!("~/Music/{name}.xml"),
            purpose: PromptFor::ExportXml { only },
            root: l.root().to_path_buf(),
        }));
    }

    /// Write the file asked for, in the background; one that's there already is replaced only
    /// once that's asked about, as `--replace` does on the command line.
    pub(super) fn write_export(&mut self, root: PathBuf, to: &str, what: Export) {
        let output = super::mix::typed_path(to);
        if output.as_os_str().is_empty() {
            self.toast("say where: a file name", Level::Info);
            return;
        }
        if output.starts_with(&root) {
            self.toast("not onto the drive: somewhere on this computer", Level::Info);
            return;
        }
        if output.exists() {
            let q = format!("{} is there already. Replace it?", output.display());
            self.modal = Some(Modal::Ask(q, Box::new(move |app: &mut App| app.export_to(root, output, what, true))));
            return;
        }
        self.export_to(root, output, what, false);
    }

    fn export_to(&mut self, root: PathBuf, output: PathBuf, what: Export, replace: bool) {
        let drive = root.display().to_string();
        let shown = output.display().to_string();
        let store_root = self.store_root.clone();
        self.task("Exporting".into(), move |ctx| {
            if let Some(dir) = output.parent() {
                std::fs::create_dir_all(dir).map_err(|source| Error::Io { path: dir.to_path_buf(), source })?;
            }
            let msg = match what {
                Export::List(list) => {
                    let store = Store::open(&store_root).ok();
                    let n = write_list(store.as_ref(), &root, list, output, replace, ctx)?;
                    format!("wrote {shown}: {n} track{}", s(n))
                }
                Export::Xml(only) => {
                    let req =
                        api::ExportXml { drive, output, location_root: None, skip_analysis: false, only, replace };
                    let r = api::export_xml(&req, ctx)?;
                    format!(
                        "wrote {shown}: {} tracks, {} playlists (rekordbox: File › Import › rekordbox xml)",
                        r.tracks, r.playlists
                    )
                }
            };
            Ok(Box::new(move |app: &mut App| app.toast(msg, Level::Info)))
        });
    }
}

/// Write `list` from the drive at `root` to `output` (a set as a playlist would be); returns how
/// many tracks it holds.
fn write_list(
    store: Option<&Store>,
    root: &Path,
    list: ExportOf,
    output: PathBuf,
    replace: bool,
    ctx: &Ctx<'_>,
) -> Result<usize, Error> {
    let (playlist, history) = match list {
        ExportOf::Everything => (None, None),
        ExportOf::Playlist(p) => (Some(p), None),
        ExportOf::History(h) => (None, Some(h)),
        // Not a list on the drive: its tracks are written as a playlist's are.
        ExportOf::Set(name, tracks) => {
            fsutil::refuse_to_replace(&[&output], replace)?;
            let format = Format::from_path(&output).unwrap_or_default();
            let locator = Locator::new(PathStyle::default(), root, &output);
            let text = export::render(format, &name, &tracks.iter().collect::<Vec<&Track>>(), &locator);
            fsutil::write_atomic(&output, text.as_bytes())?;
            return Ok(tracks.len());
        }
    };
    let req = api::ExportPlaylist {
        drive: root.display().to_string(),
        playlist,
        history,
        archived: false,
        format: None,
        output,
        paths: Default::default(),
        split: false,
        replace,
    };
    Ok(api::export_playlist(store, &req, ctx)?.files.iter().map(|f| f.tracks).sum())
}

/// What a list export writes.
#[derive(Debug, Clone)]
pub enum ExportOf {
    Everything,
    Playlist(String),
    History(String),
    /// A set: its name, and its tracks on the drive in the order played.
    Set(String, Vec<Track>),
}

/// A file to write.
pub enum Export {
    List(ExportOf),
    /// rekordbox XML, of these playlists only (all when empty).
    Xml(Vec<String>),
}

/// A role as the player says it.
pub fn role_name(r: locallibrary::fleet::Role) -> String {
    r.name().to_string()
}

#[cfg(test)]
mod tests;
