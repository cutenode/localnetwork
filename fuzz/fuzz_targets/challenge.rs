// paths: fuzz/fuzz_targets/challenge.rs
// paths: fuzz/seeds/challenge/**
// paths: crates/locallibrary/src/plugin/net/challenge.rs
//! What a server refusing a plugin's request says in its `WWW-Authenticate` headers and body: the
//! challenge parser, and whether that refusal is for scopes the token lacks.
//!
//! The input is the status on the first line, one header value on each line after it, then a
//! blank line and the body.

#![no_main]

use libfuzzer_sys::fuzz_target;
use locallibrary::testing::{Challenge, challenges, insufficient_scope};

fuzz_target!(|data: &[u8]| {
    let split = data.windows(2).position(|w| w == b"\n\n");
    let (head, body) = split.map_or((data, &[][..]), |at| (&data[..at], &data[at + 2..]));
    // reqwest only hands over headers that are visible ASCII; the parser takes any text.
    let head = String::from_utf8_lossy(head);
    let mut lines = head.lines();
    let status = lines.next().and_then(|s| s.trim().parse().ok()).unwrap_or(403);
    let said: Vec<Challenge> = lines.flat_map(challenges).collect();
    for c in &said {
        let _ = (c.param("error"), c.param("scope"));
    }
    let _ = insufficient_scope(status, &said, body);
});
