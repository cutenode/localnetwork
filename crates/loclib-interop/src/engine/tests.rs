use super::*;

#[test]
fn keys_round_the_wheel() {
    assert_eq!(key_number("C"), Some(0));
    assert_eq!(key_number("Am"), Some(1));
    assert_eq!(key_number("G"), Some(2));
    assert_eq!(key_number("Em"), Some(3));
    assert_eq!(key_number("7B"), Some(22), "F major");
    assert_eq!(key_number("Dm"), Some(23));
    assert_eq!(key_number("Fm"), Some(17));
    assert_eq!(key_number(""), None);
}

#[test]
fn dates() {
    assert_eq!(unix_date("1970-01-02"), Some(86_400));
    assert_eq!(unix_date("2026-09-30 10:00"), Some(1_790_726_400));
    assert_eq!(unix_date("soon"), None);
}
