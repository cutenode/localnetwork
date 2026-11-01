use chrono::NaiveDate;
use hegel::TestCase;
use hegel::generators as gs;
use serde_json::json;

use super::*;

/// `--move FROM:TO` and `--insert AT:TRACK` read the numbers as written, spaces around them
/// or not, and the track as typed after the first colon; anything else is refused, naming what
/// was given.
#[hegel::test(test_cases = 200)]
fn moves_and_inserts_read_as_written(tc: TestCase) {
    let (from, to) = (tc.draw(gs::integers::<usize>()), tc.draw(gs::integers::<usize>()));
    let pad = |tc: &TestCase| tc.draw(gs::sampled_from(&["", " ", "  "][..]));
    let written = format!("{}{from}{}:{}{to}{}", pad(&tc), pad(&tc), pad(&tc), pad(&tc));
    let m = a_move(&written).unwrap();
    assert_eq!((m.from, m.to), (from, to), "{written:?}");
    let track: String = tc.draw(gs::text().max_size(20));
    let i = an_insert(&format!("{from}:{track}")).unwrap();
    assert_eq!((i.at, i.track), (from, track));

    let junk: String = tc.draw(gs::text().alphabet("ab-:. 9").max_size(6));
    let numbers = junk
        .split_once(':')
        .is_some_and(|(a, b)| a.trim().parse::<usize>().is_ok() && b.trim().parse::<usize>().is_ok());
    match a_move(&junk) {
        Ok(m) => assert!(numbers, "{junk:?} read as {}:{}", m.from, m.to),
        Err(e) => assert!(!numbers && e.contains(&format!("{junk:?}")), "{junk:?}: {e}"),
    }
    let at = junk.split_once(':').is_some_and(|(a, _)| a.trim().parse::<usize>().is_ok());
    match an_insert(&junk) {
        Ok(i) => assert!(at && junk.ends_with(&i.track), "{junk:?}"),
        Err(e) => assert!(!at && e.contains(&format!("{junk:?}")), "{junk:?}: {e}"),
    }
}

/// An entry's marks say it's unnamed when it is, and where it came from unless that's the
/// session itself; nothing to say is nothing at all. (Where a track was played is listed for
/// sets titled or not, at a time or not, alongside.)
#[hegel::test(test_cases = 50)]
fn marks_say_what_sets_an_entry_apart(tc: TestCase) {
    let mut e: SetEntry = serde_json::from_value(json!({"title": "T", "artist": "A", "origin": "session"})).unwrap();
    e.unnamed = tc.draw(gs::booleans());
    e.origin = tc.draw(gs::sampled_from(&[EntryOrigin::Session, EntryOrigin::Added, EntryOrigin::Recording][..]));
    let m = marks(&e);
    assert_eq!(m.contains("unnamed"), e.unnamed, "{m:?}");
    assert_eq!(m.contains("added"), e.origin == EntryOrigin::Added, "{m:?}");
    assert_eq!(m.contains("from the recording"), e.origin == EntryOrigin::Recording, "{m:?}");
    assert_eq!(m.is_empty(), !e.unnamed && e.origin == EntryOrigin::Session, "{m:?}");

    let found: Vec<Appearance> = (0..tc.draw(gs::integers::<usize>().max_value(3)))
        .map(|i| Appearance {
            set: format!("s{i}"),
            date: NaiveDate::from_ymd_opt(2026, 10, 1 + i as u32).unwrap(),
            title: tc.draw(gs::booleans()).then(|| "Friday".to_string()),
            venue: tc.draw(gs::booleans()).then(|| "Club".to_string()),
            position: i + 1,
            start_seconds: tc.draw(gs::booleans()).then_some(600),
            track: e.name(),
        })
        .collect();
    played("Title", &found);
}

/// Positions (`3`, `1-2`, `1-2,30`, with spaces around any number) read as every position they
/// name, in the order written. A part that isn't a number or a range of them is refused quoting
/// the whole list, and a range that runs backwards is refused quoting the range.
#[hegel::test(test_cases = 200)]
fn positions_read_as_every_one_they_name(tc: TestCase) {
    let pad = |tc: &TestCase| tc.draw(gs::sampled_from(&["", " "][..]));
    let (mut parts, mut want) = (Vec::new(), Vec::new());
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        let from = tc.draw(gs::integers::<usize>().max_value(40));
        let span = tc.draw(gs::integers::<usize>().max_value(3));
        let (before, after) = (pad(&tc), pad(&tc));
        parts.push(match span {
            0 => format!("{before}{from}{after}"),
            _ => format!("{before}{from}{after}-{before}{}{after}", from + span),
        });
        want.extend(from..=from + span);
    }
    let list = parts.join(",");
    assert_eq!(positions(&list).map(|p| p.0), Ok(want), "{list:?}");

    let at = tc.draw(gs::integers::<usize>().max_value(parts.len() - 1));
    let backwards = tc.draw(gs::booleans());
    parts[at] = match backwards {
        true => "9-3".to_string(),
        false => tc.draw(gs::sampled_from(&["x", "", "4-", "-4", "2x", "1-2-3", "one-2"][..])).to_string(),
    };
    let list = parts.join(",");
    let Err(refused) = positions(&list) else { panic!("{list:?} read") };
    match backwards {
        true => assert_eq!(refused, "\"9-3\" runs backwards: give it like 1-2"),
        false => assert!(refused.starts_with(&format!("{list:?} isn't a list of positions")), "{refused}"),
    }
}
