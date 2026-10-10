//! Path keys: how a track's audio file is recognised across exports of one library, and finding
//! it below a folder.
//!
//! A *key* is a path relative to the drive root, `/`-separated, Unicode NFC-normalised, without a
//! leading slash (`Contents/A/B/x.mp3`). Normalising matters: rekordbox writes NFC names, while
//! macOS may hand back NFD names when listing a directory, and the same track must compare equal
//! either way.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

/// How file paths are written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PathStyle {
    /// Relative to the output file's folder (absolute when there is no relative path, e.g. on
    /// another Windows drive letter).
    #[default]
    Relative,
    /// The drive's own path, `/Contents/Artist/Track.mp3`.
    Drive,
    /// Absolute on this computer, through the drive's mount point.
    Absolute,
}

/// Normalise any path-ish string into a key.
pub fn key(s: &str) -> String {
    let s: String = s.nfc().collect();
    let s = s.replace('\\', "/");
    s.trim_start_matches('/').to_string()
}

/// Resolve a key below a drive root. Components that could climb out of the root (`..`,
/// `.`, drive prefixes like `C:`) are dropped, so the result is always lexically inside `root`.
/// Check keys from untrusted databases first: each component should be
/// [`is_safe_component`].
pub fn to_fs(root: &Path, key: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for part in key.split('/').filter(|c| is_safe_component(c)) {
        p.push(part);
    }
    p
}

/// Whether a key's component stays where it is: not empty, `.` or `..`, and without a drive
/// prefix or another separator.
pub fn is_safe_component(c: &str) -> bool {
    !c.is_empty() && c != "." && c != ".." && !c.contains([':', '\\', '\0'])
}

/// Make a string safe to use as a single path component on FAT32/exFAT.
pub fn sanitize_component(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    // Windows drops trailing dots and spaces in any order ("x ." is "x").
    let mut out = cleaned.trim_start().trim_end_matches(|c: char| c == '.' || c.is_whitespace()).to_string();
    // Nor can Windows open a file named after a device, with or without an extension ("nul.txt").
    let stem = out.split('.').next().unwrap_or("").trim_end();
    if is_reserved_name(stem) {
        out.insert(stem.len(), '_');
    }
    if out.is_empty() { "_".into() } else { out }
}

fn is_reserved_name(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && matches!(upper.as_bytes()[3], b'1'..=b'9'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_paths_and_windows_paths_make_the_same_key() {
        // "é" as e + combining acute (NFD) must match the precomposed form (NFC).
        assert_eq!(key("/Contents/Cafe\u{301}/x.mp3"), "Contents/Caf\u{e9}/x.mp3");
        assert_eq!(key("Contents\\Caf\u{e9}\\x.mp3"), "Contents/Caf\u{e9}/x.mp3");
    }

    #[test]
    fn keys_stay_below_their_root() {
        let root = Path::new("/r");
        assert_eq!(to_fs(root, "../../etc/passwd"), Path::new("/r/etc/passwd"));
        assert_eq!(to_fs(root, "C:/x"), Path::new("/r/x"));
    }

    #[test]
    fn sanitize() {
        assert_eq!(sanitize_component("AC/DC: Live?"), "AC_DC_ Live_");
        assert_eq!(sanitize_component("  ...  "), "_");
        assert_eq!(sanitize_component("x ."), "x");
        assert_eq!(sanitize_component("Nul.m3u8"), "Nul_.m3u8");
        assert_eq!(sanitize_component("COM1"), "COM1_");
    }
}
