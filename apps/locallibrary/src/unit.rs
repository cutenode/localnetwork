// The binary's unit tests: its source, built as an integration test (see the `unit` test in
// Cargo.toml). Modules resolve from this folder, as they do from main.rs.
include!("main.rs");

/// Counts what's allocated, for what `tui/frames/tests.rs` holds to `benchmarks/budgets/tui.json`
/// (`crates/locallibrary/tests/counters.rs` does the same for the library). Only in this binary:
/// the shipped one uses the system's allocator, uncounted.
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicU64, Ordering};

    pub static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
    pub static BYTES: AtomicU64 = AtomicU64::new(0);

    /// The system's allocator, counting what's asked of it.
    pub struct Counting;

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
}

#[global_allocator]
static COUNTING: counting::Counting = counting::Counting;
