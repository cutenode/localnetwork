//! Cue edits and listings at the edges the other cue properties leave: tracks OneLibrary doesn't
//! list, active loops carried through an edit into the files and OneLibrary's rows, and cues
//! copied onto another track of the same audio file.

use crate::common;

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::anlz::{self, CueFile, CuePoint};
use locallibrary::cue_edit::{self, CueEdit, CuePosition, CueRef, HotCue, SetCue, UpdateCue};
use locallibrary::device::Device;
use locallibrary::onelibrary::OneLibrary;
use locallibrary::paths;
use locallibrary::progress::Ctx;
use rekordbox_pdb::NewTrack;

/// OneLibrary's id for the track of the audio file `file`.
fn content_id(root: &std::path::Path, file: &str) -> u32 {
    common::plus_library(root).tracks.iter().find(|t| t.file_path == file).unwrap().id
}

/// Listing cues gives OneLibrary's count of rows for the tracks it lists, and none for tracks
/// exported since that it doesn't.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_listed_cues_count_onelibrary_rows_only_for_the_tracks_it_lists(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let listed: Vec<String> = common::plus_library(&root).tracks.iter().map(|t| t.file_path.clone()).collect();
    for i in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        common::export_new_track(&root, "Newcomer", &format!("Late {i}"));
    }
    let d = Device::open_export(&root).unwrap();
    let ids: Vec<u32> = d.library().unwrap().tracks.iter().map(|t| t.id).collect();
    for c in cue_edit::list(&d, &ids).unwrap() {
        let known = listed.contains(&c.track.file_path);
        assert_eq!(c.onelibrary_cues, known.then_some(0), "{}", c.track.file_path);
    }
}

/// An active loop stays active through an edit of the track's cues, in its analysis files and
/// in the row OneLibrary gets for it.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_an_active_loop_stays_active_through_a_cue_edit(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let d = Device::open_export(&root).unwrap();
    let t = d.library().unwrap().tracks[0].clone();
    let start_ms = tc.draw(gs::integers::<u32>().min_value(1_000).max_value((t.duration_secs - 20) * 1000));
    let looped = CuePoint {
        hot_cue: Some(0),
        start_ms,
        end_ms: Some(start_ms + tc.draw(gs::integers::<u32>().min_value(500).max_value(8_000))),
        active_loop: true,
        ..Default::default()
    };
    let dat = paths::to_fs(&root, &paths::key(&t.analyze_path));
    for (path, file) in [(dat.clone(), CueFile::Dat), (dat.with_extension("EXT"), CueFile::Ext)] {
        let bytes = anlz::write_cue_points(&fs::read(&path).unwrap(), file, std::slice::from_ref(&looped)).unwrap();
        fs::write(&path, bytes).unwrap();
    }
    let edit = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => CueEdit::Update(UpdateCue { cue: CueRef::Hot(HotCue(0)), name: Some("Roll".into()), color: None }),
        1 => CueEdit::Update(UpdateCue { cue: CueRef::Hot(HotCue(0)), name: None, color: Some("red".into()) }),
        _ => CueEdit::Set(SetCue {
            hot_cue: Some(HotCue(1)),
            at: CuePosition::Ms(start_ms - 500),
            snap: false,
            loop_beats: None,
            name: None,
            color: None,
        }),
    };
    tc.note(&format!("{edit:?}"));
    let mut d = Device::open_export(&root).unwrap();
    let plan = cue_edit::plan(&d, &[t.id], &edit, &Ctx::silent()).unwrap();
    assert!(plan.tracks.iter().all(|c| c.onelibrary), "{:?}", plan.warnings);
    cue_edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();

    let listed = cue_edit::list(&Device::open_export(&root).unwrap(), &[t.id]).unwrap();
    let a = listed[0].cues.iter().find(|c| c.hot_cue.as_deref() == Some("A")).unwrap();
    assert!(a.active_loop, "{:?}", listed[0].cues);
    let rows = OneLibrary::open_root(&root).unwrap().cue_rows(content_id(&root, &t.file_path)).unwrap();
    let row = rows.iter().find(|r| r.hot_cue == Some(0)).unwrap();
    assert!(row.active_loop && row.out_usec.is_some(), "{rows:?}");
}

/// Cues copied onto another track of the same audio file take the source's OneLibrary rows as
/// they are, seek columns and all, since those are only right for that file.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_cues_copied_onto_a_track_of_the_same_audio_keep_its_onelibrary_rows(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let mut d = Device::open_export(&root).unwrap();
    let a = d.library().unwrap().tracks[0].clone();
    let n = tc.draw(gs::integers::<u8>().min_value(1).max_value(3));
    for h in 0..n {
        let at = CuePosition::Ms(tc.draw(gs::integers::<u32>().max_value((a.duration_secs - 10) * 1000)));
        let set = SetCue { hot_cue: Some(HotCue(h)), at, snap: false, loop_beats: None, name: None, color: None };
        let plan = cue_edit::plan(&d, &[a.id], &CueEdit::Set(set), &Ctx::silent()).unwrap();
        cue_edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    }
    let cid = content_id(&root, &a.file_path);
    let rows = |root: &std::path::Path| {
        let mut r: Vec<_> = OneLibrary::open_root(root)
            .unwrap()
            .cue_rows(cid)
            .unwrap()
            .into_iter()
            .map(|r| (r.hot_cue, r.in_usec, r.stored))
            .collect();
        r.sort_by_key(|(h, at, _)| (*h, *at));
        r
    };
    let before = rows(&root);
    assert_eq!(before.len(), usize::from(n));

    let audio = fs::read(paths::to_fs(&root, &paths::key(&a.file_path))).unwrap();
    let twin = NewTrack {
        analyze_path: "/PIONEER/USBANLZ/P0EE/00000002/ANLZ0000.DAT".into(),
        duration: a.duration_secs.try_into().unwrap(),
        ..NewTrack::new("Twin", a.file_path.clone())
    };
    let twin = common::add_track(&root, twin, &audio);
    let mut d = Device::open_export(&root).unwrap();
    let plan = cue_edit::plan(&d, &[twin], &CueEdit::Copy { from: a.id }, &Ctx::silent()).unwrap();
    assert_eq!(plan.tracks.len(), 1);
    assert!(plan.tracks[0].onelibrary, "{:?}", plan.warnings);
    cue_edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert_eq!(rows(&root), before);
}

/// A cue edit that can't be made says why and leaves the drive as it was: on a drive whose
/// export.pdb can't be read, copying from a track that isn't there, setting cues one of which is
/// past the end, when cancelled, or when OneLibrary is damaged after planning.
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_cue_edits_that_cant_be_made_change_nothing(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let t = Device::open_export(&root).unwrap().library().unwrap().tracks[0].clone();
    let set = |ms: u32| SetCue {
        hot_cue: None,
        at: CuePosition::Ms(ms),
        snap: false,
        loop_beats: None,
        name: None,
        color: None,
    };
    let fine = CueEdit::Set(set(1_000));
    let case = tc.draw(gs::integers::<u8>().max_value(4));
    if case == 0 {
        fs::write(root.join(paths::EXPORT_PDB), tc.draw(gs::binary().max_size(64))).unwrap();
    }
    let before = common::drive_files(&root);
    let mut d = Device::open_export(&root).unwrap();
    let silent = Ctx::silent();
    let err = match case {
        0 => {
            assert!(cue_edit::list(&d, &[t.id]).is_err());
            cue_edit::plan(&d, &[t.id], &fine, &silent).unwrap_err()
        }
        1 => {
            let from = tc.draw(gs::integers::<u32>().min_value(10_000));
            let err = cue_edit::plan(&d, &[t.id], &CueEdit::Copy { from }, &silent).unwrap_err();
            assert!(err.to_string().contains(&format!("#{from}")), "{err}");
            err
        }
        2 => {
            let past = (t.duration_secs + 1 + tc.draw(gs::integers::<u32>().max_value(600))) * 1000;
            let edit = CueEdit::SetMany(vec![set(1_000), set(past)]);
            let err = cue_edit::plan(&d, &[t.id], &edit, &silent).unwrap_err();
            assert!(err.to_string().contains("past the end"), "{err}");
            err
        }
        3 => {
            let token = locallibrary::CancelToken::new();
            token.cancel();
            let quiet = |_: &locallibrary::Event| {};
            cue_edit::plan(&d, &[t.id], &fine, &Ctx::new(&quiet, token)).unwrap_err()
        }
        _ => {
            let plan = cue_edit::plan(&d, &[t.id], &fine, &silent).unwrap();
            common::rot(&root.join(paths::LIBRARY_PLUS_DB));
            let before = common::drive_files(&root);
            tc.note(&format!("{}", cue_edit::apply(&plan, &mut d, &silent).unwrap_err()));
            assert!(common::changed_files(&before, &common::drive_files(&root)).is_empty());
            return;
        }
    };
    tc.note(&format!("{err}"));
    assert!(common::changed_files(&before, &common::drive_files(&root)).is_empty());
}
