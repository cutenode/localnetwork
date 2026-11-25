//! Fixtures and Hegel generators for tests, behind the `test-util` feature. Not part of the
//! stable API: they change with the tests that use them.

use chrono::Utc;
use hegel::TestCase;
use hegel::generators as gs;

use crate::{DriveSpeed, MeasuredBy, Work};

/// A result a test measured just now, holding for any size, on no known link.
pub fn speed(bytes_per_sec: u64, create_us: u64, mkdir_us: u64) -> DriveSpeed {
    DriveSpeed {
        bytes_per_sec,
        create_us,
        mkdir_us,
        holds_for: None,
        measured_at: Utc::now(),
        measured_by: MeasuredBy::Test,
        link_bps: None,
        logged_bytes: None,
    }
}

/// A write of `bytes` in `files` files, making `folders` folders, from this computer's disk.
pub fn work(bytes: u64, files: u64, folders: u64) -> Work {
    Work { bytes, files, folders, ..Work::default() }
}

/// Any [`speed`] up to 2 GB/s and a second a file or folder, holding for any size or up to 4 TiB.
#[hegel::test_helper]
pub fn any_speed(tc: &TestCase) -> DriveSpeed {
    DriveSpeed {
        holds_for: tc.draw(gs::optional(gs::integers::<u64>().max_value(1 << 42))),
        ..speed(
            tc.draw(gs::integers::<u64>().min_value(1).max_value(2_000_000_000)),
            tc.draw(gs::integers::<u64>().max_value(1_000_000)),
            tc.draw(gs::integers::<u64>().max_value(1_000_000)),
        )
    }
}

/// Any [`work`] up to 4 TiB and a million files and folders.
#[hegel::test_helper]
pub fn any_work(tc: &TestCase) -> Work {
    work(
        tc.draw(gs::integers::<u64>().max_value(1 << 42)),
        tc.draw(gs::integers::<u64>().max_value(1 << 20)),
        tc.draw(gs::integers::<u64>().max_value(1 << 20)),
    )
}
