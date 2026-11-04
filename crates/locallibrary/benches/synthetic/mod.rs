//! A made-up collection, for the benchmarks and the exact counters (`tests/counters.rs`): libraries
//! rekordbox-pdb's editor writes from the empty rekordbox export in the test fixtures, with lookup
//! rows and covers shared the way a real collection shares artists, albums and genres, and
//! playlists in folders.

use std::fs;
use std::path::{Path, PathBuf};

use locallibrary::device::Device;
use locallibrary::{Library, api, artwork, paths, pdb};
use rekordbox_pdb::{NewTrack, PdbEditor};

const GENRES: [&str; 8] = ["House", "Techno", "Deep House", "Tech House", "Disco", "Électro", "Ambient", "Drum & Bass"];
pub const KEYS: [&str; 12] = ["Am", "Em", "Bm", "F#m", "C#m", "G#m", "D#m", "A#m", "Fm", "Cm", "Gm", "Dm"];

/// The tracks `first..first + count` of an imaginary collection: the same index always gives the
/// same track, so two drives built from overlapping ranges share those tracks.
pub fn track(i: usize) -> NewTrack {
    let artist = format!("Artist {}", i / 8);
    let album = format!("Album {}", i / 12);
    let title = format!("Track {i} (Original Mix)");
    NewTrack {
        artist: Some(artist.clone()),
        album: Some(album.clone()),
        genre: Some(GENRES[i % GENRES.len()].into()),
        key: Some(KEYS[i % KEYS.len()].into()),
        label: Some(format!("Label {}", i % 150)),
        comment: if i.is_multiple_of(3) { format!("comment for track {i}") } else { String::new() },
        analyze_path: format!("/PIONEER/USBANLZ/P{:03X}/{:08X}/ANLZ0000.DAT", i % 0x100, i),
        date_added: format!("20{:02}-{:02}-{:02}", 15 + i % 10, 1 + i % 12, 1 + i % 28),
        tempo: 11_000 + (i as u32 * 37) % 3_000,
        duration: 180 + (i % 240) as u16,
        year: 1990 + (i % 35) as u16,
        bitrate: 320,
        sample_rate: 44_100,
        sample_depth: 16,
        file_size: 9_000_000 + i as u32,
        rating: (i % 6) as u8,
        play_count: (i % 7) as u16,
        ..NewTrack::new(title.clone(), format!("/Contents/{artist}/{album}/{title}.mp3"))
    }
}

/// The cover of the album of track `i` (see [`track`]).
pub fn cover(i: usize) -> String {
    artwork::image_key('a', (i / 12) as u32 + 1, "jpg")
}

/// An `export.pdb` holding tracks `first..first + count` with a cover per album, and a playlist
/// per 50 tracks (in folders of 20), each holding 100 of them.
pub fn build_pdb(first: usize, count: usize) -> Vec<u8> {
    let empty = fixtures().join("rekordcrate/empty").join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_bytes(fs::read(empty).unwrap());
    let mut artwork_id = 0;
    let ids: Vec<u32> = (first..first + count)
        .map(|i| {
            if i == first || i.is_multiple_of(12) {
                artwork_id = ed.add_artwork(&paths::device_path(&cover(i))).unwrap();
            }
            ed.add_track(&NewTrack { artwork_id, ..track(i) }).unwrap()
        })
        .collect();
    let mut folder = 0;
    for p in 0..count / 50 {
        if p.is_multiple_of(20) {
            folder = ed.create_playlist(&format!("Folder {}", p / 20), 0, true).unwrap();
        }
        let list = ed.create_playlist(&format!("Playlist {p}"), folder, false).unwrap();
        for k in 0..100 {
            ed.add_to_playlist(list, ids[(p * 50 + k * 7) % count], None).unwrap();
        }
    }
    ed.to_bytes()
}

pub fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// A drive named `name` holding tracks `first..first + count` (see [`build_pdb`]) and, for each,
/// a small audio file and analysis files, adopted as drives in use are (their hash caches spare
/// re-reading every analysis file). Writing tens of thousands of files takes a while, so drives
/// are kept in Cargo's temporary directory and reused by later runs.
#[allow(dead_code)]
pub fn drive(name: &str, first: usize, count: usize) -> Device {
    drive_in(Path::new(env!("CARGO_TARGET_TMPDIR")), name, first, count)
}

/// [`drive`], kept in `dir`.
pub fn drive_in(dir: &Path, name: &str, first: usize, count: usize) -> Device {
    let root = dir.join(format!("bench-drives/v2/{name}-{first}-{count}"));
    let built = root.with_extension("built");
    if !built.exists() {
        let _ = fs::remove_dir_all(&root);
        let pdb = build_pdb(first, count);
        write(&root.join(paths::EXPORT_PDB), &pdb);
        for t in &library(&pdb).tracks {
            write(&paths::to_fs(&root, &paths::key(&t.file_path)), t.title.as_bytes());
            let dat = paths::to_fs(&root, &paths::key(&t.analyze_path));
            write(&dat, b"PMAI");
            write(&dat.with_extension("EXT"), b"PMAI");
        }
        write_covers(&root, first, count);
        api::adopt_drive(None, &api::AdoptDrive { drive: root.display().to_string(), name: Some(name.into()) })
            .unwrap();
        write(&built, b"");
    }
    Device::open(&root).unwrap()
}

/// The cover images of tracks `first..first + count`, below `root`.
pub fn write_covers(root: &Path, first: usize, count: usize) {
    for i in (first..first + count).filter(|i| *i == first || i.is_multiple_of(12)) {
        write(&root.join(cover(i)), b"cover");
        write(&root.join(artwork::medium_variant(&cover(i))), b"larger cover");
    }
}

pub fn library(pdb: &[u8]) -> Library {
    pdb::into_library(pdb::parse_database(pdb.to_vec(), Path::new("export.pdb")).unwrap())
}
