use hegel::TestCase;
use hegel::generators as gs;
use loclib_drive_speed::test_util::{any_speed, any_work, speed, work};

use super::words::{duration, link_name};
use super::*;
use crate::{Error, paths};

const USB2: u64 = 480_000_000;
const USB3: u64 = 5_000_000_000;
const MB: u64 = 1_000_000;
const GB: u64 = 1_000_000_000;

fn estimate_of(work: Work, speed: DriveSpeed) -> WriteEstimate {
    WriteEstimate::new(work, Some(speed), None, None)
}

#[test]
fn a_plan_says_the_time_and_what_it_comes_from() {
    let stick = estimate_of(work(28_942_463_040, 15_000, 3_000), speed(151 * MB, 12_000, 14_000));
    assert_eq!(estimate_lines(&stick), ["about 7 min, mostly making files (drive writes ~151 MB/s, 12 ms a file)"]);
    let card = estimate_of(work(28_942_463_040, 15_000, 3_000), speed(16 * MB, 6_000, 43_000));
    assert_eq!(estimate_lines(&card), ["about 34 min (drive writes ~16 MB/s, 6 ms a file)"]);
    let unmeasured = WriteEstimate::new(work(GB, 1, 0), None, None, None);
    assert_eq!(estimate_lines(&unmeasured), ["time unknown: this drive hasn't been measured"]);
    assert_eq!(unmeasured.seconds, None);
}

#[test]
fn what_else_the_time_depends_on_is_said() {
    let downloads = estimate_of(work(GB, 1, 0).plus("downloads"), speed(100 * MB, 1_000, 1_000));
    assert_eq!(estimate_lines(&downloads)[0], "under a minute, plus downloads (drive writes ~100 MB/s, 1 ms a file)");
}

/// A write longer than the rate held for says the least it could take.
#[test]
fn a_best_case_says_at_least() {
    let flat = DriveSpeed { holds_for: Some(2 * GB), ..speed(100 * MB, 1_000, 1_000) };
    let longer = estimate_lines(&estimate_of(work(30 * GB, 1, 0), flat.clone()));
    assert!(longer[0].starts_with("at least 5 min"), "{longer:?}");
    let quick = estimate_lines(&estimate_of(work(3 * GB, 1, 0), flat));
    assert!(quick[0].starts_with("a minute or more"), "{quick:?}");
}

/// A port near the drive's rate gets a line of its own.
#[test]
fn a_port_near_the_rate_is_a_hint() {
    let job = || work(30 * GB, 3_000, 300);
    let slow_port = WriteEstimate::new(job(), Some(speed(38 * MB, 12_000, 14_000)), None, Some(USB2));
    assert_eq!(estimate_lines(&slow_port)[1], "this port (USB 2.0) tops out near 42 MB/s, so a faster one may help");
    let fast_port = WriteEstimate::new(job(), Some(speed(38 * MB, 12_000, 14_000)), None, Some(USB3));
    assert_eq!(estimate_lines(&fast_port).len(), 1);
}

#[test]
fn a_stale_result_says_why() {
    let line = |stale| WriteEstimate::new(work(GB, 1, 0), Some(speed(100 * MB, 1_000, 1_000)), Some(stale), None);
    let said = |stale| estimate_lines(&line(stale))[0].clone();
    assert!(said(Stale::Old { days: 120 }).contains("measured 120 days ago"));
    assert!(said(Stale::Worn { bytes: 320 * GB }).contains("measured before 320.0 GB of writes"));
    assert!(said(Stale::Moved { then: USB3, now: USB2 }).contains("measured on a USB 3, 5 Gb/s port"));
}

/// A plan says it ran a test, and one from what's stored doesn't.
#[test]
fn a_plan_says_when_it_ran_a_test() {
    let mut tested = estimate_of(work(64 << 20, 300, 3), speed(100 * MB, 1_000, 1_000));
    assert!(!estimate_lines(&tested).iter().any(|l| l.contains("ran a short test")));
    tested.ran_test = true;
    assert!(estimate_lines(&tested).iter().any(|l| l.contains("ran a short test")));
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

/// The port changes what's said beside the number, never the number.
#[hegel::test]
fn prop_the_port_never_changes_the_time(tc: TestCase) {
    let (speed, job) = (any_speed(&tc), any_work(&tc));
    let link = tc.draw(gs::sampled_from(vec![12 * MB, USB2, USB3, 10 * GB, 20 * GB]));
    let with_port = WriteEstimate::new(job.clone(), Some(speed.clone()), None, Some(link));
    let without = estimate_of(job, speed);
    let (said, plain) = (estimate_lines(&with_port), estimate_lines(&without));
    assert_eq!(said.first(), plain.first());
    assert_eq!(said.len(), 1 + usize::from(with_port.port_limit_bps.is_some()));
}

/// A best case, and only a best case, says the least it could take.
#[hegel::test]
fn prop_a_best_case_is_said_as_one(tc: TestCase) {
    let made = estimate_of(any_work(&tc), any_speed(&tc));
    let said = &estimate_lines(&made)[0];
    assert_eq!(made.best_case, said.starts_with("at least") || said.starts_with("a minute or more"));
}

/// What a plan says carries each thing the estimate knows: the time, what else it waits on, why the
/// result may be out of date, the test and the port.
#[hegel::test]
fn prop_an_estimates_lines_say_what_it_knows(tc: TestCase) {
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
    let lines = estimate_lines(&made);
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

/// A drive that can't be locked fails a measure with the drive's own error, not one about speed.
#[test]
fn a_drive_that_cant_be_measured_fails_with_its_own_error() {
    let dir = tempfile::tempdir().unwrap();
    let device = Device::open(dir.path()).unwrap();
    std::fs::write(dir.path().join(paths::STATE_DIR), b"a file, not a folder").unwrap();
    let facts = DriveNow { at: Utc::now(), logged_bytes: None, capacity: None, link_bps: None };
    let err = Error::from(drive_speed::measure(&device, 64 << 20, &facts, &Ctx::silent()).unwrap_err());
    assert!(matches!(&err, Error::Io { path, .. } if path.ends_with(paths::STATE_DIR)), "{err:?}");
}

/// The test's small files are made on the drive, and only those it names.
#[test]
fn a_drives_small_files_are_the_ones_named() {
    let dir = tempfile::tempdir().unwrap();
    let device = Device::open(dir.path()).unwrap();
    let path = |i: usize| i.is_multiple_of(2).then(|| dir.path().join(format!("{i}.DAT")));
    let made = device.small_files(5, &path, b"data").unwrap();
    assert_eq!(made.len(), 3);
    assert!(made.iter().all(|p| std::fs::read(p).unwrap() == b"data"));
    assert!(!dir.path().join("1.DAT").exists());
}

/// Keeps a test's warnings.
#[derive(Default)]
struct Warnings(std::sync::Mutex<Vec<String>>);

impl crate::progress::Progress for Warnings {
    fn on_event(&self, event: &crate::progress::Event) {
        if let crate::progress::Event::Warning { message } = event {
            self.0.lock().unwrap().push(message.clone());
        }
    }
}

/// A drive that can't be tested still gets an estimate from what's known, and the warning names
/// the drive by its own name.
#[hegel::test(test_cases = 10)]
fn prop_a_drive_that_cant_be_tested_is_named_in_the_warning(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let name = tc.draw(gs::sampled_from(vec!["DNA", "Club Stick", "Ünï"]));
    let mut device = Device::open(dir.path()).unwrap();
    device.adopt(Some(name)).unwrap();
    // Its lock can't be taken: the state folder is a file now.
    let state = dir.path().join(paths::STATE_DIR);
    std::fs::remove_dir_all(&state).unwrap();
    std::fs::write(&state, b"a file, not a folder").unwrap();
    let warnings = Warnings::default();
    let ctx = Ctx::new(&warnings, Default::default());
    let facts = DriveNow { at: Utc::now(), logged_bytes: None, capacity: None, link_bps: None };
    let made = drive_speed::estimate(&device, work(GB, 300, 3), &facts, true, &ctx).unwrap().unwrap();
    assert_eq!(made.speed, None);
    let said = warnings.0.lock().unwrap().clone();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].starts_with(&format!("couldn't measure {name}'s speed: ")), "{said:?}");
}

/// A test's small files that can't all be made fail with what the drive's write said, as core's
/// own error: a file whose folder is a file fails, and so the whole batch does.
#[hegel::test(test_cases = 20)]
fn prop_a_small_file_that_cant_be_made_fails_the_batch(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let device = Device::open(dir.path()).unwrap();
    let count = tc.draw(gs::integers::<usize>().min_value(1).max_value(8));
    let blocked = tc.draw(gs::integers::<usize>().max_value(count - 1));
    std::fs::write(dir.path().join("FILE"), b"").unwrap();
    let path = |i: usize| {
        let folder = if i == blocked { dir.path().join("FILE") } else { dir.path().to_path_buf() };
        Some(folder.join(format!("{i}.DAT")))
    };
    let err = Error::from(drive_speed::Error::Drive(device.small_files(count, &path, b"data").unwrap_err()));
    assert!(matches!(&err, Error::Io { path, .. } if path.starts_with(dir.path().join("FILE"))), "{err:?}");
}
