//! Helpers that turn the fixture databases into realistic drives on disk.
#![allow(dead_code)]

#[path = "../support/sqlcipher.rs"]
pub mod sqlcipher;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use locallibrary::device::Device;
use locallibrary::edit::{Edit, EditOptions, EditPlan};
use locallibrary::onelibrary::{ContentRecord, OneLibrary, cipher};
use locallibrary::progress::Ctx;
use locallibrary::{Library, backup, edit, paths, pdb};
use rekordbox_pdb::{NewTrack, PdbEditor};

pub fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn copy_dir(src: &Path, dst: &Path) {
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.unwrap();
        let rel = entry.path().strip_prefix(src).unwrap();
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).unwrap();
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Deterministic stand-in for an audio file.
pub fn placeholder_audio(key: &str) -> Vec<u8> {
    format!("ID3 placeholder audio for {key}\n").repeat(16).into_bytes()
}

/// A minimal ANLZ file (PMAI + PPTH) for `device_path`.
pub fn synthetic_anlz(device_path: &str) -> Vec<u8> {
    let mut path_bytes: Vec<u8> = device_path.encode_utf16().flat_map(u16::to_be_bytes).collect();
    path_bytes.extend_from_slice(&[0, 0]);
    let mut f = Vec::new();
    f.extend_from_slice(b"PMAI");
    f.extend_from_slice(&0x1cu32.to_be_bytes());
    f.extend_from_slice(&((0x1c + 16 + path_bytes.len()) as u32).to_be_bytes());
    f.extend_from_slice(&[0u8; 0x1c - 12]);
    f.extend_from_slice(b"PPTH");
    f.extend_from_slice(&16u32.to_be_bytes());
    f.extend_from_slice(&((16 + path_bytes.len()) as u32).to_be_bytes());
    f.extend_from_slice(&(path_bytes.len() as u32).to_be_bytes());
    f.extend_from_slice(&path_bytes);
    f
}

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// Create audio (and missing analysis files) for every track in the drive's export.pdb.
pub fn populate(root: &Path) {
    populate_library(root, &pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap());
}

/// Create audio (and missing analysis files) for every track of `lib` below `root`.
pub fn populate_library(root: &Path, lib: &Library) {
    for t in &lib.tracks {
        let key = paths::key(&t.file_path);
        let audio = paths::to_fs(root, &key);
        if !audio.exists() {
            write(&audio, &placeholder_audio(&key));
        }
        if !t.analyze_path.is_empty() {
            let dat = paths::to_fs(root, &paths::key(&t.analyze_path));
            if !dat.exists() {
                write(&dat, &synthetic_anlz(&t.file_path));
                write(&dat.with_extension("EXT"), &synthetic_anlz(&t.file_path));
            }
        }
    }
}

/// rekordcrate's two-track "demo tracks" export (real ANLZ files, settings, exportExt.pdb).
pub fn demo_drive(dir: &Path, name: &str) -> PathBuf {
    let root = dir.join(name);
    copy_dir(&fixtures().join("rekordcrate/demo_tracks"), &root);
    populate(&root);
    root
}

/// rekordbox-pdb's twelve-track export with a playlist (database only; analysis synthesised).
pub fn bigger_drive(dir: &Path, name: &str) -> PathBuf {
    let root = dir.join(name);
    let pdb = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/rekordbox-pdb/tests/data/bigger-export.pdb");
    write(&root.join(paths::EXPORT_PDB), &fs::read(pdb).unwrap());
    populate(&root);
    root
}

/// A real 3,886-track export database (no audio unless `populate` is called).
pub fn large_drive(dir: &Path, name: &str, with_files: bool) -> PathBuf {
    let root = dir.join(name);
    write(&root.join(paths::EXPORT_PDB), &fs::read(fixtures().join("rekordcrate/large/export.pdb")).unwrap());
    if with_files {
        populate(&root);
    }
    root
}

/// Simulate rekordbox exporting one more track to the drive.
pub fn export_new_track(root: &Path, artist: &str, title: &str) -> u32 {
    let device_path = format!("/Contents/{artist}/UnknownAlbum/{title}.mp3");
    let key = paths::key(&device_path);
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    let n = ed.database().unwrap().tracks.len();
    let analyze = format!("/PIONEER/USBANLZ/P0FF/{:08X}/ANLZ0000.DAT", 0xA000 + n);
    let id = ed
        .add_track(&NewTrack {
            artist: Some(artist.into()),
            tempo: 12400,
            duration: 240,
            analyze_path: analyze.clone(),
            date_added: "2026-09-30".into(),
            file_size: 1234,
            ..NewTrack::new(title, device_path.clone())
        })
        .unwrap();
    ed.save(&pdb_path).unwrap();
    write(&paths::to_fs(root, &key), &placeholder_audio(&key));
    write(&paths::to_fs(root, &paths::key(&analyze)), &synthetic_anlz(&device_path));
    id
}

/// Sorted (key, bytes) of every library file below `root` (PIONEER + Contents, minus settings
/// and locallibrary's own files) for byte-level drive comparisons.
pub fn library_files(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for top in ["PIONEER", "Contents"] {
        let dir = root.join(top);
        if !dir.exists() {
            continue;
        }
        for e in walkdir::WalkDir::new(&dir) {
            let e = e.unwrap();
            if !e.file_type().is_file() {
                continue;
            }
            let key = paths::key(&e.path().strip_prefix(root).unwrap().to_string_lossy());
            if paths::classify(&key).is_some_and(|c| c.is_library()) {
                out.push((key, fs::read(e.path()).unwrap()));
            }
        }
    }
    out.sort();
    out
}

/// Every file on the drive outside locallibrary's own directory.
pub type Files = BTreeMap<String, Vec<u8>>;

pub fn drive_files(root: &Path) -> Files {
    let mut out = BTreeMap::new();
    for e in walkdir::WalkDir::new(root).into_iter().filter_entry(|e| e.file_name() != paths::STATE_DIR) {
        let e = e.unwrap();
        if e.file_type().is_file() {
            let key = paths::key(&e.path().strip_prefix(root).unwrap().to_string_lossy());
            out.insert(key, fs::read(e.path()).unwrap());
        }
    }
    out
}

pub fn changed_files(a: &Files, b: &Files) -> Vec<String> {
    let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    keys.into_iter().filter(|k| a.get(*k) != b.get(*k)).cloned().collect()
}

/// The drive as it was before an attempt, to tell whether a failed one left it alone.
pub struct Before {
    pub files: Files,
    journal: usize,
    backups: Vec<String>,
}

impl Before {
    pub fn take(root: &Path) -> Before {
        let d = Device::open(root).unwrap();
        Before {
            files: drive_files(root),
            journal: edit::journal::list(&d).unwrap().len(),
            backups: backup::list(&d).unwrap().into_iter().map(|b| b.id).collect(),
        }
    }

    pub fn untouched(&self, root: &Path) -> Result<(), String> {
        let changed = changed_files(&self.files, &drive_files(root));
        if !changed.is_empty() {
            return Err(format!("files changed: {changed:?}"));
        }
        let d = Device::open(root).unwrap();
        if let Some(p) = d.state().and_then(|s| s.pending.clone()) {
            return Err(format!("the drive is left marked as interrupted ({:?})", p.description));
        }
        if edit::journal::list(&d).unwrap().len() != self.journal {
            return Err("the journal changed".into());
        }
        let backups: Vec<String> = backup::list(&d).unwrap().into_iter().map(|b| b.id).collect();
        if backups != self.backups {
            return Err(format!("backups changed: {:?} -> {backups:?}", self.backups));
        }
        Ok(())
    }
}

/// Plan and apply `edits` on the drive at `root`: the plan.
pub fn apply(root: &Path, edits: &[Edit]) -> EditPlan {
    let mut d = Device::open_export(root).unwrap();
    let plan = edit::plan(&d, edits, &EditOptions::default(), &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    plan
}

/// Replay the drive's journal as `edits replay` does: the edits replayed and the library after.
pub fn replay(root: &Path) -> (Vec<Edit>, Library) {
    let mut d = Device::open_export(root).unwrap();
    let edits = edit::journal::edits(&d).unwrap();
    let plan = edit::plan(&d, &edits, &EditOptions { lenient: true, no_journal: true }, &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    (edits, d.library().unwrap())
}

/// OneLibrary's image for an export.pdb artwork path: `.../a12.jpg` -> `.../b12.jpg`.
pub fn b_image(artwork_path: &str) -> String {
    let (dir, file) = artwork_path.rsplit_once('/').unwrap();
    format!("{dir}/b{}", file.strip_prefix('a').unwrap_or(file))
}

/// A OneLibrary database holding `lib` (tracks, artwork as `b` images, playlists), built on the
/// empty database the sqlcipher CLI made (see fixtures/onelibrary/make.sh).
pub fn onelibrary_for(lib: &Library) -> OneLibrary {
    let mut one = OneLibrary::from_encrypted(&fs::read(fixtures().join("onelibrary/empty.db")).unwrap(), None).unwrap();
    let defaults = one.content_defaults().unwrap();
    let mut ids = HashMap::new();
    for t in &lib.tracks {
        let image = t.artwork_path.as_ref().map(|p| one.add_image(&b_image(p)).unwrap());
        let rec = ContentRecord::from_track(t, !t.analyze_path.is_empty(), &defaults);
        ids.insert(t.id, one.add_track(&rec, &t.file_path, &t.analyze_path, image).unwrap());
    }
    let mut playlists = HashMap::new();
    let mut queue = vec![None];
    while let Some(parent) = queue.pop() {
        for p in lib.children(parent) {
            let id = one.create_playlist(&p.name, parent.map_or(0, |pid| playlists[&pid]), p.is_folder).unwrap();
            playlists.insert(p.id, id);
            for t in &p.track_ids {
                one.add_to_playlist(id, ids[t]).unwrap();
            }
            queue.push(Some(p.id));
        }
    }
    for h in lib.history.iter().filter(|h| !h.is_folder) {
        let id = one.create_history(&h.name, 0, false).unwrap();
        for t in &h.track_ids {
            one.add_to_history(id, ids[t]).unwrap();
        }
    }
    one
}

/// Add a history playlist to a drive's export.pdb, as a player records one.
pub fn add_history(root: &Path, name: &str, tracks: &[u32]) {
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    let id = ed.create_history_playlist(name).unwrap();
    for t in tracks {
        ed.add_to_history(id, *t).unwrap();
    }
    ed.save(&pdb_path).unwrap();
}

/// Give a drive a OneLibrary database mirroring its export.pdb, as rekordbox 7 exports both.
/// Artwork files are copied to OneLibrary's `b` names.
pub fn add_onelibrary(root: &Path) {
    let lib = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    for t in &lib.tracks {
        if let Some(a) = &t.artwork_path {
            for (from, to) in [(a.clone(), b_image(a)), (medium(a), medium(&b_image(a)))] {
                let from = paths::to_fs(root, &paths::key(&from));
                if from.is_file() {
                    write(&paths::to_fs(root, &paths::key(&to)), &fs::read(from).unwrap());
                }
            }
        }
    }
    write(&root.join(paths::LIBRARY_PLUS_DB), &onelibrary_for(&lib).to_encrypted().unwrap());
}

fn medium(path: &str) -> String {
    locallibrary::artwork::medium_variant(path)
}

/// Turn a drive into one exported for OneLibrary players only (no export.pdb).
pub fn onelibrary_only(root: &Path) {
    add_onelibrary(root);
    for key in [paths::EXPORT_PDB, paths::EXPORT_EXT_PDB] {
        let _ = fs::remove_file(root.join(key));
    }
}

/// The library in a drive's OneLibrary database.
pub fn plus_library(root: &Path) -> Library {
    OneLibrary::open_root(root).unwrap().library().unwrap()
}

/// Has the `sqlcipher` CLI read the drive's OneLibrary database and its WAL: SQLCipher accepts
/// every page, SQLite finds the database intact, and it holds the tracks locallibrary reads from
/// it. Skipped without the CLI (see support/sqlcipher.rs).
pub fn sqlcipher_reads(root: &Path) {
    // A copy: SQLite folds the WAL into the database when it closes it.
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("exportLibrary.db");
    fs::copy(root.join(paths::LIBRARY_PLUS_DB), &db).unwrap();
    if root.join(paths::LIBRARY_PLUS_WAL).is_file() {
        fs::copy(root.join(paths::LIBRARY_PLUS_WAL), dir.path().join("exportLibrary.db-wal")).unwrap();
    }
    let checks = ["PRAGMA cipher_integrity_check;", "PRAGMA integrity_check;", "SELECT count(*) FROM content;"];
    if let Some(out) = sqlcipher::run(&db, cipher::PASSPHRASE, &checks) {
        let tracks = plus_library(root).tracks.len().to_string();
        assert_eq!(out, ["ok", "ok", tracks.as_str()], "sqlcipher's checks of {}", root.display());
    }
}

/// A OneLibrary-only drive from the sqlcipher-made WAL fixture: one track in exportLibrary.db,
/// a second one (and its playlist entry) only in exportLibrary.db-wal.
pub fn wal_drive(dir: &Path, name: &str) -> PathBuf {
    let root = dir.join(name);
    let f = fixtures().join("onelibrary");
    write(&root.join(paths::LIBRARY_PLUS_DB), &fs::read(f.join("wal.db")).unwrap());
    write(&root.join(paths::LIBRARY_PLUS_WAL), &fs::read(f.join("wal.db-wal")).unwrap());
    populate_library(&root, &plus_library(&root));
    root
}

/// Bytes a track's ISRC (string 0) takes in its row, written as rekordbox writes one: `0x90`, the
/// length, a pad byte, 3, the text and a NUL; the empty one is the one-byte empty string.
pub fn isrc_len(s: &str) -> usize {
    if s.is_empty() { 1 } else { s.len() + 6 }
}

/// Whether rekordcrate takes this UTF-16 string for an ISRC: it reads any `0x90` string whose
/// first byte is 3 that way, so names starting with ☃ (U+2603) or 七 (U+4E03) come back empty.
pub fn rekordcrate_reads_as_isrc(s: &str) -> bool {
    !s.is_ascii() && s.encode_utf16().next().is_some_and(|u| u & 0xff == 3)
}
