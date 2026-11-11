use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use loclib_progress::{CancelToken, Progress};

use super::*;
use crate::tests::{names, put};

/// Cancels once `after` items have begun.
struct CancelAfter {
    after: usize,
    seen: AtomicUsize,
    cancel: CancelToken,
}

impl Progress for CancelAfter {
    fn on_event(&self, event: &Event) {
        if matches!(event, Event::Item { .. }) && self.seen.fetch_add(1, Ordering::SeqCst) + 1 >= self.after {
            self.cancel.cancel();
        }
    }
}

/// Three files and one made: `b/c.txt` in a folder, which gets no entry of its own.
fn entries(dir: &Path) -> Vec<Entry> {
    let a = put(dir, "src/a.db", &[7u8; 3 * CHUNK]);
    let c = put(dir, "src/b/c.txt", b"c");
    let changed = put(dir, "src/d.xml", b"<d secret=\"1\"/>");
    vec![
        Entry { name: "a.db".into(), body: Body::File(a) },
        Entry { name: "b/c.txt".into(), body: Body::File(c) },
        Entry { name: "d.xml".into(), body: Body::Changed(changed, b"<d/>".to_vec()) },
        Entry { name: "contents.xml".into(), body: Body::Made(b"made".to_vec()) },
    ]
}

/// The entries in order, deflated, with no folder entries; each file's time (local, to DOS's two
/// seconds) and permissions; the zip readable by its owner alone.
#[test]
fn a_zip_holds_the_entries_in_order_with_their_files_times() {
    let dir = tempfile::tempdir().unwrap();
    let entries = entries(dir.path());
    let a = dir.path().join("src/a.db");
    let when = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_001);
    std::fs::File::options().write(true).open(&a).unwrap().set_modified(when).unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(&a, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    let into = dir.path().join("out/backup.zip");
    std::fs::create_dir(dir.path().join("out")).unwrap();
    let written = write(&entries, &into, &Ctx::silent()).unwrap();
    assert_eq!(written, std::fs::metadata(&into).unwrap().len());
    assert_eq!(names(&into), ["a.db", "b/c.txt", "d.xml", "contents.xml"]);
    let mut zip = zip::ZipArchive::new(File::open(&into).unwrap()).unwrap();
    let local = chrono::DateTime::<Local>::from(when).naive_local();
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).unwrap();
        assert!(!e.is_dir());
        assert_eq!(e.compression(), CompressionMethod::Deflated);
        // The zip crate's "made by", where rekordbox's say 6.3 (the README says so).
        assert_eq!(e.version_made_by(), (2, 0));
        assert_eq!(e.extra_data_fields().count(), 0, "no extra fields, as rekordbox's have none");
        let mut bytes = Vec::new();
        e.read_to_end(&mut bytes).unwrap();
        match e.name().unwrap().as_ref() {
            "a.db" => {
                assert_eq!(bytes, vec![7u8; 3 * CHUNK]);
                let t = e.last_modified().unwrap();
                let got = (t.year() as i32, t.month() as u32, t.day() as u32, t.hour() as u32, t.minute() as u32);
                assert_eq!(got, (local.year(), local.month(), local.day(), local.hour(), local.minute()));
                assert_eq!(t.second() as u32, local.second() / 2 * 2);
                #[cfg(unix)]
                assert_eq!(e.unix_mode().unwrap() & 0o777, 0o700);
            }
            "d.xml" => assert_eq!(bytes, b"<d/>"),
            "contents.xml" => assert_eq!(e.unix_mode().unwrap() & 0o777, MADE_MODE),
            _ => {}
        }
    }
    #[cfg(unix)]
    assert_eq!(
        std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&into).unwrap().permissions()) & 0o777,
        0o600
    );
    let left: Vec<_> = std::fs::read_dir(dir.path().join("out")).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(left, ["backup.zip"], "nothing but the zip");
}

/// Cancelled after any number of entries, the write leaves the zip that was there as it was and
/// nothing else.
#[test]
fn a_cancelled_write_leaves_what_was_there() {
    let dir = tempfile::tempdir().unwrap();
    let entries = entries(dir.path());
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let into = out.join("backup.zip");
    std::fs::write(&into, b"yesterday's").unwrap();
    for after in 1..=entries.len() {
        let cancel = CancelToken::new();
        let progress = CancelAfter { after, seen: AtomicUsize::new(0), cancel: cancel.clone() };
        let e = write(&entries, &into, &Ctx::new(&progress, cancel)).unwrap_err();
        assert_eq!(e.kind(), Some("cancelled"), "after {after}");
        assert_eq!(std::fs::read(&into).unwrap(), b"yesterday's", "after {after}");
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 1, "after {after}");
    }
}

/// A file gone since it was listed is left out with a line saying so; `master.db` gone fails the
/// write, which leaves nothing behind.
#[test]
fn a_file_gone_is_left_out_but_master_db_isnt() {
    let dir = tempfile::tempdir().unwrap();
    let mut entries = entries(dir.path());
    entries.insert(1, Entry { name: "gone".into(), body: Body::File(dir.path().join("gone")) });
    let into = dir.path().join("backup.zip");
    let warned = std::sync::Mutex::new(Vec::new());
    let progress = |e: &Event| {
        if let Event::Warning { message } = e {
            warned.lock().unwrap().push(message.clone());
        }
    };
    write(&entries, &into, &Ctx::new(&progress, CancelToken::new())).unwrap();
    assert_eq!(names(&into), ["a.db", "b/c.txt", "d.xml", "contents.xml"]);
    assert_eq!(*warned.lock().unwrap(), ["gone went while the backup was made, so it's left out"]);
    std::fs::remove_file(&into).unwrap();
    entries[1].name = "master.db".into();
    let e = write(&entries, &into, &Ctx::silent()).unwrap_err();
    assert_eq!(e.kind(), Some("io"), "{e:?}");
    let zips = std::fs::read_dir(dir.path()).unwrap().flatten().filter(|e| e.path().is_file()).count();
    assert_eq!(zips, 0);
}

/// A zip a killed backup left half-written goes with the next write; one under way elsewhere (new)
/// and other files stay.
#[test]
fn a_stale_part_goes_with_the_next_write() {
    let dir = tempfile::tempdir().unwrap();
    let entries = entries(dir.path());
    let out = dir.path().join("out");
    let stale = put(&out, ".rekordbox_bak_x1Yz.zip.part", b"tokens");
    let fresh = put(&out, ".rekordbox_bak_a2Bc.zip.part", b"under way");
    let other = put(&out, "notes.zip.part", b"mine");
    let long_ago = SystemTime::now() - Duration::from_secs(2 * 24 * 60 * 60);
    for p in [&stale, &other] {
        File::options().write(true).open(p).unwrap().set_modified(long_ago).unwrap();
    }
    write(&entries, &out.join("backup.zip"), &Ctx::silent()).unwrap();
    assert!(!stale.exists());
    assert!(fresh.exists() && other.exists());
}

/// `SOURCE_DATE_EPOCH` fixes every time (in UTC) and the date a backup is named after.
#[test]
fn source_date_epoch_fixes_times() {
    crate::tests::stamped();
    let t = dos_time(SystemTime::now());
    assert_eq!((t.year(), t.month(), t.day(), t.hour(), t.minute(), t.second()), (2026, 9, 21, 14, 13, 20));
    assert_eq!(today(), "20260921");
}

#[test]
fn times_out_of_dos_range_are_held_to_it() {
    let early = dos_time(SystemTime::UNIX_EPOCH);
    assert_eq!((early.year(), early.month(), early.day()), (1980, 1, 1));
    let late = dos_time(SystemTime::UNIX_EPOCH + Duration::from_secs(5_000_000_000));
    assert_eq!((late.year(), late.month(), late.day()), (2107, 12, 31));
}
