use super::*;
use crate::model::Playlist;
use loclib_model::test_util::track;
use std::collections::BTreeSet;

fn lib(lists: &[(&str, &[u32])]) -> Library {
    Library {
        tracks: (1..=6).map(|i| track(i, "A", &format!("T{i}"), &format!("/Contents/{i}.mp3"))).collect(),
        playlists: lists
            .iter()
            .enumerate()
            .map(|(n, (name, ids))| Playlist {
                id: n as u32 + 1,
                parent_id: None,
                name: name.to_string(),
                is_folder: false,
                sort_order: n as u32,
                track_ids: ids.to_vec(),
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn added_removed_and_moved_entries() {
    let a = lib(&[("Set", &[1, 2, 3, 4]), ("Same", &[1]), ("Gone", &[2])]);
    let b = lib(&[("Set", &[2, 3, 1, 5, 2]), ("Same", &[1]), ("New", &[])]);
    let d = between(&a, &b, ("a", "b"), None);
    assert_eq!(d.added_playlists, ["New"]);
    assert_eq!(d.removed_playlists, ["Gone"]);
    assert_eq!(d.unchanged_playlists, 1);
    let set = &d.changed_playlists[0];
    let names = |v: &[Entry]| v.iter().map(|e| (e.track.name.clone(), e.position)).collect::<Vec<_>>();
    assert_eq!(names(&set.added), [("A - T5".to_string(), 4), ("A - T2".to_string(), 5)]);
    assert_eq!(names(&set.removed), [("A - T4".to_string(), 4)]);
    // 2 and 3 kept their order; 1 moved from the top to after them.
    assert_eq!(set.moved.len(), 1);
    assert_eq!((set.moved[0].track.name.as_str(), set.moved[0].from, set.moved[0].to), ("A - T1", 1, 3));

    let only = between(&a, &b, ("a", "b"), Some("same"));
    assert!(only.is_empty() && only.unchanged_playlists == 1);
}

#[test]
fn lis() {
    assert_eq!(longest_increasing(&[1, 2, 0]), [true, true, false]);
    assert_eq!(longest_increasing(&[3, 0, 1, 2]), [false, true, true, true]);
    assert!(longest_increasing(&[]).is_empty());
}

/// Two libraries' playlists drawn over the same six tracks, some of them named alike.
#[hegel::test_helper]
fn drawn_lib(tc: &hegel::TestCase) -> Library {
    use hegel::generators as gs;
    let names = ["Set", "Same", "Gone", "New"];
    let lists: Vec<(&str, Vec<u32>)> = tc
        .draw(gs::subsequences(&names))
        .into_iter()
        .map(|n| (n, tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(6)).max_size(4))))
        .collect();
    let refs: Vec<(&str, &[u32])> = lists.iter().map(|(n, ids)| (*n, ids.as_slice())).collect();
    lib(&refs)
}

/// What a comparison lists, as sets: added, removed and changed playlists, and unchanged ones.
fn listed(d: &PlaylistDiff) -> [BTreeSet<String>; 4] {
    let set = |v: &[String]| v.iter().cloned().collect::<BTreeSet<_>>();
    [
        set(&d.added_playlists),
        set(&d.removed_playlists),
        d.changed_playlists.iter().map(|c| c.path.clone()).collect(),
        set(&d.unchanged),
    ]
}

/// Adding the comparison of the drives' other database: nothing is lost, a playlist stays
/// unchanged only where neither comparison shows it differs, and the order of adding doesn't
/// change what's listed. Adding a comparison to itself changes nothing.
#[hegel::test(test_cases = 300)]
fn adding_the_other_databases_comparison_keeps_every_difference(tc: hegel::TestCase) {
    use std::collections::BTreeSet as Set;
    let (a, b, c, d) = (drawn_lib(&tc), drawn_lib(&tc), drawn_lib(&tc), drawn_lib(&tc));
    let (first, second) = (between(&a, &b, ("a", "b"), None), between(&c, &d, ("a", "b"), None));
    let mut ab = first.clone();
    ab.add(second.clone());
    let mut ba = second.clone();
    ba.add(first.clone());
    assert_eq!(listed(&ab), listed(&ba));
    let [added, removed, changed, unchanged] = listed(&ab);
    let [a1, r1, c1, u1] = listed(&first);
    let [a2, r2, c2, u2] = listed(&second);
    assert_eq!(added, &a1 | &a2);
    assert_eq!(removed, &r1 | &r2);
    assert_eq!(changed, &c1 | &c2);
    let differs: Set<String> = added.iter().chain(&removed).chain(&changed).cloned().collect();
    assert_eq!(unchanged, &(&u1 | &u2) - &differs);
    assert_eq!(ab.unchanged_playlists, unchanged.len());
    let mut twice = first.clone();
    twice.add(first.clone());
    assert_eq!(listed(&twice), listed(&first));
    assert_eq!(twice.unchanged_playlists, first.unchanged_playlists);
}
