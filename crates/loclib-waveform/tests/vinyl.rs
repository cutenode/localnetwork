use std::collections::HashSet;

use loclib_waveform::color::{mix, scale};
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

/// The record as text, a character a pixel: `L` the label, `o` the rim, `.` and `:` the record
/// and its grooves, `d` and `B` the streak's two steps, a space where nothing's drawn.
fn art(cols: u16, rows: u16, angle: f32) -> Vec<String> {
    const SHEEN: Rgb = [170, 170, 196];
    let (dim, bright, groove) = (mix(RECORD, SHEEN, 0.35), mix(RECORD, SHEEN, 0.7), scale(RECORD, 0.72));
    let px = vinyl(cols, rows, angle, LABEL, RECORD);
    let char_of = |p: &Option<Rgb>| match *p {
        None => ' ',
        Some(c) if c == LABEL => 'L',
        Some(c) if c == RIM => 'o',
        Some(c) if c == RECORD => '.',
        Some(c) if c == groove => ':',
        Some(c) if c == dim => 'd',
        Some(c) if c == bright => 'B',
        Some(c) => panic!("{c:?}"),
    };
    px.chunks(usize::from(cols)).map(|row| row.iter().map(char_of).collect()).collect()
}

/// The record pixel for pixel, worked out apart from this code (in single precision, as here):
/// square with its rings, wider than it's tall, and taller than it's wide with the record across
/// the whole width. In the last two the streak leaves pixels on their own, which take their
/// first grooved neighbour's colour (left, right, up, then down).
#[test]
fn the_record_is_drawn_pixel_for_pixel() {
    assert_eq!(
        art(24, 12, 1.3),
        [
            "         oooooo         ",
            "      oooBBBdddooo      ",
            "     odBBBBBdd....o     ",
            "    oddBBBBBdd.....o    ",
            "   oddddBBBBdd:::...o   ",
            "  o..dddBBBBdd::::...o  ",
            " o...:dddBBBd...:::...o ",
            " o..:::ddBBLL....:::..o ",
            " o..::..dLLLLLL...::..o ",
            "o...::..LLLLLLLL..::...o",
            "o..::...LLLLLLLL...::..o",
            "o..::..LLLL  LLLL..::..o",
            "o..::..LLLL  LLLL..::..o",
            "o..::...LLLLLLLL...::..o",
            "o...::..LLLLLLLL..::...o",
            " o..::...LLLLLLd..::..o ",
            " o..:::....LLBBdd:::..o ",
            " o...:::...dBBBddd:...o ",
            "  o...::::ddBBBBddd..o  ",
            "   o...:::ddBBBBddddo   ",
            "    o.....ddBBBBBddo    ",
            "     o....ddBBBBBdo     ",
            "      ooodddBBBooo      ",
            "         oooooo         ",
        ]
    );
    assert_eq!(
        art(20, 6, 0.7),
        [
            "        oooo        ",
            "      oo....oo      ",
            "     oBdd.....o     ",
            "     oBBd.....o     ",
            "    oddBLLLL...o    ",
            "    o...L  L...o    ",
            "    o...L  L...o    ",
            "    o...LLLLBddo    ",
            "     o.....dBBo     ",
            "     o.....ddBo     ",
            "      oo....oo      ",
            "        oooo        ",
        ]
    );
    assert_eq!(
        art(11, 9, 2.0),
        [
            "           ",
            "           ",
            "           ",
            "     o     ",
            "  ooooooo  ",
            " oo..dBBoo ",
            " o...dBBBo ",
            "o...LLL...o",
            "o..LL LL..o",
            "o..LL LL..o",
            "o...LLL...o",
            " oBBBd...o ",
            " ooBBd..oo ",
            "  ooooooo  ",
            "     o     ",
            "           ",
            "           ",
            "           ",
        ]
    );
}

/// A pixel right on an edge is drawn by what's outside it: one whose centre is exactly the label's
/// radius away is the record, and one whose light is exactly a step of the streak's is the step
/// below. (At 51 x 25 the centre of column 25, row 15, is 9.5 pixels from the middle, the label's
/// radius; the angles are ones at which a pixel's light is exactly 0.97 or 0.87 in single
/// precision.)
#[test]
fn a_pixel_on_an_edge_is_drawn_by_whats_outside_it() {
    assert_eq!(&art(51, 25, 0.0)[15][23..28], ".....");
    assert_eq!(art(24, 12, f32::from_bits(0xbf97_80a0))[4].as_bytes()[17], b'd', "light exactly 0.97");
    assert_eq!(art(24, 12, f32::from_bits(0xbf9b_e28a))[6].as_bytes()[18], b':', "light exactly 0.87");
}
