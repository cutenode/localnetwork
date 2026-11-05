//! Work done off the UI thread: its messages, the workers that send them, and what each tick does
//! with them.

use rustc_hash::FxHashMap;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use locallibrary::api;
use locallibrary::error::panic_text;
use locallibrary::fields::TrackValues;
use locallibrary::op::AnyPlanned;
use locallibrary::query::SortField;
use locallibrary::sets::Set;
use locallibrary::watch::{VolumeWatcher, WatchEvent};
use locallibrary::{Device, DeviceInfo, Error, Store};
use loclib_waveform::bar_layout;

use crate::tui::audio::{self};
use crate::tui::browse::{Category, NodeId};
use crate::tui::deck::{OpenedOutput, OutputAsk};
use crate::tui::drives::{DriveKey, DriveRow};

use super::*;

/// The volume watcher, shared with the threads polling it. It starts with the first read of
/// the drive list (see `App::refresh_drives`).
pub(super) type Watcher = Arc<std::sync::Mutex<Option<VolumeWatcher>>>;

pub enum Msg {
    /// What the volume watcher noticed since its last poll (drives coming and going), and
    /// whether the open drive is still there.
    Volumes(Vec<WatchEvent>, Option<(PathBuf, bool)>),
    /// The drives.
    Drives(Vec<DeviceInfo>),
    /// A drive's library read, for the load with this number (see `App::load`).
    Library(u64, PathBuf, Result<Box<Loaded>, String>),
    /// Which tracks' audio files are missing, for the library load with this generation.
    Missing(u64, Vec<bool>),
    /// The drive's My Tags, for the library load with this generation.
    MyTags(u64, Result<FxHashMap<String, Vec<String>>, String>),
    /// What the analyzers worked out for the drive's tracks, for the library load with this
    /// generation.
    Fields(u64, Result<Box<TrackValues>, String>),
    /// Sort ranks and Browse's groups, for the library load with this generation.
    Indexed(u64, Box<Index>),
    /// The store's sets, for the library load with this generation.
    Sets(u64, Result<Vec<Set>, String>),
    /// A track's audio file opened (or not), for the play with this id.
    Opened(u64, Result<audio::OpenedFile, String>),
    /// The output devices and the system's default, for the listing with this id (`d`).
    Outputs(u64, Result<audio::Listed, String>),
    /// An output opened in the background (or not), for the ask with this id, as it was asked
    /// for.
    Output(u64, OutputAsk, Result<OpenedOutput, String>),
    /// Something background work has to say (a report written, say).
    Note(String, Level),
    /// A line of a plugin's log, for the messages (`!`) alone.
    Log(String),
    /// What a plugin tells the user.
    Notice(locallibrary::plugin::PluginNotice),
    /// What plugins suggested, as it's kept now: to review from the palette.
    Suggested(Vec<locallibrary::plugin::PluginNotice>),
    /// The suggestion marked so (reviewed or dismissed) is no longer kept, or couldn't be
    /// forgotten: what's kept from now on says whether it's still offered.
    Forgotten(u64),
    /// A plugin asks whether it may use more of an account, waiting for the answer here.
    AskAccess(Box<locallibrary::plugin::AccessQuestion>, Sender<bool>),
    /// A sign-in waits here for the keys it takes, entered in a form.
    EnterKeys(Box<locallibrary::plugin::AccountSignIn>, Sender<locallibrary::plugin::EnteredKeys>),
    /// The playing track's analysis, artwork and matches, for the play with this id.
    TrackInfo(u64, Box<TrackInfo>),
    Details(Box<Details>),
    /// A plugin's panel answered, for the track (as `Panels` keeps it) and the panel at this
    /// place among its panels.
    Panel((u64, usize), usize, Result<Box<crate::tui::plugins::panels::Ready>, String>),
    Status(Result<api::FleetStatus, String>),
    History(Result<History, String>),
    /// A profile build: which one, and its result.
    Profile(u64, Box<Result<api::ProfileView, String>>),
    /// Signing in to (or out of) the Atmosphere account.
    #[cfg(feature = "account")]
    Account(crate::tui::account::AccountEvent),
    /// A recorded set identified: which identification, and its result.
    Mix(u64, Box<Result<crate::tui::mix::Opened, String>>),
    /// The recordings on a drive, from which listing of it.
    MixSources(u64, PathBuf, Vec<crate::tui::mix::Source>),
    /// The recordings in the folders opened in the Mixes view (which ones, and what's in them).
    MixFolders(Vec<PathBuf>, Vec<crate::tui::mix::Source>),
    Planned {
        job: u64,
        root: PathBuf,
        result: Result<Box<dyn AnyPlanned>, Error>,
    },
    Applied {
        job: u64,
        root: PathBuf,
        result: Result<Box<crate::tui::edits::Applied>, Error>,
    },
    /// Whether replaying the open drive's recorded edits would change it, for the library load
    /// with this generation: how many edits are recorded, 0 when replaying changes nothing.
    Replay(u64, usize),
    /// A background task (a check) finished: what it hands back finishes on the UI thread.
    Done {
        job: u64,
        result: Result<crate::tui::edits::Done, Error>,
    },
    /// What a background task still running has to show (a view's rows as they come).
    Shown {
        job: u64,
        update: crate::tui::edits::Done,
    },
    Progress {
        job: u64,
        detail: String,
        committed: bool,
    },
    /// Work in the background failed (it panicked: see [`worker`]), and what went wrong.
    Failed(Work, String),
}

/// Background work that something waits for, to stop waiting when it fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Work {
    Drives,
    Volumes,
    /// The details of the track at this index, in the library load with this generation.
    Details(u64, usize),
}

thread_local! {
    /// What a panic on this thread said and where, while [`caught`] runs on it (kept by the panic
    /// hook `tui::run` installs).
    static PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Whether [`caught`] is running on this thread, to report a panic here itself.
    static CATCHING: Cell<bool> = const { Cell::new(false) };
}

/// The last panic on a thread [`caught`] isn't running on, and when: one of the library's own
/// workers, whose panic the thread that started them re-raises (saying less) or turns into an
/// error once the worker has ended.
pub(super) static ELSEWHERE: Mutex<Option<(Instant, String)>> = Mutex::new(None);

/// Keep what a panic said and where, for [`caught`] to report (the panic hook can't draw).
pub fn note_panic(info: &std::panic::PanicHookInfo) {
    let at = info.location().map(|l| format!(" ({}:{})", l.file(), l.line())).unwrap_or_default();
    note(format!("{}{at}", panic_text(info.payload())));
}

/// Keep `said` for the [`caught`] running on this thread, or (none is) for the next to fail.
pub(super) fn note(said: String) {
    if CATCHING.get() {
        PANIC.set(Some(said));
    } else {
        *ELSEWHERE.lock().unwrap_or_else(PoisonError::into_inner) = Some((Instant::now(), said));
    }
}

/// Run `f`, turning a panic in it (a bug, or a file the library trips over) into what it said.
/// When one of the library's own threads panicked meanwhile, that's what's said: the panic here
/// only re-raises it.
pub(in crate::tui) fn caught<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    let since = Instant::now();
    let outer = CATCHING.replace(true);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    CATCHING.set(outer);
    result.map_err(|payload| {
        let here = PANIC.take();
        let elsewhere = ELSEWHERE.lock().unwrap_or_else(PoisonError::into_inner).take_if(|(at, _)| *at >= since);
        let said = elsewhere.map(|(_, s)| s).or(here).unwrap_or_else(|| panic_text(&*payload));
        format!("something went wrong (a bug, please report it): {said}")
    })
}

/// Run `job` on a worker thread and send the UI what it comes to; should it panic, `failed` with
/// what went wrong, so whatever waits for it (a busy flag, a write) doesn't wait forever.
pub(in crate::tui) fn worker(
    tx: Sender<Msg>,
    job: impl FnOnce() -> Msg + Send + 'static,
    failed: impl FnOnce(String) -> Msg + Send + 'static,
) {
    std::thread::spawn(move || {
        let _ = tx.send(caught(job).unwrap_or_else(failed));
    });
}

impl App {
    /// Run `job` in the background (see [`worker`]); should it panic, the UI says so.
    pub(in crate::tui) fn spawn(&self, job: impl FnOnce() -> Msg + Send + 'static) {
        self.spawn_or(job, |e| Msg::Note(e, Level::Error));
    }

    /// [`Self::spawn`] for work something waits for: should it panic, `failed` is sent in place
    /// of its message.
    pub(in crate::tui) fn spawn_or(
        &self,
        job: impl FnOnce() -> Msg + Send + 'static,
        failed: impl FnOnce(String) -> Msg + Send + 'static,
    ) {
        worker(self.tx.clone(), job, failed);
    }

    pub(in crate::tui) fn sender(&self) -> Sender<Msg> {
        self.tx.clone()
    }

    pub(in crate::tui) fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Background results, playback progress, animation.
    pub fn tick(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.handle_msg(msg);
        }
        self.show_waiting();
        if !self.volumes_in_flight && self.last_volume_poll.elapsed() > Duration::from_secs(1) {
            self.last_volume_poll = Instant::now();
            self.volumes_in_flight = true;
            let open = self.loaded.as_ref().map(|l| l.root().to_path_buf());
            let watcher = self.watcher.clone();
            let job = move || {
                let present = open.map(|r| {
                    let ok = Device::open(&r).is_ok_and(|d| d.has_export());
                    (r, ok)
                });
                let mut watcher = watcher.lock().unwrap_or_else(PoisonError::into_inner);
                let events = watcher.as_mut().map(VolumeWatcher::poll).unwrap_or_default();
                for e in &events {
                    crate::plugins::events::deliver(&api::Notice::Watch(e.clone()));
                }
                Msg::Volumes(events, present)
            };
            self.spawn_or(job, |e| Msg::Failed(Work::Volumes, e));
        }
        if let Some((msg, Level::Info, at)) = &self.toast
            && at.elapsed() > reading_time(msg)
        {
            self.toast = None;
        }
        if self.search_due.is_some_and(|at| Instant::now() >= at) {
            self.flush_search();
        }
        self.autosave();
        self.watch_output();
        self.follow_queue();
        self.want_details();
        self.want_panels();

        let dt = self.last_tick.elapsed().as_secs_f32().min(0.25);
        self.last_tick = Instant::now();
        let mut counts: Vec<usize> = self.areas.spectra.iter().map(|&(w, m)| bar_layout(w, m).0).collect();
        counts.dedup();
        if let Some(a) = &self.audio {
            let playing = a.is_loaded() && !a.is_paused();
            self.analyzer.update(&a.tap, playing, &counts, dt);
        }

        // Flash the visualiser in a cue's colour as the playhead crosses it (not when seeking past).
        let pos_ms = self.position().as_secs_f64() * 1000.0;
        let prev = self.last_pos_ms;
        let crossed = self.now.as_ref().filter(|_| pos_ms > prev && pos_ms - prev < 500.0).and_then(|n| {
            n.analysis
                .cues
                .iter()
                .filter(|c| (c.start_ms as f64) > prev && (c.start_ms as f64) <= pos_ms)
                .max_by_key(|c| c.hot_cue.is_some())
                .map(|c| crate::tui::theme::palette().cue_color(c))
        });
        if let Some(color) = crossed {
            self.flash = Some((color, Instant::now()));
        }
        crate::tui::deck::reach_active_loop(self, prev, pos_ms);
        self.last_pos_ms = pos_ms;
    }

    pub(in crate::tui) fn handle_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Volumes(events, present) => {
                self.volumes_in_flight = false;
                self.volumes_failing = false;
                // A drive mounted while a read is in flight is read once that one is done.
                if events.iter().any(|e| matches!(e, WatchEvent::Attached(_) | WatchEvent::Detached(_))) {
                    self.refresh_drives();
                }
                self.check_present(present);
            }
            Msg::Drives(list) => {
                self.drives_in_flight = false;
                let first = !std::mem::replace(&mut self.listed, true);
                let selected = self.drive_state.selected().and_then(|i| self.drives.get(i)).map(|d| d.root.clone());
                let changed = list.iter().map(|d| &d.root).ne(self.drives.iter().map(|d| &d.root));
                let added: Vec<PathBuf> = list
                    .iter()
                    .filter(|d| !first && !self.drives.iter().any(|o| o.root == d.root))
                    .map(|d| d.root.clone())
                    .collect();
                self.drives = list;
                self.library_names = (Store::open(&self.store_root).and_then(|s| s.libraries()).unwrap_or_default())
                    .into_iter()
                    .map(|l| (l.id, l.name))
                    .collect();
                if self.drive_header.is_some_and(|h| !self.drive_rows().contains(&h)) {
                    self.drive_header = None;
                }
                // The drive changed last has gone (ejected, unplugged): `u` goes back to the open or
                // selected drive. Mounted somewhere else since, it's followed there.
                let last = self.last_backup.take().and_then(|k| Some(DriveKey { root: self.mounted(&k)?, ..k }));
                self.last_backup = last;
                if !added.is_empty() {
                    let names: Vec<String> = added.iter().map(|r| self.drive_label(r)).collect();
                    self.toast(format!("{} plugged in", names.join(", ")), Level::Info);
                }
                let idx = selected
                    .and_then(|r| self.drives.iter().position(|d| d.root == r))
                    .or(if self.drives.is_empty() { None } else { Some(0) });
                self.drive_state.select(idx);
                if changed && self.status.is_some() {
                    self.refresh_status();
                }
                // Open a library while none is (on startup, or when the first drive is plugged in):
                // the drive open last time when it's here, else the first library (its lead
                // clone), else the first drive with an export.
                if self.loaded.is_none() && self.loading.is_none() {
                    let with_export = |r: &PathBuf| self.drives.iter().any(|d| d.has_export && d.root == *r);
                    let last = self.preferred_root.clone().filter(with_export);
                    let library = self.drive_rows().into_iter().find_map(|r| match r {
                        DriveRow::Library(id) => Some(id),
                        _ => None,
                    });
                    let first = self.drives.iter().find(|d| d.has_export).map(|d| d.root.clone());
                    match (last, library, first) {
                        (Some(root), ..) | (None, None, Some(root)) => {
                            self.preferred_root = None;
                            self.drive_state.select(self.drives.iter().position(|x| x.root == root));
                            self.load_library(root);
                        }
                        (None, Some(id), _) => {
                            self.preferred_root = None;
                            self.open_library(id);
                        }
                        (None, None, None) => {}
                    }
                }
                if std::mem::take(&mut self.drives_again) {
                    self.refresh_drives();
                }
            }
            Msg::Library(load, root, r) => {
                if load != self.load {
                    return; // another drive (or the same one again) was asked for since
                }
                self.loading = None;
                match r {
                    Ok(b) => self.install(root, *b),
                    Err(e) => self.toast(e, Level::Error),
                }
            }
            Msg::Missing(generation, missing) => {
                if let Some(l) = self.loaded.as_mut().filter(|l| l.generation == generation)
                    && missing.len() == l.library.tracks.len()
                {
                    l.set_missing(missing);
                }
            }
            Msg::MyTags(generation, r) => {
                if let Some(l) = self.loaded.as_mut().filter(|l| l.generation == generation) {
                    l.my_tags = match r {
                        Ok(tags) => MyTags::Read(Arc::new(tags)),
                        Err(e) => MyTags::Failed(e),
                    };
                    // Browse › My Tags, grouped again now there are tags to group by.
                    self.regroup(Category::MyTags);
                    self.refresh_visible();
                }
            }
            Msg::Fields(generation, r) => {
                if let Some(l) = self.loaded.as_mut().filter(|l| l.generation == generation) {
                    l.fields = match r {
                        Ok(values) => Fields::Read(Arc::from(values)),
                        Err(e) => Fields::Failed(e),
                    };
                    // Sorted by a field before its values were in: sorted again by them.
                    l.ranks.retain(|(f, _)| !matches!(f, SortField::Field(_)));
                    self.refresh_visible();
                }
            }
            Msg::Sets(generation, r) => {
                let Some(l) = self.loaded.as_mut().filter(|l| l.generation == generation) else { return };
                let sets = match r {
                    Ok(sets) => sets,
                    Err(e) => {
                        l.sets.get_or_insert_default();
                        self.toast(format!("can't read the sets: {e}"), Level::Error);
                        return;
                    }
                };
                // Only a set shown last run is shown before the sets are first read.
                let restored = l.reading_set();
                let showing_a_set = matches!(l.node, NodeId::Group(Category::Sets, _));
                l.sets = Some(sets);
                self.regroup(Category::Sets);
                if showing_a_set {
                    self.refresh_visible();
                }
                // In the tree now: the cursor goes to it, as it does to a playlist shown last run.
                if let Some(l) = self.loaded.as_ref().filter(|_| restored) {
                    self.playlist_state.select(l.tree_position(&l.node).or(self.playlist_state.selected()));
                }
            }
            Msg::Indexed(generation, index) => {
                if let Some(l) = self.loaded.as_mut().filter(|l| l.generation == generation) {
                    let Index { ranks, groups } = *index;
                    for (field, r) in ranks {
                        if !l.ranks.iter().any(|(f, _)| *f == field) {
                            l.ranks.push((field, r));
                        }
                    }
                    for (c, g) in groups {
                        l.groups.entry(c).or_insert(g);
                    }
                }
            }
            Msg::Opened(id, r) => self.opened(id, r),
            Msg::Outputs(id, r) => crate::tui::deck::listed_outputs(self, id, r),
            Msg::Output(id, ask, r) => self.output_opened(id, ask, r),
            Msg::Note(msg, level) => self.toast(msg, level),
            Msg::Log(line) => self.remember(line, Level::Info),
            Msg::Notice(notice) => self.told(notice),
            Msg::Suggested(kept) => self.kept(kept),
            Msg::Forgotten(marked) => self.forgotten(marked),
            Msg::AskAccess(question, reply) => self.ask_access(&question, reply),
            Msg::EnterKeys(start, reply) => self.enter_keys(&start, reply),
            Msg::TrackInfo(id, info) => {
                let upcoming = self.upcoming.as_mut().map(|u| &mut u.now);
                if let Some(now) = self.now.as_mut().into_iter().chain(upcoming).find(|n| n.id == id) {
                    let info = *info;
                    now.phrases = phrase_spans(&info.analysis, now.duration);
                    now.analysis = info.analysis;
                    now.from_plugins = info.from_plugins;
                    now.art = info.art;
                    now.matches = info.matches;
                    now.playlists = info.playlists;
                    now.loading = false;
                }
                if self.now.as_ref().is_some_and(|n| n.id == id) {
                    self.reset_next_pick();
                }
            }
            Msg::Details(d) => {
                if self.details_asked == Some((d.generation, d.index)) {
                    self.details_asked = None;
                    let fresh = self.details.as_ref().is_none_or(|old| old.index != d.index);
                    if fresh {
                        self.details_state.select(if d.matches.items.is_empty() { None } else { Some(0) });
                    }
                    self.details = Some(*d);
                }
            }
            Msg::Panel(track, at, answer) => self.panel_answered(track, at, answer),
            Msg::Status(r) => {
                self.status_loading = false;
                let r = r.map(|s| {
                    self.fleet_libraries = s.libraries;
                    s.mounted
                });
                self.status = Some(r);
            }
            Msg::History(r) => {
                self.history = Some(r);
                // Read again, the list can be shorter, or empty.
                let n = self.history_rows();
                let sel = self.history_state.selected().unwrap_or(0).min(n.saturating_sub(1));
                self.history_state.select((n > 0).then_some(sel));
            }
            Msg::Profile(generation, r) => self.profile.loaded(generation, *r),
            #[cfg(feature = "account")]
            Msg::Account(e) => crate::tui::account::on_event(self, e),
            Msg::Mix(generation, r) => crate::tui::mix::loaded(self, generation, *r),
            Msg::MixSources(listing, root, sources) => {
                if listing == self.mix.drive_listing {
                    self.mix.drive_listed(root, sources);
                }
            }
            Msg::MixFolders(folders, sources) => crate::tui::mix::folders_listed(self, folders, sources),
            Msg::Planned { job, root, result } => self.on_planned(job, root, result),
            Msg::Applied { job, root, result } => self.on_applied(job, root, result),
            Msg::Done { job, result } => self.on_done(job, result),
            Msg::Shown { job, update } => {
                if self.job.as_ref().is_some_and(|j| j.id == job) {
                    update(self);
                }
            }
            Msg::Replay(generation, edits) => {
                let Some(l) = self.loaded.as_ref().filter(|l| l.generation == generation) else { return };
                let root = l.root().to_path_buf();
                self.replay_checked = generation;
                self.replay_offer = (edits > 0).then(|| ReplayOffer { drive: self.drive_label(&root), root, edits });
            }
            Msg::Progress { job, detail, committed } => self.on_progress(job, detail, committed),
            Msg::Failed(work, e) => {
                // A poll that keeps failing is said once, not every second.
                let again = work == Work::Volumes && std::mem::replace(&mut self.volumes_failing, true);
                match work {
                    Work::Drives => self.drives_in_flight = false,
                    Work::Volumes => self.volumes_in_flight = false,
                    Work::Details(generation, index) => {
                        self.details_asked.take_if(|k| *k == (generation, index));
                        self.details_failed = Some((generation, index));
                    }
                }
                if !again {
                    self.toast(e, Level::Error);
                }
                if work == Work::Drives && std::mem::take(&mut self.drives_again) {
                    self.refresh_drives();
                }
            }
        }
    }
}
