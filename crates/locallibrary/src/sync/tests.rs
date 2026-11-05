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

/// What one target of a parallel sync reports, as its watcher hears it.
#[derive(Default)]
struct Heard(std::sync::Mutex<Vec<(Scope, String)>>);

impl crate::progress::Progress for Heard {
    fn on_event(&self, _: &Event) {}

    fn on_scoped_event(&self, scope: &Scope, event: &Event) {
        self.0.lock().unwrap().push((scope.clone(), format!("{event:?}")));
    }
}

/// A target of a parallel sync passes on its files and warnings named for the drive, and its copy
/// bytes as they are, all scoped to it; its own phase boundaries and other bytes are dropped.
#[hegel::test(test_cases = 50)]
fn prop_a_targets_events_carry_its_name_and_scope(tc: hegel::TestCase) {
    use crate::progress::Progress;
    use hegel::generators as gs;
    let label = tc.draw(gs::sampled_from(vec!["USB A".to_string(), "B".into(), "é".into()]));
    let message = tc.draw(gs::text().max_size(12));
    let path = tc.draw(gs::sampled_from(vec!["Contents/x.mp3".to_string(), "PIONEER/rekordbox/export.pdb".into()]));
    let bytes = tc.draw(gs::integers::<u64>().max_value(1 << 20));
    let heard = Heard::default();
    let scope = Scope { target: Some(format!("/Volumes/{label}")), step: None };
    let progress = TargetProgress { inner: &heard, label: label.clone(), scope: scope.clone() };
    progress.on_event(&Event::PhaseStart { phase: Phase::Copy, items: 1, bytes });
    progress.on_event(&Event::Item { phase: Phase::Copy, path: path.clone(), bytes });
    progress.on_event(&Event::Warning { message: message.clone() });
    progress.on_event(&Event::Bytes { phase: Phase::Copy, bytes });
    progress.on_event(&Event::Bytes { phase: Phase::Hash, bytes });
    progress.on_event(&Event::PhaseEnd { phase: Phase::Copy });
    let want = [
        format!("{:?}", Event::Item { phase: Phase::Copy, path: format!("{label}: {path}"), bytes }),
        format!("{:?}", Event::Warning { message: format!("{label}: {message}") }),
        format!("{:?}", Event::Bytes { phase: Phase::Copy, bytes }),
    ];
    let heard = heard.0.into_inner().unwrap();
    assert_eq!(heard.iter().map(|(_, e)| e.clone()).collect::<Vec<_>>(), want);
    assert!(heard.iter().all(|(s, _)| *s == scope));
}
