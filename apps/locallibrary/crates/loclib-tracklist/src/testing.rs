//! Helpers for the tests.

use crate::Entry;

pub(crate) fn utf16le(s: &str) -> Vec<u8> {
    let mut out = vec![0xFF, 0xFE];
    out.extend(s.encode_utf16().flat_map(u16::to_le_bytes));
    out
}

pub(crate) fn at(entries: &[Entry]) -> Vec<(String, String)> {
    entries.iter().map(|e| (e.artist.clone(), e.title.clone())).collect()
}

pub(crate) fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter().map(|(a, t)| (a.to_string(), t.to_string())).collect()
}
