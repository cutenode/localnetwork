use std::fs;

use super::*;
use crate::device::Device;

use filetime::FileTime;
use hegel::TestCase;
use hegel::generators as gs;

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

/// The plugins, versions and analyzers (each with a field of its own) values are kept for, the
/// last of each a name no folder can have.
const PLUGINS: [&str; 3] = ["sound", "other", "../up"];
const VERSIONS: [&str; 3] = ["0.1.0", "1.0.0-beta+2", ".hidden"];
const ANALYZERS: [(&str, &str); 3] = [("tempo", "bpm"), ("key", "key"), ("a/b", "x")];

/// A value an analyzer might answer: of each type, and numbers JSON can't hold.
#[hegel::test_helper]
fn field_value(tc: &TestCase) -> FieldValue {
    match tc.draw(gs::integers::<u8>().max_value(6)) {
        0 => FieldValue::Integer(tc.draw(gs::integers::<i64>())),
        1 => FieldValue::Number(tc.draw(gs::floats::<f64>().allow_nan(false).allow_infinity(false))),
        2 => FieldValue::Number(tc.draw(gs::sampled_from(vec![f64::NAN, f64::INFINITY, f64::NEG_INFINITY]))),
        3 => FieldValue::Boolean(tc.draw(gs::booleans())),
        4 => FieldValue::Strings(tc.draw(gs::vecs(gs::text().max_size(4)).max_size(3))),
        _ => FieldValue::String(tc.draw(gs::text().max_size(8))),
    }
}

/// For any sequence of values kept for a drive's audio files, by any plugin, version and
/// analyzer, and of files changing: a value is kept only when its names can be folders and its
/// numbers are finite, and reads back (as the store has it, and named `plugin.field` for a
/// library's tracks) from the analyzers asked for, until its file changes.
#[hegel::test(test_cases = 50, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn kept_fields_read_back_until_their_file_changes(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let st = Store::open_or_init(dir.path().join("store")).unwrap();
    let store = FieldStore::in_store(&st);
    let drive = dir.path().join("USB");
    let files: Vec<PathBuf> = (0..3).map(|i| audio(&drive.join("Contents"), &format!("{i}.mp3"), b"audio")).collect();
    let track = |id: u32, file: &str| loclib_model::test_util::track(id, "A", "T", file);
    let mut tracks: Vec<_> = (0..3).map(|i| track(i + 1, &format!("/Contents/{i}.mp3"))).collect();
    tracks.push(track(4, "/Contents/gone.mp3"));
    let library = Library { tracks, ..Default::default() };
    // What's kept, by file and analyzer, as a model has it.
    let mut kept: Vec<(usize, Analyzer, Fields)> = Vec::new();
    let mut changes = 0;
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(10)) {
        let file = tc.draw(gs::integers::<usize>().max_value(2));
        if tc.draw(gs::integers::<u8>().max_value(4)) == 0 {
            // Longer each time: its size, and so what keys its values, changes.
            changes += 1;
            let longer = format!("audio{}", "!".repeat(changes));
            audio(&drive.join("Contents"), &format!("{file}.mp3"), longer.as_bytes());
            kept.retain(|(f, ..)| *f != file);
            continue;
        }
        let (plugin, version) =
            (tc.draw(gs::sampled_from(PLUGINS.to_vec())), tc.draw(gs::sampled_from(VERSIONS.to_vec())));
        let (name, field) = tc.draw(gs::sampled_from(ANALYZERS.to_vec()));
        let a = analyzer(plugin, version, name);
        let values = fields(&[(field, field_value(&tc))]);
        let named = PLUGINS[..2].contains(&plugin) && VERSIONS[..2].contains(&version) && name != ANALYZERS[2].0;
        let finite = values.values().all(|v| !matches!(v, FieldValue::Number(n) if !n.is_finite()));
        match (store.put(&files[file], &a, &values), named, finite) {
            (Ok(()), true, true) => {
                kept.retain(|(f, k, _)| (*f, k) != (file, &a));
                kept.push((file, a, values));
            }
            (Err(Error::Invalid(m)), false, _) => assert!(m.contains("isn't a plugin id, version or analyzer"), "{m}"),
            (Err(Error::Invalid(m)), true, false) => {
                assert!(m.contains(&format!("{field:?} isn't a finite number")), "{m}")
            }
            (other, ..) => panic!("{a:?} {values:?}: {other:?}"),
        }
    }
    let valid: Vec<Analyzer> = PLUGINS[..2]
        .iter()
        .flat_map(|p| {
            VERSIONS[..2].iter().flat_map(move |v| ANALYZERS[..2].iter().map(move |(a, _)| analyzer(p, v, a)))
        })
        .collect();
    let asked: Vec<Analyzer> = tc.draw(gs::subsequences(valid.clone()));
    for (i, file) in files.iter().enumerate() {
        // Every version's values are its own: asked for them all, but one version at a time.
        for version in &VERSIONS[..2] {
            let of_version: Vec<Analyzer> = valid.iter().filter(|a| a.version == *version).cloned().collect();
            let mut want = PluginsFields::new();
            for (_, a, values) in kept.iter().filter(|(f, a, _)| *f == i && a.version == *version) {
                want.entry(a.plugin.clone()).or_default().insert(a.analyzer.clone(), values.clone());
            }
            assert_eq!(store.get(file, &of_version), want, "file {i}, version {version}");
        }
    }
    let declared = Declared::new();
    let source = Source { store: Some(&st), analyzers: &asked, declared: &declared };
    let values = TrackValues::read(source, &Device::open(&drive).unwrap(), &library);
    let mut want: FxHashMap<u32, Fields> = FxHashMap::default();
    for a in &asked {
        for (f, _, values) in kept.iter().filter(|(_, k, _)| k == a) {
            let named = values.iter().map(|(k, v)| (format!("{}.{k}", a.plugin), v.clone()));
            want.entry(*f as u32 + 1).or_default().extend(named);
        }
    }
    assert_eq!(values.tracks, want);
}
