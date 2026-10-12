use super::*;

#[test]
fn paths_are_summarised_by_their_top_two_folders() {
    let keys = ["Contents/A/x.mp3", "Contents/A/y.mp3", "Contents/B/z.mp3", "PIONEER/rekordbox/export.pdb", "top.mp3"];
    let summary = summarize_paths(keys.into_iter().map(String::from));
    let want = [("Contents/A", 2), ("Contents/B", 1), ("PIONEER/rekordbox", 1), ("top.mp3", 1)];
    assert_eq!(summary.into_iter().collect::<Vec<_>>(), want.map(|(k, n)| (k.to_string(), n)));
}

/// A plan's paths grouped by their top two folders: every path counted once, under its own
/// group, and the groups in order.
#[hegel::test(test_cases = 200)]
fn every_path_is_counted_under_its_top_folders(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let part = || gs::sampled_from(vec!["Contents", "PIONEER", "A", "b", "é"]);
    let paths: Vec<String> = tc
        .draw(gs::vecs(gs::vecs(part()).min_size(1).max_size(4)).max_size(8))
        .into_iter()
        .map(|parts| parts.join("/"))
        .collect();
    let summary = summarize_paths(paths.iter().cloned());
    assert_eq!(summary.values().sum::<usize>(), paths.len());
    assert!(summary.keys().is_sorted());
    for (group, n) in &summary {
        let under = paths.iter().filter(|p| p.split('/').take(2).collect::<Vec<_>>().join("/") == *group).count();
        assert_eq!(under, *n, "{group}");
        assert!(group.split('/').count() <= 2);
    }
}
