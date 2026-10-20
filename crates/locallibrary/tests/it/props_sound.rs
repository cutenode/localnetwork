//! How sure a stored key is when an analysis disagrees: a key the analysis confirms needs no
//! doubt; one a neighbour on the Camelot wheel away (a fifth, the relative or the parallel key),
//! even odds; any other, a quarter; a key that isn't one, nothing to weigh.

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::harmony::Camelot;
use locallibrary::sound;

#[hegel::test(test_cases = 400)]
fn a_disagreeing_analysis_lowers_trust_in_a_key_by_how_far_it_is(tc: TestCase) {
    let (n, minor) = (tc.draw(gs::integers::<u8>().min_value(1).max_value(12)), tc.draw(gs::booleans()));
    let (m, other_minor) = (tc.draw(gs::integers::<u8>().min_value(1).max_value(12)), tc.draw(gs::booleans()));
    let code = |n: u8, minor: bool| format!("{n}{}", if minor { 'A' } else { 'B' });
    let heard = Camelot::parse(&code(m, other_minor)).unwrap();
    let step = |a: u8, b: u8| (i16::from(b) - i16::from(a)).rem_euclid(12);
    let fifth = minor == other_minor && matches!(step(n, m), 1 | 11);
    let relative = minor != other_minor && n == m;
    // Am (8A) and A major (11B): the major key three steps round from its parallel minor.
    let parallel = minor != other_minor && step(n, m) == if minor { 3 } else { 9 };
    let want = match (n == m && minor == other_minor, fifth || relative || parallel) {
        (true, _) => None,
        (false, true) => Some(0.5),
        (false, false) => Some(0.25),
    };
    assert_eq!(
        sound::key_confidence(&code(n, minor), heard),
        want,
        "{} against {}",
        code(n, minor),
        code(m, other_minor)
    );
    let not_a_key = tc.draw(gs::sampled_from(vec!["", "13A", "x", "8C"]));
    assert_eq!(sound::key_confidence(not_a_key, heard), None);
}
