use crate::device::LineageOp;
use crate::progress::Ctx;
use crate::tidy::{self, TidyKind, TidyOptions};

use super::*;

const TIME: i64 = 1_600_000_000;

/// A drive cut off while writing `new` (noted before a byte was written), holding `there` too.
fn cut_off(dir: &std::path::Path) -> Device {
    let mut d = Device::open(dir).unwrap();
    d.begin(LineageOp::Sync, "sync", None).unwrap();
    d
}

fn put(d: &Device, key: &str, bytes: &[u8], mtime: i64) {
    let p = d.path(key);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, bytes).unwrap();
    fsutil::set_mtime(&p, mtime).unwrap();
}

/// Whatever state a cut-off write left each of its new files in, `drive tidy` removes the ones it
/// didn't finish and leaves the finished ones, the ones that were there before it started and the
/// ones it never got to. The next write to the drive does the same.
#[hegel::test(test_cases = 40)]
fn what_a_cut_off_write_left_goes_and_nothing_else(tc: hegel::TestCase) {
    use hegel::generators as gs;
    // Never started, part written, whole but unstamped, finished.
    let states: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>().max_value(3)).max_size(6));
    let by_tidy = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let mut d = cut_off(dir.path());
    put(&d, "Contents/old.mp3", b"the DJ's", TIME);
    let key = |i: usize| format!("Contents/{i}.mp3");
    let files = states.iter().enumerate().map(|(i, _)| (key(i), 4, TIME));
    note(&d, files.chain([("Contents/old.mp3".to_string(), 4, TIME), ("../outside.mp3".to_string(), 4, TIME)]))
        .unwrap();
    let now = chrono::Utc::now().timestamp();
    let mut left = Vec::new();
    for (i, state) in states.iter().enumerate() {
        match state {
            0 => {}
            1 => put(&d, &key(i), b"ab", now),
            2 => put(&d, &key(i), b"abcd", now),
            _ => put(&d, &key(i), b"abcd", TIME),
        }
        if matches!(state, 1 | 2) {
            left.push(key(i));
        }
    }
    let mut noted = unfinished(&d);
    noted.sort();
    left.sort();
    assert_eq!(noted, left);

    if by_tidy {
        let plan = tidy::plan(&d, &TidyOptions::default(), &Ctx::silent()).unwrap();
        assert!(plan.items.iter().all(|i| i.kind == TidyKind::TempFiles));
        assert_eq!(plan.items.len(), left.len());
        assert_eq!(tidy::apply(&plan, &d, &Ctx::silent()).unwrap().removed, left.len());
    } else {
        d.begin(LineageOp::Sync, "sync again", None).unwrap();
    }
    for (i, state) in states.iter().enumerate() {
        assert_eq!(d.path(&key(i)).exists(), *state == 3, "{i}: {state}");
    }
    assert_eq!(std::fs::read(d.path("Contents/old.mp3")).unwrap(), b"the DJ's");
}

/// Files are only ever taken for a cut-off write's on a drive marked as cut off, and once a write
/// is done nothing's noted.
#[test]
fn a_finished_write_leaves_nothing_noted() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = cut_off(dir.path());
    note(&d, [("Contents/a.mp3".to_string(), 4, TIME)].into_iter()).unwrap();
    put(&d, "Contents/a.mp3", b"ab", TIME + 500);
    assert_eq!(unfinished(&d), ["Contents/a.mp3"]);
    d.finish(None).unwrap();
    assert_eq!(unfinished(&d), Vec::<String>::new(), "not without the mark");
    forget(&d).unwrap();
    assert!(!file(&d).exists());
    note(&d, std::iter::empty()).unwrap();
    assert!(!file(&d).exists(), "nothing to note, nothing written");
}

/// A source written moments ago gives its copy a time like the write's own, so size and time say
/// nothing: what was written since the write began counts as unfinished, as a plan takes it.
#[test]
fn a_torn_file_with_its_sources_time_is_unfinished() {
    let dir = tempfile::tempdir().unwrap();
    let d = cut_off(dir.path());
    let now = chrono::Utc::now().timestamp();
    note(&d, [("Contents/a.mp3".to_string(), 4, now)].into_iter()).unwrap();
    put(&d, "Contents/a.mp3", b"\0\0\0\0", now);
    assert_eq!(unfinished(&d), ["Contents/a.mp3"]);
}

/// An export leaves nothing noted, and one cut off part way leaves its record until it's run again.
#[test]
fn nothing_is_left_noted_after_a_copy() {
    let dir = tempfile::tempdir().unwrap();
    let (root, src) = (dir.path().join("drive"), dir.path().join("a.mp3"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&src, b"abcd").unwrap();
    let target = Device::open(&root).unwrap();
    let mut planned = crate::library_write::Planned::default();
    planned.copies.push(crate::library_write::FileCopy { path: "Contents/a.mp3".into(), size: 4 });
    planned.sources.insert("Contents/a.mp3".into(), crate::library_write::Source::file(src, 4, TIME));
    crate::library_write::copy(&target, &planned, &Ctx::silent()).unwrap();
    assert!(!file(&target).exists());
}
