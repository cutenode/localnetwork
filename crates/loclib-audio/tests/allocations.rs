//! What vetting a file that isn't an MP4 or a FLAC allocates. `check` vets every file it can't read, so a
//! read buffer here would be one for each of them.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use loclib_audio::vet_container;

thread_local! {
    /// Allocations asked for on this thread, so the test harness's own don't count.
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

/// The system's allocator, counting what's asked of it.
struct Counting;

// SAFETY: every call goes straight to `System`, which upholds `GlobalAlloc`'s contract.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.with(|n| n.set(n.get() + 1));
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.with(|n| n.set(n.get() + 1));
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// What `f` allocates on this thread.
fn allocations(f: impl FnOnce()) -> u64 {
    let before = ALLOCATIONS.with(Cell::get);
    f();
    ALLOCATIONS.with(Cell::get) - before
}

/// A file whose bytes and name don't say MP4, and whose bytes don't say FLAC, is passed allocating nothing beyond opening it.
#[test]
fn vetting_a_file_that_isnt_an_mp4_allocates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.mp3");
    std::fs::write(&path, b"ID3\x04\0\0\0\0\0\0 not really an mp3").unwrap();
    // On Windows std's `File::open` allocates the path as UTF-16, which isn't the vetting's.
    let open = allocations(|| drop(std::fs::File::open(&path).unwrap()));
    assert_eq!(allocations(|| vet_container(&path).unwrap()), open);
}
