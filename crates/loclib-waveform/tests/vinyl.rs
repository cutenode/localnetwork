use std::collections::HashSet;

use loclib_waveform::{Rgb, vinyl};

const LABEL: Rgb = [220, 40, 90];
const RECORD: Rgb = [40, 40, 52];
const RIM: Rgb = [74, 74, 90];

/// The record at `size` pixels square, turned to `angle`.
fn record(size: u16, angle: f32) -> Vec<Option<Rgb>> {
    vinyl(size, size / 2, angle, LABEL, RECORD)
}

/// Every pixel's one of a small set of colours, at the sizes the player draws it and as it turns.
#[test]
fn the_record_is_drawn_in_a_few_colours() {
    for size in [18, 36] {
        for step in 0..24 {
            let px = record(size, step as f32 * 0.27);
            let colours: HashSet<_> = px.iter().flatten().collect();
            assert!(colours.len() <= 6, "{size}x{size}: {} colours", colours.len());
            assert!(colours.contains(&LABEL), "a flat label");
        }
    }
}

/// No pixel in the rings is on its own: each has a neighbour, up, down or to the side, the same
/// colour as it.
#[test]
fn the_rings_have_no_speckle() {
    for size in [18, 24, 36, 50] {
        let w = usize::from(size);
        for step in 0..48 {
            let px = record(size, step as f32 * 0.131);
            let at = |x: usize, y: usize| px[y * w + x];
            for y in 0..w {
                for x in 0..w {
                    let Some(c) = at(x, y) else { continue };
                    // The label's flat, and the rim's a line one pixel wide: it steps corner to corner.
                    if c == LABEL || c == RIM {
                        continue;
                    }
                    let same = [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)]
                        .into_iter()
                        .filter(|&(x, y)| x < w && y < w)
                        .any(|(x, y)| at(x, y) == Some(c));
                    assert!(same, "{size}x{size} at step {step}: ({x}, {y}) is on its own");
                }
            }
        }
    }
}

/// The rings are whole: along a radius away from the streak, the colour changes only a few times,
/// each ring at least two pixels wide.
#[test]
fn the_rings_are_whole_pixels() {
    for size in [18u16, 36] {
        let w = usize::from(size);
        // Straight up, with the streak lying across.
        let px = record(size, 0.0);
        let column: Vec<_> = (0..w / 2).map(|y| px[y * w + w / 2]).collect();
        let mut runs: Vec<(Option<Rgb>, usize)> = vec![];
        for c in column {
            match runs.last_mut() {
                Some((last, n)) if *last == c => *n += 1,
                _ => runs.push((c, 1)),
            }
        }
        let rings: Vec<_> = runs.iter().filter(|(c, _)| c.is_some_and(|c| c != LABEL && c != RIM)).collect();
        assert!((1..=4).contains(&rings.len()), "{size}x{size}: {runs:?}");
        assert!(rings.iter().all(|(_, n)| *n >= 2), "{size}x{size}: {runs:?}");
        assert_eq!(runs.iter().filter(|(c, _)| *c == Some(RIM)).map(|r| r.1).sum::<usize>(), 1, "a one-pixel rim");
        assert_eq!(px[(w / 2) * w + w / 2], None, "the spindle hole");
    }
}

/// The streak turns with the record, and holds still at a still angle.
#[test]
fn the_streak_turns_with_the_record() {
    assert_eq!(record(36, 0.6), record(36, 0.6));
    assert_ne!(record(36, 0.0), record(36, 1.0));
}
