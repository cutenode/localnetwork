//! Cue edits on drives whose analysis files or databases aren't as an edit needs them: listing
//! says why a track's cues can't be read and planning refuses for the same reason; edits a track
//! can't take are refused; copies replace the targets' cues and leave the source's; and a plan
//! applied after its drive changed is refused.

use crate::common;

use std::fs;
use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::cue_edit::{self, CueEdit, CueInfo, CuePosition, CueRef, HotCue, SetCue, UpdateCue};
use locallibrary::device::Device;
use locallibrary::progress::{CancelToken, Ctx, NoProgress};
use locallibrary::{Error, paths};
use rekordbox_pdb::NewTrack;

fn set(hot: Option<u8>, at: CuePosition) -> SetCue {
    SetCue { hot_cue: hot.map(HotCue), at, snap: false, loop_beats: None, name: None, color: None }
}

fn plan(root: &Path, ids: &[u32], edit: &CueEdit) -> locallibrary::Result<cue_edit::CuePlan> {
    cue_edit::plan(&Device::open_export(root).unwrap(), ids, edit, &Ctx::silent())
}

fn apply(root: &Path, ids: &[u32], edit: &CueEdit) {
    let p = plan(root, ids, edit).unwrap();
    cue_edit::apply(&p, &mut Device::open_export(root).unwrap(), &Ctx::silent()).unwrap();
}

fn cues(root: &Path, id: u32) -> Vec<CueInfo> {
    cue_edit::list(&Device::open_export(root).unwrap(), &[id]).unwrap().remove(0).cues
}

/// A track's analysis file (`ext`: its `.EXT`) on the drive at `root`.
fn analysis(root: &Path, id: u32, ext: bool) -> std::path::PathBuf {
    let t = Device::open_export(root).unwrap().library().unwrap().track(id).unwrap().clone();
    let dat = paths::key(&t.analyze_path);
    paths::to_fs(root, &if ext { dat.replace(".DAT", ".EXT") } else { dat })
}

/// A file turned into a folder with something in it.
fn folder_over(p: &Path) {
    fs::remove_file(p).unwrap();
    fs::create_dir_all(p.join("inside")).unwrap();
}

/// However a track's analysis files can't be read (the `.DAT` gone, a folder, not an analysis
/// file, its `.EXT` a folder, or the database naming a file outside the analysis folder), listing its cues says why instead of failing, planning an edit
/// of them refuses with that same reason, and the other tracks list as before. A track a drive's
/// OneLibrary doesn't list has no count of OneLibrary cues, and one the drive lacks isn't listed.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn unreadable_analysis_is_listed_with_why_and_refused_for_editing(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let onelibrary = tc.draw(gs::booleans());
    let at = tc.draw(gs::integers::<u32>().min_value(1_000).max_value(60_000));
    for mode in 0..5u8 {
        let root = common::demo_drive(dir.path(), &format!("Main{mode}"));
        if onelibrary {
            common::add_onelibrary(&root);
        }
        let new = common::export_new_track(&root, "Late", "Not In OneLibrary");
        let (broken, other) = (1, 2);
        let others_cues = cues(&root, other);
        match mode {
            0 => fs::remove_file(analysis(&root, broken, false)).unwrap(),
            1 => folder_over(&analysis(&root, broken, false)),
            2 => fs::write(analysis(&root, broken, false), b"PMAI but not really").unwrap(),
            3 => folder_over(&analysis(&root, broken, true)),
            // The database names a file that isn't where analysis files go.
            _ => {
                let pdb = root.join(paths::EXPORT_PDB);
                let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb).unwrap();
                ed.set_track_strings(broken, &[("analyze_path", "/PIONEER/rekordbox/ANLZ0000.DAT")]).unwrap();
                ed.save(&pdb).unwrap();
            }
        }
        let before = common::Before::take(&root);
        let d = Device::open_export(&root).unwrap();
        let listed = cue_edit::list(&d, &[broken, other, new]).unwrap();
        tc.note(&format!("mode {mode}: {:?}", listed[0].warnings));
        assert!(listed[0].cues.is_empty());
        let [why] = &listed[0].warnings[..] else { panic!("mode {mode}: {:?}", listed[0].warnings) };
        assert_eq!(listed[1].cues, others_cues);
        assert!(listed[1].warnings.is_empty(), "{:?}", listed[1].warnings);
        assert_eq!(listed[1].onelibrary_cues.is_some(), onelibrary);
        assert_eq!(listed[2].onelibrary_cues, None, "a track OneLibrary lacks");
        let err = cue_edit::plan(&d, &[broken], &CueEdit::Set(set(Some(0), CuePosition::Ms(at))), &Ctx::silent())
            .unwrap_err();
        assert_eq!(&err.to_string(), why, "mode {mode}");
        assert!(matches!(cue_edit::list(&d, &[999]), Err(Error::NotFound(_))));
        before.untouched(&root).unwrap();
    }
}

/// Edits a track can't take are refused, with why, and change nothing: a beat past its grid, a
/// time snapped or a memory cue found by beat on a track without one, a loop running past its
/// end, a colour that isn't one (set or updated), cues carried to before its start or copied
/// past its end, and an edit of one track's cues given several.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn cue_edits_past_a_tracks_grid_or_end_are_refused(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let flat = common::add_track(
        &root,
        NewTrack {
            analyze_path: "/PIONEER/USBANLZ/P0EE/00000001/ANLZ0000.DAT".into(),
            duration: 60,
            ..NewTrack::new("Flat", "/Contents/Flat.mp3")
        },
        b"ID3",
    );
    let d = Device::open_export(&root).unwrap();
    let demo = d.library().unwrap().track(1).unwrap().clone();
    let grid = locallibrary::anlz::beat_grid(&fs::read(analysis(&root, demo.id, false)).unwrap()).unwrap();
    // A hot cue to update, and a cue late in the demo track to copy onto the short one.
    apply(&root, &[demo.id], &CueEdit::Set(set(Some(1), CuePosition::Ms(2_000))));
    let late = (demo.duration_secs - tc.draw(gs::integers::<u32>().max_value(30))) * 1000;
    apply(&root, &[demo.id], &CueEdit::Set(set(None, CuePosition::Ms(late))));
    let before = common::Before::take(&root);
    let past = tc.draw(gs::integers::<u32>().min_value(1).max_value(100));
    let bad_colour = tc.draw(gs::sampled_from(vec!["plaid", "#12345", "63", "#gg0000"])).to_string();
    let early = tc.draw(gs::integers::<i32>().min_value(1).max_value(1_000_000));
    let cases: Vec<(Vec<u32>, CueEdit, &str)> = vec![
        (
            vec![demo.id],
            CueEdit::Set(set(Some(2), CuePosition::Beat(grid.len() as u32 + past))),
            "isn't on the beat grid",
        ),
        (vec![flat], CueEdit::Set(SetCue { snap: true, ..set(None, CuePosition::Ms(1_000)) }), "no beat grid"),
        (vec![flat], CueEdit::Delete(CueRef::Memory(CuePosition::Beat(past))), "no beat grid"),
        (
            vec![demo.id],
            CueEdit::Set(SetCue { loop_beats: Some(512.0), ..set(Some(3), CuePosition::Ms(late)) }),
            "past the end",
        ),
        (
            vec![demo.id],
            CueEdit::Set(SetCue { color: Some(bad_colour.clone()), ..set(Some(4), CuePosition::Ms(3_000)) }),
            "unknown colour",
        ),
        (
            vec![demo.id],
            CueEdit::Update(UpdateCue { cue: CueRef::Hot(HotCue(1)), name: None, color: Some(bad_colour.clone()) }),
            "unknown colour",
        ),
        (
            vec![flat],
            // Earlier than its every cue.
            CueEdit::Carry { from: demo.id, shift_ms: -((demo.duration_secs as i32 + 1) * 1000 + early) },
            "before the start",
        ),
        (vec![flat], CueEdit::Copy { from: demo.id }, "past the end"),
        (vec![demo.id, flat], CueEdit::Set(set(Some(0), CuePosition::Ms(1_000))), "pick one"),
        (vec![demo.id + 1_000], CueEdit::Delete(CueRef::All), "track #"),
    ];
    for (ids, edit, why) in &cases {
        let err = cue_edit::plan(&d, ids, edit, &Ctx::silent()).unwrap_err();
        assert!(err.to_string().contains(why), "{edit:?}: {err}");
    }
    before.untouched(&root).unwrap();
}

/// Copying a track's cues makes each target's cues the source's (a target with some of them
/// already keeps those as they are, recoloured or renamed alike), and leaves the source as it
/// was, even when it's among the targets. With an update after, the copy's cue takes the new
/// name and colour.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn copied_cues_are_the_sources_and_the_source_keeps_its_own(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    if tc.draw(gs::booleans()) {
        common::add_onelibrary(&root);
    }
    let (from, to) = (1, 2);
    // Both start with a hot cue at the same place, and the source has more.
    let shared = tc.draw(gs::integers::<u32>().min_value(1_000).max_value(20_000));
    let start = cue_edit::plan(
        &Device::open_export(&root).unwrap(),
        &[from, to],
        &CueEdit::Delete(CueRef::All),
        &Ctx::silent(),
    );
    if let Ok(p) = start {
        cue_edit::apply(&p, &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap();
    }
    for id in [from, to] {
        apply(&root, &[id], &CueEdit::Set(set(Some(0), CuePosition::Ms(shared))));
    }
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
        let at = tc.draw(gs::integers::<u32>().min_value(21_000).max_value(60_000));
        let hot = tc.draw(gs::optional(gs::integers::<u8>().min_value(1).max_value(7)));
        apply(&root, &[from], &CueEdit::Set(set(hot, CuePosition::Ms(at))));
    }
    let source = cues(&root, from);
    let targets = if tc.draw(gs::booleans()) { vec![from, to] } else { vec![to] };
    apply(&root, &targets, &CueEdit::Copy { from });
    assert_eq!(cues(&root, from), source, "the source");
    let strip = |l: &[CueInfo]| -> Vec<_> { l.iter().map(|c| (c.hot_cue.clone(), c.start_ms, c.end_ms)).collect() };
    assert_eq!(strip(&cues(&root, to)), strip(&source));
    // The copy's hot cue A, renamed and recoloured.
    let name = tc.draw(gs::sampled_from(vec!["Drop", "Intro"])).to_string();
    let color = tc.draw(gs::sampled_from(vec!["red", "#00e1fe", "12", "none"])).to_string();
    let update = UpdateCue { cue: CueRef::Hot(HotCue(0)), name: Some(name.clone()), color: Some(color) };
    apply(&root, &[to], &CueEdit::Update(update));
    let a = cues(&root, to).into_iter().find(|c| c.hot_cue.as_deref() == Some("A")).unwrap();
    assert_eq!((a.name.as_str(), a.start_ms), (name.as_str(), shared));
}

/// A cue plan applied after its drive changed (an analysis file gone, unreadable or with other
/// cues, OneLibrary's rows for the track changed, a journal of edits this build can't add to), or
/// stopped, is refused, and the drive is as the change left it.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_cue_plan_is_refused_once_its_drive_changed(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let at = tc.draw(gs::integers::<u32>().min_value(1_000).max_value(60_000));
    let hot = tc.draw(gs::integers::<u8>().max_value(7));
    for change in 0..7u8 {
        let root = common::demo_drive(dir.path(), &format!("Main{change}"));
        if change == 4 {
            common::add_onelibrary(&root);
        }
        let id = 1;
        let edit = CueEdit::Set(set(Some(hot), CuePosition::Ms(at)));
        let p = plan(&root, &[id], &edit).unwrap();
        let cancel = CancelToken::new();
        match change {
            0 => fs::remove_file(analysis(&root, id, false)).unwrap(),
            1 => folder_over(&analysis(&root, id, true)),
            2 => fs::write(analysis(&root, id, false), b"not an analysis file").unwrap(),
            // Other cues: another edit made in between.
            3 => apply(&root, &[id], &CueEdit::Set(set(Some((hot + 1) % 8), CuePosition::Ms(500)))),
            4 => {
                // OneLibrary's rows change, the analysis files don't.
                let files = [analysis(&root, id, false), analysis(&root, id, true)].map(|p| (fs::read(&p).unwrap(), p));
                apply(&root, &[id], &CueEdit::Set(set(Some((hot + 1) % 8), CuePosition::Ms(500))));
                for (bytes, p) in files {
                    fs::write(p, bytes).unwrap();
                }
            }
            5 => {
                let journal = Device::open(&root).unwrap().state_dir().join("edits.json");
                fs::create_dir_all(journal.parent().unwrap()).unwrap();
                fs::write(journal, br#"{"version": 99, "entries": []}"#).unwrap();
            }
            _ => cancel.cancel(),
        }
        let before = common::library_files(&root);
        let ctx = Ctx::new(&NoProgress, cancel);
        let err = cue_edit::apply(&p, &mut Device::open_export(&root).unwrap(), &ctx).unwrap_err();
        tc.note(&format!("change {change}: {err}"));
        match change {
            0 | 3 | 4 => assert!(matches!(err, Error::StalePlan(_)), "{err}"),
            6 => assert!(matches!(err, Error::Cancelled), "{err}"),
            _ => {}
        }
        assert_eq!(common::library_files(&root), before, "change {change}");
    }
}

/// On a track with no `.EXT` on a drive without OneLibrary, a cue's name or colour has nowhere to
/// go: renaming or recolouring one changes nothing, and the plan says why. With OneLibrary, it's
/// made there.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn names_a_track_cant_store_change_nothing_and_say_so(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let onelibrary = tc.draw(gs::booleans());
    if onelibrary {
        common::add_onelibrary(&root);
    }
    let id = 2;
    let at = tc.draw(gs::integers::<u32>().min_value(1_000).max_value(60_000));
    apply(&root, &[id], &CueEdit::Set(set(Some(0), CuePosition::Ms(at))));
    fs::remove_file(analysis(&root, id, true)).unwrap();
    let name = tc.draw(gs::sampled_from(vec!["Drop", "Intro"])).to_string();
    let color = tc.draw(gs::optional(gs::sampled_from(vec!["red", "aqua"]))).map(String::from);
    let update = CueEdit::Update(UpdateCue { cue: CueRef::Hot(HotCue(0)), name: Some(name), color });
    let before = common::Before::take(&root);
    let p = plan(&root, &[id], &update).unwrap();
    assert!(p.warnings.iter().any(|w| w.contains("has no .EXT analysis file")), "{:?}", p.warnings);
    assert_eq!(p.is_noop(), !onelibrary);
    if !onelibrary {
        before.untouched(&root).unwrap();
    }
}

/// A cue edit that fails just before its swap (as a crash or a full disk would) leaves the drive's
/// library as it was, and so does one on a drive another write holds.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn a_cue_edit_that_cant_write_leaves_the_drive_as_it_was(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let at = tc.draw(gs::integers::<u32>().min_value(1_000).max_value(60_000));
    let p =
        plan(&root, &[1], &CueEdit::Set(set(Some(tc.draw(gs::integers::<u8>().max_value(7))), CuePosition::Ms(at))))
            .unwrap();
    let before = common::Before::take(&root);
    let mut d = Device::open_export(&root).unwrap();
    locallibrary::testing::fail_before_swap(true);
    let failed = cue_edit::apply(&p, &mut d, &Ctx::silent());
    locallibrary::testing::fail_before_swap(false);
    assert!(failed.is_err());
    before.untouched(&root).unwrap();
    let held = d.lock().unwrap();
    let locked = cue_edit::apply(&p, &mut Device::open_export(&root).unwrap(), &Ctx::silent());
    assert!(matches!(locked, Err(Error::Locked(_))), "{locked:?}");
    drop(held);
    before.untouched(&root).unwrap();
}

/// A memory cue is found by a time within 0.1 s of it: deleting one named that close takes it
/// (the nearest of two), and one named further off isn't there.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(at = 10_000u32, gap = 300u32, off = 110i32)]
#[hegel::explicit_test_case(at = 10_000u32, gap = 30u32, off = -100i32)]
fn a_memory_cue_is_found_within_a_tenth_of_a_second(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let id = 1;
    let start = plan(&root, &[id], &CueEdit::Delete(CueRef::All));
    if let Ok(p) = start {
        cue_edit::apply(&p, &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap();
    }
    let at = tc.draw(gs::integers::<u32>().min_value(10_000).max_value(60_000));
    let gap = tc.draw(gs::integers::<u32>().min_value(2).max_value(300));
    for t in [at, at + gap] {
        apply(&root, &[id], &CueEdit::Set(set(None, CuePosition::Ms(t))));
    }
    let off = tc.draw(gs::integers::<i32>().min_value(-200).max_value(200));
    let named = (at as i64 + off as i64) as u32;
    let near = |t: u32| t.abs_diff(named);
    let found = [at, at + gap].into_iter().filter(|t| near(*t) <= 100).min_by_key(|t| near(*t));
    let got = plan(&root, &[id], &CueEdit::Delete(CueRef::Memory(CuePosition::Ms(named))));
    tc.note(&format!("cues at {at} and {} named {named}: {got:?}", at + gap));
    match found {
        Some(t) => {
            cue_edit::apply(&got.unwrap(), &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap();
            let left: Vec<u32> = cues(&root, id).iter().map(|c| c.start_ms).collect();
            assert_eq!(left, [at, at + gap].into_iter().filter(|x| *x != t).collect::<Vec<_>>());
        }
        None => assert!(matches!(got, Err(Error::NotFound(_))), "{got:?}"),
    }
}

/// On a track whose cues live in one analysis file (its `.EXT` gone, or the database naming the
/// `.EXT` itself), a hot cue is set where it's asked for, without its name, which the plan says
/// can't be stored.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn a_track_with_one_analysis_file_takes_cues_without_names(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let lone_ext = tc.draw(gs::booleans());
    let id = match lone_ext {
        true => common::add_track(
            &root,
            NewTrack {
                analyze_path: "/PIONEER/USBANLZ/P0EE/00000002/ANLZ0000.EXT".into(),
                duration: 200,
                ..NewTrack::new("Lone", "/Contents/Lone.mp3")
            },
            b"ID3",
        ),
        false => {
            fs::remove_file(analysis(&root, 2, true)).unwrap();
            2
        }
    };
    let at = tc.draw(gs::integers::<u32>().min_value(1_000).max_value(120_000));
    let hot = tc.draw(gs::integers::<u8>().max_value(7));
    let edit = CueEdit::Set(SetCue { name: Some("Named".into()), ..set(Some(hot), CuePosition::Ms(at)) });
    let p = plan(&root, &[id], &edit).unwrap();
    assert!(p.warnings.iter().any(|w| w.contains("has no .EXT analysis file")), "{:?}", p.warnings);
    cue_edit::apply(&p, &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap();
    let got: Vec<_> = cues(&root, id).into_iter().filter(|c| c.hot_cue.is_some()).collect();
    let set = got.iter().find(|c| c.hot_cue == Some(HotCue(hot).to_string())).expect("the hot cue");
    assert_eq!((set.start_ms, set.name.as_str()), (at, ""));
}

/// Converting a drive to OneLibrary gives each track as many cue rows as its analysis files hold
/// cues; a track whose analysis files can't be read gets none, and the conversion says so.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn converted_tracks_get_the_cues_their_analysis_files_hold(tc: TestCase) {
    use locallibrary::convert::{self, ConvertOptions, Target};
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
        let at = tc.draw(gs::integers::<u32>().min_value(1_000).max_value(100_000));
        apply(
            &root,
            &[1],
            &CueEdit::Set(set(tc.draw(gs::optional(gs::integers::<u8>().max_value(7))), CuePosition::Ms(at))),
        );
    }
    let unreadable = tc.draw(gs::booleans());
    if unreadable {
        fs::write(analysis(&root, 2, false), b"not an analysis file").unwrap();
    }
    let lib = common::library(&root);
    let want: Vec<(String, usize)> = lib
        .tracks
        .iter()
        .map(|t| (t.file_path.clone(), if unreadable && t.id == 2 { 0 } else { cues(&root, t.id).len() }))
        .collect();
    let warnings = common::Warnings::default();
    let ctx = warnings.ctx();
    let opts = ConvertOptions { to: Target::OneLibrary, replace: false };
    let mut d = Device::open_export(&root).unwrap();
    let plan = convert::plan(&d, &opts, &ctx).unwrap();
    let report = convert::apply(&plan, &mut d, &opts, &ctx).unwrap();
    assert_eq!(report.cues, want.iter().map(|(_, n)| n).sum::<usize>());
    let one = locallibrary::onelibrary::OneLibrary::open_root(&root).unwrap();
    let plus = one.library().unwrap();
    for (file, n) in &want {
        let id = plus.tracks.iter().find(|t| t.file_path == *file).unwrap().id;
        assert_eq!(one.cue_rows(id).unwrap().len(), *n, "{file}");
    }
    let said = warnings.said().iter().any(|w| w.contains("analysis files that can't be read"));
    assert_eq!(said, unreadable);
}
