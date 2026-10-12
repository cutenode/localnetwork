//! Replacing a copy found by its sound, and the cues it has: carried onto a keeper with none,
//! moved by the offset measured between the two, or keeping the copy when the keeper has cues of
//! its own that aren't these.

use crate::common::{self, add_track as add};
use crate::doctor::sound::{DELAY_MS, HI_RES, ORIGINAL, cues, recording, set_points, store, track};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::anlz::CuePoint;
use locallibrary::api::{self, DedupeRequest};
use locallibrary::progress::Ctx;
use locallibrary::testing;

/// The cues drawn (a hot cue, a memory cue, a looping hot cue), from `at`, 3 s apart.
fn points(kinds: &[u8], at: u32) -> Vec<CuePoint> {
    kinds
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let start_ms = at + 3_000 * i as u32;
            match k {
                0 => CuePoint { hot_cue: Some(i as u8), start_ms, ..Default::default() },
                1 => CuePoint { start_ms, ..Default::default() },
                _ => {
                    CuePoint { hot_cue: Some(i as u8), start_ms, end_ms: Some(start_ms + 2_000), ..Default::default() }
                }
            }
        })
        .collect()
}

/// A 20 s song as a 44.1 kHz WAV with drawn cues, and the same song as a 48 kHz 24-bit WAV
/// starting 26 ms later, the keeper, with no cues, the copy's cues where they fall in it, or cues of its
/// own: the copy is replaced and its cues carried onto the keeper (each where the same moment
/// is, to the millisecond) when it has none, replaced without carrying when the keeper has them
/// already, and otherwise kept, the plan saying why. Each case decodes both songs, which is
/// slow: the explicit cases take each kind of keeper once.
#[hegel::test(test_cases = 1, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(kinds = vec![0u8, 1, 2], at = 10_000u32, keeper = 0u8)]
#[hegel::explicit_test_case(kinds = vec![2u8], at = 4_000u32, keeper = 1u8)]
#[hegel::explicit_test_case(kinds = vec![1u8, 0], at = 6_000u32, keeper = 2u8)]
fn a_copys_cues_are_carried_onto_a_keeper_without_any(tc: TestCase) {
    let kinds = tc.draw(gs::vecs(gs::integers::<u8>().max_value(2)).min_size(1).max_size(3));
    let at = tc.draw(gs::integers::<u32>().min_value(1_000).max_value(10_000));
    // The keeper's cues: none, the copy's, or one of its own.
    let keeper = tc.draw(gs::integers::<u8>().max_value(2));

    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let _store = store(dir.path());
    let original = add(&root, track("Song", ORIGINAL, 1, 20, 44_100, 16), &recording(7919, 20.0, 44_100, 16, 0, None));
    let hi_res = add(
        &root,
        track("Song (hi-res)", HI_RES, 2, 20, 48_000, 24),
        &recording(7919, 20.0, 48_000, 24, DELAY_MS, None),
    );
    let theirs = points(&kinds, at);
    set_points(&root, original, &theirs);
    match keeper {
        0 => {}
        1 => {
            let moved: Vec<CuePoint> = theirs
                .iter()
                .map(|p| CuePoint {
                    start_ms: p.start_ms + DELAY_MS,
                    end_ms: p.end_ms.map(|e| e + DELAY_MS),
                    ..p.clone()
                })
                .collect();
            set_points(&root, hi_res, &moved);
        }
        _ => set_points(&root, hi_res, &[CuePoint { start_ms: 500, ..Default::default() }]),
    }
    let before = cues(&root, original);
    let kept_before = cues(&root, hi_res);

    let req = DedupeRequest { drive: common::path(&root), by_sound: true, ..Default::default() };
    let plan = api::dedupe_plan(&req, &Ctx::silent()).unwrap();
    let removed: Vec<u32> = plan.tracks_removed.iter().map(|t| t.id).collect();
    let carries = plan.changes.iter().any(|c| c.starts_with(&format!("carry the cues of {ORIGINAL}")));
    match keeper {
        2 => {
            assert!(removed.is_empty(), "{:?}", plan.warnings);
            let why = format!(
                "{ORIGINAL} stays: its cues couldn't be carried onto {HI_RES}: it has cues of its own, and not all of these"
            );
            assert!(plan.warnings.contains(&why), "{:?}", plan.warnings);
        }
        _ => {
            assert_eq!(removed, [original], "{:?}", plan.warnings);
            assert_eq!(carries, keeper == 0, "{:?}", plan.changes);
        }
    }
    plan.apply(None, &Ctx::silent()).unwrap();
    let after = cues(&root, hi_res);
    match keeper {
        0 => {
            assert_eq!(after.len(), before.len());
            let moved = |at: u32, was: u32| (i64::from(at) - i64::from(was) - i64::from(DELAY_MS)).abs() <= 2;
            for ((slot, start, end), (was_slot, was_start, was_end)) in after.iter().zip(&before) {
                assert_eq!(slot, was_slot);
                assert!(moved(*start, *was_start), "{after:?} from {before:?}");
                assert!(end.zip(*was_end).is_none_or(|(e, w)| moved(e, w)) && end.is_some() == was_end.is_some());
            }
        }
        _ => assert_eq!(after, kept_before, "the keeper's own cues stay"),
    }
    testing::store_root_on_this_thread(None);
}
