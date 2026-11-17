use hegel::TestCase;
use hegel::generators as gs;

use super::*;

fn entry(artist: Option<&str>, title: Option<&str>, start_seconds: Option<u32>) -> TracklistEntry {
    TracklistEntry {
        artist: artist.map(String::from),
        title: title.map(String::from),
        start_seconds,
        ..Default::default()
    }
}

fn set(title: Option<&str>, venue: Option<&str>, city: Option<&str>) -> PublishedSet {
    let given = |s: Option<&str>| s.map(String::from);
    PublishedSet {
        id: "s1".into(),
        date: "2026-10-03".into(),
        title: given(title),
        venue: given(venue),
        city: given(city),
        ..PublishedSet::default()
    }
}

/// A line is the track's time (`H:MM:SS` past an hour, none for a track not placed), artist and
/// title, each on one line, `ID` for one unnamed; a chapter's time stands in for the track's own.
#[test]
fn a_line_says_when_and_what() {
    let tracklist = [
        entry(Some("Young Fathers"), Some("In My View"), Some(0)),
        entry(None, Some(" "), Some(330)),
        entry(Some("Chris Lake"), Some("Turn Off the Lights"), Some(3723)),
        entry(Some("Someone"), Some("Unplaced"), None),
        entry(Some("Two\nLines"), Some("\tTabbed\r\n"), Some(59)),
    ];
    let lines: Vec<String> = tracklist.iter().map(TracklistEntry::line).collect();
    assert_eq!(
        lines,
        [
            "00:00 Young Fathers - In My View",
            "05:30 ID - ID",
            "1:02:03 Chris Lake - Turn Off the Lights",
            "Someone - Unplaced",
            "00:59 Two Lines - Tabbed"
        ]
    );
    assert_eq!(tracklist[1].line_at(Some(3600)), "1:00:00 ID - ID");
    assert_eq!(tracklist[2].line_at(None), "Chris Lake - Turn Off the Lights");
}

/// Any entry's line has a time exactly when it's given one, `MM:SS` before an hour and `H:MM:SS`
/// after, reading back as it; then its artist and title on one line, trimmed, `ID` for one
/// unnamed.
#[hegel::test(test_cases = 100)]
fn prop_a_line_is_its_time_and_names(tc: TestCase) {
    let names = vec![" Young Fathers ", "\u{dc}n\u{ef}c\u{f8}d\u{e9}", " ", "", "Two\nLines", "\u{7}\u{1b}[0m\u{85}"];
    let name = |tc: &TestCase| tc.draw(gs::optional(gs::sampled_from(names.clone())));
    // Where a field turns over, as often as anywhere.
    let secs = match tc.draw(gs::booleans()) {
        true => tc.draw(gs::sampled_from(vec![0, 59, 60, 3599, 3600, 3601, 35999, 36000, u32::MAX])),
        false => tc.draw(gs::integers::<u32>()),
    };
    let at = tc.draw(gs::optional(gs::just(secs)));
    let (artist, title) = (name(&tc), name(&tc));
    let e = TracklistEntry {
        artist: artist.map(String::from),
        title: title.map(String::from),
        start_seconds: at,
        ..Default::default()
    };
    let line = e.line();
    assert_eq!(line, e.line_at(at));
    assert!(!line.contains(char::is_control), "{line:?}");

    let one_line = |s: Option<&str>| {
        let s: String = s.unwrap_or_default().chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        if s.trim().is_empty() { "ID".into() } else { s.trim().to_string() }
    };
    let name = format!("{} - {}", one_line(artist), one_line(title));
    let Some(secs) = at else {
        assert_eq!(line, name);
        return;
    };
    let (time, rest) = line.split_once(' ').unwrap();
    assert_eq!(rest, name);
    let fields: Vec<u64> = time.split(':').map(|f| f.parse().unwrap()).collect();
    assert!(time.split(':').skip(1).all(|f| f.len() == 2), "{line}");
    match fields[..] {
        [m, s] => assert_eq!((secs < 3600, time.len(), m * 60 + s), (true, 5, u64::from(secs)), "{line}"),
        [h, m, s] => assert_eq!((secs >= 3600, h * 3600 + m * 60 + s), (true, u64::from(secs)), "{line}"),
        _ => panic!("{line}"),
    }
}

/// A set is named by its title, else where (its venue, else its city, else "DJ set") and when,
/// each on one line.
#[test]
fn a_set_is_named_by_its_title_else_where_and_when() {
    assert_eq!(set(Some(" Closing set\n"), Some("Warehouse"), None).name(), "Closing set");
    assert_eq!(set(Some("Two\nLines"), None, None).name(), "Two Lines");
    assert_eq!(set(Some(" "), Some("Warehouse"), Some("Leeds")).name(), "Warehouse, 2026-10-03");
    assert_eq!(set(None, Some("\t"), Some("Leeds")).name(), "Leeds, 2026-10-03");
    assert_eq!(set(None, None, None).name(), "DJ set, 2026-10-03");
    assert_eq!(set(None, Some("Club\u{0}X"), None).place().as_deref(), Some("Club X"));
    assert_eq!(set(None, Some(""), Some("")).place(), None);
}

/// Whatever its title, venue, city and date, a set's name is never empty and on one line: its
/// title, trimmed, when it has one, else its place (else "DJ set") and date.
#[hegel::test(test_cases = 100)]
fn prop_a_sets_name_is_never_empty(tc: TestCase) {
    let words = || gs::optional(gs::sampled_from(vec!["Warehouse", " ", "", "Two\nLines", "\u{1b}", " Le\u{7}eds "]));
    let (title, venue, city) = (tc.draw(words()), tc.draw(words()), tc.draw(words()));
    let date = tc.draw(gs::sampled_from(vec!["2026-10-03", "", "\n2026-10-03"]));
    let set = PublishedSet { date: date.into(), ..set(title, venue, city) };
    let name = set.name();
    assert!(!name.is_empty() && !name.contains(char::is_control), "{name:?}");
    let one_line = |s: &str| {
        let s: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        s.trim().to_string()
    };
    let given = |s: Option<&str>| s.map(one_line).filter(|s| !s.is_empty());
    let place = given(venue).or_else(|| given(city)).unwrap_or_else(|| "DJ set".into());
    assert_eq!(name, given(title).unwrap_or_else(|| format!("{place}, {}", one_line(date))), "{set:?}");
}

/// A tracklist too long for its room keeps the lines that fit and says how many more there are,
/// measured in bytes or characters as it's asked.
#[test]
fn lines_that_dont_fit_are_counted() {
    let lines: Vec<String> = ["\u{3a9}".repeat(4), "\u{3a9}".repeat(4), "abc".into()].into();
    assert_eq!(fit_lines(&lines, 18, |l: &str| l.chars().count()), (3, lines.join("\n")));
    assert_eq!(fit_lines(&lines, 18, str::len), (1, format!("{}\n+ 2 more", lines[0])));
    assert_eq!(fit_lines(&lines, 3, str::len), (0, "+ 3 more".into()));
    assert_eq!(fit_lines(&[], 0, str::len), (0, String::new()));
}

/// Whatever the lines, and room for at least the line saying how many more there are, what's kept
/// is within the room: the first lines, in order, then how many more there are, and the next line
/// wouldn't have fit beside one fewer to say there are more of.
#[hegel::test(test_cases = 100)]
fn prop_the_lines_that_fit_are_kept(tc: TestCase) {
    let count = tc.draw(gs::integers::<usize>().max_value(30));
    let length = |tc: &TestCase| tc.draw(gs::integers::<usize>().max_value(40));
    let lines: Vec<String> = (0..count).map(|_| "\u{3a9}".repeat(length(&tc))).collect();
    let room = tc.draw(gs::integers::<usize>().min_value(10).max_value(800));
    let chars = |l: &str| l.chars().count();
    let (kept, text) = fit_lines(&lines, room, chars);
    if kept == lines.len() {
        assert_eq!(text, lines.join("\n"));
        return;
    }
    assert!(chars(&lines.join("\n")) > room);
    assert!(chars(&text) <= room, "{} > {room}", chars(&text));
    let more = format!("+ {} more", lines.len() - kept);
    assert_eq!(text, lines[..kept].iter().map(|l| format!("{l}\n")).collect::<String>() + &more);
    let fewer = format!("+ {} more", lines.len() - kept - 1);
    assert!(chars(&text) - more.len() + chars(&lines[kept]) + 1 + fewer.len() > room);
}

/// A time reads as `MM:SS`, or past an hour `H:MM:SS`.
#[test]
fn a_clock_reads_minutes_then_hours() {
    let read = [0, 59, 60, 330, 3599, 3600, 3723, 36000].map(clock);
    assert_eq!(read, ["00:00", "00:59", "01:00", "05:30", "59:59", "1:00:00", "1:02:03", "10:00:00"]);
}
