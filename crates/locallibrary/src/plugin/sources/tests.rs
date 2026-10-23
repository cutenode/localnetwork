use hegel::generators as gs;
use loclib_model::analysis::Beat;
use loclib_plugin::{SourceCue, SourceTrack};

use super::*;

/// A track a source answered with `beats` beats and `cues` cues.
fn answered(beats: usize, cues: usize) -> Answered {
    let beat = Beat { beat_number: 1, bpm: 120.0, time_ms: 0 };
    let track =
        SourceTrack { beats: vec![beat; beats], cues: vec![SourceCue::default(); cues], ..SourceTrack::default() };
    Answered { track, bands: None }
}

/// A read's beats and cues add up across its pages, and past the limit it stops, saying so.
#[test]
fn a_read_stops_past_its_beats_and_cues() {
    let page = [answered(4, 1), answered(3, 0)];
    assert_eq!(counted(0, &page, 10, "t:x").unwrap(), 8);
    assert_eq!(counted(2, &page, 10, "t:x").unwrap(), 10);
    let e = counted(3, &page, 10, "t:x").unwrap_err();
    assert!(e.to_string().contains("t:x read more than 10 beats and cues"), "{e}");
    // A large library's: 50,000 ten-minute tracks at 128 BPM.
    const { assert!(MAX_ITEMS >= 50_000 * 1_280) };
}

/// For any pages, a read goes on while their beats and cues come to at most the limit, and
/// stops at the first page that takes them past it.
#[hegel::test(test_cases = 100)]
fn any_read_stops_at_the_page_past_the_limit(tc: hegel::TestCase) {
    let limit = tc.draw(gs::integers::<usize>().max_value(200));
    let pages: Vec<Vec<(usize, usize)>> = tc.draw(
        gs::vecs(
            gs::vecs(hegel::tuples!(gs::integers::<usize>().max_value(40), gs::integers::<usize>().max_value(8)))
                .max_size(4),
        )
        .max_size(6),
    );
    let (mut items, mut total) = (0, 0);
    for page in &pages {
        total += page.iter().map(|(b, c)| b + c).sum::<usize>();
        let page: Vec<Answered> = page.iter().map(|&(b, c)| answered(b, c)).collect();
        match counted(items, &page, limit, "t:x") {
            Ok(n) => {
                assert!(total <= limit && n == total);
                items = n;
            }
            Err(_) => {
                assert!(total > limit);
                return;
            }
        }
    }
}
