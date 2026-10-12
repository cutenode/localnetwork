use super::*;

#[test]
fn paths_are_summarised_by_their_top_two_folders() {
    let keys = ["Contents/A/x.mp3", "Contents/A/y.mp3", "Contents/B/z.mp3", "PIONEER/rekordbox/export.pdb", "top.mp3"];
    let summary = summarize_paths(keys.into_iter().map(String::from));
    let want = [("Contents/A", 2), ("Contents/B", 1), ("PIONEER/rekordbox", 1), ("top.mp3", 1)];
    assert_eq!(summary.into_iter().collect::<Vec<_>>(), want.map(|(k, n)| (k.to_string(), n)));
}
