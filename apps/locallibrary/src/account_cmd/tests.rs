use super::*;

#[test]
fn the_wait_shown_is_the_wait_used() {
    let asked = |timeout| BrowserArgs { no_browser: false, paste: false, timeout }.wait();
    assert_eq!(atmosphere::span(asked(90)), "90 seconds");
    assert_eq!(atmosphere::span(asked(2 * 60 * 60)), "60 minutes");
    assert_eq!(asked(u64::MAX), atmosphere::LONGEST_LOGIN_WAIT);
}
