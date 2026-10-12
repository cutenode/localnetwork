use super::*;
use hegel::TestCase;
use hegel::generators as gs;

/// Tokens compare equal exactly when they are, whatever their lengths and contents.
#[hegel::test(test_cases = 500)]
fn tokens_compare_as_strings(tc: TestCase) {
    let a: String = tc.draw(gs::from_regex("[0-9a-f]{0,34}").fullmatch(true));
    let b: String = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => a.clone(),
        1 => tc.draw(gs::from_regex("[0-9a-f]{0,34}").fullmatch(true)),
        _ => {
            let mut b = a.clone().into_bytes();
            if let Some(c) = b.last_mut() {
                *c = if *c == b'0' { b'1' } else { b'0' };
            }
            String::from_utf8(b).unwrap()
        }
    };
    assert_eq!(same(&a, &b), a == b, "{a:?} {b:?}");
    assert!(same(&a, &a));
}
