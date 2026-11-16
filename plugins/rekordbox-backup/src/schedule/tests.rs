use std::fs::File;
use std::time::Instant;

use hegel::TestCase;
use hegel::generators as gs;
use loclib_plugin::method;
use serde_json::json;

use super::*;
use crate::tests::{fixture, host, names, stamped};

/// `name` in `dir`, last changed `days` ago.
fn old(dir: &Path, name: &str, days: u64) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, name).unwrap();
    let when = SystemTime::now() - Duration::from_secs(days * 86_400);
    File::options().write(true).open(&path).unwrap().set_modified(when).unwrap();
    path
}

/// The oldest of its own go beyond `keep`; one changed since, and those it didn't make, stay.
#[test]
fn rotation_removes_only_its_own_oldest() {
    let dir = tempfile::tempdir().unwrap();
    let (data, dest) = (dir.path().join("data"), dir.path().join("dest"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    let theirs = old(&dest, "rekordbox_bak_20250101.zip", 40);
    let made: Vec<PathBuf> = (1..=4).map(|d| old(&dest, &format!("rekordbox_bak_2026010{d}.zip"), 10 - d)).collect();
    for zip in &made {
        rotate(&data, zip, None).unwrap();
    }
    assert!(made.iter().all(|z| z.exists()), "kept, with no limit");
    // The second was replaced since (the user made one that day): it isn't its own any more.
    std::fs::write(&made[1], "replaced").unwrap();
    let newest = old(&dest, "rekordbox_bak_20260110.zip", 0);
    rotate(&data, &newest, Some(2)).unwrap();
    let left: Vec<bool> = made.iter().map(|z| z.exists()).collect();
    assert_eq!(left, [false, true, false, true]);
    assert!(theirs.exists() && newest.exists());
    let list: Vec<Kept> = serde_json::from_slice(&std::fs::read(data.join(MADE)).unwrap()).unwrap();
    let listed: Vec<&Path> = list.iter().map(|k| k.path.as_path()).collect();
    assert_eq!(listed, [made[3].as_path(), newest.as_path()]);
}

/// What `rotate` listed in `data`.
fn listed(data: &Path) -> Vec<PathBuf> {
    let list: Vec<Kept> = serde_json::from_slice(&std::fs::read(data.join(MADE)).unwrap()).unwrap();
    list.into_iter().map(|k| k.path).collect()
}

/// However many backups it made and whatever it keeps, what's left of them is the newest that
/// many (or all), and the list names just those.
#[hegel::test(test_cases = 30)]
fn prop_rotation_keeps_its_newest(tc: TestCase) {
    let n = tc.draw(gs::integers::<u64>().min_value(1).max_value(6));
    let keep = tc.draw(gs::optional(gs::integers::<usize>().min_value(1).max_value(6)));
    let dir = tempfile::tempdir().unwrap();
    let (data, dest) = (dir.path().join("data"), dir.path().join("dest"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    let made: Vec<PathBuf> = (1..=n).map(|d| old(&dest, &format!("rekordbox_bak_202601{d:02}.zip"), n - d)).collect();
    for zip in &made {
        assert!(rotate(&data, zip, keep).unwrap().is_empty());
    }
    let kept = keep.map_or(made.len(), |k| k.min(made.len()));
    let newest = &made[made.len() - kept..];
    let left: Vec<&PathBuf> = made.iter().filter(|z| z.exists()).collect();
    assert_eq!(left, newest.iter().collect::<Vec<_>>());
    assert_eq!(listed(&data), newest);
}

/// One of its own it can't remove (its folder made read-only) stays listed and is said; the next
/// rotation that can removes it.
#[cfg(unix)]
#[test]
fn a_backup_it_cant_remove_stays_listed() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let (data, dest) = (dir.path().join("data"), dir.path().join("dest"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    let first = old(&dest, "rekordbox_bak_20260101.zip", 3);
    rotate(&data, &first, None).unwrap();
    let second = old(&dest, "rekordbox_bak_20260102.zip", 2);
    std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o555)).unwrap();
    let said = rotate(&data, &second, Some(1));
    std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755)).unwrap();
    let said = said.unwrap();
    let [why] = &said[..] else { panic!("{said:?}") };
    assert!(why.starts_with(&format!("couldn't remove the old backup {}: ", first.display())), "{why}");
    assert!(first.exists() && second.exists());
    assert_eq!(listed(&data), [first.clone(), second.clone()]);
    let third = old(&dest, "rekordbox_bak_20260103.zip", 1);
    assert!(rotate(&data, &third, Some(1)).unwrap().is_empty());
    assert_eq!((first.exists(), second.exists(), third.exists()), (false, false, true));
}

/// With its data folder gone, the list can't be written: the rotation fails naming it, and
/// removes nothing.
#[test]
fn a_list_it_cant_write_fails_the_rotation() {
    let dir = tempfile::tempdir().unwrap();
    let zip = old(dir.path(), "rekordbox_bak_20260101.zip", 1);
    let gone = dir.path().join("gone");
    let e = rotate(&gone, &zip, Some(1)).unwrap_err();
    assert_eq!(e.kind(), Some("io"), "{e:?}");
    assert!(e.message.starts_with(&gone.display().to_string()), "{e:?}");
    assert!(zip.exists());
}

/// A schedule that can't back up (here, run by a host that gives it no scratch folder) tells the
/// user why, and makes nothing.
#[test]
fn a_schedule_that_cant_back_up_says_why() {
    stamped();
    let f = fixture();
    let host = host(&f);
    let settings = json!({ "destination": f.dest, "every_days": 1 });
    let req = json!({ "service": "schedule", "context": { "settings": settings, "scratch": null } });
    let running = host.send(method::SERVICE_START, &req);
    let until = Instant::now() + Duration::from_secs(30);
    while host.notices().is_empty() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    running.cancel();
    assert_eq!(running.wait::<()>().unwrap_err().kind(), Some("cancelled"));
    let notices: Vec<String> = host.notices().into_iter().map(|n| n.message).collect();
    assert_eq!(notices, ["Couldn't back up rekordbox's library: no scratch folder"]);
    assert!(list::find(&f.dest).is_empty());
    host.stop();
}

/// Started with a backup folder whose newest is older than `every_days`, the service makes one
/// without music, tells the user, and removes the oldest of its own beyond `keep`. It then waits
/// until cancelled.
#[test]
fn the_schedule_backs_up_when_the_newest_is_old() {
    stamped();
    let f = fixture();
    let host = host(&f);
    let data = f.path("data");
    std::fs::create_dir(&data).unwrap();
    let mine = old(&f.dest, "rekordbox_bak_20260901.zip", 3);
    rotate(&data, &mine, None).unwrap();
    let theirs = old(&f.dest, "rekordbox_bak_20260801.zip", 30);
    let settings = json!({ "destination": f.dest, "music": true, "every_days": 2, "keep": 1 });
    let req = json!({ "service": "schedule", "context": { "settings": settings, "data_dir": data } });
    let running = host.send(method::SERVICE_START, &req);
    let until = Instant::now() + Duration::from_secs(30);
    while host.notices().is_empty() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    running.cancel();
    assert_eq!(running.wait::<()>().unwrap_err().kind(), Some("cancelled"));
    let notices = host.notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0].message, format!("Backed up rekordbox's library to {}", f.zip().display()));
    assert!(names(&f.zip()).contains(&"contents.xml".to_string()));
    assert!(!f.dest.join("rekordbox_bak").exists(), "no music, whatever the setting");
    assert!(!mine.exists() && theirs.exists());
    // Its copy of master.db is gone from the scratch folder it keeps while it runs.
    assert!(running.scratch().unwrap().read_dir().unwrap().next().is_none());
    host.stop();
}

/// A backup is due once the newest in the folder is as old as the setting says, or there's none;
/// one dated ahead of the clock isn't old.
#[test]
fn a_backup_is_due_once_the_newest_is_old() {
    let dir = tempfile::tempdir().unwrap();
    let days = |n: u64| Duration::from_secs(n * 86_400);
    assert!(is_due(dir.path(), days(2)));
    old(dir.path(), "rekordbox_bak_20260101.zip", 3);
    assert!(is_due(dir.path(), days(2)));
    let newer = old(dir.path(), "rekordbox_bak_20260102.zip", 1);
    assert!(!is_due(dir.path(), days(2)));
    File::options().write(true).open(&newer).unwrap().set_modified(SystemTime::now() + days(9)).unwrap();
    assert!(!is_due(dir.path(), days(2)));
    // Not a backup: it doesn't count.
    std::fs::remove_file(&newer).unwrap();
    old(dir.path(), "notes.zip", 0);
    assert!(is_due(dir.path(), days(2)));
    // Today's there already, however old its file says it is: it isn't replaced.
    let today = backup::zip_in(dir.path());
    old(dir.path(), &today.file_name().unwrap().to_string_lossy(), 3);
    assert!(!is_due(dir.path(), days(2)));
}

#[test]
fn every_and_keep_read_as_the_settings_say() {
    let settings = |every_days: f64, keep: f64| Settings { every_days, keep, ..Settings::default() };
    assert_eq!(settings(2.0, 0.0).every(), Some(Duration::from_secs(2 * 86_400)));
    assert_eq!(settings(0.5, 0.0).every(), Some(Duration::from_secs(43_200)));
    for off in [0.0, -1.0, f64::NAN] {
        assert_eq!(settings(off, 0.0).every(), None, "{off}");
    }
    assert_eq!(settings(0.0, 3.7).keep(), Some(3));
    for all in [0.0, 0.5, -2.0, f64::NAN] {
        assert_eq!(settings(0.0, all).keep(), None, "{all}");
    }
}
