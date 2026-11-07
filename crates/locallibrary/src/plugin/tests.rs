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
