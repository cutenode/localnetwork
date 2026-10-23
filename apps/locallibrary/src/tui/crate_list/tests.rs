use super::super::app::testing::{library, track};
use super::*;

#[test]
fn adds_once_removes_and_reorders() {
    let root = Path::new("/Volumes/A");
    let mut list = Crate::default();
    assert_eq!(list.add(root, "A", [track(1, "a", "One"), track(2, "b", "Two"), track(1, "a", "One")]), 2);
    assert_eq!(list.add(root, "A", [track(2, "b", "Two"), track(3, "c", "Three")]), 1);
    // The same file on another drive is another track.
    assert_eq!(list.add(Path::new("/Volumes/B"), "B", [track(1, "a", "One")]), 1);
    let ids = |t: &Crate| t.items.iter().map(|x| x.track.id).collect::<Vec<_>>();
    assert_eq!(ids(&list), [1, 2, 3, 1]);
    assert_eq!(list.shift(0, 1), 1);
    assert_eq!(ids(&list), [2, 1, 3, 1]);
    assert_eq!(list.shift(1, -5), 0, "stops at the top");
    assert_eq!(list.shift(0, 9), 3, "and at the bottom");
    assert_eq!(ids(&list), [2, 3, 1, 1]);
    assert_eq!(list.remove(1).map(|t| t.track.id), Some(3));
    assert!(list.remove(9).is_none());
    assert_eq!(list.duration_secs(), 900);
    list.clear();
    assert!(list.is_empty());
}

#[test]
fn refresh_takes_new_details_and_drops_missing_tracks() {
    let root = Path::new("/Volumes/A");
    let mut lib = library();
    let mut list = Crate::default();
    list.add(root, "A", lib.tracks.iter().take(2).cloned());
    list.add(Path::new("/Volumes/B"), "B", [track(7, "x", "Elsewhere")]);
    lib.tracks[0].rating = 1;
    lib.tracks.remove(1);
    list.refresh(root, &lib);
    assert_eq!(list.len(), 2);
    assert_eq!(list.items[0].track.rating, 1);
    assert_eq!(list.items[1].track.title, "Elsewhere");
}

/// Reading a drive again keeps the crate's order, drops its tracks that drive no longer has,
/// gives the rest their new details, and leaves other drives' tracks alone.
#[hegel::test(test_cases = 200)]
fn reading_a_drive_again_keeps_the_crate_in_order(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (a, b) = (Path::new("/A"), Path::new("/B"));
    let lib = library();
    let mut list = Crate::default();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(8)) {
        let root = if tc.draw(gs::booleans()) { a } else { b };
        let t = tc.draw(gs::sampled_from(&lib.tracks[..]));
        list.add(root, "D", [t]);
    }
    // The drive read again: some tracks gone, the others rated.
    let mut again = lib.clone();
    again.tracks.retain(|_| tc.draw(gs::booleans()));
    again.tracks.iter_mut().for_each(|t| t.rating = 4);
    let before: Vec<(PathBuf, Track)> = list.items.iter().map(|t| (t.root.clone(), t.track.clone())).collect();
    list.refresh(a, &again);
    let kept: Vec<_> = (before.iter())
        .filter(|(root, t)| root == b || again.tracks.iter().any(|x| x.file_path == t.file_path))
        .collect();
    assert_eq!(list.len(), kept.len());
    for (now, (root, was)) in list.items.iter().zip(kept) {
        assert_eq!((&now.root, &now.track.file_path), (root, &was.file_path));
        assert_eq!(now.track.rating, if now.root == a { 4 } else { was.rating });
    }
}

/// Moving a track along the crate moves it so far, never past either end, and keeps the others
/// in their order; a place past the end moves nothing.
#[hegel::test(test_cases = 300)]
fn moving_a_crated_track_keeps_the_rest_in_order(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let n = tc.draw(gs::integers::<u32>().max_value(6));
    let mut c = Crate::default();
    c.add(std::path::Path::new("/Volumes/A"), "A", (1..=n).map(|id| track(id, "DJ", &format!("T{id}"))));
    let ids = |c: &Crate| c.items.iter().map(|t| t.track.id).collect::<Vec<_>>();
    let before = ids(&c);
    let i = tc.draw(gs::integers::<usize>().max_value(n as usize + 1));
    let delta = tc.draw(gs::integers::<isize>().min_value(-8).max_value(8));
    let to = c.shift(i, delta);
    if i >= before.len() {
        return assert_eq!((to, ids(&c)), (i, before), "past the end");
    }
    assert_eq!(to, (i as isize + delta).clamp(0, before.len() as isize - 1) as usize);
    let after = ids(&c);
    assert_eq!(after[to], before[i]);
    let (mut a, mut b) = (after.clone(), before.clone());
    a.remove(to);
    b.remove(i);
    assert_eq!(a, b, "the others in their order");
}
