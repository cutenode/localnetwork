//! Beat grids, phrases and waveforms set as edits: written into the track's analysis files with
//! its BPM, the rest of each file as it was, recorded with a waveform staged on the drive, taken
//! back by undo, made again by a replay after rekordbox exports, and refused where they can't be
//! stored.

use crate::common;

use std::path::Path;

use base64::prelude::*;
use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::analysis_edit::{NewAnalysis, WaveSource};
use locallibrary::anlz::{
    self, Bands, Beat, CueFile, CuePoint, Mood, Phrase, PhraseKind, SongStructure, WaveformStyle,
};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions, EditPlan, TrackFields};
use locallibrary::model::Track;
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::sound::{Plausible, TempoFinding, TempoRelation};
use locallibrary::{Hash, backup, doctor, paths};

fn plan(root: &Path, edits: &[Edit]) -> locallibrary::Result<EditPlan> {
    edit::plan(&Device::open_export(root).unwrap(), edits, &EditOptions::default(), &Ctx::silent())
}

fn set(t: &Track, analysis: NewAnalysis) -> Edit {
    Edit::SetAnalysis { track: t.file_path.clone(), analysis }
}

fn track(root: &Path, title: &str) -> Track {
    let lib = Device::open_export(root).unwrap().library().unwrap();
    lib.tracks.iter().find(|t| t.title == title).unwrap_or(&lib.tracks[0]).clone()
}

/// A track's analysis files on the drive: `.DAT`, `.EXT` and `.2EX`, those it has.
fn files(root: &Path, t: &Track) -> [Option<Vec<u8>>; 3] {
    let dat = paths::to_fs(root, &paths::key(&t.analyze_path));
    ["DAT", "EXT", "2EX"].map(|e| std::fs::read(dat.with_extension(e)).ok())
}

/// `beats` beats at `bpm`, the first at `from` ms.
fn steady(beats: usize, bpm: f64, from: u32) -> Vec<Beat> {
    (0..beats)
        .map(|i| Beat { beat_number: (i % 4) as u16 + 1, bpm, time_ms: from + (i as f64 * 60_000.0 / bpm) as u32 })
        .collect()
}

/// A grid set on a rekordbox 6 export's track: its `.DAT` holds it and the databases its tempo,
/// the `.EXT` loses its extended grid (saying so) and nothing else changes; recorded with the
/// BPM edit after it, in a journal of version 4. Undo puts the files and the BPM back, and a
/// replay after rekordbox exports again makes it again.
#[test]
fn a_beat_grid_is_written_with_its_bpm_undone_and_replayed() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let exported = common::library_files(&root);
    let t = track(&root, "Demo Track 1");
    let before = files(&root, &t);
    let beats = steady(300, 125.0, 40);
    let edit = set(&t, NewAnalysis { beats: Some(beats.clone()), ..Default::default() });
    let p = plan(&root, std::slice::from_ref(&edit)).unwrap();
    assert_eq!(p.changes[0], format!("set the beat grid of {}", t.file_path));
    assert!(p.warnings.iter().any(|w| w.contains("extended beat grid (PQT2)")), "{:?}", p.warnings);
    let tempo = Edit::EditTracks {
        tracks: TrackSelector::paths([t.file_path.clone()]),
        fields: TrackFields { bpm: Some(125.0), ..Default::default() },
    };
    assert_eq!(p.recorded, [edit, tempo]);
    let mut d = Device::open_export(&root).unwrap();
    edit::apply(&p, &mut d, &Ctx::silent()).unwrap();

    let after = files(&root, &t);
    let [dat, ext, _] = &after;
    assert_eq!(anlz::beat_grid(dat.as_ref().unwrap()).unwrap(), beats);
    assert!(!anlz::has_section(ext.as_ref().unwrap(), b"PQT2").unwrap());
    assert_eq!(after[2], before[2], "the .2EX holds no grid");
    for (a, b) in after.iter().zip(&before) {
        let (a, b) = (a.as_deref().unwrap(), b.as_deref().unwrap());
        assert_eq!(anlz::song_structure(a).unwrap(), anlz::song_structure(b).unwrap());
        assert_eq!(anlz::read_cue_points(a, None).unwrap(), anlz::read_cue_points(b, None).unwrap());
        assert_eq!(anlz::detail(a).unwrap(), anlz::detail(b).unwrap());
    }
    assert_eq!(track(&root, "Demo Track 1").bpm, 125.0);
    let journal = std::fs::read(root.join(paths::STATE_DIR).join("edits.json")).unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&journal).unwrap()["version"], 4);

    backup::restore(&mut d, None, false).unwrap();
    assert_eq!(files(&root, &t), before);
    assert_eq!(track(&root, "Demo Track 1").bpm, t.bpm);

    // Applied again, then rekordbox exports over it: a replay writes the same files again.
    let p = plan(&root, &[set(&t, NewAnalysis { beats: Some(beats), ..Default::default() })]).unwrap();
    edit::apply(&p, &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap();
    let edited = files(&root, &t);
    for (key, bytes) in &exported {
        std::fs::write(paths::to_fs(&root, key), bytes).unwrap();
    }
    assert_eq!(files(&root, &t), before);
    common::replay(&root);
    assert_eq!(files(&root, &t), edited);
    assert_eq!(track(&root, "Demo Track 1").bpm, 125.0);
}

/// `doctor --fix-tempo`'s edits: a track heard at twice its stored tempo gets its grid doubled
/// and its BPM with it, one heard at half gets them halved, and the plan says the extended grid
/// (PQT2) isn't rescaled; undo puts both back. A tempo the library favours as stored, one that's
/// 3/2 off, and a grid at another tempo than the stored one are left as they are.
#[test]
fn doctor_doubles_and_halves_grids_heard_at_double_or_half_tempo() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let d = Device::open_export(&root).unwrap();
    let (a, b) = (track(&root, "Demo Track 1"), track(&root, "Demo Track 2"));
    assert_ne!(a.id, b.id);
    let heard = |t: &Track, relation, likelier| TempoFinding {
        track: t.into(),
        stored: t.bpm,
        estimate: 0.0,
        confidence: None,
        relation,
        likelier,
    };
    let grid = |t: &Track| anlz::beat_grid(files(&root, t)[0].as_ref().unwrap()).unwrap();
    let (grid_a, grid_b) = (grid(&a), grid(&b));
    assert!(!grid_a.is_empty() && !grid_b.is_empty());

    let (left, notes) = doctor::tempo_fixes(
        &d,
        &[heard(&a, TempoRelation::Double, Some(Plausible::Stored)), heard(&b, TempoRelation::ThreeHalves, None)],
    )
    .unwrap();
    assert!(left.is_empty(), "{left:?}");
    assert_eq!(notes, ["1 track(s) left as they are: the library's other tempos favour the stored one"]);

    let findings = [heard(&a, TempoRelation::Half, None), heard(&b, TempoRelation::Double, Some(Plausible::Estimate))];
    let (edits, notes) = doctor::tempo_fixes(&d, &findings).unwrap();
    assert!(notes.iter().any(|n| n.contains("(PQT2) isn't decoded, so it isn't rescaled")), "{notes:?}");
    let p = plan(&root, &edits).unwrap();
    assert_eq!(
        p.changes[..2],
        [format!("set the beat grid of {}", a.file_path), format!("set the beat grid of {}", b.file_path)]
    );
    let before = [files(&root, &a), files(&root, &b)];
    edit::apply(&p, &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap();
    assert_eq!(grid(&a), anlz::doubled_grid(&grid_a));
    assert_eq!(grid(&b), anlz::halved_grid(&grid_b));
    let tempo = |t: &Track| NewAnalysis { beats: Some(grid(t)), ..Default::default() }.tempo().unwrap();
    assert_eq!(track(&root, "Demo Track 1").bpm, tempo(&a));
    assert_eq!(track(&root, "Demo Track 2").bpm, tempo(&b));
    assert!((tempo(&a) - 2.0 * a.bpm).abs() < 0.02 && (tempo(&b) - b.bpm / 2.0).abs() < 0.02, "{} {}", a.bpm, b.bpm);

    backup::restore(&mut Device::open_export(&root).unwrap(), None, false).unwrap();
    assert_eq!([files(&root, &a), files(&root, &b)], before);
    assert_eq!(track(&root, "Demo Track 1").bpm, a.bpm);

    // Stored at 3/2 of its grid's tempo, doubling the grid wouldn't fix the stored tempo.
    let fields = TrackFields { bpm: Some(a.bpm * 1.5), ..Default::default() };
    let p = plan(&root, &[Edit::EditTracks { tracks: TrackSelector::paths([a.file_path.clone()]), fields }]).unwrap();
    edit::apply(&p, &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap();
    let d = Device::open_export(&root).unwrap();
    let (left, notes) = doctor::tempo_fixes(&d, &[heard(&a, TempoRelation::Half, None)]).unwrap();
    assert!(left.is_empty(), "{left:?}");
    assert_eq!(notes, ["1 track(s) left as they are: their beat grid isn't at their stored tempo"]);
}

/// Phrases set on a track go into its `.EXT`, as given, and nothing else there changes.
#[test]
fn phrases_are_written_into_the_ext() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let t = track(&root, "Demo Track 2");
    let before = files(&root, &t);
    let phrase = |kind, start_beat, end_beat| Phrase { kind, start_beat, end_beat, fill_beat: None };
    let phrases = SongStructure {
        mood: Mood::High,
        phrases: vec![
            phrase(PhraseKind::Intro, 1, 65),
            Phrase { fill_beat: Some(125), ..phrase(PhraseKind::Up, 65, 129) },
        ],
    };
    let p = plan(&root, &[set(&t, NewAnalysis { phrases: Some(phrases.clone()), ..Default::default() })]).unwrap();
    assert_eq!(p.recorded.len(), 1, "no tempo without a grid");
    edit::apply(&p, &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap();
    let after = files(&root, &t);
    assert_eq!(after[0], before[0]);
    assert_eq!(anlz::song_structure(after[1].as_ref().unwrap()).unwrap(), Some(phrases));
    let others =
        |b: &[u8]| (anlz::detail(b).unwrap(), anlz::preview(b).unwrap(), anlz::read_cue_points(b, None).unwrap());
    assert_eq!(others(after[1].as_ref().unwrap()), others(before[1].as_ref().unwrap()));
}

/// The demo drive with a track's analysis files without rekordbox's colour overview (`PWV4`) and
/// the `.2EX`.
fn without_colour_overview(dir: &Path) -> (std::path::PathBuf, Track) {
    let root = common::demo_drive(dir, "USB");
    let t = track(&root, "Demo Track 1");
    strip_colour_overview(&root, &t);
    (root, t)
}

/// Takes rekordbox's colour overview (`PWV4`) out of the `.EXT` of `t` on the drive at `root`,
/// and its `.2EX` away: a waveform can then be written to it.
pub(crate) fn strip_colour_overview(root: &Path, t: &Track) {
    let dat = paths::to_fs(root, &paths::key(&t.analyze_path));
    let ext = std::fs::read(dat.with_extension("EXT")).unwrap();
    let be = |at: usize| u32::from_be_bytes(ext[at..at + 4].try_into().unwrap()) as usize;
    let mut pos = be(4);
    while &ext[pos..pos + 4] != b"PWV4" {
        pos += be(pos + 8);
    }
    let mut without = [&ext[..pos], &ext[pos + be(pos + 8)..]].concat();
    let len = without.len() as u32;
    without[8..12].copy_from_slice(&len.to_be_bytes());
    std::fs::write(dat.with_extension("EXT"), without).unwrap();
    std::fs::remove_file(dat.with_extension("2EX")).unwrap();
}

fn bands(n: usize) -> Bands {
    Bands((0..n).map(|i| [(i % 256) as u8, 128, (255 - i % 256) as u8]).collect())
}

fn wave(b: &Bands) -> WaveSource {
    WaveSource::Base64(BASE64_STANDARD.encode(b.pack()))
}

/// rekordbox's colour overview (`PWV4`) can't be worked out from another waveform, so a track
/// with one keeps its waveforms: the plan is refused, saying why.
#[test]
fn a_waveform_is_refused_for_a_track_with_rekordboxs_colour_overview() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let t = track(&root, "Demo Track 1");
    let err = plan(&root, &[set(&t, NewAnalysis { waveform: Some(wave(&bands(150))), ..Default::default() })]);
    let err = err.unwrap_err().to_string();
    assert!(err.contains("(PWV4) that can't be worked out from another waveform"), "{err}");
}

/// A waveform set on a track without rekordbox's colour one goes into every waveform tag its
/// files take, worked out from the bands; it's recorded staged on the drive by its hash, and a
/// replay after rekordbox exports writes the same files from the staged copy.
#[test]
fn a_waveform_is_written_staged_and_replayed() {
    let dir = tempfile::tempdir().unwrap();
    let (root, t) = without_colour_overview(dir.path());
    let exported = common::library_files(&root);
    let b = bands(30 * 150);
    let garbled = NewAnalysis { waveform: Some(WaveSource::Base64("AAA=".into())), ..Default::default() };
    let err = plan(&root, &[set(&t, garbled)]).unwrap_err().to_string();
    assert_eq!(err, "a waveform is three bytes a column (lows, mids, highs), at least one column");
    let p = plan(&root, &[set(&t, NewAnalysis { waveform: Some(wave(&b)), ..Default::default() })]).unwrap();
    let hash = Hash::of(&b.pack());
    let [Edit::SetAnalysis { analysis, .. }] = &p.recorded[..] else { panic!("{:?}", p.recorded) };
    assert_eq!(analysis.waveform, Some(WaveSource::Staged(hash)));
    let staged = root.join(paths::STATE_DIR).join("waveforms").join(format!("{hash}.bands"));
    assert!(!staged.exists(), "nothing on the drive until it's applied");
    assert!(p.warnings.iter().any(|w| w.contains("waveform is 30 s long")), "{:?}", p.warnings);
    edit::apply(&p, &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap();
    assert_eq!(std::fs::read(&staged).unwrap(), b.pack());

    let edited = files(&root, &t);
    let (dat, ext) = (edited[0].as_ref().unwrap(), edited[1].as_ref().unwrap());
    let preview = anlz::preview(dat).unwrap().unwrap();
    assert_eq!((preview.style, preview.columns.len()), (WaveformStyle::Blue, 400));
    let detail = anlz::detail(ext).unwrap().unwrap();
    assert_eq!(
        (detail.style, detail.columns.len(), detail.columns_per_sec),
        (WaveformStyle::Rgb, b.0.len(), Some(150.0))
    );
    assert_eq!(anlz::waveform_entries(ext, b"PWV3").unwrap().unwrap().len(), b.0.len());
    assert_eq!(anlz::waveform_entries(dat, b"PWV2").unwrap().unwrap().len(), 100);

    for (key, bytes) in &exported {
        std::fs::write(paths::to_fs(&root, key), bytes).unwrap();
    }
    common::replay(&root);
    assert_eq!(files(&root, &t), edited);
    // Forgotten with the journal.
    edit::journal::clear(&Device::open_export(&root).unwrap(), None).unwrap();
    assert!(!staged.exists());
}

/// Moving the grid leaves cues where they are, saying how many sat on beats of the old one.
#[test]
fn cues_on_beats_stay_put_when_the_grid_moves() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let t = track(&root, "Demo Track 1");
    let [dat, ext, _] = files(&root, &t);
    let (dat, ext) = (dat.unwrap(), ext.unwrap());
    let grid = anlz::beat_grid(&dat).unwrap();
    let cues = [
        CuePoint { hot_cue: Some(0), start_ms: grid[4].time_ms, ..Default::default() },
        CuePoint { start_ms: grid[8].time_ms + 100, ..Default::default() },
    ];
    let at = paths::to_fs(&root, &paths::key(&t.analyze_path));
    std::fs::write(&at, anlz::write_cue_points(&dat, CueFile::Dat, &cues).unwrap()).unwrap();
    std::fs::write(at.with_extension("EXT"), anlz::write_cue_points(&ext, CueFile::Ext, &cues).unwrap()).unwrap();
    let moved: Vec<Beat> = grid.iter().map(|b| Beat { time_ms: b.time_ms + 20, ..*b }).collect();
    let p = plan(&root, &[set(&t, NewAnalysis { beats: Some(moved), ..Default::default() })]).unwrap();
    let said = format!(
        "1 cue(s) of {} sat on beats of its old grid: they stay where they are, off the new one",
        t.display_name()
    );
    assert!(p.warnings.contains(&said), "{:?}", p.warnings);
    edit::apply(&p, &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap();
    let after = files(&root, &t);
    let back = anlz::read_cue_points(after[0].as_ref().unwrap(), after[1].as_deref()).unwrap();
    assert_eq!(back.iter().map(|c| c.start_ms).collect::<Vec<_>>(), [grid[8].time_ms + 100, grid[4].time_ms]);
}

/// What can't be written is refused before anything is: nothing to write, a track without
/// analysis files, phrases for a track without an `.EXT`, a beat past the track's end.
#[test]
fn analysis_that_cant_be_written_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let t = track(&root, "Demo Track 2");
    let refused = |t: &Track, a: NewAnalysis| plan(&root, &[set(t, a)]).unwrap_err().to_string();
    assert_eq!(refused(&t, NewAnalysis::default()), "say what to write: a beat grid, phrases or a waveform");
    let past = steady(4, 120.0, (t.duration_secs + 2) * 1000);
    let err = refused(&t, NewAnalysis { beats: Some(past), ..Default::default() });
    assert!(
        err.starts_with("beat 1 at ") && err.ends_with(&format!("is past the end of {}", t.display_name())),
        "{err}"
    );
    let dat = paths::to_fs(&root, &paths::key(&t.analyze_path));
    std::fs::remove_file(dat.with_extension("EXT")).unwrap();
    let phrases = SongStructure {
        mood: Mood::Mid,
        phrases: vec![Phrase { kind: PhraseKind::Intro, start_beat: 1, end_beat: 9, fill_beat: None }],
    };
    let err = refused(&t, NewAnalysis { phrases: Some(phrases), ..Default::default() });
    assert_eq!(err, format!("{} has no .EXT analysis file to hold phrases", t.display_name()));
    std::fs::remove_file(&dat).unwrap();
    let err = refused(&t, NewAnalysis { beats: Some(steady(4, 120.0, 0)), ..Default::default() });
    assert_eq!(err, format!("{} has no .DAT analysis file", t.display_name()));
    let gone = Edit::SetAnalysis { track: "/Contents/nowhere.mp3".into(), analysis: NewAnalysis::default() };
    assert!(plan(&root, &[gone]).unwrap_err().to_string().contains("/Contents/nowhere.mp3"));
}

/// Any grid and phrases set on a rekordbox 6 export's track read back as given, with the tempo
/// most beats have in the databases, and so does any waveform on a track without rekordbox's
/// colour one, staged on the drive; undo puts every file back.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn any_grid_phrases_and_waveform_read_back_and_undo(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let with_wave = tc.draw(gs::booleans());
    let (root, t) = match with_wave {
        true => without_colour_overview(dir.path()),
        false => {
            let root = common::demo_drive(dir.path(), "USB");
            let t = track(&root, "Demo Track 2");
            (root, t)
        }
    };
    let before = files(&root, &t);
    let n = tc.draw(gs::integers::<usize>().min_value(8).max_value(200));
    let bpms: Vec<u16> =
        tc.draw(gs::vecs(gs::integers::<u16>().min_value(6_000).max_value(18_000)).min_size(1).max_size(3));
    let beats: Vec<Beat> = (0..n)
        .map(|i| {
            let bpm = f64::from(bpms[i * bpms.len() / n]) / 100.0;
            Beat { beat_number: (i % 4) as u16 + 1, bpm, time_ms: 10 + i as u32 * 400 }
        })
        .collect();
    let ends: Vec<u16> = {
        let mut e: Vec<u16> =
            tc.draw(gs::vecs(gs::integers::<u16>().min_value(2).max_value(n as u16)).min_size(1).max_size(6));
        e.sort_unstable();
        e.dedup();
        e
    };
    let kinds = [PhraseKind::Intro, PhraseKind::Verse(1), PhraseKind::Bridge, PhraseKind::Chorus, PhraseKind::Outro];
    let mut start = 1;
    let phrases = ends
        .iter()
        .map(|&end| {
            let p = Phrase {
                kind: tc.draw(gs::sampled_from(kinds.to_vec())),
                start_beat: start,
                end_beat: end,
                fill_beat: None,
            };
            start = end;
            p
        })
        .collect();
    let phrases = SongStructure { mood: Mood::Low, phrases };
    let columns = tc.draw(gs::integers::<usize>().min_value(1).max_value(900));
    let bytes: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>()).min_size(3 * columns).max_size(3 * columns));
    let bands = Bands::unpack(&bytes).unwrap();
    let waveform = with_wave.then(|| wave(&bands));
    let analysis = NewAnalysis { beats: Some(beats.clone()), phrases: Some(phrases.clone()), waveform };
    let p = plan(&root, &[set(&t, analysis.clone())]).unwrap();
    let mut d = Device::open_export(&root).unwrap();
    edit::apply(&p, &mut d, &Ctx::silent()).unwrap();
    let after = files(&root, &t);
    let (dat, ext) = (after[0].as_ref().unwrap(), after[1].as_ref().unwrap());
    assert_eq!(anlz::beat_grid(dat).unwrap(), beats);
    assert_eq!(anlz::song_structure(ext).unwrap(), Some(phrases));
    assert_eq!(Some(track(&root, &t.title).bpm), analysis.tempo());
    if with_wave {
        let detail: Vec<u8> = anlz::waveform_from_bands(b"PWV5", &bands).unwrap().unwrap();
        assert_eq!(anlz::waveform_entries(ext, b"PWV5").unwrap(), Some(detail));
        assert_eq!(anlz::preview(dat).unwrap().map(|p| p.columns.len()), Some(400));
        let staged = root.join(paths::STATE_DIR).join("waveforms").join(format!("{}.bands", Hash::of(&bytes)));
        assert_eq!(std::fs::read(staged).unwrap(), bytes);
    }
    backup::restore(&mut d, None, false).unwrap();
    assert_eq!(files(&root, &t), before);
}

/// The tempo a grid gives the databases is the one most of its beats have, the first of those
/// alike.
#[test]
fn a_grids_tempo_is_most_of_its_beats() {
    let tempo = |bpms: &[f64]| {
        let beats = bpms.iter().enumerate().map(|(i, &bpm)| Beat { beat_number: 1, bpm, time_ms: i as u32 }).collect();
        NewAnalysis { beats: Some(beats), ..Default::default() }.tempo()
    };
    assert_eq!(tempo(&[120.0, 128.0, 128.0]), Some(128.0));
    assert_eq!(tempo(&[120.0, 128.0, 128.0, 120.0]), Some(120.0));
    assert_eq!(tempo(&[127.996]), Some(128.0));
    assert_eq!(NewAnalysis::default().tempo(), None);
    let a = NewAnalysis { beats: Some(vec![]), phrases: None, waveform: Some(WaveSource::Staged(Hash::of(b"x"))) };
    assert_eq!(a.describe("/Contents/a.mp3"), "set the beat grid and the waveform of /Contents/a.mp3");
}

/// Each way an analysis edit can't be made is refused as it's planned, saying why, and leaves
/// the drive as it was, no waveform staged: nothing to write, a grid or phrases the files can't
/// hold, a beat past the end, phrases without an `.EXT`, no `.DAT`, no analysis files at all or
/// ones outside rekordbox's folder, a waveform over rekordbox's colour overview, one that isn't
/// bands, given as a plugin's blob or staged and gone, and a track that isn't on the drive.
#[hegel::test(test_cases = 40, suppress_health_check = [HealthCheck::TooSlow])]
fn analysis_edits_that_cant_be_made_change_nothing(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let case = tc.draw(gs::integers::<u8>().max_value(14));
    let root = match case {
        13 => common::bigger_drive(dir.path(), "USB"),
        _ => common::demo_drive(dir.path(), "USB"),
    };
    let t = track(&root, "Demo Track 2");
    let name = t.display_name();
    let dat = paths::to_fs(&root, &paths::key(&t.analyze_path));
    let beat = |n: u16, bpm: f64, ms: u32| Beat { beat_number: n, bpm, time_ms: ms };
    let grid = |beats: Vec<Beat>| NewAnalysis { beats: Some(beats), ..Default::default() };
    let phrased = |mood, phrases: Vec<(PhraseKind, u16, u16)>| NewAnalysis {
        phrases: Some(SongStructure {
            mood,
            phrases: phrases
                .into_iter()
                .map(|(kind, start_beat, end_beat)| Phrase { kind, start_beat, end_beat, fill_beat: None })
                .collect(),
        }),
        ..Default::default()
    };
    let waved = |w: WaveSource| NewAnalysis { waveform: Some(w), ..Default::default() };
    let intro = vec![(PhraseKind::Intro, 1, 9)];
    let (edit, why) = match case {
        0 => (set(&t, NewAnalysis::default()), "say what to write: a beat grid, phrases or a waveform".to_string()),
        1 => {
            let n = tc.draw(hegel::one_of!(gs::just(0u16), gs::integers::<u16>().min_value(5)));
            (set(&t, grid(vec![beat(n, 120.0, 0)])), format!("beat 1 is beat {n} of its bar: 1-4"))
        }
        2 => {
            let bpm = tc.draw(hegel::one_of!(
                gs::just(f64::NAN),
                gs::floats::<f64>().max_value(0.004),
                gs::floats::<f64>().min_value(655.36)
            ));
            (set(&t, grid(vec![beat(1, bpm, 0)])), format!("beat 1's tempo, {bpm} BPM, isn't 0.01-655.35"))
        }
        3 => {
            let ms = tc.draw(gs::integers::<u32>().max_value(10_000));
            let back = tc.draw(gs::integers::<u32>().max_value(ms));
            (
                set(&t, grid(vec![beat(1, 120.0, ms), beat(2, 120.0, back)])),
                "beat 2 isn't after the one before it".into(),
            )
        }
        4 => {
            let past = (t.duration_secs + 1) * 1000 + tc.draw(gs::integers::<u32>().max_value(1_000_000));
            (set(&t, grid(steady(1, 120.0, past))), format!("is past the end of {name}"))
        }
        5 => {
            let gap = tc.draw(gs::integers::<u16>().min_value(10).max_value(100));
            let gapped = phrased(Mood::Mid, vec![(PhraseKind::Intro, 1, 9), (PhraseKind::Outro, gap, gap + 8)]);
            (set(&t, gapped), "phrases follow one another from beat 1".into())
        }
        6 => (
            set(&t, phrased(Mood::High, vec![(PhraseKind::Bridge, 1, 9)])),
            "a High track has no Bridge phrases".into(),
        ),
        7 => {
            std::fs::remove_file(dat.with_extension("EXT")).unwrap();
            (set(&t, phrased(Mood::Low, intro)), format!("{name} has no .EXT analysis file to hold phrases"))
        }
        8 => {
            std::fs::remove_file(&dat).unwrap();
            (set(&t, grid(steady(4, 120.0, 0))), format!("{name} has no .DAT analysis file"))
        }
        9 => (set(&t, waved(wave(&bands(150)))), "(PWV4) that can't be worked out from another waveform".into()),
        10 => {
            let mut bytes = tc.draw(gs::binary().max_size(40));
            if !bytes.is_empty() && bytes.len().is_multiple_of(3) {
                bytes.pop();
            }
            let why = "a waveform is three bytes a column (lows, mids, highs), at least one column";
            match tc.draw(gs::booleans()) {
                true => (set(&t, waved(WaveSource::Base64(BASE64_STANDARD.encode(bytes)))), why.into()),
                false => (set(&t, waved(WaveSource::Base64("not base64!".into()))), "the waveform isn't base64".into()),
            }
        }
        11 => (
            set(&t, waved(WaveSource::Blob("w.bands".into()))),
            "only a plugin's plan gives a waveform as a blob".into(),
        ),
        12 => (set(&t, waved(WaveSource::Staged(Hash::of(b"gone")))), "the staged waveform".into()),
        13 => (set(&t, grid(steady(4, 120.0, 0))), format!("refusing analysis path {:?} of {name}", t.analyze_path)),
        _ => {
            let audio = "/Contents/none/a.mp3";
            common::add_track(
                &root,
                rekordbox_pdb::NewTrack::new("Unanalysed", audio),
                &common::placeholder_audio(audio),
            );
            let t = track(&root, "Unanalysed");
            let why = format!("{} has no analysis files (rekordbox hasn't analysed it)", t.display_name());
            (set(&t, grid(steady(4, 120.0, 0))), why)
        }
    };
    let before = common::drive_files(&root);
    let err = plan(&root, &[edit]).unwrap_err().to_string();
    assert!(err.contains(&why), "{err}");
    assert!(common::changed_files(&before, &common::drive_files(&root)).is_empty());
    assert!(!root.join(paths::STATE_DIR).join("waveforms").exists());
}

/// For any grid moved or kept, phrases or not, a waveform or not (on a track with or without an
/// `.EXT`), and cues on the old grid's beats: the plan warns of the cues left off the new grid
/// and of a waveform with no `.EXT` to hold its detail, and applied, a replay makes it again as
/// it was made: nothing more when the drive has it, the same files after rekordbox exports again
/// (the waveform from its staged copy), skipped with a warning once the track's `.DAT` is gone;
/// and a grid set over a moved one has no extended grid left to lose.
#[hegel::test(test_cases = 24, suppress_health_check = [HealthCheck::TooSlow])]
fn any_analysis_edit_warns_of_what_it_leaves_and_replays_as_made(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let t = track(&root, "Demo Track 1");
    let name = t.display_name();
    let dat = paths::to_fs(&root, &paths::key(&t.analyze_path));
    let waved = tc.draw(gs::booleans());
    let no_ext = waved && tc.draw(gs::booleans());
    if waved {
        strip_colour_overview(&root, &t);
    }
    if no_ext {
        std::fs::remove_file(dat.with_extension("EXT")).unwrap();
    }
    let grid = anlz::beat_grid(&std::fs::read(&dat).unwrap()).unwrap();
    let on_beats = tc.draw(gs::integers::<usize>().max_value(3));
    let mut cues: Vec<CuePoint> = (0..on_beats)
        .map(|i| CuePoint { hot_cue: Some(i as u8), start_ms: grid[4 * (i + 1)].time_ms, ..Default::default() })
        .collect();
    cues.push(CuePoint { start_ms: grid[2].time_ms + 100, ..Default::default() });
    let [d, e, _] = files(&root, &t);
    std::fs::write(&dat, anlz::write_cue_points(&d.unwrap(), CueFile::Dat, &cues).unwrap()).unwrap();
    if let Some(e) = e {
        std::fs::write(dat.with_extension("EXT"), anlz::write_cue_points(&e, CueFile::Ext, &cues).unwrap()).unwrap();
    }
    let exported = common::library_files(&root);

    let phrases = (!no_ext && tc.draw(gs::booleans())).then(|| SongStructure {
        mood: Mood::Mid,
        phrases: vec![Phrase { kind: PhraseKind::Intro, start_beat: 1, end_beat: 9, fill_beat: None }],
    });
    let mut shift = tc.draw(gs::sampled_from(vec![0u32, 20, 40]));
    if shift == 0 && phrases.is_none() && !waved {
        shift = 20;
    }
    let moved: Vec<Beat> = grid.iter().map(|b| Beat { time_ms: b.time_ms + shift, ..*b }).collect();
    let b = bands(tc.draw(gs::integers::<usize>().min_value(1).max_value(600)));
    let analysis = NewAnalysis { beats: Some(moved.clone()), phrases, waveform: waved.then(|| wave(&b)) };
    let p = plan(&root, &[set(&t, analysis)]).unwrap();
    let off = format!("cue(s) of {name} sat on beats of its old grid: they stay where they are, off the new one");
    let moved_cues = if shift > 0 { on_beats } else { 0 };
    let said = p.warnings.iter().find(|w| w.ends_with(&off));
    assert_eq!(said, (moved_cues > 0).then(|| format!("{moved_cues} {off}")).as_ref(), "{:?}", p.warnings);
    let overview_only = format!("{name} has no .EXT analysis file, so only its overview waveform is written");
    assert_eq!(p.warnings.contains(&overview_only), no_ext, "{:?}", p.warnings);
    edit::apply(&p, &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap();
    let edited = files(&root, &t);
    assert_eq!(anlz::beat_grid(edited[0].as_ref().unwrap()).unwrap(), moved);

    let replay = |root: &Path| {
        let mut d = Device::open_export(root).unwrap();
        let edits = edit::journal::edits(&d).unwrap();
        let opts = EditOptions { lenient: true, no_journal: true };
        let plan = edit::plan(&d, &edits, &opts, &Ctx::silent()).unwrap();
        edit::apply(&plan, &mut d, &Ctx::silent()).unwrap()
    };
    match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => {
            let report = replay(&root);
            assert_eq!(files(&root, &t), edited, "as it is already");
            assert!(report.warnings.iter().all(|w| !w.contains("skipped")), "{:?}", report.warnings);
        }
        1 => {
            for (key, bytes) in &exported {
                std::fs::write(paths::to_fs(&root, key), bytes).unwrap();
            }
            replay(&root);
            assert_eq!(files(&root, &t), edited);
        }
        2 => {
            std::fs::remove_file(&dat).unwrap();
            let report = replay(&root);
            let skipped = "skipped an edit that no longer applies (set the beat grid";
            assert!(report.warnings.iter().any(|w| w.starts_with(skipped)), "{:?}", report.warnings);
            assert_eq!(files(&root, &t)[1..], edited[1..]);
        }
        _ => {
            let again: Vec<Beat> = moved.iter().map(|b| Beat { time_ms: b.time_ms + 7, ..*b }).collect();
            let p = plan(&root, &[set(&t, NewAnalysis { beats: Some(again.clone()), ..Default::default() })]).unwrap();
            // Its extended grid went with the first grid that moved.
            let loses = p.warnings.iter().any(|w| w.contains("loses its extended beat grid (PQT2)"));
            assert_eq!(loses, shift == 0 && !no_ext, "{:?}", p.warnings);
            edit::apply(&p, &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap();
            assert_eq!(anlz::beat_grid(files(&root, &t)[0].as_ref().unwrap()).unwrap(), again);
        }
    }
}
