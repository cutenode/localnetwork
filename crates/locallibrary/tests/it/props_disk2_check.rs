//! The check before a set, on what earlier properties leave to chance: the loudness item over
//! levels analyzers kept in the store already, history lists against a store the drive moved on
//! from, a packed drive given before the drive it was packed from, and a check that's cancelled.

use crate::common;

use rustc_hash::FxHashSet;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::check::{self, CheckOptions, CheckReport, Level};
use locallibrary::device::Device;
use locallibrary::fields::{FieldValue, Role};
use locallibrary::pack::{self, PackOptions};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::{CancelToken, Error, paths};

fn items<'a>(r: &'a CheckReport, drive: &str, check: &str) -> Vec<(Level, &'a str)> {
    r.items.iter().filter(|i| i.drive == drive && i.check == check).map(|i| (i.level, i.message.as_str())).collect()
}

/// A level of `kind`, by role: 0 fine, 1 over 0 dBTP, 2 clipped, 3 far from the rest.
fn level(kind: u8) -> [(Role, FieldValue); 3] {
    [
        (Role::Loudness, FieldValue::Number(if kind == 3 { -30.0 } else { -9.0 })),
        (Role::TruePeak, FieldValue::Number(if kind == 1 { 1.6 } else { -0.9 })),
        (Role::ClippedRuns, FieldValue::Integer(i64::from(kind == 2) * 3)),
    ]
}

/// With levels kept for some of a drive's tracks, the check has one `loudness` item: a
/// warning naming exactly the tracks over 0 dBTP, clipped, or far from the rest's level (one
/// quiet track among at least three), else fine, counting the tracks analysed; with none
/// analysed, or no store, there's no item. A check that's cancelled stops with nothing said.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(kinds = vec![0u8, 0, 4, 1, 4, 2, 0, 4, 4, 4, 4, 4], far = Some(6usize))]
#[hegel::explicit_test_case(kinds = vec![0u8, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4], far = None::<usize>)]
#[hegel::explicit_test_case(kinds = vec![4u8; 12], far = None::<usize>)]
fn the_loudness_item_names_the_tracks_whose_level_is_a_problem(tc: TestCase) {
    // Per track: its level's kind (as `level` takes it), 4 for none kept.
    let kinds: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>().max_value(4)).min_size(12).max_size(12));
    // One analysed track much quieter than the rest.
    let far: Option<usize> = tc.draw(gs::optional(gs::integers::<usize>().max_value(11)));
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let lib = common::library(&root);
    let analysed: Vec<usize> = (0..lib.tracks.len()).filter(|&i| kinds.get(i).is_some_and(|k| *k < 4)).collect();
    let far = far.filter(|f| analysed.contains(f) && analysed.len() >= 3);
    let mut flagged = FxHashSet::default();
    for &i in &analysed {
        let t = &lib.tracks[i];
        let kind = if far == Some(i) {
            3
        } else if kinds[i] == 3 {
            0
        } else {
            kinds[i]
        };
        if kind > 0 {
            flagged.insert(t.id);
        }
        common::heard(&store, &root, &t.file_path, &level(kind));
    }

    let devices = [Device::open_export(&root).unwrap()];
    let r = check::check(&devices, Some(&store), &CheckOptions::default(), &Ctx::silent()).unwrap();
    let loudness = items(&r, "Main", "loudness");
    let counted = analysed.iter().map(|&i| lib.tracks[i].id).collect::<FxHashSet<_>>().len();
    match (counted, flagged.len()) {
        (0, _) => assert!(loudness.is_empty() && r.loudness.is_empty(), "{loudness:?}"),
        (n, 0) => {
            assert_eq!(loudness.len(), 1);
            assert_eq!(loudness[0].0, Level::Ok);
            assert!(loudness[0].1.starts_with(&format!("none of the {n} analysed track(s)")), "{loudness:?}");
            assert!(r.loudness.is_empty());
        }
        (_, n) => {
            assert_eq!(loudness.len(), 1);
            assert_eq!(loudness[0].0, Level::Warning);
            assert!(loudness[0].1.starts_with(&format!("{n} track(s) are over 0 dBTP")), "{loudness:?}");
            let [d] = r.loudness.as_slice() else { panic!("{:?}", r.loudness) };
            assert_eq!(d.drive, "Main");
            assert_eq!(d.tracks.iter().map(|f| f.track.id).collect::<FxHashSet<_>>(), flagged);
            let named = d.tracks.iter().take(3).all(|f| loudness[0].1.contains(&f.track.name));
            assert!(named, "{loudness:?}");
        }
    }
    // Without a store there's nothing to compare against.
    let alone = check::check(&devices, None, &CheckOptions::default(), &Ctx::silent()).unwrap();
    assert!(items(&alone, "Main", "loudness").is_empty() && alone.loudness.is_empty());

    let cancel = CancelToken::new();
    cancel.cancel();
    let quiet = common::Warnings::default();
    let cancelled = check::check(&devices, Some(&store), &CheckOptions::default(), &Ctx::new(&quiet, cancel));
    assert!(matches!(cancelled, Err(Error::Cancelled)), "{cancelled:?}");
}

/// A drive packed from another, checked before it: the other's `same_library` item says it was
/// packed from it, fine until the source changes. History lists pulled into the store stay safe
/// after the drive moves on (every list is in some snapshot), and so they are on a copy of the
/// drive that was never adopted.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(moved_on = true, copied = true)]
#[hegel::explicit_test_case(moved_on = false, copied = false)]
fn a_drive_checked_after_its_pack_and_its_history_after_it_moved_on(tc: TestCase) {
    let moved_on = tc.draw(gs::booleans());
    let copied = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::bigger_drive(dir.path(), "A");
    let lib = common::library(&a);
    common::add_history(&a, "HISTORY 001", &[lib.tracks[0].id, lib.tracks[1].id]);
    common::apply(
        &a,
        &[locallibrary::edit::Edit::SetTracks {
            playlist: "Packed".into(),
            tracks: locallibrary::query::TrackSelector::paths([lib.tracks[0].file_path.clone()]),
            sort: None,
            limit: None,
            create: true,
        }],
    );
    let mut da = Device::open_export(&a).unwrap();
    da.adopt(Some("A")).unwrap();
    sync::pull(&store, &mut da, &PullOptions::default(), &Ctx::silent()).unwrap();
    let c = dir.path().join("C");
    if copied {
        common::copy_dir(&a, &c);
        let _ = std::fs::remove_dir_all(c.join(paths::STATE_DIR));
    }
    let b = common::blank(dir.path(), "B");
    let (src, mut club) = (Device::open_export(&a).unwrap(), Device::open(&b).unwrap());
    let opts = PackOptions { playlists: vec!["Packed".into()], ..Default::default() };
    let plan = pack::plan(&src, &club, &opts, &Ctx::silent()).unwrap();
    pack::apply(&plan, &src, &mut club, Some(&store), &Ctx::silent()).unwrap();
    if moved_on {
        common::export_new_track(&a, "Moved", "On");
    }

    let mut devices = vec![Device::open_export(&b).unwrap(), Device::open_export(&a).unwrap()];
    if copied {
        devices.push(Device::open_export(&c).unwrap());
    }
    let r = check::check(&devices, Some(&store), &CheckOptions::default(), &Ctx::silent()).unwrap();
    let packed = items(&r, "A", "same_library");
    let [(level, message)] = packed.as_slice() else { panic!("{packed:?}") };
    assert!(message.starts_with("B is packed from A (Packed)"), "{message}");
    assert_eq!(*level == Level::Ok, !moved_on, "{message}");
    if moved_on {
        assert!(message.contains("`locallibrary pack A B`"), "{message}");
    }

    let history = items(&r, "A", "history");
    let [(level, message)] = history.as_slice() else { panic!("{history:?}") };
    assert_eq!(*level, Level::Ok, "{message}");
    let snapshot = message.contains("saved in snapshot");
    assert_eq!(snapshot, !moved_on, "{message}");
    if moved_on {
        assert_eq!(*message, "1 history list(s), all in the store");
    }
    if copied {
        let history = items(&r, "C", "history");
        assert_eq!(history.iter().map(|(l, _)| *l).collect::<Vec<_>>(), [Level::Ok], "{history:?}");
    }
}
