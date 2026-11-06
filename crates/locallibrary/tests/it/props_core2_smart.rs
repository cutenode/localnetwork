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
    let held: Vec<&str> = tc.draw(gs::subsequences(CASES));
    let mut rules = SmartPlaylists::default();
    for path in &held {
        let rule = SmartPlaylist {
            path: path.to_string(),
            filter: "bpm:>=120".into(),
            sort: None,
            limit: None,
            drives: vec![],
        };
        rules.set(rule, &Default::default()).unwrap();
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

/// Paths a collection export's rules and the store's are drawn from: the last two are one path.
const EXPORTED: &[&str] = &["Peak", "Warm up", "Smart/Closing", " Smart / Closing "];

/// The path as rules are told apart by it: names trimmed, empty ones left out.
fn trimmed(path: &str) -> String {
    path.split('/').map(str::trim).filter(|n| !n.is_empty()).collect::<Vec<_>>().join("/")
}

/// Whatever rules the store holds, and whatever a collection export brings for one drive: the
/// export's earlier rules for paths now gone are taken away when limited to that drive alone,
/// each new rule is added or put in place of that drive's own, a path another rule holds is left
/// to it and returned, and a rule whose filter can't be read is refused.
#[hegel::test(test_cases = 200)]
fn prop_an_export_keeps_rules_for_its_drive_only(tc: TestCase) {
    use uuid::Uuid;
    let (drive, other) = (Uuid::from_u128(1), Uuid::from_u128(2));
    let owners = vec![vec![drive], vec![other], vec![], vec![drive, other]];
    let rule = |path: &str, filter: String, drives: Vec<Uuid>| SmartPlaylist {
        path: path.to_string(),
        filter,
        sort: None,
        limit: None,
        drives,
    };
    let mut held = SmartPlaylists::default();
    for path in tc.draw(gs::subsequences(&EXPORTED[..3])) {
        let drives = tc.draw(gs::sampled_from(owners.clone()));
        held.set(rule(path, "bpm:>=120".into(), drives), &Default::default()).unwrap();
    }
    let gone: Vec<String> =
        tc.draw(gs::vecs(gs::sampled_from(EXPORTED)).max_size(3)).into_iter().map(String::from).collect();
    let exported: Vec<SmartPlaylist> = tc
        .draw(gs::vecs(gs::tuples!(gs::sampled_from(EXPORTED), gs::integers::<u8>().max_value(20))).max_size(4))
        .into_iter()
        .map(|(path, n)| rule(path, if n == 0 { "key:H".into() } else { format!("bpm:{n}") }, vec![]))
        .collect();

    let mut model: Vec<SmartPlaylist> = held.playlists.clone();
    model.retain(|p| !(p.drives == [drive] && gone.iter().any(|g| trimmed(g) == trimmed(&p.path))));
    let mut left = Vec::new();
    let mut refused = false;
    for r in &exported {
        let mine = SmartPlaylist { drives: vec![drive], ..r.clone() };
        match model.iter().position(|p| trimmed(&p.path) == trimmed(&r.path)) {
            Some(i) if model[i].drives != [drive] => left.push(r.path.clone()),
            _ if r.filter == "key:H" => {
                refused = true;
                break;
            }
            Some(i) => model[i] = mine,
            None => model.push(mine),
        }
    }
    match held.keep_for_drive(drive, &exported, &gone) {
        Ok(kept) => {
            assert!(!refused, "{exported:?}");
            assert_eq!(kept, left);
            assert_eq!(held.playlists, model);
        }
        Err(e) => assert!(refused, "{e}"),
    }
}
