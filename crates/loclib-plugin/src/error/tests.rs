use hegel::TestCase;
use hegel::generators as gs;
use loclib_progress::{CancelToken, NoProgress};

use super::*;

/// An error wrapping another, as a library's error wraps a file's.
#[derive(Debug, thiserror::Error)]
#[error("{path}: {source}")]
struct AtPath {
    path: String,
    source: std::io::Error,
}

#[derive(Debug, thiserror::Error)]
#[error("not a tracklist")]
struct Unreadable;

/// A library's error is `io` when a file is behind it, the kind the plugin gives otherwise, and
/// `cancelled` once the work was, with its message.
#[test]
fn a_failure_keeps_its_message_and_says_its_kind() {
    let cancel = CancelToken::new();
    let ctx = Ctx::new(&NoProgress, cancel.clone());
    let io = AtPath { path: "/a.mp3".into(), source: std::io::Error::other("unreadable") };
    let kinds = |ctx: &Ctx<'_>| {
        [RpcError::failure(&io, "parse", ctx), RpcError::failure(&Unreadable, "invalid", ctx)]
            .map(|e| (e.code, e.kind().unwrap().to_string(), e.message))
    };
    assert_eq!(
        kinds(&ctx),
        [(FAILED, "io".into(), "/a.mp3: unreadable".into()), (FAILED, "invalid".into(), "not a tracklist".into())]
    );
    cancel.cancel();
    assert_eq!(kinds(&ctx).map(|(_, kind, _)| kind), ["cancelled", "cancelled"]);
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct Said(String);

/// For any work that failed with any message, the error keeps the message and is of kind `io`
/// when an I/O error is behind it, else the kind given; once the work was cancelled, it's
/// `cancelled` either way, as checking the work's context says.
#[hegel::test(test_cases = 50)]
fn prop_a_failure_keeps_its_message_and_says_its_kind(tc: TestCase) {
    let message = tc.draw(gs::text().max_size(12));
    let kind = tc.draw(gs::from_regex("[a-z_]{1,12}").fullmatch(true));
    let (from_io, cancelled) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    let cancel = CancelToken::new();
    if cancelled {
        cancel.cancel();
    }
    let ctx = Ctx::new(&NoProgress, cancel);
    let io = AtPath { path: "/a.mp3".into(), source: std::io::Error::other(message.clone()) };
    let e = match from_io {
        true => RpcError::failure(&io, &kind, &ctx),
        false => RpcError::failure(&Said(message.clone()), &kind, &ctx),
    };
    let checked = ctx.check().map_err(RpcError::from);
    if cancelled {
        assert_eq!(e, RpcError::cancelled());
        assert_eq!(checked, Err(RpcError::cancelled()));
        return;
    }
    assert_eq!(checked, Ok(()));
    let said = if from_io { format!("/a.mp3: {message}") } else { message };
    let kind = if from_io { "io" } else { kind.as_str() };
    assert_eq!((e.code, e.kind(), e.message.as_str()), (FAILED, Some(kind), said.as_str()));
}

/// Any params read as what they're for when they fit it; when they don't, `invalid_params` says
/// where (`[2]`), unless it's the params as a whole.
#[hegel::test(test_cases = 50)]
fn prop_params_that_dont_fit_say_where(tc: TestCase) {
    let ids: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>()).max_size(4));
    assert_eq!(params::<Vec<u32>>(serde_json::json!(ids)), Ok(ids.clone()));
    let at = tc.draw(gs::integers::<usize>().max_value(ids.len()));
    let mut wrong: Vec<serde_json::Value> = ids.iter().map(|id| serde_json::json!(id)).collect();
    wrong.insert(at, serde_json::json!(tc.draw(gs::text().max_size(3))));
    let e = params::<Vec<u32>>(serde_json::Value::from(wrong)).unwrap_err();
    assert_eq!(e.kind(), Some("invalid_params"));
    assert_eq!(e.data.unwrap().fields.get("path"), Some(&serde_json::json!(format!("[{at}]"))));
    let whole = params::<Vec<u32>>(serde_json::json!(tc.draw(gs::booleans()))).unwrap_err();
    assert!(whole.message.starts_with("invalid params: invalid type: boolean"), "{}", whole.message);
    assert_eq!(whole.data.unwrap().fields.get("path"), None);
}

/// Any result is sent as its JSON, and one that has none (a map keyed by what isn't a string) is
/// an internal error rather than a response.
#[hegel::test(test_cases = 50)]
fn prop_a_result_without_json_is_an_internal_error(tc: TestCase) {
    let keys: Vec<(u8, u8)> = tc.draw(gs::vecs(gs::tuples!(gs::integers::<u8>(), gs::integers::<u8>())).max_size(3));
    let map: indexmap::IndexMap<(u8, u8), bool> = keys.into_iter().map(|k| (k, true)).collect();
    match ok(&map) {
        Ok(v) => assert!(map.is_empty() && v == serde_json::json!({}), "{v}"),
        Err(e) => {
            assert!(!map.is_empty());
            assert_eq!((e.code, e.kind(), e.message.as_str()), (INTERNAL, Some("internal"), "key must be a string"));
        }
    }
}
