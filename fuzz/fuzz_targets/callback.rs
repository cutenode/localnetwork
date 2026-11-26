// paths: fuzz/fuzz_targets/callback.rs
// paths: fuzz/seeds/callback/**
// paths: crates/locallibrary/src/callback.rs
// paths: crates/locallibrary/src/export.rs
// coverage: crates/locallibrary/src/callback.rs
//! The HTTP request the OAuth sign-in reads on its loopback port, which any program on the
//! computer can send: the request line, headers and the redirect's query, as a request and as a
//! pasted address, and the refusal worded from it.

#![no_main]

use libfuzzer_sys::fuzz_target;
use locallibrary::callback::CallbackQuery;
use locallibrary::testing::callback_request;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let queries =
        [callback_request(data).flatten(), Some(CallbackQuery::parse(&text)), CallbackQuery::from_url(&text).ok()];
    for q in queries.into_iter().flatten() {
        let _ = q.refusal();
    }
});
