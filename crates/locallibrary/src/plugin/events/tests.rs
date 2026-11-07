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

/// For any damage to where suggestions are kept or remembered (text that isn't their JSON), the
/// suggestions can't be read, kept, forgotten or checked against: each fails, naming the file,
/// which is left as it is.
#[hegel::test(test_cases = 20)]
fn prop_suggestions_kept_or_seen_that_dont_read_fail_and_stay(tc: hegel::TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path()).unwrap();
    let damaged = tc.draw(gs::sampled_from(vec!["{", "[1", "{\"a\": 1}", "nul"]));
    let seen = tc.draw(gs::booleans());
    let file = match seen {
        true => seen_file(&store),
        false => suggested_file(&store),
    };
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, damaged).unwrap();
    let names_it = |e: crate::Error| assert!(e.to_string().contains(&file.display().to_string()), "{e}");
    let n = notice("echo", analyze(vec![1]));
    names_it(forget_suggested(&store, &n).unwrap_err());
    if !seen {
        names_it(keep_suggested(&store, &n).unwrap_err());
        names_it(suggested(&store).unwrap_err());
    }
    names_it(unseen(&store, "echo", analyze(vec![1])).unwrap_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), damaged);
}

/// A notice kept that suggests nothing never stands for a suggestion made again, and forgetting
/// it remembers nothing.
#[hegel::test(test_cases = 20)]
fn prop_a_notice_suggesting_nothing_is_no_suggestion_seen(tc: hegel::TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path()).unwrap();
    let message: String = tc.draw(gs::text().max_size(10));
    let plain = PluginNotice { plugin: "echo".into(), message, suggests: None };
    assert_eq!(keep_suggested(&store, &plain).unwrap(), std::slice::from_ref(&plain));
    let tracks: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(6)).max_size(3));
    assert_eq!(unseen(&store, "echo", analyze(tracks.clone())).unwrap(), Some(analyze(tracks)));
    assert_eq!(forget_suggested(&store, &plain).unwrap(), []);
    assert_eq!(read_seen(&store).unwrap(), []);
}

/// Where suggestions are kept can't be made (a file in the plugins' folder's place), locked (a
/// lock nobody may open) or, on Unix, written (a folder the host can't write): keeping one
/// fails, naming where, and what was kept stays.
#[hegel::test(test_cases = 1)]
fn prop_suggestions_that_cant_be_written_fail_to_keep(tc: hegel::TestCase) {
    for how in (0..=2u8).filter(|h| cfg!(unix) || *h < 2).collect::<Vec<u8>>() {
        prop_suggestions_that_cant_be_written_fail_to_keep_case(&tc, how);
    }
}

/// [`prop_suggestions_that_cant_be_written_fail_to_keep`] for one `how`.
#[hegel::test_helper]
fn prop_suggestions_that_cant_be_written_fail_to_keep_case(tc: &hegel::TestCase, how: u8) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path()).unwrap();
    let n = notice("echo", analyze(tc.draw(gs::vecs(gs::integers::<u32>().max_value(6)).max_size(3))));
    let plugins = suggested_file(&store).parent().unwrap().to_path_buf();
    let blocked = match how {
        0 => {
            std::fs::write(&plugins, "").unwrap();
            plugins.clone()
        }
        1 => {
            let lock = plugins.join("suggested.lock");
            std::fs::create_dir_all(&plugins).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::write(&lock, "").unwrap();
                std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o000)).unwrap();
            }
            #[cfg(not(unix))]
            std::fs::create_dir_all(&lock).unwrap();
            lock
        }
        _ => {
            keep_suggested(&store, &n).unwrap();
            read_only(&plugins, true);
            plugins.clone()
        }
    };
    let e = keep_suggested(&store, &n).unwrap_err();
    read_only(&plugins, false);
    assert!(e.to_string().contains(&blocked.display().to_string()), "{e}");
    if how == 2 {
        assert_eq!(suggested(&store).unwrap().notices, [n]);
    }
}

/// Makes the folder `dir` one the host can't write in (on Unix), or can again.
fn read_only(dir: &std::path::Path, only: bool) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if only { 0o500 } else { 0o700 };
        if dir.is_dir() {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).unwrap();
        }
    }
    #[cfg(not(unix))]
    let _ = (dir, only);
}

/// For any suggestions another version kept that this one can't read (an action it doesn't know,
/// or what isn't a notice), they're counted, never offered, and kept as they are, first, as this
/// version keeps its own.
#[hegel::test(test_cases = 20)]
fn prop_suggestions_this_version_cant_read_are_counted_and_kept(tc: hegel::TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path()).unwrap();
    let names: Vec<String> = tc.draw(gs::vecs(gs::from_regex("[a-z]{1,6}\\.[a-z]{1,6}").fullmatch(true)).max_size(3));
    let mut unread: Vec<Value> =
        names.iter().map(|n| json!({ "plugin": "echo", "message": "newer", "action": { "name": n } })).collect();
    unread.push(json!({ "plugin": "echo" }));
    let file = suggested_file(&store);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, serde_json::to_vec(&unread).unwrap()).unwrap();
    let kept = suggested(&store).unwrap();
    assert_eq!((kept.notices, kept.unread), (Vec::new(), unread.len()));
    let n = notice("echo", analyze(vec![1]));
    assert_eq!(keep_suggested(&store, &n).unwrap(), std::slice::from_ref(&n));
    let on_disk: Vec<Value> = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(on_disk[..unread.len()], unread[..]);
}

/// For any notice, what its schema says it holds is what it's written as: `plugin`, `message`,
/// and its suggestion as `operation` or `action`.
#[hegel::test(test_cases = 20)]
fn prop_a_notices_schema_names_what_its_written_as(tc: hegel::TestCase) {
    let schema = serde_json::to_value(schemars::schema_for!(PluginNotice)).unwrap();
    let tracks: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>().max_value(9)).max_size(3));
    let written = serde_json::to_value(notice("echo", analyze(tracks))).unwrap();
    let named = |key: &str| schema.to_string().contains(&format!("\"{key}\""));
    for key in written.as_object().unwrap().keys() {
        assert!(named(key), "{key} isn't in {schema}");
    }
    assert!(named("operation") && named("action"));
}
