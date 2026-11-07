use super::a_or_an;

#[test]
fn an_goes_before_a_vowel_letter_and_a_before_anything_else() {
    let said = ["archive", "Archive", "integer", "Unknown", "plyr", "Mock", "4-track", ""].map(a_or_an);
    assert_eq!(said, ["an archive", "an Archive", "an integer", "an Unknown", "a plyr", "a Mock", "a 4-track", "a "]);
}

/// Any word comes after "an" exactly when its first letter is a vowel, and after "a" otherwise
/// (no word at all too).
#[hegel::test(test_cases = 100)]
fn prop_an_goes_before_a_vowel_letter_and_a_before_anything_else(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let first = tc.draw(gs::sampled_from(vec!["", "a", "E", "u", "b", "Z", "4", "é", " "]));
    let word = format!("{first}{}", tc.draw(gs::text().max_size(6)));
    let vowel = word.chars().next().is_some_and(|c| "aeiouAEIOU".contains(c));
    let article = if vowel { "an" } else { "a" };
    assert_eq!(a_or_an(&word), format!("{article} {word}"));
}
