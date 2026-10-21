use hegel::TestCase;
use hegel::generators::{self as gs, Generator};

use super::*;
use crate::Error;
use crate::edit::TrackFields;
use crate::harmony::Camelot;

/// A cue as a file might have it: a hot cue (`A` to `H`, or a slot that isn't one) or a memory
/// cue, somewhere in its first ten seconds, so some are near each other.
#[hegel::test_helper]
fn cue(tc: &TestCase) -> ImportedCue {
    let hot_cue = tc.draw(gs::optional(gs::sampled_from(vec!["A", "b", "C", "H", "I"]))).map(String::from);
    ImportedCue { hot_cue, ..cue_at(tc.draw(gs::integers::<u32>().max_value(10_000))) }
}

fn cue_at(start_ms: u32) -> ImportedCue {
    ImportedCue { hot_cue: None, start_ms, loop_beats: None, name: None, color: None }
}

/// Of a file's cues, only those a track has no cue for (in its slot, or within 0.1 s of where a
/// memory cue is), and none from where the track ends, are set; each slot and place once, the
/// first in the file's order the cue edit takes. One in a slot that isn't one, or that the edit
/// refuses, is left out saying why, and takes no slot or place. Every other is left out quietly.
#[hegel::test(test_cases = 200)]
fn only_cues_a_track_has_nothing_for_are_added(tc: TestCase) {
    let slots = gs::optional(gs::integers::<u8>().max_value(HotCue::COUNT - 1).map(HotCue));
    let has: Vec<(Option<HotCue>, u32)> =
        tc.draw(gs::vecs(gs::tuples!(slots, gs::integers::<u32>().max_value(10_000))).max_size(4));
    // Each named by its place in the file, and drawn to be refused by the edit or not.
    let cues: Vec<(ImportedCue, bool)> = (0..tc.draw(gs::integers::<usize>().max_value(8)))
        .map(|i| (ImportedCue { name: Some(i.to_string()), ..cue(&tc) }, tc.draw(gs::booleans())))
        .collect();
    let ends = tc.draw(gs::optional(gs::integers::<u32>().max_value(12_000)));
    let file: Vec<&ImportedCue> = cues.iter().map(|(c, _)| c).collect();
    let mut checked = Vec::new();
    let (added, refused) = adding(&has, &file, ends, |s| {
        let i: usize = s.name.as_deref().unwrap().parse().unwrap();
        checked.push(i);
        match cues[i].1 {
            true => Err(Error::invalid("refused")),
            false => Ok(()),
        }
    });
    let (mut taken, mut expected, mut why) = (has.clone(), Vec::new(), Vec::new());
    let mut checks = Vec::new();
    for (i, (c, no)) in cues.iter().enumerate() {
        let Ok(slot) = c.hot_cue.as_deref().map(str::parse::<HotCue>).transpose() else {
            why.push(format!("hot cues are A-H, not {:?}", c.hot_cue.as_deref().unwrap()));
            continue;
        };
        let free = match slot {
            Some(_) => !taken.iter().any(|(s, _)| *s == slot),
            None => !taken.iter().any(|(s, at)| s.is_none() && at.abs_diff(c.start_ms) <= FIND_TOLERANCE_MS),
        };
        if !free || ends.is_some_and(|end| c.start_ms >= end) {
            continue;
        }
        checks.push(i);
        match no {
            true => why.push("refused".into()),
            false => {
                taken.push((slot, c.start_ms));
                expected.push((slot, c.start_ms));
            }
        }
    }
    let at = |c: &SetCue| match c.at {
        CuePosition::Ms(ms) => (c.hot_cue, ms),
        CuePosition::Beat(_) => panic!("a file's cue is at a time"),
    };
    assert_eq!(added.iter().map(at).collect::<Vec<_>>(), expected);
    assert_eq!(refused, why);
    assert_eq!(checked, checks, "each cue to set is checked once, in turn");
}

/// A file's key and BPM are written as an analyzer's are: only where the track has none (or over
/// it, when asked), never one it has in another notation, a key as rekordbox names it and a BPM
/// to hundredths, within what a track can have.
#[hegel::test(test_cases = 200)]
fn a_key_and_a_bpm_are_written_only_where_a_track_has_none(tc: TestCase) {
    let codes: Vec<String> = (1..=12).flat_map(|n| [format!("{n}A"), format!("{n}B")]).collect();
    let keys = || gs::optional(gs::sampled_from(codes.clone()));
    let (has_key, key) = (tc.draw(keys()), tc.draw(keys()));
    let bpms = || gs::optional(gs::floats::<f64>().min_value(-10.0).max_value(1100.0));
    let (has_bpm, bpm) = (tc.draw(bpms()).unwrap_or(0.0), tc.draw(bpms()));
    let overwrite = tc.draw(gs::booleans());
    let key_of = |code: &String| Camelot::parse(code).unwrap().note();
    let t = Track { key: has_key.as_ref().map(key_of).unwrap_or_default(), bpm: has_bpm, ..Track::default() };
    let set = key_and_bpm(&t, key.as_deref(), bpm, overwrite);
    let (had, given) = (Camelot::parse(&t.key), key.as_deref().and_then(Camelot::parse));
    let key_wanted = given.is_some() && given != had && (overwrite || had.is_none());
    assert_eq!(set.key, given.filter(|_| key_wanted).map(|k| k.note()));
    let rounded = bpm.map(|b| (b * 100.0).round() / 100.0).filter(|b| (1.0..=999.99).contains(b));
    let bpm_wanted = rounded.is_some_and(|b| b != t.bpm && (overwrite || t.bpm <= 0.0));
    assert_eq!(set.bpm, rounded.filter(|_| bpm_wanted));
    assert_eq!(TrackFields { key: None, bpm: None, ..set }, TrackFields::default(), "nothing else");
}
