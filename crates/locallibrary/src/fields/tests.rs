use std::fs;

use super::*;

use filetime::FileTime;

fn fields(pairs: &[(&str, FieldValue)]) -> Fields {
    pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
}

fn audio(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::create_dir_all(dir).unwrap();
    fs::write(&path, bytes).unwrap();
    filetime::set_file_mtime(&path, FileTime::from_unix_time(1_790_000_000, 0)).unwrap();
    path
}

fn analyzer(plugin: &str, version: &str, analyzer: &str) -> Analyzer {
    Analyzer { plugin: plugin.into(), version: version.into(), analyzer: analyzer.into() }
}

fn store(dir: &Path) -> FieldStore {
    FieldStore::in_store(&Store::open_or_init(dir.join("store")).unwrap())
}

/// Each analyzer's values come back with their types, beside the plugin's other analyzers' and
/// other plugins', for the same file wherever it is (another drive, exported again), of the
/// analyzers asked for.
#[test]
fn fields_are_kept_by_file_plugin_and_analyzer() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let track = audio(&dir.path().join("A/Contents"), "Track.mp3", b"some audio");
    let tempo = fields(&[
        ("bpm", FieldValue::Number(128.0)),
        ("beats", FieldValue::Integer(512)),
        ("vocal", FieldValue::Boolean(true)),
    ]);
    let key = fields(&[
        ("key", FieldValue::String("8A".into())),
        ("moods", FieldValue::Strings(vec!["dark".into(), "driving".into()])),
    ]);
    let score = fields(&[("score", FieldValue::Number(0.5))]);
    store.put(&track, &analyzer("analysis", "0.1.0", "tempo"), &tempo).unwrap();
    store.put(&track, &analyzer("analysis", "0.1.0", "key"), &key).unwrap();
    store.put(&track, &analyzer("other", "2.0.0", "score"), &score).unwrap();

    let all = [
        analyzer("analysis", "0.1.0", "tempo"),
        analyzer("analysis", "0.1.0", "key"),
        analyzer("other", "2.0.0", "score"),
    ];
    assert!(store.get(&track, &all[..0]).is_empty(), "none asked for");
    let got = store.get(&track, &all);
    assert_eq!(got.keys().collect::<Vec<_>>(), ["analysis", "other"]);
    assert_eq!((&got["analysis"]["tempo"], &got["analysis"]["key"], &got["other"]["score"]), (&tempo, &key, &score));
    assert_eq!(store.get(&track, &all[2..]).keys().collect::<Vec<_>>(), ["other"]);
    assert!(store.get(&track, &[analyzer("nope", "0.1.0", "tempo")]).is_empty());

    // The same file on another drive.
    let copy = audio(&dir.path().join("B/Contents/elsewhere"), "track.MP3", b"some audio");
    assert_eq!(store.get(&copy, &all), got);
    assert!(store.get(&dir.path().join("gone.mp3"), &all).is_empty());
}

/// Readers read the versions they ask for. A file that changed since has no values.
#[test]
fn versions_are_read_as_asked_and_changed_files_not_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let (a, b) = (audio(dir.path(), "a.mp3", b"first"), audio(dir.path(), "b.mp3", b"second"));
    let (old, new) = (analyzer("analysis", "0.1.0", "tempo"), analyzer("analysis", "0.2.0", "tempo"));
    let (slow, fast) = (fields(&[("bpm", FieldValue::Number(120.0))]), fields(&[("bpm", FieldValue::Number(121.5))]));
    store.put(&a, &old, &slow).unwrap();
    store.put(&b, &old, &slow).unwrap();

    // A new version writes b: a reader of 0.1.0 still has both.
    store.put(&b, &new, &fast).unwrap();
    assert_eq!(store.get(&a, std::slice::from_ref(&old))["analysis"]["tempo"], slow);
    assert_eq!(store.get(&b, std::slice::from_ref(&old))["analysis"]["tempo"], slow);
    assert!(store.get(&a, std::slice::from_ref(&new)).is_empty());
    assert_eq!(store.get(&b, std::slice::from_ref(&new))["analysis"]["tempo"], fast);

    // b changes: its values aren't its own any more.
    fs::write(&b, b"second, longer").unwrap();
    assert!(store.get(&b, &[old, new]).is_empty());
}

/// Names that can't be a folder, numbers JSON can't hold and audio that isn't there are
/// refused, writing nothing.
#[test]
fn bad_fields_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let track = audio(dir.path(), "a.mp3", b"audio");
    let ok = fields(&[("bpm", FieldValue::Number(120.0))]);
    for bad in [
        analyzer("../up", "0.1.0", "tempo"),
        analyzer("", "0.1.0", "tempo"),
        analyzer("analysis", ".version", "tempo"),
        analyzer("analysis", "1/2", "tempo"),
        analyzer("analysis", "0.1.0", ""),
    ] {
        let e = store.put(&track, &bad, &ok).unwrap_err();
        assert!(matches!(e, Error::Invalid(ref m) if m.contains("isn't a plugin id, version or analyzer")), "{e}");
    }
    let tempo = analyzer("analysis", "0.1.0", "tempo");
    let nan = fields(&[("bpm", FieldValue::Number(f64::NAN))]);
    assert!(matches!(store.put(&track, &tempo, &nan), Err(Error::Invalid(m)) if m.contains("\"bpm\"")));
    assert!(matches!(store.put(&dir.path().join("gone.mp3"), &tempo, &ok), Err(Error::NotFound(_))));
    assert!(!store.root.exists(), "nothing was written");
}
