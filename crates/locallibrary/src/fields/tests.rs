use std::fs;

use super::*;
use crate::device::Device;

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

/// A library's values, named by plugin and field (`sound.bpm`), from the analyzers asked for and
/// by each track's audio file on the drive: a track whose file has none, or isn't there, has
/// none; with no store, or no analyzer, no track has any, and the fields are still declared.
#[test]
fn a_librarys_values_are_named_by_plugin_and_field() {
    let dir = tempfile::tempdir().unwrap();
    let st = Store::open_or_init(dir.path().join("store")).unwrap();
    let store = FieldStore::in_store(&st);
    let drive = dir.path().join("USB");
    let a = audio(&drive.join("Contents"), "a.mp3", b"first");
    audio(&drive.join("Contents"), "b.mp3", b"second");
    let tempo = analyzer("sound", "0.1.0", "tempo");
    store.put(&a, &tempo, &fields(&[("bpm", FieldValue::Number(120.0))])).unwrap();
    store.put(&a, &analyzer("other", "0.1.0", "x"), &fields(&[("bpm", FieldValue::Number(1.0))])).unwrap();
    let device = Device::open(&drive).unwrap();
    let track = |id, file: &str| loclib_model::test_util::track(id, "A", "T", file);
    let library = Library {
        tracks: vec![track(1, "/Contents/a.mp3"), track(2, "/Contents/b.mp3"), track(3, "/Contents/gone.mp3")],
        ..Default::default()
    };
    let declared: Declared = [("sound.bpm".to_string(), FieldType::Number)].into_iter().collect();
    let analyzers = [tempo];
    let source = Source { store: Some(&st), analyzers: &analyzers, declared: &declared };
    let values = TrackValues::read(source, &device, &library);
    assert_eq!(values.declared, declared);
    assert_eq!(values.tracks.keys().copied().collect::<Vec<_>>(), [1]);
    assert_eq!(values.get(1, "sound.bpm"), Some(&FieldValue::Number(120.0)));
    assert_eq!(values.get(1, "other.bpm"), None, "not asked for");
    assert_eq!(values.get(2, "sound.bpm"), None);
    for source in [Source { store: None, ..source }, Source { analyzers: &[], ..source }] {
        let none = TrackValues::read(source, &device, &library);
        assert!(none.tracks.is_empty() && none.declared == declared);
    }
}

/// An analyzer's workers keep values at once, and every value lands.
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
    assert!(tracks.iter().all(|t| !store.get(t, std::slice::from_ref(&a)).is_empty()));
}
