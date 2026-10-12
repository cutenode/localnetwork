use super::*;
use crate::Device;
use hegel::TestCase;
use hegel::generators as gs;
use loclib_model::test_util::track;

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
/// has, and the estimates on every annotated track there's audio for (the ones that fail to
/// decode as misses); the rest are unmatched. Without annotations, or anything to score,
/// there's nothing to do.
#[hegel::test(test_cases = 8)]
fn scores_count_every_annotated_track_once(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (root, folder, keys_dir, tempos_dir) =
        (dir.path().join("drive"), dir.path().join("audio"), dir.path().join("key"), dir.path().join("tempo"));
    for d in [root.join("Contents"), folder.clone(), keys_dir.clone(), tempos_dir.clone()] {
        std::fs::create_dir_all(d).unwrap();
    }
    let key =
        |tc: &TestCase| Camelot::from_pitch(tc.draw(gs::integers::<u32>().max_value(11)), tc.draw(gs::booleans()));
    let in_folder = tc.draw(gs::booleans());
    let (mtg, with_tempos) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    // Files that aren't annotations are passed over.
    std::fs::write(keys_dir.join("README.md"), "GiantSteps").unwrap();
    let (mut tracks, mut table) = (Vec::new(), vec!["ID\tMANUAL KEY\tC".to_string()]);
    let (mut keyed, mut timed, mut audible, mut on_drive, mut broken) = (vec![], vec![], vec![], vec![], vec![]);
    for i in 0..tc.draw(gs::integers::<u32>().min_value(1).max_value(5)) {
        let id = format!("10{i}");
        let annotated_key = tc.draw(gs::optional(gs::just(()))).map(|_| key(&tc));
        if let Some(k) = annotated_key {
            keyed.push(id.clone());
            match mtg {
                true => table.push(format!("{id}\t{}\t2", spelled(k))),
                false => std::fs::write(keys_dir.join(format!("{id}.LOFI.key")), spelled(k)).unwrap(),
            }
        } else if mtg {
            // Annotated with too little confidence to count.
            table.push(format!("{id}\t{}\t1", spelled(key(&tc))));
        }
        if with_tempos && tc.draw(gs::booleans()) {
            timed.push(id.clone());
            std::fs::write(tempos_dir.join(format!("{id}.LOFI.bpm")), "128.0\n").unwrap();
        }
        let file = format!("{id}.LOFI.wav");
        if tc.draw(gs::booleans()) {
            on_drive.push(id.clone());
            let mut t = track(i, "A", &id, &format!("/Contents/{file}"));
            t.key = tc.draw(gs::optional(gs::just(()))).map_or(String::new(), |_| key(&tc).note());
            tracks.push(t);
        }
        let has_audio = tc.draw(gs::weighted_booleans(0.3)) && (in_folder || on_drive.contains(&id));
        if has_audio {
            let at = if in_folder { folder.join(&file) } else { root.join("Contents").join(&file) };
            let bad = tc.draw(gs::weighted_booleans(0.3));
            let wav = if bad {
                loclib_mixid::test_util::wav(0, 16, 0.0)
            } else {
                loclib_mixid::test_util::wav(44_100, 16, 0.2)
            };
            std::fs::write(at, wav).unwrap();
            audible.push(id.clone());
            if bad {
                broken.push(id.clone());
            }
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
    let cache = Cache::new(dir.path().join("cache"));
    let result = bench(&opts, Some((&device, &library)), &cache, &Ctx::silent());
    if keyed.is_empty() && timed.is_empty() {
        let err = result.unwrap_err();
        return assert!(err.to_string().contains("no key or tempo annotations"), "{err}");
    }
    let r = result.unwrap();
    let both = |a: &[String], b: &[String]| a.iter().filter(|id| b.contains(id)).count();
    let tracks = |reports: &IndexMap<String, Report<KeyOutcome>>, name| reports.get(name).map_or(0, |r| r.tracks);
    assert_eq!(tracks(&r.keys, REKORDBOX), both(&keyed, &on_drive));
    assert_eq!(tracks(&r.keys, ESTIMATE), both(&keyed, &audible));
    let tempo_tracks = |name| r.tempos.get(name).map_or(0, |r| r.tracks);
    assert_eq!((tempo_tracks(REKORDBOX), tempo_tracks(ESTIMATE)), (both(&timed, &on_drive), both(&timed, &audible)));
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
    assert!(bench(&nowhere, None, &cache, &Ctx::silent()).unwrap_err().to_string().contains("give a drive"));
}
