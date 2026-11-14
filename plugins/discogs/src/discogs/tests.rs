use loclib_progress::{CancelToken, NoProgress};

use super::*;

/// `Retry-After` is a number of seconds (one too big to count longer than any wait); an HTTP
/// date, or none, says nothing, and the search waits a whole window.
#[test]
fn retry_after_says_seconds() {
    let said = |value: &str| {
        let headers = IndexMap::from([("retry-after".to_string(), value.to_string())]);
        retry_after(&Fetched { status: 429, headers, body: None, blob: None })
    };
    assert_eq!(said(" 30 "), Some(30));
    assert_eq!(said("0"), Some(0));
    assert_eq!(said("99999999999999999999999"), Some(u64::MAX));
    assert_eq!(said("Wed, 21 Oct 2026 07:28:00 GMT"), None);
    assert_eq!(said("-1"), None);
    assert_eq!(said(""), None);
    assert_eq!(retry_after(&Fetched { status: 429, headers: IndexMap::new(), body: None, blob: None }), None);
}

/// A wait ends once it's out, or at once when the hook is cancelled, however long it was.
#[test]
fn a_wait_stops_once_cancelled() {
    let began = Instant::now();
    assert_eq!(pause(&Ctx::new(&NoProgress, CancelToken::new()), Duration::from_millis(20)), Ok(()));
    assert!(began.elapsed() >= Duration::from_millis(20));
    let token = CancelToken::new();
    token.cancel();
    assert_eq!(pause(&Ctx::new(&NoProgress, token), Duration::from_secs(3600)), Err(RpcError::cancelled()));
}

/// A label's name loses only the number Discogs tells apart labels of one name with.
#[test]
fn labels_lose_discogs_numbers() {
    assert_eq!(plain_label(" Warp (2) "), "Warp");
    assert_eq!(plain_label("Ninja Tune"), "Ninja Tune");
    assert_eq!(plain_label("Not On Label (Bicep Self-released)"), "Not On Label (Bicep Self-released)");
    assert_eq!(plain_label("Label ()"), "Label ()");
    assert_eq!(plain_label("(2)"), "(2)");
}

/// A release's title is what follows the artist in a search's `Artist - Title`, and titles match
/// as tags spell them.
#[test]
fn titles_match_as_tags_spell_them() {
    assert_eq!(title_of("Bicep - Isles"), "Isles");
    assert_eq!(title_of("Bicep - Bicep - Remixes"), "Bicep - Remixes");
    assert_eq!(title_of("Untitled"), "Untitled");
    assert!(same("Don\u{2019}t  Stop", " don't stop"));
    assert!(same("Re\u{2010}Edit", "re-edit"));
    assert!(!same("Isles", "Isles (Remixes)"));
}

/// A release's line leaves out what it lacks.
#[test]
fn a_release_is_shown_with_what_it_has() {
    let release = |label: Option<&str>, catno: Option<&str>, year: Option<u16>| Release {
        id: 1,
        album: "Isles".into(),
        year,
        label: label.map(String::from),
        catno: catno.map(String::from),
        genre: None,
        cover: None,
    };
    assert_eq!(release(Some("Ninja Tune"), Some("ZEN275"), Some(2021)).to_string(), "Isles, Ninja Tune ZEN275, 2021");
    assert_eq!(release(None, Some("ZEN275"), None).to_string(), "Isles, ZEN275");
    assert_eq!(release(Some("Ninja Tune"), None, Some(2021)).to_string(), "Isles, Ninja Tune, 2021");
    assert_eq!(release(None, None, None).to_string(), "Isles");
}
