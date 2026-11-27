use super::*;
use crate::anlz::{Mood, Phrase, PhraseKind};

/// `n` beats at `bpm`, the first at `from` ms.
fn steady(n: usize, bpm: f64, from: u32) -> Vec<Beat> {
    (0..n)
        .map(|i| Beat { beat_number: (i % 4) as u16 + 1, bpm, time_ms: from + (i as f64 * 60_000.0 / bpm) as u32 })
        .collect()
}

fn phrases(p: &[(PhraseKind, u16, u16, Option<u16>)]) -> SongStructure {
    let phrases =
        p.iter().map(|&(kind, start_beat, end_beat, fill_beat)| Phrase { kind, start_beat, end_beat, fill_beat });
    SongStructure { mood: Mood::High, phrases: phrases.collect() }
}

/// rekordbox's phrases on 64 beats at 120 BPM from 1 s: intro, chorus (a fill-in on beat 29) and
/// outro, to the grid's end.
fn rekordboxs() -> SongStructure {
    phrases(&[
        (PhraseKind::Intro, 1, 17, None),
        (PhraseKind::Chorus, 17, 33, Some(29)),
        (PhraseKind::Outro, 33, 65, None),
    ])
}

#[test]
fn phrases_follow_a_new_grid_by_time() {
    let old = steady(64, 120.0, 1000);
    let on = |new: &[Beat]| phrases_on(&rekordboxs(), &old, new, "a");
    // Two beats earlier: the same times are two beats on.
    let (moved, warned) = on(&steady(70, 120.0, 0));
    let want =
        [(PhraseKind::Intro, 3, 19, None), (PhraseKind::Chorus, 19, 35, Some(31)), (PhraseKind::Outro, 35, 67, None)];
    assert_eq!((moved, warned), (Some(phrases(&want)), Vec::<String>::new()));
    // At double tempo, every beat number doubles.
    let (moved, warned) = on(&steady(128, 240.0, 1000));
    let want =
        [(PhraseKind::Intro, 1, 33, None), (PhraseKind::Chorus, 33, 65, Some(57)), (PhraseKind::Outro, 65, 129, None)];
    assert_eq!((moved, warned), (Some(phrases(&want)), Vec::<String>::new()));
    // A grid starting 4 s later: the intro starts on its first beat, 4 s off.
    let (moved, warned) = on(&steady(64, 120.0, 5000));
    let want =
        [(PhraseKind::Intro, 1, 9, None), (PhraseKind::Chorus, 9, 25, Some(21)), (PhraseKind::Outro, 25, 57, None)];
    assert_eq!(moved, Some(phrases(&want)));
    assert_eq!(warned, ["a's phrases 1 move by more than a beat onto the new grid"]);
}

#[test]
fn phrases_that_meet_on_the_new_grid_go() {
    let old = steady(64, 120.0, 1000);
    let short =
        phrases(&[(PhraseKind::Intro, 1, 2, None), (PhraseKind::Up, 2, 3, None), (PhraseKind::Chorus, 3, 65, None)]);
    // At 50 BPM the up (1.5 s) is nearest the intro's beat, and goes.
    let (moved, warned) = phrases_on(&short, &old, &steady(30, 50.0, 1000), "a");
    assert_eq!(moved, Some(phrases(&[(PhraseKind::Intro, 1, 2, None), (PhraseKind::Chorus, 2, 28, None)])));
    assert_eq!(warned, ["a's phrases 2 fall on one beat with the next on the new grid, so they go"]);
    // A grid starting after the old one ends: the one phrase starts and ends on its first beat.
    let whole = phrases(&[(PhraseKind::Intro, 1, 65, None)]);
    let (moved, warned) = phrases_on(&whole, &old, &steady(8, 120.0, 40_000), "a");
    assert_eq!(moved, None);
    assert!(warned.last().is_some_and(|w| w.ends_with("so the .EXT loses them")), "{warned:?}");
}
