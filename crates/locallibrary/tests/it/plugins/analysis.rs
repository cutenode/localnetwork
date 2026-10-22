//! Structured analysis (feature `analysis`): an analyzer's beat grid, phrases and waveform kept
//! apart from its columns, refused when they're not what they say, and written to the tracks'
//! analysis files as an edit plan only when asked (`plugins.apply_analysis`); and plugins'
//! operations planning analysis edits, which takes `analysis:plan`.

use locallibrary::analysis_edit::NewAnalysis;
use locallibrary::anlz::{self, Bands, Beat, Mood, Phrase, PhraseKind, SongStructure};
use locallibrary::edit::Edit;
use locallibrary::fields::{Analyzer, FieldStore, OverlayStore};
use locallibrary::paths;
use locallibrary::plugin::{AnalysisPart, ApplyAnalysis};

use super::*;

/// The echo manifest with the analyzer `heard` answering a beat grid, phrases, a waveform and a
/// tempo: what the test writes in each audio file.
fn structured() -> String {
    ECHO.replace(
        "fields = { bytes = \"integer\" }\n",
        "fields = { bytes = \"integer\" }\n\n[[contributes.analyzers]]\nname = \"heard\"\ntitle = \"Heard\"\n\
         version = \"1\"\nfields = { grid = \"beats\", phrases = \"phrases\", wave = \"waveform\", tempo = \"number\" }\n",
    )
}

fn heard() -> Analyzer {
    Analyzer { plugin: "echo".into(), analyzer: "heard".into(), version: "1".into() }
}

fn grid(n: usize, from: u32) -> Vec<Beat> {
    (0..n).map(|i| Beat { beat_number: (i % 4) as u16 + 1, bpm: 120.0, time_ms: from + i as u32 * 500 }).collect()
}

fn phrases() -> SongStructure {
    let phrase = |kind, start_beat, end_beat| Phrase { kind, start_beat, end_beat, fill_beat: None };
    SongStructure {
        mood: Mood::Mid,
        phrases: vec![phrase(PhraseKind::Intro, 1, 33), phrase(PhraseKind::Chorus, 33, 65)],
    }
}

/// What the analyzer answers for a track: its grid of `beats`, phrases, a waveform of `columns`
/// at 75 a second in a blob, and a tempo.
fn answer(beats: usize, columns: usize) -> Value {
    let bands = Bands((0..columns).map(|i| [i as u8, 100, 200]).collect());
    json!({
        "grid": grid(beats, 25),
        "phrases": phrases(),
        "wave": { "blob": "wave.bands", "columns_per_sec": 75.0 },
        "tempo": 120.0,
        "blobs": { "wave.bands": bands.pack() },
    })
}

impl Fixture {
    fn hear_all(&self, answers: &[Value]) -> Vec<locallibrary::Track> {
        let tracks = common::library(&self.drive).tracks;
        for (t, answer) in tracks.iter().zip(answers) {
            std::fs::write(self.audio(t), answer.to_string()).unwrap();
        }
        tracks
    }

    fn audio(&self, t: &locallibrary::Track) -> PathBuf {
        paths::to_fs(&self.drive, &paths::key(&t.file_path))
    }

    fn apply_analysis(&self, parts: &[AnalysisPart], overwrite: bool) -> Result<Planned<EditPlan>> {
        let req = ApplyAnalysis {
            plugin: "echo".into(),
            analyzer: "heard".into(),
            drive: "USB".into(),
            parts: parts.to_vec(),
            tracks: None,
            overwrite,
        };
        self.plugins.plan_apply_analysis(&req, &self.store, &Ctx::silent())
    }
}

use locallibrary::edit::EditPlan;
use locallibrary::op::Planned;

/// The grid, phrases and waveform go to the overlay store (the waveform put at 150 columns a
/// second), the tempo to the columns; the structured fields are no columns.
#[test]
fn structured_fields_are_kept_apart_from_the_columns() {
    let f = Fixture::new(|dir| vec![spec(dir, &structured(), &[])], Options::default());
    let tracks = f.hear_all(&[answer(64, 300), json!({ "grid": null, "phrases": null, "wave": null, "tempo": 1 })]);
    let analyzed = f.plugins.analyze("echo", "heard", "USB", None, &f.store, &Ctx::silent()).unwrap();
    assert_eq!((analyzed.analyzed, analyzed.failed), (2, 0));
    let overlays = OverlayStore::in_store(&f.store);
    let kept = overlays.get(&f.audio(&tracks[0]), &heard()).unwrap();
    assert_eq!(kept.beats, Some(grid(64, 25)));
    assert_eq!(kept.phrases, Some(phrases()));
    let wave = kept.waveform.unwrap();
    assert_eq!((wave.0.len(), wave.0[0], wave.0[599]), (600, [0, 100, 200], [43, 100, 200]));
    assert_eq!(overlays.get(&f.audio(&tracks[1]), &heard()), None, "nothing to report, nothing kept");
    let fields = FieldStore::in_store(&f.store).get(&f.audio(&tracks[0]), &[heard()]);
    assert_eq!(serde_json::to_value(&fields["echo"]["heard"]).unwrap(), json!({ "tempo": 120.0 }));
    let columns: Vec<String> = f.plugins.fields().into_keys().collect();
    assert_eq!(columns, ["echo.bytes", "echo.tempo"]);
    // Drawn from the first analyzer with each.
    let shown = overlays.first(&f.audio(&tracks[0]), &[heard()]);
    assert_eq!((shown.beats_from, shown.overlay.beats.map(|b| b.len())), (Some(heard()), Some(64)));
}

/// A structured field that isn't what its type says fails the track, saying why, and keeps
/// nothing of it.
#[test]
fn structured_fields_that_arent_what_they_say_fail_the_track() {
    let f = Fixture::new(|dir| vec![spec(dir, &structured(), &[])], Options::default());
    let bad_beat = json!({ "grid": [{ "beat_number": 7, "bpm": 120.0, "time_ms": 0 }] });
    let not_beats = json!({ "grid": [1, 2] });
    let tracks = f.hear_all(&[bad_beat, not_beats]);
    let analyzed = f.plugins.analyze("echo", "heard", "USB", None, &f.store, &Ctx::silent()).unwrap();
    let errors: Vec<&str> = analyzed.failures.iter().map(|x| x.error.as_str()).collect();
    assert_eq!(errors.len(), 2);
    assert!(errors[0].ends_with("its grid: beat 1 is beat 7 of its bar: 1-4"), "{}", errors[0]);
    assert!(errors[1].ends_with("its field grid isn't the beats heard declares"), "{}", errors[1]);
    let overlays = OverlayStore::in_store(&f.store);
    assert!(tracks.iter().all(|t| overlays.get(&f.audio(t), &heard()).is_none()));
    for (wave, why) in [
        (json!({ "blob": "gone.bands", "columns_per_sec": 75.0 }), "its wave: its file \"gone.bands\" isn't there"),
        (
            json!({ "blob": "two.bands", "columns_per_sec": 75.0 }),
            "its wave isn't three bytes a column, at least one column",
        ),
        (json!({ "blob": "two.bands", "columns_per_sec": 0.0 }), "its field wave isn't the waveform heard declares"),
    ] {
        f.hear_all(&[json!({ "wave": wave, "blobs": { "two.bands": [1, 2] } })]);
        // Kept already: analysed again by a version of its own.
        let _ = std::fs::remove_dir_all(f.store.root().join("cache/fields"));
        let analyzed =
            f.plugins.analyze("echo", "heard", "USB", Some(&[tracks[0].id]), &f.store, &Ctx::silent()).unwrap();
        let error = &analyzed.failures[0].error;
        assert!(error.contains(why), "{error}");
    }
}

/// What the analyzer kept is written to the tracks' analysis files as an edit plan: only over
/// what they have when asked, a grid with its BPM, phrases only on the grid they count, and a
/// track whose files can't take a waveform left out, saying why.
#[test]
fn the_analysis_is_written_as_an_edit() {
    let f = Fixture::new(|dir| vec![spec(dir, &structured(), &[])], Options::default());
    let tracks = f.hear_all(&[answer(64, 300), answer(64, 300)]);
    // Only the first is analysed yet.
    f.plugins.analyze("echo", "heard", "USB", Some(&[tracks[0].id]), &f.store, &Ctx::silent()).unwrap();
    let kept = f.apply_analysis(&[AnalysisPart::Grid, AnalysisPart::Phrases], false).unwrap();
    assert!(kept.noop, "the track has a grid and phrases: {:?}", kept.summary);
    assert_eq!(
        kept.warnings,
        [
            "1 track(s) aren't analysed by Heard yet",
            "2 of what's asked the tracks have already, kept: overwrite to replace it"
        ]
    );
    f.plugins.analyze("echo", "heard", "USB", None, &f.store, &Ctx::silent()).unwrap();

    // Phrases alone count the analyzer's grid, not the tracks'.
    let alone = f.apply_analysis(&[AnalysisPart::Phrases], true).unwrap();
    assert!(alone.noop);
    assert_eq!(alone.warnings, ["2 track(s)' phrases left out: they count the beats of Heard's grid, not the track's"]);
    let waveform = f.apply_analysis(&[AnalysisPart::Waveform], true).unwrap();
    assert!(waveform.noop);
    assert!(waveform.warnings[0].starts_with("2 track(s) left out, e.g. "), "{:?}", waveform.warnings);
    assert!(waveform.warnings[0].contains("(PWV4)"), "{:?}", waveform.warnings);

    let planned = f.apply_analysis(&[AnalysisPart::Grid, AnalysisPart::Phrases], true).unwrap();
    assert_eq!(&*planned.operation, "plugins.apply_analysis");
    assert_eq!(planned.summary[0], "write the grid and phrases from Heard to 2 track(s)");
    planned.apply(None, &Ctx::silent()).unwrap();
    let device = locallibrary::Device::open_export(&f.drive).unwrap();
    for t in common::library(&f.drive).tracks {
        let a = anlz::load(&device, &t);
        assert_eq!((a.beats, a.structure, t.bpm), (grid(64, 25), Some(phrases()), 120.0));
    }
    let journal = api::edits_list(&locallibrary::api::DriveRef { drive: common::path(&f.drive) }).unwrap();
    assert_eq!(journal.len(), 1, "journaled");
    assert!(f.apply_analysis(&[AnalysisPart::Grid, AnalysisPart::Phrases], true).unwrap().noop, "as it is now");
}

#[test]
fn only_what_the_analyzer_works_out_is_written() {
    let f = Fixture::new(|dir| vec![spec(dir, &structured(), &[])], Options::default());
    let e = |parts: &[AnalysisPart]| f.apply_analysis(parts, false).unwrap_err().to_string();
    assert_eq!(e(&[]), "say what to write: grid, phrases, waveform");
    let req = ApplyAnalysis {
        plugin: "echo".into(),
        analyzer: "size".into(),
        drive: "USB".into(),
        parts: vec![AnalysisPart::Grid],
        tracks: Some(vec![9999]),
        overwrite: false,
    };
    let size = f.plugins.plan_apply_analysis(&req, &f.store, &Ctx::silent()).unwrap_err();
    assert_eq!(size.to_string(), "File size works out no grid");
    let req = ApplyAnalysis { analyzer: "heard".into(), ..req };
    assert_eq!(f.plugins.plan_apply_analysis(&req, &f.store, &Ctx::silent()).unwrap_err().code(), "not_found");
}

/// A manifest declaring a structured field needs the `analysis` feature: one whose plugin says it
/// hasn't it is refused as it starts.
#[test]
fn structured_fields_need_the_analysis_feature() {
    let f = Fixture::new(|dir| vec![spec(dir, &structured(), &["--without", "analysis"])], Options::default());
    let e = f.plugins.analyze("echo", "heard", "USB", None, &f.store, &Ctx::silent()).unwrap_err();
    assert!(failure(e).contains("it says it doesn't have the analysis feature, which its manifest needs"));
}

/// An operation's analysis edit takes `analysis:plan`, and gives its waveform as a file in its
/// scratch folder, which the host reads as it plans.
#[test]
fn an_operations_analysis_edit_takes_analysis_plan() {
    let plan = |f: &Fixture, edit: Value| {
        let params = json!({ "name": "x", "edits": [edit], "blobs": { "wave.bands": [1, 2, 3] } });
        f.plugins.plan_op("echo", "playlist", params, f.context(), Some(&f.store), &Ctx::silent())?.plan()
    };
    let without = Fixture::echo();
    let file = common::library(&without.drive).tracks[0].file_path.clone();
    let wave = json!({ "op": "set_analysis", "track": file, "waveform": { "blob": "wave.bands" } });
    let e = failure(plan(&without, wave.clone()).unwrap_err());
    assert!(e.contains("its edit 0 needs the analysis:plan capability, which it doesn't have"), "{e}");
    let with = Fixture::new(|dir| vec![spec(dir, &granting(&["analysis:plan"]), &[])], Options::default());
    // Read from the file, then refused: rekordbox's colour overview can't be worked out from it.
    let e = plan(&with, wave).unwrap_err().to_string();
    assert!(e.contains("(PWV4)"), "{e}");
    let given = json!({ "op": "set_analysis", "track": file, "waveform": { "base64": "AQID" } });
    let e = failure(plan(&with, given).unwrap_err());
    assert!(e.contains("its edit 0 gives a waveform otherwise than as a file in its scratch folder"), "{e}");
    let beats = json!({ "op": "set_analysis", "track": file, "beats": grid(8, 0) });
    let planned = plan(&with, beats).unwrap();
    let analysis = NewAnalysis { beats: Some(grid(8, 0)), ..Default::default() };
    assert_eq!(planned.plan.recorded[0], Edit::SetAnalysis { track: file, analysis });
}

/// What a track's drive lacks is drawn from what analyzers kept, each part from the first with
/// it, and said: a grid where it has none, phrases where it has none on the grid they count,
/// waveforms where it has none. What the drive has stays.
#[hegel::test(test_cases = 100)]
fn what_the_drive_lacks_is_drawn_from_the_overlay(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use locallibrary::anlz::TrackAnalysis;
    use locallibrary::fields::Overlay;
    use locallibrary::fields::overlay::Shown;
    let has = |tc: &hegel::TestCase| tc.draw(gs::booleans());
    let drive_grid = grid(32, 10);
    let drive = TrackAnalysis {
        beats: if has(&tc) { drive_grid.clone() } else { Vec::new() },
        structure: has(&tc).then(phrases),
        preview: has(&tc).then(|| Bands(vec![[1, 1, 1]]).waveform(None)),
        ..Default::default()
    };
    let own_grid = if has(&tc) { drive_grid.clone() } else { grid(32, 0) };
    let (a, b) = (heard(), Analyzer { analyzer: "other".into(), ..heard() });
    let overlay = Overlay {
        beats: has(&tc).then(|| own_grid.clone()),
        phrases: has(&tc).then(phrases),
        waveform: has(&tc).then(|| Bands(vec![[9, 9, 9]; 300])),
    };
    let from = |given: bool| given.then(|| if has(&tc) { a.clone() } else { b.clone() });
    let shown = Shown {
        beats_from: from(overlay.beats.is_some()),
        phrases_from: from(overlay.phrases.is_some()),
        phrases_grid: overlay.beats.clone(),
        waveform_from: from(overlay.waveform.is_some()),
        overlay: overlay.clone(),
    };
    let mut filled = drive.clone();
    let said = shown.clone().fill(&mut filled);

    let grid_in = drive.beats.is_empty() && overlay.beats.is_some();
    assert_eq!(filled.beats, if grid_in { own_grid.clone() } else { drive.beats.clone() });
    let counts = overlay.beats.as_ref().is_none_or(|own| *own == filled.beats);
    let phrases_in = drive.structure.is_none() && overlay.phrases.is_some() && counts;
    assert_eq!(filled.structure.is_some(), drive.structure.is_some() || phrases_in);
    let wave_in = overlay.waveform.is_some() && (drive.preview.is_none() || drive.detail.is_none());
    match &drive.preview {
        Some(p) => assert_eq!(filled.preview.as_ref(), Some(p)),
        None => assert_eq!(filled.preview.as_ref().map(|p| p.columns.len()), wave_in.then_some(1200)),
    }
    assert_eq!(
        filled.detail.as_ref().map(|d| (d.columns.len(), d.columns_per_sec)),
        wave_in.then_some((300, Some(150.0)))
    );
    let parts: Vec<&str> = [(grid_in, "grid"), (phrases_in, "phrases"), (wave_in, "waveform")]
        .into_iter()
        .filter_map(|(on, part)| on.then_some(part))
        .collect();
    let listed = match parts.as_slice() {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    match said {
        None => assert!(parts.is_empty()),
        Some(said) => {
            assert!(said.starts_with(&format!("{listed} from echo.")) && said.ends_with(", not on the drive"), "{said}")
        }
    }
}

/// Any grid and waveform an analyzer answers are kept as given (the waveform put at 150 columns a
/// second, as long), and planned onto the tracks as given, each with its BPM.
#[hegel::test(test_cases = 6, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn any_structured_answer_is_kept_and_planned_as_given(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let f = Fixture::new(|dir| vec![spec(dir, &structured(), &[])], Options::default());
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(64));
    let mut time = 0;
    let beats: Vec<Beat> = (0..n)
        .map(|i| {
            time += tc.draw(gs::integers::<u32>().min_value(1).max_value(900));
            let bpm = f64::from(tc.draw(gs::integers::<u16>().min_value(6_000).max_value(18_000))) / 100.0;
            Beat { beat_number: (i % 4) as u16 + 1, bpm, time_ms: time }
        })
        .collect();
    let columns = tc.draw(gs::integers::<usize>().min_value(1).max_value(400));
    let bytes: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>()).min_size(3 * columns).max_size(3 * columns));
    let rate = tc.draw(gs::sampled_from(vec![75.0, 150.0, 300.0]));
    let answer = json!({
        "grid": beats,
        "wave": { "blob": "w.bands", "columns_per_sec": rate },
        "blobs": { "w.bands": bytes },
    });
    let tracks = f.hear_all(&[answer.clone(), answer]);
    let analyzed = f.plugins.analyze("echo", "heard", "USB", None, &f.store, &Ctx::silent()).unwrap();
    assert_eq!(analyzed.analyzed, 2, "{:?}", analyzed.failures);
    let kept = OverlayStore::in_store(&f.store).get(&f.audio(&tracks[0]), &heard()).unwrap();
    assert_eq!(kept.beats.as_ref(), Some(&beats));
    assert_eq!(kept.waveform, anlz::at_detail_rate(&Bands::unpack(&bytes).unwrap(), rate).ok());
    let planned = f.apply_analysis(&[AnalysisPart::Grid], true).unwrap();
    let grids: Vec<Option<&Vec<Beat>>> = planned
        .plan
        .recorded
        .iter()
        .filter_map(|e| match e {
            Edit::SetAnalysis { analysis, .. } => Some(analysis.beats.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(grids, [Some(&beats), Some(&beats)]);
}

/// What an analyzer kept reads back as it was put, for the audio file as it is now (changed, it
/// has nothing kept), and is drawn from the first analyzer with each part. Names that aren't a
/// plugin's, an analyzer's or a version are refused, an audio file that isn't there keeps
/// nothing, and a stored overlay that's damaged reads as none.
#[hegel::test(test_cases = 100)]
fn overlays_read_back_as_kept_and_refuse_what_isnt_theirs(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use locallibrary::fields::Overlay;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let overlays = OverlayStore::in_store(&store);
    let audio = dir.path().join("a.mp3");
    std::fs::write(&audio, b"audio").unwrap();
    let drawn = || {
        let columns = tc.draw(gs::binary().max_size(30));
        Overlay {
            beats: tc.draw(gs::booleans()).then(|| grid(tc.draw(gs::integers::<usize>().max_value(40)), 0)),
            phrases: tc.draw(gs::booleans()).then(phrases),
            waveform: tc
                .draw(gs::booleans())
                .then(|| Bands(columns.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect())),
        }
    };
    let (first, second) = (drawn(), drawn());
    let other = Analyzer { analyzer: "other".into(), ..heard() };
    let stored = || {
        walkdir::WalkDir::new(store.root().join("cache/overlay"))
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .count()
    };
    match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => {
            let bad = tc.draw(gs::sampled_from(vec!["", ".hidden", "a/b", "a b", "..", "é"]));
            let named = match tc.draw(gs::integers::<u8>().max_value(2)) {
                0 => Analyzer { plugin: bad.into(), ..heard() },
                1 => Analyzer { analyzer: bad.into(), ..heard() },
                _ => Analyzer { version: bad.into(), ..heard() },
            };
            let e = overlays.put(&audio, &named, &first).unwrap_err().to_string();
            assert_eq!(e, format!("{bad:?} isn't a plugin id, version or analyzer"));
            assert_eq!(stored(), 0);
        }
        1 => {
            let gone = dir.path().join("gone.mp3");
            assert_eq!(overlays.put(&gone, &heard(), &first).unwrap_err().code(), "not_found");
            assert_eq!(overlays.get(&gone, &heard()), None);
            assert_eq!(stored(), 0);
        }
        2 => {
            overlays.put(&audio, &heard(), &first).unwrap();
            let file = walkdir::WalkDir::new(store.root().join("cache/overlay"))
                .into_iter()
                .flatten()
                .find(|e| e.file_type().is_file())
                .unwrap();
            let bytes = std::fs::read(file.path()).unwrap();
            let keep = tc.draw(gs::integers::<usize>().max_value(bytes.len() - 1));
            std::fs::write(file.path(), &bytes[..keep]).unwrap();
            assert_eq!(overlays.get(&audio, &heard()), None);
        }
        _ => {
            overlays.put(&audio, &heard(), &first).unwrap();
            overlays.put(&audio, &other, &second).unwrap();
            assert_eq!(overlays.get(&audio, &heard()).as_ref(), Some(&first));
            let shown = overlays.first(&audio, &[heard(), other.clone()]);
            let pick = |mine: bool, theirs: bool| match (mine, theirs) {
                (true, _) => Some(heard()),
                (false, true) => Some(other.clone()),
                _ => None,
            };
            assert_eq!(shown.beats_from, pick(first.beats.is_some(), second.beats.is_some()));
            assert_eq!(shown.phrases_from, pick(first.phrases.is_some(), second.phrases.is_some()));
            assert_eq!(shown.waveform_from, pick(first.waveform.is_some(), second.waveform.is_some()));
            let beats = if first.beats.is_some() { &first.beats } else { &second.beats };
            assert_eq!(&shown.overlay.beats, beats);
            // A changed audio file has nothing kept.
            std::fs::write(&audio, b"another take").unwrap();
            assert_eq!(overlays.get(&audio, &heard()), None);
        }
    }
}

/// Writing what an analyzer kept is refused, before anything's planned, when it isn't asked for
/// anything, when the analyzer doesn't work out what's asked, and for an analyzer, a plugin, a
/// drive or a track that isn't there.
#[hegel::test(test_cases = 12, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn writing_an_analysis_is_refused_for_what_isnt_there(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let f = Fixture::new(|dir| vec![spec(dir, &structured(), &[])], Options::default());
    let parts = tc.draw(gs::vecs(gs::sampled_from(AnalysisPart::ALL.to_vec())).min_size(1).max_size(3));
    let req = ApplyAnalysis {
        plugin: "echo".into(),
        analyzer: "heard".into(),
        drive: "USB".into(),
        parts: parts.clone(),
        tracks: None,
        overwrite: tc.draw(gs::booleans()),
    };
    let (req, code) = match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => (ApplyAnalysis { parts: Vec::new(), ..req }, "invalid"),
        1 => (ApplyAnalysis { analyzer: "size".into(), ..req }, "invalid"),
        2 => (ApplyAnalysis { analyzer: "unheard".into(), ..req }, "not_found"),
        3 => (ApplyAnalysis { plugin: "nobody".into(), ..req }, "not_found"),
        4 => (ApplyAnalysis { drive: "NOWHERE".into(), ..req }, "not_found"),
        _ => {
            let id = tc.draw(gs::integers::<u32>().min_value(10_000));
            (ApplyAnalysis { tracks: Some(vec![id]), ..req }, "not_found")
        }
    };
    let e = f.plugins.plan_apply_analysis(&req, &f.store, &Ctx::silent()).unwrap_err();
    assert_eq!(e.code(), code, "{e}");
    if req.analyzer == "size" {
        assert_eq!(e.to_string(), format!("File size works out no {}", parts[0].name()));
    }
}

/// A waveform an analyzer says is slower than a day's worth of columns fails the track, saying
/// so, rather than being pooled out to fill memory, and keeps nothing of it.
#[hegel::test(test_cases = 6, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn waveforms_over_a_day_fail_the_track(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let f = Fixture::new(|dir| vec![spec(dir, &structured(), &[])], Options::default());
    let columns = tc.draw(gs::integers::<usize>().min_value(1).max_value(100));
    let rate = tc.draw(gs::floats::<f64>().min_value(f64::MIN_POSITIVE).max_value(1e-5));
    let answer = json!({
        "wave": { "blob": "w.bands", "columns_per_sec": rate },
        "blobs": { "w.bands": vec![7u8; 3 * columns] },
    });
    let tracks = f.hear_all(&[answer]);
    let analyzed = f.plugins.analyze("echo", "heard", "USB", Some(&[tracks[0].id]), &f.store, &Ctx::silent()).unwrap();
    let error = &analyzed.failures[0].error;
    assert!(error.ends_with(&format!("its wave: {columns} columns at {rate} a second run over a day")), "{error}");
    assert_eq!(OverlayStore::in_store(&f.store).get(&f.audio(&tracks[0]), &heard()), None);
}

/// Any beat grid, phrases and waveform an analyzer answers are kept as it gave them (nulls and an
/// empty grid as nothing), or fail the track, saying why, keeping nothing: a beat out of its bar,
/// of a tempo out of range or not after the one before, a waveform whose file isn't there or
/// isn't three bytes a column.
#[hegel::test(test_cases = 20, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn any_structured_answer_is_kept_or_fails_the_track_saying_why(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let f = Fixture::new(|dir| vec![spec(dir, &structured(), &[])], Options::default());
    let beat = |n: u16, bpm: f64, ms: u32| Beat { beat_number: n, bpm, time_ms: ms };
    let wave = |blob: &str| json!({ "blob": blob, "columns_per_sec": 150.0 });
    let columns = tc.draw(gs::integers::<usize>().min_value(1).max_value(200));
    let packed = Bands((0..columns).map(|i| [i as u8, 7, 9]).collect()).pack();
    let (answer, why): (Value, Option<String>) = match tc.draw(gs::integers::<u8>().max_value(6)) {
        0 => {
            let grid = tc.draw(gs::sampled_from(vec![json!(null), json!([])]));
            (json!({ "grid": grid, "phrases": null, "wave": null, "tempo": 1 }), None)
        }
        1 => (json!({ "phrases": phrases() }), None),
        2 => {
            let n = tc.draw(hegel::one_of!(gs::just(0u16), gs::integers::<u16>().min_value(5)));
            (
                json!({ "grid": [beat(1, 120.0, 0), beat(n, 120.0, 500)] }),
                Some(format!("its grid: beat 2 is beat {n} of its bar: 1-4")),
            )
        }
        3 => {
            let bpm = tc.draw(hegel::one_of!(
                gs::floats::<f64>().min_value(0.0).max_value(0.004),
                gs::floats::<f64>().min_value(655.36).max_value(1e9)
            ));
            (
                json!({ "grid": [beat(1, bpm, 0)] }),
                Some(format!("its grid: beat 1's tempo, {bpm} BPM, isn't 0.01-655.35")),
            )
        }
        4 => {
            let ms = tc.draw(gs::integers::<u32>().max_value(10_000));
            let back = tc.draw(gs::integers::<u32>().max_value(ms));
            (
                json!({ "grid": [beat(1, 120.0, ms), beat(2, 120.0, back)] }),
                Some("its grid: beat 2 isn't after the one before it".into()),
            )
        }
        5 => (json!({ "wave": wave("gone.bands") }), Some("its wave: its file \"gone.bands\" isn't there".into())),
        _ => {
            let mut bytes = tc.draw(gs::binary().max_size(40));
            if !bytes.is_empty() && bytes.len().is_multiple_of(3) {
                bytes.pop();
            }
            let answer = json!({ "wave": wave("w.bands"), "blobs": { "w.bands": bytes } });
            (answer, Some("its wave isn't three bytes a column, at least one column".into()))
        }
    };
    // The other track's answer is always kept: a grid and a waveform.
    let kept = json!({ "grid": grid(8, 0), "wave": wave("w.bands"), "blobs": { "w.bands": packed } });
    let tracks = f.hear_all(&[answer.clone(), kept]);
    let analyzed = f.plugins.analyze("echo", "heard", "USB", None, &f.store, &Ctx::silent()).unwrap();
    let overlays = OverlayStore::in_store(&f.store);
    let (first, second) = (overlays.get(&f.audio(&tracks[0]), &heard()), overlays.get(&f.audio(&tracks[1]), &heard()));
    let second = second.unwrap();
    assert_eq!((second.beats, second.waveform.map(|w| w.0.len())), (Some(grid(8, 0)), Some(columns)));
    match why {
        Some(why) => {
            assert_eq!((analyzed.analyzed, analyzed.failed), (1, 1));
            let error = &analyzed.failures[0].error;
            assert!(error.contains(&why), "{error}");
            assert_eq!(first, None);
        }
        None => {
            assert_eq!((analyzed.analyzed, analyzed.failed), (2, 0), "{:?}", analyzed.failures);
            let phrases = answer.get("phrases").filter(|p| !p.is_null()).map(|_| phrases());
            assert_eq!(first.map(|o| o.phrases), phrases.map(Some), "nothing to keep, nothing kept");
        }
    }
}

/// For any of what an analyzer kept for each track (a grid like the drive's or another, phrases,
/// a waveform, or nothing yet: the track failed it or it answered none of them), parts asked for, overwriting or not, of every track or one, and
/// a track's files that can take a waveform or can't: the plan writes to each track just what's
/// asked that it gives, over what the track has only when overwriting, phrases only on the grid
/// they count and no waveform where it can't go (leaving the track out), saying what it left out
/// and why; applied, the drive has what it planned.
#[hegel::test(test_cases = 50, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn any_kept_analysis_is_planned_as_asked_and_left_out_as_said(tc: hegel::TestCase) {
    use crate::analysis_edit::strip_colour_overview;
    use hegel::generators as gs;
    let f = Fixture::new(|dir| vec![spec(dir, &structured(), &[])], Options::default());
    let device = locallibrary::Device::open_export(&f.drive).unwrap();
    let tracks = common::library(&f.drive).tracks;
    let own = grid(64, 25);
    let mut answers = Vec::new();
    // Each track's: analysed, its answer's grid, phrases and waveform, its files taking one.
    let mut drawn = Vec::new();
    for t in &tracks {
        let on_drive = anlz::load(&device, t).beats;
        let analysed = tc.draw(gs::weighted_booleans(0.8));
        let beats = tc.draw(gs::sampled_from(vec![None, Some(on_drive), Some(own.clone())]));
        let (phrased, waved) = (tc.draw(gs::weighted_booleans(0.7)), tc.draw(gs::booleans()));
        let takes_wave = tc.draw(gs::booleans());
        if takes_wave {
            strip_colour_overview(&f.drive, t);
        }
        let mut answer = json!({ "grid": beats, "tempo": 120.0 });
        if phrased {
            answer["phrases"] = json!(phrases());
        }
        if waved {
            answer["wave"] = json!({ "blob": "w.bands", "columns_per_sec": 150.0 });
            answer["blobs"] = json!({ "w.bands": Bands(vec![[1, 2, 3]; 300]).pack() });
        }
        // An audio file that isn't the answer's JSON fails the track: nothing kept for it.
        answers.push(if analysed { answer } else { json!("not an answer") });
        drawn.push((analysed, beats, phrased, waved, takes_wave));
    }
    f.hear_all(&answers);
    f.plugins.analyze("echo", "heard", "USB", None, &f.store, &Ctx::silent()).unwrap();
    let device = locallibrary::Device::open_export(&f.drive).unwrap();
    let before: Vec<_> = tracks.iter().map(|t| anlz::load(&device, t)).collect();

    let mut parts: Vec<AnalysisPart> =
        AnalysisPart::ALL.into_iter().filter(|_| tc.draw(gs::weighted_booleans(0.6))).collect();
    if parts.is_empty() {
        parts.push(tc.draw(gs::sampled_from(AnalysisPart::ALL.to_vec())));
    }
    let overwrite = tc.draw(gs::weighted_booleans(0.7));
    let only = tc.draw(gs::optional(gs::integers::<usize>().max_value(tracks.len() - 1)));
    let req = ApplyAnalysis {
        plugin: "echo".into(),
        analyzer: "heard".into(),
        drive: "USB".into(),
        parts: parts.clone(),
        tracks: only.map(|i| vec![tracks[i].id]),
        overwrite,
    };
    let planned = f.plugins.plan_apply_analysis(&req, &f.store, &Ctx::silent()).unwrap();

    let wants = |p| parts.contains(&p);
    let (mut unanalysed, mut had, mut other_grid, mut refused) = (0, 0, 0, 0);
    // Each written track's new grid and phrases.
    let mut written: Vec<(usize, Option<Vec<Beat>>, bool, bool)> = Vec::new();
    for (i, (analysed, beats, phrased, waved, takes_wave)) in drawn.iter().enumerate() {
        if only.is_some_and(|o| o != i) {
            continue;
        }
        // An answer with none of them keeps no overlay: the plan has nothing of it.
        if !analysed || (beats.is_none() && !phrased && !waved) {
            unanalysed += 1;
            continue;
        }
        let drive = &before[i];
        let mut take = |wanted: bool, given: bool, has: bool| {
            had += usize::from(wanted && given && !overwrite && has);
            wanted && given && (overwrite || !has)
        };
        let grid_in = take(wants(AnalysisPart::Grid), beats.is_some(), !drive.beats.is_empty());
        let ends_with = if grid_in { beats.clone().unwrap() } else { drive.beats.clone() };
        let counts = beats.as_ref().is_none_or(|b| *b == ends_with);
        let phrases_asked = take(wants(AnalysisPart::Phrases), *phrased, drive.structure.is_some());
        other_grid += usize::from(phrases_asked && !counts);
        let phrases_in = phrases_asked && counts;
        let wave_in = take(wants(AnalysisPart::Waveform), *waved, drive.preview.is_some() || drive.detail.is_some());
        if wave_in && !takes_wave {
            refused += 1;
            continue;
        }
        let grid_changes = grid_in && ends_with != drive.beats;
        let phrases_change = phrases_in && drive.structure != Some(phrases());
        if grid_changes || phrases_change || wave_in {
            written.push((i, grid_in.then_some(ends_with), phrases_in, wave_in));
        }
    }
    let what: Vec<&str> = AnalysisPart::ALL.into_iter().filter(|p| wants(*p)).map(AnalysisPart::name).collect();
    assert_eq!(
        planned.summary[0],
        format!("write the {} from Heard to {} track(s)", what.join(" and "), written.len())
    );
    let yet = format!("{unanalysed} track(s) aren't analysed by Heard yet");
    assert_eq!(planned.warnings.contains(&yet), unanalysed > 0, "{:?}", planned.warnings);
    let kept = format!("{had} of what's asked the tracks have already, kept: overwrite to replace it");
    assert_eq!(planned.warnings.contains(&kept), had > 0, "{:?}", planned.warnings);
    let counted =
        format!("{other_grid} track(s)' phrases left out: they count the beats of Heard's grid, not the track's");
    assert_eq!(planned.warnings.contains(&counted), other_grid > 0, "{:?}", planned.warnings);
    let left_out = planned.warnings.iter().find(|w| w.contains("track(s) left out, e.g. "));
    assert_eq!(left_out.is_some(), refused > 0, "{:?}", planned.warnings);
    if let Some(w) = left_out {
        assert!(w.starts_with(&format!("{refused} track(s) left out, e.g. ")) && w.contains("(PWV4)"), "{w}");
    }
    let set: Vec<(&String, &NewAnalysis)> = planned
        .plan
        .recorded
        .iter()
        .filter_map(|e| match e {
            Edit::SetAnalysis { track, analysis } => Some((track, analysis)),
            _ => None,
        })
        .collect();
    assert_eq!(set.len(), written.len());
    for ((track, analysis), (i, beats, phrases_in, wave_in)) in set.iter().zip(&written) {
        assert_eq!(*track, &tracks[*i].file_path);
        assert_eq!(
            (&analysis.beats, analysis.phrases.is_some(), analysis.waveform.is_some()),
            (beats, *phrases_in, *wave_in)
        );
    }
    assert_eq!(planned.noop, written.is_empty());
    if written.is_empty() {
        return;
    }
    planned.apply(None, &Ctx::silent()).unwrap();
    let device = locallibrary::Device::open_export(&f.drive).unwrap();
    let after = common::library(&f.drive).tracks;
    for (i, beats, phrases_in, wave_in) in &written {
        let a = anlz::load(&device, &after[*i]);
        assert_eq!(&a.beats, beats.as_ref().unwrap_or(&before[*i].beats));
        if beats.as_ref() == Some(&own) {
            assert_eq!(after[*i].bpm, 120.0);
        }
        assert_eq!(a.structure, if *phrases_in { Some(phrases()) } else { before[*i].structure.clone() });
        if *wave_in {
            let detail = a.detail.unwrap();
            assert_eq!((detail.columns.len(), detail.columns_per_sec), (300, Some(150.0)));
        }
    }
}
