//! Properties of format plans: what they erase, what they warn about, and what applying one
//! refuses.

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::progress::Ctx;

/// What a format plan destroys, volume by volume: each rekordbox library on the disk with its
/// track count, the adopted drive's name (which the drive keeps, and which confirms the erase),
/// and whether the library is safe in the store, with a warning for every one that isn't. A
/// disk holding a protected path is refused.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
#[cfg_attr(
    target_os = "linux",
    ignore = "a plan on Linux asks lsblk what's mounted on the disk, and this one isn't there"
)]
fn prop_plans_list_the_libraries_they_erase(tc: TestCase) {
    use crate::common;
    use locallibrary::api;
    use locallibrary::format::{Disk, FormatOptions, Scheme, Volume};
    let dir = tempfile::tempdir().unwrap();
    let store = tc.draw(gs::booleans()).then(|| locallibrary::Store::open_or_init(dir.path().join("store")).unwrap());
    let volumes = common::isolate(dir.path(), store.as_ref().map(|s| s.root()));

    // Each volume: a folder (0), a library (1), an adopted one (2), or one also saved in the store (3).
    let kinds: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>().max_value(3)).min_size(1).max_size(3));
    let mut parts = Vec::new();
    for (i, kind) in kinds.iter().enumerate() {
        let name = format!("Drive{i}");
        let root = if *kind == 0 {
            let r = volumes.join(&name);
            std::fs::create_dir_all(&r).unwrap();
            r
        } else {
            common::demo_drive(&volumes, &name)
        };
        if *kind >= 2 {
            common::adopt(store.as_ref(), &root, &name);
        }
        if let (3, Some(s)) = (kind, &store) {
            let pull = api::PullRequest {
                drive: common::path(&root),
                library: Some(name.clone()),
                new_library: true,
                ..Default::default()
            };
            api::pull(s, &pull, &Ctx::silent()).unwrap();
        }
        parts.push(Volume {
            id: format!("disk9s{}", i + 1),
            size: 1 << 30,
            filesystem: Some(locallibrary::format::FsKind::Fat32),
            filesystem_name: Some("MS-DOS FAT32".into()),
            label: Some(name),
            mount: Some(root),
            cluster_size: Some(32768),
            uuid: None,
        });
    }
    let disk = Disk {
        id: "disk9".into(),
        device: "/dev/disk9".into(),
        model: "Stick".into(),
        serial: None,
        size: 64 << 30,
        sector_size: 512,
        bus: Some("USB".into()),
        removable: true,
        internal: false,
        system: false,
        virtual_disk: false,
        read_only: false,
        scheme: Some(Scheme::Mbr),
        volumes: parts,
    };
    let plan = locallibrary::format::plan(disk.clone(), &FormatOptions::default(), store.as_ref(), &[], &Ctx::silent())
        .unwrap();
    assert_eq!(plan.erases.len(), kinds.len());
    let mut kept = None;
    for (i, (e, kind)) in plan.erases.iter().zip(&kinds).enumerate() {
        let name = format!("Drive{i}");
        assert_eq!(e.tracks, (*kind > 0).then_some(2), "{name}");
        assert_eq!(e.drive_name.as_deref(), (*kind >= 2).then_some(name.as_str()));
        let saved = *kind == 3 && store.is_some();
        assert_eq!(e.in_store, store.as_ref().and(Some(saved)).filter(|_| *kind > 0), "{name}");
        let warned =
            |what: &str| plan.warnings.iter().any(|w| w.starts_with(&format!("{name} holds")) && w.contains(what));
        assert_eq!(warned("isn't in the store"), *kind > 0 && store.is_some() && !saved);
        assert_eq!(warned("Everything on it will be lost"), *kind > 0 && store.is_none());
        if *kind >= 2 {
            kept = kept.or(Some(name));
        }
    }
    assert_eq!(plan.keeps_name, kept);
    assert_eq!(plan.confirmation(), kept.as_deref().unwrap_or("disk9"));
    let first_label = kept.clone().unwrap_or_else(|| "Drive0".into());
    assert_eq!(plan.label, locallibrary::format::Filesystem::Fat32.label(&first_label));

    let protected = [volumes.join("Drive0").join("store")];
    assert!(
        locallibrary::format::plan(disk, &FormatOptions::default(), store.as_ref(), &protected, &Ctx::silent())
            .is_err()
    );
    locallibrary::testing::volumes_on_this_thread(None);
    locallibrary::testing::store_root_on_this_thread(None);
}

/// A 64 GB USB stick whose one FAT32 volume is mounted at `mount`, as a format sees it.
#[cfg(any(target_os = "macos", windows))]
fn stick(mount: &std::path::Path) -> locallibrary::format::Disk {
    use locallibrary::format::{Disk, FsKind, Scheme, Volume};
    Disk {
        id: "disk998".into(),
        device: "/dev/disk998".into(),
        model: "Cruzer Blade".into(),
        serial: Some("4C530002".into()),
        size: 64_000_000_000,
        sector_size: 512,
        bus: Some("USB".into()),
        removable: true,
        internal: false,
        system: false,
        virtual_disk: false,
        read_only: false,
        scheme: Some(Scheme::Mbr),
        volumes: vec![Volume {
            id: "disk998s1".into(),
            size: 63_998_951_424,
            filesystem: Some(FsKind::Fat32),
            filesystem_name: Some("MS-DOS FAT32".into()),
            label: Some("STICK".into()),
            mount: Some(mount.to_path_buf()),
            cluster_size: Some(32768),
            uuid: Some("1234-ABCD".into()),
        }],
    }
}

/// Tracks of 4 GB or more (which a OneLibrary database, unlike export.pdb, can hold) are
/// counted in a warning that they won't fit when the plan is FAT32, and only then.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
#[cfg(any(target_os = "macos", windows))]
fn prop_plans_count_the_tracks_fat32_cant_hold(tc: TestCase) {
    use crate::common;
    use locallibrary::format::{self, Filesystem, FormatOptions};
    use locallibrary::paths;
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let mut lib = common::library(&root);
    for t in &mut lib.tracks {
        t.file_size = tc.draw(gs::sampled_from(vec![1234u64, (1 << 32) - 1, 1 << 32, 9 << 30]));
    }
    std::fs::write(root.join(paths::LIBRARY_PLUS_DB), common::onelibrary_for(&lib).to_encrypted().unwrap()).unwrap();
    for key in [paths::EXPORT_PDB, paths::EXPORT_EXT_PDB] {
        let _ = std::fs::remove_file(root.join(key));
    }
    let too_big = lib.tracks.iter().filter(|t| t.file_size >= 1 << 32).count();
    let filesystem = tc.draw(gs::sampled_from(vec![Filesystem::Fat32, Filesystem::Exfat]));
    let opts = FormatOptions { filesystem: Some(filesystem), ..Default::default() };
    let p = format::plan(stick(&root), &opts, None, &[], &Ctx::silent()).unwrap();
    assert_eq!(p.erases[0].tracks, Some(lib.tracks.len()));
    let warned: Vec<&String> = p.warnings.iter().filter(|w| w.contains("won't fit on FAT32")).collect();
    if filesystem == Filesystem::Fat32 && too_big > 0 {
        assert_eq!(warned.len(), 1, "{:?}", p.warnings);
        assert!(warned[0].starts_with(&format!("{too_big} track(s) on ")), "{}", warned[0]);
    } else {
        assert!(warned.is_empty(), "{:?}", p.warnings);
    }
}

/// Applying a format plan erases nothing unless the disk is still the planned one, no
/// locallibrary operation holds a drive on it, and it isn't cancelled: each refuses it with its
/// own error before the disk is asked anything, leaving the drive as it was. With a store, the
/// plan says the library is safe in it exactly when nothing was exported to the drive since its
/// pull; without one, that nobody knows.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
#[cfg(any(target_os = "macos", windows))]
fn prop_applying_refuses_before_the_disk_is_asked_anything(tc: TestCase) {
    use crate::common;
    use locallibrary::Error;
    use locallibrary::device::Device;
    use locallibrary::format::{self, FormatOptions};
    use locallibrary::progress::{CancelToken, NoProgress};
    use locallibrary::sync::{self, PullOptions};
    let dir = tempfile::tempdir().unwrap();
    let store = locallibrary::Store::open_or_init(dir.path().join("store")).unwrap();
    let mut main = Device::open(common::demo_drive(dir.path(), "Main")).unwrap();
    main.adopt(Some("Main")).unwrap();
    let pull = PullOptions { library: None, message: None, force: false };
    sync::pull(&store, &mut main, &pull, &Ctx::silent()).unwrap();
    let unsaved = tc.draw(gs::booleans());
    if unsaved {
        common::export_new_track(main.root(), "Artist", "Unsaved");
    }
    let disk = stick(main.root());
    let opts = FormatOptions::default();
    let store = tc.draw(gs::booleans()).then_some(&store);
    let plan = format::plan(disk.clone(), &opts, store, &[], &Ctx::silent()).unwrap();
    assert_eq!(plan.erases[0].in_store, store.map(|_| !unsaved));
    let warned = |what: &str| plan.warnings.iter().any(|w| w.contains(what));
    assert_eq!(warned("isn't in the store"), store.is_some() && unsaved, "{:?}", plan.warnings);
    assert_eq!(warned("Everything on it will be lost"), store.is_none(), "{:?}", plan.warnings);
    assert_eq!(plan.confirmation(), "Main");

    let before = common::Before::take(main.root());
    let cancel = CancelToken::new();
    let ctx = Ctx::new(&NoProgress, cancel.clone());
    let refusal = tc.draw(gs::integers::<u8>().max_value(2));
    let got = match refusal {
        // Another disk under the same identifier, or this one with something else on it.
        0 => {
            let mut other = disk.clone();
            match tc.draw(gs::integers::<u8>().max_value(2)) {
                0 => other.serial = Some("4C530003".into()),
                1 => other.size += 512,
                _ => other.volumes[0].label = Some("OTHER".into()),
            }
            format::apply(other, &plan.identity, &opts, store, &[], &ctx)
        }
        1 => {
            let _syncing = main.lock().unwrap();
            format::apply(disk, &plan.identity, &opts, store, &[], &ctx)
        }
        _ => {
            cancel.cancel();
            format::apply(disk, &plan.identity, &opts, store, &[], &ctx)
        }
    };
    match (refusal, &got) {
        (0, Err(Error::StalePlan(_))) | (1, Err(Error::Locked(_))) | (2, Err(Error::Cancelled)) => {}
        _ => panic!("refusal {refusal}: {:?}", got.as_ref().map(|r| &r.disk)),
    }
    before.untouched(main.root()).unwrap();
}

/// A disk image or virtual disk is never formatted, whatever else it is: it is refused for being
/// one, or for something worse (the running system on it, an internal disk).
#[hegel::test(test_cases = 100)]
#[cfg(any(target_os = "macos", windows))]
fn prop_disk_images_are_never_formatted(tc: TestCase) {
    let mut disk = stick(std::path::Path::new("/locallibrary-nowhere"));
    disk.virtual_disk = true;
    (disk.removable, disk.internal, disk.system, disk.read_only) =
        (tc.draw(gs::booleans()), tc.draw(gs::booleans()), tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    let problem = disk.problem(&[]).expect("refused");
    let image = !disk.system && disk.removable && !disk.internal;
    assert_eq!(problem == "it's a disk image or virtual disk", image, "{problem}");
}
