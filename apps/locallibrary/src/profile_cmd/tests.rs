use super::*;

#[test]
fn bars_and_sparklines_scale() {
    assert_eq!(bar(1.0, 4), "████");
    assert_eq!(bar(0.5, 4), "██");
    assert_eq!(bar(0.0, 4), "");
    assert_eq!(bar(1.0 / 32.0, 4), "▏");
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
