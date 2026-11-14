use super::*;
use loclib_model::test_util::track;

/// Tracks found through one map are the ones finding each in the library finds, a repeated id
/// its first track, and an id no track has fails it, naming the first such.
#[hegel::test(test_cases = 200)]
fn tracks_named_are_those_the_library_finds(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let tracks: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(6)).max_size(8));
    let library = Library {
        tracks: tracks
            .iter()
            .enumerate()
            .map(|(i, &id)| track(id, "A", &i.to_string(), &format!("/{i}.mp3")))
            .collect(),
        ..Library::default()
    };
    let ids: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(7)).max_size(6));
    let each: Result<Vec<&Track>> =
        ids.iter().map(|&id| library.track(id).ok_or_else(|| Error::no_track(id))).collect();
    let text = |r: Result<Vec<&Track>>| {
        r.map(|t| t.iter().map(|t| t.title.clone()).collect::<Vec<_>>()).map_err(|e| e.to_string())
    };
    assert_eq!(text(tracks_named(&library, &ids)), text(each));
}

/// For any settings kept for the host and for plugins (as a settings import keeps them), they're
/// read back as they were kept, each plugin's by its id in order; one plugin's that can't be read
/// fails reading them all, naming its file, and the host's can't be kept where a file is in the
/// plugins' folder's place.
#[hegel::test(test_cases = 30)]
fn prop_settings_kept_are_read_back_or_fail_naming_the_file(tc: hegel::TestCase) {
    use crate::store::Store;
    use hegel::generators::{self as gs, Generator};
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path()).unwrap();
    let value = || gs::integers::<i64>().map(Value::from);
    let host: IndexMap<String, Value> = tc
        .draw(gs::vecs(gs::tuples!(gs::from_regex("[a-z]{1,4}").fullmatch(true), value())).max_size(3))
        .into_iter()
        .collect();
    save_host_settings(&store, &host).unwrap();
    assert_eq!(host_settings(Some(&store)), host);
    let ids: Vec<String> =
        tc.draw(gs::vecs(gs::sampled_from(vec!["a".to_string(), "b".into(), "c".into()])).max_size(3));
    let mut expected: IndexMap<String, IndexMap<String, Value>> = IndexMap::new();
    for id in &ids {
        let kept: IndexMap<String, Value> = [("n".to_string(), Value::from(id.len()))].into_iter().collect();
        save_settings(&store, id, &kept).unwrap();
        expected.insert(id.clone(), kept);
    }
    expected.sort_keys();
    assert_eq!(all_saved_settings(&store).unwrap(), expected);
    if let Some(id) = ids.first() {
        let file = store.root().join("plugins").join(id).join("settings.json");
        std::fs::write(&file, "damaged").unwrap();
        let e = all_saved_settings(&store).unwrap_err();
        assert!(e.to_string().contains(&file.display().to_string()), "{e}");
    }
    std::fs::remove_dir_all(store.root().join("plugins")).unwrap();
    std::fs::write(store.root().join("plugins"), "").unwrap();
    assert!(save_host_settings(&store, &host).is_err());
}

/// For any text in a record a plugin writes, the record as it's shown to the user has no control
/// character but its line breaks and no bidirectional text mark, each written as an escape
/// instead, and is the record's JSON as it is when there's none.
#[hegel::test(test_cases = 200)]
fn prop_a_record_is_shown_without_what_could_hide_it(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let tricky = gs::sampled_from(vec!['\u{1b}', '\u{9b}', '\u{7f}', '\u{202e}', '\u{2066}', '\u{200f}', 'a', '\n']);
    let mut title: String = tc.draw(gs::text().max_size(8));
    title.extend(tc.draw(gs::vecs(tricky).max_size(4)));
    let record = serde_json::json!({ "$type": "fm.plyr.track", "title": title });
    let write = RecordWrite {
        plugin: "Echo".into(),
        account: "@dj.example.com".into(),
        collection: "fm.plyr.track".into(),
        rkey: None,
        swap: None,
        record: record.clone(),
    };
    let shown = write.shown();
    let hiding = |c: char| {
        (c.is_control() && c != '\n')
            || matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    };
    assert!(!shown.chars().any(hiding), "{shown:?}");
    let pretty = serde_json::to_string_pretty(&record).unwrap();
    if !pretty.chars().any(hiding) {
        assert_eq!(shown, pretty);
    }
    for c in pretty.chars().filter(|c| hiding(*c)) {
        assert!(shown.contains(&format!("\\u{:04x}", c as u32)), "{c:?} in {shown:?}");
    }
}
