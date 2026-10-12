//! Cue edits the cue property leaves out: the ones refused (a track rekordbox hasn't analysed, a
//! hot cue past H, a time past the end, beats or loops without a grid or tempo, a cue that isn't
//! there), cues carried onto copies of a recording that have none, and what an edit says it can't
//! store (names without an .EXT, cues for a track OneLibrary doesn't list).

use crate::common;

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::cue_edit::{self, CueEdit, CuePosition, CueRef, HotCue, SetCue, UpdateCue};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::{paths, pdb};
use rekordbox_pdb::NewTrack;

fn set(hot: Option<u8>, at: CuePosition, loop_beats: Option<f64>) -> SetCue {
    SetCue { hot_cue: hot.map(HotCue), at, snap: false, loop_beats, name: None, color: None }
}

/// Edits refused for what they ask of a track, each on a track it doesn't fit: the error says why
/// and the drive is as it was.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn cue_edits_a_track_cant_take_are_refused(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    // A track rekordbox hasn't analysed, and one analysed without a beat grid or tempo.
    let bare = common::add_track(&root, NewTrack::new("Unanalysed", "/Contents/Bare.mp3"), b"ID3");
    let file = "/Contents/Flat.mp3";
    let flat = common::add_track(
        &root,
        NewTrack {
            analyze_path: "/PIONEER/USBANLZ/P0EE/00000001/ANLZ0000.DAT".into(),
            duration: 60,
            ..NewTrack::new("Flat", file)
        },
        b"ID3",
    );
    let before = common::Before::take(&root);
    let d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    let demo = lib.tracks[0].clone();
    let past = tc.draw(gs::integers::<u32>().min_value(1).max_value(600));
    let cases: Vec<(Vec<u32>, CueEdit, &str)> = vec![
        (vec![bare], CueEdit::Set(set(None, CuePosition::Ms(1000), None)), "has no analysis files"),
        (
            vec![demo.id],
            CueEdit::Set(set(
                Some(HotCue::COUNT + tc.draw(gs::integers::<u8>().max_value(10))),
                CuePosition::Ms(1000),
                None,
            )),
            "hot cues are A-H",
        ),
        (
            vec![demo.id],
            CueEdit::Set(set(None, CuePosition::Ms((demo.duration_secs + past) * 1000), None)),
            "past the end",
        ),
        (
            vec![flat],
            CueEdit::Set(set(None, CuePosition::Beat(tc.draw(gs::integers::<u32>().min_value(1).max_value(64))), None)),
            "beat grid",
        ),
        (vec![flat], CueEdit::Set(set(Some(0), CuePosition::Ms(1000), Some(4.0))), "a loop can't be sized in beats"),
        (
            vec![demo.id],
            CueEdit::Set(set(
                Some(0),
                CuePosition::Ms(1000),
                Some(tc.draw(gs::sampled_from(vec![0.3, 0.75, 513.0, 1.0 / 64.0, -1.0, 0.0]))),
            )),
            "loop",
        ),
        (
            vec![demo.id],
            CueEdit::Update(UpdateCue { cue: CueRef::Hot(HotCue(7)), name: Some("x".into()), color: None }),
            "hot cue H",
        ),
        (vec![], CueEdit::Delete(CueRef::All), "no tracks selected"),
    ];
    for (ids, edit, why) in &cases {
        let err = cue_edit::plan(&d, ids, edit, &Ctx::silent()).unwrap_err();
        assert!(err.to_string().to_lowercase().contains(&why.to_lowercase()), "{edit:?}: {err}");
    }
    let blank = Device::open(common::blank(dir.path(), "Blank")).unwrap();
    assert!(matches!(
        cue_edit::plan(&blank, &[1], &CueEdit::Delete(CueRef::All), &Ctx::silent()),
        Err(locallibrary::Error::NotAnExport(_))
    ));
    before.untouched(&root).unwrap();
}

/// Carried from one copy of a recording to another, the cues land `shift_ms` later on a track that
/// has none, and a track with cues of its own keeps them. A track with no .EXT gets its cues
/// without names (said), and one OneLibrary doesn't list gets them in export.pdb's analysis only
/// (said).
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn cues_are_carried_onto_copies_that_have_none(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let mut d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    let (from, to) = (lib.tracks[0].id, lib.tracks[1].id);
    let cues = |d: &Device, id: u32| -> Vec<(Option<String>, u32)> {
        let mut c: Vec<_> =
            cue_edit::list(d, &[id]).unwrap()[0].cues.iter().map(|c| (c.hot_cue.clone(), c.start_ms)).collect();
        c.sort();
        c
    };
    // The source's cues; the target's, at times, of its own.
    let start = cue_edit::plan(&d, &[from, to], &CueEdit::Delete(CueRef::All), &Ctx::silent());
    if let Ok(p) = start {
        cue_edit::apply(&p, &mut d, &Ctx::silent()).unwrap();
    }
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        let s = set(
            tc.draw(gs::optional(gs::integers::<u8>().max_value(7))),
            CuePosition::Ms(tc.draw(gs::integers::<u32>().min_value(20_000).max_value(60_000))),
            None,
        );
        let p = cue_edit::plan(&d, &[from], &CueEdit::Set(s), &Ctx::silent()).unwrap();
        cue_edit::apply(&p, &mut d, &Ctx::silent()).unwrap();
    }
    let own = tc.draw(gs::booleans());
    if own {
        let p = cue_edit::plan(&d, &[to], &CueEdit::Set(set(Some(0), CuePosition::Ms(5_000), None)), &Ctx::silent())
            .unwrap();
        cue_edit::apply(&p, &mut d, &Ctx::silent()).unwrap();
    }
    let (source, target) = (cues(&d, from), cues(&d, to));
    let shift_ms = tc.draw(gs::integers::<i32>().min_value(-10_000).max_value(10_000));
    let carry = CueEdit::Carry { from, shift_ms };
    let plan = cue_edit::plan(&d, &[to], &carry, &Ctx::silent()).unwrap();
    cue_edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let after = cues(&d, to);
    match own {
        true => assert_eq!(after, target, "a track with cues keeps its own"),
        false => {
            let mut want: Vec<_> =
                source.iter().map(|(h, ms)| (h.clone(), ms.saturating_add_signed(shift_ms))).collect();
            want.sort();
            assert_eq!(after, want, "carried {shift_ms:+} ms");
        }
    }

    // A track without its .EXT, and one exported to export.pdb alone after OneLibrary.
    let no_ext = &lib.tracks[1];
    fs::remove_file(paths::to_fs(&root, &paths::key(&no_ext.analyze_path.replace(".DAT", ".EXT")))).unwrap();
    let named = SetCue { name: Some("Drop".into()), ..set(Some(2), CuePosition::Ms(8_000), None) };
    let p = cue_edit::plan(
        &Device::open_export(&root).unwrap(),
        &[no_ext.id],
        &CueEdit::Set(named.clone()),
        &Ctx::silent(),
    )
    .unwrap();
    assert!(p.warnings.iter().any(|w| w.contains("has no .EXT analysis file")), "{:?}", p.warnings);
    let late = common::export_new_track(&root, "Late", "Export");
    let p =
        cue_edit::plan(&Device::open_export(&root).unwrap(), &[late], &CueEdit::Set(named), &Ctx::silent()).unwrap();
    assert!(p.warnings.iter().any(|w| w.contains("isn't in exportLibrary.db")), "{:?}", p.warnings);
    assert!(pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap().track(late).is_some());
}
