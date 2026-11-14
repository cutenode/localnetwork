use super::*;
use crate::Device;
use hegel::generators as gs;
use hegel::{Generator, TestCase};
use loclib_model::test_util::{keys, track};

/// The test's analyzers, `a.heard` and `b.heard`, each with a key (`tone`) and a tempo (`tempo`),
/// offered to this thread's readers in that order; and a store to choose among them in.
fn offered(dir: &Path) -> Store {
    let field = |plugin: &str, field: &str| RoleField {
        analyzer: Analyzer { plugin: plugin.into(), analyzer: "heard".into(), version: "1".into() },
        field: field.into(),
    };
    let both = |name| vec![field("a", name), field("b", name)];
    let role = roles::RoleOrPart::Role;
    crate::testing::roles_on_this_thread(Some(
        [(role(Role::Key), both("tone")), (role(Role::Bpm), both("tempo"))].into(),
    ));
    Store::open_or_init(dir.join("store")).unwrap()
}

/// What the test's analyzers hear in each audio file handed to them (the drive's, as `library`
/// has them on the drive at `root`): what the file says under the analyzer's plugin, as JSON (as
/// the test plugin's `heard` does). A file that isn't JSON fails.
fn hear<'a>(library: &'a Library, root: &'a Path) -> impl Fn(&Analyzer, Audio<'_>) -> Result<Vec<Result<Fields>>> + 'a {
    move |a: &Analyzer, audio: Audio<'_>| {
        let files: Vec<PathBuf> = match audio {
            Audio::Files(files) => files.to_vec(),
            Audio::Tracks { tracks, .. } => {
                let path = |id: &u32| root.join(library.track(*id).unwrap().file_path.trim_start_matches('/'));
                tracks.iter().map(path).collect()
            }
        };
        let heard = |f: &PathBuf| -> Result<Fields> {
            let all: serde_json::Value = serde_json::from_str(&read(f)?)
                .map_err(|e| Error::Parse { path: f.clone(), message: e.to_string() })?;
            Ok(serde_json::from_value(all[&a.plugin].clone()).unwrap_or_default())
        };
        Ok(files.iter().map(heard).collect())
    }
}

/// What `a.heard` hears in a file, as it's written: a key and a tempo.
fn heard_as(key: Camelot, bpm: f64) -> String {
    serde_json::json!({ "a": { "tone": key.code(), "tempo": bpm } }).to_string()
}

/// The name `a.heard` is scored under: the analyzer chosen for both roles by default.
const HEARD: &str = "a.heard";

/// What the datasets name a key: `C minor`.
fn spelled(key: Camelot) -> String {
    let tonic = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"][key.pitch() as usize];
    format!("{tonic} {}", if key.major { "major" } else { "minor" })
}

/// Ids are file names up to the first dot, in any folder: a name made of an id and dotted
/// suffixes reads as that id, and one with nothing before its first dot has none.
#[hegel::test(test_cases = 300)]
fn ids_are_names_up_to_the_first_dot(tc: TestCase) {
    let named = [
        ("audio/1030011.LOFI.mp3", Some("1030011")),
        (r"C:\gs\5.LOFI.key", Some("5")),
        ("a.b/c", Some("c")),
        ("key/", None),
        (".LOFI.key", None),
    ];
    for (path, id) in named {
        assert_eq!(id_of(path).as_deref(), id, "{path:?}");
    }
    let folders: Vec<String> = tc.draw(gs::vecs(gs::text().alphabet("ab.c ").max_size(5)).max_size(3));
    let id = tc.draw(gs::text().alphabet("0123ab ").max_size(8));
    let suffixes: Vec<String> = tc.draw(gs::vecs(gs::text().alphabet("LOFImp3.").max_size(5)).max_size(3));
    let mut path = String::new();
    for f in &folders {
        path += f;
        path += tc.draw(gs::sampled_from(vec!["/", "\\"]));
    }
    path += &id;
    for s in &suffixes {
        path += &format!(".{s}");
    }
    assert_eq!(id_of(&path), (!id.is_empty()).then_some(id), "{path:?}");
}

/// Any annotations, drive and audio: rekordbox is scored on every annotated track the drive
/// has, and the analyzer chosen for the key and bpm roles on every annotated track there's audio
/// for (the ones it fails on as misses), by what it heard; the rest are unmatched. Without
/// annotations, or anything to score, there's nothing to do.
#[hegel::test(test_cases = 8)]
fn scores_count_every_annotated_track_once(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (root, folder, keys_dir, tempos_dir) =
        (dir.path().join("drive"), dir.path().join("audio"), dir.path().join("key"), dir.path().join("tempo"));
    for d in [root.join("Contents"), folder.clone(), keys_dir.clone(), tempos_dir.clone()] {
        std::fs::create_dir_all(d).unwrap();
    }
    let store = offered(dir.path());
    let in_folder = tc.draw(gs::booleans());
    let (mtg, with_tempos) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    // Files that aren't annotations are passed over.
    std::fs::write(keys_dir.join("README.md"), "GiantSteps").unwrap();
    let (mut tracks, mut table) = (Vec::new(), vec!["ID\tMANUAL KEY\tC".to_string()]);
    let (mut keyed, mut timed, mut audible, mut on_drive, mut broken) = (vec![], vec![], vec![], vec![], vec![]);
    // Tracks whose key, and whose tempo, the analyzer hears as annotated.
    let (mut keys_heard, mut tempos_heard) = (0, 0);
    for i in 0..tc.draw(gs::integers::<u32>().min_value(1).max_value(5)) {
        let id = format!("10{i}");
        let annotation = tc.draw(gs::optional(keys()));
        if let Some(k) = annotation {
            keyed.push(id.clone());
            match mtg {
                true => table.push(format!("{id}\t{}\t2", spelled(k))),
                false => std::fs::write(keys_dir.join(format!("{id}.LOFI.key")), spelled(k)).unwrap(),
            }
        } else if mtg {
            // Annotated with too little confidence to count.
            table.push(format!("{id}\t{}\t1", spelled(tc.draw(keys()))));
        }
        let timed_here = with_tempos && tc.draw(gs::booleans());
        if timed_here {
            timed.push(id.clone());
            std::fs::write(tempos_dir.join(format!("{id}.LOFI.bpm")), "128.0\n").unwrap();
        }
        let file = format!("{id}.LOFI.wav");
        if tc.draw(gs::booleans()) {
            on_drive.push(id.clone());
            let mut t = track(i, "A", &id, &format!("/Contents/{file}"));
            t.key = tc.draw(gs::optional(keys().map(Camelot::note))).unwrap_or_default();
            tracks.push(t);
        }
        let has_audio = tc.draw(gs::weighted_booleans(0.3)) && (in_folder || on_drive.contains(&id));
        if has_audio {
            let at = if in_folder { folder.join(&file) } else { root.join("Contents").join(&file) };
            let bad = tc.draw(gs::weighted_booleans(0.3));
            let (key, bpm) = (tc.draw(keys()), tc.draw(gs::sampled_from(vec![128.0, 64.0])));
            std::fs::write(at, if bad { "broken".to_string() } else { heard_as(key, bpm) }).unwrap();
            audible.push(id.clone());
            if bad {
                broken.push(id.clone());
            }
            keys_heard += usize::from(!bad && annotation == Some(key));
            tempos_heard += usize::from(!bad && timed_here && bpm == 128.0);
        }
    }
    if mtg {
        std::fs::write(keys_dir.join("annotations.txt"), table.join("\n")).unwrap();
    }
    let opts = BenchOptions {
        keys: Some(if mtg { keys_dir.join("annotations.txt") } else { keys_dir }),
        tempos: with_tempos.then_some(tempos_dir),
        audio: in_folder.then_some(folder),
    };
    let device = Device::open(&root).unwrap();
    let library = Library { tracks, ..Default::default() };
    let result = bench(&opts, Some((&device, &library)), &store, hear(&library, &root), &Ctx::silent());
    if keyed.is_empty() && timed.is_empty() {
        let err = result.unwrap_err();
        return assert!(err.to_string().contains("no key or tempo annotations"), "{err}");
    }
    let r = result.unwrap();
    let both = |a: &[String], b: &[String]| a.iter().filter(|id| b.contains(id)).count();
    let tracks = |reports: &IndexMap<String, Report<KeyOutcome>>, name| reports.get(name).map_or(0, |r| r.tracks);
    assert_eq!(tracks(&r.keys, REKORDBOX), both(&keyed, &on_drive));
    assert_eq!(tracks(&r.keys, HEARD), both(&keyed, &audible));
    assert_eq!(r.keys.get(HEARD).map_or(0, |k| k.count(KeyOutcome::Correct)), keys_heard);
    let tempo_tracks = |name| r.tempos.get(name).map_or(0, |r| r.tracks);
    assert_eq!((tempo_tracks(REKORDBOX), tempo_tracks(HEARD)), (both(&timed, &on_drive), both(&timed, &audible)));
    assert_eq!(r.tempos.get(HEARD).map_or(0, |t| t.count(TempoOutcome::Correct)), tempos_heard);
    assert!(r.accuracy2.keys().eq(r.tempos.keys()));
    let annotated: Vec<String> = keyed.iter().chain(&timed).cloned().collect::<FxHashSet<_>>().into_iter().collect();
    assert_eq!(r.unmatched, annotated.len() - both(&annotated, &audible));
    let mut failed: Vec<String> = r.failed.iter().filter_map(|(name, _)| id_of(name)).collect();
    failed.sort();
    let mut expected: Vec<String> = broken.into_iter().filter(|id| annotated.contains(id)).collect();
    expected.sort();
    assert_eq!(failed, expected);
    // The stored tempo, 128, agrees with the annotation wherever there's one to score.
    assert!(r.tempos.get(REKORDBOX).is_none_or(|t| t.count(TempoOutcome::Correct) == t.tracks));

    let nowhere = BenchOptions { audio: None, ..opts.clone() };
    let e = bench(&nowhere, None, &store, hear(&library, &root), &Ctx::silent()).unwrap_err();
    assert!(e.to_string().contains("give a drive"), "{e}");
}

/// Annotations that don't read are refused before anything is analysed, naming what couldn't
/// be read: a key or tempo file that isn't one, or a row of the MTG table that isn't one, is a
/// parse error with that file's path, and a tempo folder that isn't there an I/O error with its
/// path. Without the bad one, the same annotations read (and scoring then wants something to
/// score).
#[hegel::test(test_cases = 40)]
fn annotations_that_dont_read_are_refused_naming_the_file(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (keys_dir, tempos_dir) = (dir.path().join("key"), dir.path().join("tempo"));
    std::fs::create_dir_all(&keys_dir).unwrap();
    std::fs::create_dir_all(&tempos_dir).unwrap();
    std::fs::write(keys_dir.join("1.LOFI.key"), "C minor").unwrap();
    std::fs::write(tempos_dir.join("1.LOFI.bpm"), "128.0\n").unwrap();
    let table = keys_dir.join("annotations.txt");
    let good_rows = "ID\tMANUAL KEY\tC\n1\tC minor\t2\n";
    std::fs::write(&table, good_rows).unwrap();
    let mtg = tc.draw(gs::booleans());
    let opts = BenchOptions {
        keys: Some(if mtg { table.clone() } else { keys_dir.clone() }),
        tempos: Some(tempos_dir.clone()),
        audio: None,
    };
    let (store, library) = (offered(dir.path()), Library::default());
    let run = |opts: &BenchOptions| bench(opts, None, &store, hear(&library, dir.path()), &Ctx::silent()).unwrap_err();
    let fine = run(&opts);
    assert!(fine.to_string().contains("give a drive"), "{fine}");

    let (opts, bad, code) = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 if mtg => {
            let row = tc.draw(gs::sampled_from(vec!["2\tC minor", "2\tC minor\tsure", "2\tH minor\t2", "2"]));
            std::fs::write(&table, format!("{good_rows}{row}\n")).unwrap();
            (opts, table, "parse")
        }
        0 => {
            let text = tc.draw(gs::text().alphabet("xqz! ").max_size(6));
            let bad = keys_dir.join("2.LOFI.key");
            std::fs::write(&bad, text).unwrap();
            (opts, bad, "parse")
        }
        1 => {
            let text = tc.draw(gs::sampled_from(vec!["fast", "-5", "inf", "", "12 0"]));
            let bad = tempos_dir.join("2.LOFI.bpm");
            std::fs::write(&bad, text).unwrap();
            (opts, bad, "parse")
        }
        _ => {
            let gone = dir.path().join("no tempos");
            (BenchOptions { tempos: Some(gone.clone()), ..opts }, gone, "io")
        }
    };
    let e = run(&opts);
    assert_eq!(e.code(), code, "{e}");
    assert!(e.to_string().contains(&bad.display().to_string()), "{e}");
}

/// Annotations, audio files and tracks with nothing before their first dot have no id, so they're
/// passed over unread (annotations that wouldn't parse included), as is a track whose path leaves
/// the drive: tracks without audio are unmatched, and nothing fails.
#[hegel::test(test_cases = 8)]
fn prop_what_has_no_id_or_leaves_the_drive_is_passed_over(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (root, folder, keys_dir, tempos_dir) =
        (dir.path().join("drive"), dir.path().join("audio"), dir.path().join("key"), dir.path().join("tempo"));
    for d in [root.join("Contents"), folder.clone(), keys_dir.clone(), tempos_dir.clone()] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(keys_dir.join("1.LOFI.key"), "C minor").unwrap();
    std::fs::write(keys_dir.join("2.LOFI.key"), "A major").unwrap();
    std::fs::write(keys_dir.join(".LOFI.key"), "xqz!").unwrap();
    std::fs::write(tempos_dir.join(".LOFI.bpm"), "fast").unwrap();
    let wav = heard_as(Camelot::parse("8A").unwrap(), 128.0);
    std::fs::write(folder.join(".LOFI.wav"), &wav).unwrap();
    std::fs::write(root.join("Contents/.LOFI.wav"), &wav).unwrap();
    let in_folder = tc.draw(gs::booleans());
    let with_tempos = tc.draw(gs::booleans());
    let opts = BenchOptions {
        keys: Some(keys_dir),
        tempos: with_tempos.then_some(tempos_dir),
        audio: in_folder.then_some(folder),
    };
    let library = Library {
        tracks: vec![
            track(1, "A", "1", "/Contents/1.LOFI.wav"),
            track(2, "A", "2", "/../2.LOFI.wav"),
            track(3, "A", "none", "/Contents/.LOFI.wav"),
        ],
        ..Default::default()
    };
    let device = Device::open(&root).unwrap();
    let store = offered(dir.path());
    let r = bench(&opts, Some((&device, &library)), &store, hear(&library, &root), &Ctx::silent()).unwrap();
    assert_eq!(r.keys.get(REKORDBOX).map(|r| r.tracks), Some(2));
    assert_eq!(r.keys.get(HEARD).map_or(0, |r| r.tracks), 0);
    assert!(r.tempos.is_empty());
    assert_eq!((r.unmatched, r.failed.len()), (2, 0));
}

/// A benchmark cancelled before the audio is measured, or while a file is, stops with nothing
/// scored, whatever the analyzers answered for the other files.
#[hegel::test(test_cases = 4)]
fn prop_a_cancelled_benchmark_scores_nothing(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (folder, keys_dir) = (dir.path().join("audio"), dir.path().join("key"));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::create_dir_all(&keys_dir).unwrap();
    for id in ["1", "2"] {
        std::fs::write(keys_dir.join(format!("{id}.LOFI.key")), "C minor").unwrap();
        std::fs::write(folder.join(format!("{id}.LOFI.wav")), heard_as(Camelot::parse("5A").unwrap(), 1.0)).unwrap();
    }
    let opts = BenchOptions { keys: Some(keys_dir), tempos: None, audio: Some(folder) };
    let store = offered(dir.path());
    // Before, or as the analyzer is on one of the files.
    let (before, on) = (tc.draw(gs::booleans()), tc.draw(gs::integers::<usize>().max_value(1)));
    let library = Library::default();
    let heard = hear(&library, dir.path());
    let measure = |a: &Analyzer, audio: Audio<'_>| -> Result<Vec<Result<Fields>>> {
        if before {
            return Err(Error::Cancelled);
        }
        let mut answers = heard(a, audio)?;
        answers[on] = Err(Error::Cancelled);
        Ok(answers)
    };
    let e = bench(&opts, None, &store, measure, &Ctx::silent()).unwrap_err();
    assert_eq!(e.code(), "cancelled", "{e}");
}

/// The analyzer chosen for each role is the one scored, under its name, by what it heard: the one
/// the store's settings choose, else the first offered; none when the role is turned off. With
/// neither role's, a folder of audio has nothing to score.
#[hegel::test(test_cases = 20)]
fn prop_the_analyzer_chosen_for_a_role_is_scored(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (folder, keys_dir, tempos_dir) = (dir.path().join("audio"), dir.path().join("key"), dir.path().join("tempo"));
    for d in [&folder, &keys_dir, &tempos_dir] {
        std::fs::create_dir_all(d).unwrap();
    }
    let store = offered(dir.path());
    let annotated = tc.draw(keys());
    std::fs::write(keys_dir.join("1.LOFI.key"), spelled(annotated)).unwrap();
    std::fs::write(tempos_dir.join("1.LOFI.bpm"), "120.0").unwrap();
    // Each analyzer hears a key (the annotated one or its relative) and a tempo (right, or double).
    let mut heard = serde_json::Map::new();
    let mut hears = Vec::new();
    for plugin in ["a", "b"] {
        let (key, bpm) = (
            tc.draw(gs::sampled_from(vec![annotated, annotated.relative()])),
            tc.draw(gs::sampled_from(vec![120.0, 240.0])),
        );
        heard.insert(plugin.into(), serde_json::json!({ "tone": key.code(), "tempo": bpm }));
        hears.push((format!("{plugin}.heard"), key == annotated, bpm == 120.0));
    }
    std::fs::write(folder.join("1.LOFI.wav"), serde_json::Value::Object(heard).to_string()).unwrap();
    // `None`: the default; `Some(None)`: turned off.
    let mut expected = Vec::new();
    for role in [Role::Key, Role::Bpm] {
        let choice = tc.draw(gs::sampled_from(vec![None, Some(None), Some(Some(0)), Some(Some(1))]));
        let chosen = match choice {
            None => Some(&hears[0]),
            Some(None) => {
                roles::turn_off(&store, role).unwrap();
                None
            }
            Some(Some(i)) => {
                roles::choose(&store, role, Some(hears[i].0.as_str())).unwrap();
                Some(&hears[i])
            }
        };
        expected.push(chosen);
    }
    let opts = BenchOptions { keys: Some(keys_dir), tempos: Some(tempos_dir), audio: Some(folder) };
    let library = Library::default();
    let result = bench(&opts, None, &store, hear(&library, dir.path()), &Ctx::silent());
    if expected.iter().all(Option::is_none) {
        let e = result.unwrap_err();
        return assert_eq!(e.to_string(), "no plugin works out key, bpm (`locallibrary plugin` lists what they do)");
    }
    let r = result.unwrap();
    let scored =
        |reports: Vec<(&String, f64)>| reports.into_iter().map(|(n, s)| (n.clone(), s == 1.0)).collect::<Vec<_>>();
    let keys: Vec<(String, bool)> = expected[0].map(|(name, right, _)| (name.clone(), *right)).into_iter().collect();
    let tempos: Vec<(String, bool)> = expected[1].map(|(name, _, right)| (name.clone(), *right)).into_iter().collect();
    assert_eq!(scored(r.keys.iter().map(|(n, k)| (n, k.score)).collect()), keys);
    assert_eq!(scored(r.tempos.iter().map(|(n, t)| (n, t.score)).collect()), tempos);
}

/// With no analyzer for the roles annotated, a drive's stored values are still scored, and the
/// warning says why the analyzers aren't.
#[test]
fn without_an_analyzer_rekordbox_is_scored_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (root, keys_dir) = (dir.path().join("drive"), dir.path().join("key"));
    std::fs::create_dir_all(root.join("Contents")).unwrap();
    std::fs::create_dir_all(&keys_dir).unwrap();
    std::fs::write(keys_dir.join("1.LOFI.key"), "C minor").unwrap();
    std::fs::write(root.join("Contents/1.LOFI.wav"), heard_as(Camelot::parse("5A").unwrap(), 1.0)).unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    crate::testing::roles_on_this_thread(Some(Default::default()));
    let library = Library { tracks: vec![track(1, "A", "1", "/Contents/1.LOFI.wav")], ..Default::default() };
    let warnings = std::sync::Mutex::new(Vec::new());
    let progress = |e: &crate::progress::Event| {
        if let crate::progress::Event::Warning { message } = e {
            warnings.lock().unwrap().push(message.clone());
        }
    };
    let opts = BenchOptions { keys: Some(keys_dir), tempos: None, audio: None };
    let device = Device::open(&root).unwrap();
    let ctx = Ctx::new(&progress, crate::progress::CancelToken::new());
    let r = bench(&opts, Some((&device, &library)), &store, hear(&library, &root), &ctx).unwrap();
    assert_eq!(r.keys.keys().collect::<Vec<_>>(), [REKORDBOX]);
    assert_eq!(
        *warnings.lock().unwrap(),
        ["no plugin works out key (`locallibrary plugin` lists what they do): only rekordbox is scored"]
    );
}
