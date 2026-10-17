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
/// other plugins', for the same file wherever it is (another drive, exported again); a filter
/// picks one plugin.
#[test]
fn fields_are_kept_by_file_plugin_and_analyzer() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    assert!(store.last_written(None).is_empty(), "nothing kept yet");
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

    let all = store.last_written(None);
    let expected = [
        analyzer("analysis", "0.1.0", "key"),
        analyzer("analysis", "0.1.0", "tempo"),
        analyzer("other", "2.0.0", "score"),
    ];
    assert_eq!(all, expected);
    let got = store.get(&track, &all);
    assert_eq!(got.keys().collect::<Vec<_>>(), ["analysis", "other"]);
    assert_eq!((&got["analysis"]["tempo"], &got["analysis"]["key"], &got["other"]["score"]), (&tempo, &key, &score));
    assert_eq!(store.last_written(Some("other")), all[2..]);
    assert!(store.last_written(Some("nope")).is_empty());

    // The same file on another drive.
    let copy = audio(&dir.path().join("B/Contents/elsewhere"), "track.MP3", b"some audio");
    assert_eq!(store.get(&copy, &all), got);
    assert!(store.get(&dir.path().join("gone.mp3"), &all).is_empty());
}

/// Readers read the versions they ask for; those that don't know read the version that wrote
/// last. A file that changed since has no values.
#[test]
fn versions_are_read_as_asked_and_changed_files_not_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let (a, b) = (audio(dir.path(), "a.mp3", b"first"), audio(dir.path(), "b.mp3", b"second"));
    let (old, new) = (analyzer("analysis", "0.1.0", "tempo"), analyzer("analysis", "0.2.0", "tempo"));
    let (slow, fast) = (fields(&[("bpm", FieldValue::Number(120.0))]), fields(&[("bpm", FieldValue::Number(121.5))]));
    store.put(&a, &old, &slow).unwrap();
    store.put(&b, &old, &slow).unwrap();
    assert_eq!(store.last_written(None), std::slice::from_ref(&old));

    // A new version writes b: a reader of 0.1.0 still has both, the last written is 0.2.0.
    store.put(&b, &new, &fast).unwrap();
    assert_eq!(store.get(&a, std::slice::from_ref(&old))["analysis"]["tempo"], slow);
    assert_eq!(store.get(&b, std::slice::from_ref(&old))["analysis"]["tempo"], slow);
    assert_eq!(store.last_written(None), std::slice::from_ref(&new));
    assert!(store.get(&a, std::slice::from_ref(&new)).is_empty());
    assert_eq!(store.get(&b, std::slice::from_ref(&new))["analysis"]["tempo"], fast);

    // b changes: its values aren't its own any more.
    fs::write(&b, b"second, longer").unwrap();
    assert!(store.get(&b, &[old, new]).is_empty());
}

/// Names that can't be a folder, numbers JSON can't hold and audio that isn't there are
/// refused, writing nothing; a last-written version that names no folder isn't read.
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
    assert!(store.last_written(None).is_empty());

    fs::create_dir_all(store.root.join("odd/x")).unwrap();
    fs::write(store.root.join("odd").join(LAST_WRITTEN), "../..").unwrap();
    assert!(store.last_written(None).is_empty());
}

/// Regression: an analyzer's workers keep values at once, and each rewrote `.version` when it was
/// out of date, so on Windows all but one failed ("Access is denied").
#[test]
fn values_kept_at_once_all_land() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let a = analyzer("sound", "1.0.0", "tempo");
    let tracks: Vec<PathBuf> = (0..16)
        .map(|i| audio(&dir.path().join("A/Contents"), &format!("{i}.mp3"), format!("{i}").as_bytes()))
        .collect();
    let bpm = fields(&[("bpm", FieldValue::Number(128.0))]);
    std::thread::scope(|s| {
        for t in &tracks {
            s.spawn(|| store.put(t, &a, &bpm).unwrap());
        }
    });
    assert_eq!(store.last_written(None), std::slice::from_ref(&a));
    assert!(tracks.iter().all(|t| !store.get(t, std::slice::from_ref(&a)).is_empty()));
}
