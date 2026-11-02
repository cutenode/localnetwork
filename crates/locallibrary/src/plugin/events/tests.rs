use hegel::generators as gs;
use loclib_plugin::AnalyzeAction;

use super::*;

/// The analyzer `size`'s action on drive `USB`, on `tracks` (none: every one).
fn analyze(tracks: Vec<u32>) -> Suggestion {
    Suggestion::Action(HostAction::Analyze(AnalyzeAction { analyzer: "size".into(), drive: "USB".into(), tracks }))
}

fn notice(plugin: &str, suggests: Suggestion) -> PluginNotice {
    PluginNotice { plugin: plugin.into(), message: "new".into(), suggests: Some(suggests) }
}

/// For any suggestions of one action kept (waiting) and forgotten (reviewed or dismissed), a new
/// one of it keeps the tracks none of them had, and is none when that leaves nothing or one of
/// them was on every track. Another plugin's is as it is.
#[hegel::test(test_cases = 50)]
fn a_suggestion_keeps_what_wasnt_suggested_before(tc: hegel::TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path()).unwrap();
    let tracks = || gs::vecs(gs::integers::<u32>().min_value(1).max_value(6)).max_size(4);
    let changes: Vec<(bool, Vec<u32>)> =
        tc.draw(gs::vecs(gs::tuples!(gs::booleans(), tracks())).max_size(MAX_SUGGESTED - 1));
    // Each kept or forgotten had these tracks, or every one.
    let (mut had, mut every) = (FxHashSet::default(), false);
    for (keep, tracks) in changes {
        let n = notice("echo", analyze(tracks.clone()));
        match keep {
            true => keep_suggested(&store, &n).unwrap(),
            false => forget_suggested(&store, &n).unwrap(),
        };
        every |= tracks.is_empty();
        had.extend(tracks);
    }
    let asked: Vec<u32> = tc.draw(tracks());
    let new: Vec<u32> = asked.iter().copied().filter(|t| !had.contains(t)).collect();
    let expected = match asked.is_empty() {
        true => (!every).then(|| analyze(Vec::new())),
        false => (!every && !new.is_empty()).then(|| analyze(new)),
    };
    assert_eq!(unseen(&store, "echo", analyze(asked.clone())).unwrap(), expected);
    assert_eq!(unseen(&store, "other", analyze(asked.clone())).unwrap(), Some(analyze(asked)));
}

/// An operation reviewed or dismissed, or waiting, is none when suggested again; with other params
/// it's as it is.
#[test]
fn an_operation_suggested_before_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path()).unwrap();
    let op = |name: &str| Suggestion::Operation(Suggested { name: "playlist".into(), params: json!({ "name": name }) });
    keep_suggested(&store, &notice("echo", op("a"))).unwrap();
    forget_suggested(&store, &notice("echo", op("b"))).unwrap();
    assert_eq!(unseen(&store, "echo", op("a")).unwrap(), None);
    assert_eq!(unseen(&store, "echo", op("b")).unwrap(), None);
    assert_eq!(unseen(&store, "echo", op("c")).unwrap(), Some(op("c")));
}

/// What's remembered of one action is one entry, its tracks added up, the newest last; past
/// [`MAX_SEEN`] actions the oldest go.
#[test]
fn what_was_seen_is_one_entry_an_action_the_newest_kept() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path()).unwrap();
    forget_suggested(&store, &notice("echo", analyze(vec![1, 2]))).unwrap();
    forget_suggested(&store, &notice("echo", analyze(vec![2, 3]))).unwrap();
    let seen = read_seen(&store).unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].tracks, [1, 2, 3]);
    for n in 0..MAX_SEEN {
        let other = Suggestion::Operation(Suggested { name: "playlist".into(), params: json!({ "n": n }) });
        forget_suggested(&store, &notice("echo", other)).unwrap();
    }
    let seen = read_seen(&store).unwrap();
    assert_eq!(seen.len(), MAX_SEEN);
    assert!(seen.iter().all(|s| s.tracks.is_empty()), "the action went first");
    assert_eq!(unseen(&store, "echo", analyze(vec![1])).unwrap(), Some(analyze(vec![1])));
}
