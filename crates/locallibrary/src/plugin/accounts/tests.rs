use hegel::TestCase;
use hegel::generators as gs;
use serde_json::json;

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
        refresh_expires_at: None,
        scopes: Vec::new(),
    };
    let refreshed = kept.expiring(now);
    assert_eq!(refreshed, left.is_some_and(|s| s <= REFRESH_BEFORE.as_secs() as i64), "{left:?}");
}

/// An account signing itself out is warned about within `WARN_DAYS` of it, or once it has.
#[hegel::test(test_cases = 300)]
fn an_account_is_due_soon_within_warn_days(tc: TestCase) {
    let day = 86_400;
    let offset = tc.draw(gs::integers::<i64>().min_value(-40 * day).max_value(40 * day));
    // Checked a moment after: far enough from the boundary not to cross it.
    tc.assume((offset - WARN_DAYS * day).abs() > 5);
    assert_eq!(due_soon(Utc::now() + chrono::Duration::seconds(offset)), offset <= WARN_DAYS * day, "{offset}");
}

/// For any token endpoint answer, saying how long its tokens last or not, a refresh token or not,
/// and the tokens it had before or not: the access token expires and the refresh token runs out
/// when the answer says; a refresh token it leaves out is the one it had, running out when that
/// did, and a new one runs out only when the answer says.
#[hegel::test(test_cases = 200)]
fn tokens_last_as_the_service_says(tc: TestCase) {
    let lasting = || tc.draw(gs::optional(gs::integers::<u64>().min_value(60).max_value(400 * 86_400)));
    let (expires_in, refresh_lasts) = (lasting(), lasting());
    let refresh = tc.draw(gs::booleans());
    let answer = json!({
        "access_token": "at-2",
        "token_type": "bearer",
        "expires_in": expires_in,
        "refresh_token": refresh.then_some("rt-2"),
        "refresh_token_expires_in": refresh_lasts,
    });
    let answer: Answer = serde_json::from_value(answer).unwrap();
    let before = Utc::now() + chrono::Duration::days(3);
    let had = tc.draw(gs::booleans()).then(|| Tokens {
        access_token: "at-1".into(),
        refresh_token: Some("rt-1".into()),
        expires_at: None,
        refresh_expires_at: Some(before),
        scopes: Vec::new(),
    });
    let kept_refresh = had.is_some();
    let now = Utc::now();
    let t = Tokens::from(&answer, had);
    // When `secs` from about now is.
    let about = |at: Option<DateTime<Utc>>, secs: Option<u64>| match (at, secs) {
        (Some(at), Some(s)) => (at - now - chrono::Duration::seconds(s as i64)).num_seconds().abs() < 5,
        (at, s) => at.is_none() && s.is_none(),
    };
    assert!(about(t.expires_at, expires_in), "{:?} {expires_in:?}", t.expires_at);
    match (refresh, refresh_lasts) {
        (false, None) if kept_refresh => {
            assert_eq!((t.refresh_token.as_deref(), t.refresh_expires_at), (Some("rt-1"), Some(before)));
        }
        _ => assert!(about(t.refresh_expires_at, refresh_lasts), "{:?} {refresh_lasts:?}", t.refresh_expires_at),
    }
}
