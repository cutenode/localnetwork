//! Benchmarks of the library operations whose cost grows with the size of a drive's library:
//! reading it, filtering, sorting and exporting its tracks, comparing two libraries, summarising
//! its play history, planning a merge, a sync, a pack and a check of drives, converting between
//! the two databases, planning and applying an edit for each track, and writing a library to a
//! blank drive.
//!
//! The libraries are made up: see [`synthetic`].
//!
//! `cargo bench -p locallibrary --bench library`

use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use locallibrary::check::{self, CheckOptions};
use locallibrary::convert::{self, ConvertOptions, Target};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions, TrackFields};
use locallibrary::merge::{self, MergeOptions};
use locallibrary::pack::{self, PackOptions};
use locallibrary::profile::{self, ProfileOptions, Sources};
use locallibrary::progress::Ctx;
use locallibrary::query::{Filter, QueryContext, TrackSelector, parse_sort, sort_tracks};
use locallibrary::store::Store;
use locallibrary::sync::{self, SyncOptions};
use locallibrary::{diff, export, paths, pdb};
use rekordbox_pdb::PdbEditor;
use synthetic::{KEYS, build_pdb, drive, library, write, write_covers};

mod synthetic;

/// `pdb` with a history list per 100 of its tracks, each holding 30 of them, as a drive's players
/// write one per session.
fn with_history(pdb: Vec<u8>) -> Vec<u8> {
    let ids: Vec<u32> = library(&pdb).tracks.iter().map(|t| t.id).collect();
    let mut ed = PdbEditor::from_bytes(pdb);
    for h in 0..ids.len() / 100 {
        let list = ed.create_history_playlist(&format!("HISTORY {:03}", h + 1)).unwrap();
        for k in 0..30 {
            ed.add_to_history(list, ids[(h * 100 + k * 13) % ids.len()]).unwrap();
        }
    }
    ed.to_bytes()
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

/// The profile `locallibrary profile` shows for a drive's sessions, without reading analysis
/// files (the drive has none).
fn play_history(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join(paths::EXPORT_PDB), &with_history(build_pdb(0, 20_000)));
    let sources = Sources { drives: vec![Device::open(dir.path()).unwrap()], store: None, libraries: None };
    let options = ProfileOptions { analysis: false, ..Default::default() };
    let ctx = Ctx::silent();
    let mut g = c.benchmark_group("profile");
    g.sample_size(10);
    g.bench_function("build/20k", |b| b.iter(|| profile::build(black_box(&sources), &options, &ctx).unwrap()));
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
    // Nearly all of it new to a small library.
    let small = drive("C", 20_000, 500);
    g.bench_function("merge_plan_new/10k", |bench| {
        bench.iter(|| merge::plan(&a, &small, &MergeOptions::default(), &ctx).unwrap())
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

/// Pulling a drive into a new store: every file of its library hashed, copied and made durable.
fn pull(c: &mut Criterion) {
    let root = drive("P", 0, 2_000).root().to_path_buf();
    let ctx = Ctx::silent();
    let opts = sync::PullOptions { library: None, message: None, force: true };
    let mut g = c.benchmark_group("pull");
    g.sample_size(10);
    g.bench_function("new_store/2k", |b| {
        b.iter_batched(
            || (tempfile::tempdir().unwrap(), Device::open(&root).unwrap()),
            |(dir, mut device)| {
                let store = Store::open_or_init(dir.path().join("store")).unwrap();
                sync::pull(&store, &mut device, &opts, &ctx).unwrap()
            },
            BatchSize::PerIteration,
        )
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

/// Setting every track's key and tempo in a 5,000-track library, one edit for each, as an import
/// or `analyze --apply` does: planned with the tracks named by id, and by file as a replay names
/// them, and applied to both databases.
fn edits(c: &mut Criterion) {
    let ctx = Ctx::silent();
    let dir = tempfile::tempdir().unwrap();
    let both = dir.path().join("D");
    write(&both.join(paths::EXPORT_PDB), &build_pdb(0, 5_000));
    write_covers(&both, 0, 5_000);
    let mut device = Device::open(&both).unwrap();
    let to = ConvertOptions { to: Target::OneLibrary, replace: false };
    convert::apply(&convert::plan(&device, &to, &ctx).unwrap(), &mut device, &to, &ctx).unwrap();
    let lib = device.library().unwrap();
    let fields = |i: usize| TrackFields {
        key: Some(KEYS[(i + 5) % KEYS.len()].into()),
        bpm: Some(120.0 + (i % 10) as f64),
        ..Default::default()
    };
    let each = |select: &dyn Fn(&locallibrary::Track) -> TrackSelector| -> Vec<Edit> {
        lib.tracks.iter().enumerate().map(|(i, t)| Edit::EditTracks { tracks: select(t), fields: fields(i) }).collect()
    };
    let by_id = each(&|t| TrackSelector { ids: vec![t.id], ..Default::default() });
    let by_file = each(&|t| TrackSelector::paths([t.file_path.clone()]));
    let opts = EditOptions { no_journal: true, ..Default::default() };
    let mut g = c.benchmark_group("edit");
    g.sample_size(10);
    g.bench_function("plan_fields/5k", |b| b.iter(|| edit::plan(&device, black_box(&by_id), &opts, &ctx).unwrap()));
    g.bench_function("plan_fields_by_file/5k", |b| {
        b.iter(|| edit::plan(&device, black_box(&by_file), &opts, &ctx).unwrap())
    });
    g.bench_function("apply_fields/5k", |b| {
        b.iter_batched(
            || {
                let (dir, root) = copy_with(&both, paths::EXPORT_PDB);
                write(&root.join(paths::LIBRARY_PLUS_DB), &fs::read(both.join(paths::LIBRARY_PLUS_DB)).unwrap());
                let device = Device::open(&root).unwrap();
                let plan = edit::plan(&device, &by_id, &opts, &ctx).unwrap();
                (dir, device, plan)
            },
            |(_dir, mut device, plan)| edit::apply(&plan, &mut device, &ctx).unwrap(),
            BatchSize::PerIteration,
        )
    });
    g.finish();
}

/// Writing one folder's 20 playlists of a 10,000-track drive to a blank one, as `pack` does: the
/// plan, the copies, the databases built on the target and the scan that records them. A
/// temporary folder has no USB drive's latency, so this keeps the work per file honest and leaves
/// the wait for the drive to the counts in `tests/export_counters.rs` and to real drives.
fn export_library(c: &mut Criterion) {
    let a = drive("A", 0, 10_000);
    let ctx = Ctx::silent();
    let opts = PackOptions { playlists: vec!["Folder 0".into()], ..Default::default() };
    let mut g = c.benchmark_group("export");
    g.sample_size(10);
    g.bench_function("pack_apply/10k", |bench| {
        bench.iter_batched(
            || {
                let dir = tempfile::tempdir().unwrap();
                let target = Device::open(dir.path()).unwrap();
                let plan = pack::plan(&a, &target, &opts, &ctx).unwrap();
                (dir, target, plan)
            },
            |(_dir, mut target, plan)| pack::apply(&plan, &a, &mut target, None, &ctx).unwrap(),
            BatchSize::PerIteration,
        )
    });
    g.finish();
}

criterion_group!(benches, read, query, compare, play_history, drives, pull, write_pdb, convert, edits, export_library);
criterion_main!(benches);
