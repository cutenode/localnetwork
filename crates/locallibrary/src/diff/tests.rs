use super::*;
use crate::model::Playlist;
use loclib_model::test_util::track;

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
