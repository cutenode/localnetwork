use super::*;
use crate::test_util::track;

/// Every colour reads from its number and its name in any case, 0 clears it, and any other
/// number or word says why it isn't one.
#[hegel::test(test_cases = 50)]
fn prop_a_colour_parses_from_its_name_or_number(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let n = tc.draw(gs::integers::<u8>());
    let parsed = TrackColor::parse(&format!(" {n} "));
    match TrackColor::from_id(n) {
        Some(c) => {
            assert_eq!(parsed, Ok(Some(c)));
            assert_eq!(TrackColor::parse(&format!("{c:?}").to_uppercase()), Ok(Some(c)));
        }
        None if n == 0 => assert_eq!(parsed, Ok(None)),
        None => assert_eq!(parsed, Err(format!("no colour number {n}"))),
    }
    assert_eq!(TrackColor::parse("none"), Ok(None));
    assert!(TrackColor::parse("teal").unwrap_err().starts_with("unknown colour \"teal\""));
}

/// Fields left out stay as they were, a blank album takes its artist with it, and the BPM is kept
/// to hundredths.
#[test]
fn an_edit_changes_only_the_fields_it_gives() {
    let mut t = track(1, "Artist", "Title", "/Contents/a.mp3");
    (t.album, t.album_artist) = ("Old".into(), "Someone".into());
    let f =
        TrackFields { album: Some(" ".into()), color: Some("aqua".into()), bpm: Some(128.004), ..Default::default() };
    f.apply_to(&mut t);
    assert_eq!(
        (t.title.as_str(), t.artist.as_str(), t.album.as_str(), t.album_artist.as_str()),
        ("Title", "Artist", "", "")
    );
    assert_eq!((t.color, t.bpm), (Some(TrackColor::Aqua), 128.0));
    assert_eq!(f.describe(), r#"album " ", colour "aqua", BPM 128"#);
    assert!(!f.is_empty() && TrackFields::default().is_empty());
    let bad = TrackFields { color: Some("9".into()), ..Default::default() };
    assert_eq!(bad.color_value(), Err("no colour number 9".to_string()));
}
