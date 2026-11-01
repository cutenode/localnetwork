//! Path keys: how a track's audio file is recognised across exports of one library, and finding
//! it below a folder; and paths on this computer made absolute, and relative to a folder.
//!
//! A *key* is a path relative to the drive root, `/`-separated, Unicode NFC-normalised, without a
//! leading slash (`Contents/A/B/x.mp3`). Normalising matters: rekordbox writes NFC names, while
//! macOS may hand back NFD names when listing a directory, and the same track must compare equal
//! either way.

use std::path::{Component, Path, PathBuf};

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

/// `to` relative to the folder `from`, `/`-separated. `None` when `from` has no root (a relative
/// folder) or they share none (another Windows drive).
pub fn relative(from: &Path, to: &Path) -> Option<String> {
    let a: Vec<Component> = from.components().filter(|c| *c != Component::CurDir).collect();
    let b: Vec<Component> = to.components().filter(|c| *c != Component::CurDir).collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if common == 0 || !matches!(a.first(), Some(Component::RootDir | Component::Prefix(_))) {
        return None;
    }
    let ups = (common..a.len()).map(|_| "..".to_string());
    let downs = b[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned());
    Some(ups.chain(downs).collect::<Vec<_>>().join("/"))
}

/// `path` made absolute, part by part: `.` is nothing and `..` takes away the part before it.
/// Each name is followed by what `real` says the path so far is (when it says), so that what
/// a `..` takes away is a real folder. `std::path::absolute` leaves `..` in, except on Windows,
/// which folds it this way itself before a path is opened.
pub fn absolute_with(path: &Path, real: impl Fn(&Path) -> Option<PathBuf>) -> PathBuf {
    let mut out = PathBuf::new();
    for c in std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()).components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            Component::Normal(_) => {
                out.push(c);
                out = real(&out).unwrap_or(out);
            }
            c => out.push(c),
        }
    }
    out
}

/// `path` made absolute, with symlinks resolved as far as it exists (macOS's `/tmp` is
/// `/private/tmp`), so two paths compare, and relative paths between them lead where they say;
/// without Windows' `\\?\` prefix. A `..` goes where the OS takes it, out of a link's target
/// rather than back to the link's folder, so a path is inside a folder here exactly when what it
/// opens is (once the folders that aren't there yet are made, `..` folded over them).
pub fn resolved(path: &Path) -> PathBuf {
    absolute_with(path, |p| p.canonicalize().ok().map(without_verbatim_prefix))
}

/// `p` without the `\\?\` prefix [`Path::canonicalize`] adds on Windows (a `\\?\UNC\` share
/// keeps it), so it stays readable and joinable, and compares with the mount points the OS
/// reports (`E:\`). Elsewhere `p` itself.
pub fn without_verbatim_prefix(p: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\")
            && !rest.starts_with("UNC\\")
        {
            return PathBuf::from(rest);
        }
    }
    p
}

/// Normalise any path-ish string into a key.
pub fn key(s: &str) -> String {
    let s: String = s.nfc().collect();
    let s = s.replace('\\', "/");
    s.trim_start_matches('/').to_string()
}

/// A key (or a name) as drives compare it: FAT32, exFAT and HFS+ ignore letter case.
pub fn fold(key: &str) -> String {
    key.to_lowercase()
}

/// A track's file (or a database's path or name) as drives compare it: its [`key`], folded. How a
/// track is matched across databases, libraries and sessions.
pub fn file_key(path: &str) -> String {
    fold(&key(path))
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
mod tests;
