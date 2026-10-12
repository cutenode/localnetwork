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

/// What a library's changes are sorted into, against a model: history lists new or grown and
/// TAG LIST playlists added are what players do; history lists removed, playlists removed or
/// added, and tracks added are not, and each is said. Sorted from both databases, a change counts
/// once.
#[hegel::test(test_cases = 200)]
fn library_changes_are_sorted_as_players_make_them_or_not(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use loclib_model::test_util::track;
    let list = |id: u32, name: &str, track_ids: Vec<u32>| crate::model::Playlist {
        id,
        parent_id: None,
        name: name.into(),
        is_folder: false,
        sort_order: id,
        track_ids,
    };
    let tracks: Vec<crate::model::Track> =
        (1..=3).map(|id| track(id, "A", &format!("T{id}"), &format!("/Contents/{id}.mp3"))).collect();
    let then = Library {
        tracks: tracks.clone(),
        playlists: vec![list(1, "Friday", vec![1]), list(2, "Peak", vec![2])],
        history: vec![list(10, "HISTORY 001", vec![1]), list(11, "HISTORY 002", vec![2])],
        ..Default::default()
    };
    let mut now = then.clone();
    let (mut history, mut tags, mut said) = (Vec::new(), Vec::new(), Vec::new());
    if tc.draw(gs::booleans()) {
        now.history.push(list(12, "HISTORY 003", vec![3]));
        history.push("HISTORY 003".to_string());
    }
    if tc.draw(gs::booleans()) {
        now.history[1].track_ids.push(3);
        history.push("HISTORY 002".to_string());
    }
    if tc.draw(gs::booleans()) {
        now.history.remove(0);
        said.push("history list removed: HISTORY 001".to_string());
    }
    if tc.draw(gs::booleans()) {
        now.playlists.push(list(3, "TAG LIST 001", vec![1, 2]));
        tags.push("TAG LIST 001".to_string());
    }
    if tc.draw(gs::booleans()) {
        now.playlists.retain(|p| p.id != 2);
        said.push("playlist removed: Peak".to_string());
    }
    if tc.draw(gs::booleans()) {
        now.playlists.push(list(4, "Saturday", vec![3]));
        said.push("playlist added: Saturday".to_string());
    }
    if tc.draw(gs::booleans()) {
        now.tracks.push(track(4, "B", "New", "/Contents/4.mp3"));
        said.push("track added: B - New".to_string());
    }
    let mut c = library_changes(&then, &now);
    c.history_lists.sort();
    history.sort();
    said.sort();
    let mut other = c.other.clone();
    other.sort();
    assert_eq!((&c.history_lists, &c.tag_lists, &other), (&history, &tags, &said));
    assert_eq!(c.player_only(), said.is_empty() && !(history.is_empty() && tags.is_empty()));
    assert_eq!(c.describe() == "nothing", history.is_empty() && tags.is_empty());

    // Both databases making the same changes: each counted once. A player that writes only to
    // OneLibrary adds there what export.pdb lacks.
    let both = Databases { pdb: Some(then.clone()), plus: Some(then), plus_error: None, images: Vec::new() };
    let mut plus = now.clone();
    if tc.draw(gs::booleans()) {
        plus.history.push(list(13, "HISTORY 009", vec![1]));
        plus.playlists.push(list(5, "TAG LIST 009", vec![1]));
        history.push("HISTORY 009".to_string());
        history.sort();
        tags.push("TAG LIST 009".to_string());
    }
    let after = Databases { pdb: Some(now), plus: Some(plus), plus_error: None, images: Vec::new() };
    let mut twice = database_changes(&both, &after);
    twice.history_lists.sort();
    twice.other.sort();
    assert_eq!((twice.history_lists, twice.tag_lists, twice.other), (history, tags, said));
}
