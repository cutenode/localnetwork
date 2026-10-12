//! History lists OneLibrary keeps in folders: read with their folder paths (a list whose folder
//! is gone still counts, at the top), and found by that path in any case, by name ignoring case
//! when only one has it, or by a session id of four characters or more.

use crate::common;

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::onelibrary::OneLibrary;
use locallibrary::paths;
use locallibrary::plays;

#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn history_lists_in_folders_are_found_by_path_name_or_session(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    common::onelibrary_only(&root);
    let mut one = OneLibrary::open_root(&root).unwrap();
    let tracks: Vec<u32> = one.library().unwrap().tracks.iter().map(|t| t.id).collect();
    let folder = one.create_history("2026", 0, true).unwrap();
    let nested = one.create_history("HISTORY 001", folder, false).unwrap();
    let top = one.create_history("HISTORY 002", 0, false).unwrap();
    // A list whose folder is gone.
    let stray = one.create_history("HISTORY 003", 9_999, false).unwrap();
    // Another list sharing a name, in the folder or at the top.
    let shared = tc.draw(gs::booleans());
    let twin = one.create_history("HISTORY 002", if shared { folder } else { 0 }, false).unwrap();
    for (list, n) in [(nested, 1), (top, 2), (stray, 1), (twin, 1)] {
        for t in tracks.iter().cycle().take(n) {
            one.add_to_history(list, *t).unwrap();
        }
    }
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();

    let lists = plays::drive_history(&Device::open(&root).unwrap()).unwrap().lists;
    let mut found: Vec<&str> = lists.iter().map(|l| l.path.as_str()).collect();
    found.sort();
    let twin_path = if shared { "2026/HISTORY 002" } else { "HISTORY 002" };
    let mut want = vec!["2026/HISTORY 001", "HISTORY 002", "HISTORY 003", twin_path];
    want.sort();
    assert_eq!(found, want);

    let path_of = |q: &str| plays::find_list(&lists, q).map(|l| l.path.clone());
    let shout = |s: &str| {
        s.chars().map(|c| if tc.draw(gs::booleans()) { c.to_ascii_lowercase() } else { c }).collect::<String>()
    };
    assert_eq!(path_of("/2026/HISTORY 001/").as_deref(), Some("2026/HISTORY 001"));
    assert_eq!(path_of(&shout("HISTORY 001")).as_deref(), Some("2026/HISTORY 001"), "by name, once");
    assert_eq!(path_of(&shout("HISTORY 003")).as_deref(), Some("HISTORY 003"));
    // Two lists are called HISTORY 002: its path finds the one at the top.
    assert_eq!(path_of(&shout("HISTORY 002")).as_deref(), Some("HISTORY 002"));
    let session = lists.iter().find(|l| l.path == "HISTORY 003").unwrap().fingerprint();
    let len = tc.draw(gs::integers::<usize>().min_value(4).max_value(session.len()));
    assert_eq!(path_of(&session[..len].to_uppercase()).as_deref(), Some("HISTORY 003"));
    assert_eq!(path_of(&session[..3]), None, "too short to be a session");
}
