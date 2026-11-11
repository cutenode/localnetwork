use std::time::Duration;

use loclib_progress::Ctx;

use super::*;
use crate::entries::{Body, Entry};
use crate::write;

/// A backup `name` in `dir` whose `contents.xml` holds `values`, made `age` ago.
fn backup(dir: &Path, name: &str, values: &[(&str, &str)], age: u64) -> PathBuf {
    let values: Vec<(String, String)> = values.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect();
    let path = dir.join(name);
    let made = Entry { name: "contents.xml".into(), body: Body::Made(contents::write(&values)) };
    write::write(&[made], &path, &Ctx::silent()).unwrap();
    let when = SystemTime::now() - Duration::from_secs(age);
    File::options().write(true).open(&path).unwrap().set_modified(when).unwrap();
    path
}

#[test]
fn backups_are_found_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let old = backup(dir.path(), "rekordbox_bak_20250101.zip", &[], 300);
    let new = backup(dir.path(), "rekordbox_bak_20260101.zip", &[], 100);
    let renamed = backup(dir.path(), "rekordbox_bak_mine.zip", &[], 200);
    backup(dir.path(), "other.zip", &[], 0);
    std::fs::create_dir(dir.path().join("rekordbox_bak_folder.zip")).unwrap();
    let found: Vec<PathBuf> = find(dir.path()).into_iter().map(|f| f.path).collect();
    assert_eq!(found, [new, renamed, old]);
    let found = find(dir.path());
    assert_eq!(found[0].date(), "2026-01-01");
    // Named otherwise, it's dated by its file.
    let local = chrono::DateTime::<Local>::from(found[1].modified.unwrap()).format("%Y-%m-%d").to_string();
    assert_eq!(found[1].date(), local);
    assert!(find(&dir.path().join("none")).is_empty());
}

/// The numbers an earlier backup of the same library has for the roots, from the newest that
/// has any; another library's are never taken.
#[test]
fn root_numbers_come_from_an_earlier_backup_of_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let roots = ["/Users/dj/Music".to_string(), "/Users/dj".to_string()];
    assert!(earlier_roots(dir.path(), "1", &roots).is_empty());
    backup(dir.path(), "rekordbox_bak_1.zip", &[("/Users/dj/Music", "11"), ("/Users/dj", "12"), ("DB_ID", "1")], 300);
    backup(dir.path(), "rekordbox_bak_2.zip", &[("/Users/dj/Music", "21"), ("DB_ID", "1")], 200);
    backup(dir.path(), "rekordbox_bak_3.zip", &[("/Users/dj/Music", "31"), ("DB_ID", "2")], 100);
    backup(dir.path(), "rekordbox_bak_4.zip", &[("DB_ID", "1")], 50);
    std::fs::write(dir.path().join("rekordbox_bak_5.zip"), b"not a zip").unwrap();
    assert_eq!(earlier_roots(dir.path(), "1", &roots), [("/Users/dj/Music".to_string(), "21".to_string())]);
    assert_eq!(earlier_roots(dir.path(), "2", &roots[..1]), [("/Users/dj/Music".to_string(), "31".to_string())]);
    assert!(earlier_roots(dir.path(), "3", &roots).is_empty());
}

#[test]
fn contents_xml_is_read_without_unpacking() {
    let dir = tempfile::tempdir().unwrap();
    let path =
        backup(dir.path(), "rekordbox_bak_1.zip", &[("CONTENT_NUM", "7"), ("026EE51D", "/a/b/026EE51D_x.mp3")], 0);
    let values = read_contents(&path).unwrap();
    assert_eq!(value(&values, "CONTENT_NUM"), Some("7"));
    assert_eq!(value(&values, "DB_ID"), None);
    assert_eq!(values.iter().filter(|(n, _)| is_track(n)).count(), 1);
    let no_contents = dir.path().join("rekordbox_bak_2.zip");
    let other = Entry { name: "master.db".into(), body: Body::Made(b"db".to_vec()) };
    write::write(&[other], &no_contents, &Ctx::silent()).unwrap();
    assert_eq!(read_contents(&no_contents).unwrap_err(), "it has no contents.xml");
}
