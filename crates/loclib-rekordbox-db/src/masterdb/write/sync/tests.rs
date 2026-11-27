use super::*;

/// The mode the agent's last run names counts: what earlier runs said doesn't, and a log with no
/// run's start in it says nothing.
#[test]
fn the_mode_the_last_run_names_counts() {
    let log = b"[261125T022235.047-0600] I/main\n  rekordboxAgent/3.1.4 (darwin_arm64; 27.0.0)\n\
        [261125T022235.047-0600] I/sync mode: null => standalone\n\
        [261126T012209.793-0600] I/main\n  rekordboxAgent/3.1.4 (darwin_arm64; 27.0.0)\n\
        [261126T012213.828-0600] I/sync mode: standalone => cloud\n[261126T012213.900-0600] I/done\n";
    assert_eq!(last_run(log), Some(Some("cloud".into())));
    assert_eq!(last_run(b"  rekordboxAgent/3\nI/sync mode: standalone\n"), Some(Some("standalone".into())));
    let silent = [&log[..], b"[261127T000000.000-0600] I/main\n  rekordboxAgent/3.1.5\nI/done\n"].concat();
    assert_eq!(last_run(&silent), Some(None));
    assert_eq!(last_run(b"I/sync mode: null => standalone\n"), None);
}

/// A log that can't be read counts as sync being on.
#[test]
fn a_log_that_cant_be_read_counts_as_sync() {
    let dir = tempfile::tempdir().unwrap();
    let conn = Connection::open_in_memory().unwrap();
    let why = sync_on(&conn, Some(dir.path()), dir.path()).unwrap().unwrap();
    assert!(why.contains("can't be read"), "{why}");
}

/// A log whose end holds no run's start is read whole: the mode its last run says, further back,
/// counts.
#[test]
fn a_long_log_is_read_whole_when_its_end_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let master = dir.path().join("master.db");
    std::fs::write(&master, b"").unwrap();
    let path = dir.path().join("log.log");
    let mut log = b"  rekordboxAgent/3.1.4\n[261125T022235.047-0600] I/sync mode: null => standalone\n".to_vec();
    log.resize(log.len() + LOG_TAIL as usize + 1, b'\n');
    std::fs::write(&path, &log).unwrap();
    let conn = Connection::open_in_memory().unwrap();
    assert_eq!(sync_on(&conn, Some(&path), &master).unwrap(), None);
    std::fs::write(&path, vec![b'\n'; LOG_TAIL as usize + 1]).unwrap();
    let why = sync_on(&conn, Some(&path), &master).unwrap().unwrap();
    assert!(why.contains("doesn't say"), "{why}");
}
