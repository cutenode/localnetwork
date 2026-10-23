//! A sync plan from drawn file lists, checked against what the sync rules say (src/sync.rs, its
//! module docs): every file the source has is copied or left as it is, the target's managed files
//! the source lacks are deleted (database files even with `--keep-extra`), unsafe and
//! case-colliding paths are left out with a warning, OneLibrary's WAL goes with its database,
//! `--delete-first` copies every database file back, and the plan says when it can't fit. File
//! sizes are drawn up to a petabyte, so no drive has room for some plans.

use std::collections::BTreeSet;
use std::path::Path;

use indexmap::IndexMap;

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::device::Device;
use locallibrary::hash::Hash;
use locallibrary::paths::{self, FileClass};
use locallibrary::space::PLAYER_HEADROOM;
use locallibrary::sync::{self, CopyReason, Source, SyncOptions, SyncPlan};
use locallibrary::tree::{FileEntry, FileTree};

/// Keys a drive can list: two spellings of one audio file, an unsafe one, and one of each other
/// kind of library file, the four database files among them.
const POOL: &[&str] = &[
    "Contents/ARTIST/track.mp3",
    "Contents/Artist/Track.mp3",
    "Contents/Other/Song.flac",
    "Contents/../../escaped.mp3",
    "PIONEER/Artwork/00001/a1.jpg",
    "PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT",
    "PIONEER/djprofile.nxs",
    paths::EXPORT_PDB,
    paths::EXPORT_EXT_PDB,
    paths::LIBRARY_PLUS_DB,
    paths::LIBRARY_PLUS_WAL,
];

/// More than any drive holds.
const HUGE: u64 = 1 << 50;

const MTIME: i64 = 1_700_000_000;

fn fold(key: &str) -> String {
    key.to_lowercase()
}

fn is_database(key: &str) -> bool {
    paths::DATABASE_FILES.iter().any(|k| k.eq_ignore_ascii_case(key))
}

/// A file drawn for `key`: its content one of four (two of a size), perhaps a petabyte, written
/// at one of three times, hashed or not.
#[hegel::test_helper]
fn draw_entry(tc: &TestCase, key: &str, unhashed_ok: bool) -> FileEntry {
    let class = paths::classify(key).unwrap_or(FileClass::Audio);
    let content = tc.draw(gs::integers::<u8>().max_value(3));
    let huge = tc.draw(gs::weighted_booleans(0.2));
    let mtime = MTIME + tc.draw(gs::sampled_from(vec![0i64, 1, 50]));
    // Planning needs every file but audio hashed (else it refuses); audio is compared by time.
    let hashed = match class {
        FileClass::Audio => tc.draw(gs::booleans()),
        _ => !(unhashed_ok && tc.draw(gs::weighted_booleans(0.1))),
    };
    FileEntry {
        path: key.to_string(),
        size: if huge { HUGE } else { 100 } + u64::from(content / 2),
        mtime,
        class,
        hash: hashed.then(|| Hash::of(&[content])),
    }
}

/// A tree listing `entries`, in key order as a scan lists them.
fn tree(root: &Path, mut entries: Vec<FileEntry>) -> FileTree {
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let mut t = FileTree::default();
    t.root = root.to_path_buf();
    for e in entries {
        t.entries.insert(e.path.clone(), e);
    }
    t
}

/// Whether the target's `t` already holds the source's `s`.
fn same(s: &FileEntry, t: &FileEntry) -> bool {
    match (s.hash, t.hash) {
        _ if s.size != t.size => false,
        (Some(a), Some(b)) => a == b,
        _ => (s.mtime - t.mtime).abs() <= 2,
    }
}

/// What the plan must hold, worked out from the rules, or `None` when planning must refuse
/// (a file that must be compared by content isn't hashed on both sides).
struct Expected {
    copies: IndexMap<String, (CopyReason, Option<u64>)>,
    deletes: BTreeSet<String>,
    warnings: Vec<String>,
    planned: usize,
}

fn expected(source: &FileTree, target: &FileTree, opts: &SyncOptions) -> Option<Expected> {
    let by_fold: IndexMap<String, &FileEntry> = target.library_entries().map(|e| (fold(&e.path), e)).collect();
    let mut warnings = Vec::new();
    let mut seen = BTreeSet::new();
    let mut planned = Vec::new();
    for s in source.library_entries() {
        if !paths::is_safe_key(&s.path) {
            warnings.push(format!("skipping unsafe path {:?}", s.path));
        } else if !seen.insert(fold(&s.path)) {
            warnings.push(format!("{} collides with another file differing only in case; skipped", s.path));
        } else {
            planned.push(s);
        }
    }
    let mut copies = IndexMap::new();
    for s in &planned {
        match by_fold.get(&fold(&s.path)) {
            None => {
                copies.insert(s.path.clone(), (CopyReason::New, None));
            }
            Some(t) => {
                let by_content = s.class != FileClass::Audio || opts.verify;
                if s.size == t.size && (s.hash.is_none() || t.hash.is_none()) && by_content {
                    return None;
                }
                if !same(s, t) {
                    copies.insert(s.path.clone(), (CopyReason::Changed, Some(t.size)));
                }
            }
        }
    }
    let deletes: BTreeSet<String> = target
        .library_entries()
        .filter(|t| paths::is_safe_key(&t.path) && (opts.delete || is_database(&t.path)))
        .filter(|t| !seen.contains(&fold(&t.path)))
        .map(|t| t.path.clone())
        .collect();
    let again = |key: &str, copies: &mut IndexMap<String, (CopyReason, Option<u64>)>| {
        if let (Some(s), Some(t)) = (planned.iter().find(|s| fold(&s.path) == fold(key)), by_fold.get(&fold(key))) {
            copies.entry(s.path.clone()).or_insert((CopyReason::Changed, Some(t.size)));
        }
    };
    // `--delete-first` takes the target's databases off first: each one the source has goes back.
    if opts.delete_first && deletes.iter().any(|d| !is_database(d)) {
        for key in paths::DATABASE_FILES {
            again(key, &mut copies);
        }
    }
    // OneLibrary's WAL goes with its database.
    let plus =
        |k: &String| k.eq_ignore_ascii_case(paths::LIBRARY_PLUS_DB) || k.eq_ignore_ascii_case(paths::LIBRARY_PLUS_WAL);
    if copies.keys().chain(deletes.iter()).any(plus) {
        again(paths::LIBRARY_PLUS_WAL, &mut copies);
    }
    Some(Expected { copies, deletes, warnings, planned: planned.len() })
}

/// Check `plan` against the rules.
fn check(plan: &SyncPlan, want: &Expected, source: &FileTree, opts: &SyncOptions) {
    let size_of = |key: &str| source.entries[key].size;
    let copies: IndexMap<String, (CopyReason, Option<u64>)> =
        plan.copies.iter().map(|c| (c.path.clone(), (c.reason, c.target_size))).collect();
    assert_eq!(copies, want.copies, "copies");
    assert_eq!(copies.len(), plan.copies.len(), "each file copied once");
    let deletes: BTreeSet<String> = plan.deletes.iter().map(|d| d.path.clone()).collect();
    assert_eq!(deletes, want.deletes, "deletes");
    for w in &want.warnings {
        assert!(plan.warnings.contains(w), "{w:?} in {:?}", plan.warnings);
    }
    assert_eq!(plan.copies.len() + plan.unchanged, want.planned, "every file is copied or left as it is");
    assert_eq!(plan.copy_bytes, plan.copies.iter().map(|c| size_of(&c.path)).sum::<u64>());
    assert_eq!(plan.delete_bytes, plan.deletes.iter().map(|d| d.size).sum::<u64>());
    // Databases go in last.
    let order: Vec<(bool, &str)> =
        plan.copies.iter().map(|c| (c.class == FileClass::Database, c.path.as_str())).collect();
    assert!(order.is_sorted(), "{order:?}");
    // Counted by class.
    for (counts, classes) in [
        (&plan.copies_by_class, plan.copies.iter().map(|c| c.class).collect::<Vec<_>>()),
        (&plan.deletes_by_class, plan.deletes.iter().map(|d| d.class).collect()),
    ] {
        let n = |c: FileClass| classes.iter().filter(|x| **x == c).count();
        let other = classes.len()
            - n(FileClass::Audio)
            - n(FileClass::Analysis)
            - n(FileClass::Artwork)
            - n(FileClass::Database);
        assert_eq!(
            (counts.audio, counts.analysis, counts.artwork, counts.database, counts.other),
            (n(FileClass::Audio), n(FileClass::Analysis), n(FileClass::Artwork), n(FileClass::Database), other)
        );
    }
    // Whether it fits, and saying so when it doesn't.
    let free = plan.free_space.expect("the temporary folder's free space");
    let needed = plan.bytes_needed();
    let room = if opts.delete_first { free + plan.delete_bytes } else { free };
    assert_eq!(plan.fits, needed <= room, "{needed} needed, {room} free");
    let full = plan.warnings.iter().find(|w| w.starts_with(&format!("needs {needed} bytes but only {free} are free")));
    assert_eq!(full.is_some(), !plan.fits, "{:?}", plan.warnings);
    if let Some(w) = full {
        let hint = !opts.delete_first && needed <= free + plan.delete_bytes;
        assert_eq!(w.ends_with("(it would fit if stale files were deleted first)"), hint, "{w}");
    }
    // Fitting, it says when it leaves players too little room.
    let freed = if opts.delete { plan.delete_bytes } else { 0 };
    let tight = plan.fits && (free + freed).checked_sub(needed).is_some_and(|after| after < PLAYER_HEADROOM);
    let says = plan.warnings.iter().any(|w| w.starts_with("leaves only ") && w.contains(" MB free: players need room"));
    assert_eq!(says, tight, "{:?}", plan.warnings);
    // The same plan is the one confirmed; one copying a file less isn't.
    assert!(plan.same_changes(plan));
    let mut less = plan.clone();
    if less.copies.pop().is_some() || less.deletes.pop().is_some() {
        assert!(!plan.same_changes(&less));
    }
}

/// What a sync plans is what its rules say, for drawn files on both sides and drawn options, and
/// for these every time: both spellings of a file and an unsafe path (left out, with warnings);
/// OneLibrary's database changed beside an unchanged WAL (copied too); a database file that isn't
/// hashed (refused); a petabyte to copy with a petabyte of stale files (fits only when they're
/// deleted first, and the plan says so) or none (doesn't fit at all), stale files deleted first,
/// and copies that leave players too little room.
#[hegel::test(test_cases = 20)]
fn a_plan_copies_and_deletes_what_the_rules_say(tc: TestCase) {
    for variant in 0..8 {
        a_plan_copies_and_deletes_what_the_rules_say_with(&tc, variant);
    }
}

/// One case of [`a_plan_copies_and_deletes_what_the_rules_say`], `variant` forcing what's above.
#[hegel::test_helper]
fn a_plan_copies_and_deletes_what_the_rules_say_with(tc: &TestCase, variant: u8) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("A"), dir.path().join("B"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let unhashed_ok = variant == 0;
    let mut src: Vec<FileEntry> =
        tc.draw(gs::subsequences(POOL)).into_iter().map(|k| draw_entry(tc, k, unhashed_ok)).collect();
    let mut dst: Vec<FileEntry> = Vec::new();
    for k in tc.draw(gs::subsequences(POOL)) {
        // A target lists one spelling of a file.
        if !dst.iter().any(|e| fold(&e.path) == fold(k)) {
            dst.push(draw_entry(tc, k, unhashed_ok));
        }
    }
    let mut opts = SyncOptions {
        delete: tc.draw(gs::booleans()),
        verify: tc.draw(gs::booleans()),
        delete_first: tc.draw(gs::booleans()),
    };
    let put = |list: &mut Vec<FileEntry>, e: FileEntry| {
        list.retain(|x| fold(&x.path) != fold(&e.path));
        list.push(e);
    };
    let file = |key: &str, size: u64, content: u8| FileEntry {
        path: key.to_string(),
        size,
        mtime: MTIME,
        class: paths::classify(key).unwrap_or(FileClass::Audio),
        hash: Some(Hash::of(&[content])),
    };
    match variant {
        1 => {
            // Both spellings, and a path that would leave the drive.
            for k in &POOL[..4] {
                src.retain(|e| e.path != *k);
                src.push(draw_entry(tc, k, false));
            }
        }
        2 => {
            // OneLibrary's database changed, its WAL the same on both sides.
            put(&mut src, file(paths::LIBRARY_PLUS_DB, 100, 1));
            put(&mut dst, file(paths::LIBRARY_PLUS_DB, 100, 2));
            put(&mut src, file(paths::LIBRARY_PLUS_WAL, 100, 3));
            put(&mut dst, file(paths::LIBRARY_PLUS_WAL, 100, 3));
        }
        3 => {
            // A database file only one side hashed.
            put(&mut src, file(paths::EXPORT_PDB, 100, 1));
            put(&mut dst, FileEntry { hash: None, ..file(paths::EXPORT_PDB, 100, 1) });
        }
        4 | 5 => {
            // A petabyte to copy, and a petabyte of stale files or none.
            src = vec![file("Contents/Other/Song.flac", HUGE, 1)];
            dst = match variant {
                4 => vec![file("Contents/Gone/Old.flac", HUGE, 2)],
                _ => Vec::new(),
            };
            opts = SyncOptions { delete: true, delete_first: false, ..opts };
        }
        6 => {
            // Stale files deleted first.
            put(&mut dst, file("Contents/Gone/Old.flac", 100, 2));
            opts = SyncOptions { delete: true, delete_first: true, ..opts };
        }
        7 => {
            // Copies that fit once a petabyte of stale files is deleted first, leaving about
            // 100 MB: too little room for players (unless the free space moves by as much).
            let free = locallibrary::fsutil::available_space(&b).unwrap();
            src = vec![file("Contents/Other/Song.flac", HUGE + free.saturating_sub(100_000_000), 1)];
            dst = vec![file("Contents/Gone/Old.flac", HUGE, 2)];
            opts = SyncOptions { delete: true, delete_first: true, ..opts };
        }
        _ => {}
    }
    let (source_tree, target_tree) = (tree(&a, src), tree(&b, dst));
    let (source_dev, target_dev) = (Device::open(&a).unwrap(), Device::open(&b).unwrap());
    let source = Source::Drive { device: &source_dev, tree: &source_tree };
    let planned = sync::plan(&source, &target_dev, &target_tree, &opts);
    let want = expected(&source_tree, &target_tree, &opts);
    assert!(variant != 3 || want.is_none(), "an unhashed database file is refused");
    match (want, planned) {
        (None, Err(e)) => assert!(e.to_string().ends_with(" must be hashed on both sides before planning"), "{e}"),
        (Some(want), Ok(plan)) => check(&plan, &want, &source_tree, &opts),
        (None, Ok(plan)) => panic!("planned without hashes: {plan:?}"),
        (Some(_), Err(e)) => panic!("refused: {e}"),
    }
}
