use std::fs::File;
use std::time::Instant;

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
