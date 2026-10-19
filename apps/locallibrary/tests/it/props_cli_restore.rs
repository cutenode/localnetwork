//! Property tests of `restore --playlist` on a folder: what the snapshot had in it comes back, and
//! what was made in it since is left as it is, and named.

use std::collections::BTreeSet;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use serde_json::Value;

use crate::support::Env;

/// The playlists the folder `Gigs` may hold.
const NIGHTS: [&str; 3] = ["Friday", "Saturday", "Sunday"];

/// Playlists in a folder on drive A when it's pulled, then some of them deleted and others made in
/// it: restoring the folder from that snapshot plans to leave exactly the ones made since, saying
/// so as its JSON does (or there's nothing to do when the folder is as it was); restored, the
/// folder holds the snapshot's playlists and the ones made since.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn restoring_a_folder_leaves_what_was_made_since(tc: TestCase) {
    let fx = Env::from_template();
    let before: Vec<&str> = tc.draw(gs::subsequences(NIGHTS.to_vec()).min_size(1));
    for night in &before {
        fx.ok(&["--yes", "playlist", "new", "A", &format!("Gigs/{night}"), "--playlist", "Set"]);
    }
    fx.ok(&["pull", "A"]);
    let deleted: Vec<&str> = tc.draw(gs::subsequences(before.clone()));
    for night in &deleted {
        fx.ok(&["--yes", "playlist", "delete", "A", &format!("Gigs/{night}")]);
    }
    let others: Vec<&str> = NIGHTS.iter().copied().filter(|n| !before.contains(n)).collect();
    let made: Vec<&str> = tc.draw(gs::subsequences(others));
    for night in &made {
        fx.ok(&["--yes", "playlist", "new", "A", &format!("Gigs/{night}")]);
    }

    let args = ["restore", "A", "--playlist", "Gigs", "--dry-run"];
    let v: Value = serde_json::from_str(&fx.ok(&[&["--json"][..], &args].concat())).unwrap();
    let out = fx.ok(&args);
    let said = format!("before {before:?}, deleted {deleted:?}, made {made:?}\n{out}\n{v}");
    let p = &v["plan"];
    let since: Vec<String> = made.iter().map(|n| format!("Gigs/{n}")).collect();
    assert_eq!(p["untouched"], serde_json::json!(since), "{said}");
    match since.is_empty() {
        true => assert!(!out.contains("Left as they are"), "{said}"),
        false => {
            assert!(out.contains(&format!("Left as they are (not in the snapshot): {}", since.join(", "))), "{said}")
        }
    }
    assert_eq!(p["noop"] == true, deleted.is_empty(), "{said}");
    if deleted.is_empty() {
        assert!(out.contains("Gigs on A is already as it was"), "{said}");
        return;
    }
    fx.ok(&["--yes", "restore", "A", "--playlist", "Gigs"]);
    let listed = fx.ok(&["playlist", "ls", "A"]);
    let holds: BTreeSet<&str> = NIGHTS.iter().copied().filter(|n| listed.contains(n)).collect();
    let want: BTreeSet<&str> = before.iter().chain(&made).copied().collect();
    assert_eq!(holds, want, "{listed}\n{said}");
}
