use super::*;

#[test]
fn the_last_mode_the_log_names_counts() {
    let log = b"[261125T022235.047-0600] I/sync mode: null => standalone\n\
        [261126T012209.793-0600] I/other\n[261126T012213.828-0600] I/sync mode: standalone => cloud\n\
        [261126T012213.900-0600] I/done\n";
    assert_eq!(sync_mode(log).as_deref(), Some("cloud"));
    assert_eq!(sync_mode(b"I/sync mode: standalone\n").as_deref(), Some("standalone"));
    assert_eq!(sync_mode(b"nothing\n"), None);
}

/// A log that can't be read counts as sync being on.
#[test]
fn a_log_that_cant_be_read_counts_as_sync() {
    let dir = tempfile::tempdir().unwrap();
    let conn = Connection::open_in_memory().unwrap();
    let why = sync_on(&conn, Some(dir.path())).unwrap().unwrap();
    assert!(why.contains("can't be read"), "{why}");
}

/// A log whose end says nothing is read whole: the mode it last says, further back, counts.
#[test]
fn a_long_log_is_read_whole_when_its_end_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log.log");
    let mut log = b"[261125T022235.047-0600] I/sync mode: null => standalone\n".to_vec();
    log.resize(log.len() + LOG_TAIL as usize + 1, b'\n');
    std::fs::write(&path, &log).unwrap();
    let conn = Connection::open_in_memory().unwrap();
    assert_eq!(sync_on(&conn, Some(&path)).unwrap(), None);
    std::fs::write(&path, vec![b'\n'; LOG_TAIL as usize + 1]).unwrap();
    let why = sync_on(&conn, Some(&path)).unwrap().unwrap();
    assert!(why.contains("never says"), "{why}");
}
