use super::*;

#[test]
fn matching_ignores_case_and_spacing() {
    assert!(is_useless("rekordbox", "House 1"));
    assert!(is_useless("Rekordbox ", "house  1"));
    assert!(!is_useless("rekordbox", "House 10"));
    assert!(!is_useless("Someone", "House 1"));
}
