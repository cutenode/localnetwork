//! Removing a rule by a path that matches several only ignoring case: refused, naming each, and
//! nothing removed; by its exact path, just that one goes.

use std::collections::BTreeSet;

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::smart::{SmartPlaylist, SmartPlaylists};

/// One path written in the cases rules can be told apart by.
const CASES: &[&str] = &["Smart/Peak", "smart/peak", "SMART/PEAK", "Smart/PEAK", "sMART/pEAK"];

#[hegel::test(test_cases = 100)]
fn a_path_matching_several_rules_only_ignoring_case_removes_none(tc: TestCase) {
    let held: Vec<&str> = CASES.iter().copied().filter(|_| tc.draw(gs::booleans())).collect();
    let mut rules = SmartPlaylists::default();
    for path in &held {
        let rule = SmartPlaylist {
            path: path.to_string(),
            filter: "bpm:>=120".into(),
            sort: None,
            limit: None,
            drives: vec![],
        };
        rules.set(rule).unwrap();
    }
    let asked = tc.draw(gs::sampled_from(CASES));
    let before: BTreeSet<String> = rules.playlists.iter().map(|p| p.path.clone()).collect();
    let r = rules.remove(asked);
    match (held.contains(&asked), held.len()) {
        (true, _) | (false, 1) => {
            let removed = r.unwrap();
            assert!(removed.path == asked || held.len() == 1, "{} for {asked}", removed.path);
            assert_eq!(rules.playlists.len(), held.len() - 1);
        }
        (false, 0) => assert!(r.unwrap_err().to_string().contains("smart playlist")),
        (false, n) => {
            let e = r.unwrap_err().to_string();
            assert!(e.contains(&format!("matches {n} smart playlists")), "{e}");
            assert!(held.iter().all(|p| e.contains(p)), "{e}");
            let after: BTreeSet<String> = rules.playlists.iter().map(|p| p.path.clone()).collect();
            assert_eq!(after, before);
        }
    }
}
