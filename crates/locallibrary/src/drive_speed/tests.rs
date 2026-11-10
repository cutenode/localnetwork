use std::time::{Duration, Instant};

use chrono::TimeDelta;
use hegel::TestCase;
use hegel::generators as gs;

use super::estimate::{duration, link_name};
use super::probe::{more, settle, timed_write};
use super::*;
use crate::progress::{CancelToken, Event, Progress};

const USB2: u64 = 480_000_000;
const USB3: u64 = 5_000_000_000;
const MB: u64 = 1_000_000;
const GB: u64 = 1_000_000_000;

fn speed(bytes_per_sec: u64, create_us: u64, mkdir_us: u64) -> DriveSpeed {
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

fn now() -> DriveNow {
    DriveNow { at: Utc::now(), logged_bytes: None, capacity: None, link_bps: None }
}

fn work(bytes: u64, files: u64, folders: u64) -> Work {
    Work { bytes, files, folders, ..Work::default() }
}

/// Keeps a test's warnings.
#[derive(Default)]
struct Warnings(std::sync::Mutex<Vec<String>>);

impl Progress for Warnings {
    fn on_event(&self, event: &Event) {
        if let Event::Warning { message } = event {
            self.0.lock().unwrap().push(message.clone());
        }
    }
}

impl Warnings {
    fn said(&self, what: &str) -> bool {
        self.0.lock().unwrap().iter().any(|m| m.contains(what))
    }
}

/// `speed`, as a drive at `root` has it.
fn keep(root: &Path, speed: &DriveSpeed) {
    std::fs::create_dir_all(root.join(crate::paths::STATE_DIR)).unwrap();
    save(root, speed).unwrap();
}

fn estimate_of(work: Work, speed: DriveSpeed) -> WriteEstimate {
    WriteEstimate::new(work, Some(speed), None, None)
}

/// A drive for a test: a temp folder, opened.
fn drive() -> (tempfile::TempDir, Device) {
    let dir = tempfile::tempdir().unwrap();
    let device = Device::open(dir.path()).unwrap();
    (dir, device)
}

/// The two exports timed end to end, 3,000 tracks, with the costs measured on those drives without
/// locallibrary: the sum lands within a tenth of what the export took.
#[test]
fn the_sum_is_close_to_real_exports() {
    let fixture = work(28_942_463_040, 15_000, 3_000);
    let stick = estimate_of(fixture.clone(), speed(151 * MB, 12_000, 14_000));
    let took = [439.0, 442.0, 431.0, 430.0].iter().sum::<f64>() / 4.0;
    assert!((stick.seconds.unwrap() as f64 / took - 1.0).abs() < 0.1, "{stick:?}");
    assert_eq!(stick.limit, Some(Limit::Files));

    let card = estimate_of(fixture, speed(16 * MB, 6_000, 43_000));
    let took = [2006.0, 2151.0].iter().sum::<f64>() / 2.0;
    assert!((card.seconds.unwrap() as f64 / took - 1.0).abs() < 0.1, "{card:?}");
    assert_eq!(card.limit, Some(Limit::Rate));
}

#[test]
fn a_plan_says_the_time_and_what_it_comes_from() {
    let stick = estimate_of(work(28_942_463_040, 15_000, 3_000), speed(151 * MB, 12_000, 14_000));
    assert_eq!(stick.lines(), ["about 7 min, mostly making files (drive writes ~151 MB/s, 12 ms a file)"]);
    let card = estimate_of(work(28_942_463_040, 15_000, 3_000), speed(16 * MB, 6_000, 43_000));
    assert_eq!(card.lines(), ["about 34 min (drive writes ~16 MB/s, 6 ms a file)"]);
    let unmeasured = WriteEstimate::new(work(GB, 1, 0), None, None, None);
    assert_eq!(unmeasured.lines(), ["time unknown: this drive hasn't been measured"]);
    assert_eq!(unmeasured.seconds, None);
}

#[test]
fn what_else_the_time_depends_on_is_said() {
    let downloads = estimate_of(work(GB, 1, 0).plus("downloads"), speed(100 * MB, 1_000, 1_000));
    assert_eq!(downloads.lines()[0], "under a minute, plus downloads (drive writes ~100 MB/s, 1 ms a file)");
    assert!(!downloads.drive_alone());
}

/// A rate that stayed flat in the test vouches for no more than the test wrote.
#[test]
fn past_what_the_rate_held_for_it_is_a_best_case() {
    let flat = DriveSpeed { holds_for: Some(2 * GB), ..speed(100 * MB, 1_000, 1_000) };
    assert!(!estimate_of(work(2 * GB, 1, 0), flat.clone()).best_case);
    let longer = estimate_of(work(30 * GB, 1, 0), flat.clone());
    assert!(longer.best_case);
    assert!(longer.lines()[0].starts_with("at least 5 min"), "{:?}", longer.lines());
    let quick = estimate_of(work(3 * GB, 1, 0), flat);
    assert!(quick.lines()[0].starts_with("a minute or more"), "{:?}", quick.lines());
    let past_the_cache = estimate_of(work(30 * GB, 1, 0), speed(100 * MB, 1_000, 1_000));
    assert!(!past_the_cache.best_case);
}

/// The port never changes the number, only what's said beside it.
#[test]
fn a_port_near_the_rate_is_a_hint() {
    let job = || work(30 * GB, 3_000, 300);
    let slow_port = WriteEstimate::new(job(), Some(speed(38 * MB, 12_000, 14_000)), None, Some(USB2));
    assert_eq!(slow_port.port_limit_bps, Some(USB2));
    assert_eq!(slow_port.lines()[1], "this port (USB 2.0) tops out near 42 MB/s, so a faster one may help");
    let fast_port = WriteEstimate::new(job(), Some(speed(38 * MB, 12_000, 14_000)), None, Some(USB3));
    assert_eq!(fast_port.port_limit_bps, None);
    assert_eq!(fast_port.lines().len(), 1);
    assert_eq!(slow_port.seconds, fast_port.seconds);
    let drive_below_it = WriteEstimate::new(job(), Some(speed(18 * MB, 12_000, 14_000)), None, Some(USB2));
    assert_eq!(drive_below_it.port_limit_bps, None);
}

#[test]
fn a_stale_result_says_why() {
    let line = |stale| WriteEstimate::new(work(GB, 1, 0), Some(speed(100 * MB, 1_000, 1_000)), Some(stale), None);
    let said = |stale| line(stale).lines()[0].clone();
    assert!(said(Stale::Old { days: 120 }).contains("measured 120 days ago"));
    assert!(said(Stale::Worn { bytes: 320 * GB }).contains("measured before 320.0 GB of writes"));
    assert!(said(Stale::Moved { then: USB3, now: USB2 }).contains("measured on a USB 3, 5 Gb/s port"));
}

#[test]
fn a_result_goes_stale_with_age_writes_and_ports() {
    let fresh = speed(100 * MB, 1_000, 1_000);
    assert_eq!(fresh.stale(&now()), None);
    let old = DriveSpeed { measured_at: Utc::now() - TimeDelta::days(91), ..fresh.clone() };
    assert_eq!(old.stale(&now()), Some(Stale::Old { days: 91 }));
    let keeps = DriveSpeed { measured_at: Utc::now() - TimeDelta::days(90), ..fresh.clone() };
    assert_eq!(keeps.stale(&now()), None);

    // The card that fell from 47 to 16 MB/s: 320 GB of rewrites on 128 GB.
    let logged = DriveSpeed { logged_bytes: Some(10 * GB), ..fresh.clone() };
    let worn = DriveNow { logged_bytes: Some(330 * GB), capacity: Some(128 * GB), ..now() };
    assert_eq!(logged.stale(&worn), Some(Stale::Worn { bytes: 320 * GB }));
    let little = DriveNow { logged_bytes: Some(137 * GB), capacity: Some(128 * GB), ..now() };
    assert_eq!(logged.stale(&little), None);
    // A store that's never logged this drive says nothing about it.
    assert_eq!(logged.stale(&DriveNow { capacity: Some(128 * GB), ..now() }), None);

    let port = DriveSpeed { link_bps: Some(USB3), ..fresh };
    let moved = DriveNow { link_bps: Some(USB2), ..now() };
    assert_eq!(port.stale(&moved), Some(Stale::Moved { then: USB3, now: USB2 }));
    assert_eq!(port.stale(&DriveNow { link_bps: Some(USB3), ..now() }), None);
    assert_eq!(port.stale(&now()), None);
}

#[test]
fn durations_read_like_a_guess() {
    for (seconds, said) in [
        (60, "1 min"),
        (89, "1 min"),
        (90, "2 min"),
        (3599, "60 min"),
        (3600, "1 h"),
        (5100, "1 h 25 min"),
        (7199, "2 h"),
    ] {
        assert_eq!(duration(seconds), said);
    }
}

#[test]
fn links_go_by_their_names() {
    for (bps, said) in [
        (12 * MB, "USB 1"),
        (USB2, "USB 2.0"),
        (USB3, "USB 3, 5 Gb/s"),
        (10 * GB, "USB 3, 10 Gb/s"),
        (20 * GB, "USB 3, 20 Gb/s"),
    ] {
        assert_eq!(link_name(bps), said);
    }
}

/// The folders it finds are those the files go in that the drive lacks, ancestors included, each
/// once: an existing folder costs nothing.
#[test]
fn work_counts_only_new_folders() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("Contents/Have")).unwrap();
    let copies = [
        ("Contents/Have/a.mp3", 10),
        ("Contents/Have/b.mp3", 20),
        ("Contents/New/Album/c.mp3", 30),
        ("Contents/New/Album/d.mp3", 40),
        ("PIONEER/USBANLZ/P001/ANLZ0000.DAT", 5),
        ("loose.txt", 1),
    ];
    let work = Work::copying(dir.path(), copies);
    assert_eq!((work.bytes, work.files), (106, 6));
    // Contents/New, Contents/New/Album, PIONEER, PIONEER/USBANLZ, PIONEER/USBANLZ/P001.
    assert_eq!(work.folders, 5);
}

#[test]
fn small_writes_say_nothing() {
    assert!(!work((64 << 20) - 1, 255, 10).matters());
    assert!(work(64 << 20, 1, 0).matters());
    assert!(work(1, 256, 0).matters());
}

/// A drive at 5 MB/s for its cache region, then 1: slow, so a test's budget is spent on few bytes.
fn cached_drive(cache: u64) -> impl FnMut(&[u8]) -> std::io::Result<Duration> {
    let mut written = 0;
    move |chunk: &[u8]| {
        let rate = if written < cache { 5 * MB } else { MB };
        written += chunk.len() as u64;
        Ok(Duration::from_secs_f64(chunk.len() as f64 / rate as f64))
    }
}

#[test]
fn a_test_past_the_cache_finds_the_rate_behind_it() {
    let mut chunk = vec![0; 1 << 20];
    let rate = timed_write(1 << 30, &mut chunk, &mut cached_drive(40 << 20), &Ctx::silent()).unwrap();
    assert!(rate.bytes_per_sec.abs_diff(MB) < 10, "{rate:?}");
    assert_eq!(rate.holds_for, None);
}

/// A cache bigger than the test can't be told from a drive with none.
#[test]
fn a_test_that_stays_in_the_cache_vouches_only_for_what_it_wrote() {
    let mut chunk = vec![0; 1 << 20];
    let rate = timed_write(32 << 20, &mut chunk, &mut cached_drive(1 << 30), &Ctx::silent()).unwrap();
    assert!(rate.bytes_per_sec.abs_diff(5 * MB) < 10, "{rate:?}");
    assert_eq!(rate.holds_for, Some(32 << 20));
}

#[test]
fn a_test_stops_when_its_time_is_up() {
    let mut chunk = vec![0; 1 << 20];
    let mut slow = |_: &[u8]| Ok(Duration::from_secs(2));
    let rate = timed_write(32 << 20, &mut chunk, &mut slow, &Ctx::silent()).unwrap();
    // 15 s at 2 s a chunk.
    assert_eq!(rate.holds_for, Some(8 << 20));
}

/// A test makes its small files and folders up to its count, and past its minimum only while its
/// time lasts: a slow drive stops early, with enough made to tell their cost.
#[test]
fn a_test_makes_items_within_its_count_and_time() {
    let now = Instant::now();
    let late = now - probe::ITEM_BUDGET - Duration::from_millis(1);
    assert!(more(now, probe::ITEMS - 1));
    assert!(!more(now, probe::ITEMS));
    assert!(more(late, probe::MIN_ITEMS - 1));
    assert!(!more(late, probe::MIN_ITEMS));
}

#[test]
fn each_chunk_of_a_test_differs() {
    let mut chunk = vec![0; 1 << 20];
    let mut seen = Vec::new();
    let mut record = |bytes: &[u8]| {
        seen.push(bytes.to_vec());
        Ok(Duration::from_millis(5))
    };
    timed_write(3 << 20, &mut chunk, &mut record, &Ctx::silent()).unwrap();
    assert_eq!(seen.len(), 3);
    assert_ne!(seen[0], seen[1]);
    assert_ne!(seen[1], seen[2]);
}

#[test]
fn a_cancelled_test_stops() {
    let cancel = CancelToken::new();
    cancel.cancel();
    let progress = crate::progress::NoProgress;
    let ctx = Ctx::new(&progress, cancel);
    let mut chunk = vec![0; 1 << 20];
    let mut write = |_: &[u8]| panic!("nothing should be written");
    let err = timed_write(4 << 20, &mut chunk, &mut write, &ctx).unwrap_err();
    assert!(matches!(err, crate::Error::Cancelled));
}

#[test]
fn a_drive_that_fails_fails_the_test() {
    let mut chunk = vec![0; 1 << 20];
    let mut full = |_: &[u8]| Err(std::io::Error::other("no room"));
    assert!(matches!(timed_write(4 << 20, &mut chunk, &mut full, &Ctx::silent()), Err(crate::Error::Io { .. })));
}

/// Measured on a real folder: the costs are kept, and nothing's left of the test.
#[test]
fn measuring_keeps_the_result_and_leaves_no_scratch() {
    let (dir, device) = drive();
    // What a test cut off leaves goes too.
    let left = probe::scratch_path(dir.path());
    std::fs::create_dir_all(&left).unwrap();
    std::fs::write(left.join("rate"), b"cut off").unwrap();

    let facts = DriveNow { link_bps: Some(USB3), logged_bytes: Some(7), ..now() };
    let measured = measure(&device, 64 << 20, &facts, &Ctx::silent()).unwrap().unwrap();
    assert!(measured.bytes_per_sec > 0);
    assert!(measured.create_us > 0);
    assert!(measured.mkdir_us > 0);
    assert_eq!(measured.measured_by, MeasuredBy::Test);
    assert_eq!((measured.link_bps, measured.logged_bytes), (Some(USB3), Some(7)));
    assert_eq!(stored(&device), Some(measured));
    assert!(!left.exists());
}

#[test]
fn a_cancelled_measure_leaves_no_scratch_and_no_result() {
    let (dir, device) = drive();
    let cancel = CancelToken::new();
    cancel.cancel();
    let progress = crate::progress::NoProgress;
    let ctx = Ctx::new(&progress, cancel);
    assert!(matches!(measure(&device, 64 << 20, &now(), &ctx), Err(crate::Error::Cancelled)));
    assert!(!probe::scratch_path(dir.path()).exists());
    assert_eq!(stored(&device), None);
}

#[test]
fn measuring_writes_nothing_with_no_room_for_a_test() {
    let (dir, device) = drive();
    assert_eq!(measure(&device, MIN_TEST_BYTES - 1, &now(), &Ctx::silent()).unwrap(), None);
    assert_eq!(stored(&device), None);
    assert!(!dir.path().join(crate::paths::STATE_DIR).exists());
}

#[test]
fn a_record_that_cant_be_read_counts_as_none() {
    let (dir, device) = drive();
    let path = record_path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"{not json").unwrap();
    assert_eq!(stored(&device), None);
    // Hand-edited to a rate that would divide by nothing.
    keep(dir.path(), &speed(0, 1, 1));
    assert_eq!(stored(&device), None);
}

#[test]
fn an_estimate_uses_what_is_stored_without_writing() {
    let (dir, device) = drive();
    let kept = speed(100 * MB, 1_000, 1_000);
    keep(dir.path(), &kept);
    let made = estimate(&device, work(GB, 300, 3), &now(), true, &Ctx::silent()).unwrap().unwrap();
    assert_eq!(made.speed, Some(kept.clone()));
    assert_eq!(stored(&device), Some(kept));
}

#[test]
fn too_small_a_write_measures_nothing() {
    let (dir, device) = drive();
    assert!(estimate(&device, work(10, 1, 0), &now(), true, &Ctx::silent()).unwrap().is_none());
    assert!(!dir.path().join(crate::paths::STATE_DIR).exists());
}

/// A dry run may not write: it says what it knows.
#[test]
fn without_leave_to_measure_nothing_is_written() {
    let (dir, device) = drive();
    let made = estimate(&device, work(GB, 300, 3), &now(), false, &Ctx::silent()).unwrap().unwrap();
    assert_eq!(made.speed, None);
    assert!(!dir.path().join(crate::paths::STATE_DIR).exists());

    let old = DriveSpeed { measured_at: Utc::now() - TimeDelta::days(200), ..speed(100 * MB, 1_000, 1_000) };
    keep(dir.path(), &old);
    let made = estimate(&device, work(GB, 300, 3), &now(), false, &Ctx::silent()).unwrap().unwrap();
    assert_eq!(made.stale, Some(Stale::Old { days: 200 }));
    assert_eq!(stored(&device), Some(old));
}

#[test]
fn a_drive_never_measured_or_gone_stale_is_measured() {
    let (dir, device) = drive();
    let made = estimate(&device, work(64 << 20, 300, 3), &now(), true, &Ctx::silent()).unwrap().unwrap();
    assert!(made.speed.is_some() && made.seconds.is_some());
    assert_eq!(stored(&device), made.speed);

    let old = DriveSpeed { measured_at: Utc::now() - TimeDelta::days(200), ..speed(1, 1_000, 1_000) };
    keep(dir.path(), &old);
    let made = estimate(&device, work(64 << 20, 300, 3), &now(), true, &Ctx::silent()).unwrap().unwrap();
    assert_eq!(made.stale, None);
    assert_ne!(made.speed, Some(old));
}

/// The test never writes more than the write it's for: on a fake drive whose free space wraps onto
/// its library, it mustn't reach further than the write would.
#[test]
fn a_test_writes_no_more_than_the_write_it_is_for() {
    let (_dir, device) = drive();
    let made = estimate(&device, work(64 << 20, 300, 3), &now(), true, &Ctx::silent()).unwrap().unwrap();
    assert!(made.speed.unwrap().holds_for.is_none_or(|held| held <= 64 << 20));
}

/// What stops a test is said, and the write goes ahead on what's known.
#[test]
fn a_drive_that_cant_be_tested_still_gets_an_estimate() {
    let (dir, device) = drive();
    std::fs::write(dir.path().join(crate::paths::STATE_DIR), b"a file, not a folder").unwrap();
    let warnings = Warnings::default();
    let ctx = Ctx::new(&warnings, CancelToken::new());
    let made = estimate(&device, work(GB, 300, 3), &now(), true, &ctx).unwrap().unwrap();
    assert_eq!(made.speed, None);
    assert!(warnings.said("couldn't measure"));
}

/// A plan says it ran a test, and one from what's stored doesn't.
#[test]
fn a_plan_says_when_it_ran_a_test() {
    let (dir, device) = drive();
    let tested = estimate(&device, work(64 << 20, 300, 3), &now(), true, &Ctx::silent()).unwrap().unwrap();
    assert!(tested.ran_test);
    assert!(tested.lines().iter().any(|l| l.contains("ran a short test")), "{:?}", tested.lines());
    tested.keep();
    drop(tested);
    assert!(dir.path().join(crate::paths::STATE_DIR).join(RECORD_FILE).exists());
    let again = estimate(&device, work(64 << 20, 300, 3), &now(), true, &Ctx::silent()).unwrap().unwrap();
    assert!(!again.ran_test);
    assert!(!again.lines().iter().any(|l| l.contains("ran a short test")));
}

/// Whatever becomes of a plan short of applying it, a drive with no `.locallibrary/` ends up as it
/// was.
#[test]
fn a_plan_not_applied_leaves_a_blank_drive_as_it_found_it() {
    let (dir, device) = drive();
    let state = dir.path().join(crate::paths::STATE_DIR);
    let made = estimate(&device, work(64 << 20, 300, 3), &now(), true, &Ctx::silent()).unwrap().unwrap();
    assert!(state.exists(), "the test keeps its result until the plan's answered");
    drop(made);
    assert!(!state.exists());
}

#[test]
fn a_plan_applied_keeps_what_the_test_left() {
    let (dir, device) = drive();
    let made = estimate(&device, work(64 << 20, 300, 3), &now(), true, &Ctx::silent()).unwrap().unwrap();
    made.keep();
    drop(made);
    assert!(stored(&device).is_some());
    assert!(dir.path().join(crate::paths::STATE_DIR).exists());
}

/// A drive that had the folder keeps it: it isn't the test's, and neither is what's in it.
#[test]
fn a_plan_not_applied_leaves_a_used_drive_its_folder() {
    let (dir, device) = drive();
    let state = dir.path().join(crate::paths::STATE_DIR);
    std::fs::create_dir(&state).unwrap();
    std::fs::write(state.join("state.json"), b"{}").unwrap();
    drop(estimate(&device, work(64 << 20, 300, 3), &now(), true, &Ctx::silent()).unwrap());
    assert!(state.join("state.json").exists());
    assert!(stored(&device).is_some());
}

/// Something else in the folder by the time the plan goes isn't removed with it.
#[test]
fn what_arrives_during_a_plan_stays() {
    let (dir, device) = drive();
    let made = estimate(&device, work(64 << 20, 300, 3), &now(), true, &Ctx::silent()).unwrap().unwrap();
    let state = dir.path().join(crate::paths::STATE_DIR);
    std::fs::write(state.join("state.json"), b"{}").unwrap();
    drop(made);
    assert!(state.join("state.json").exists());
    assert!(!state.join(RECORD_FILE).exists());
}

#[test]
fn a_cancelled_estimate_stops_the_plan() {
    let (dir, device) = drive();
    let cancel = CancelToken::new();
    cancel.cancel();
    let progress = crate::progress::NoProgress;
    let ctx = Ctx::new(&progress, cancel);
    assert!(matches!(estimate(&device, work(GB, 300, 3), &now(), true, &ctx), Err(crate::Error::Cancelled)));
    assert!(!dir.path().join(crate::paths::STATE_DIR).exists());
}

/// 30 GB in 400 s, 100 of them making files, is 100 MB/s, whatever the test said.
#[test]
fn a_real_write_corrects_the_rate() {
    let (dir, device) = drive();
    std::fs::create_dir(dir.path().join(crate::paths::STATE_DIR)).unwrap();
    let test = DriveSpeed { holds_for: Some(2 * GB), ..speed(400 * MB, 10_000, 0) };
    let job = estimate_of(work(30 * GB, 10_000, 0), test.clone());
    let facts = DriveNow { logged_bytes: Some(40 * GB), ..now() };
    learn(dir.path(), &job, Duration::from_secs(400), &facts, &Ctx::silent());
    let learned = stored(&device).unwrap();
    assert_eq!(learned.bytes_per_sec, 100 * MB);
    assert_eq!(learned.holds_for, Some(30 * GB));
    assert_eq!(learned.measured_by, MeasuredBy::Write);
    assert_eq!(learned.logged_bytes, Some(40 * GB));
    assert_eq!((learned.create_us, learned.mkdir_us), (test.create_us, test.mkdir_us));
    // Now it's "about", not "at least".
    assert!(!estimate_of(work(30 * GB, 1, 0), learned).best_case);
}

#[test]
fn a_real_write_says_nothing_when_it_cant() {
    let (dir, device) = drive();
    let test = speed(400 * MB, 10_000, 0);
    let elapsed = Duration::from_secs(400);
    let too_small = work(MIN_LEARNED_BYTES - 1, 10, 0);
    learn(dir.path(), &estimate_of(too_small, test.clone()), elapsed, &now(), &Ctx::silent());
    let source = work(30 * GB, 10, 0).read_elsewhere();
    learn(dir.path(), &estimate_of(source, test.clone()), elapsed, &now(), &Ctx::silent());
    let plus = work(30 * GB, 10, 0).plus("downloads");
    learn(dir.path(), &estimate_of(plus, test.clone()), elapsed, &now(), &Ctx::silent());
    let unmeasured = WriteEstimate::new(work(30 * GB, 10, 0), None, None, None);
    learn(dir.path(), &unmeasured, elapsed, &now(), &Ctx::silent());
    // Quicker than its files alone could be: the costs are off, so the rate can't be told.
    let impossible = estimate_of(work(30 * GB, 100_000, 0), test);
    learn(dir.path(), &impossible, elapsed, &now(), &Ctx::silent());
    assert_eq!(stored(&device), None);
}

#[test]
fn a_correction_that_cant_be_kept_is_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(crate::paths::STATE_DIR), b"a file, not a folder").unwrap();
    let warnings = Warnings::default();
    let ctx = Ctx::new(&warnings, CancelToken::new());
    let job = estimate_of(work(30 * GB, 10, 0), speed(400 * MB, 1_000, 0));
    learn(dir.path(), &job, Duration::from_secs(400), &now(), &ctx);
    assert!(warnings.said("couldn't keep"));
}

#[hegel::test_helper]
fn any_speed(tc: &TestCase) -> DriveSpeed {
    DriveSpeed {
        holds_for: tc.draw(gs::optional(gs::integers::<u64>().max_value(1 << 42))),
        ..speed(
            tc.draw(gs::integers::<u64>().min_value(1).max_value(2_000 * MB)),
            tc.draw(gs::integers::<u64>().max_value(1_000_000)),
            tc.draw(gs::integers::<u64>().max_value(1_000_000)),
        )
    }
}

#[hegel::test_helper]
fn any_work(tc: &TestCase) -> Work {
    work(
        tc.draw(gs::integers::<u64>().max_value(1 << 42)),
        tc.draw(gs::integers::<u64>().max_value(1 << 20)),
        tc.draw(gs::integers::<u64>().max_value(1 << 20)),
    )
}

/// The estimate is the sum of its three costs, rounded up, and more of any of them never shortens
/// it.
#[hegel::test]
fn the_estimate_is_the_sum_of_its_costs(tc: TestCase) {
    let (speed, job) = (any_speed(&tc), any_work(&tc));
    let made = estimate_of(job.clone(), speed.clone());
    let sum = job.bytes as f64 / speed.bytes_per_sec as f64
        + (job.files as f64 * speed.create_us as f64 + job.folders as f64 * speed.mkdir_us as f64) / 1e6;
    assert_eq!(made.seconds, Some(sum.ceil() as u64));

    let more = Work {
        bytes: job.bytes + tc.draw(gs::integers::<u64>().max_value(1 << 40)),
        files: job.files + tc.draw(gs::integers::<u64>().max_value(1 << 20)),
        folders: job.folders + tc.draw(gs::integers::<u64>().max_value(1 << 20)),
        ..job
    };
    assert!(estimate_of(more, speed).seconds >= made.seconds);
}

/// The port changes what's said beside the number, never the number.
#[hegel::test]
fn the_port_never_changes_the_time(tc: TestCase) {
    let (speed, job) = (any_speed(&tc), any_work(&tc));
    let link = tc.draw(gs::sampled_from(vec![12 * MB, USB2, USB3, 10 * GB, 20 * GB]));
    let with_port = WriteEstimate::new(job.clone(), Some(speed.clone()), None, Some(link));
    let without = estimate_of(job, speed);
    assert_eq!(with_port.seconds, without.seconds);
    assert_eq!(with_port.limit, without.limit);
    assert_eq!(with_port.lines().first(), without.lines().first());
    assert_eq!(with_port.lines().len(), 1 + usize::from(with_port.port_limit_bps.is_some()));
}

/// `best_case` is exactly a rate seen to hold for less than the write.
#[hegel::test]
fn a_best_case_is_a_write_past_what_was_seen(tc: TestCase) {
    let (speed, job) = (any_speed(&tc), any_work(&tc));
    let made = estimate_of(job.clone(), speed.clone());
    assert_eq!(made.best_case, speed.holds_for.is_some_and(|held| held < job.bytes));
    let said = &made.lines()[0];
    assert_eq!(made.best_case, said.starts_with("at least") || said.starts_with("a minute or more"));
}

/// A flat run is its mean, good for what it wrote. A run whose last third is well under its first
/// is the last third's rate, good for any size. Either way it's within the chunks' rates.
#[hegel::test(test_cases = 500)]
fn a_rate_comes_from_its_chunks(tc: TestCase) {
    let chunks: Vec<Duration> = tc
        .draw(gs::vecs(gs::integers::<u64>().min_value(1).max_value(10_000_000)).min_size(1).max_size(64))
        .into_iter()
        .map(Duration::from_micros)
        .collect();
    let size = 32 << 20;
    let rate = settle(size, &chunks);
    let of = |d: &Duration| size as f64 / d.as_secs_f64();
    let fastest = chunks.iter().map(of).fold(0.0, f64::max);
    let slowest = chunks.iter().map(of).fold(f64::MAX, f64::min);
    assert!(rate.bytes_per_sec as f64 <= fastest + 1.0);
    assert!(rate.bytes_per_sec as f64 >= slowest - 1.0);
    if let Some(held) = rate.holds_for {
        assert_eq!(held, chunks.len() as u64 * size);
    } else {
        assert!(chunks.len() >= 3);
    }
}

/// A real write taking what the estimate said gives back the rate it was worked out from.
#[hegel::test]
fn an_estimate_met_changes_no_rate(tc: TestCase) {
    let speed = any_speed(&tc);
    let bytes = tc.draw(gs::integers::<u64>().min_value(MIN_LEARNED_BYTES).max_value(1 << 42));
    let job = work(bytes, tc.draw(gs::integers::<u64>().max_value(100_000)), 0);
    let secs = job.bytes as f64 / speed.bytes_per_sec as f64 + job.small_files_secs(&speed);
    let learned = speed.corrected(&job, Duration::from_secs_f64(secs), &now()).unwrap();
    // Within a percent, and a rate of a few bytes a second rounds down by one.
    let off = learned.bytes_per_sec.abs_diff(speed.bytes_per_sec);
    assert!(off <= 1 + speed.bytes_per_sec / 100, "{speed:?} {learned:?}");
    match speed.holds_for {
        Some(held) => assert_eq!(learned.holds_for, Some(held.max(bytes))),
        None => assert_eq!(learned.holds_for, None),
    }
}

/// However files are laid out, the new folders are the distinct ancestors a naive walk finds.
#[hegel::test(test_cases = 50)]
fn work_matches_a_walk_of_the_paths(tc: TestCase) {
    let part = || gs::sampled_from(vec!["a", "b", "c"]);
    let paths: Vec<Vec<&str>> = tc.draw(gs::vecs(gs::vecs(part()).min_size(1).max_size(4)).max_size(24));
    let keys: Vec<String> = paths.iter().map(|p| p.join("/")).collect();
    let dir = tempfile::tempdir().unwrap();
    let have = dir.path().join(tc.draw(part()));
    std::fs::create_dir(&have).unwrap();
    let work = Work::copying(dir.path(), keys.iter().map(|k| (k.as_str(), 1)));

    let mut folders = std::collections::BTreeSet::new();
    for key in &keys {
        let mut path = Path::new(key).parent();
        while let Some(p) = path.filter(|p| !p.as_os_str().is_empty()) {
            folders.insert(p.to_path_buf());
            path = p.parent();
        }
    }
    let missing = folders.iter().filter(|f| !dir.path().join(f).is_dir()).count();
    assert_eq!(work.folders, missing as u64);
    assert_eq!((work.files, work.bytes), (keys.len() as u64, keys.len() as u64));
}

/// Read back, a kept result is the one saved, and damage never panics.
#[hegel::test(test_cases = 50)]
fn a_record_reads_back_or_counts_as_none(tc: TestCase) {
    let (dir, device) = drive();
    let speed = any_speed(&tc);
    keep(dir.path(), &speed);
    assert_eq!(stored(&device), Some(speed));
    let junk = tc.draw(gs::vecs(gs::integers::<u8>()).max_size(64));
    std::fs::write(record_path(dir.path()), junk).unwrap();
    let _ = stored(&device);
}

/// What a drive holds before a plan: nothing, a result that holds, one gone stale, or a record that
/// can't be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Before {
    Nothing,
    Fresh,
    Stale,
    Damaged,
}

/// A plan's estimate uses what's stored when it holds, tests the drive only with leave to and when
/// it doesn't, says when it did, and a plan that isn't applied leaves a drive that had no
/// `.locallibrary/` as it found it.
#[hegel::test(test_cases = 12)]
fn an_estimate_tests_only_when_it_must_and_may(tc: TestCase) {
    let (dir, device) = drive();
    let state = dir.path().join(crate::paths::STATE_DIR);
    let before = tc.draw(gs::sampled_from(vec![Before::Nothing, Before::Fresh, Before::Stale, Before::Damaged]));
    let used = before != Before::Nothing || tc.draw(gs::booleans());
    if used {
        std::fs::create_dir(&state).unwrap();
        std::fs::write(state.join("state.json"), b"{}").unwrap();
    }
    let kept = DriveSpeed {
        measured_at: Utc::now() - TimeDelta::days(if before == Before::Stale { 400 } else { 1 }),
        ..speed(100 * MB, 1_000, 1_000)
    };
    match before {
        Before::Fresh | Before::Stale => save(dir.path(), &kept).unwrap(),
        Before::Damaged => std::fs::write(record_path(dir.path()), b"{damaged").unwrap(),
        Before::Nothing => {}
    }
    let may_measure = tc.draw(gs::booleans());
    let cancelled = tc.draw(gs::weighted_booleans(0.15));
    let cancel = CancelToken::new();
    if cancelled {
        cancel.cancel();
    }
    let progress = crate::progress::NoProgress;
    let ctx = Ctx::new(&progress, cancel);
    let must_test = before != Before::Fresh;
    let made = match estimate(&device, work(64 << 20, 300, 3), &now(), may_measure, &ctx) {
        Err(crate::Error::Cancelled) => {
            assert!(cancelled && may_measure && must_test);
            assert_eq!(state.exists(), used, "a cancelled test leaves the drive as it was");
            return;
        }
        made => made.unwrap().unwrap(),
    };
    let tested = may_measure && must_test;
    assert_eq!(made.ran_test, tested);
    assert_eq!(made.lines().iter().any(|l| l.contains("ran a short test")), tested);
    match (tested, before) {
        (true, _) => assert_eq!(made.speed.as_ref().map(|s| s.measured_by), Some(MeasuredBy::Test)),
        (false, Before::Fresh | Before::Stale) => assert_eq!(made.speed.as_ref(), Some(&kept)),
        (false, _) => assert_eq!(made.speed, None),
    }
    assert_eq!(made.stale.is_some(), !tested && before == Before::Stale);
    if !may_measure {
        assert_eq!(state.exists(), used, "without leave to test, nothing is written");
    }
    let applied = tc.draw(gs::booleans());
    if applied {
        made.keep();
    }
    drop(made);
    match (tested, used, applied) {
        (true, false, false) => assert!(!state.exists(), "a blank drive is left as it was"),
        (true, _, _) => assert!(stored(&device).is_some()),
        _ => {}
    }
    if used {
        assert!(state.join("state.json").exists());
    }
}

/// A result is stale on another port, after a capacity's worth of writes, or after 90 days, in that
/// order.
#[hegel::test]
fn staleness_is_ports_then_writes_then_age(tc: TestCase) {
    let links = || gs::optional(gs::sampled_from(vec![USB2, USB3]));
    let days = tc.draw(gs::integers::<i64>().min_value(-30).max_value(400));
    let then_logged = tc.draw(gs::optional(gs::integers::<u64>().max_value(1 << 40)));
    let result = DriveSpeed {
        measured_at: Utc::now() - TimeDelta::days(days),
        link_bps: tc.draw(links()),
        logged_bytes: then_logged,
        ..speed(100 * MB, 1_000, 1_000)
    };
    let facts = DriveNow {
        at: Utc::now(),
        logged_bytes: tc.draw(gs::optional(gs::integers::<u64>().max_value(1 << 41))),
        capacity: tc.draw(gs::optional(gs::integers::<u64>().min_value(1).max_value(1 << 40))),
        link_bps: tc.draw(links()),
    };
    let written = facts.logged_bytes.unwrap_or(0).saturating_sub(then_logged.unwrap_or(0));
    let want = match (result.link_bps, facts.link_bps) {
        (Some(then), Some(now)) if then != now => Some(Stale::Moved { then, now }),
        _ if facts.capacity.is_some_and(|c| written >= c) => Some(Stale::Worn { bytes: written }),
        _ if days > MAX_AGE_DAYS => Some(Stale::Old { days }),
        _ => None,
    };
    assert_eq!(result.stale(&facts), want);
}

/// What a plan says carries each thing the estimate knows: the time, what else it waits on, why the
/// result may be out of date, the test and the port.
#[hegel::test]
fn an_estimates_lines_say_what_it_knows(tc: TestCase) {
    let mut job = any_work(&tc);
    if tc.draw(gs::booleans()) {
        job = job.plus("downloads");
    }
    let stale = tc.draw(gs::optional(gs::sampled_from(vec![
        Stale::Old { days: 120 },
        Stale::Worn { bytes: 320 * GB },
        Stale::Moved { then: USB3, now: USB2 },
    ])));
    let link = tc.draw(gs::optional(gs::sampled_from(vec![12 * MB, USB2, USB3, 10 * GB])));
    let measured = tc.draw(gs::booleans()).then(|| any_speed(&tc));
    let mut made = WriteEstimate::new(job.clone(), measured.clone(), stale, link);
    made.ran_test = tc.draw(gs::booleans());
    let lines = made.lines();
    let Some(_) = measured else {
        assert_eq!(lines, ["time unknown: this drive hasn't been measured"]);
        return;
    };
    let first = &lines[0];
    assert!(
        ["about ", "under a minute", "at least ", "a minute or more"].iter().any(|s| first.starts_with(s)),
        "{first}"
    );
    assert_eq!(first.contains(", plus downloads"), job.plus.is_some());
    assert_eq!(first.contains("measured "), stale.is_some(), "{first}");
    assert_eq!(lines.iter().any(|l| l.contains("ran a short test")), made.ran_test);
    assert_eq!(lines.iter().any(|l| l.starts_with("this port")), made.port_limit_bps.is_some());
    assert_eq!(lines.len(), 1 + usize::from(made.ran_test) + usize::from(made.port_limit_bps.is_some()));
}

/// A real write replaces the stored rate only when it's big, from this computer, measured against a
/// result, and slower than its files alone; one that can't be kept is said, not an error.
#[hegel::test(test_cases = 50)]
fn a_real_write_is_learned_from_only_when_it_tells(tc: TestCase) {
    let (dir, device) = drive();
    let unwritable = tc.draw(gs::weighted_booleans(0.2));
    if unwritable {
        std::fs::write(dir.path().join(crate::paths::STATE_DIR), b"a file, not a folder").unwrap();
    } else {
        std::fs::create_dir(dir.path().join(crate::paths::STATE_DIR)).unwrap();
    }
    let bytes = tc.draw(gs::integers::<u64>().min_value(MIN_LEARNED_BYTES / 2).max_value(64 * GB));
    let mut job = work(bytes, tc.draw(gs::integers::<u64>().max_value(20_000)), 0);
    if tc.draw(gs::weighted_booleans(0.2)) {
        job = job.read_elsewhere();
    }
    let measured = tc.draw(gs::weighted_booleans(0.85)).then(|| any_speed(&tc));
    let made = WriteEstimate::new(job.clone(), measured.clone(), None, None);
    let elapsed = Duration::from_millis(tc.draw(gs::integers::<u64>().min_value(1).max_value(10_000_000)));
    let warnings = Warnings::default();
    let ctx = Ctx::new(&warnings, CancelToken::new());
    learn(dir.path(), &made, elapsed, &now(), &ctx);
    let tells = measured.as_ref().is_some_and(|s| {
        made.drive_alone() && bytes >= MIN_LEARNED_BYTES && elapsed.as_secs_f64() > job.small_files_secs(s)
    });
    assert_eq!(warnings.said("couldn't keep"), tells && unwritable);
    let learned = (!unwritable).then(|| stored(&device)).flatten();
    match learned {
        Some(l) => {
            assert!(tells);
            assert_eq!(l.measured_by, MeasuredBy::Write);
        }
        None => assert!(!tells || unwritable || speed_rounds_to_nothing(bytes, elapsed, &job, measured.as_ref())),
    }
}

/// A rate under one byte a second reads back as none: it can't be divided by.
fn speed_rounds_to_nothing(bytes: u64, elapsed: Duration, job: &Work, measured: Option<&DriveSpeed>) -> bool {
    let small = measured.map_or(0.0, |s| job.small_files_secs(s));
    (bytes as f64 / (elapsed.as_secs_f64() - small)) < 1.0
}

/// A plan that tested a blank drive and wasn't applied takes away what the test left, and only
/// that: anything else that arrived in the folder meanwhile stays, and so does the folder then.
#[hegel::test(test_cases = 8)]
fn a_blank_drive_gets_back_to_blank_unless_something_else_arrived(tc: TestCase) {
    let (dir, device) = drive();
    let state = dir.path().join(crate::paths::STATE_DIR);
    let files = tc.draw(gs::integers::<u64>().min_value(256).max_value(20_000));
    let made = estimate(&device, work(64 << 20, files, 3), &now(), true, &Ctx::silent()).unwrap().unwrap();
    assert!(made.ran_test && state.join(RECORD_FILE).exists());
    let arrived = tc.draw(gs::booleans());
    if arrived {
        std::fs::write(state.join("state.json"), b"{}").unwrap();
    }
    drop(made);
    assert!(!state.join(RECORD_FILE).exists());
    assert_eq!(state.exists(), arrived);
}
