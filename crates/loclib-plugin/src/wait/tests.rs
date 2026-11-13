use hegel::TestCase;
use hegel::generators as gs;
use indexmap::IndexMap;
use loclib_progress::{CancelToken, NoProgress};

use super::*;

/// An answer saying `value` in its header `name`.
fn answer(name: &str, value: &str) -> Fetched {
    let headers: IndexMap<String, String> = [(name.to_string(), value.to_string())].into_iter().collect();
    Fetched { status: 503, headers, body: None, blob: None }
}

/// `Retry-After` is a number of seconds (one too big to count longer than any wait) or an
/// HTTP-date, counted from now and rounded up; one past, missing or unreadable says nothing.
#[test]
fn retry_after_says_seconds_or_a_date() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_millis(784_111_770_500);
    let said = |name: &str, value: &str| answer(name, value).retry_after(now);
    assert_eq!(said("retry-after", " 12 "), Some(12));
    assert_eq!(said("Retry-After", "0"), Some(0));
    assert_eq!(said("retry-after", "99999999999999999999"), Some(u64::MAX));
    // 784111777 is Sun, 06 Nov 1994 08:49:37 GMT: 6.5 seconds after `now`.
    assert_eq!(said("retry-after", "Sun, 06 Nov 1994 08:49:37 GMT"), Some(7));
    assert_eq!(said("retry-after", "Sun, 06 Nov 1994 08:49:30 GMT"), None, "past");
    assert_eq!(said("retry-after", "Fri, 31 Dec 9999 23:59:59 GMT"), Some(253_402_300_799 - 784_111_770));
    let unread = [said("retry-after", "soon"), said("retry-after", "-5"), said("retry-after", ""), said("x-wait", "9")];
    assert_eq!(unread, [None; 4]);
}

/// Any `Retry-After` comes to its seconds: a number as it is (one too big to count longer than
/// any wait), an HTTP-date as the seconds until then, rounded up (none once it's past), and
/// anything else, or another header, as none.
#[hegel::test(test_cases = 100)]
fn prop_any_retry_after_is_its_seconds(tc: TestCase) {
    let base = tc.draw(gs::integers::<u64>().min_value(1_000_000_000).max_value(4_000_000_000));
    let millis = tc.draw(gs::integers::<u64>().max_value(999));
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(base) + Duration::from_millis(millis);
    let n = tc.draw(gs::integers::<u64>().max_value(1_000_000_000));
    let date = |secs: u64| httpdate::fmt_http_date(SystemTime::UNIX_EPOCH + Duration::from_secs(secs));
    let secs = tc.draw(gs::integers::<u64>());
    // Ten times this is more than a u64 holds.
    let big = tc.draw(gs::integers::<u64>().min_value(u64::MAX / 10 + 1));
    let (said, seconds) = tc.draw(gs::sampled_from(vec![
        (secs.to_string(), Some(secs)),
        (format!("{big}0"), Some(u64::MAX)),
        // `n` seconds and a part of one after `now` (`n + 1` when it's on a second): `n + 1` rounded up.
        (date(base + 1 + n), Some(n + 1)),
        (date(base - 1 - n.min(base - 1)), None),
        ("in a minute".to_string(), None),
    ]));
    let name = tc.draw(gs::sampled_from(vec!["retry-after", "Retry-After", "RETRY-AFTER", "x-retry-after"]));
    let seconds = seconds.filter(|_| name != "x-retry-after");
    assert_eq!(answer(name, &said).retry_after(now), seconds, "{name}: {said}");
}

/// Any short wait lasts at least as long as it's asked to, unless it's cancelled, which ends it
/// at once.
#[hegel::test(test_cases = 20)]
fn prop_a_wait_lasts_until_its_cancelled(tc: TestCase) {
    let wait = Duration::from_millis(tc.draw(gs::integers::<u64>().max_value(60)));
    let token = CancelToken::new();
    let cancelled = tc.draw(gs::booleans());
    if cancelled {
        token.cancel();
    }
    let started = Instant::now();
    let waited = pause(&Ctx::new(&NoProgress, token), wait);
    match cancelled {
        true => assert_eq!(waited, Err(RpcError::cancelled())),
        false => assert_eq!((waited, started.elapsed() >= wait), (Ok(()), true)),
    }
}

/// A wait stops as soon as it's cancelled (here, before it starts), and otherwise lasts as long
/// as it's asked to.
#[test]
fn a_wait_ends_when_its_cancelled() {
    let token = CancelToken::new();
    token.cancel();
    let started = Instant::now();
    let cancelled = pause(&Ctx::new(&NoProgress, token), Duration::from_secs(30));
    assert_eq!((cancelled, started.elapsed() < Duration::from_secs(5)), (Err(RpcError::cancelled()), true));
    let started = Instant::now();
    assert_eq!(pause(&Ctx::new(&NoProgress, CancelToken::new()), Duration::from_millis(120)), Ok(()));
    assert!(started.elapsed() >= Duration::from_millis(120), "{:?}", started.elapsed());
}
