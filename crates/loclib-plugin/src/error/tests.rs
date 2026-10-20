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
