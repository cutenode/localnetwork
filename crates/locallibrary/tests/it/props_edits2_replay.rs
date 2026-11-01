//! Replaying recorded cue edits and track pointings: each is made again where it still applies,
//! and skipped, said so, where it doesn't, as making the same edits one by one would.

use crate::common;

use std::fs;
use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::cue_edit::{self, CueChange, CueEdit, CueInfo, CuePosition, CueRef, HotCue, SetCue, UpdateCue};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions, EditReport};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::{Library, paths, pdb};

/// Give the drive at `root` a journal of `edits`, as if they had been made there.
fn record(root: &Path, edits: &[Edit]) {
    let file = Device::open(root).unwrap().state_dir().join("edits.json");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    let journal = serde_json::json!({
        "version": edit::journal::VERSION,
        "entries": [{ "at": "2026-10-01T12:00:00Z", "edits": edits, "changes": [] }],
    });
    fs::write(file, serde_json::to_vec(&journal).unwrap()).unwrap();
}

/// Replay the journal of the drive at `root`, as `edits replay` does.
fn replay(root: &Path) -> EditReport {
    let mut d = Device::open_export(root).unwrap();
    let edits = edit::journal::edits(&d).unwrap();
    let opts = EditOptions { lenient: true, no_journal: true };
    let plan = edit::plan(&d, &edits, &opts, &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap()
}

fn cues(root: &Path, lib: &Library) -> Vec<Vec<CueInfo>> {
    let ids: Vec<u32> = lib.tracks.iter().map(|t| t.id).collect();
    cue_edit::list(&Device::open_export(root).unwrap(), &ids).unwrap().into_iter().map(|c| c.cues).collect()
}

/// A recorded cue edit on drawn tracks (some gone from the drive), from a drawn source.
#[hegel::test_helper]
fn recorded_cue_edit(tc: &TestCase, files: &[String]) -> Edit {
    let tracks: Vec<String> = tc.draw(gs::subsequences(files).min_size(1));
    let hot = HotCue(tc.draw(gs::integers::<u8>().max_value(3)));
    let from = || tc.draw(gs::sampled_from(files.to_vec()));
    let set = |hot_cue| SetCue {
        hot_cue,
        at: CuePosition::Ms(tc.draw(gs::integers::<u32>().min_value(1_000).max_value(60_000))),
        snap: false,
        loop_beats: None,
        name: None,
        color: None,
    };
    let change = match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => CueChange::Set(set(Some(hot))),
        5 => {
            let slots = || tc.draw(gs::optional(gs::integers::<u8>().max_value(3))).map(HotCue);
            CueChange::SetMany(
                (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3))).map(|_| set(slots())).collect(),
            )
        }
        1 => CueChange::Update(UpdateCue { cue: CueRef::Hot(hot), name: Some("Again".into()), color: None }),
        2 => CueChange::Delete(if tc.draw(gs::booleans()) { CueRef::Hot(hot) } else { CueRef::All }),
        3 => CueChange::Copy { from: from() },
        _ => CueChange::Carry { from: from(), shift_ms: tc.draw(gs::integers::<i32>().min_value(-500).max_value(500)) },
    };
    Edit::Cues { tracks, change }
}

/// Replaying a journal of cue edits (and a playlist made after them, on tracks with or without an
/// `.EXT`) leaves the drive's cues and
/// playlists as making each edit in turn would, skipping those that no longer apply (a track or
/// source gone, a cue that isn't there, one track's edit on several) with a warning. A replay
/// that makes nothing says it made no changes.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn replayed_cue_edits_redo_what_still_applies_and_skip_the_rest(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let replayed = common::demo_drive(dir.path(), "Replayed");
    let model = common::demo_drive(dir.path(), "Model");
    let lib = common::library(&replayed);
    let gone = "/Contents/Gone/gone.mp3".to_string();
    let files: Vec<String> = lib.tracks.iter().map(|t| t.file_path.clone()).chain([gone.clone()]).collect();
    let mut edits: Vec<Edit> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)))
        .map(|_| recorded_cue_edit(&tc, &files))
        .collect();
    // A playlist made after them: none, a new one, or one the drive has (which changes nothing).
    let playlist = tc.draw(gs::integers::<u8>().max_value(2));
    let made = Edit::CreatePlaylist { path: "Replayed".into(), folder: false };
    if playlist > 0 {
        edits.push(made.clone());
    }
    // A track with no `.EXT`, at times.
    let no_ext = tc.draw(gs::booleans());
    for root in [&replayed, &model] {
        if playlist == 2 {
            common::apply(root, std::slice::from_ref(&made));
        }
        if no_ext {
            let t = &lib.tracks[1];
            fs::remove_file(paths::to_fs(root, &paths::key(&t.analyze_path).replace(".DAT", ".EXT"))).unwrap();
        }
    }
    record(&replayed, &edits);

    // The model: each edit made in turn, those that can't skipped.
    let id = |f: &str| lib.tracks.iter().find(|t| t.file_path == f).map(|t| t.id);
    let (mut skipped, mut changed) = (false, false);
    for e in &edits {
        let Edit::Cues { tracks, change } = e else { continue };
        let ids: Vec<u32> = tracks.iter().filter_map(|f| id(f)).collect();
        let edit = match change {
            CueChange::Set(s) => Some(CueEdit::Set(s.clone())),
            CueChange::SetMany(cues) => Some(CueEdit::SetMany(cues.clone())),
            CueChange::Update(u) => Some(CueEdit::Update(u.clone())),
            CueChange::Delete(r) => Some(CueEdit::Delete(r.clone())),
            CueChange::Copy { from } => id(from).map(|from| CueEdit::Copy { from }),
            CueChange::Carry { from, shift_ms } => id(from).map(|from| CueEdit::Carry { from, shift_ms: *shift_ms }),
        };
        let mut d = Device::open_export(&model).unwrap();
        match edit.map(|edit| cue_edit::plan(&d, &ids, &edit, &Ctx::silent())) {
            Some(Ok(p)) => {
                changed |= !p.is_noop();
                cue_edit::apply(&p, &mut d, &Ctx::silent()).unwrap();
            }
            _ => skipped = true,
        }
    }
    if playlist == 1 {
        common::apply(&model, std::slice::from_ref(&made));
    }

    let report = replay(&replayed);
    tc.note(&format!("{edits:?}: {report:?}"));
    assert_eq!(cues(&replayed, &lib), cues(&model, &lib));
    let playlists = |root: &Path| pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap().playlists;
    assert_eq!(playlists(&replayed), playlists(&model));
    let warned = report.warnings.iter().any(|w| w.starts_with("skipped an edit that no longer applies"));
    assert_eq!(warned, skipped, "{:?}", report.warnings);
    let named_gone = edits.iter().any(|e| matches!(e, Edit::Cues { tracks, .. } if tracks.contains(&gone)));
    if named_gone {
        assert!(report.warnings.iter().any(|w| w.contains("aren't on the drive any more")), "{:?}", report.warnings);
    }
    if !changed && playlist != 1 {
        assert_eq!(report.changes, 0, "{report:?}");
    }
}

/// A recorded pointing of a track at another file is made again after rekordbox exports the old
/// file again; skipped, said so, when the export no longer has the old file or the new one is
/// gone; and changes nothing when the track points there already.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(scenario = 0u8)]
#[hegel::explicit_test_case(scenario = 1u8)]
#[hegel::explicit_test_case(scenario = 2u8)]
#[hegel::explicit_test_case(scenario = 3u8)]
fn a_recorded_pointing_is_replayed_where_it_still_applies(tc: TestCase) {
    let scenario = tc.draw(gs::integers::<u8>().max_value(3));
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let lib = common::library(&root);
    let t = lib.tracks[0].clone();
    let pristine = common::library_files(&root);
    let new_path = format!("/Contents/Moved/{}", t.file_path.rsplit('/').next().unwrap());
    let new_file = paths::to_fs(&root, &paths::key(&new_path));
    fs::create_dir_all(new_file.parent().unwrap()).unwrap();
    fs::copy(paths::to_fs(&root, &paths::key(&t.file_path)), &new_file).unwrap();
    let pointing = Edit::SetPath {
        track: TrackSelector::paths([t.file_path.clone()]),
        path: new_path.clone(),
        file: None,
        shift_ms: 0,
    };
    common::apply(&root, std::slice::from_ref(&pointing));
    let pointed = common::library(&root);
    // What the drive has when the journal is replayed.
    match scenario {
        // rekordbox exported the old file again.
        0 | 2 => {
            for (key, bytes) in &pristine {
                fs::write(paths::to_fs(&root, key), bytes).unwrap();
            }
            if scenario == 2 {
                fs::remove_file(&new_file).unwrap();
            }
        }
        // Nothing since.
        1 => {}
        // A pointing of the track at where it is.
        _ => record(
            &root,
            &[Edit::SetPath {
                track: TrackSelector::paths([new_path.clone()]),
                path: new_path.clone(),
                file: None,
                shift_ms: 0,
            }],
        ),
    }
    let before = common::library(&root);
    let report = replay(&root);
    let after = common::library(&root);
    let skipped = report.warnings.iter().any(|w| w.starts_with("a track's new file:") && w.ends_with("; skipped"));
    assert_eq!(skipped, matches!(scenario, 1 | 2), "{:?}", report.warnings);
    match scenario {
        0 => assert_eq!(after.tracks, pointed.tracks),
        _ => assert_eq!(after.tracks, before.tracks),
    }
}

/// A replay whose writes fail (as a crash or a full disk would, just before a swap) stops with
/// the edits it made counted and undo offered, whether it was making a recorded cue edit, a
/// pointing, or the playlist edits after them, and leaves the drive's library as it was. An edit
/// made outside a replay fails the same way.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(step = 0u8, playlist = false)]
#[hegel::explicit_test_case(step = 1u8, playlist = false)]
#[hegel::explicit_test_case(step = 2u8, playlist = true)]
fn a_replay_that_fails_stops_saying_how_far_it_got(tc: TestCase) {
    let step = tc.draw(gs::integers::<u8>().max_value(2));
    let playlist = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let lib = common::library(&root);
    let t = lib.tracks[0].clone();
    let mut edits = vec![match step {
        // A cue edit that applies.
        0 => Edit::Cues {
            tracks: vec![t.file_path.clone()],
            change: CueChange::Set(SetCue {
                hot_cue: Some(HotCue(7)),
                at: CuePosition::Ms(1_234),
                snap: false,
                loop_beats: None,
                name: None,
                color: None,
            }),
        },
        // A pointing of a track at a copy of its file.
        1 => {
            let new_path = "/Contents/Moved/copy.mp3".to_string();
            let p = paths::to_fs(&root, &paths::key(&new_path));
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::copy(paths::to_fs(&root, &paths::key(&t.file_path)), &p).unwrap();
            Edit::SetPath {
                track: TrackSelector::paths([t.file_path.clone()]),
                path: new_path,
                file: None,
                shift_ms: 0,
            }
        }
        // A cue edit that no longer applies, so only the playlist edit writes.
        _ => Edit::Cues { tracks: vec!["/Contents/Gone/gone.mp3".into()], change: CueChange::Delete(CueRef::All) },
    }];
    if playlist || step == 2 {
        edits.push(Edit::CreatePlaylist { path: "Replayed".into(), folder: false });
    }
    record(&root, &edits);
    let before = common::library_files(&root);
    let mut d = Device::open_export(&root).unwrap();
    let opts = EditOptions { lenient: true, no_journal: true };
    let plan = edit::plan(&d, &edit::journal::edits(&d).unwrap(), &opts, &Ctx::silent()).unwrap();
    locallibrary::testing::fail_before_swap(true);
    let replayed = edit::apply(&plan, &mut d, &Ctx::silent());
    let outside = Edit::CreatePlaylist { path: "Outside".into(), folder: false };
    let made = edit::plan(&d, &[outside], &EditOptions::default(), &Ctx::silent())
        .and_then(|plan| edit::apply(&plan, &mut d, &Ctx::silent()));
    locallibrary::testing::fail_before_swap(false);
    let err = replayed.unwrap_err();
    let done = if step == 2 { 1 } else { 0 };
    assert!(err.to_string().contains(&format!("the replay stopped after {done} of its edits")), "{err}");
    assert!(made.is_err());
    assert_eq!(common::library_files(&root), before);
}
