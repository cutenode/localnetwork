//! Benchmarks of the library operations whose cost grows with the size of a drive's library:
//! reading it, filtering, sorting and exporting its tracks, comparing two libraries, planning a
//! merge, a sync, a pack and a check of drives, and converting between the two databases.
//!
//! The libraries are synthetic (see [`build_pdb`]): rekordbox-pdb's editor writes them from the
//! empty rekordbox export in the test fixtures, with lookup rows and covers shared the way a real
//! collection shares artists, albums and genres, and playlists in folders.
//!
//! `cargo bench -p locallibrary --bench library`

use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use locallibrary::check::{self, CheckOptions};
use locallibrary::convert::{self, ConvertOptions, Target};
use locallibrary::device::Device;
use locallibrary::merge::{self, MergeOptions};
use locallibrary::pack::{self, PackOptions};
use locallibrary::progress::Ctx;
use locallibrary::query::{Filter, QueryContext, parse_sort, sort_tracks};
use locallibrary::sync::{self, SyncOptions};
use locallibrary::{Library, api, artwork, diff, export, paths, pdb};
use rekordbox_pdb::{NewTrack, PdbEditor};

const GENRES: [&str; 8] = ["House", "Techno", "Deep House", "Tech House", "Disco", "Électro", "Ambient", "Drum & Bass"];
const KEYS: [&str; 12] = ["Am", "Em", "Bm", "F#m", "C#m", "G#m", "D#m", "A#m", "Fm", "Cm", "Gm", "Dm"];

/// The tracks `first..first + count` of an imaginary collection: the same index always gives the
/// same track, so two drives built from overlapping ranges share those tracks.
fn track(i: usize) -> NewTrack {
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
fn cover(i: usize) -> String {
    artwork::image_key('a', (i / 12) as u32 + 1, "jpg")
}

/// An `export.pdb` holding tracks `first..first + count` with a cover per album, and a playlist
/// per 50 tracks (in folders of 20), each holding 100 of them.
fn build_pdb(first: usize, count: usize) -> Vec<u8> {
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

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// A drive named `name` holding tracks `first..first + count` (see [`build_pdb`]) and, for each,
/// a small audio file and analysis files, adopted as drives in use are (their hash caches spare
/// re-reading every analysis file). Writing tens of thousands of files takes a while, so drives
/// are kept in Cargo's temporary directory and reused by later runs.
fn drive(name: &str, first: usize, count: usize) -> Device {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("bench-drives/v2/{name}-{first}-{count}"));
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
fn write_covers(root: &Path, first: usize, count: usize) {
    for i in (first..first + count).filter(|i| *i == first || i.is_multiple_of(12)) {
        write(&root.join(cover(i)), b"cover");
        write(&root.join(artwork::medium_variant(&cover(i))), b"larger cover");
    }
}

fn library(pdb: &[u8]) -> Library {
    pdb::library_from_database(&pdb::parse_database(pdb.to_vec(), Path::new("export.pdb")).unwrap())
}

fn read(c: &mut Criterion) {
    let pdb = build_pdb(0, 20_000);
    let path = Path::new("export.pdb");
    let mut g = c.benchmark_group("read");
    g.bench_function("parse_database/20k", |b| {
        b.iter_batched(
            || pdb.clone(),
            |bytes| pdb::parse_database(black_box(bytes), path).unwrap(),
            BatchSize::LargeInput,
        )
    });
    let db = pdb::parse_database(pdb.clone(), path).unwrap();
    g.bench_function("library_from_database/20k", |b| b.iter(|| pdb::library_from_database(black_box(&db))));
    g.finish();
}

fn query(c: &mut Criterion) {
    let lib = library(&build_pdb(0, 20_000));
    let mut g = c.benchmark_group("query");
    for (name, filter) in [
        ("text", "deep"),
        ("fields", "artist:\"artist 12\" bpm:120-130 -genre:techno"),
        ("playlist", "playlist:\"Playlist 7\" is:played"),
        ("unfiled", "is:unfiled"),
    ] {
        let f = Filter::parse(filter).unwrap();
        g.bench_function(format!("select/{name}/20k"), |b| {
            b.iter(|| f.select(&QueryContext::new(black_box(&lib))).len())
        });
    }
    g.bench_function("export_folder/20k", |b| {
        b.iter(|| export::select(black_box(&lib), Some("Folder 0"), true, "Library").unwrap().len())
    });
    for order in ["artist,title", "key,bpm", "-added,path"] {
        let keys = parse_sort(order).unwrap();
        g.bench_function(format!("sort/{order}/20k"), |b| {
            b.iter_batched(
                || lib.tracks.iter().collect::<Vec<_>>(),
                |mut tracks| sort_tracks(black_box(&mut tracks), &keys),
                BatchSize::LargeInput,
            )
        });
    }
    g.finish();
}

fn compare(c: &mut Criterion) {
    let left = library(&build_pdb(0, 20_000));
    let right = library(&build_pdb(2_000, 20_000));
    let mut g = c.benchmark_group("compare");
    g.bench_function("diff/20k", |b| b.iter(|| diff::between(black_box(&left), &right, ("a", "b"), None)));
    g.finish();
}

fn drives(c: &mut Criterion) {
    let (a, b) = (drive("A", 0, 10_000), drive("B", 1_000, 10_000));
    let ctx = Ctx::silent();
    let mut g = c.benchmark_group("drives");
    g.sample_size(10);
    g.bench_function("read_library/10k", |bench| bench.iter(|| a.library().unwrap()));
    g.bench_function("merge_plan/10k", |bench| {
        bench.iter(|| merge::plan(&a, &b, &MergeOptions::default(), &ctx).unwrap())
    });
    g.bench_function("sync_plan/10k", |bench| {
        bench.iter(|| sync::plan_drive_to_drive(&a, &b, &SyncOptions::default(), &ctx).unwrap())
    });
    // A drive for one folder's 20 playlists.
    let blank = tempfile::tempdir().unwrap();
    let pack = PackOptions { playlists: vec!["Folder 0".into()], ..Default::default() };
    g.bench_function("pack_plan/10k", |bench| {
        bench.iter(|| pack::plan(&a, &Device::open(blank.path()).unwrap(), &pack, &ctx).unwrap())
    });
    let both = [drive("A", 0, 10_000), drive("B", 1_000, 10_000)];
    g.bench_function("check/10k", |bench| {
        bench.iter(|| check::check(&both, None, &CheckOptions::default(), &ctx).unwrap())
    });
    g.finish();
}

fn write_pdb(c: &mut Criterion) {
    let mut g = c.benchmark_group("write");
    g.sample_size(10);
    g.bench_function("build_pdb/10k", |b| b.iter(|| build_pdb(0, black_box(10_000))));
    g.finish();
}

/// A copy of the drive at `from` holding only its covers and the database `key`, in a new
/// temporary directory.
fn copy_with(from: &Path, key: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("D");
    for e in walkdir::WalkDir::new(from.join("PIONEER/Artwork")).into_iter().map(Result::unwrap) {
        if e.file_type().is_file() {
            write(&root.join(e.path().strip_prefix(from).unwrap()), &fs::read(e.path()).unwrap());
        }
    }
    write(&root.join(key), &fs::read(from.join(key)).unwrap());
    (dir, root)
}

/// Converting a 5,000-track library to the other database, as `locallibrary convert` does. The
/// drive holds the covers but not the tracks' files, so there are no cues to bring.
fn convert(c: &mut Criterion) {
    let ctx = Ctx::silent();
    let run = |root: &Path, to| {
        let mut device = Device::open(root).unwrap();
        let opts = ConvertOptions { to, replace: false };
        let plan = convert::plan(&device, &opts, &ctx).unwrap();
        convert::apply(&plan, &mut device, &opts, &ctx).unwrap();
    };
    // A drive with both databases, each one's covers under its own names.
    let dir = tempfile::tempdir().unwrap();
    let both = dir.path().join("D");
    write(&both.join(paths::EXPORT_PDB), &build_pdb(0, 5_000));
    write_covers(&both, 0, 5_000);
    run(&both, Target::OneLibrary);
    let mut g = c.benchmark_group("convert");
    g.sample_size(10);
    g.bench_function("to_onelibrary/5k", |b| {
        b.iter_batched(
            || copy_with(&both, paths::EXPORT_PDB),
            |(_dir, root)| run(&root, Target::OneLibrary),
            BatchSize::PerIteration,
        )
    });
    g.bench_function("to_device_library/5k", |b| {
        b.iter_batched(
            || copy_with(&both, paths::LIBRARY_PLUS_DB),
            |(_dir, root)| run(&root, Target::DeviceLibrary),
            BatchSize::PerIteration,
        )
    });
    g.finish();
}

criterion_group!(benches, read, query, compare, drives, write_pdb, convert);
criterion_main!(benches);
