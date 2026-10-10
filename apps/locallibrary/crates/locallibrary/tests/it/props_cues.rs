//! Properties of hot cue names, cue colours and cue times (`cue_edit`), and of cue edits replayed
//! from the drive's journal after a new export.

use crate::common;

use std::fs;
use std::path::Path;

use hegel::generators as gs;
use hegel::{Generator, HealthCheck, TestCase};
use locallibrary::anlz::{CuePoint, HOT_CUE_PALETTE};
use locallibrary::cue_edit::{self, CueEdit, CuePosition, CueRef, HotCue, SetCue, UpdateCue};
use locallibrary::device::Device;
use locallibrary::edit::{self, EditOptions};
use locallibrary::onelibrary::OneLibrary;
use locallibrary::progress::Ctx;
use locallibrary::{Library, paths};

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn distance(a: [u8; 3], b: [u8; 3]) -> i32 {
    (0..3).map(|i| (i32::from(a[i]) - i32::from(b[i])).pow(2)).sum()
}

fn hot_cue() -> CuePoint {
    CuePoint { hot_cue: Some(0), ..Default::default() }
}

/// The eight slots read back from their letter (either case, with spaces) and from JSON.
#[hegel::test(test_cases = 100)]
fn hot_cue_names_round_trip(tc: TestCase) {
    let h = HotCue(tc.draw(gs::integers::<u8>().max_value(HotCue::COUNT - 1)));
    let name = h.to_string();
    assert_eq!(name.parse::<HotCue>().unwrap(), h);
    assert_eq!(format!(" {} ", name.to_lowercase()).parse::<HotCue>().unwrap(), h);
    let json = serde_json::to_string(&h).unwrap();
    assert_eq!(json, format!("\"{name}\""));
    assert_eq!(serde_json::from_str::<HotCue>(&json).unwrap(), h);
}

/// Only A-H parse, and a slot's name is never another slot's: any slot number a file holds,
/// however corrupt, has a name, and only the eight real slots have A-H.
#[hegel::test(test_cases = 500)]
#[hegel::explicit_test_case(h = 32u8)]
#[hegel::explicit_test_case(h = 191u8)]
#[hegel::explicit_test_case(h = 255u8)]
fn hot_cue_letters_are_injective(tc: TestCase) {
    let h = tc.draw(gs::integers::<u8>());
    let name = HotCue(h).to_string();
    assert_eq!(name.parse::<HotCue>().ok(), (h < HotCue::COUNT).then_some(HotCue(h)), "{h} -> {name:?}");
}

/// Parsing any text never panics and accepts only one letter A-H.
#[hegel::test(test_cases = 500)]
fn hot_cue_parse_accepts_only_letters(tc: TestCase) {
    let s = tc.draw(hegel::one_of!(gs::text().max_size(4), gs::text().alphabet("aAhHiI ").max_size(3)));
    if let Ok(h) = s.parse::<HotCue>() {
        assert!(h.0 < HotCue::COUNT);
        assert!(s.trim().eq_ignore_ascii_case(&h.to_string()), "{s:?}");
    }
}

/// rekordbox's 62 hot cue colours are all different, so a colour names one palette code.
#[test]
fn hot_cue_palette_colours_are_unique() {
    for (i, a) in HOT_CUE_PALETTE.iter().enumerate() {
        for b in &HOT_CUE_PALETTE[i + 1..] {
            assert_ne!(a, b, "palette code {} repeats", i + 1);
        }
    }
}

/// A hot cue given palette code n, or palette entry n's colour, gets code n and its RGB.
#[hegel::test(test_cases = 200)]
fn hot_cue_palette_codes_round_trip(tc: TestCase) {
    let code = tc.draw(gs::integers::<u8>().min_value(1).max_value(62));
    let rgb = HOT_CUE_PALETTE[usize::from(code) - 1];
    for spec in [code.to_string(), hex(rgb), hex(rgb).to_uppercase()] {
        let mut c = hot_cue();
        cue_edit::set_color(&mut c, &spec).unwrap();
        assert_eq!((c.color_code, c.color_rgb), (code, rgb), "{spec}");
    }
}

/// Any `#rrggbb` picks the nearest palette colour for a hot cue and the nearest of the eight
/// colours for a memory cue.
#[hegel::test(test_cases = 300)]
fn any_rgb_picks_a_nearest_colour(tc: TestCase) {
    let rgb = tc.draw(gs::arrays::<_, u8, 3>(gs::integers::<u8>()));
    let mut c = hot_cue();
    cue_edit::set_color(&mut c, &hex(rgb)).unwrap();
    assert!((1..=62).contains(&c.color_code));
    let best = HOT_CUE_PALETTE.iter().map(|p| distance(*p, rgb)).min().unwrap();
    assert_eq!(distance(c.color_rgb, rgb), best);

    let mut m = CuePoint::default();
    cue_edit::set_color(&mut m, &hex(rgb)).unwrap();
    assert!((1..=8).contains(&m.color_id));
}

/// Whatever the text, a colour either applies (leaving a code the players know) or is refused.
#[hegel::test(test_cases = 500)]
fn set_color_accepts_only_known_colours(tc: TestCase) {
    let spec = tc.draw(hegel::one_of!(
        gs::text().max_size(10),
        gs::from_regex("#[0-9a-fA-F+-]{0,7}").fullmatch(true),
        gs::sampled_from(vec!["red", "Aqua", "violet", "none", "0", "1", "8", "9", "62", "63", "teal", "sky"])
            .map(String::from),
    ));
    let hot = tc.draw(gs::booleans());
    let mut c = if hot { hot_cue() } else { CuePoint::default() };
    if cue_edit::set_color(&mut c, &spec).is_ok() {
        if hot {
            assert!(c.color_code <= 62, "{spec:?}");
        } else {
            assert!(c.color_id <= 8, "{spec:?}");
        }
    }
}

/// `m:ss.mmm` names the exact millisecond.
#[hegel::test(test_cases = 300)]
fn clock_reads_back(tc: TestCase) {
    let ms = tc.draw(gs::integers::<u32>());
    let s = cue_edit::clock(ms);
    let (m, rest) = s.split_once(':').unwrap();
    let (sec, milli) = rest.split_once('.').unwrap();
    assert_eq!((sec.len(), milli.len()), (2, 3), "{s}");
    let back = u64::from(m.parse::<u32>().unwrap()) * 60_000
        + u64::from(sec.parse::<u32>().unwrap()) * 1000
        + u64::from(milli.parse::<u32>().unwrap());
    assert_eq!(back, u64::from(ms));
}

// ---------------------------------------------------------------------------------------------
// Replaying `cues` edits
// ---------------------------------------------------------------------------------------------

/// What a new export replaces: the databases, and the tracks' analysis files.
fn exported(root: &Path, lib: &Library) -> Vec<(String, Vec<u8>)> {
    let mut keys: Vec<String> =
        [paths::EXPORT_PDB, paths::EXPORT_EXT_PDB, paths::LIBRARY_PLUS_DB].iter().map(|k| k.to_string()).collect();
    for t in &lib.tracks {
        let dat = paths::key(&t.analyze_path);
        keys.push(dat.replace(".DAT", ".EXT"));
        keys.push(dat);
    }
    keys.into_iter().filter_map(|k| fs::read(paths::to_fs(root, &k)).ok().map(|b| (k, b))).collect()
}

/// Every track's cues, from its analysis files and OneLibrary's rows, by file.
fn cue_state(root: &Path) -> String {
    let d = Device::open_export(root).unwrap();
    let lib = d.library().unwrap();
    let one = OneLibrary::open_root(root).unwrap();
    let plus = one.library().unwrap();
    let mut out = String::new();
    let ids: Vec<u32> = lib.tracks.iter().map(|t| t.id).collect();
    for c in cue_edit::list(&d, &ids).unwrap() {
        out += &format!("{} {:?}\n", c.track.file_path, c.cues);
        let content = plus.tracks.iter().find(|t| t.file_path == c.track.file_path).unwrap().id;
        let mut rows: Vec<String> = one
            .cue_rows(content)
            .unwrap()
            .iter()
            .map(|r| {
                let loop_beats = r.loop_beats;
                format!("{:?}", (r.hot_cue, r.in_usec, r.out_usec, &r.comment, r.color, r.active_loop, loop_beats))
            })
            .collect();
        rows.sort();
        out += &format!("  {rows:?}\n");
    }
    out
}

/// A position in a track of `secs` seconds: a time, or a beat of its grid.
#[hegel::test_helper]
fn position(tc: &TestCase, secs: u32) -> CuePosition {
    match tc.draw(gs::booleans()) {
        true => CuePosition::Ms(tc.draw(gs::integers::<u32>().max_value(secs * 1000))),
        false => CuePosition::Beat(tc.draw(gs::integers::<u32>().min_value(1).max_value(64))),
    }
}

/// One `cues` edit on one of the drive's tracks (`copy` from the other).
#[hegel::test_helper]
fn cue_edit(tc: &TestCase, lib: &Library) -> (Vec<u32>, CueEdit) {
    let i = tc.draw(gs::integers::<usize>().max_value(lib.tracks.len() - 1));
    let t = &lib.tracks[i];
    let hot = |tc: &TestCase| HotCue(tc.draw(gs::integers::<u8>().max_value(3)));
    let name = |tc: &TestCase| tc.draw(gs::optional(gs::sampled_from(&["Drop", "Intro ♪", ""][..]))).map(String::from);
    let color = |tc: &TestCase| tc.draw(gs::optional(gs::sampled_from(&["red", "aqua", "none"][..]))).map(String::from);
    let which = |tc: &TestCase| match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => CueRef::Hot(hot(tc)),
        1 => CueRef::Memory(position(tc, t.duration_secs)),
        _ => CueRef::All,
    };
    let edit = match tc.draw(gs::integers::<u8>().max_value(5)) {
        0..=2 => CueEdit::Set(SetCue {
            hot_cue: tc.draw(gs::booleans()).then(|| hot(tc)),
            at: position(tc, t.duration_secs),
            snap: tc.draw(gs::booleans()),
            loop_beats: tc.draw(gs::optional(gs::sampled_from(&[4.0, 0.5][..]))),
            name: name(tc),
            color: color(tc),
        }),
        3 => CueEdit::Update(UpdateCue { cue: which(tc), name: name(tc), color: color(tc) }),
        4 => CueEdit::Delete(which(tc)),
        _ => CueEdit::Copy { from: lib.tracks[(i + 1) % lib.tracks.len()].id },
    };
    (vec![t.id], edit)
}

/// After rekordbox exports over `cues` edits (the databases and analysis files as they were),
/// replaying the drive's journal gives back the cues the edits made, in the analysis files and in
/// OneLibrary's rows.
#[hegel::test(test_cases = 25, suppress_health_check = [HealthCheck::TooSlow])]
fn replaying_cue_edits_after_a_new_export_redoes_them(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let lib = Device::open_export(&root).unwrap().library().unwrap();
    let before = exported(&root, &lib);
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(6)) {
        let (ids, edit) = cue_edit(&tc, &lib);
        let mut d = Device::open_export(&root).unwrap();
        match cue_edit::plan(&d, &ids, &edit, &Ctx::silent()) {
            Ok(plan) => {
                tc.note(&format!("{ids:?} {edit:?}"));
                cue_edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
            }
            Err(e) => tc.note(&format!("{ids:?} {edit:?} refused: {e}")),
        }
    }
    let edited = cue_state(&root);
    for (key, bytes) in &before {
        fs::write(paths::to_fs(&root, key), bytes).unwrap();
    }
    let mut d = Device::open_export(&root).unwrap();
    let edits = edit::journal::edits(&d).unwrap();
    tc.note(&format!("journal: {edits:?}"));
    if edits.is_empty() {
        return;
    }
    let opts = EditOptions { lenient: true, no_journal: true };
    let plan = edit::plan(&d, &edits, &opts, &Ctx::silent()).unwrap();
    let report = edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert!(report.warnings.iter().all(|w| !w.starts_with("skipped")), "{:?}", report.warnings);
    assert_eq!(cue_state(&root), edited, "cues after the replay");
}
