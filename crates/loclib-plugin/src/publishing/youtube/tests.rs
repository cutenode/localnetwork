use hegel::TestCase;
use hegel::generators as gs;

use super::*;

fn entry(artist: Option<&str>, title: Option<&str>, start_seconds: Option<u32>) -> TracklistEntry {
    TracklistEntry { artist: artist.map(Into::into), title: title.map(Into::into), start_seconds, ..Default::default() }
}

/// The first timed track starts at 0; a later one is a chapter when it's at least ten seconds
/// after the chapter before and ten before the recording ends, else it has none.
#[test]
fn a_chapter_starts_ten_seconds_after_the_last_and_before_the_end() {
    let starts = [Some(5), Some(8), Some(10), None, Some(15), Some(100), Some(3591), Some(3590)];
    let tracklist: Vec<TracklistEntry> = starts.iter().map(|&s| entry(Some("A"), Some("B"), s)).collect();
    assert_eq!(chapters(&tracklist, 3600), [Some(0), None, Some(10), None, None, Some(100), None, Some(3590)]);
    assert_eq!(chapters(&tracklist[..1], 9), [None::<u32>], "the recording ends before its first ten seconds do");
    assert_eq!(chapters(&[entry(None, None, None), entry(None, None, None)], 3600), [None::<u32>, None]);
    assert!(chapters(&[], 3600).is_empty());
}

/// A description is a line a track, without `<` or `>`, saying which times it left off and when
/// there are too few chapters for YouTube to show them.
#[test]
fn a_description_says_what_it_left_off() {
    let tracklist = [
        entry(Some("A <B>"), Some("C"), Some(5)),
        entry(None, None, Some(8)),
        entry(Some("D"), Some("E"), Some(30)),
        entry(Some("F"), Some("G"), None),
        entry(Some("H"), Some("I"), Some(100)),
    ];
    let left_off = "left the time off 1 of the tracks: YouTube's chapters are in order, each at least 10 seconds long";
    assert_eq!(
        description(&tracklist, 3600),
        Description {
            text: "00:00 A B - C\nID - ID\n00:30 D - E\nF - G\n01:40 H - I".into(),
            tracks: 5,
            chapters: 3,
            notes: vec![left_off.into()],
        }
    );
    let few = "YouTube shows chapters once there are 3, and the description has 2";
    assert_eq!(description(&tracklist[..3], 3600).notes, [left_off, few]);
    let untimed = description(&tracklist[3..4], 3600);
    assert_eq!((untimed.text.as_str(), untimed.chapters, untimed.notes.len()), ("F - G", 0, 0));
    assert_eq!(description(&[], 3600), Description { text: String::new(), tracks: 0, chapters: 0, notes: vec![] });
}

/// A tracklist of up to `most` entries, each maybe named (some with what YouTube doesn't take),
/// maybe timed, in any order.
#[hegel::test_helper]
fn tracklist(tc: &TestCase, most: usize) -> Vec<TracklistEntry> {
    let names = ["Young Fathers", "\u{dc}n\u{ef}c\u{f8}d\u{e9} <Art\u{ef}st>", " ", "Two\nLines", &"Long ".repeat(80)]
        .map(String::from);
    let name = |tc: &TestCase| tc.draw(gs::optional(gs::sampled_from(names.to_vec())));
    let count = tc.draw(gs::integers::<usize>().max_value(most));
    (0..count)
        .map(|_| TracklistEntry {
            artist: name(tc),
            title: name(tc),
            start_seconds: tc.draw(gs::optional(gs::integers::<u32>().max_value(5 * 3600))),
            ..Default::default()
        })
        .collect()
}

/// A line's time in seconds, if it starts with one (`05:30`, `1:02:03`), and the rest.
fn timed(line: &str) -> (Option<u32>, &str) {
    let Some((time, rest)) = line.split_once(' ') else { return (None, line) };
    let fields: Option<Vec<u32>> = time.split(':').map(|f| f.parse().ok()).collect();
    match fields.as_deref() {
        Some([m, s]) => (Some(m * 60 + s), rest),
        Some([h, m, s]) => (Some(h * 3600 + m * 60 + s), rest),
        _ => (None, line),
    }
}

/// Whatever the recording and tracklist, the description is within what YouTube takes, and its
/// times are chapters YouTube makes: the first line at 00:00, then in order, each at least ten
/// seconds after the one before and before the recording's last ten seconds, each the track's own
/// start. Every track that could have been one is, and the notes say how many weren't; a
/// tracklist too long keeps the lines that fit, in order, and says how many more.
#[hegel::test(test_cases = 300)]
fn prop_the_description_is_chapters_youtube_takes(tc: TestCase) {
    let length = tc.draw(gs::integers::<u32>().max_value(6 * 3600));
    let tracklist = tracklist(&tc, 60);
    let d = description(&tracklist, length);
    assert!(!d.text.contains(['<', '>']), "{:?}", d.text);
    assert!(d.text.len() <= MAX_DESCRIPTION, "{}", d.text.len());

    let mut lines: Vec<&str> = d.text.lines().collect();
    let more = lines.last().and_then(|l| l.strip_prefix("+ ")).and_then(|l| l.strip_suffix(" more"));
    if let Some(more) = more {
        lines.pop();
        assert_eq!(more.parse::<usize>().unwrap(), tracklist.len() - lines.len());
        // The next line wouldn't have fit, with one fewer to say there are more of.
        let next = line(&tracklist[lines.len()], chapters(&tracklist, length)[lines.len()]).len();
        let fewer = format!("+ {} more", tracklist.len() - lines.len() - 1).len();
        assert!(d.text.len() - "+  more".len() - more.len() + next + 1 + fewer > MAX_DESCRIPTION);
    } else {
        assert_eq!(lines.len(), tracklist.len());
    }
    assert_eq!(d.tracks, lines.len());
    let any_timed = tracklist.iter().any(|e| e.start_seconds.is_some());
    let mut last: Option<u32> = None;
    let mut left_off = 0;
    for (i, (line, entry)) in lines.iter().zip(&tracklist).enumerate() {
        let (at, name) = timed(line);
        let plain = |s: &Option<String>| {
            let s: String = s.as_deref().unwrap_or_default().chars().filter(|c| !matches!(c, '<' | '>')).collect();
            let s = s.replace('\n', " ").trim().to_string();
            if s.is_empty() { "ID".into() } else { s }
        };
        assert_eq!(name, format!("{} - {}", plain(&entry.artist), plain(&entry.title)), "{line:?}");
        let start = if i == 0 && any_timed { Some(0) } else { entry.start_seconds };
        let could = start.filter(|&s| last.is_none_or(|l| s >= l + MIN_CHAPTER) && s + MIN_CHAPTER <= length);
        assert_eq!(at, could, "line {i}: {line:?}, after {last:?}, of {length} s");
        left_off += usize::from(start.is_some() && at.is_none());
        last = at.or(last);
    }
    let chapters = lines.iter().filter(|l| timed(l).0.is_some()).count();
    assert_eq!(chapters, d.chapters);
    let has = |start: &str| d.notes.iter().any(|n| n.starts_with(start));
    assert_eq!(has("YouTube shows chapters once"), (1..MIN_CHAPTERS).contains(&chapters), "{:?}", d.notes);
    assert_eq!(has("the description lists"), more.is_some());
    assert_eq!(has(&format!("left the time off {left_off} of the tracks: ")), left_off > 0, "{:?}", d.notes);
    assert_eq!(
        d.notes.len(),
        usize::from(more.is_some()) + usize::from(left_off > 0) + usize::from(has("YouTube shows"))
    );
}
