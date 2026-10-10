//! Properties of drive path keys, file and volume names (`paths`, `format::fat32::sanitize_label`).

use std::path::{Component, Path};

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use locallibrary::format::fat32::sanitize_label;
use locallibrary::paths::{self, FileClass};

/// Path-ish text: separators of both kinds, dots, drive colons, NUL, combining accents (NFD) and
/// precomposed letters (NFC), mixed with arbitrary text.
fn pathish() -> impl hegel::PrintableGenerator<String> {
    hegel::one_of!(
        gs::text().alphabet("/\\.:aBé\u{301}e \0").max_size(24),
        gs::text().max_size(24),
        gs::sampled_from(vec![
            "Contents",
            "PIONEER",
            "rekordbox",
            "USBANLZ",
            ".locallibrary",
            "..",
            ".",
            "C:",
            "._x",
            // FAT32 and exFAT ignore letter case: these name the same folders.
            ".LOCALLIBRARY",
            ".LocalLibrary",
            "contents",
            "Pioneer",
        ])
        .map(String::from),
    )
}

/// A key made of components drawn from [`pathish`] and joined with `/`.
fn keyish() -> impl hegel::PrintableGenerator<String> {
    gs::vecs(pathish()).max_size(5).map(|parts| parts.join("/"))
}

const FORBIDDEN: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2",
    "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Normalising a key twice changes nothing, and a key never starts with `/` or holds a `\`.
#[hegel::test(test_cases = 500)]
fn key_is_idempotent(tc: TestCase) {
    let s = tc.draw(keyish());
    let k = paths::key(&s);
    assert_eq!(paths::key(&k), k);
    assert!(!k.starts_with('/') && !k.contains('\\'), "{k:?}");
    assert_eq!(paths::key(&paths::device_path(&k)), k, "device paths map back to their key");
}

/// Whatever a key says, it resolves to a path lexically inside the root, and a key
/// [`paths::is_safe_key`] accepts resolves exactly to `root/key`.
#[hegel::test(test_cases = 500)]
fn to_fs_stays_below_the_root(tc: TestCase) {
    let root = Path::new("/drives/USB");
    let k = tc.draw(keyish());
    let p = paths::to_fs(root, &k);
    let rest = p.strip_prefix(root).expect("below the root");
    assert!(rest.components().all(|c| matches!(c, Component::Normal(_))), "{p:?}");
    if paths::is_safe_key(&k) {
        assert_eq!(p, root.join(&k));
        assert_eq!(rest.components().count(), k.split('/').count());
    }
}

/// Only keys under `Contents/` or `PIONEER/` are managed, never OS clutter or locallibrary's
/// own folder, and everything under `Contents/` is audio.
#[hegel::test(test_cases = 500)]
fn classify_only_manages_contents_and_pioneer(tc: TestCase) {
    let k = tc.draw(keyish());
    let top = k.split('/').next().unwrap_or("");
    match paths::classify(&k) {
        Some(class) => {
            assert!(top == paths::CONTENTS_DIR || top == paths::PIONEER_DIR, "{k:?}");
            assert!(!k.split('/').any(paths::is_ignored_name), "{k:?}");
            assert_eq!(class == FileClass::Audio, top == paths::CONTENTS_DIR, "{k:?}");
        }
        None => assert!(
            !k.contains('/')
                || !(top == paths::CONTENTS_DIR || top == paths::PIONEER_DIR)
                || k.split('/').any(paths::is_ignored_name),
            "{k:?}"
        ),
    }
}

/// Nothing from a database ever reaches locallibrary's own folder, whatever its letter case:
/// FAT32 and exFAT drives ignore case, so `.LOCALLIBRARY/state.json` is `.locallibrary/state.json`.
#[hegel::test(test_cases = 500)]
fn the_state_folder_is_off_limits_in_any_case(tc: TestCase) {
    let k = tc.draw(keyish());
    if k.split('/').next().is_some_and(|top| top.eq_ignore_ascii_case(paths::STATE_DIR)) {
        assert!(!paths::is_safe_key(&k), "{k:?}");
        assert_eq!(paths::classify(&k), None, "{k:?}");
    }
}

/// A sanitised name is a usable FAT32/exFAT file name on every OS: not empty, none of the
/// characters FAT forbids, no leading or trailing spaces or trailing dot (Windows strips them),
/// not a reserved device name (`CON`, `nul.txt`), and sanitising it again changes nothing.
#[hegel::test(test_cases = 1000)]
#[hegel::explicit_test_case(s = "x .".to_string())]
#[hegel::explicit_test_case(s = "nul.m3u8".to_string())]
fn sanitize_component_is_fat_legal_and_idempotent(tc: TestCase) {
    let s = tc.draw(hegel::one_of!(
        gs::text().max_size(30),
        gs::text().alphabet(". aCoNuLx1/:*\t").max_size(12),
        gs::sampled_from(RESERVED.to_vec()).map(|r| r.to_lowercase() + ".txt"),
    ));
    let out = paths::sanitize_component(&s);
    assert!(!out.is_empty());
    assert!(!out.contains(FORBIDDEN) && !out.chars().any(char::is_control), "{out:?}");
    assert_eq!(out.trim(), out, "no leading or trailing whitespace");
    assert!(!out.ends_with('.'), "{out:?}");
    let stem = out.split('.').next().unwrap().trim_end();
    assert!(!RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)), "{out:?} is a reserved device name");
    assert_eq!(paths::sanitize_component(&out), out);
}

/// Names already legal are kept as they are.
#[hegel::test]
fn sanitize_component_keeps_plain_names(tc: TestCase) {
    let s = tc.draw(gs::from_regex("[A-Za-z0-9][A-Za-z0-9 _()-]{0,20}[A-Za-z0-9]").fullmatch(true));
    tc.assume(!RESERVED.iter().any(|r| r.eq_ignore_ascii_case(&s)));
    assert_eq!(paths::sanitize_component(&s), s);
}

/// A FAT volume label is at most 11 characters of upper-case ASCII without the characters FAT
/// forbids, never empty, never padded with spaces at either end, and sanitising it again
/// changes nothing.
#[hegel::test(test_cases = 1000)]
fn sanitize_label_is_a_fat_label(tc: TestCase) {
    let name = tc.draw(hegel::one_of!(gs::text().max_size(30), gs::text().alphabet("aZ .:_/é ").max_size(16)));
    let fallback = tc.draw(gs::sampled_from(vec!["REKORDBOX", "NO NAME", "X"]));
    let label = sanitize_label(&name, fallback);
    assert!(!label.is_empty() && label.len() <= 11, "{label:?}");
    assert!(
        label.chars().all(|c| (c.is_ascii_graphic() || c == ' ')
            && !c.is_ascii_lowercase()
            && !"\"*+,./:;<=>?[\\]|".contains(c)),
        "{label:?}"
    );
    assert_eq!(label.trim(), label);
    assert_eq!(sanitize_label(&label, fallback), label);
}
