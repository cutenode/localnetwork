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
