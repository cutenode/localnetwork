//! Properties of the FAT32 formatter's layout, and a few volumes checked with an independent FAT
//! implementation.

use std::fs::OpenOptions;
use std::io::{Read, Write};

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use locallibrary::format::fat32::{self, Layout, MAX_CLUSTER_SIZE, MAX_CLUSTERS, MIN_CLUSTERS};
use locallibrary::progress::Ctx;

const GIB: u64 = 1 << 30;

fn sector_size(tc: &TestCase) -> u32 {
    tc.draw(gs::sampled_from(vec![512u32, 1024, 2048, 4096]))
}

/// A cluster size FAT32 accepts with `bps`-byte sectors.
fn cluster_size(tc: &TestCase, bps: u32) -> u32 {
    let shift =
        tc.draw(gs::integers::<u32>().min_value(bps.trailing_zeros()).max_value(MAX_CLUSTER_SIZE.trailing_zeros()));
    1 << shift
}

/// Every layout the planner accepts is self-consistent: clusters start on a cluster boundary, the
/// FAT has an entry for every cluster (plus the two reserved ones) and is at most one sector
/// larger than the smallest that does (the planner rounds a fractional cluster count up),
/// everything fits in the volume, the cluster count makes it FAT32, and the boot sector's 16-
/// and 8-bit fields can hold the values.
#[hegel::test(test_cases = 1000)]
fn layouts_are_consistent(tc: TestCase) {
    let bps = sector_size(&tc);
    let cluster = cluster_size(&tc, bps);
    let volume = tc.draw(hegel::one_of!(
        gs::integers::<u64>().max_value(20 << 40),
        gs::integers::<u64>().min_value(64 << 20).max_value(2 << 40),
    ));
    let hidden = tc.draw(gs::integers::<u32>());
    let Ok(l) = Layout::new(volume, bps, cluster, hidden) else { return };
    let (spc, eps) = (u64::from(l.sectors_per_cluster), u64::from(bps / 4));
    let (fat, clusters) = (u64::from(l.fat_sectors), u64::from(l.clusters));
    assert_eq!(l.cluster_size(), cluster);
    assert_eq!(u64::from(l.total_sectors), volume / u64::from(bps));
    assert_eq!(l.data_start() % spc, 0, "data region is cluster-aligned");
    assert!(fat * eps >= clusters + 2, "FAT covers every cluster");
    // Two sectors less could not cover what the remaining space holds (before alignment padding).
    if let Some(smaller) = fat.checked_sub(2) {
        let unpadded = (u64::from(l.total_sectors) - 32 - 2 * smaller) / spc;
        assert!(smaller * eps < unpadded + 2, "FAT is within a sector of minimal: {l:?}");
    }
    assert!(l.data_start() + clusters * spc <= u64::from(l.total_sectors), "fits the volume");
    assert!(u64::from(l.total_sectors) - (l.data_start() + clusters * spc) < spc, "no whole cluster wasted");
    assert!((MIN_CLUSTERS..=MAX_CLUSTERS).contains(&clusters));
    assert!(l.reserved_sectors >= 32 && l.reserved_sectors <= u32::from(u16::MAX));
    assert!(l.sectors_per_cluster <= u32::from(u8::MAX));
    assert_eq!(l.hidden_sectors, hidden);
    assert_eq!(l.capacity(), clusters * u64::from(cluster));
}

/// The default cluster size works for every drive between 1 GiB and FAT32's 2 TiB limit.
#[hegel::test(test_cases = 500)]
fn default_cluster_size_always_fits(tc: TestCase) {
    let volume = tc.draw(gs::integers::<u64>().min_value(GIB).max_value((2 << 40) - 512));
    let cluster = fat32::default_cluster_size(volume);
    let l = Layout::new(volume, 512, cluster, 2048).unwrap();
    assert!(u64::from(l.clusters) >= MIN_CLUSTERS);
}

/// Formatted volumes (sparse files) open as FAT32 in `fatfs` with the right label and cluster
/// size, report every cluster but the root directory's free, and hold a file that reads back.
#[hegel::test(test_cases = 10)]
fn fatfs_reads_formatted_volumes(tc: TestCase) {
    let cluster = cluster_size(&tc, 512);
    let clusters = tc.draw(gs::integers::<u64>().min_value(MIN_CLUSTERS + 64).max_value(300_000));
    let slack = tc.draw(gs::integers::<u64>().max_value(u64::from(cluster) * 3));
    let label = tc.draw(gs::text().max_size(14));
    let volume_id = tc.draw(gs::integers::<u32>());
    // Room for the clusters plus the reserved area and FATs.
    let size = clusters * u64::from(cluster) + 64 * 512 + 2 * (clusters * 4).div_ceil(512) * 512 + slack;
    let layout = Layout::new(size, 512, cluster, 2048).unwrap();

    let file = tempfile::NamedTempFile::new().unwrap();
    file.as_file().set_len(size).unwrap();
    let mut f = OpenOptions::new().read(true).write(true).open(file.path()).unwrap();
    fat32::write(&mut f, 0, &layout, &label, volume_id, &Ctx::silent()).unwrap();
    drop(f);

    let f = OpenOptions::new().read(true).write(true).open(file.path()).unwrap();
    let fs = fatfs::FileSystem::new(f, fatfs::FsOptions::new()).unwrap();
    assert_eq!(fs.fat_type(), fatfs::FatType::Fat32);
    assert_eq!(fs.cluster_size(), cluster);
    assert_eq!(fs.volume_id(), volume_id);
    let expected = fat32::sanitize_label(&label, "NO NAME");
    assert_eq!(fs.volume_label().trim_end(), expected);
    assert_eq!(fs.read_volume_label_from_root_dir().unwrap().as_deref().map(str::trim_end), Some(expected.as_str()));
    let stats = fs.stats().unwrap();
    assert_eq!(stats.total_clusters(), layout.clusters);
    assert_eq!(stats.free_clusters(), layout.clusters - 1);

    let data: Vec<u8> = (0..3 * cluster + 17).map(|i| (i % 251) as u8).collect();
    {
        let root = fs.root_dir();
        root.create_dir("PIONEER").unwrap().create_file("export.pdb").unwrap().write_all(&data).unwrap();
        let mut back = Vec::new();
        root.open_file("PIONEER/export.pdb").unwrap().read_to_end(&mut back).unwrap();
        assert_eq!(back, data);
    }
    fs.unmount().unwrap();
}

/// What a format plan destroys, volume by volume: each rekordbox library on the disk with its
/// track count, the adopted drive's name (which the drive keeps, and which confirms the erase),
/// and whether the library is safe in the store, with a warning for every one that isn't. A
/// disk holding a protected path is refused.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
#[cfg_attr(
    target_os = "linux",
    ignore = "a plan on Linux asks lsblk what's mounted on the disk, and this one isn't there"
)]
fn plans_list_the_libraries_they_erase(tc: TestCase) {
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

/// A layout is refused, as invalid, for a sector size other than the four drives use, and for
/// a cluster size that isn't a power of two from the sector size to 32 KB, whatever the volume.
#[hegel::test(test_cases = 300)]
fn layouts_refuse_sectors_and_clusters_fat32_cant_take(tc: TestCase) {
    let volume = tc.draw(gs::integers::<u64>().min_value(64 << 20).max_value(2 << 40));
    let hidden = tc.draw(gs::integers::<u32>());
    let good_bps = sector_size(&tc);
    let bps = tc.draw(hegel::one_of!(gs::just(good_bps), gs::integers::<u32>().max_value(1 << 16)));
    let cluster = tc.draw(hegel::one_of!(
        gs::integers::<u32>().max_value(20).map(|s| 1u32 << s),
        gs::integers::<u32>().max_value(1 << 20),
    ));
    let sector_ok = [512, 1024, 2048, 4096].contains(&bps);
    let cluster_ok = cluster.is_power_of_two() && cluster >= bps && cluster <= 32 << 10;
    match Layout::new(volume, bps, cluster, hidden) {
        Ok(l) => {
            assert!(sector_ok && cluster_ok, "{bps}-byte sectors, {cluster}-byte clusters: {l:?}");
            assert_eq!((l.bytes_per_sector, l.cluster_size()), (bps, cluster));
        }
        Err(e) => {
            assert_eq!(e.code(), "invalid", "{e}");
            let msg = e.to_string();
            if !sector_ok {
                assert!(msg.contains(&format!("unsupported sector size {bps}")), "{msg}");
            } else if !cluster_ok {
                assert!(msg.contains("power of two") && msg.ends_with(&format!("not {cluster}")), "{msg}");
            }
        }
    }
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
fn plans_count_the_tracks_fat32_cant_hold(tc: TestCase) {
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
    let filesystem = tc.draw(gs::sampled_from(vec![Filesystem::Fat32, Filesystem::Exfat]).print_as_debug());
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
fn applying_refuses_before_the_disk_is_asked_anything(tc: TestCase) {
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
