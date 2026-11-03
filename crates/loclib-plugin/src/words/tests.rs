use super::a_or_an;

#[test]
fn an_goes_before_a_vowel_letter_and_a_before_anything_else() {
    let said = ["archive", "Archive", "integer", "Unknown", "plyr", "Mock", "4-track", ""].map(a_or_an);
    assert_eq!(said, ["an archive", "an Archive", "an integer", "an Unknown", "a plyr", "a Mock", "a 4-track", "a "]);
}
