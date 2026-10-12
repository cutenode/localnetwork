//! Drives drawn for the property tests: the twelve-track export with tracks in other formats,
//! playlists, folders and history lists drawn on top, so properties see libraries of many shapes;
//! for the reads, the two-track demo export too, with artwork and audio files gone missing.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::{Library, paths, pdb};
use rekordbox_pdb::{NewTrack, PdbEditor};

/// Playlist names, folded together by case on purpose (`Warm Up` and `warm up`).
pub const NAMES: &[&str] = &["Warm Up", "warm up", "Peak", "Ünï ♪", "Closing", "B2B"];

/// Audio a track can be: extension, sample rate and bit depth as export.pdb records them (0 when
/// rekordbox left it to the file).
const FORMATS: &[(&str, u32, u16)] = &[
    ("mp3", 44_100, 0),
    ("flac", 96_000, 24),
    ("flac", 192_000, 32),
    ("wav", 48_000, 16),
    ("aiff", 44_100, 24),
    ("m4a", 44_100, 16),
    ("ogg", 44_100, 0),
    ("mp3", 0, 0),
];

/// Keys a track can have, in either notation, or none.
const KEYS: &[&str] = &["8A", "9A", "8B", "3A", "Am", ""];

/// One more track exported by rekordbox, in a drawn format, key and tempo, with its audio and
/// analysis files.
#[hegel::test_helper]
pub fn track(tc: &TestCase, root: &Path, n: usize) -> u32 {
    let (ext, rate, depth) = tc.draw(gs::sampled_from(FORMATS));
    let artist = tc.draw(gs::sampled_from(&["Bicep", "Four Tet", "Ünï"][..]));
    let key = tc.draw(gs::sampled_from(KEYS));
    let path = format!("/Contents/{artist}/Drawn/Track {n}.{ext}");
    let bitrate = if ext == "m4a" { tc.draw(gs::sampled_from(&[0, 256, 900][..])) } else { 320 };
    let track = NewTrack {
        artist: Some(artist.into()),
        key: (!key.is_empty()).then(|| key.into()),
        tempo: tc.draw(gs::integers::<u32>().min_value(60).max_value(180)) * 100,
        duration: 200,
        analyze_path: format!("/PIONEER/USBANLZ/P0D{n:X}/0000D{n:03X}/ANLZ0000.DAT"),
        date_added: "2026-09-30".into(),
        sample_rate: rate,
        sample_depth: depth,
        bitrate,
        ..NewTrack::new(format!("Drawn {n}"), path.clone())
    };
    common::add_track(root, track, &common::placeholder_audio(&path))
}

/// The twelve-track drive `name` in `dir`, with up to two more tracks, playlists (some in a
/// folder) and history lists drawn on top. Returns its root and its library.
#[hegel::test_helper]
pub fn drive(tc: &TestCase, dir: &Path, name: &str) -> (PathBuf, Library) {
    let root = common::bigger_drive(dir, name);
    let library = draw_on(tc, &root);
    (root, library)
}

/// [`drive`], or the two-track demo export (real analysis files, with waveforms) drawn on the
/// same way, then given artwork on some tracks and some tracks' audio taken away.
#[hegel::test_helper]
pub fn varied(tc: &TestCase, dir: &Path, name: &str) -> PathBuf {
    let root = match tc.draw(gs::booleans()) {
        true => common::bigger_drive(dir, name),
        false => common::demo_drive(dir, name),
    };
    let lib = draw_on(tc, &root);
    let art = tc.draw(gs::vecs(gs::sampled_from(lib.tracks.iter().map(|t| t.id).collect::<Vec<_>>())).max_size(2));
    if !art.is_empty() {
        common::media::set_art(&root, &art);
    }
    let files: Vec<String> = lib.tracks.iter().map(|t| t.file_path.clone()).collect();
    for file in tc.draw(gs::vecs(gs::sampled_from(files)).max_size(2)) {
        let _ = fs::remove_file(paths::to_fs(&root, &paths::key(&file)));
    }
    root
}

/// Up to two more tracks, playlists and history lists drawn on the drive at `root`: its library.
#[hegel::test_helper]
fn draw_on(tc: &TestCase, root: &Path) -> Library {
    for n in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
        track(tc, root, n);
    }
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    let ids: Vec<u32> = ed.database().unwrap().tracks.iter().map(|t| t.id).collect();
    let mut folder = None;
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
        let parent = match tc.draw(gs::booleans()) {
            true => *folder.get_or_insert_with(|| ed.create_playlist("Crates", 0, true).unwrap()),
            false => 0,
        };
        let id = ed.create_playlist(tc.draw(gs::sampled_from(NAMES)), parent, false).unwrap();
        for t in tc.draw(gs::vecs(gs::sampled_from(ids.clone())).max_size(5)) {
            ed.add_to_playlist(id, t, None).unwrap();
        }
    }
    for n in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
        let id = ed.create_history_playlist(&format!("HISTORY {:03}", n + 1)).unwrap();
        for t in tc.draw(gs::vecs(gs::sampled_from(ids.clone())).min_size(1).max_size(4)) {
            ed.add_to_history(id, t).unwrap();
        }
    }
    ed.save(&pdb_path).unwrap();
    pdb::read_library(&pdb_path).unwrap()
}

/// Damage drawn into the drive at `root`: up to four of `files` rotted (a byte flipped, size and
/// modification time kept, as failing flash damages a file), and up to two of `deletable` that
/// weren't, deleted. The keys rotted, and those deleted.
#[hegel::test_helper]
pub fn damage(tc: &TestCase, root: &Path, files: &[String], deletable: &[String]) -> (Vec<String>, Vec<String>) {
    let rotted: Vec<String> = some(tc, files).into_iter().take(4).collect();
    let gone: Vec<String> = some(tc, deletable).into_iter().filter(|k| !rotted.contains(k)).take(2).collect();
    for k in &rotted {
        common::rot(&paths::to_fs(root, k));
    }
    for k in &gone {
        fs::remove_file(paths::to_fs(root, k)).unwrap();
    }
    (rotted, gone)
}

/// Some of `items`, each kept or not as drawn.
#[hegel::test_helper]
pub fn some<T: Clone>(tc: &TestCase, items: &[T]) -> Vec<T> {
    items.iter().filter(|_| tc.draw(gs::booleans())).cloned().collect()
}
