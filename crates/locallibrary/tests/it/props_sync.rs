//! Model-based tests of the copy paths: drive-to-drive sync (to one drive or several at once),
//! pull and push through the store, pack and merge, and what compare says after each. The drives
//! are built from the fixtures, with the DJ's own files around their libraries, and runs are
//! cancelled at any progress checkpoint, or cut off there as by a pulled plug, then run again.
//!
//! What must hold (docs/how-it-works.md, "Safety"):
//! * nothing outside `PIONEER/`, `Contents/` and `.locallibrary/` is written or deleted, except
//!   audio a library references there;
//! * after a sync the drives compare as identical, and the next sync never runs the other way;
//! * a run cancelled or cut off at any checkpoint leaves the drive's library finding every file
//!   it references (with `--delete-first`, possibly no library at all), and run again, ends where
//!   an uninterrupted run does;
//! * a push writes the snapshot back exactly, and undoing a merge gives every file back.

use crate::common;

use indexmap::IndexMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::common::Files;
use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::compare::{self, Relation};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions};
use locallibrary::merge::{self, MergeOptions};
use locallibrary::pack::{self, PackOptions};
use locallibrary::paths::{self, FileClass};
use locallibrary::progress::{CancelToken, Ctx, Event, Phase, Progress};
use locallibrary::query::TrackSelector;
use locallibrary::store::{Snapshot, Store};
use locallibrary::sync::{self, PullOptions, PushOptions, SyncOptions};
use locallibrary::{Error, Library, backup, plays};
use rekordbox_pdb::{NewTrack, PdbEditor};

// ---------------------------------------------------------------------------------------------
// Drives
// ---------------------------------------------------------------------------------------------

/// Files by key, lower-cased: FAT32 and exFAT ignore letter case.
type Folded = IndexMap<String, Vec<u8>>;

fn fold(files: impl IntoIterator<Item = (String, Vec<u8>)>) -> Folded {
    files.into_iter().map(|(k, b)| (k.to_lowercase(), b)).collect()
}

/// Whether a key is outside the folders locallibrary manages (its own folder isn't listed by
/// [`common::drive_files`]).
fn outside_library(key: &str) -> bool {
    let top = key.split('/').next().unwrap_or("");
    !key.contains('/')
        || !(top.eq_ignore_ascii_case(paths::PIONEER_DIR) || top.eq_ignore_ascii_case(paths::CONTENTS_DIR))
}

/// rekordbox exports one more track to the drive, its audio at `device_path` (anywhere on the
/// drive), with an analysis file.
fn export_track_at(root: &Path, device_path: &str, title: &str) {
    let key = paths::key(device_path);
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    let n = ed.database().unwrap().tracks.len();
    let analyze = format!("/PIONEER/USBANLZ/P0EE/{:08X}/ANLZ0000.DAT", 0xB000 + n);
    let audio = common::placeholder_audio(&key);
    ed.add_track(&NewTrack {
        artist: Some("Exported".into()),
        tempo: 12600,
        duration: 200,
        analyze_path: analyze.clone(),
        date_added: "2026-10-01".into(),
        file_size: audio.len() as u32,
        ..NewTrack::new(title, device_path)
    })
    .unwrap();
    ed.save(&pdb_path).unwrap();
    write(&paths::to_fs(root, &key), &audio);
    write(&paths::to_fs(root, &paths::key(&analyze)), &common::synthetic_anlz(device_path));
}

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// Copy a drive as it stands, modification times included (an image of it at one instant).
fn copy_tree(from: &Path, to: &Path) {
    for e in walkdir::WalkDir::new(from) {
        let Ok(e) = e else { continue }; // a temp file renamed away mid-walk
        let dst = to.join(e.path().strip_prefix(from).unwrap());
        if e.file_type().is_dir() {
            fs::create_dir_all(&dst).unwrap();
        } else if e.file_type().is_file()
            && let Ok(meta) = fs::metadata(e.path())
            && fs::copy(e.path(), &dst).is_ok()
        {
            filetime::set_file_mtime(&dst, filetime::FileTime::from_last_modification_time(&meta)).unwrap();
        }
    }
}

/// Overwrite a file's data with zeros, keeping its length and modification time: what a file
/// renamed into place shows when its data never reached the disk.
fn tear(path: &Path) {
    let meta = fs::metadata(path).unwrap();
    fs::write(path, vec![0u8; meta.len() as usize]).unwrap();
    filetime::set_file_mtime(path, filetime::FileTime::from_last_modification_time(&meta)).unwrap();
}

/// The drive's library still reads, and every file its database references is there.
fn assert_consistent(root: &Path) {
    let d = Device::open(root).unwrap();
    if !d.has_export() {
        return;
    }
    let lib = d.library().unwrap_or_else(|e| panic!("{} doesn't read after an interrupted run: {e}", root.display()));
    for t in &lib.tracks {
        assert!(
            paths::to_fs(root, &paths::key(&t.file_path)).is_file(),
            "{}: {} is missing after an interrupted run",
            root.display(),
            t.file_path
        );
    }
}

fn assert_same_files(what: &str, actual: &Folded, expected: &Folded) {
    if actual != expected {
        let keys: BTreeSet<&String> = actual.keys().chain(expected.keys()).collect();
        let diff: Vec<String> = keys
            .into_iter()
            .filter(|k| actual.get(*k) != expected.get(*k))
            .map(|k| match (actual.contains_key(k), expected.contains_key(k)) {
                (true, false) => format!("{k} (unexpected)"),
                (false, true) => format!("{k} (missing)"),
                _ => format!("{k} (different)"),
            })
            .collect();
        panic!("{what}: files differ: {diff:?}");
    }
}

/// The twelve-track export with two history lists, and OneLibrary's database for it made once
/// (its key derivation is cached per salt, and every copy shares this file's salt).
fn main_drive(dir: &Path, plus: bool) -> PathBuf {
    static DB: OnceLock<Vec<u8>> = OnceLock::new();
    let make = |dir: &Path| {
        let root = common::bigger_drive(dir, "Main");
        common::add_history(&root, "HISTORY 001", &[1, 2]);
        common::add_history(&root, "HISTORY 002", &[3]);
        root
    };
    let root = make(dir);
    if plus {
        let db = DB.get_or_init(|| {
            let tmp = tempfile::tempdir().unwrap();
            let root = make(tmp.path());
            common::add_onelibrary(&root);
            fs::read(root.join(paths::LIBRARY_PLUS_DB)).unwrap()
        });
        write(&root.join(paths::LIBRARY_PLUS_DB), db);
    }
    root
}

// ---------------------------------------------------------------------------------------------
// Interrupting runs
// ---------------------------------------------------------------------------------------------

/// How a run ends.
#[derive(Debug, Clone, Copy)]
enum Stop {
    /// It runs to the end.
    No,
    /// Cancelled (Ctrl-C) at the `n`th progress event.
    Cancel(usize),
    /// The plug is pulled at the `n`th progress event: the drive as it stood then, with every
    /// file the run wrote since its last flush torn, is what the next run finds.
    Unplug(usize),
}

/// Runs on these small drives report well under a hundred progress events.
fn draw_stop(tc: &TestCase, unplug: bool) -> Stop {
    let checkpoint = || tc.draw(gs::integers::<usize>().max_value(80));
    match tc.draw(gs::integers::<u8>().max_value(if unplug { 4 } else { 1 })) {
        0 => Stop::No,
        1 => Stop::Cancel(checkpoint()),
        _ => Stop::Unplug(checkpoint()),
    }
}

/// Watches a run's progress, and at the chosen event cancels it or takes an image of the drive.
struct Watch {
    stop: Stop,
    drive: PathBuf,
    image: PathBuf,
    /// The run writes its databases right after flushing the files it copied (a sync or push;
    /// a pack writes them without progress events, after its copy phase ends).
    databases_follow_flush: bool,
    seen: AtomicUsize,
    flushed: AtomicBool,
    /// Taken, and whether everything copied by then had been flushed.
    imaged: Mutex<Option<bool>>,
    cancel: CancelToken,
}

impl Progress for Watch {
    fn on_event(&self, e: &Event) {
        match e {
            Event::Item { phase: Phase::Copy, path, .. }
                if self.databases_follow_flush && paths::DATABASE_FILES.contains(&path.as_str()) =>
            {
                self.flushed.store(true, Ordering::SeqCst)
            }
            Event::PhaseEnd { phase: Phase::Copy } => self.flushed.store(true, Ordering::SeqCst),
            _ => {}
        }
        let n = self.seen.fetch_add(1, Ordering::SeqCst);
        match self.stop {
            Stop::Cancel(k) if k == n => self.cancel.cancel(),
            Stop::Unplug(k) if k == n => {
                copy_tree(&self.drive, &self.image);
                *self.imaged.lock().unwrap() = Some(self.flushed.load(Ordering::SeqCst));
            }
            _ => {}
        }
    }
}

/// An image of a drive taken mid-run.
struct Image {
    dir: PathBuf,
    flushed: bool,
}

impl Image {
    /// Put the image in the drive's place, as the drive was when the plug was pulled: files the
    /// run wrote that hadn't been flushed (anything that differs from `before`, except the
    /// databases, which are written durably) are torn.
    fn unplug(self, drive: &Path, before: &Files) {
        if !self.flushed {
            for (key, bytes) in common::drive_files(&self.dir) {
                let durable = paths::DATABASE_FILES.contains(&key.as_str());
                if !durable && before.get(&key) != Some(&bytes) && !bytes.is_empty() {
                    tear(&paths::to_fs(&self.dir, &key));
                }
            }
        }
        fs::remove_dir_all(drive).unwrap();
        fs::rename(&self.dir, drive).unwrap();
    }
}

/// Run `f` on `drive` with progress watched for `stop`.
fn watched<T>(
    drive: &Path,
    scratch: &Path,
    stop: Stop,
    sync_like: bool,
    f: impl FnOnce(&Ctx<'_>) -> T,
) -> (T, Option<Image>) {
    let image = scratch.join("image");
    let _ = fs::remove_dir_all(&image);
    let cancel = CancelToken::new();
    let watch = Watch {
        stop,
        drive: drive.to_path_buf(),
        image: image.clone(),
        databases_follow_flush: sync_like,
        seen: AtomicUsize::new(0),
        flushed: AtomicBool::new(false),
        imaged: Mutex::new(None),
        cancel: cancel.clone(),
    };
    let out = f(&Ctx::new(&watch, cancel));
    let imaged = watch.imaged.lock().unwrap().take();
    (out, imaged.map(|flushed| Image { dir: image, flushed }))
}

// ---------------------------------------------------------------------------------------------
// Sync, pull and push
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
struct Config {
    /// Main has tracks outside `Contents/`, and the DJ's own files sit where their audio goes on
    /// the other drives.
    outside: bool,
    /// Syncs and pushes may keep files the source lacks (`--keep-extra`).
    keep_extra: bool,
    /// Runs may be cut off as by a pulled plug, and interrupted runs left for later.
    unplug: bool,
}

/// What a copy is copied from.
#[derive(Debug, Clone, Copy)]
enum Origin {
    Main,
    /// One of [`Copies::snapshots`].
    Snapshot(usize),
}

struct Pulled {
    snapshot: Snapshot,
    /// Main's version when it was pulled, and its library files then.
    version: u32,
    files: Folded,
}

/// Main, where rekordbox exports, and drives that are copies of it.
struct Copies {
    cfg: Config,
    _dir: tempfile::TempDir,
    scratch: PathBuf,
    /// `drives[0]` is Main.
    drives: Vec<PathBuf>,
    store: Store,
    /// The DJ's own files on each drive, outside its library.
    user: Vec<Files>,
    /// Keys of Main's tracks outside `Contents/` (they're never deleted from any drive).
    outside: BTreeSet<String>,
    /// Main's library version, bumped by every export.
    version: u32,
    /// The version of Main each copy holds, after a sync or push that finished.
    holds: Vec<Option<u32>>,
    /// Whether a copy has held (some of) Main's library.
    related: Vec<bool>,
    /// Whether something other than locallibrary changed a copy's library since.
    changed: Vec<bool>,
    snapshots: Vec<Pulled>,
    counter: u32,
}

const USER_FILES: &[&str] = &["notes.txt", "Music/mine.mp3", "Music/Old/set.wav", "IF FOUND.txt", "Contents.txt"];

impl Copies {
    fn new(tc: &TestCase, cfg: Config) -> Copies {
        let dir = tempfile::tempdir().unwrap();
        let root = locallibrary::device::dunce_canonicalize(dir.path());
        let main = main_drive(&root, tc.draw(gs::booleans()));
        let b = root.join("B");
        fs::create_dir_all(&b).unwrap();
        // C holds another library, with a database file Main doesn't have (exportExt.pdb).
        let c = if tc.draw(gs::booleans()) {
            common::demo_drive(&root, "C")
        } else {
            fs::create_dir_all(root.join("C")).unwrap();
            root.join("C")
        };
        let scratch = root.join("scratch");
        fs::create_dir_all(&scratch).unwrap();
        let drives = vec![main, b, c];
        // Files already outside the library (the fixture's analysis files are in `.PIONEER/`)
        // count as the DJ's.
        let user = drives
            .iter()
            .map(|d| common::drive_files(d).into_iter().filter(|(k, _)| outside_library(k)).collect())
            .collect();
        Copies {
            cfg,
            store: Store::open_or_init(root.join("store")).unwrap(),
            _dir: dir,
            scratch,
            drives,
            user,
            outside: BTreeSet::new(),
            version: 0,
            holds: vec![Some(0), None, None],
            related: vec![true, false, false],
            changed: vec![false; 3],
            snapshots: Vec::new(),
            counter: 0,
        }
    }

    fn name(&self, d: usize) -> &str {
        ["Main", "B", "C"][d]
    }

    fn device(&self, d: usize) -> Device {
        Device::open(&self.drives[d]).unwrap()
    }

    /// A drive's library files: `PIONEER/` and `Contents/`, and Main's audio outside them.
    fn managed(&self, d: usize) -> Folded {
        let root = &self.drives[d];
        let mut files = fold(common::library_files(root));
        for key in &self.outside {
            if let Ok(bytes) = fs::read(paths::to_fs(root, key)) {
                files.insert(key.to_lowercase(), bytes);
            }
        }
        files
    }

    /// Whether a drive's library references `key`.
    fn references(&self, d: usize, key: &str) -> bool {
        let dev = self.device(d);
        dev.has_export()
            && dev.library().unwrap().tracks.iter().any(|t| paths::key(&t.file_path).eq_ignore_ascii_case(key))
    }

    fn pick_copy(&self, tc: &TestCase) -> usize {
        tc.draw(gs::integers::<usize>().min_value(1).max_value(self.drives.len() - 1))
    }

    /// What a finished copy of `files` onto drive `t` leaves there, given its files `before`:
    /// the source's files, audio outside `Contents/` it had (never deleted), and with
    /// `keep_extra` its other files the source lacks, except database files (a drive's database
    /// is always the source's).
    fn expected(&self, files: &Folded, before: &Files, keep_extra: bool) -> Folded {
        let mut out = files.clone();
        for (key, bytes) in before {
            let folded = key.to_lowercase();
            if out.contains_key(&folded) {
                continue;
            }
            let keep = match paths::classify(key) {
                None => self.outside.contains(key),
                Some(class) => {
                    class.is_library()
                        && keep_extra
                        && !paths::DATABASE_FILES.iter().any(|k| k.eq_ignore_ascii_case(key))
                }
            };
            if keep {
                out.insert(folded, bytes.clone());
            }
        }
        out
    }

    /// Whether copying `files` onto drive `t` would land on one of the DJ's own files there.
    fn conflicts(&self, t: usize, files: &Folded) -> bool {
        self.user[t].keys().any(|k| files.contains_key(&k.to_lowercase()))
    }

    fn attempt(&self, t: usize, from: Origin, opts: &SyncOptions, ctx: &Ctx<'_>) -> locallibrary::Result<()> {
        let mut target = self.device(t);
        match from {
            Origin::Main => {
                let mut main = self.device(0);
                sync::sync_drive_to_drive(&mut main, &mut target, opts, ctx).map(|_| ())
            }
            Origin::Snapshot(i) => {
                let opts = PushOptions { force: true, sync: opts.clone(), ..Default::default() };
                sync::push(&self.store, &self.snapshots[i].snapshot, &mut target, &opts, ctx).map(|_| ())
            }
        }
    }

    /// Copy Main or a snapshot onto drive `t`, stopped as drawn, and check the outcome.
    fn copy(&mut self, tc: &TestCase, t: usize, from: Origin) {
        let keep_extra = self.cfg.keep_extra && tc.draw(gs::booleans());
        let opts =
            SyncOptions { delete: !keep_extra, verify: tc.draw(gs::booleans()), delete_first: tc.draw(gs::booleans()) };
        let stop = draw_stop(tc, self.cfg.unplug);
        tc.note(&format!("{from:?} -> {}: {opts:?}, {stop:?}", self.name(t)));
        let (files, version) = match from {
            Origin::Main => (self.managed(0), self.version),
            Origin::Snapshot(i) => (self.snapshots[i].files.clone(), self.snapshots[i].version),
        };
        let before = common::drive_files(&self.drives[t]);
        let library_before = self.managed(t);
        let conflict = self.conflicts(t, &files);
        let (result, image) =
            watched(&self.drives[t], &self.scratch, stop, true, |ctx| self.attempt(t, from, &opts, ctx));
        self.user_files_intact();
        if conflict {
            // Refused before writing anything.
            assert!(
                matches!(result.as_ref().map_err(Error::inner), Err(Error::Conflict(_) | Error::Cancelled)),
                "{result:?}"
            );
            assert_same_files("a refused copy", &self.managed(t), &library_before);
            tc.event("refused: the DJ's own file is in the way");
            return;
        }
        match (result, image) {
            (Ok(()), None) => {}
            (Ok(()), Some(image)) => {
                // The run finished; now see what the next run makes of the drive had the plug been
                // pulled at that checkpoint.
                let finished = self.managed(t);
                image.unplug(&self.drives[t], &before);
                tc.event("unplugged");
                // Also with `--delete-first`: the old library comes off the drive before its files.
                assert_consistent(&self.drives[t]);
                self.user_files_intact();
                if !self.interrupted(tc, t) {
                    return;
                }
                self.attempt(t, from, &opts, &Ctx::silent()).unwrap();
                assert_same_files("finishing after the plug was pulled", &self.managed(t), &finished);
            }
            (Err(Error::Cancelled), _) => {
                tc.event("cancelled");
                assert_consistent(&self.drives[t]);
                if !self.interrupted(tc, t) {
                    return;
                }
                self.attempt(t, from, &opts, &Ctx::silent()).unwrap();
            }
            (Err(e), _) => panic!("{from:?} onto {} failed: {e}", self.name(t)),
        }
        assert_same_files(
            &format!("{} after {from:?}", self.name(t)),
            &self.managed(t),
            &self.expected(&files, &before, keep_extra),
        );
        let d = self.device(t);
        assert!(d.state().is_some_and(|s| s.pending.is_none()), "{} is left marked as interrupted", self.name(t));
        if let Origin::Main = from {
            let (plan, _) = sync::plan_drive_to_drive(&self.device(0), &d, &opts, &Ctx::silent()).unwrap();
            assert!(plan.is_noop(), "a second sync copies {:?} and deletes {:?}", plan.copies, plan.deletes);
        }
        self.holds[t] = Some(version);
        self.related[t] = true;
        self.changed[t] = false;
    }

    /// After an interrupted run on drive `t`: whether to run it again now (always, unless runs
    /// may be left interrupted). A run that got as far as marking the drive leaves it related to
    /// Main, holding no finished version.
    fn interrupted(&mut self, tc: &TestCase, t: usize) -> bool {
        if self.device(t).state().is_some_and(|s| s.pending.is_some()) {
            self.related[t] = true;
            self.holds[t] = None;
            self.changed[t] = false;
        }
        !self.cfg.unplug || tc.draw(gs::booleans())
    }

    /// The DJ's files are where they were, unchanged, and nothing else appeared outside the
    /// library but audio Main's library references.
    fn user_files_intact(&self) {
        for (d, root) in self.drives.iter().enumerate() {
            for (key, bytes) in common::drive_files(root) {
                // A temp file a cut-off run left behind is clutter, not a change.
                if !outside_library(&key) || key.split('/').any(paths::is_ignored_name) {
                    continue;
                }
                match self.user[d].get(&key) {
                    Some(mine) => assert_eq!(&bytes, mine, "{}: the DJ's {key} was overwritten", self.name(d)),
                    None => {
                        assert!(self.outside.contains(&key), "{}: {key} appeared outside the library", self.name(d))
                    }
                }
            }
            for key in self.user[d].keys() {
                assert!(paths::to_fs(root, key).is_file(), "{}: the DJ's {key} was deleted", self.name(d));
            }
        }
    }
}

#[hegel::state_machine]
impl Copies {
    /// rekordbox exports another track to Main, under `Contents/` or (rekordbox leaves tracks
    /// where they are) elsewhere on the drive.
    #[rule(weight = 3)]
    fn export(&mut self, tc: TestCase) {
        self.counter += 1;
        let n = self.counter;
        let outside = self.cfg.outside && tc.draw(gs::booleans());
        let device_path = if outside {
            format!("/Music/Set {n}.mp3")
        } else {
            format!("/Contents/Exported/UnknownAlbum/Track {n}.mp3")
        };
        export_track_at(&self.drives[0], &device_path, &format!("Track {n}"));
        self.version += 1;
        tc.note(&format!("export {device_path}"));
        if outside {
            let key = paths::key(&device_path);
            self.outside.insert(key.clone());
            // The DJ may have a file of their own there on a copy already.
            if tc.draw(gs::booleans()) {
                let t = self.pick_copy(&tc);
                let bytes = format!("the DJ's own {key}").into_bytes();
                write(&paths::to_fs(&self.drives[t], &key), &bytes);
                tc.note(&format!("user file {key} on {}", self.name(t)));
                self.user[t].insert(key, bytes);
            }
        }
    }

    /// rekordbox analyses one of Main's tracks again.
    #[rule]
    fn reanalyse(&mut self, tc: TestCase) {
        let files: Vec<String> = common::library_files(&self.drives[0])
            .into_iter()
            .map(|(k, _)| k)
            .filter(|k| paths::classify(k) == Some(FileClass::Analysis))
            .collect();
        tc.assume(!files.is_empty());
        let key = &tc.draw(gs::sampled_from(&files));
        self.counter += 1;
        let mut bytes = fs::read(paths::to_fs(&self.drives[0], key)).unwrap();
        bytes.extend(format!("reanalysed {}", self.counter).bytes());
        fs::write(paths::to_fs(&self.drives[0], key), bytes).unwrap();
        self.version += 1;
        tc.note(&format!("reanalyse {key}"));
    }

    /// The DJ puts a file of their own on a drive, outside its library: possibly where Main keeps
    /// audio outside `Contents/`.
    #[rule]
    fn user_file(&mut self, tc: TestCase) {
        let d = tc.draw(gs::integers::<usize>().max_value(self.drives.len() - 1));
        // Where Main keeps audio outside `Contents/`, as often as not.
        let keys: Vec<String> = match d > 0 && !self.outside.is_empty() && tc.draw(gs::booleans()) {
            true => self.outside.iter().cloned().collect(),
            false => USER_FILES.iter().map(|k| k.to_string()).collect(),
        };
        let key = tc.draw(gs::sampled_from(&keys));
        tc.assume(!self.references(d, &key));
        let mut bytes = b"the DJ's own ".to_vec();
        bytes.extend(tc.draw(gs::vecs(gs::integers::<u8>()).max_size(32)));
        write(&paths::to_fs(&self.drives[d], &key), &bytes);
        tc.note(&format!("user file {key} on {}", self.name(d)));
        self.user[d].insert(key, bytes);
    }

    #[rule]
    fn remove_user_file(&mut self, tc: TestCase) {
        let d = tc.draw(gs::integers::<usize>().max_value(self.drives.len() - 1));
        let keys: Vec<String> = self.user[d].keys().cloned().collect();
        tc.assume(!keys.is_empty());
        let key = &tc.draw(gs::sampled_from(&keys));
        fs::remove_file(paths::to_fs(&self.drives[d], key)).unwrap();
        self.user[d].shift_remove(key);
    }

    /// A file appears in a copy's `Contents/` that Main doesn't have.
    #[rule]
    fn extra_file(&mut self, tc: TestCase) {
        let t = self.pick_copy(&tc);
        self.counter += 1;
        let key = format!("Contents/Mine/extra {}.mp3", self.counter);
        write(&paths::to_fs(&self.drives[t], &key), format!("extra audio {}", self.counter).as_bytes());
        self.holds[t] = None;
        self.changed[t] = true;
    }

    #[rule(weight = 4)]
    fn sync(&mut self, tc: TestCase) {
        let t = self.pick_copy(&tc);
        self.copy(&tc, t, Origin::Main);
    }

    /// Main onto every copy at once (`sync_drive_to_many`).
    #[rule]
    fn sync_all(&mut self, tc: TestCase) {
        let keep_extra = self.cfg.keep_extra && tc.draw(gs::booleans());
        let opts = SyncOptions { delete: !keep_extra, verify: tc.draw(gs::booleans()), delete_first: false };
        let stop = match draw_stop(&tc, false) {
            Stop::Cancel(k) => Stop::Cancel(k),
            _ => Stop::No,
        };
        tc.note(&format!("Main -> every copy: {opts:?}, {stop:?}"));
        let files = self.managed(0);
        let before: Vec<Files> = self.drives.iter().map(|d| common::drive_files(d)).collect();
        let (results, _) = watched(&self.drives[1], &self.scratch, stop, false, |ctx| {
            let mut main = self.device(0);
            let mut targets: Vec<Device> = (1..self.drives.len()).map(|t| self.device(t)).collect();
            sync::sync_drive_to_many(&mut main, &mut targets, &opts, ctx)
        });
        self.user_files_intact();
        let results = match results {
            Ok(results) => results,
            // Cancelled while scanning Main, before any copy was touched.
            Err(Error::Cancelled) => return,
            Err(e) => panic!("syncing every copy failed: {e}"),
        };
        for (i, result) in results.into_iter().enumerate() {
            let t = i + 1;
            match result.as_ref().map_err(Error::inner) {
                // Refused before writing anything (or cancelled before it got that far).
                Err(Error::Conflict(_) | Error::Cancelled) if self.conflicts(t, &files) => continue,
                Err(Error::Cancelled) => {
                    assert_consistent(&self.drives[t]);
                    let mut main = self.device(0);
                    sync::sync_drive_to_drive(&mut main, &mut self.device(t), &opts, &Ctx::silent()).unwrap();
                }
                Err(e) => panic!("syncing {} failed: {e}", self.name(t)),
                Ok(_) => {}
            }
            assert_same_files(
                &format!("{} after syncing every copy", self.name(t)),
                &self.managed(t),
                &self.expected(&files, &before[t], keep_extra),
            );
            self.holds[t] = Some(self.version);
            self.related[t] = true;
            self.changed[t] = false;
        }
    }

    /// Main's library is saved in the store.
    #[rule]
    fn pull(&mut self, _tc: TestCase) {
        let mut main = self.device(0);
        let report = sync::pull(&self.store, &mut main, &PullOptions::default(), &Ctx::silent()).unwrap();
        let snapshot = self.store.load_snapshot(&report.snapshot.unwrap().id).unwrap();
        self.snapshots.push(Pulled { snapshot, version: self.version, files: self.managed(0) });
    }

    /// A snapshot is written back onto a copy.
    #[rule(weight = 2)]
    fn push(&mut self, tc: TestCase) {
        tc.assume(!self.snapshots.is_empty());
        let i = tc.draw(gs::integers::<usize>().max_value(self.snapshots.len() - 1));
        let t = self.pick_copy(&tc);
        self.copy(&tc, t, Origin::Snapshot(i));
    }

    #[invariant]
    fn the_djs_files_are_left_alone(&self, _tc: TestCase) {
        self.user_files_intact();
    }

    /// A copy that only locallibrary wrote to is never newer than Main: identical when it holds
    /// Main's current version, otherwise behind (also mid-way through an interrupted run), so the
    /// next sync goes from Main to it.
    #[invariant]
    fn no_copy_is_newer_than_main(&self, _tc: TestCase) {
        let main = self.device(0);
        for t in 1..self.drives.len() {
            let d = self.device(t);
            if !self.related[t] || self.changed[t] || !d.has_export() {
                continue;
            }
            let c = compare::compare_devices(&main, &d, &Ctx::silent()).unwrap();
            let expected: &[Relation] = match self.holds[t] {
                Some(v) if v == self.version => &[Relation::Identical],
                Some(_) => &[Relation::LeftNewer],
                None => &[Relation::LeftNewer, Relation::Identical],
            };
            assert!(
                expected.contains(&c.relation),
                "Main vs {}: {:?} ({:?}), expected {expected:?}",
                self.name(t),
                c.relation,
                c.evidence.iter().map(|e| &e.detail).collect::<Vec<_>>()
            );
        }
    }
}

fn run_copies(tc: TestCase, cfg: Config) {
    let copies = Copies::new(&tc, cfg);
    hegel::stateful::machine(copies).steps(12).run(tc);
}

/// Syncs and pushes onto drives holding nothing, an older copy or another library, cancelled at
/// any checkpoint and run again: each ends with the source's library files, and compare agrees.
#[hegel::test(test_cases = 30, suppress_health_check = [HealthCheck::TooSlow])]
fn copies_match_the_model(tc: TestCase) {
    run_copies(tc, Config::default());
}

/// `--keep-extra` keeps a drive's own audio but never its old databases, and the next sync still
/// runs from the source.
#[hegel::test(test_cases = 30, suppress_health_check = [HealthCheck::TooSlow])]
fn keep_extra_never_turns_a_copy_into_the_newer_drive(tc: TestCase) {
    run_copies(tc, Config { keep_extra: true, ..Config::default() });
}

/// Audio outside `Contents/` goes where the database says, but never over a file of the DJ's
/// that the target's own library doesn't use.
#[hegel::test(test_cases = 30, suppress_health_check = [HealthCheck::TooSlow])]
fn copies_never_overwrite_the_djs_own_files(tc: TestCase) {
    run_copies(tc, Config { outside: true, ..Config::default() });
}

/// A run cut off as by a pulled plug, possibly left that way while Main moves on: the next run
/// finishes the job, torn files included, and compare sends it the right way meanwhile.
#[hegel::test(test_cases = 30, suppress_health_check = [HealthCheck::TooSlow])]
fn an_unplugged_copy_is_finished_by_the_next_run(tc: TestCase) {
    run_copies(tc, Config { unplug: true, ..Config::default() });
}

/// Cancels a run as it starts copying its first file, before any database goes in.
struct CancelOnFirstCopy(CancelToken);

impl Progress for CancelOnFirstCopy {
    fn on_event(&self, e: &Event) {
        if matches!(e, Event::Item { phase: Phase::Copy, .. }) {
            self.0.cancel();
        }
    }
}

/// Sync a copy of `template` onto a fresh copy of the demo drive (another library), cancelled at
/// each of the run's checkpoints in turn: the drive's library never refers to a file that isn't
/// there, then or after `then` (more runs, stopped as they please), and the next sync ends where
/// an uninterrupted one does.
fn sync_onto_another_library_cancelled_anywhere(template: &Path, opts: &SyncOptions, then: impl Fn(&Path, &Path)) {
    for n in 0.. {
        let dir = tempfile::tempdir().unwrap();
        let root = locallibrary::device::dunce_canonicalize(dir.path());
        // A copy keeps OneLibrary's database, and so its cached key derivation.
        let main = root.join("Main");
        copy_tree(template, &main);
        let c = common::demo_drive(&root, "C");
        let (result, _) = watched(&c, &root, Stop::Cancel(n), true, |ctx| {
            sync::sync_drive_to_drive(&mut Device::open(&main).unwrap(), &mut Device::open(&c).unwrap(), opts, ctx)
        });
        assert_consistent(&c);
        match result {
            Ok(_) => return,
            Err(Error::Cancelled) => {}
            Err(e) => panic!("the sync failed: {e}"),
        }
        then(&main, &c);
        assert_consistent(&c);
        sync::sync_drive_to_drive(
            &mut Device::open(&main).unwrap(),
            &mut Device::open(&c).unwrap(),
            &SyncOptions::default(),
            &Ctx::silent(),
        )
        .unwrap();
        assert_same_files(
            &format!("C finished after a sync cancelled at checkpoint {n}"),
            &fold(common::library_files(&c)),
            &fold(common::library_files(&main)),
        );
        assert!(Device::open(&c).unwrap().state().is_some_and(|s| s.pending.is_none()));
    }
}

/// The example hegel found for `an_unplugged_copy_is_finished_by_the_next_run`: a `--delete-first`
/// sync onto a drive holding another library, cancelled after deleting some of that library's
/// audio and left that way, then a plain sync cancelled before its databases went in. The old
/// library comes off the drive before its audio does, so neither leaves a library missing a file.
#[test]
fn a_delete_first_sync_left_unfinished_never_leaves_a_library_missing_its_audio() {
    let dir = tempfile::tempdir().unwrap();
    let main = main_drive(&locallibrary::device::dunce_canonicalize(dir.path()), false);
    let opts = SyncOptions { delete_first: true, ..SyncOptions::default() };
    sync_onto_another_library_cancelled_anywhere(&main, &opts, |main, c| {
        let cancel = CancelToken::new();
        let progress = CancelOnFirstCopy(cancel.clone());
        let ctx = Ctx::new(&progress, cancel);
        let mut target = Device::open(c).unwrap();
        match sync::sync_drive_to_drive(&mut Device::open(main).unwrap(), &mut target, &SyncOptions::default(), &ctx) {
            Ok(_) | Err(Error::Cancelled) => {}
            Err(e) => panic!("the second sync failed: {e}"),
        }
    });
}

/// A source exported for OneLibrary players only, synced onto a drive whose export.pdb it lacks:
/// that export.pdb (which players read before OneLibrary) goes before the audio it refers to.
#[test]
fn a_database_the_source_lacks_goes_before_the_audio_it_uses() {
    let dir = tempfile::tempdir().unwrap();
    let main = main_drive(&locallibrary::device::dunce_canonicalize(dir.path()), true);
    for key in [paths::EXPORT_PDB, paths::EXPORT_EXT_PDB] {
        let _ = fs::remove_file(main.join(key));
    }
    for opts in [SyncOptions::default(), SyncOptions { delete_first: true, ..SyncOptions::default() }] {
        sync_onto_another_library_cancelled_anywhere(&main, &opts, |_, _| {});
    }
}

// ---------------------------------------------------------------------------------------------
// Pack
// ---------------------------------------------------------------------------------------------

/// Playlists and folders a pack can choose from Main.
const SELECTIONS: &[&str] = &["Sets", "Sets/Friday", "Other", "aab"];

/// Main with "Sets/Friday" (three tracks) and "Other" (two) besides the fixture's playlist. One
/// track of each has its analysis file in `PIONEER/` (the fixture's are in `.PIONEER/`).
fn pack_source(dir: &Path, plus: bool) -> PathBuf {
    let root = main_drive(dir, plus);
    for n in 0..2 {
        export_track_at(&root, &format!("/Contents/Exported/UnknownAlbum/Packed {n}.mp3"), &format!("Packed {n}"));
    }
    let mut d = Device::open_export(&root).unwrap();
    d.adopt(None).unwrap();
    let lib = d.library().unwrap();
    let ids = |r: std::ops::Range<usize>, extra: usize| TrackSelector {
        ids: lib.tracks[r].iter().chain([&lib.tracks[12 + extra]]).map(|t| t.id).collect(),
        ..Default::default()
    };
    let edits = vec![
        Edit::SetTracks { playlist: "Sets/Friday".into(), tracks: ids(0..2, 0), sort: None, limit: None, create: true },
        Edit::SetTracks { playlist: "Other".into(), tracks: ids(3..4, 1), sort: None, limit: None, create: true },
    ];
    let plan = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    root
}

/// The audio files (lower-cased keys) a pack of `selection` from `lib` holds.
fn packed_tracks(lib: &Library, selection: &[String], history: bool) -> BTreeSet<String> {
    let mut nodes: Vec<u32> = lib
        .playlists
        .iter()
        .filter(|p| selection.iter().any(|s| lib.playlist_path(p.id) == *s))
        .map(|p| p.id)
        .collect();
    let mut ids = BTreeSet::new();
    while let Some(id) = nodes.pop() {
        let p = lib.playlist(id).unwrap();
        ids.extend(p.track_ids.iter().copied());
        nodes.extend(lib.children(Some(id)).iter().map(|c| c.id));
    }
    if history {
        ids.extend(lib.history.iter().flat_map(|h| h.track_ids.iter().copied()));
    }
    ids.into_iter().map(|id| paths::file_key(&lib.track(id).unwrap().file_path)).collect()
}

/// A history list as players record it: name and tracks by file.
type Recorded = (String, Vec<String>);

fn recorded(list: &plays::HistoryList) -> Recorded {
    (list.name.clone(), list.tracks.iter().map(|t| paths::file_key(&t.file_path)).collect())
}

#[derive(Debug, Clone, Copy, Default)]
struct PackConfig {
    /// The club drive goes to sets between packs: players record history lists and memory cues.
    sets: bool,
    /// Runs may be cut off as by a pulled plug.
    unplug: bool,
}

/// Main's playlists packed onto a club drive (blank, or holding another library) again and again,
/// with exports and sets in between and runs stopped at any checkpoint.
fn run_packs(tc: TestCase, cfg: PackConfig) {
    let dir = tempfile::tempdir().unwrap();
    let root = locallibrary::device::dunce_canonicalize(dir.path());
    let main = pack_source(&root, tc.draw(gs::booleans()));
    let club = if tc.draw(gs::booleans()) {
        let club = common::demo_drive(&root, "Club");
        if cfg.sets {
            common::add_history(&club, "HISTORY 001", &[1]);
        }
        club
    } else {
        fs::create_dir_all(root.join("Club")).unwrap();
        root.join("Club")
    };
    let scratch = root.join("scratch");
    fs::create_dir_all(&scratch).unwrap();
    let store = Store::open_or_init(root.join("store")).unwrap();
    let mut user = Files::new();
    for key in tc.draw(gs::subsequences(USER_FILES)) {
        let bytes = format!("the DJ's own {key}").into_bytes();
        write(&paths::to_fs(&club, key), &bytes);
        user.insert(key.to_string(), bytes);
    }
    // What players recorded on the club drive: history lists, and analysis files with memory cues.
    let mut sets: Vec<Recorded> = Device::open(&club)
        .ok()
        .filter(Device::has_export)
        .map(|d| plays::drive_history(&d).unwrap().lists.iter().map(recorded).collect())
        .unwrap_or_default();
    let mut cues: Vec<locallibrary::Hash> = Vec::new();
    // Whether the club drive was packed, and with history lists.
    let mut packed: Option<bool> = None;

    for round in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        if round > 0 && tc.draw(gs::booleans()) {
            export_track_at(&main, &format!("/Contents/Exported/UnknownAlbum/Round {round}.mp3"), "New");
        }
        if cfg.sets && packed.is_some() && tc.draw(gs::booleans()) {
            // A set: a history list of some of the club drive's tracks, and memory cues saved.
            let lib = Device::open_export(&club).unwrap().library().unwrap();
            let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(lib.tracks.len().min(3)));
            let ids: Vec<u32> = lib.tracks.iter().take(n).map(|t| t.id).collect();
            let name = format!("HISTORY {:03}", 10 + round);
            common::add_history(&club, &name, &ids);
            let analysed = lib
                .tracks
                .iter()
                .map(|t| paths::to_fs(&club, &paths::key(&t.analyze_path)))
                .find(|p| p.is_file() && p.starts_with(club.join(paths::PIONEER_DIR)));
            if let Some(dat) = analysed {
                let mut bytes = fs::read(&dat).unwrap();
                bytes.extend(format!("memory cue {round}").bytes());
                fs::write(&dat, &bytes).unwrap();
                cues.push(locallibrary::Hash::of(&bytes));
            }
            let d = Device::open_export(&club).unwrap();
            sets = plays::drive_history(&d).unwrap().lists.iter().map(recorded).chain(sets).collect();
            tc.note(&format!("a set: {name}"));
        }
        // The first pack names at least one playlist.
        let selection: Vec<String> = tc
            .draw(gs::subsequences(SELECTIONS).min_size(usize::from(packed.is_none())))
            .into_iter()
            .map(str::to_string)
            .collect();
        let opts = PackOptions {
            playlists: selection.clone(),
            history: tc.draw(gs::booleans()),
            keep_extra: tc.draw(gs::booleans()),
            ..PackOptions::default()
        };
        let stop = draw_stop(&tc, cfg.unplug);
        tc.note(&format!("pack {opts:?}, {stop:?}"));
        let before = common::drive_files(&club);
        let run = |ctx: &Ctx<'_>| -> locallibrary::Result<Vec<String>> {
            let src = Device::open_export(&main)?;
            let mut dst = Device::open(&club)?;
            let plan = pack::plan(&src, &dst, &opts, ctx)?;
            pack::apply(&plan, &src, &mut dst, Some(&store), ctx)?;
            Ok(plan.playlists)
        };
        let (result, image) = watched(&club, &scratch, stop, false, run);
        let chosen = match (result, image) {
            (Ok(chosen), None) => chosen,
            (Ok(chosen), Some(image)) => {
                let finished = fold(common::library_files(&club));
                let fp = Device::open(&club).unwrap().scan_fingerprinted(None, &Ctx::silent()).unwrap().fingerprint;
                image.unplug(&club, &before);
                tc.event("unplugged");
                assert_consistent(&club);
                run(&Ctx::silent()).unwrap();
                // OneLibrary is encrypted afresh each time: compare its content, by fingerprint.
                let plain = |f: Folded| -> Folded {
                    f.into_iter().filter(|(k, _)| !k.starts_with(&paths::LIBRARY_PLUS_DB.to_lowercase())).collect()
                };
                assert_same_files(
                    "packing again after the plug was pulled",
                    &plain(fold(common::library_files(&club))),
                    &plain(finished),
                );
                let again = Device::open(&club).unwrap().scan_fingerprinted(None, &Ctx::silent()).unwrap().fingerprint;
                assert_eq!(again, fp, "the library packed again differs from the one packed in one go");
                chosen
            }
            (Err(Error::Cancelled), _) => {
                tc.event("cancelled");
                assert_consistent(&club);
                run(&Ctx::silent()).unwrap()
            }
            (Err(e), _) => panic!("pack failed: {e}"),
        };
        // An empty selection repeats the last pack's, history lists included if it had them.
        let history = opts.history || (opts.playlists.is_empty() && packed == Some(true));
        packed = Some(history);

        // The DJ's files are untouched.
        let outside: Files = common::drive_files(&club)
            .into_iter()
            .filter(|(k, _)| outside_library(k) && !k.split('/').any(paths::is_ignored_name))
            .collect();
        assert_eq!(outside, user, "files outside the library changed");

        // The club drive holds the chosen playlists' tracks, with Main's files.
        let src = Device::open_export(&main).unwrap();
        let src_lib = src.library().unwrap();
        let d = Device::open_export(&club).unwrap();
        let lib = d.library().unwrap();
        let tracks: BTreeSet<String> = lib.tracks.iter().map(|t| paths::file_key(&t.file_path)).collect();
        assert_eq!(tracks, packed_tracks(&src_lib, &chosen, history), "packed {chosen:?}");
        let main_files = fold(common::library_files(&main));
        let club_files = fold(common::library_files(&club));
        for t in &lib.tracks {
            for key in [paths::key(&t.file_path), paths::key(&t.analyze_path)] {
                let k = key.to_lowercase();
                assert_eq!(club_files.get(&k), main_files.get(&k), "{key} on the club drive isn't Main's");
            }
        }
        assert!(d.state().is_some_and(|s| s.pending.is_none()), "the club drive is left marked as interrupted");
        let again = pack::plan(&src, &d, &PackOptions { playlists: vec![], ..opts.clone() }, &Ctx::silent()).unwrap();
        assert!(
            again.copies.iter().all(|c| paths::classify(&c.path) == Some(FileClass::Database)),
            "packing again copies {:?}",
            again.copies
        );

        if cfg.sets {
            // Nothing players recorded is lost: it's on the drive or in the store.
            let archive = plays::archive(&store, None, &Ctx::silent()).unwrap();
            let on_drive: Vec<Recorded> = plays::drive_history(&d).unwrap().lists.iter().map(recorded).collect();
            for list in &sets {
                assert!(
                    on_drive.contains(list) || archive.lists.iter().any(|a| recorded(&a.list) == *list),
                    "history list {:?} was lost",
                    list.0
                );
            }
            let hashes: BTreeSet<locallibrary::Hash> =
                common::library_files(&club).iter().map(|(_, b)| locallibrary::Hash::of(b)).collect();
            for cue in &cues {
                assert!(hashes.contains(cue) || store.has_object(cue), "memory cues saved on a player were lost");
            }
            // Packing over a library first backs up its databases.
            if let Some(pdb) = before.get(paths::EXPORT_PDB) {
                let saved = backup::list(&d)
                    .unwrap()
                    .iter()
                    .any(|b| fs::read(backup::dir(&d, &b.id).join(paths::EXPORT_PDB)).is_ok_and(|saved| saved == *pdb));
                assert!(saved, "no backup of the database the pack replaced");
            }
        }
    }
}

/// Packing again and again with a remembered or new selection, cancelled at any checkpoint and
/// run again: the club drive holds the chosen playlists with the source's files, and nothing else
/// on it changes.
#[hegel::test(test_cases = 25, suppress_health_check = [HealthCheck::TooSlow])]
fn packs_match_the_model(tc: TestCase) {
    run_packs(tc, PackConfig::default());
}

/// A club drive back from a set keeps what the players recorded: packing it again saves its
/// history lists and cues first, and backs up the databases it replaces.
#[hegel::test(test_cases = 25, suppress_health_check = [HealthCheck::TooSlow])]
fn packing_again_keeps_what_players_recorded(tc: TestCase) {
    run_packs(tc, PackConfig { sets: true, ..PackConfig::default() });
}

/// A pack cut off as by a pulled plug ends, once run again, where an uninterrupted one does.
#[hegel::test(test_cases = 25, suppress_health_check = [HealthCheck::TooSlow])]
fn an_unplugged_pack_is_finished_by_the_next_run(tc: TestCase) {
    run_packs(tc, PackConfig { unplug: true, ..PackConfig::default() });
}

// ---------------------------------------------------------------------------------------------
// Merge
// ---------------------------------------------------------------------------------------------

/// A guest's drive merged into Main, which has the DJ's own files around its library (with
/// `outside`, where the guest's audio outside `Contents/` would go), finished or cancelled at any
/// checkpoint, then undone.
fn run_merge(tc: TestCase, outside: bool) {
    let dir = tempfile::tempdir().unwrap();
    let root = locallibrary::device::dunce_canonicalize(dir.path());
    let main = main_drive(&root, tc.draw(gs::booleans()));
    let guest = common::demo_drive(&root, "Guest");
    let mut candidates: Vec<String> = USER_FILES.iter().map(|k| k.to_string()).collect();
    if outside {
        for i in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(2)) {
            let device_path = format!("/Music/Guest {i}.mp3");
            export_track_at(&guest, &device_path, &format!("Guest {i}"));
            candidates.push(paths::key(&device_path));
        }
    }
    let mut user = Files::new();
    for key in tc.draw(gs::subsequences(&candidates)) {
        let bytes = format!("the DJ's own {key}").into_bytes();
        write(&paths::to_fs(&main, &key), &bytes);
        user.insert(key, bytes);
    }
    let stop = match draw_stop(&tc, false) {
        Stop::Cancel(k) => Stop::Cancel(k),
        _ => Stop::No,
    };
    tc.note(&format!("user files {:?}, {stop:?}", user.keys().collect::<Vec<_>>()));
    let before = common::drive_files(&main);
    let (result, _) = watched(&main, &root, stop, false, |ctx| {
        let src = Device::open_export(&guest)?;
        let mut dst = Device::open_export(&main)?;
        let plan = merge::plan(&src, &dst, &MergeOptions::default(), ctx)?;
        merge::apply(&plan, &src, &mut dst, &MergeOptions::default(), ctx)
    });
    let user_files_intact = || {
        for (key, bytes) in &user {
            assert_eq!(fs::read(paths::to_fs(&main, key)).ok().as_ref(), Some(bytes), "the DJ's {key} was overwritten");
        }
    };
    user_files_intact();
    let mut d = Device::open_export(&main).unwrap();
    match result {
        Ok(report) => {
            tc.event("merged");
            let lib = d.library().unwrap();
            assert_eq!(lib.tracks.len(), 12 + report.tracks_added);
            for t in &lib.tracks {
                let key = paths::key(&t.file_path);
                assert!(!user.contains_key(&key), "{} plays the DJ's own {key}", t.title);
                assert!(paths::to_fs(&main, &key).is_file(), "{} has no audio", t.title);
            }
        }
        Err(Error::Cancelled) => {
            tc.event("cancelled");
            assert_consistent(&main);
        }
        Err(e) => panic!("merge failed: {e}"),
    }
    // Undo: every file the drive had is back as it was.
    if backup::list(&d).unwrap().is_empty() {
        assert_same_files("a merge cancelled before its backup", &fold(common::drive_files(&main)), &fold(before));
        return;
    }
    backup::restore(&mut d, None, false).unwrap();
    user_files_intact();
    let after = common::drive_files(&main);
    for (key, bytes) in &before {
        assert_eq!(after.get(key), Some(bytes), "{key} isn't restored");
    }
    for key in paths::DATABASE_FILES {
        assert_eq!(after.contains_key(key), before.contains_key(key), "{key} after the restore");
    }
    assert!(Device::open(&main).unwrap().state().is_some_and(|s| s.pending.is_none()));
}

/// Undoing a merge, finished or cancelled at any checkpoint, gives every file back.
#[hegel::test(test_cases = 25, suppress_health_check = [HealthCheck::TooSlow])]
fn a_merge_is_undone_exactly(tc: TestCase) {
    run_merge(tc, false);
}

/// A guest's track whose audio would land on a file of the DJ's outside `Contents/` goes beside it.
#[hegel::test(test_cases = 25, suppress_health_check = [HealthCheck::TooSlow])]
fn merging_never_overwrites_the_djs_own_files(tc: TestCase) {
    run_merge(tc, true);
}
