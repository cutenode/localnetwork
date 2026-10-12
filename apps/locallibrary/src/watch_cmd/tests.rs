use super::*;

/// The installed watch syncs a drive's group after pulling it, unless told not to.
#[test]
fn the_installed_watch_syncs_groups_unless_told_not_to() {
    assert_eq!(agent_args(&None, false, SETTLE).unwrap(), ["watch", "--pull", "--sync-group"]);
    assert_eq!(agent_args(&None, true, 9).unwrap(), ["watch", "--pull", "--settle", "9"]);
}
