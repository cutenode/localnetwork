use super::*;

#[test]
fn paths_split_at_the_first_slash() {
    assert_eq!(split_path("Genre/Deep House").unwrap(), ("Genre", Some("Deep House")));
    assert_eq!(split_path(" Genre / Drum/Bass ").unwrap(), ("Genre", Some("Drum/Bass")));
    assert_eq!(split_path("Situation").unwrap(), ("Situation", None));
    assert!(split_path("/Deep").is_err());
    assert!(split_path("Genre/").is_err());
}

#[test]
fn names_match_exactly_then_ignoring_case() {
    let names = ["Deep", "deep", "House"];
    assert_eq!(find_named(&names, |n| n, "deep"), Some(&"deep"));
    assert_eq!(find_named(&names, |n| n, "DEEP"), None, "ambiguous");
    assert_eq!(find_named(&names, |n| n, "house"), Some(&"House"));
}
