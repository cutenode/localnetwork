use hegel::TestCase;
use hegel::generators as gs;

use super::*;

/// A token is refreshed before it's used once it expires within `REFRESH_BEFORE`, or has, and
/// never when the service didn't say when it expires.
#[hegel::test(test_cases = 100)]
fn a_token_is_refreshed_once_it_expires_within_a_minute(tc: TestCase) {
    let now = Utc::now();
    let left = tc.draw(gs::optional(gs::integers::<i64>().min_value(-86_400).max_value(86_400)));
    let kept = Tokens {
        access_token: "a".into(),
        refresh_token: None,
        expires_at: left.map(|s| now + chrono::Duration::seconds(s)),
        scopes: Vec::new(),
    };
    let refreshed = kept.expiring(now);
    assert_eq!(refreshed, left.is_some_and(|s| s <= REFRESH_BEFORE.as_secs() as i64), "{left:?}");
}
