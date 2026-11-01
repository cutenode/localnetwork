use super::*;

#[test]
fn device_paths_and_windows_paths_make_the_same_key() {
    // "é" as e + combining acute (NFD) must match the precomposed form (NFC).
    assert_eq!(key("/Contents/Cafe\u{301}/x.mp3"), "Contents/Caf\u{e9}/x.mp3");
    assert_eq!(key("Contents\\Caf\u{e9}\\x.mp3"), "Contents/Caf\u{e9}/x.mp3");
}

/// A track's file is the same file in any letter case, however its path is written.
#[test]
fn a_file_key_ignores_letter_case() {
    assert_eq!(file_key("/Contents/CAFE\u{301}/X.mp3"), "contents/caf\u{e9}/x.mp3");
    assert_eq!(file_key("Contents\\Caf\u{e9}\\x.MP3"), file_key("/contents/CAF\u{c9}/x.mp3"));
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

use std::ffi::OsStr;
use std::path::Component;

use hegel::generators as gs;
use hegel::{Generator, TestCase};

/// Path-ish text: both separators, dots, drive prefixes, accents composed and not, and
/// anything else.
fn pathish() -> impl hegel::PrintableGenerator<String> {
    let parts = vec!["/", "\\", ".", "..", "C:", "e\u{301}", "\u{e9}", "a", "B", "\0", " ", "x.mp3"];
    gs::vecs(hegel::one_of!(gs::sampled_from(parts).map(String::from), gs::text().max_size(3)))
        .max_size(8)
        .map(|p| p.concat())
}

/// A key is the same however the path was written (NFC or NFD, `\` or `/`, a leading slash
/// or not), has no leading slash or backslash, and keying it again changes nothing.
#[hegel::test(test_cases = 300)]
fn keys_are_the_same_however_the_path_is_written(tc: TestCase) {
    let s = tc.draw(pathish());
    let k = key(&s);
    assert_eq!(key(&k), k);
    assert!(!k.starts_with('/') && !k.contains('\\'), "{k:?}");
    assert_eq!(k.nfc().collect::<String>(), k);
    assert_eq!(key(&s.nfd().collect::<String>()), k);
    assert_eq!(key(&s.replace('/', "\\")), k);
    assert_eq!(key(&format!("/{s}")), k);
}

/// Whatever a key holds, its file is inside the root: below the root are only plain names
/// (as this platform reads paths), each a piece of the key in the key's order, and a file
/// made there is still inside the root once the file system resolves the path. A key of
/// plain names is joined on as it is; what could climb out is dropped.
#[hegel::test(test_cases = 300)]
fn keys_resolve_inside_their_root(tc: TestCase) {
    for (k, want) in [
        ("Contents/A/x.mp3", "Contents/A/x.mp3"),
        ("/../../etc/passwd", "etc/passwd"),
        ("C:/x.mp3", "x.mp3"),
        ("a/./b//c", "a/b/c"),
        (r"..\..\x.mp3", ""),
    ] {
        assert_eq!(to_fs(Path::new("/r"), k), Path::new("/r").join(want), "{k:?}");
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let k = tc.draw(pathish());
    let p = to_fs(&root, &k);
    let below: Vec<Component> = p.strip_prefix(&root).unwrap().components().collect();
    assert!(below.iter().all(|c| matches!(c, Component::Normal(_))), "{k:?} gave {below:?}");
    let mut pieces = k.split('/');
    assert!(below.iter().all(|c| pieces.any(|piece| c.as_os_str() == OsStr::new(piece))), "{k:?} gave {below:?}");
    // Names the file system takes: Windows refuses some ASCII (`?`, `CON`) and APFS some
    // other code points, but what could climb out is ASCII.
    #[cfg(unix)]
    if !below.is_empty() && k.is_ascii() {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"").unwrap();
        assert!(p.canonicalize().unwrap().starts_with(&root), "{k:?} reached {p:?}");
    }
}

/// A sanitised name is a safe component that FAT32 and Windows take as it is: something
/// left, none of the characters they refuse, no blank first, no dot or blank last, and not a
/// device's name. Sanitising it again changes nothing, and a name that was fine already is
/// kept.
#[hegel::test(test_cases = 500)]
fn sanitised_names_are_safe_on_fat(tc: TestCase) {
    let parts = vec!["CON", "nul", "Com1", "lpt9", "aux", "prn", " ", ".", "x", "/", ":", "?", "\u{7f}", "\t", "é"];
    let s = tc.draw(hegel::one_of!(
        gs::text().max_size(8),
        gs::vecs(gs::sampled_from(parts).map(String::from)).max_size(4).map(|p| p.concat()),
    ));
    let out = sanitize_component(&s);
    let refused = |c: char| "/\\:*?\"<>|".contains(c) || c.is_control();
    let stem = |n: &str| n.split('.').next().unwrap_or("").trim_end().to_string();
    assert!(!out.is_empty() && is_safe_component(&out), "{s:?} -> {out:?}");
    assert!(!out.chars().any(refused), "{out:?}");
    assert!(!out.starts_with(char::is_whitespace), "{out:?}");
    assert!(!out.ends_with(|c: char| c == '.' || c.is_whitespace()), "{out:?}");
    assert!(!is_reserved_name(&stem(&out)), "{out:?}");
    assert_eq!(sanitize_component(&out), out, "{s:?}");
    let fine = !s.is_empty()
        && !s.chars().any(refused)
        && !s.starts_with(char::is_whitespace)
        && !s.ends_with(|c: char| c == '.' || c.is_whitespace())
        && !is_reserved_name(&stem(&s));
    if fine {
        assert_eq!(out, s);
    }
}

/// Path styles serialise in snake case and read back; relative is the default.
#[hegel::test(test_cases = 20)]
fn path_styles_round_trip_through_json(tc: TestCase) {
    let style = tc.draw(gs::sampled_from(vec![PathStyle::Relative, PathStyle::Drive, PathStyle::Absolute]));
    let json = serde_json::to_value(style).unwrap();
    assert_eq!(json, format!("{style:?}").to_lowercase());
    assert_eq!(serde_json::from_value::<PathStyle>(json).unwrap(), style);
    assert_eq!(PathStyle::default(), PathStyle::Relative);
}

/// Made absolute, a path is the current folder (when it's relative) followed by its parts, each
/// `..` taking the part before it away and each `.` nothing; nothing at all stays nothing. With
/// what's real followed, the part that exists is the real folder, followed by the rest.
#[hegel::test(test_cases = 60)]
#[hegel::explicit_test_case(parts = vec!["a", "..", ".", "b"], rooted = false, made = 1usize)]
#[hegel::explicit_test_case(parts = Vec::<&str>::new(), rooted = false, made = 0usize)]
// Regression: `..` was folded before symlinks were resolved, so this many up from a macOS temp
// folder (`/var/folders/xx/yyy/T/.tmpX`, `/var` being `/private/var`) gave `/`, not `/private`.
#[hegel::explicit_test_case(parts = vec![".."; 6], rooted = false, made = 0usize)]
fn paths_are_folded_part_by_part(tc: TestCase) {
    let real = |p: &Path| dunce::canonicalize(p).ok();
    let parts: Vec<&str> = tc.draw(gs::vecs(gs::sampled_from(vec!["a", "b", "..", ".", "c d"])).max_size(6));
    let rooted = tc.draw(gs::booleans());
    let joined = parts.join("/");
    let path = PathBuf::from(if rooted { format!("/{joined}") } else { joined.clone() });
    // On Windows a rooted path is on the current folder's drive.
    let mut want = if rooted { std::path::absolute("/").unwrap() } else { std::env::current_dir().unwrap() };
    if !rooted && parts.is_empty() {
        want = PathBuf::new();
    }
    for p in &parts {
        match *p {
            ".." => {
                want.pop();
            }
            "." => {}
            name => want.push(name),
        }
    }
    assert_eq!(absolute_with(&path, |_| None), want, "{path:?}");
    if !rooted && parts.is_empty() {
        assert_eq!(absolute_with(&path, real), PathBuf::new());
    }
    // Below a real folder: the parts that exist resolve, the rest are appended.
    let dir = tempfile::tempdir().unwrap();
    let root = real(dir.path()).unwrap();
    let made = tc.draw(gs::integers::<usize>().max_value(2));
    std::fs::create_dir_all(root.join("a/b").iter().take(root.components().count() + made).collect::<PathBuf>())
        .unwrap();
    let below = dir.path().join(&joined);
    let mut folded = root.clone();
    for p in &parts {
        match *p {
            ".." => {
                folded.pop();
            }
            "." => {}
            name => folded.push(name),
        }
    }
    assert_eq!(absolute_with(&below, real), folded, "{below:?}");
}

/// The part of a path that exists is resolved (symlinks too, a `..` after one stepping out of its
/// target), the rest appended; `..` below a folder that isn't there yet takes it away.
/// Regressions: `..` after a link went back to the link's folder; export's own copy of this left
/// such a path unresolved, so `/tmp/new/../out` and `/private/tmp/out` didn't compare.
#[test]
fn the_part_that_exists_is_resolved() {
    let dir = tempfile::tempdir().unwrap();
    let real = without_verbatim_prefix(dir.path().canonicalize().unwrap());
    assert_eq!(resolved(&dir.path().join("new/../out/x")), real.join("out/x"));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&real, real.join("link")).unwrap();
        assert_eq!(resolved(&real.join("link/missing/../there")), real.join("there"));
        // `..` out of a link is the folder its target is in, where the OS takes it.
        std::fs::create_dir_all(real.join("deep/er")).unwrap();
        std::os::unix::fs::symlink(real.join("deep/er"), real.join("up")).unwrap();
        assert_eq!(resolved(&real.join("up/../x")), real.join("deep/x"));
        assert_eq!(resolved(&real.join("new/../up/x")), real.join("deep/er/x"));
    }
}

#[test]
fn relative_paths() {
    assert_eq!(
        relative(Path::new("/Volumes/USB/Lists"), Path::new("/Volumes/USB/Contents/A/x.mp3")).as_deref(),
        Some("../Contents/A/x.mp3")
    );
    assert_eq!(
        relative(Path::new("/Volumes/USB"), Path::new("/Volumes/USB/Contents/x.mp3")).as_deref(),
        Some("Contents/x.mp3")
    );
    assert_eq!(
        relative(Path::new("/Users/me/out/Engine Library"), Path::new("/Volumes/USB/Contents/a.mp3")).as_deref(),
        Some("../../../../Volumes/USB/Contents/a.mp3")
    );
}

#[test]
fn a_relative_path_needs_a_root_in_common() {
    assert_eq!(relative(Path::new("Lists"), Path::new("/Volumes/USB/x.mp3")), None);
    assert_eq!(relative(Path::new("out"), Path::new("out/x.mp3")), None);
}

/// A path made relative to an absolute folder leads back to the file from that folder (each `..`
/// climbing one folder, only as far as the two paths part); from a relative folder, which shares
/// no root with the file, there's none.
#[hegel::test(test_cases = 300)]
fn a_relative_path_leads_from_the_folder_to_the_file(tc: TestCase) {
    let parts = || gs::vecs(gs::sampled_from(vec!["Volumes", "USB", "Lists", "Contents", "a b", "é"])).max_size(4);
    let (from, to): (Vec<&str>, Vec<&str>) = (tc.draw(parts()), tc.draw(parts().min_size(1)));
    let rooted = tc.draw(gs::booleans());
    let from_path = PathBuf::from(format!("{}{}", if rooted { "/" } else { "" }, from.join("/")));
    let to_path = PathBuf::from(format!("/{}", to.join("/")));
    let Some(r) = relative(&from_path, &to_path) else {
        return assert!(!rooted, "{from_path:?} to {to_path:?}");
    };
    assert!(rooted, "{from_path:?} is relative, yet {r:?}");
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut at: Vec<&str> = from.clone();
    for step in r.split('/').filter(|s| !s.is_empty()) {
        match step {
            ".." => assert!(at.pop().is_some(), "{r:?} climbs past the root"),
            s => at.push(s),
        }
    }
    assert_eq!(at, to, "{from_path:?} + {r:?}");
    assert_eq!(r.split('/').filter(|s| *s == "..").count(), from.len() - common, "{r:?}");
}
