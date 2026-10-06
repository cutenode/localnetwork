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
use locallibrary::pack::PackOptions;
use locallibrary::{Error, Store};
use uuid::Uuid;

use super::app::{App, Focus, Level, View};
use super::browse::NodeId;
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
        if let Some(root) = self.acting_on() {
            self.drive_act(act, root);
        }
    }

    /// One of a drive's actions, on the drive at `root`.
    pub(super) fn drive_act(&mut self, act: Act, root: PathBuf) {
        match act {
            Act::Pull => self.pull(root),
            Act::Check => self.check(root, false),
            Act::SetPrep => self.check(root, true),
            Act::SyncGroup => self.sync_group(root),
            Act::Pack => self.pack(Some(root)),
            Act::RestorePlaylist => self.restore_playlist(root),
            Act::Replay => self.replay(root),
            Act::Undo => self.undo_on(root),
            Act::Eject => self.eject(root),
            act => self.perform(act, None),
        }
    }

    /// `e` in the Drives view: what can be done to the selected drive. What's chosen is done to
    /// that drive (by its identity: see [`DriveKey`]), even if another is selected by then, or
    /// refused once it's been unplugged.
    pub(super) fn drive_menu(&mut self) {
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
        let (acts, items): (Vec<Act>, Vec<Item>) = items.into_iter().unzip();
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

    /// Eject the drive (playback from it stops first), once the plan is confirmed.
    pub(super) fn eject(&mut self, root: PathBuf) {
        let (drive, store_root) = (root.display().to_string(), self.store_root.clone());
        self.plan(root, move |_| {
            Ok(Box::new(api::drive_eject_plan(Some(&store_root), &EjectDrives { drives: vec![drive] })?))
        });
    }

    /// Check the drive (`group`: and the other mounted drives of its library, as for a set)
    /// against the player models remembered for it, in the background; the results show in a
    /// panel.
    pub(super) fn check(&mut self, root: PathBuf, group: bool) {
        let name = self.drive_label(&root);
        let drives = if group { self.group_of(&root) } else { vec![root] };
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
                        (_, Some(l)) => format!("{}  {l}", s.created.format("%Y-%m-%d %H:%M")),
                        (Some(m), None) => format!("{}  {m}", s.created.format("%Y-%m-%d %H:%M")),
                        (None, None) => s.created.format("%Y-%m-%d %H:%M").to_string(),
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

    /// Write the list shown (a playlist or folder, a history list, or every track) to a file as
    /// M3U8, CSV or text: asks where first.
    pub(super) fn export_list(&mut self) {
        let Some(l) = &self.loaded else {
            self.toast("open a drive first", Level::Info);
            return;
        };
        let (list, name) = match &l.node {
            NodeId::All => (ExportOf::Everything, l.device.name()),
            NodeId::Playlist(id) => (ExportOf::Playlist(l.playlist_path(*id)), l.playlist_path(*id)),
            NodeId::Group(super::browse::Category::History, _) => {
                let name = l.node_title(&l.node).rsplit(" › ").next().unwrap_or_default().to_string();
                (ExportOf::History(name.clone()), name)
            }
            _ => {
                self.toast("export writes a playlist, a history list or all tracks: open one first", Level::Info);
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
                    let (playlist, history) = match list {
                        ExportOf::Everything => (None, None),
                        ExportOf::Playlist(p) => (Some(p), None),
                        ExportOf::History(h) => (None, Some(h)),
                    };
                    let store = Store::open(&store_root).ok();
                    let req = api::ExportPlaylist {
                        drive,
                        playlist,
                        history,
                        archived: false,
                        format: None,
                        output,
                        paths: Default::default(),
                        split: false,
                        replace,
                    };
                    let r = api::export_playlist(store.as_ref(), &req, ctx)?;
                    let n: usize = r.files.iter().map(|f| f.tracks).sum();
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

/// What a list export writes.
#[derive(Debug, Clone)]
pub enum ExportOf {
    Everything,
    Playlist(String),
    History(String),
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
