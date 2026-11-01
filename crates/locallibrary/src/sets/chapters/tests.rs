use hegel::TestCase;
use hegel::generators as gs;

use super::*;
use crate::sets::query::tests::set;

/// A set of tracks `Artist n - Title n`, each starting where `starts` says.
fn timed(starts: &[Option<u32>]) -> Set {
    let mut s = set("3m2abcdefgh22", "2026-10-03", Some("Friday"));
    s.tracks = (1..)
        .zip(starts)
        .map(|(n, &start)| SetEntry { start_seconds: start, ..SetEntry::added(&format!("Artist {n} - Title {n}")) })
        .collect();
    s
}

fn named(n: usize, e: &SetEntry) -> String {
    format!("{n}. {}", e.name())
}

/// Chapters come in the order the tracks play, whatever the set's order, each ending where the next
/// starts and the last where the recording does. A track without a start time, starting with
/// another or after the recording ends gets none, and a note says so.
#[test]
fn chapters_follow_the_start_times() {
    let s = timed(&[Some(300), None, Some(0), Some(300), Some(4000), None]);
    let (found, notes) = chapters(&s, Some(3600), named);
    let got: Vec<(u32, &str)> = found.iter().map(|c| (c.start, c.title.as_str())).collect();
    assert_eq!(got, [(0, "3. Artist 3 - Title 3"), (300, "1. Artist 1 - Title 1")]);
    assert_eq!(
        notes,
        [
            "tracks 2, 6 have no start time, so no chapters",
            "track 4 starts when track 1 does (0:05:00), so no chapter of its own",
            "track 5 starts at 1:06:40, after the recording ends, so no chapter",
        ]
    );
    let spans: Vec<(u32, u64)> = ends(&found, 3600).map(|(c, end)| (c.start, end)).collect();
    assert_eq!(spans, [(0, 300), (300, 3600)]);

    // Without the recording's length, a late start is a chapter like any other.
    let (late, _) = chapters(&s, None, named);
    assert_eq!(late.last().map(|c| c.start), Some(4000));
    assert_eq!(chapters(&timed(&[None, Some(5)]), None, named).1, ["track 1 has no start time, so no chapter"]);
    let (none, notes) = chapters(&timed(&[None, None]), None, named);
    assert!(none.is_empty());
    assert_eq!(notes, ["no track has a start time, so there are no chapters"]);
    assert_eq!(chapters(&timed(&[]), Some(60), named), (vec![], vec![]));
}

/// Over drawn tracklists and recordings: chapters are in order and don't overlap, each ending where
/// the next starts and the last where the recording does; every track starting before the end gets
/// a chapter at its start, named for the first track starting there; and every other track is in a
/// note.
#[hegel::test(test_cases = 300)]
fn chapters_are_ordered_and_end_where_the_next_starts(tc: TestCase) {
    let starts = tc.draw(gs::vecs(gs::optional(gs::integers::<u32>().max_value(600))).max_size(12));
    let length = tc.draw(gs::optional(gs::integers::<u64>().max_value(700)));
    let s = timed(&starts);
    let (found, notes) = chapters(&s, length, named);
    for w in found.windows(2) {
        assert!(w[0].start < w[1].start, "{found:?}");
    }
    if let Some(length) = length {
        let spans: Vec<(u32, u64)> = ends(&found, length).map(|(c, end)| (c.start, end)).collect();
        assert_eq!(spans.len(), found.len());
        for (i, &(start, end)) in spans.iter().enumerate() {
            assert!(u64::from(start) < end, "{spans:?}");
            let next = spans.get(i + 1).map_or(length, |&(s, _)| u64::from(s));
            assert_eq!(end, next, "{spans:?}");
        }
    }
    let before_end = |start: u32| length.is_none_or(|l| u64::from(start) < l);
    for (n, start) in (1..).zip(&starts) {
        let Some(start) = *start else { continue };
        if !before_end(start) {
            assert!(found.iter().all(|c| c.start != start), "{found:?}");
            continue;
        }
        let first = 1 + starts.iter().position(|&s| s == Some(start)).unwrap();
        let chapter = found.iter().find(|c| c.start == start).unwrap_or_else(|| panic!("{n}: {found:?}"));
        assert_eq!(chapter.title, named(first, &s.tracks[first - 1]));
    }
    let timed_before_end = starts.iter().flatten().filter(|&&s| before_end(s));
    let mut distinct: Vec<u32> = timed_before_end.copied().collect();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(found.len(), distinct.len());
    // A note each for the late and the doubled, and one for every track without a start.
    let left_out = starts.iter().flatten().count() - found.len();
    let untimed = usize::from(starts.contains(&None));
    assert_eq!(notes.len(), left_out + untimed, "{notes:?}");
}
