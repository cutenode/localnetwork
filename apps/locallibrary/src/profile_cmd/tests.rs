use hegel::generators as gs;
use hegel::{Generator, TestCase};

use super::*;

#[test]
fn sparklines_scale() {
    assert_eq!(spark(&[Some(0.0), None, Some(1.0)]), "▁ █");
    assert_eq!(spark(&[Some(2.0), Some(2.0)]), "▄▄");
    assert_eq!(spark(&[None]), "");
}

#[test]
fn text_fits_columns() {
    assert_eq!(fit("abc", 5), "abc  ");
    assert_eq!(fit("abcdef", 4), "abc…");
    assert_eq!(thousands(3886), "3,886");
    assert_eq!(thousands(12), "12");
    assert_eq!(thousands(1_234_567), "1,234,567");
}

#[test]
fn queries_pick_one_played_track() {
    let t = |name: &str| PlayedTrack {
        name: name.into(),
        artist: String::new(),
        title: String::new(),
        sets: 1,
        plays: 1,
        opened: 0,
        closed: 0,
        unnamed: false,
    };
    let played = [t("Bicep - Glue"), t("Bicep - Apricots"), t("Four Tet - Glue")];
    assert_eq!(pick(&played, "apricots").unwrap(), "Bicep - Apricots");
    assert_eq!(pick(&played, "bicep - glue").unwrap(), "Bicep - Glue");
    assert!(pick(&played, "glue").unwrap_err().to_string().contains("matches 2"));
    assert!(pick(&played, "nothing").is_err());
}

/// Names, some the same but for case and spacing, and words in several of them.
const NAMES: [&str; 6] = ["Bicep - Glue", "bicep -  glue", "Bicep - Apricots", "Four Tet - Glue", "Ünïcödé", "ID - ID"];

/// The one of `names` a query picks, as a person would: the one it spells (case and runs of spaces
/// aside), else the only one it's in; or how many it's in.
fn picked<'a>(names: &[&'a str], query: &str) -> Result<&'a str, usize> {
    let norm = |s: &str| s.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    let q = norm(query);
    if let Some(n) = names.iter().find(|n| norm(n) == q) {
        return Ok(n);
    }
    match names.iter().filter(|n| norm(n).contains(&q)).collect::<Vec<_>>()[..] {
        [one] => Ok(one),
        ref many => Err(many.len()),
    }
}

/// A query picks the played track (or the preference entry) it spells, else the only one with it
/// in; one in none or several is refused, saying how many and, for tracks, listing up to 8 of them.
#[hegel::test(test_cases = 200)]
fn a_query_picks_the_one_entry_it_names(tc: TestCase) {
    let names: Vec<&str> = tc.draw(gs::vecs(gs::sampled_from(&NAMES[..])).max_size(12));
    let query = tc.draw(gs::sampled_from(&["glue", "BICEP - GLUE", "bicep", " - ", "ünïcödé", "id", "nothing"][..]));
    let played: Vec<PlayedTrack> = names
        .iter()
        .map(|n| PlayedTrack {
            name: n.to_string(),
            artist: String::new(),
            title: String::new(),
            sets: 1,
            plays: 1,
            opened: 0,
            closed: 0,
            unnamed: false,
        })
        .collect();
    let list: Vec<String> = names.iter().map(|n| n.to_string()).collect();
    let said = format!("{query:?} among {names:?}");
    match picked(&names, query) {
        Ok(name) => {
            assert_eq!(pick(&played, query).unwrap(), name, "{said}");
            assert_eq!(find_entry(&list, query, "hidden").unwrap(), name, "{said}");
        }
        Err(0) => {
            assert!(pick(&played, query).unwrap_err().to_string().contains("--exact"), "{said}");
            assert_eq!(
                find_entry(&list, query, "hidden").unwrap_err().to_string(),
                format!("nothing hidden matches {query:?}")
            );
        }
        Err(n) => {
            let e = pick(&played, query).unwrap_err().to_string();
            assert!(e.starts_with(&format!("{query:?} matches {n} tracks; be more specific:\n")), "{e}: {said}");
            assert_eq!(e.lines().filter(|l| l.starts_with("  ") && !l.starts_with("  ...")).count(), n.min(8), "{e}");
            assert_eq!(e.contains(&format!("  ... and {} more", n.saturating_sub(8))), n > 8, "{e}");
            let e = find_entry(&list, query, "hidden").unwrap_err().to_string();
            assert_eq!(e, format!("{query:?} matches {n} hidden entries; be more specific"), "{said}");
        }
    }
}

/// A sparkline has a block per value (a space where there's none) scaled from the lowest block to
/// the highest, the middle one when they're all the same; with no values known it's empty.
#[hegel::test(test_cases = 200)]
fn a_sparkline_has_a_block_per_value(tc: TestCase) {
    let values: Vec<Option<f64>> =
        tc.draw(gs::vecs(gs::optional(gs::integers::<i32>().min_value(-50).max_value(50).map(f64::from))).max_size(10));
    let s: Vec<char> = spark(&values).chars().collect();
    let known: Vec<f64> = values.iter().flatten().copied().collect();
    if known.is_empty() {
        assert!(s.is_empty(), "{values:?}");
        return;
    }
    assert_eq!(s.len(), values.len(), "{values:?}");
    let (lo, hi) = known.iter().fold((f64::MAX, f64::MIN), |(l, h), x| (l.min(*x), h.max(*x)));
    for (v, c) in values.iter().zip(&s) {
        match v {
            None => assert_eq!(*c, ' '),
            Some(_) if lo == hi => assert_eq!(*c, BLOCKS[3]),
            Some(x) if *x == lo => assert_eq!(*c, BLOCKS[0]),
            Some(x) if *x == hi => assert_eq!(*c, BLOCKS[7]),
            Some(_) => assert!(BLOCKS.contains(c), "{c:?}"),
        }
    }
}
