use super::*;

#[test]
fn tag_lists_are_recognised_by_name() {
    assert!(is_tag_list("TAG LIST 001"));
    assert!(is_tag_list("Folder/Tag List 2"));
    assert!(!is_tag_list("Friday"));
}

#[test]
fn player_only_needs_changes_and_nothing_else() {
    let mut c = DriveChanges::default();
    assert!(!c.player_only());
    c.history_lists.push("HISTORY 001".into());
    assert!(c.player_only());
    assert_eq!(c.describe(), "1 history list(s)");
    c.other.push("track added: x".into());
    assert!(!c.player_only());
}
