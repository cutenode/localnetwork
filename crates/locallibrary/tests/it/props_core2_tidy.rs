//! Tidying with a plan the drive moved on from: what someone removed or turned into something
//! else since the plan isn't touched, a plan item naming a path that leaves the drive fails
//! without anything outside it going, and a link that looks like clutter is never followed.

use crate::common;

use std::collections::BTreeSet;
use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::tidy::{self, TidyItem, TidyKind, TidyOptions};

/// Clutter of each kind: its path, and whether it's a folder that goes whole.
const CLUTTER: &[(&str, bool)] = &[
    ("Contents/A/._Track.mp3", false),
    ("Contents/A/.DS_Store", false),
    ("Contents/A/Thumbs.db", false),
    ("Contents/A/.locallibrary-tmp-77", false),
    (".TemporaryItems", true),
    (".Spotlight-V100", true),
    (".fseventsd", true),
    (".Trashes", true),
];

/// Plan items that name paths outside the drive, or ones no plan holds.
const FORGED: &[&str] = &["../outside.txt", "Contents//x", "./.DS_Store", "Contents\\A\\.DS_Store", "a/../b"];

#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn tidying_a_stale_plan_removes_only_what_is_still_clutter(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    for (key, is_dir) in CLUTTER {
        let p = root.join(key);
        match is_dir {
            true => {
                fs::create_dir_all(p.join("inner")).unwrap();
                fs::write(p.join("inner/x"), b"clutter").unwrap();
                fs::write(p.join("y"), b"more").unwrap();
            }
            false => {
                fs::create_dir_all(p.parent().unwrap()).unwrap();
                fs::write(&p, [&[0u8, 5, 0x16, 7][..], &[0; 28]].concat()).unwrap();
            }
        }
    }
    // A link named like clutter, to a file outside the drive.
    let outside = dir.path().join("outside.txt");
    fs::write(&outside, b"not the drive's").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, root.join("Contents/Thumbs.db")).unwrap();

    let d = Device::open_export(&root).unwrap();
    let mut plan = tidy::plan(&d, &TidyOptions { empty_trash: true }, &Ctx::silent()).unwrap();
    let planned: BTreeSet<&str> = plan.items.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(planned, CLUTTER.iter().map(|(k, _)| *k).collect(), "the link isn't clutter");
    let kinds: Vec<TidyKind> = plan.by_kind().into_iter().map(|(k, _, _)| k).collect();
    let described: BTreeSet<&str> = kinds.iter().map(|k| k.describe()).collect();
    assert_eq!(described.len(), kinds.len(), "each kind says what it is");

    // Since the plan: each item left, removed, or turned into something tidying doesn't remove.
    let mut want_removed = 0;
    let mut want_bytes = 0;
    let mut stays = Vec::new();
    for item in &plan.items {
        let p = root.join(&item.path);
        match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => {
                want_removed += item.files.max(1);
                want_bytes += item.bytes;
            }
            1 => match item.is_dir {
                true => fs::remove_dir_all(&p).unwrap(),
                false => fs::remove_file(&p).unwrap(),
            },
            _ => {
                match item.is_dir {
                    true => {
                        fs::remove_dir_all(&p).unwrap();
                        fs::write(&p, b"a file now").unwrap();
                    }
                    // AppleDouble without its magic number; anything else a folder now.
                    false if item.kind == TidyKind::AppleDouble => fs::write(&p, b"plain").unwrap(),
                    false => {
                        fs::remove_file(&p).unwrap();
                        fs::create_dir(&p).unwrap();
                    }
                }
                stays.push(p);
            }
        }
    }
    let forged: Vec<&str> = tc.draw(gs::subsequences(FORGED));
    for f in &forged {
        plan.items.push(TidyItem { path: f.to_string(), kind: TidyKind::DsStore, is_dir: false, files: 1, bytes: 1 });
    }

    let r = tidy::apply(&plan, &d, &Ctx::silent()).unwrap();
    assert_eq!((r.removed, r.bytes), (want_removed, want_bytes));
    let failed: BTreeSet<&str> = r.failed.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(failed, forged.iter().copied().collect());
    assert!(outside.exists());
    assert!(stays.iter().all(|p| p.exists()), "{stays:?}");
}
