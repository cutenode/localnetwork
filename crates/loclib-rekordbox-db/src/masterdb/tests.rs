use std::time::Duration;

use rusqlite::params;

use super::*;
use crate::onelibrary::cipher::{Codec, PAGE_SIZE, RESERVE, SALT_LEN, SQLITE_MAGIC};
use crate::onelibrary::kind_for_hot_cue;
use crate::test_util::{Fixture, STATS, TABLES, codec, sample, scratch_db, with_broken_table};

thread_local! {
    /// rekordbox's places for [`places`] to give instead of this computer's, once set.
    pub(super) static PLACES: std::cell::RefCell<Option<Vec<Place>>> = const { std::cell::RefCell::new(None) };
}

/// With no temporary folder (`TMPDIR` on a drive that's gone), a `master.db` is still read and
/// its tracks listed, as both decrypt it in memory; copying it, which needs a file, fails naming
/// that folder, not the database.
#[test]
fn without_a_temporary_folder_a_master_db_is_read_but_not_copied() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), &crate::test_util::library().encrypted(DEFAULT_KEY));
    let gone = dir.path().join("unplugged");
    crate::scratch::test_root(&gone);
    assert_eq!(open(&path, DEFAULT_KEY).unwrap().library.tracks.len(), 3);
    #[cfg(feature = "files")]
    {
        assert_eq!(files::tracks(&path, DEFAULT_KEY, 0, 10).unwrap().total, 4);
        let e = files::export(&path, DEFAULT_KEY, false, &dir.path().join("copy.db")).unwrap_err();
        assert!(matches!(&e, Error::Io { path, .. } if *path == gone), "{e:?}");
    }
}

/// What the child [`a_killed_read_leaves_nothing_decrypted_behind`] starts reads: the
/// `master.db` to read, and `read` or `export`.
const KILLED: &str = "LOCALLIBRARY_TEST_KILLED_READ";

/// A read of `master.db` (and an export of it, which goes through a private folder) killed at
/// some point while it runs leaves nothing in the temporary folder: the read decrypts in memory,
/// and the export's folder is swept at the next start, as nobody holds its lock any more.
#[test]
fn a_killed_read_leaves_nothing_decrypted_behind() {
    use std::io::BufRead;
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), &crate::test_util::library().encrypted(DEFAULT_KEY));
    let modes: &[&str] = if cfg!(feature = "files") { &["read", "export"] } else { &["read"] };
    for (i, mode) in modes.iter().enumerate() {
        let tmp = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["masterdb::tests::killed_read", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
            .env(KILLED, format!("{mode}:{}", path.display()))
            .env("LOCALLIBRARY_MASTERDB", "no-rekordbox/master.db")
            .envs(["TMPDIR", "TMP", "TEMP"].map(|v| (v, tmp.path())))
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = std::io::BufReader::new(child.stdout.take().unwrap()).lines();
        // libtest names the test on the same line first.
        assert!(lines.any(|l| l.unwrap().ends_with("reading")), "the child never started reading");
        // Early in the second iteration, most of which is deriving the key (some 60 ms): this
        // catches what's written before that. What's written later is
        // `a_read_writes_nothing_to_the_temporary_folder`'s.
        std::thread::sleep(Duration::from_micros(300 + 700 * i as u64));
        child.kill().unwrap();
        child.wait().unwrap();
        let left = |tmp: &Path| -> Vec<String> {
            std::fs::read_dir(tmp).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()
        };
        if *mode == "read" {
            assert_eq!(left(tmp.path()), Vec::<String>::new(), "a read wrote to the temporary folder");
        } else {
            let found = left(tmp.path());
            assert!(found.iter().all(|n| n.starts_with(crate::scratch::PREFIX)), "{found:?}");
            crate::scratch::sweep_in(tmp.path(), Duration::from_secs(3600));
            // A folder killed in the moment before its lock was taken stays for an hour, but
            // holds nothing yet.
            for name in left(tmp.path()) {
                let inside = left(&tmp.path().join(&name));
                assert!(inside.iter().all(|n| n == ".lock-making"), "the sweep left {name} holding {inside:?}");
            }
        }
    }
}

/// The child [`a_killed_read_leaves_nothing_decrypted_behind`] kills: reads until it's killed.
#[test]
#[ignore = "started by a_killed_read_leaves_nothing_decrypted_behind"]
fn killed_read() {
    let Ok(what) = std::env::var(KILLED) else { return };
    // Its own temporary folder, the one the test looks in.
    crate::scratch::test_root(std::env::temp_dir());
    let (mode, path) = what.split_once(':').unwrap();
    let path = Path::new(path);
    for n in 0.. {
        match mode {
            "read" => drop(open(path, DEFAULT_KEY).unwrap()),
            #[cfg(feature = "files")]
            _ => {
                let into = path.with_file_name("copy.db");
                files::export(path, DEFAULT_KEY, false, &into).unwrap();
                std::fs::remove_file(&into).unwrap();
            }
            #[cfg(not(feature = "files"))]
            _ => unreachable!(),
        }
        if n == 0 {
            println!("reading");
        }
    }
}

/// A decrypted `master.db` more than SQLite takes into memory is read from a file in a private
/// folder instead, read only, and reads as it does in memory.
#[test]
fn a_master_db_too_big_for_memory_is_read_from_a_private_folder() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), &sample().encrypted(DEFAULT_KEY));
    let len = std::fs::metadata(&path).unwrap().len() as usize;
    let in_memory = open_db_within(&path, DEFAULT_KEY, len).unwrap();
    assert!(in_memory._folder.is_none());
    let from_file = open_db_within(&path, DEFAULT_KEY, len - 1).unwrap();
    let folder = from_file._folder.as_ref().expect("read from a file");
    assert!(folder.path().join("master.db").is_file());
    assert_eq!(from_file.collection().unwrap().library, in_memory.collection().unwrap().library);
    assert!(from_file.conn.execute_batch("CREATE TABLE notes (body TEXT)").is_err(), "read only");
    // A file under the limit whose WAL grows it past: it's read into memory, then written out.
    let grown = dir.path().join("grown");
    std::fs::create_dir(&grown).unwrap();
    let (f, codec) = (sample(), codec(DEFAULT_KEY));
    let before = f.plain();
    f.track("5", "/Users/dj/Music/New.mp3", "Newcomer", "Added Later");
    f.exec("CREATE TABLE filler (body BLOB); INSERT INTO filler VALUES (zeroblob(9000))");
    let after = f.plain();
    assert!(after.len() > before.len());
    let path = write(&grown, &codec.encrypt(&before).unwrap());
    let wal = loclib_sqlcipher::wal::tests::build(PAGE_SIZE, (1, 2), &frames(&codec, &before, &after));
    std::fs::write(grown.join("master.db-wal"), wal).unwrap();
    let from_memory = open_db_within(&path, DEFAULT_KEY, before.len()).unwrap();
    assert!(from_memory._folder.is_some(), "written to a file");
    assert_eq!(from_memory.collection().unwrap().library.track(5).unwrap().title, "Added Later");
}

/// Every connection to a decrypted copy keeps SQLite's temporary tables and sorts in memory
/// (2), never in files of their own: in memory, from a file, a copy and a plain `master.db`.
#[test]
fn decrypted_copies_keep_temporary_data_in_memory() {
    let dir = tempfile::tempdir().unwrap();
    let encrypted = write(dir.path(), &sample().encrypted(DEFAULT_KEY));
    let temp_store = |db: &Db| db.conn.query_row("PRAGMA temp_store", [], |r| r.get::<_, i64>(0)).unwrap();
    assert_eq!(temp_store(&open_db(&encrypted, DEFAULT_KEY).unwrap()), 2);
    assert_eq!(temp_store(&open_db_within(&encrypted, DEFAULT_KEY, 0).unwrap()), 2);
    let folder = crate::scratch::folder().unwrap();
    assert_eq!(temp_store(&open_copy(&encrypted, DEFAULT_KEY, &folder, true).unwrap().0), 2);
    let plain = dir.path().join("plain");
    std::fs::create_dir(&plain).unwrap();
    let plain = write(&plain, &sample().plain());
    assert_eq!(temp_store(&open_db(&plain, DEFAULT_KEY).unwrap()), 2);
}

/// What SQLite copies a database in from is zeroed behind it, a page at a time, so only SQLite's
/// copy is ever whole.
#[test]
fn a_database_read_into_sqlite_is_zeroed_behind_it() {
    let mut bytes: Vec<u8> = (0..3 * PAGE_SIZE + 5).map(|i| (i % 251) as u8 + 1).collect();
    let want = bytes.clone();
    let mut reader = Moving(&mut bytes);
    let mut page = vec![0; 2 * PAGE_SIZE];
    assert_eq!(reader.read(&mut page).unwrap(), PAGE_SIZE, "a page at a time");
    let mut got = page[..PAGE_SIZE].to_vec();
    reader.read_to_end(&mut got).unwrap();
    assert_eq!(got, want);
    assert!(bytes.iter().all(|&b| b == 0));
}

/// What the child [`a_read_writes_nothing_to_the_temporary_folder`] starts does: `read`,
/// `tracks` or `hash`, and the `master.db` to read.
const WATCHED: &str = "LOCALLIBRARY_TEST_WATCHED_READ";

/// Reads of `master.db` (and a OneLibrary's hash) write nothing to the temporary folder at any
/// moment, even what they'd remove again: a child process reads 20 times while a thread of its
/// own lists its temporary folder as fast as it can.
#[test]
fn a_read_writes_nothing_to_the_temporary_folder() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), &crate::test_util::library().encrypted(DEFAULT_KEY));
    let modes: &[&str] = if cfg!(feature = "files") { &["read", "hash", "tracks"] } else { &["read", "hash"] };
    for mode in modes {
        let tmp = tempfile::tempdir().unwrap();
        let out =
            child("masterdb::tests::watched_read", WATCHED, &format!("{mode}:{}", path.display()), tmp.path(), "seen");
        assert!(out.ends_with("seen nothing\n"), "{mode}: {out}");
    }
}

/// The child [`a_read_writes_nothing_to_the_temporary_folder`] starts.
#[test]
#[ignore = "started by a_read_writes_nothing_to_the_temporary_folder"]
fn watched_read() {
    let Ok(what) = std::env::var(WATCHED) else { return };
    let (mode, path) = what.split_once(':').unwrap();
    let path = Path::new(path);
    crate::scratch::test_root(std::env::temp_dir());
    let drive = crate::onelibrary::OneLibrary::create().unwrap().to_encrypted().unwrap();
    let stop = std::sync::atomic::AtomicBool::new(false);
    let seen = std::thread::scope(|s| {
        let watcher = s.spawn(|| {
            let mut seen = std::collections::BTreeSet::new();
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                seen.extend(std::fs::read_dir(std::env::temp_dir()).unwrap().map(|e| e.unwrap().file_name()));
            }
            seen
        });
        for _ in 0..20 {
            match mode {
                "read" => drop(open(path, DEFAULT_KEY).unwrap()),
                "hash" => drop(crate::onelibrary::content_hash(&drive, None).unwrap()),
                #[cfg(feature = "files")]
                _ => drop(files::tracks(path, DEFAULT_KEY, 0, 10).unwrap()),
                #[cfg(not(feature = "files"))]
                _ => unreachable!(),
            }
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        watcher.join().unwrap()
    });
    match seen.is_empty() {
        true => println!("seen nothing"),
        false => println!("seen {seen:?}"),
    }
}

/// The first read a process makes sweeps its temporary folder: an abandoned folder (a lock
/// nobody holds) is gone after one `open` in a process of its own.
#[test]
fn a_process_sweeps_its_temporary_folder_on_its_first_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), &crate::test_util::library().encrypted(DEFAULT_KEY));
    let tmp = tempfile::tempdir().unwrap();
    let left = tmp.path().join(format!("{}LEFT01", crate::scratch::PREFIX));
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut std::fs::DirBuilder::new(), 0o700).create(&left).unwrap();
    #[cfg(not(unix))]
    std::fs::create_dir(&left).unwrap();
    std::fs::write(left.join(".lock"), b"").unwrap();
    std::fs::write(left.join("export.db"), b"decrypted").unwrap();
    let out = child("masterdb::tests::watched_read", WATCHED, &format!("read:{}", path.display()), tmp.path(), "seen");
    assert!(out.contains("seen"), "{out}");
    assert!(!left.exists(), "the first read didn't sweep");
}

/// This test binary run as a child: `test` alone, with `var` set to `value` and its temporary
/// folder `tmp`. Answers the lines it printed that hold `marker`.
fn child(test: &str, var: &str, value: &str, tmp: &Path, marker: &str) -> String {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--ignored", "--nocapture", "--test-threads=1"])
        .env(var, value)
        .env("LOCALLIBRARY_MASTERDB", "no-rekordbox/master.db")
        .envs(["TMPDIR", "TMP", "TEMP"].map(|v| (v, tmp)))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    String::from_utf8(out.stdout).unwrap().lines().filter(|l| l.contains(marker)).map(|l| format!("{l}\n")).collect()
}

/// A plain `master.db`'s private folder lasts as long as the database read from it.
#[test]
fn a_plain_master_dbs_folder_lasts_as_long_as_its_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), &sample().plain());
    let opened = open_db(&path, DEFAULT_KEY).unwrap();
    let folder = opened._folder.as_ref().expect("a plain one is read from a file").path().to_path_buf();
    assert!(folder.join("master.db").is_file());
    assert_eq!(opened.collection().unwrap().library.tracks.len(), 3);
    drop(opened);
    assert!(!folder.exists());
}

/// A WAL whose last commit shrinks the database (rekordbox deleted rows and SQLite vacuumed),
/// over a file that says WAL mode as rekordbox's do, reads with the smaller page count.
#[test]
fn a_wal_that_shrinks_the_database_reads_smaller() {
    let dir = tempfile::tempdir().unwrap();
    let f = sample();
    let codec = codec(DEFAULT_KEY);
    f.exec("CREATE TABLE filler (body BLOB); WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 40) INSERT INTO filler SELECT zeroblob(3000) FROM n");
    let mut before = f.plain();
    f.exec("DROP TABLE filler; VACUUM");
    let mut after = f.plain();
    let pages = (after.len() / PAGE_SIZE) as u32;
    assert!(after.len() < before.len());
    // Both say WAL mode, as rekordbox's pages do, whichever page 1 is read.
    before[18..20].copy_from_slice(&[2, 2]);
    after[18..20].copy_from_slice(&[2, 2]);
    let frames: Vec<(u32, u32, Vec<u8>)> = (1..=pages)
        .map(|p| {
            let at = (p as usize - 1) * PAGE_SIZE;
            (p, if p == pages { pages } else { 0 }, codec.encrypt_page(p, &after[at..at + PAGE_SIZE]).unwrap())
        })
        .collect();
    let path = write(dir.path(), &codec.encrypt(&before).unwrap());
    std::fs::write(dir.path().join("master.db-wal"), loclib_sqlcipher::wal::tests::build(PAGE_SIZE, (1, 2), &frames))
        .unwrap();
    let opened = open_db(&path, DEFAULT_KEY).unwrap();
    let count: u32 = opened.conn.query_row("PRAGMA page_count", [], |r| r.get(0)).unwrap();
    assert_eq!(count, pages);
    assert_eq!(opened.collection().unwrap().library.tracks.len(), 3);
    // As decrypted, before SQLite reads it (which goes by the size in the header, and would read
    // a longer image the same): cut to the smaller size, its header saying rollback journal.
    let Image::Decrypted(image) = decrypt(&path, DEFAULT_KEY).unwrap() else { panic!("not decrypted") };
    assert_eq!(image.len(), pages as usize * PAGE_SIZE, "shrunk in memory");
    assert_eq!(image[18..20], [1, 1], "a rollback journal in memory");
    let folder = crate::scratch::folder().unwrap();
    let (copy, _) = decrypt_into(&path, DEFAULT_KEY, &folder).unwrap();
    let written = std::fs::read(copy).unwrap();
    assert_eq!(written.len(), pages as usize * PAGE_SIZE, "shrunk on disk");
    assert_eq!(written[18..20], [1, 1], "a rollback journal on disk");
    assert_eq!(written, *image, "the same image both ways");
}

/// A file whose pages authenticate under the key given but decrypt to something not laid out as
/// SQLCipher's (no reserved bytes for each page's IV and HMAC) reads as encrypted with another
/// key: the layout check does that (without it SQLite would read the IVs as rows and call the file
/// damaged), whether page 1 comes from the file or from the WAL.
#[test]
fn a_master_db_laid_out_otherwise_reads_as_another_key() {
    let dir = tempfile::tempdir().unwrap();
    let codec = codec(DEFAULT_KEY);
    let mut plain = sample().plain();
    plain[20] = 0;
    let pages = plain.chunks(PAGE_SIZE).enumerate();
    let bytes: Vec<u8> = pages.flat_map(|(i, p)| codec.encrypt_page(i as u32 + 1, p).unwrap()).collect();
    let path = write(dir.path(), &bytes);
    let err = open(&path, DEFAULT_KEY).unwrap_err();
    assert!(matches!(&err, Error::WrongKey(_)), "{err:?}");
    // The same first page from a WAL, over a file laid out as it should be.
    let good = sample().plain();
    let path = write(dir.path(), &codec.encrypt(&good).unwrap());
    let frame = codec.encrypt_page(1, &plain[..PAGE_SIZE]).unwrap();
    let pages = (good.len() / PAGE_SIZE) as u32;
    let wal = loclib_sqlcipher::wal::tests::build(PAGE_SIZE, (1, 2), &[(1, pages, frame)]);
    std::fs::write(dir.path().join("master.db-wal"), wal).unwrap();
    let err = open(&path, DEFAULT_KEY).unwrap_err();
    assert!(matches!(&err, Error::WrongKey(_)), "from the WAL: {err:?}");
}

/// The frames of a WAL that takes `before` to `after`, encrypted, the last one committing.
fn frames(codec: &Codec, before: &[u8], after: &[u8]) -> Vec<(u32, u32, Vec<u8>)> {
    let pages = (after.len() / PAGE_SIZE) as u32;
    let changed: Vec<u32> = (1..=pages)
        .filter(|&p| {
            let at = (p as usize - 1) * PAGE_SIZE;
            before.get(at..at + PAGE_SIZE) != Some(&after[at..at + PAGE_SIZE])
        })
        .collect();
    let last = changed.len();
    changed
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let at = (p as usize - 1) * PAGE_SIZE;
            (p, if i + 1 == last { pages } else { 0 }, codec.encrypt_page(p, &after[at..at + PAGE_SIZE]).unwrap())
        })
        .collect()
}

/// rekordbox restarts its WAL on disk while it's being read, after its header was: the header
/// read from the file after the WAL shows it, and `master.db` and its WAL are read again, as of
/// the checkpoint. Real files, encrypted as rekordbox's are, read by the reader `snapshot` uses.
#[test]
fn a_wal_restarted_on_disk_while_read_is_read_again() {
    let dir = tempfile::tempdir().unwrap();
    let f = sample();
    let codec = codec(DEFAULT_KEY);
    let before = f.plain();
    f.exec("UPDATE djmdContent SET Title = 'First' WHERE ID = '84935471'");
    let first = f.plain();
    f.exec("UPDATE djmdContent SET Title = 'Second' WHERE ID = '84935471'");
    let second = f.plain();
    let path = write(dir.path(), &codec.encrypt(&before).unwrap());
    let wal_path = dir.path().join("master.db-wal");
    let old = loclib_sqlcipher::wal::tests::build(PAGE_SIZE, (1, 2), &frames(&codec, &before, &first));
    std::fs::write(&wal_path, &old).unwrap();
    // After the checkpoint: the first change in master.db, a new generation holding the second.
    let (checkpointed, new) = (
        codec.encrypt(&first).unwrap(),
        loclib_sqlcipher::wal::tests::build(PAGE_SIZE, (3, 4), &frames(&codec, &first, &second)),
    );
    let copied = std::cell::RefCell::new(Vec::new());
    let whole_reads = std::cell::Cell::new(0);
    let wal = snapshot_with(
        &path,
        || {
            *copied.borrow_mut() = std::fs::read(&path)?;
            Ok(0)
        },
        |limit| {
            let read = read_wal(&wal_path, limit);
            if limit == u64::MAX && whole_reads.replace(whole_reads.get() + 1) == 0 {
                std::fs::write(&path, &checkpointed).unwrap();
                std::fs::write(&wal_path, &new).unwrap();
            }
            read
        },
    )
    .unwrap();
    assert_eq!(whole_reads.get(), 2, "read again");
    assert_eq!(wal, Some(new));
    assert_eq!(*copied.borrow(), checkpointed);
    assert_eq!(open(&path, DEFAULT_KEY).unwrap().library.track(84935471).unwrap().title, "Second");
}

/// What the child [`an_export_doesnt_hold_the_library_in_memory`] starts exports.
#[cfg(all(unix, feature = "files"))]
const EXPORTED: &str = "LOCALLIBRARY_TEST_EXPORTED";

/// An export decrypts `master.db` a page at a time into its scratch folder, never holding the
/// whole library in memory: a child process's peak memory grows by less than half the database
/// while it exports one of 24 MB.
#[cfg(all(unix, feature = "files"))]
#[test]
fn an_export_doesnt_hold_the_library_in_memory() {
    let dir = tempfile::tempdir().unwrap();
    let f = sample();
    f.exec(
        "CREATE TABLE filler (body BLOB); WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n \
         WHERE i < 6000) INSERT INTO filler SELECT randomblob(3900) FROM n",
    );
    let path = write(dir.path(), &f.encrypted(DEFAULT_KEY));
    let len = std::fs::metadata(&path).unwrap().len();
    assert!(len > 24_000_000, "{len}");
    let tmp = tempfile::tempdir().unwrap();
    let out = child("masterdb::tests::exported", EXPORTED, &path.display().to_string(), tmp.path(), "grew");
    let grew: u64 = out.trim().rsplit(' ').next().unwrap().parse().unwrap();
    assert!(grew < len / 2, "peak memory grew {grew} bytes exporting {len}");
}

/// The child [`an_export_doesnt_hold_the_library_in_memory`] starts: how much its peak memory
/// grew exporting.
#[cfg(all(unix, feature = "files"))]
#[test]
#[ignore = "started by an_export_doesnt_hold_the_library_in_memory"]
fn exported() {
    let Ok(path) = std::env::var(EXPORTED) else { return };
    crate::scratch::test_root(std::env::temp_dir());
    let peak = || {
        // SAFETY: `usage` is a local getrusage fills.
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) }, 0);
        // Bytes on macOS, KiB on Linux.
        let scale = if cfg!(target_os = "macos") { 1 } else { 1024 };
        usage.ru_maxrss as u64 * scale
    };
    let path = Path::new(&path);
    let into = path.with_file_name("copy.db");
    // Nothing big read before: the peak is a high-water mark, so a read of the library first would
    // hide the export's.
    let before = peak();
    files::export(path, DEFAULT_KEY, false, &into).unwrap();
    println!("grew {}", peak().saturating_sub(before));
}

fn write(dir: &Path, bytes: &[u8]) -> PathBuf {
    let path = dir.join("master.db");
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn reads_tracks_playlists_cues_tags_and_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), &sample().encrypted(DEFAULT_KEY));
    let c = open(&path, DEFAULT_KEY).unwrap();
    assert_eq!(c.db_version.as_deref(), Some("7000"));
    let ids: Vec<u32> = c.library.tracks.iter().map(|t| t.id).collect();
    assert_eq!(ids, [4, 84935471, 205356547], "rekordbox's ids, without the deleted track");
    let t = c.library.track(84935471).unwrap();
    assert_eq!(
        (t.artist.as_str(), t.title.as_str(), t.album.as_str(), t.album_artist.as_str()),
        ("Daft Punk", "One More Time", "Discovery", "Daft Punk")
    );
    assert_eq!((t.genre.as_str(), t.label.as_str(), t.key.as_str()), ("House", "Virgin", "Am"));
    assert_eq!((t.bpm, t.duration_secs, t.rating, t.color), (122.8, 320, 5, Some(TrackColor::Red)));
    assert_eq!((t.play_count, t.year, t.isrc.as_str()), (7, 2000, "GBDUW0000059"));
    assert_eq!((t.mix_name.as_str(), t.comment.as_str()), ("Radio Edit", "classic"));
    assert_eq!(t.file_path, "/Users/dj/Music/Daft Punk/Discovery/01 One More Time.mp3");
    assert_eq!(
        (t.date_added.as_str(), t.artwork_path.as_deref()),
        ("2024-03-01", Some("/PIONEER/Artwork/00001/a1.jpg"))
    );
    assert_eq!(c.library.track(205356547).unwrap().date_added, "2023-12-24", "falls back to DateCreated");

    let top: Vec<&Playlist> = c.library.children(None);
    assert_eq!(
        top.iter().map(|p| (p.name.as_str(), p.is_folder)).collect::<Vec<_>>(),
        [("Shows", true), ("Recent", false)]
    );
    let friday = c.library.find_playlist("Shows/Friday").unwrap();
    assert_eq!(friday.track_ids, [205356547, 84935471], "in TrackNo order, without the deleted track");
    assert_eq!(c.smart_playlists, [12]);
    assert_eq!(c.library.history_playlists, 1);
    assert_eq!(c.library.history[0].track_ids, [84935471]);

    let cues = &c.cues[&84935471];
    assert_eq!(cues.len(), 3);
    assert_eq!((cues[0].hot_cue, cues[0].start_ms, cues[0].end_ms), (None, 500, None));
    assert_eq!((cues[1].hot_cue, cues[1].comment.as_str()), (Some(0), "Drop"));
    assert_eq!(cues[1].color, Some(loclib_model::analysis::HOT_CUE_PALETTE[0]));
    assert_eq!((cues[2].hot_cue, cues[2].end_ms), (Some(1), Some(4000)));
    assert!(!c.cues.contains_key(&3));

    assert_eq!(c.tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Genre", "Situation"]);
    assert_eq!(c.tags[1].tags[0].track_ids, [205356547, 84935471]);
    let one_more_time = c.library.tracks.iter().find(|t| t.title == "One More Time").unwrap();
    let tags_of = |id: u32| {
        let mut out = Vec::new();
        for cat in &c.tags {
            for tag in cat.tags.iter().filter(|t| t.track_ids.contains(&id)) {
                out.push(format!("{}/{}", cat.name, tag.name));
            }
        }
        out
    };
    assert_eq!(tags_of(one_more_time.id), ["Genre/Deep", "Situation/Peak"]);

    let info = c.info();
    assert_eq!((info.summary.tracks, info.summary.playlists, info.summary.folders), (3, 2, 1));
    assert_eq!((info.hot_cues, info.memory_cues, info.loops), (2, 1, 1));
    assert_eq!((info.tag_categories, info.tags, info.tagged_tracks), (2, 2, 2));
    assert_eq!(info.smart_playlists, ["Recent"]);
}

#[test]
fn kind_4_is_a_memory_loop_and_hot_cue_d_is_5() {
    let f = sample();
    f.exec(
        "INSERT INTO djmdCue (ID, ContentID, InMsec, OutMsec, Kind, ColorTableIndex, Comment) VALUES
                 ('c5', '205356547', 1000, 9000, 4, 0, ''), ('c6', '205356547', 3000, -1, 3, 0, 'C'),
                 ('c7', '205356547', 5000, -1, 5, 0, 'D'), ('c8', '205356547', 7000, -1, 9, 0, 'H');",
    );
    let dir = tempfile::tempdir().unwrap();
    let c = open(&write(dir.path(), &f.encrypted(DEFAULT_KEY)), DEFAULT_KEY).unwrap();
    let cues: Vec<_> = c.cues[&205356547].iter().map(|c| (c.hot_cue, c.start_ms, c.end_ms)).collect();
    assert_eq!(cues, [(None, 1000, Some(9000)), (Some(2), 3000, None), (Some(3), 5000, None), (Some(7), 7000, None)]);
}

#[test]
fn a_wrong_key_is_refused_and_the_right_one_given_works() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), &sample().encrypted("another key"));
    let err = open(&path, DEFAULT_KEY).unwrap_err();
    assert!(matches!(&err, Error::WrongKey(p) if *p == path), "{err}");
    assert!(err.to_string().contains(KEY_ENV), "{err}");
    assert_eq!(open(&path, "another key").unwrap().library.tracks.len(), 3);
    // Not a database at all.
    let junk = write(dir.path(), &[7u8; 3 * PAGE_SIZE]);
    assert!(open(&junk, DEFAULT_KEY).is_err());
}

#[test]
fn a_damaged_page_is_reported_as_master_dbs() {
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = sample().encrypted(DEFAULT_KEY);
    bytes[PAGE_SIZE + 100] ^= 1;
    let path = write(dir.path(), &bytes);
    let err = open(&path, DEFAULT_KEY).unwrap_err().to_string();
    assert!(err.starts_with(&format!("could not parse {}: page 2 fails authentication", path.display())), "{err}");
}

#[test]
fn a_damaged_b_tree_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let f = sample();
    let codec = codec(DEFAULT_KEY);
    let bytes = with_broken_table(&f.conn, &codec, f.plain(), "djmdCue");
    let path = write(dir.path(), &bytes);
    let err = open(&path, DEFAULT_KEY).unwrap_err().to_string();
    assert!(err.contains("SQLite finds it damaged"), "{err}");
}

#[test]
fn a_plain_database_reads_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), &sample().plain());
    assert_eq!(open(&path, "unused").unwrap().library.tracks.len(), 3);
}

#[test]
fn committed_wal_frames_are_replayed_and_the_original_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let f = sample();
    let codec = codec(DEFAULT_KEY);
    let before = f.plain();
    f.exec("UPDATE djmdContent SET Title = 'One More Time (WAL)' WHERE ID = '84935471'");
    f.track("5", "/Users/dj/Music/New.mp3", "Newcomer", "Added Later");
    let after = f.plain();
    assert!(after.len() >= before.len());
    let pages = (after.len() / PAGE_SIZE) as u32;
    let changed: Vec<u32> = (1..=pages)
        .filter(|&p| {
            let at = (p as usize - 1) * PAGE_SIZE;
            before.get(at..at + PAGE_SIZE) != Some(&after[at..at + PAGE_SIZE])
        })
        .collect();
    let frames: Vec<(u32, u32, Vec<u8>)> = changed
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let at = (p as usize - 1) * PAGE_SIZE;
            let commit = if i + 1 == changed.len() { pages } else { 0 };
            (p, commit, codec.encrypt_page(p, &after[at..at + PAGE_SIZE]).unwrap())
        })
        .collect();
    let path = write(dir.path(), &codec.encrypt(&before).unwrap());
    let wal = loclib_sqlcipher::wal::tests::build(PAGE_SIZE, (1, 2), &frames);
    std::fs::write(dir.path().join("master.db-wal"), &wal).unwrap();
    let original = std::fs::read(&path).unwrap();

    let c = open(&path, DEFAULT_KEY).unwrap();
    assert_eq!(c.library.track(84935471).unwrap().title, "One More Time (WAL)");
    assert_eq!(c.library.track(5).unwrap().title, "Added Later");
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(std::fs::read(dir.path().join("master.db-wal")).unwrap(), wal);
    assert!(!dir.path().join("master.db-shm").exists(), "never opened with SQLite");
    assert_eq!(c.info().wal_bytes, Some(wal.len() as u64));
}

#[test]
fn master_db_is_copied_again_when_its_wal_restarts_meanwhile() {
    let dir = tempfile::tempdir().unwrap();
    let (path, to) = (dir.path().join("master.db"), dir.path().join("copy"));
    let wal_path = dir.path().join("master.db-wal");
    let generation = |salts| loclib_sqlcipher::wal::tests::build(PAGE_SIZE, salts, &[]);
    std::fs::write(&path, "before the checkpoint").unwrap();
    std::fs::write(&wal_path, generation((1, 2))).unwrap();
    // rekordbox checkpoints into master.db and restarts its WAL just after the first copy.
    let copies = std::cell::Cell::new(0);
    let wal = snapshot(&path, || {
        let copied = std::fs::copy(&path, &to);
        if copies.replace(copies.get() + 1) == 0 {
            std::fs::write(&path, "after the checkpoint").unwrap();
            std::fs::write(&wal_path, generation((3, 4))).unwrap();
        }
        copied
    })
    .unwrap();
    assert_eq!(std::fs::read_to_string(&to).unwrap(), "after the checkpoint");
    assert_eq!(wal, Some(generation((3, 4))));
}

/// A restart while the WAL is being read, after its header was (rekordbox writes the new header
/// first, then frames that overtake the read), shows only in the header read after it: the
/// copy is made again, and a WAL that never holds still gives up saying so.
#[test]
fn master_db_is_copied_again_when_its_wal_restarts_while_read() {
    let path = Path::new("master.db");
    let generation = |salts| Some(loclib_sqlcipher::wal::tests::build(PAGE_SIZE, salts, &[]));
    // Each copy reads three headers: before the copy, with the whole WAL, and after it.
    let reads = |torn: u32| {
        let reads = std::cell::RefCell::new(Vec::new());
        for round in 0..3 {
            let restarted = generation((10 + round, 1));
            let header = if round < torn { restarted } else { generation((1, 2)) };
            reads.borrow_mut().extend([generation((1, 2)), generation((1, 2)), header]);
        }
        reads.borrow_mut().reverse();
        reads
    };
    for torn in [0, 1, 2] {
        let (reads, copies) = (reads(torn), std::cell::Cell::new(0));
        let wal =
            snapshot_with(path, || Ok(copies.replace(copies.get() + 1)), |_| Ok(reads.borrow_mut().pop().unwrap()));
        assert_eq!(wal.unwrap(), generation((1, 2)), "{torn} torn");
        assert_eq!(copies.get(), u64::from(torn) + 1);
    }
    let reads = reads(3);
    let err = snapshot_with(path, || Ok(0), |_| Ok(reads.borrow_mut().pop().unwrap())).unwrap_err();
    assert!(matches!(err, Error::Changing(_)), "{err}");
}

/// SQLCipher itself, given rekordbox's key as `PRAGMA key` (as pyrekordbox opens master.db):
/// a database it writes reads here, and it reads the fixtures. Skipped without the sqlcipher
/// CLI (see loclib-testkit's `sqlcipher`).
#[test]
fn sqlcipher_and_this_reader_agree_on_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let theirs = dir.path().join("theirs.db");
    let create = "CREATE TABLE djmdContent (ID VARCHAR(255) PRIMARY KEY, FolderPath VARCHAR(255), \
                      Title VARCHAR(255), BPM INTEGER, rb_local_deleted INTEGER DEFAULT 0); \
                      INSERT INTO djmdContent (ID, FolderPath, Title, BPM) VALUES ('77', '/Users/dj/x.mp3', 'Theirs', 12450);";
    if super::sqlcipher::run(&theirs, DEFAULT_KEY, &[create]).is_none() {
        return;
    }
    let c = open(&theirs, DEFAULT_KEY).unwrap();
    assert_eq!((c.library.tracks[0].id, c.library.tracks[0].title.as_str()), (77, "Theirs"));
    assert_eq!(c.library.tracks[0].bpm, 124.5);

    let ours = write(dir.path(), &sample().encrypted(DEFAULT_KEY));
    let checks = ["PRAGMA cipher_integrity_check;", "SELECT count(*) FROM djmdContent WHERE rb_local_deleted = 0;"];
    assert_eq!(super::sqlcipher::run(&ours, DEFAULT_KEY, &checks).unwrap(), ["ok", "3"]);
}

#[test]
fn numeric_ids_are_kept_others_renumbered() {
    let kept = id_map(["30", "7", "123456789"].into_iter().map(String::from));
    assert_eq!((kept["30"], kept["7"], kept["123456789"]), (30, 7, 123456789));
    let renumbered = id_map(["30", "x", "99999999999"].into_iter().map(String::from));
    assert_eq!((renumbered["30"], renumbered["x"], renumbered["99999999999"]), (1, 2, 3));
    // A zero, or two spellings of one number, renumbers too.
    assert_eq!(id_map(["0", "5"].into_iter().map(String::from))["5"], 2);
    assert_eq!(id_map(["05", "5"].into_iter().map(String::from))["5"], 2);
}

#[test]
fn values_read_leniently() {
    assert_eq!(int(&Value::Text(" 12800 ".into())), 12800);
    assert_eq!(int(&Value::Text("3.0".into())), 3);
    assert_eq!(int(&Value::Null), 0);
    assert_eq!(text(&Value::Integer(42)), "42");
    assert_eq!(clamp::<u16>(70_000), u16::MAX);
    assert_eq!(clamp::<u32>(-5), 0);
}

#[test]
fn blobs_reals_and_nulls_read_too() {
    assert_eq!(text(&Value::Blob(b"Am".to_vec())), "Am");
    assert_eq!(text(&Value::Real(128.5)), "128.5");
    assert_eq!(text(&Value::Null), "");
    assert_eq!(int(&Value::Real(320.9)), 320);
}

#[test]
fn files_that_arent_a_collection_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let err = |bytes: &[u8]| {
        let path = write(dir.path(), bytes);
        open(&path, DEFAULT_KEY).unwrap_err().to_string()
    };
    assert!(err(&[0; 100]).ends_with("too short for a SQLite database"));
    assert!(err(&[0; PAGE_SIZE + 10]).ends_with("4106 bytes is not a whole number of 4096-byte pages"));

    let other = dir.path().join("other.db");
    scratch_db(&other).execute_batch("CREATE TABLE notes (body TEXT)").unwrap();
    let message = err(&std::fs::read(&other).unwrap());
    assert!(message.ends_with("no djmdContent table; not a rekordbox collection database"), "{message}");
}

/// A plain (already decrypted) master.db is read with its WAL, as SQLite would read it.
#[test]
fn a_plain_database_is_read_with_its_wal() {
    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("live");
    std::fs::create_dir(&live).unwrap();
    let path = write(&live, &sample().plain());
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode = WAL; PRAGMA wal_autocheckpoint = 0;
             UPDATE djmdContent SET Title = 'Never Grow Old (WAL)' WHERE ID = '205356547';",
    )
    .unwrap();
    // Copied while the WAL holds the change, as a backup of rekordbox's folder would be.
    let copy = dir.path().join("copy");
    std::fs::create_dir(&copy).unwrap();
    for name in ["master.db", "master.db-wal"] {
        std::fs::copy(live.join(name), copy.join(name)).unwrap();
    }
    drop(conn);
    let titles: Vec<String> =
        open(&copy.join("master.db"), "unused").unwrap().library.tracks.into_iter().map(|t| t.title).collect();
    assert!(titles.contains(&"Never Grow Old (WAL)".to_string()), "{titles:?}");
    // Copied for an export, the same: the copy keeps its WAL beside it for SQLite.
    let (folder, copied) = (crate::scratch::folder().unwrap(), copy.join("master.db"));
    let (db, salt) = open_copy(&copied, "unused", &folder, false).unwrap();
    assert_eq!(salt, None, "plain");
    let titles: Vec<String> = db.collection().unwrap().library.tracks.into_iter().map(|t| t.title).collect();
    assert!(titles.contains(&"Never Grow Old (WAL)".to_string()), "copied: {titles:?}");
}

#[test]
fn cloud_folders_come_from_rekordbox_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rekordbox3.settings");
    std::fs::write(
        &path,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<PROPERTIES>
  <VALUE name="DropboxSharingPath" val="/Users/dj/Library/CloudStorage/Dropbox &amp; Co"/>
  <VALUE name="GoogleDriveSharingPath" val=""/>
  <VALUE name="DropboxToken" val="not-for-us"/>
</PROPERTIES>"#,
    )
    .unwrap();
    let folders = CloudFolders::from_settings(&path).unwrap();
    assert_eq!(folders.dropbox, Some(PathBuf::from("/Users/dj/Library/CloudStorage/Dropbox & Co")));
    assert_eq!(folders.google_drive, None, "an empty value is no folder");
    assert!(CloudFolders::from_settings(&dir.path().join("missing")).is_err());
}

/// A collection whose second track Cloud Library Sync keeps in Dropbox, with the analysis
/// file it lists for the first.
fn cloud_sample() -> Fixture {
    let f = sample();
    f.exec(&format!(
        "CREATE TABLE djmdCloudProperty (ID VARCHAR(255) PRIMARY KEY, Reserved1 TEXT, Reserved2 TEXT, {STATS});
             INSERT INTO djmdCloudProperty (ID, Reserved1, Reserved2) VALUES ('1', 'SyncService', 'Dropbox');
             CREATE TABLE contentFile (ID VARCHAR(255) PRIMARY KEY, ContentID VARCHAR(255), Path VARCHAR(255),
                 Hash VARCHAR(255), Size INTEGER, {STATS});
             INSERT INTO contentFile (ID, ContentID, Path, Size) VALUES
                 ('f1', '84935471', '/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT', 4);
             UPDATE djmdContent SET FolderPath = '/contents_2354715273/Floorplan/ngo.mp3' WHERE ID = '205356547';"
    ));
    f
}

#[test]
fn finds_audio_cloud_library_sync_keeps_in_a_storage_service() {
    let dir = tempfile::tempdir().unwrap();
    let rb = dir.path().join("rekordbox");
    std::fs::create_dir(&rb).unwrap();
    let c = open(&write(&rb, &cloud_sample().encrypted(DEFAULT_KEY)), DEFAULT_KEY).unwrap();
    assert_eq!(c.sync_service.as_deref(), Some("Dropbox"));
    let local = c.library.track(84935471).unwrap();
    let synced = c.library.track(205356547).unwrap();
    let (dropbox, drive) = (dir.path().join("Dropbox"), dir.path().join("Google Drive"));
    let audio = dropbox.join("rekordbox/contents_2354715273/Floorplan/ngo.mp3");
    std::fs::create_dir_all(audio.parent().unwrap()).unwrap();
    std::fs::write(&audio, b"mp3").unwrap();

    let both = CloudFolders { dropbox: Some(dropbox.clone()), google_drive: Some(drive.clone()) };
    assert_eq!(c.audio_file(local, &both), Some(PathBuf::from(&local.file_path)), "a full path stays as it is");
    assert_eq!(c.audio_file(synced, &both), Some(audio.clone()));
    let only_drive = CloudFolders { dropbox: None, google_drive: Some(drive) };
    assert_eq!(c.audio_file(synced, &only_drive), None, "not in any folder rekordbox syncs to");
    assert_eq!(c.audio_file(synced, &CloudFolders::default()), None);

    // The analysis file contentFile lists, missing, then a partial copy, then all of it.
    let dat = "/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT";
    assert_eq!(c.share_file(dat), rb.join("share/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT"));
    assert!(c.still_syncing(dat));
    std::fs::create_dir_all(c.share_file(dat).parent().unwrap()).unwrap();
    std::fs::write(c.share_file(dat), b"PM").unwrap();
    assert!(c.still_syncing(dat));
    std::fs::write(c.share_file(dat), b"PMAI").unwrap();
    assert!(!c.still_syncing(dat));
    assert!(!c.still_syncing("/PIONEER/Artwork/00001/a1.jpg"), "a file Cloud Library Sync doesn't list");
}

#[test]
fn entries_rekordbox_left_on_a_folder_are_ignored() {
    let f = sample();
    f.exec("INSERT INTO djmdSongPlaylist (ID, PlaylistID, ContentID, TrackNo) VALUES ('z', '10', '4', 1);");
    let dir = tempfile::tempdir().unwrap();
    let c = open(&write(dir.path(), &f.encrypted(DEFAULT_KEY)), DEFAULT_KEY).unwrap();
    let shows = c.library.find_playlist("Shows").unwrap();
    assert!(shows.is_folder && shows.track_ids.is_empty(), "{:?}", shows.track_ids);
}

#[test]
fn keys_resolve_in_order() {
    assert_eq!(resolve_key(Some("abc")), "abc");
    // The environment is shared between tests; only check the fallback when it's unset.
    if std::env::var_os(KEY_ENV).is_none() {
        assert_eq!(resolve_key(None), DEFAULT_KEY);
        assert_eq!(resolve_key(Some("")), DEFAULT_KEY);
    }
}

/// One drawn row of `djmdContent`, and what the collection should make of it.
#[derive(Debug, Clone)]
struct Row {
    id: String,
    path: String,
    artist: &'static str,
    title: String,
    bpm: i64,
    rating: i64,
    color: i64,
    deleted: bool,
}

/// A drawn `master.db` (tracks, some deleted; folders, playlists and an intelligent one with
/// entries in any order; a history list; cues of every kind; My Tags) reads back as rekordbox
/// holds it: deleted rows left out, ids kept only when they're all distinct numbers, entries in
/// `TrackNo` order, and cues and tags with the tracks they belong to. A drive holding exactly
/// the collection then differs from it in nothing.
#[hegel::test(test_cases = 25)]
fn prop_a_drawn_collection_reads_back_as_written(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let f = Fixture::new();
    let numeric = tc.draw(gs::booleans());
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(6));
    let mut rows = Vec::new();
    for i in 0..n {
        let id = match numeric {
            true => (tc.draw(gs::integers::<u32>().min_value(1).max_value(40)) * 100 + i as u32).to_string(),
            false => format!("t{i}"),
        };
        let artist = tc.draw(gs::sampled_from(vec!["Daft Punk", "Ünï", "Floorplan"]));
        let row = Row {
            id,
            path: format!("/Users/dj/Music/{artist}/Track {i}.mp3"),
            artist,
            title: format!("Track {i}"),
            bpm: tc.draw(gs::integers::<i64>().min_value(0).max_value(20_000)),
            rating: tc.draw(gs::integers::<i64>().min_value(-1).max_value(7)),
            color: tc.draw(gs::integers::<i64>().min_value(0).max_value(9)),
            deleted: tc.draw(gs::weighted_booleans(0.2)),
        };
        f.track(&row.id, &row.path, row.artist, &row.title);
        f.conn
            .execute(
                "UPDATE djmdContent SET BPM = ?2, Rating = ?3, ColorID = ?4, rb_local_deleted = ?5 WHERE ID = ?1",
                params![row.id, row.bpm, row.rating, row.color.to_string(), row.deleted as i64],
            )
            .unwrap();
        rows.push(row);
    }
    let live: Vec<&Row> = rows.iter().filter(|r| !r.deleted).collect();
    let ids_kept = numeric;
    let id_of = |row: &Row| -> u32 {
        match ids_kept {
            true => row.id.parse().unwrap(),
            false => live.iter().position(|r| r.id == row.id).unwrap() as u32 + 1,
        }
    };

    // Playlists: a folder, playlists in it or at the top, one intelligent.
    f.exec("INSERT INTO djmdPlaylist (ID, Seq, Name, Attribute, ParentID) VALUES ('p0', 0, 'Shows', 1, 'root');");
    // (path, id, entries as TrackNo and row)
    type Drawn<'r> = (String, String, Vec<(i64, &'r Row)>);
    let mut playlists: Vec<Drawn> = Vec::new();
    for p in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
        let (parent, folder_name) = if tc.draw(gs::booleans()) { ("p0", "Shows/") } else { ("root", "") };
        let id = format!("p{}", p + 1);
        let name = format!("List {p}");
        let smart = tc.draw(gs::weighted_booleans(0.2));
        let deleted = tc.draw(gs::weighted_booleans(0.15));
        f.conn
            .execute(
                "INSERT INTO djmdPlaylist (ID, Seq, Name, Attribute, ParentID, rb_local_deleted) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, p as i64 + 1, name, if smart { 4 } else { 0 }, parent, deleted as i64],
            )
            .unwrap();
        let mut entries = Vec::new();
        if !smart {
            let picks: Vec<usize> = tc.draw(gs::vecs(gs::integers::<usize>().max_value(n - 1)).max_size(4));
            for (e, k) in picks.into_iter().enumerate() {
                let track_no = tc.draw(gs::integers::<i64>().min_value(1).max_value(5));
                f.conn
                    .execute(
                        "INSERT INTO djmdSongPlaylist (ID, PlaylistID, ContentID, TrackNo) VALUES (?1, ?2, ?3, ?4)",
                        params![format!("{id}e{e}"), id, rows[k].id, track_no],
                    )
                    .unwrap();
                entries.push((track_no, &rows[k]));
            }
        }
        if !deleted {
            playlists.push((format!("{folder_name}{name}"), id, entries));
        }
    }
    // Entries go by TrackNo, then by the order they were stored in; deleted tracks go.
    let want_entries = |entries: &[(i64, &Row)]| -> Vec<u32> {
        let mut e: Vec<(usize, &(i64, &Row))> = entries.iter().enumerate().collect();
        e.sort_by_key(|(i, (no, _))| (*no, *i));
        e.into_iter().filter(|(_, (_, r))| !r.deleted).map(|(_, (_, r))| id_of(r)).collect()
    };

    // Cues on any row, of any kind.
    let mut cues: Vec<(&Row, i64, i64, i64)> = Vec::new();
    for c in 0..tc.draw(gs::integers::<usize>().max_value(5)) {
        let row = &rows[tc.draw(gs::integers::<usize>().max_value(n - 1))];
        let start = tc.draw(gs::integers::<i64>().min_value(0).max_value(9_000));
        let end = if tc.draw(gs::booleans()) { start + tc.draw(gs::integers::<i64>().max_value(4_000)) } else { -1 };
        let kind = tc.draw(gs::integers::<i64>().min_value(0).max_value(10));
        f.conn
            .execute(
                "INSERT INTO djmdCue (ID, ContentID, InMsec, OutMsec, Kind, ColorTableIndex, Comment) \
                     VALUES (?1, ?2, ?3, ?4, ?5, 1, 'cue')",
                params![format!("c{c}"), row.id, start, end, kind],
            )
            .unwrap();
        cues.push((row, start, end, kind));
    }

    // My Tags: one category, a tag on drawn tracks (repeats too).
    f.exec(
        "INSERT INTO djmdMyTag (ID, Seq, Name, Attribute, ParentID) VALUES ('100', 1, 'Mood', 1, 'root'), \
             ('101', 0, 'Deep', 0, '100');",
    );
    let tagged: Vec<usize> = tc.draw(gs::vecs(gs::integers::<usize>().max_value(n - 1)).max_size(4));
    for (i, k) in tagged.iter().enumerate() {
        f.conn
            .execute(
                "INSERT INTO djmdSongMyTag (ID, MyTagID, ContentID, TrackNo) VALUES (?1, '101', ?2, ?3)",
                params![format!("s{i}"), rows[*k].id, i as i64],
            )
            .unwrap();
    }

    let dir = tempfile::tempdir().unwrap();
    let encrypted = tc.draw(gs::booleans());
    let path = write(dir.path(), &if encrypted { f.encrypted(DEFAULT_KEY) } else { f.plain() });
    let c = open(&path, DEFAULT_KEY).unwrap();

    // Tracks.
    let mut want: Vec<(u32, String, String, f64, u8, Option<TrackColor>)> = live
        .iter()
        .map(|r| {
            let color = TrackColor::from_id(r.color as u8);
            (id_of(r), r.path.clone(), r.artist.to_string(), r.bpm as f64 / 100.0, r.rating.clamp(0, 5) as u8, color)
        })
        .collect();
    want.sort_by_key(|w| w.0);
    let got: Vec<_> = c
        .library
        .tracks
        .iter()
        .map(|t| (t.id, t.file_path.clone(), t.artist.clone(), t.bpm, t.rating, t.color))
        .collect();
    assert_eq!(got, want);

    // Playlists.
    let mut got: Vec<(String, Vec<u32>)> = c
        .library
        .playlists
        .iter()
        .filter(|p| !p.is_folder)
        .map(|p| (c.library.playlist_path(p.id), p.track_ids.clone()))
        .collect();
    let mut want: Vec<(String, Vec<u32>)> =
        playlists.iter().map(|(path, _, e)| (path.clone(), want_entries(e))).collect();
    got.sort();
    want.sort();
    assert_eq!(got, want);
    assert_eq!(c.smart_playlists.len(), c.info().smart_playlists.len());

    // Cues: those of live tracks, as hot cues, memory cues and loops.
    let live_cues: Vec<_> = cues.iter().filter(|(r, ..)| !r.deleted).collect();
    assert_eq!(c.cues.values().map(Vec::len).sum::<usize>(), live_cues.len());
    for (r, start, end, kind) in &live_cues {
        let list = &c.cues[&id_of(r)];
        let hot = hot_cue_from_kind(*kind);
        let end = (*end > *start).then_some(*end as u32);
        assert!(list.iter().any(|q| q.hot_cue == hot && q.start_ms == *start as u32 && q.end_ms == end), "{list:?}");
        assert!(hot.is_none_or(|h| hot_cue_from_kind(kind_for_hot_cue(h)) == Some(h)));
    }
    let info = c.info();
    assert_eq!(
        info.hot_cues + info.memory_cues + info.loops,
        live_cues.len() + c.cues.values().flatten().filter(|q| q.hot_cue.is_some() && q.end_ms.is_some()).count()
    );

    // My Tags: each live track once, in the order tagged.
    let mut seen = FxHashSet::default();
    let tag_want: Vec<u32> =
        tagged.iter().map(|k| &rows[*k]).filter(|r| !r.deleted).map(id_of).filter(|id| seen.insert(*id)).collect();
    assert_eq!(c.tags.len(), 1);
    assert_eq!(c.tags[0].tags[0].track_ids, tag_want);
    assert_eq!(info.tagged_tracks, tag_want.len());
    assert!(tag_want.iter().all(|id| c.library.track(*id).is_some()), "only the library's tracks");
}

/// A file that isn't a rekordbox collection is refused saying why: too short, not whole pages,
/// SQLite without rekordbox's tables or that SQLite can't read, or encrypted with another key or
/// to something that isn't rekordbox's layout (then with the way on: give the right key).
#[hegel::test(test_cases = 15)]
fn prop_files_that_arent_a_collection_say_why(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let kind = tc.draw(gs::integers::<u8>().max_value(5));
    let (bytes, why): (Vec<u8>, &str) = match kind {
        0 => (vec![0; tc.draw(gs::integers::<usize>().max_value(PAGE_SIZE - 1))], "too short for a SQLite database"),
        1 => {
            let pages = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
            (
                vec![0; pages * PAGE_SIZE + tc.draw(gs::integers::<usize>().min_value(1).max_value(PAGE_SIZE - 1))],
                "whole number of 4096-byte pages",
            )
        }
        2 => {
            let other = dir.path().join("other.db");
            scratch_db(&other).execute_batch("CREATE TABLE notes (body TEXT)").unwrap();
            (std::fs::read(&other).unwrap(), "no djmdContent table")
        }
        // Few keys: deriving one takes a while, and each is derived once.
        3 => (sample().encrypted(tc.draw(gs::sampled_from(vec!["another key", "rekordbox 8"]))), KEY_ENV),
        // SQLite's header and nothing SQLite can read after it.
        4 => {
            let mut bytes = vec![0; tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) * PAGE_SIZE];
            bytes[..16].copy_from_slice(SQLITE_MAGIC);
            (bytes, "not a database")
        }
        // Encrypted with the key given, but what that decrypts to isn't laid out as rekordbox's
        // databases are (its pages reserve other than the bytes the cipher needs).
        _ => {
            let mut plain = sample().plain();
            let reserve = tc.draw(gs::integers::<u8>().max_value(254));
            plain[20] = if usize::from(reserve) >= RESERVE { reserve + 1 } else { reserve };
            let codec = codec(DEFAULT_KEY);
            let pages = plain.chunks(PAGE_SIZE).enumerate();
            (pages.flat_map(|(i, p)| codec.encrypt_page(i as u32 + 1, p).unwrap()).collect(), KEY_ENV)
        }
    };
    let path = write(dir.path(), &bytes);
    let err = open(&path, DEFAULT_KEY).unwrap_err();
    assert!(err.to_string().contains(why), "{err}");
    match kind {
        3 | 5 => assert!(matches!(err, Error::WrongKey(_)), "{err}"),
        _ => assert!(matches!(err, Error::Parse { .. }), "{err}"),
    }
}

#[test]
fn the_library_from_root_names_files_where_they_are_on_this_computer() {
    let dir = tempfile::tempdir().unwrap();
    let rb = dir.path().join("rekordbox");
    std::fs::create_dir(&rb).unwrap();
    let f = cloud_sample();
    // rekordbox on Windows names a file on this computer from its drive.
    if cfg!(windows) {
        f.exec("UPDATE djmdContent SET FolderPath = 'C:' || FolderPath WHERE FolderPath NOT LIKE '/contents_%'");
    }
    let c = open(&write(&rb, &f.encrypted(DEFAULT_KEY)), DEFAULT_KEY).unwrap();
    let dropbox = dir.path().join("Dropbox");
    let audio = dropbox.join("rekordbox/contents_2354715273/Floorplan/ngo.mp3");
    std::fs::create_dir_all(audio.parent().unwrap()).unwrap();
    std::fs::write(&audio, b"mp3").unwrap();
    let cloud = CloudFolders { dropbox: Some(dropbox), google_drive: None };

    let lib = c.library_from_root(&cloud);
    let (local, synced) = (lib.track(84935471).unwrap(), lib.track(205356547).unwrap());
    let was = c.library.track(84935471).unwrap();
    assert_eq!(local.file_path, was.file_path, "a full path stays as it is");
    assert_eq!(PathBuf::from(&synced.file_path), audio);
    assert_eq!(PathBuf::from(&local.analyze_path), c.share_file(&was.analyze_path));
    if let Some(art) = &was.artwork_path {
        assert_eq!(local.artwork_path.as_deref().map(PathBuf::from), Some(c.share_file(art)));
    }
    for t in &lib.tracks {
        for p in [&t.file_path, &t.analyze_path].into_iter().chain(&t.artwork_path).filter(|p| !p.is_empty()) {
            assert!(Path::new(p).is_absolute(), "{p} is a full path");
        }
    }
    let gone = c.library_from_root(&CloudFolders::default());
    assert_eq!(
        gone.track(205356547).unwrap().file_path,
        "/contents_2354715273/Floorplan/ngo.mp3",
        "not here: as it was"
    );
    assert_eq!(gone.tracks.len(), c.library.tracks.len());
}

/// The cloud sample with Cloud Library Sync on `service`, read once per service: building and
/// reading it is most of a case's time, and the draws only pick among three.
fn cloud_collection(service: &'static str) -> Collection {
    static READ: std::sync::Mutex<Vec<(&str, Collection)>> = std::sync::Mutex::new(Vec::new());
    let mut read = READ.lock().unwrap();
    if let Some((_, c)) = read.iter().find(|(s, _)| *s == service) {
        return c.clone();
    }
    let f = cloud_sample();
    f.exec(&format!("UPDATE djmdCloudProperty SET Reserved2 = '{service}'"));
    let dir = tempfile::tempdir().unwrap();
    let c = open(&write(dir.path(), &f.plain()), DEFAULT_KEY).unwrap();
    read.push((service, c.clone()));
    c
}

/// rekordbox's settings say where its cloud folders are (an empty value is none, other values
/// are left alone), and a track Cloud Library Sync keeps in a storage service is found in the
/// first folder that has it, the sync service's own first.
#[hegel::test(test_cases = 12)]
fn prop_cloud_folders_and_the_audio_they_hold(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let (dropbox, drive) = (dir.path().join("Dropbox & Co"), dir.path().join("Google Drive"));
    let given = |tc: &hegel::TestCase, p: &Path| match tc.draw(gs::booleans()) {
        true => p.display().to_string(),
        false => String::new(),
    };
    let (d, g) = (given(&tc, &dropbox), given(&tc, &drive));
    let esc = |s: &str| s.replace('&', "&amp;");
    let settings = dir.path().join("rekordbox3.settings");
    std::fs::write(
        &settings,
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<PROPERTIES>\n  <VALUE name=\"DropboxToken\" val=\"x\"/>\n  \
             <VALUE name=\"DropboxSharingPath\" val=\"{}\"/>\n  <VALUE name=\"GoogleDriveSharingPath\" val=\"{}\"></VALUE>\n</PROPERTIES>",
            esc(&d),
            esc(&g)
        ),
    )
    .unwrap();
    let folders = CloudFolders::from_settings(&settings).unwrap();
    assert_eq!(folders.dropbox, (!d.is_empty()).then(|| dropbox.clone()));
    assert_eq!(folders.google_drive, (!g.is_empty()).then(|| drive.clone()));

    // The track's file, in some of the folders.
    let service = tc.draw(gs::sampled_from(vec!["Dropbox", "Google Drive", "googledrive"]));
    let c = cloud_collection(service);
    let synced = c.library.track(205356547).unwrap();
    let in_folder = |folder: &Path| folder.join("rekordbox/contents_2354715273/Floorplan/ngo.mp3");
    let mut has = Vec::new();
    for folder in [&dropbox, &drive] {
        if tc.draw(gs::booleans()) {
            std::fs::create_dir_all(in_folder(folder).parent().unwrap()).unwrap();
            std::fs::write(in_folder(folder), b"mp3").unwrap();
            has.push(folder.clone());
        }
    }
    let google_first = service.to_lowercase().contains("google");
    let order = if google_first {
        [&folders.google_drive, &folders.dropbox]
    } else {
        [&folders.dropbox, &folders.google_drive]
    };
    let want = order.into_iter().flatten().find(|f| has.contains(f)).map(|f| in_folder(f));
    assert_eq!(c.audio_file(synced, &folders), want);
}

/// A copy of rekordbox's folder taken while its WAL holds drawn changes reads with them, the
/// database plain (SQLite's own WAL) or encrypted (its pages' frames encrypted too), and the copy
/// is left as it was.
#[hegel::test(test_cases = 8)]
fn prop_a_collection_reads_with_the_changes_its_wal_holds(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let f = sample();
    let titles: Vec<(&str, String)> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)))
        .map(|i| {
            let id = if i % 2 == 0 { "205356547" } else { "84935471" };
            (id, format!("Changed {i} {}", tc.draw(gs::text().alphabet("ab é").max_size(4))))
        })
        .collect();
    let change = |conn: &Connection| {
        for (id, title) in &titles {
            conn.execute("UPDATE djmdContent SET Title = ?2 WHERE ID = ?1", params![id, title]).unwrap();
        }
    };
    let (db, wal) = match tc.draw(gs::booleans()) {
        false => {
            let path = write(dir.path(), &f.plain());
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA wal_autocheckpoint = 0;").unwrap();
            change(&conn);
            let copied = (std::fs::read(&path).unwrap(), std::fs::read(dir.path().join("master.db-wal")).unwrap());
            drop(conn);
            copied
        }
        true => {
            let codec =
                Codec::with_key(DEFAULT_KEY.as_bytes(), tc.draw(gs::arrays::<_, _, SALT_LEN>(gs::integers::<u8>())));
            let before = f.plain();
            change(&f.conn);
            let after = f.plain();
            let pages = (after.len() / PAGE_SIZE) as u32;
            let changed: Vec<u32> = (1..=pages)
                .filter(|&p| {
                    let at = (p as usize - 1) * PAGE_SIZE;
                    before.get(at..at + PAGE_SIZE) != Some(&after[at..at + PAGE_SIZE])
                })
                .collect();
            let frames: Vec<(u32, u32, Vec<u8>)> = changed
                .iter()
                .enumerate()
                .map(|(i, &p)| {
                    let at = (p as usize - 1) * PAGE_SIZE;
                    let commit = if i + 1 == changed.len() { pages } else { 0 };
                    (p, commit, codec.encrypt_page(p, &after[at..at + PAGE_SIZE]).unwrap())
                })
                .collect();
            (codec.encrypt(&before).unwrap(), loclib_sqlcipher::wal::tests::build(PAGE_SIZE, (1, 2), &frames))
        }
    };
    let copy = dir.path().join("copy");
    std::fs::create_dir(&copy).unwrap();
    std::fs::write(copy.join("master.db"), &db).unwrap();
    std::fs::write(copy.join("master.db-wal"), &wal).unwrap();
    let c = open(&copy.join("master.db"), DEFAULT_KEY).unwrap();
    for id in ["205356547", "84935471"] {
        if let Some((_, want)) = titles.iter().rfind(|(i, _)| *i == id) {
            assert_eq!(&c.library.track(id.parse().unwrap()).unwrap().title, want);
        }
    }
    assert_eq!(std::fs::read(copy.join("master.db")).unwrap(), db, "the copy is left alone");
}

/// rekordbox's columns hold what SQLite lets them: a lookup whose ids are numbers and whose names
/// are text, bytes, a whole number or a fraction still names the track, written as the value
/// reads; numbers stored as fractions (or as text with spaces) are cut to whole ones.
#[hegel::test(test_cases = 20)]
fn prop_values_of_any_sqlite_type_read_as_what_they_say(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let f = Fixture::new();
    // A lookup table with no column types, so SQLite keeps each value as given.
    f.exec("DROP TABLE djmdGenre; CREATE TABLE djmdGenre (ID, Name, rb_local_deleted);");
    let word = tc.draw(gs::text().alphabet("ab Üé&'").min_size(1).max_size(8));
    let bytes = tc.draw(gs::text().alphabet("xy ♪").max_size(6));
    let whole = tc.draw(gs::integers::<i64>());
    let fraction = tc.draw(gs::integers::<i32>().min_value(-99_999).max_value(99_999)) as f64
        + f64::from(tc.draw(gs::integers::<u8>().min_value(1).max_value(99))) / 100.0;
    let names: [(Value, String); 4] = [
        (Value::Text(word.clone()), word),
        (Value::Blob(bytes.clone().into_bytes()), bytes),
        (Value::Integer(whole), whole.to_string()),
        (Value::Real(fraction), fraction.to_string()),
    ];
    // A count rekordbox keeps as text: a fraction is cut, anything else reads as none.
    let plays = tc.draw(gs::integers::<u16>().max_value(5000));
    let (played, want_plays) = match tc.draw(gs::booleans()) {
        true => (format!(" {plays}.{} ", tc.draw(gs::integers::<u8>().max_value(9))), plays),
        false => (tc.draw(gs::sampled_from(vec!["", "many", "1e", "-"])).to_string(), 0),
    };
    let bpm = tc.draw(gs::integers::<u32>().max_value(30_000));
    let size = tc.draw(gs::integers::<u32>());
    let length = tc.draw(gs::integers::<u16>());
    for (i, (name, _)) in names.iter().enumerate() {
        let id = i as i64 + 1;
        f.conn.execute("INSERT INTO djmdGenre (ID, Name) VALUES (?1, ?2)", params![id, name]).unwrap();
        f.track(&id.to_string(), &format!("/Music/{id}.mp3"), "Artist", &format!("Track {id}"));
        f.conn
            .execute(
                "UPDATE djmdContent SET GenreID = ?2, BPM = ?3, FileSize = ?4, Length = ?5, DJPlayCount = ?6 WHERE ID = ?1",
                params![
                    id.to_string(),
                    id.to_string(),
                    f64::from(bpm) + 0.5,
                    f64::from(size) + 0.75,
                    format!(" {length} "),
                    played
                ],
            )
            .unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let c = open(&write(dir.path(), &f.plain()), DEFAULT_KEY).unwrap();
    let got: Vec<&str> = c.library.tracks.iter().map(|t| t.genre.as_str()).collect();
    let want: Vec<&str> = names.iter().map(|(_, s)| s.as_str()).collect();
    assert_eq!(got, want);
    for t in &c.library.tracks {
        let want = (f64::from(bpm) / 100.0, u64::from(size), u32::from(length), want_plays);
        assert_eq!((t.bpm, t.file_size, t.duration_secs, t.play_count), want, "{played:?}");
    }
}

/// A collection SQLite finds damaged (a table's b-tree broken, encrypted or not), with a page
/// that fails authentication, or whose table can't be read (a view on a table that isn't there,
/// or any view, which has no row order) is refused as a parse error naming the file, never read
/// as far as it goes.
#[hegel::test(test_cases = 6)]
fn prop_a_collection_that_cant_be_trusted_is_refused(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let codec = Codec::with_key(DEFAULT_KEY.as_bytes(), tc.draw(gs::arrays::<_, _, SALT_LEN>(gs::integers::<u8>())));
    let tables: Vec<&str> = TABLES.iter().map(|(name, _)| *name).collect();
    // Each kind of damage, once a case.
    for kind in 0..4 {
        let f = sample();
        let (bytes, why) = match kind {
            0 => {
                let table = tc.draw(gs::sampled_from(tables.clone()));
                let root: u32 = f
                    .conn
                    .query_row("SELECT rootpage FROM sqlite_master WHERE name = ?1", [table], |r| r.get(0))
                    .unwrap();
                let mut plain = f.plain();
                let at = (root as usize - 1) * PAGE_SIZE;
                plain[at + 3..at + 5].copy_from_slice(&u16::MAX.to_be_bytes());
                let bytes = if tc.draw(gs::booleans()) { codec.encrypt(&plain).unwrap() } else { plain };
                (bytes, "SQLite finds it damaged".to_string())
            }
            1 => {
                let mut bytes = f.encrypted(DEFAULT_KEY);
                let pages = bytes.len() / PAGE_SIZE;
                let page = tc.draw(gs::integers::<usize>().min_value(2).max_value(pages));
                let at = (page - 1) * PAGE_SIZE + tc.draw(gs::integers::<usize>().max_value(PAGE_SIZE - 1));
                bytes[at] ^= tc.draw(gs::integers::<u8>().min_value(1));
                (bytes, format!("page {page} fails authentication"))
            }
            2 => {
                let table = tc.draw(gs::sampled_from(vec!["djmdArtist", "djmdKey", "djmdCue", "djmdMyTag"]));
                f.exec(&format!("DROP TABLE {table}; CREATE VIEW {table} AS SELECT * FROM gone;"));
                (f.plain(), "no such table: main.gone".to_string())
            }
            // A view, which has no row order to read it in.
            _ => {
                let table = tc.draw(gs::sampled_from(vec!["djmdGenre", "djmdLabel", "djmdKey"]));
                f.exec(&format!("DROP TABLE {table}; CREATE VIEW {table} AS SELECT 1 AS ID;"));
                (f.plain(), "no such column: rowid".to_string())
            }
        };
        let path = write(dir.path(), &bytes);
        let err = open(&path, DEFAULT_KEY).unwrap_err();
        assert!(matches!(err, Error::Parse { .. }), "{err}");
        let message = err.to_string();
        assert!(message.starts_with(&format!("could not parse {}: ", path.display())), "{message}");
        assert!(message.contains(&why), "{message}");
    }
}

/// A copy is taken again while the WAL restarts between reading its header and copying (a
/// checkpoint), so the copy and the WAL read are of one moment: it's the last one after up to two
/// restarts, and after three the read gives up saying so. A WAL that's there but can't be read
/// is an error, an empty one none at all.
#[hegel::test(test_cases = 20)]
fn prop_a_copy_is_taken_again_until_the_wal_holds_still(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let (path, to) = (dir.path().join("master.db"), dir.path().join("copy"));
    let wal_path = dir.path().join("master.db-wal");
    let generation = |n: u32| loclib_sqlcipher::wal::tests::build(PAGE_SIZE, (n, n + 1), &[]);
    std::fs::write(&path, "state 0").unwrap();
    let start = tc.draw(gs::integers::<u8>().max_value(2));
    match start {
        0 => {}
        1 => std::fs::write(&wal_path, b"").unwrap(),
        _ => std::fs::write(&wal_path, generation(0)).unwrap(),
    }
    let restarts = tc.draw(gs::integers::<u32>().max_value(4));
    let copies = std::cell::Cell::new(0u32);
    let got = snapshot(&path, || {
        let copied = std::fs::copy(&path, &to);
        let n = copies.get();
        copies.set(n + 1);
        if n < restarts {
            std::fs::write(&path, format!("state {}", n + 1)).unwrap();
            std::fs::write(&wal_path, generation(2 * n + 2)).unwrap();
        }
        copied
    });
    match restarts {
        0..=2 => {
            let wal = got.unwrap();
            assert_eq!(copies.get(), restarts + 1);
            assert_eq!(std::fs::read_to_string(&to).unwrap(), format!("state {restarts}"));
            let want = match (restarts, start) {
                (0, 0 | 1) => None,
                (0, _) => Some(generation(0)),
                (n, _) => Some(generation(2 * n)),
            };
            assert_eq!(wal, want);
        }
        _ => {
            let err = got.unwrap_err();
            assert!(matches!(err, Error::Changing(_)) && err.to_string().contains("kept changing"), "{err}");
            assert_eq!(copies.get(), 3);
        }
    }
    // A WAL that can't be opened (a link to itself): the read stops there. Windows needs a
    // privilege to make symlinks, so this part runs on Unix only.
    #[cfg(unix)]
    {
        std::fs::remove_file(&wal_path).ok();
        std::os::unix::fs::symlink(&wal_path, &wal_path).unwrap();
        let err = snapshot(&path, || std::fs::copy(&path, &to)).unwrap_err();
        assert!(matches!(&err, Error::Io { path, .. } if *path == wal_path), "{err}");
    }
}

/// rekordbox's settings file is read for the two folders only: a `VALUE` without a name, or
/// another name, is passed over, and a file that isn't well-formed XML (or isn't there) is
/// refused naming it.
#[hegel::test(test_cases = 20)]
fn prop_settings_name_the_cloud_folders_or_are_refused(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let folder = tc.draw(gs::text().alphabet("ab /é").min_size(1).max_size(10));
    let skipped =
        tc.draw(gs::sampled_from(vec!["<VALUE/>", "<VALUE val=\"/x\"/>", "<VALUE name=\"Other\" val=\"/y\"/>"]));
    let settings = dir.path().join("rekordbox3.settings");
    let xml = format!("<PROPERTIES>{skipped}<VALUE name=\"GoogleDriveSharingPath\" val=\"{folder}\"/></PROPERTIES>");
    std::fs::write(&settings, &xml).unwrap();
    let got = CloudFolders::from_settings(&settings).unwrap();
    assert_eq!(got, CloudFolders { dropbox: None, google_drive: Some(PathBuf::from(&folder)) });

    let broken = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => xml.replace("</PROPERTIES>", "</SETTINGS>"),
        1 => format!("{xml}</PROPERTIES>"),
        _ => format!("{xml}<!-- unclosed"),
    };
    std::fs::write(&settings, &broken).unwrap();
    let err = CloudFolders::from_settings(&settings).unwrap_err();
    assert!(matches!(err, Error::Parse { .. }), "{broken}: {err}");
    assert!(err.to_string().contains(&settings.display().to_string()), "{err}");
    let missing = dir.path().join("rekordbox6/rekordbox3.settings");
    assert!(matches!(CloudFolders::from_settings(&missing), Err(Error::Io { path, .. }) if path == missing));
}

/// A key given is the one used, whatever it says; none (or an empty one) falls back to
/// `LOCALLIBRARY_MASTERDB_KEY`, then the key every rekordbox uses. The collection read is the one
/// `LOCALLIBRARY_MASTERDB` names; without it a test reads none, though a release build would read
/// rekordbox's own, in this user's folders or where rekordbox's settings moved it.
#[hegel::test(test_cases = 20)]
fn prop_keys_and_the_collection_default_to_rekordboxs(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let key = tc.draw(gs::text().max_size(70));
    let fallback = std::env::var(KEY_ENV).ok().filter(|k| !k.is_empty()).unwrap_or_else(|| DEFAULT_KEY.to_string());
    let want = if key.is_empty() { fallback.clone() } else { key.clone() };
    assert_eq!(resolve_key(Some(&key)), want);
    assert_eq!(resolve_key(None), fallback);
    #[cfg(target_os = "macos")]
    {
        let home = directories::BaseDirs::new().map(|b| b.home_dir().join("Library/Pioneer/rekordbox/master.db"));
        assert_eq!(rekordbox_path(), moved(settings_path().as_deref(), agent_options_path().as_deref()).or(home));
    }
    let named = tc.draw(gs::booleans()).then(|| std::env::temp_dir().join("named/master.db"));
    // SAFETY: nextest runs each test in a process of its own.
    unsafe {
        match &named {
            Some(path) => std::env::set_var(PATH_ENV, path),
            None => std::env::remove_var(PATH_ENV),
        }
    }
    assert_eq!(default_path(), named, "a test never falls back to rekordbox's own");
}

/// On every computer, `open` refuses rekordbox's own `master.db` (here the sample library, in a
/// made-up home [`places`] gives) unless [`PATH_ENV`] names it, and reads a copy of it elsewhere.
#[test]
fn open_reads_rekordboxs_own_collection_only_when_it_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let place = fake_place(dir.path());
    std::fs::create_dir_all(place.master_db.parent().unwrap()).unwrap();
    std::fs::write(&place.master_db, sample().encrypted(DEFAULT_KEY)).unwrap();
    PLACES.set(Some(vec![place.clone()]));
    // SAFETY: nextest runs each test in a process of its own.
    unsafe { std::env::remove_var(PATH_ENV) };
    let refused = std::panic::catch_unwind(|| open(&place.master_db, DEFAULT_KEY)).unwrap_err();
    let message = refused.downcast_ref::<String>().unwrap();
    assert!(message.contains("rekordbox's own collection") && message.contains(PATH_ENV), "{message}");
    let other = dir.path().join("other.db");
    std::fs::copy(&place.master_db, &other).unwrap();
    assert_eq!(open(&other, DEFAULT_KEY).unwrap().library.tracks.len(), 3);
    unsafe { std::env::set_var(PATH_ENV, &place.master_db) };
    assert_eq!(open(&place.master_db, DEFAULT_KEY).unwrap().library.tracks.len(), 3);
}

/// A release build that nextest started (`cargo nextest run --release`) is a test's too, and so
/// is every program it starts; one nothing started as a test isn't.
#[test]
fn a_release_build_under_nextest_is_a_test() {
    // SAFETY: nextest runs each test in a process of its own.
    unsafe { std::env::set_var("NEXTEST", "1") };
    assert!(started_by_nextest_or(false));
    unsafe { std::env::remove_var("NEXTEST") };
    assert!(!started_by_nextest_or(false));
    assert!(started_by_nextest_or(true));
    assert!(under_test(), "this crate's own tests, in any build");
}

/// A test can't read rekordbox's own `master.db`, however it gets the path, unless [`PATH_ENV`]
/// names it. Here that collection is the sample library, below a made-up home.
#[cfg(target_os = "macos")]
#[test]
fn a_test_reads_rekordboxs_own_collection_only_when_it_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let own = dir.path().join("Library/Pioneer/rekordbox/master.db");
    std::fs::create_dir_all(own.parent().unwrap()).unwrap();
    std::fs::write(&own, sample().encrypted(DEFAULT_KEY)).unwrap();
    // SAFETY: nextest runs each test in a process of its own.
    unsafe {
        std::env::remove_var(PATH_ENV);
        std::env::set_var("HOME", dir.path());
    }
    assert_eq!(rekordbox_path().as_deref(), Some(own.as_path()));
    assert_eq!(default_path(), None);
    let refused = std::panic::catch_unwind(|| open(&own, DEFAULT_KEY)).unwrap_err();
    let message = refused.downcast_ref::<String>().unwrap();
    assert!(message.contains("rekordbox's own collection"), "{message}");
    let other = dir.path().join("elsewhere/master.db");
    std::fs::create_dir(other.parent().unwrap()).unwrap();
    std::fs::copy(&own, &other).unwrap();
    assert_eq!(open(&other, DEFAULT_KEY).unwrap().library.tracks.len(), 3);
    unsafe { std::env::set_var(PATH_ENV, &own) };
    assert_eq!(open(&own, DEFAULT_KEY).unwrap().library.tracks.len(), 3);
}

/// rekordbox's own `master.db` (here a made-up one, on any computer) is refused unless
/// [`PATH_ENV`] names it, under whatever path it's reached; any other is read.
#[test]
fn only_a_named_rekordbox_collection_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let own = dir.path().join("rekordbox/master.db");
    std::fs::create_dir_all(own.parent().unwrap()).unwrap();
    std::fs::write(&own, b"").unwrap();
    let other = dir.path().join("other.db");
    std::fs::write(&other, b"").unwrap();
    // SAFETY: nextest runs each test in a process of its own.
    unsafe { std::env::remove_var(PATH_ENV) };
    refuse_unnamed(&other, std::slice::from_ref(&own));
    refuse_unnamed(&own, &[]);
    let roundabout = dir.path().join("rekordbox/../rekordbox/master.db");
    let refused = std::panic::catch_unwind(|| refuse_unnamed(&roundabout, std::slice::from_ref(&own))).unwrap_err();
    let message = refused.downcast_ref::<String>().unwrap();
    assert!(message.contains("rekordbox's own collection") && message.contains(PATH_ENV), "{message}");
    let linked = dir.path().join("linked.db");
    std::fs::hard_link(&own, &linked).unwrap();
    // macOS reaches every folder of the data volume through /System/Volumes/Data too.
    #[cfg(target_os = "macos")]
    let spellings = [
        linked,
        Path::new("/System/Volumes/Data").join(std::fs::canonicalize(&own).unwrap().strip_prefix("/").unwrap()),
    ];
    #[cfg(not(target_os = "macos"))]
    let spellings = [linked];
    for spelling in spellings {
        assert!(spelling.is_file(), "{}", spelling.display());
        assert!(
            std::panic::catch_unwind(|| refuse_unnamed(&spelling, std::slice::from_ref(&own))).is_err(),
            "{}",
            spelling.display()
        );
    }
    unsafe { std::env::set_var(PATH_ENV, &roundabout) };
    refuse_unnamed(&own, std::slice::from_ref(&own));
}

/// A test reads none of rekordbox's settings unless [`SETTINGS_ENV`] names their folder, even with
/// them in rekordbox's place: not the cloud folders, not where the library moved, not the folder a
/// backup would take.
#[test]
fn a_test_reads_rekordboxs_settings_only_from_the_folder_it_names() {
    let dir = tempfile::tempdir().unwrap();
    let place = fake_place(dir.path());
    let moved_to = dir.path().join("moved");
    let xml = format!(
        "<PROPERTIES><VALUE name=\"masterDbDirectory\" val=\"{}\"/><VALUE name=\"DropboxSharingPath\" \
         val=\"/Dropbox\"/></PROPERTIES>",
        moved_to.display()
    );
    write_settings(&place, &xml, r#"{"options": [["db-path", "/agent/master.db"]]}"#);
    PLACES.set(Some(vec![place.clone()]));
    // SAFETY: nextest runs each test in a process of its own.
    unsafe { std::env::remove_var(SETTINGS_ENV) };
    let read = || (settings_dir(), settings_path(), agent_options_path(), CloudFolders::this_computer());
    assert_eq!(read(), (None, None, None, CloudFolders::default()));
    assert_eq!(rekordbox_path(), Some(place.master_db.clone()));
    unsafe { std::env::set_var(SETTINGS_ENV, &place.pioneer) };
    let cloud = CloudFolders { dropbox: Some("/Dropbox".into()), google_drive: None };
    let paths = (settings_file(&place.pioneer), agent_options_file(&place.pioneer));
    assert_eq!(read(), (Some(place.pioneer.join("rekordbox6")), Some(paths.0), Some(paths.1), cloud));
    assert_eq!(rekordbox_path(), Some(moved_to.join("master.db")));
}

/// A made-up home's rekordbox, as [`Place`]: its `master.db`'s place and its Pioneer folder.
fn fake_place(home: &Path) -> Place {
    Place { master_db: home.join("rekordbox/master.db"), pioneer: home.join("Pioneer") }
}

/// `xml` as the `rekordbox3.settings`, and `json` as the agent's `options.json`, of `place`.
fn write_settings(place: &Place, xml: &str, json: &str) {
    for (path, text) in [(settings_file(&place.pioneer), xml), (agent_options_file(&place.pioneer), json)] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

/// Of rekordbox's settings, the guard takes where the library moved and nothing else: the last
/// absolute `masterDbDirectory`, else the agent's absolute `db-path`. The sign-in tokens and cloud
/// folders beside them don't come out.
#[test]
fn the_guard_takes_only_where_the_library_moved_from_rekordboxs_settings() {
    let dir = tempfile::tempdir().unwrap();
    let place = fake_place(dir.path());
    let (moved_to, agent) = (dir.path().join("moved"), dir.path().join("agent/master.db"));
    let xml = format!(
        "<PROPERTIES><VALUE name=\"masterDbDirectory\" val=\"{}\"/><VALUE name=\"masterDbDirectory\" val=\"relative\"/>\
         <VALUE name=\"DropboxSharingPath\" val=\"/Dropbox\"/><VALUE name=\"DropboxAccessToken\" val=\"secret\"/></PROPERTIES>",
        moved_to.display()
    );
    let json = serde_json::json!({"options": [["login-token", "secret"], ["db-path", agent]]}).to_string();
    write_settings(&place, &xml, &json);
    let (settings, options) = (settings_file(&place.pioneer), agent_options_file(&place.pioneer));
    let taken = settings_values(&settings, &["masterDbDirectory"]).unwrap();
    let dirs = [moved_to.display().to_string(), "relative".into()];
    assert_eq!(taken, dirs.map(|d| ("masterDbDirectory".to_string(), Some(d))));
    assert_eq!(moved(Some(&settings), Some(&options)), Some(moved_to.join("master.db")));
    write_settings(&place, "<PROPERTIES/>", &json);
    assert_eq!(moved(Some(&settings), Some(&options)), Some(agent));
    write_settings(&place, "<PROPERTIES/>", r#"{"options": [["db-path", "relative/master.db"]]}"#);
    assert_eq!(moved(Some(&settings), Some(&options)), None);
}

/// rekordbox's own `master.db` is where its platform keeps it and where its settings say it
/// moved, in every home it has; the guard refuses each.
#[test]
fn a_moved_collection_is_rekordboxs_own_too() {
    let dir = tempfile::tempdir().unwrap();
    let place = fake_place(&dir.path().join("home"));
    let moved_to = dir.path().join("moved");
    write_settings(
        &place,
        &format!("<PROPERTIES><VALUE name=\"masterDbDirectory\" val=\"{}\"/></PROPERTIES>", moved_to.display()),
        "{}",
    );
    let other = fake_place(&dir.path().join("other home"));
    PLACES.set(Some(vec![place.clone(), other.clone()]));
    let own = [moved_to.join("master.db"), place.master_db.clone(), other.master_db.clone()];
    assert_eq!(own_collections(), own);
    // SAFETY: nextest runs each test in a process of its own.
    unsafe { std::env::remove_var(PATH_ENV) };
    for path in &own {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
        assert!(std::panic::catch_unwind(|| refuse_unnamed(path, &own_collections())).is_err(), "{}", path.display());
    }
}

/// With `$HOME` made up, as a test may, rekordbox's own `master.db` is still looked for under the
/// account's real home too (from the password database), as well as the made-up one.
#[cfg(target_os = "macos")]
#[test]
fn rekordboxs_own_is_looked_for_under_the_accounts_home_whatever_home_says() {
    let dir = tempfile::tempdir().unwrap();
    // SAFETY: nextest runs each test in a process of its own.
    unsafe { std::env::set_var("HOME", dir.path()) };
    let account = nix::unistd::User::from_uid(nix::unistd::getuid()).unwrap().unwrap().dir;
    assert_ne!(account, dir.path());
    let homes: Vec<_> = platform_places().into_iter().map(|p| p.master_db).collect();
    let at = |home: &Path| home.join("Library/Pioneer/rekordbox/master.db");
    assert_eq!(homes, [at(dir.path()), at(&account)]);
}

/// Every table the collection is read from has to read: one that can't (a view of a table that's
/// gone) refuses the whole collection, saying why, rather than reading it without that table.
#[test]
fn a_collection_any_of_whose_tables_cant_be_read_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    for table in [
        "djmdContent",
        "djmdArtist",
        "djmdGenre",
        "djmdLabel",
        "djmdKey",
        "djmdAlbum",
        "djmdPlaylist",
        "djmdSongPlaylist",
        "djmdHistory",
        "djmdSongHistory",
        "djmdMyTag",
        "djmdSongMyTag",
        "djmdCue",
        "djmdProperty",
        "djmdCloudProperty",
        "contentFile",
    ] {
        let f = sample();
        f.exec(&format!("DROP TABLE IF EXISTS {table}; CREATE VIEW {table} AS SELECT * FROM gone;"));
        let path = write(dir.path(), &f.plain());
        let message = open(&path, DEFAULT_KEY).unwrap_err().to_string();
        assert!(message.starts_with(&format!("could not parse {}: ", path.display())), "{table}: {message}");
        assert!(message.contains("no such table: main.gone"), "{table}: {message}");
    }
}

/// The key comes from [`KEY_ENV`] when none is given, unless it's empty there.
#[test]
fn a_key_from_the_environment_is_used_unless_its_empty() {
    // SAFETY: nextest runs each test in a process of its own.
    unsafe { std::env::set_var(KEY_ENV, "from the environment") };
    assert_eq!(resolve_key(None), "from the environment");
    assert_eq!(resolve_key(Some("given")), "given");
    // SAFETY: as above.
    unsafe { std::env::set_var(KEY_ENV, "") };
    assert_eq!(resolve_key(None), DEFAULT_KEY);
}
