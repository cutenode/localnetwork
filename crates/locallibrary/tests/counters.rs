//! Exact counts of what everyday operations allocate, on a made-up library. Unlike a time, they come
//! out the same on any machine, so CI holds them to the budgets in `benchmarks/budgets/alloc.json`
//! (see `benchmarks/README.md`): with `LOCALLIBRARY_BUDGETS_OUT` set, they're written there as
//! `alloc.json` for `tools/budgets/cli.ts` to check.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use indexmap::IndexMap;
use locallibrary::check::{self, CheckOptions};
use locallibrary::diff;
use locallibrary::merge::{self, MergeOptions};
use locallibrary::pdb;
use locallibrary::progress::Ctx;
use locallibrary::query::{Filter, QueryContext};
use locallibrary::sync::{self, SyncOptions};
use synthetic::{build_pdb, drive_in, library};

#[path = "../benches/synthetic/mod.rs"]
mod synthetic;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

/// The system's allocator, counting what's asked of it.
struct Counting;

// SAFETY: every call goes straight to `System`, which upholds `GlobalAlloc`'s contract.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// How many allocations `f` makes and how many bytes they come to, each as a metric of `counts`.
fn count<T>(counts: &mut IndexMap<String, u64>, name: &str, f: impl FnOnce() -> T) {
    let (allocations, bytes) = (ALLOCATIONS.load(Ordering::Relaxed), BYTES.load(Ordering::Relaxed));
    std::hint::black_box(f());
    let (allocations, bytes) =
        (ALLOCATIONS.load(Ordering::Relaxed) - allocations, BYTES.load(Ordering::Relaxed) - bytes);
    counts.insert(format!("{name}.allocations"), allocations);
    counts.insert(format!("{name}.bytes"), bytes);
}

#[test]
fn everyday_operations_allocate_what_their_budgets_say() {
    // The same pool whatever the machine: how work is split decides some of what it allocates.
    rayon::ThreadPoolBuilder::new().num_threads(2).build_global().unwrap();
    let bytes = build_pdb(0, 2_000);
    let path = Path::new("export.pdb");
    let lib = library(&bytes);
    let other = library(&build_pdb(200, 2_000));
    // In the temporary folder, not the target folder: a path's length is part of what's counted,
    // and CI's target folder isn't where anyone else's is.
    let dir = std::env::temp_dir().join("locallibrary-counters");
    let (a, b) = (drive_in(&dir, "A", 0, 500), drive_in(&dir, "B", 50, 500));
    let ctx = Ctx::silent();
    // Once uncounted: the first scan of a new drive hashes what later ones find in its hash cache.
    sync::plan_drive_to_drive(&a, &b, &SyncOptions::default(), &ctx).unwrap();
    let filter = Filter::parse("artist:\"artist 12\" bpm:120-130 -genre:techno").unwrap();

    let mut counts = IndexMap::new();
    count(&mut counts, "read_library.2k", || pdb::into_library(pdb::parse_database(bytes.clone(), path).unwrap()));
    count(&mut counts, "filter.2k", || filter.select(&QueryContext::new(&lib)).len());
    count(&mut counts, "compare.2k", || diff::between(&lib, &other, ("a", "b"), None));
    count(&mut counts, "merge_plan.500", || merge::plan(&a, &b, &MergeOptions::default(), &ctx).unwrap());
    count(&mut counts, "sync_plan.500", || sync::plan_drive_to_drive(&a, &b, &SyncOptions::default(), &ctx).unwrap());
    let both = [a, b];
    count(&mut counts, "check.500", || check::check(&both, None, &CheckOptions::default(), &ctx).unwrap());

    assert!(counts.values().all(|&n| n > 0), "{counts:?}");
    let Some(out) = std::env::var_os("LOCALLIBRARY_BUDGETS_OUT") else { return };
    fs::create_dir_all(&out).unwrap();
    fs::write(Path::new(&out).join("alloc.json"), serde_json::to_vec_pretty(&counts).unwrap()).unwrap();
}
