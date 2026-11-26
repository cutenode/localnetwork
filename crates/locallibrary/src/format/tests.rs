use super::*;
#[cfg(unix)]
use loclib_format::test_util::FakeTools;
use loclib_format::test_util::{FS_KINDS, TEST_IMAGE, any_disk, name, sample_disk, volume};

/// A drive folder in `dir` named `name`, holding the demo rekordbox export.
#[cfg(unix)]
pub(crate) fn demo_drive(dir: &Path, name: &str) -> PathBuf {
    let demo = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rekordcrate/demo_tracks");
    let root = dir.join(name);
    for e in walkdir::WalkDir::new(&demo).into_iter().map(Result::unwrap) {
        let to = root.join(e.path().strip_prefix(&demo).unwrap());
        if e.file_type().is_dir() {
            std::fs::create_dir_all(&to).unwrap();
        } else {
            std::fs::copy(e.path(), &to).unwrap();
        }
    }
    root
}

/// Regression: the disk was compared with the plan once, before planning again and the
/// capacity test, and erased by its identifier without looking again.
#[test]
fn the_disk_is_checked_again_just_before_it_is_erased() {
    let planned = sample_disk();
    assert!(still_the_disk(&planned, Some(&planned.clone()), true).is_ok());
    let mut relabelled = planned.clone();
    relabelled.volumes[0].label = Some("OTHER".into());
    assert!(matches!(still_the_disk(&planned, Some(&relabelled), true), Err(Error::StalePlan(_))));
    // After the capacity test, which rewrote what was on it, only the disk itself counts.
    relabelled.volumes.clear();
    assert!(still_the_disk(&planned, Some(&relabelled), false).is_ok());
    let swapped = Disk { serial: Some("4C530002".into()), ..planned.clone() };
    assert!(still_the_disk(&planned, Some(&swapped), false).is_err());
    assert!(still_the_disk(&planned, None, false).is_err(), "gone");
}

/// Regression: the CLI asked for a volume label when the disk held no adopted drive, the very
/// word that may have found an unrelated disk.
#[test]
fn erasing_is_confirmed_by_the_drives_name_or_the_disks_identifier() {
    let disk = sample_disk();
    let plan = FormatPlan {
        identity: disk.identity(),
        filesystem: Filesystem::Fat32,
        scheme: Scheme::Mbr,
        label: "DNA".into(),
        cluster_size: 32768,
        partition_size: disk.partition_size(),
        erases: Vec::new(),
        keeps_name: None,
        compatibility: players::assess_disk(&disk),
        steps: Vec::new(),
        warnings: Vec::new(),
        protected: Vec::new(),
        disk,
    };
    assert_eq!(plan.disk.volumes[0].label.as_deref(), Some("DNA"));
    assert_eq!(plan.confirmation(), "disk4", "not the label");
    let named = FormatPlan { keeps_name: Some("Backup".into()), ..plan };
    assert_eq!(named.confirmation(), "Backup");
}

/// The plan says formatting hasn't run on Linux or Windows, so the sidecar and the terminal
/// player show it too (only the CLI used to).
#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "a plan on Linux asks lsblk what's mounted on the disk, and the sample disk isn't there"
)]
fn plans_warn_where_formatting_is_untested() {
    let warnings = |os, check_capacity| {
        THREAD_OS.with(|o| o.set(os));
        let opts = FormatOptions { check_capacity, ..Default::default() };
        let w = plan(sample_disk(), &opts, None, &[], &Ctx::silent()).map(|p| p.warnings);
        THREAD_OS.with(|o| o.set(None));
        w.unwrap()
    };
    let untested = |w: Vec<String>| w.into_iter().filter(|w| w.contains("tested")).collect::<Vec<_>>();
    assert_eq!(
        untested(warnings(Some("linux"), false)),
        ["formatting hasn't been tested on Linux yet; check the result"]
    );
    // Windows itself refuses the capacity check, so there the plan is made without one.
    let checked = !cfg!(windows);
    let what = if checked { "formatting with a capacity check" } else { "formatting" };
    assert_eq!(
        untested(warnings(Some("windows"), checked)),
        [format!("{what} hasn't been tested on Windows yet; check the result")]
    );
    assert!(untested(warnings(Some("macos"), false)).is_empty());
}

/// Only external and removable physical disks are listed, each with why it can't be
/// formatted when it can't.
#[test]
fn lists_only_external_and_removable_disks() {
    let disks = vec![
        sample_disk(),
        Disk { id: "disk0".into(), internal: true, removable: false, ..sample_disk() },
        Disk { id: "disk1".into(), removable: false, ..sample_disk() },
        Disk { id: "disk9".into(), virtual_disk: true, ..sample_disk() },
        Disk { id: "disk5".into(), read_only: true, ..sample_disk() },
    ];
    let views = views(disks, &[PathBuf::from("/Volumes/DNA/store")]);
    let ids: Vec<&str> = views.iter().map(|v| v.disk.id.as_str()).collect();
    assert_eq!(ids, ["disk4", "disk5"]);
    assert_eq!(views[0].identity, views[0].disk.identity());
    assert_eq!(views[0].compatibility.verdict, Verdict::AllPlayers);
    assert_eq!(views[0].problem.as_deref(), Some("it holds /Volumes/DNA/store"));
    assert_eq!(views[1].problem.as_deref(), Some("it's write-protected"));
}

/// The disk image a test attached is never listed: tests name it by its identifier.
#[test]
fn the_tests_own_image_is_not_listed() {
    let image = Disk { virtual_disk: true, internal: true, removable: false, ..sample_disk() };
    TEST_IMAGE.set(Some(image.id.clone()));
    let listed = views(vec![image], &[]);
    TEST_IMAGE.set(None);
    assert!(listed.is_empty());
}

/// Refused before anything is asked of the system's tools: more than MBR can address, a
/// cluster size the file system can't take, a FAT32 volume too small for its clusters.
#[test]
fn plans_refuse_what_cant_be_formatted() {
    let plan_of = |disk: Disk, opts: FormatOptions| plan(disk, &opts, None, &[], &Ctx::silent()).unwrap_err();
    let huge = Disk { size: 3_000_000_000_000, ..sample_disk() };
    assert!(plan_of(huge, FormatOptions::default()).to_string().contains("2 TiB"));
    let odd = FormatOptions { cluster_size: Some(3000), ..Default::default() };
    assert!(plan_of(sample_disk(), odd).to_string().contains("power of two"));
    let small = Disk { size: 1_000_000_000, ..sample_disk() };
    let big_clusters = FormatOptions { cluster_size: Some(32768), ..Default::default() };
    assert!(plan_of(small, big_clusters).to_string().contains("too small for FAT32"));
    let system = Disk { system: true, ..sample_disk() };
    assert!(plan_of(system, FormatOptions::default()).to_string().contains("refusing to format"));
}

/// A plan made for one disk formats nothing on another.
#[test]
fn applying_needs_the_planned_disks_identity() {
    let disk = sample_disk();
    let err = apply(disk, "0123456789abcdef", &FormatOptions::default(), None, &[], &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::StalePlan(_)), "{err}");
}

// -----------------------------------------------------------------------------------------
// Properties
// -----------------------------------------------------------------------------------------

use hegel::generators as gs;
use hegel::{Generator, TestCase};

/// A disk's identity changes with what is on it (its id, model, serial, size, and each
/// volume's id, uuid, label, size and file system) and not with where volumes are mounted; the
/// disk-only fingerprint ignores the volumes. The check before erasing agrees with it.
#[hegel::test(test_cases = 300)]
fn prop_identity_follows_what_is_on_the_disk(tc: TestCase) {
    let disk = any_disk(&tc);
    let mut other = disk.clone();
    let kind = tc.draw(gs::integers::<u8>().max_value(9));
    let has_volumes = !other.volumes.is_empty();
    let i = if has_volumes { tc.draw(gs::integers::<usize>().max_value(other.volumes.len() - 1)) } else { 0 };
    // What each change leaves alone: (the identity, the disk-only fingerprint).
    let (same_identity, same_disk) = match (kind, has_volumes) {
        (0, _) => {
            for v in &mut other.volumes {
                v.mount = v.mount.take().xor(Some("/locallibrary-nowhere/elsewhere".into()));
            }
            (true, true)
        }
        (1, _) => {
            other.id.push('0');
            (false, false)
        }
        (2, _) => {
            other.model.push('x');
            (false, false)
        }
        (3, _) => {
            other.serial = Some(format!("{}x", other.serial.unwrap_or_default()));
            (false, false)
        }
        (4, _) => {
            other.size ^= 1;
            (false, false)
        }
        (5, true) => {
            other.volumes[i].id.push('0');
            (false, true)
        }
        (6, true) => {
            other.volumes[i].uuid = Some(format!("{}x", other.volumes[i].uuid.take().unwrap_or_default()));
            (false, true)
        }
        (7, true) => {
            other.volumes[i].label = Some(format!("{}x", other.volumes[i].label.take().unwrap_or_default()));
            (false, true)
        }
        (8, true) => {
            other.volumes[i].size ^= 1;
            (false, true)
        }
        (9, true) => {
            let was = other.volumes[i].filesystem;
            other.volumes[i].filesystem =
                Some(tc.draw(gs::sampled_from(FS_KINDS.to_vec()).filter(move |k| Some(*k) != was)));
            (false, true)
        }
        _ => {
            other.volumes.push(volume(&tc, &disk.id, 9));
            (false, true)
        }
    };
    assert_eq!(disk.identity() == other.identity(), same_identity, "change {kind}");
    assert_eq!(disk.fingerprint(false) == other.fingerprint(false), same_disk, "change {kind}");
    assert_eq!(still_the_disk(&disk, Some(&other), true).is_ok(), same_identity);
    assert_eq!(still_the_disk(&disk, Some(&other), false).is_ok(), same_disk);
    assert!(matches!(still_the_disk(&disk, None, false), Err(Error::StalePlan(_))));
    assert_eq!(disk.identity().len(), 16);
}

/// A disk may be formatted exactly when it is external or removable, physical, writable, and
/// holds neither the system nor a protected path; when it holds one, that path is under one of
/// its mount points. Listing keeps exactly the external and removable physical disks, as they
/// are.
#[hegel::test(test_cases = 300)]
fn prop_disks_are_refused_for_exactly_their_problems(tc: TestCase) {
    let disks: Vec<Disk> = (0..tc.draw(gs::integers::<usize>().max_value(3))).map(|_| any_disk(&tc)).collect();
    let protected: Vec<PathBuf> = tc
        .draw(
            gs::vecs(gs::sampled_from(vec!["/locallibrary-nowhere/a/store", "/locallibrary-nowhere/b", "/Users/x"]))
                .max_size(2),
        )
        .into_iter()
        .map(PathBuf::from)
        .collect();
    for d in &disks {
        let held = d.holding(&protected);
        if let Some((mount, path)) = held {
            assert!(path.starts_with(mount) && protected.contains(path));
            assert!(d.volumes.iter().any(|v| v.mount.as_deref() == Some(mount)));
        } else {
            assert!(
                !d.volumes
                    .iter()
                    .filter_map(|v| v.mount.as_deref())
                    .any(|m| protected.iter().any(|p| p.starts_with(m)))
            );
        }
        let root = d.volumes.iter().any(|v| v.mount.as_deref() == Some(Path::new("/")));
        let fine =
            !d.system && !d.internal && d.removable && !d.virtual_disk && !d.read_only && !root && held.is_none();
        assert_eq!(d.problem(&protected).is_none(), fine, "{d:?}");
        assert!(describe(d).contains(&d.id));
        let usable = d.size.saturating_sub(loclib_format::PARTITION_START);
        let p = d.partition_size();
        assert!(p <= usable && usable - p < u64::from(d.sector_size) && p % u64::from(d.sector_size) == 0);
    }
    let shown = views(disks.clone(), &protected);
    let kept: Vec<&Disk> = disks.iter().filter(|d| d.removable && !d.internal && !d.virtual_disk).collect();
    assert_eq!(shown.len(), kept.len());
    for (v, d) in shown.iter().zip(kept) {
        assert_eq!((v.disk.identity(), &v.identity, &v.problem), (d.identity(), &d.identity(), &d.problem(&protected)));
        assert_eq!(v.compatibility.verdict, players::assess_disk(d).verdict);
    }
}

/// A plan is made exactly when the disk may be formatted, fits MBR, and the cluster size suits
/// the file system and the volume; it then describes that disk, its whole partition, a label
/// the file system takes, a step list, and a warning for each unmounted volume it destroys.
/// Applying it to any other identity changes nothing.
#[hegel::test(test_cases = 200)]
#[cfg_attr(
    target_os = "linux",
    ignore = "a plan on Linux asks lsblk what's mounted on the disk, and the generated disk isn't there"
)]
fn prop_plans_follow_the_disk_and_the_options(tc: TestCase) {
    let disk = any_disk(&tc);
    let opts = FormatOptions {
        filesystem: tc.draw(gs::sampled_from(vec![
            None,
            Some(Filesystem::Fat32),
            Some(Filesystem::Exfat),
            Some(Filesystem::Hfsplus),
        ])),
        label: if tc.draw(gs::booleans()) { Some(name(&tc)) } else { None },
        cluster_size: tc.draw(gs::sampled_from(vec![
            None,
            Some(512),
            Some(4096),
            Some(32768),
            Some(65536),
            Some(3000),
        ])),
        check_capacity: tc.draw(gs::booleans()),
    };
    let os = tc.draw(gs::sampled_from(vec![None, Some("linux"), Some("windows"), Some("macos")]));
    THREAD_OS.with(|o| o.set(os));
    let made = plan(disk.clone(), &opts, None, &[], &Ctx::silent());
    THREAD_OS.with(|o| o.set(None));

    let fs = opts.filesystem.unwrap_or(Filesystem::Fat32);
    let part = disk.partition_size();
    let cluster = opts.cluster_size.unwrap_or_else(|| fs.default_cluster_size(part));
    let fits = disk.problem(&[]).is_none()
        && disk.size < loclib_equipment::MBR_MAX
        && fs.check_cluster_size(cluster, disk.sector_size).is_ok()
        && (fs != Filesystem::Fat32 || fat32::Layout::new(part, disk.sector_size, cluster, 0).is_ok())
        // Windows itself makes no HFS+ and checks no capacity.
        && !(cfg!(windows) && (fs == Filesystem::Hfsplus || opts.check_capacity));
    let p = match made {
        Ok(p) => p,
        Err(e) => {
            assert!(!fits, "{e}");
            assert_eq!(e.code(), "invalid", "{e}");
            return;
        }
    };
    assert!(fits);
    assert_eq!((p.identity.as_str(), p.filesystem, p.scheme), (disk.identity().as_str(), fs, Scheme::Mbr));
    assert_eq!((p.cluster_size, p.partition_size, p.confirmation()), (cluster, part, disk.id.as_str()));
    assert_eq!(fs.label(&p.label), p.label);
    if let Some(l) = &opts.label {
        assert_eq!(p.label, fs.label(l));
    }
    assert_eq!(p.erases.len(), disk.volumes.len());
    assert!(p.erases.iter().all(|e| e.drive_name.is_none() && e.tracks.is_none() && e.in_store.is_none()));
    let unmounted =
        disk.volumes.iter().filter(|v| v.mount.is_none() && v.size > 0 && v.label.as_deref() != Some("EFI"));
    let warned = p.warnings.iter().filter(|w| w.contains("isn't mounted")).count();
    assert_eq!(warned, unmounted.count());
    let untested = p.warnings.iter().any(|w| w.contains("hasn't been tested on"));
    assert_eq!(untested, matches!(os.unwrap_or(std::env::consts::OS), "linux" | "windows"));
    let all = p.compatibility.verdict == Verdict::AllPlayers;
    assert_eq!(p.warnings.iter().any(|w| w.contains("won't work on every player")), !all);
    assert_eq!(all, fs != Filesystem::Exfat);
    assert!(!p.steps.is_empty());
    assert_eq!(p.steps[0].starts_with("test the real capacity"), opts.check_capacity);
    let wrong = format!("{}x", p.identity);
    assert!(matches!(apply(disk, &wrong, &opts, None, &[], &Ctx::silent()), Err(Error::StalePlan(_))));
}

/// A disk players could use but for its size is refused when MBR, which players need, can't
/// address all of it, naming the limit; one a byte smaller isn't refused for its size.
#[hegel::test(test_cases = 100)]
#[cfg_attr(
    target_os = "linux",
    ignore = "a plan on Linux asks lsblk what's mounted on the disk, and the generated disk isn't there"
)]
fn prop_disks_mbr_cant_address_are_refused(tc: TestCase) {
    let excess =
        tc.draw(hegel::one_of!(gs::integers::<u64>().max_value(1 << 20), gs::integers::<u64>().max_value(1 << 44)));
    let disk = Disk { size: loclib_equipment::MBR_MAX + excess, volumes: Vec::new(), ..sample_disk() };
    let e = plan(disk.clone(), &FormatOptions::default(), None, &[], &Ctx::silent()).unwrap_err();
    assert_eq!(e.code(), "invalid", "{e}");
    assert!(e.to_string().contains("larger than the 2 TiB an MBR partition table can address"), "{e}");
    let fits = Disk { size: loclib_equipment::MBR_MAX - 1, ..disk };
    if let Err(e) = plan(fits, &FormatOptions::default(), None, &[], &Ctx::silent()) {
        assert!(!e.to_string().contains("2 TiB"), "{e}");
    }
}

/// Regression: under `$LOCALLIBRARY_VOLUMES`, the disk holding a drive folder was still looked up
/// with the system's disk tools (`diskutil info`, `lsblk`), though that disk is this machine's
/// own. It's refused without running any, as listing and formatting are.
#[cfg(unix)]
#[test]
fn the_test_switch_runs_no_disk_tool_for_a_drive_folder() {
    let mut fake = FakeTools::new();
    let root = fake.dir().join("DNA");
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    fake.volume_at(&root, "vfat", true);
    assert!(disk_at(&root).is_ok(), "the fake tools answer for it");
    let before = fake.calls().len();
    crate::testing::volumes_on_this_thread(Some(Vec::new()));
    let hidden = disk_at(&root);
    crate::testing::volumes_on_this_thread(None);
    assert!(matches!(&hidden, Err(Error::Invalid(m)) if m.starts_with("LOCALLIBRARY_VOLUMES is set")), "{hidden:?}");
    assert_eq!(fake.calls().len(), before, "no tool ran");
}

/// Applying a plan that checks the capacity first unmounts the whole disk with diskutil, in its
/// own words when it can't, and then opens the disk's raw device: one that isn't there is an I/O
/// error naming it. Nothing is erased either way. The disk's identifier names no device on this
/// Mac, so nothing real is opened.
#[cfg(target_os = "macos")]
#[hegel::test(test_cases = 10)]
fn prop_a_capacity_check_unmounts_the_disk_before_opening_it(tc: TestCase) {
    let mut disk = sample_disk();
    disk.id = format!("disk{}", tc.draw(gs::integers::<u32>().min_value(900).max_value(999)));
    disk.volumes[0].mount = None;
    let raw = format!("/dev/r{}", disk.id);
    tc.assume(!Path::new(&raw).exists());
    let mut fake = FakeTools::new();
    let unmounts = tc.draw(gs::booleans());
    let unmount = ["unmountDisk", disk.id.as_str()];
    match unmounts {
        true => fake.answer("diskutil", &unmount, ""),
        false => {
            fake.refuse("diskutil", &unmount, "Unmount of disk failed: at least one volume could not be unmounted")
        }
    }
    let opts = FormatOptions { check_capacity: true, ..Default::default() };
    let identity = disk.identity();
    let applied = apply(disk.clone(), &identity, &opts, None, &[], &Ctx::silent());
    assert_eq!(fake.calls(), [Cmd::new("diskutil", &unmount)], "nothing but the unmount ran");
    match (unmounts, applied) {
        (true, Err(Error::Io { path, .. })) => assert_eq!(path, Path::new(&raw)),
        (false, Err(Error::External { message, .. })) => assert!(message.starts_with("Unmount of disk failed")),
        (_, other) => panic!("{other:?}"),
    }
}

/// Applying a plan formats the disk it was made for and no other: diskutil is asked about the
/// disk again just before the erase, and when what is on it changed since the plan (its volume
/// relabelled or resized, another disk under its identifier) nothing is erased and the plan is
/// stale. Otherwise the planned erase runs, and the drive that was on the disk keeps its name and
/// identity on the new volume.
#[cfg(target_os = "macos")]
#[hegel::test(test_cases = 15)]
fn prop_a_plan_formats_only_the_disk_it_was_made_for(tc: TestCase) {
    use crate::device::Device;
    use serde_json::json;
    let mut fake = FakeTools::new();
    fake.echo("plutil", &["-convert", "json", "-o", "-", "--", "-"]);
    let (old, new) = (fake.dir().join("old"), fake.dir().join("new"));
    std::fs::create_dir(&old).unwrap();
    std::fs::create_dir(&new).unwrap();
    let name = tc.draw(gs::sampled_from(vec!["Main", "Backup", "Club"]));
    let mut drive = Device::open(&old).unwrap();
    let identity = drive.adopt(Some(name)).unwrap().id;
    let (old, new) = (drive.root().to_path_buf(), new.canonicalize().unwrap());

    let id = format!("disk{}", tc.draw(gs::integers::<u32>().min_value(2).max_value(40)));
    let pid = format!("{id}s1");
    let size = tc.draw(gs::integers::<u64>().min_value(1 << 30).max_value(1 << 40));
    let info = |size: u64| {
        json!({ "DeviceIdentifier": id, "ParentWholeDisk": id, "Content": "FDisk_partition_scheme", "MediaName": "Stick",
                "Size": size, "DeviceBlockSize": 512, "Internal": false, "RemovableMediaOrExternalDevice": true,
                "VirtualOrPhysical": "Physical", "BusProtocol": "USB", "WritableMedia": true })
        .to_string()
    };
    let listing = |label: &str, size: u64, mount: &Path| {
        let part = json!({ "DeviceIdentifier": pid, "Content": "Windows_FAT_32", "Size": size, "VolumeName": label,
                "MountPoint": mount });
        json!({ "AllDisksAndPartitions": [{ "DeviceIdentifier": id, "Content": "FDisk_partition_scheme",
                "Partitions": [part] }] })
    };
    let pinfo = json!({ "DeviceIdentifier": pid, "ParentWholeDisk": id, "FilesystemType": "msdos",
            "FilesystemName": "MS-DOS FAT32" });
    let psize = size - (1 << 20);
    // The disk as diskutil describes it from the answers above.
    let disk = Disk {
        id: id.clone(),
        device: format!("/dev/{id}"),
        model: "Stick".into(),
        serial: None,
        size,
        sector_size: 512,
        bus: Some("USB".into()),
        removable: true,
        internal: false,
        system: false,
        virtual_disk: false,
        read_only: false,
        scheme: Some(Scheme::Mbr),
        volumes: vec![Volume {
            id: pid.clone(),
            size: psize,
            filesystem: Some(FsKind::Fat32),
            filesystem_name: Some("MS-DOS FAT32".into()),
            label: Some("DNA".into()),
            mount: Some(old.clone()),
            cluster_size: None,
            uuid: None,
        }],
    };
    let opts = FormatOptions {
        filesystem: Some(tc.draw(gs::sampled_from(vec![Filesystem::Fat32, Filesystem::Exfat]))),
        ..FormatOptions::default()
    };
    let planned = plan(disk.clone(), &opts, None, &[], &Ctx::silent()).unwrap();
    assert_eq!(planned.keeps_name.as_deref(), Some(name));

    // What diskutil says just before the erase.
    let change = tc.draw(gs::sampled_from(vec!["nothing", "label", "volume size", "disk"]));
    let label = if change == "label" { "OTHER" } else { "DNA" };
    let now_psize = if change == "volume size" { psize - 512 } else { psize };
    fake.answer("diskutil", &["info", "-plist", &id], &info(if change == "disk" { size + 512 } else { size }));
    fake.answer("diskutil", &["list", "-plist", &id], &listing(label, now_psize, &old).to_string());
    fake.answer("diskutil", &["info", "-plist", &pid], &pinfo.to_string());
    let personality = if planned.filesystem == Filesystem::Fat32 { "MS-DOS FAT32" } else { "ExFAT" };
    let erase = Cmd::new("diskutil", &["eraseDisk", personality, &planned.label, "MBR", &id]);
    fake.then_after("diskutil", &erase.args.iter().map(String::as_str).collect::<Vec<_>>());
    // And after it: the new volume, mounted.
    fake.answer("diskutil", &["info", "-plist", &id], &info(size));
    fake.answer("diskutil", &["list", "-plist", &id], &listing(&planned.label, psize, &new).to_string());
    fake.answer("diskutil", &["info", "-plist", &pid], &pinfo.to_string());

    let applied = apply(disk, &planned.identity, &opts, None, &[], &Ctx::silent());
    let erased = fake.calls().contains(&erase);
    if change != "nothing" {
        assert!(matches!(applied, Err(Error::StalePlan(_))), "{change}: {applied:?}");
        assert!(!erased, "{change}");
        return;
    }
    let report = applied.unwrap();
    assert!(erased);
    assert_eq!((report.disk.as_str(), report.label.as_str()), (id.as_str(), planned.label.as_str()));
    assert_eq!((report.mount.as_deref(), report.drive_name.as_deref()), (Some(new.as_path()), Some(name)));
    let kept = Device::open(&new).unwrap();
    assert_eq!((kept.state().unwrap().id, kept.name()), (identity, name.to_string()));
}

/// A capacity test that found every region keeping its data lets the format go on; one that
/// found any failing refuses it as a fake drive, saying the disk, what it claims, how far it
/// works and how many regions failed.
#[hegel::test(test_cases = 100)]
fn prop_only_a_drive_the_capacity_test_found_fake_is_refused(tc: TestCase) {
    let disk = any_disk(&tc);
    let regions = tc.draw(gs::integers::<usize>().max_value(1024));
    let failed = tc.draw(gs::integers::<usize>().max_value(regions));
    let works_up_to = if failed > 0 { tc.draw(gs::optional(gs::integers::<u64>().max_value(disk.size))) } else { None };
    let report = capacity::ProbeReport { claimed: disk.size, regions, failed, works_up_to, aliases: Vec::new() };
    let refused = refuse_fake(&disk, &report);
    if failed == 0 {
        refused.unwrap();
        return;
    }
    let e = refused.unwrap_err();
    assert_eq!((e.code(), e.reason()), ("conflict", Some(Reason::FakeCapacity)));
    let works = crate::op::bytes(works_up_to.unwrap_or(0));
    let said = format!(
        "{} is fake: it reports {} but only about the first {works} keep their data ({failed} of {regions} test regions came back wrong).",
        describe(&disk),
        crate::op::bytes(disk.size)
    );
    assert!(e.to_string().starts_with(&said), "{e}");
}

/// A drive's name is never taken for a disk identifier, so nothing is asked of the system's tools
/// for it, and a disk that's gone (unplugged since it was listed) has no link speed.
#[hegel::test(test_cases = 20)]
fn prop_a_drives_name_is_no_disk_and_a_gone_disk_has_no_speed(tc: TestCase) {
    let spec = tc.draw(gs::sampled_from(vec!["Backup", "Club Stick", "DNA"]));
    assert!(disk_by_id(spec).unwrap().is_none(), "{spec}");
    let unplugged = tc.draw(gs::integers::<u16>());
    let gone = Disk { id: format!("gone{unplugged}"), device: format!("/dev/gone{unplugged}"), ..any_disk(&tc) };
    assert_eq!(link_speed(&gone), None);
}

/// Linux: applying a plan formats the disk it was made for and no other. lsblk is asked about the
/// disk again just before the erase, and when what is on it changed since the plan (its volume
/// relabelled or resized, another disk under its device) nothing is erased and the plan is stale.
/// Otherwise the planned steps run, in order, and nothing is mounted after (Linux formats as root,
/// with no desktop session to mount it for). A capacity check unmounts the volume first: when
/// that fails, nothing is erased either. The disk's device is a file in `/dev/shm`, as lsblk names
/// disks under `/dev/`.
#[cfg(all(unix, not(target_os = "macos")))]
#[hegel::test(test_cases = 15)]
#[hegel::explicit_test_case(size = 1u64 << 30, filesystem = Filesystem::Fat32, check_capacity = false, change = "nothing")]
#[hegel::explicit_test_case(size = 1u64 << 30, filesystem = Filesystem::Exfat, check_capacity = true, change = "nothing")]
fn prop_linux_formats_only_the_disk_the_plan_was_made_for(tc: TestCase) {
    use serde_json::json;
    let mut fake = FakeTools::new();
    let shm = tempfile::Builder::new().prefix("locallibrary-format").tempdir_in("/dev/shm").unwrap();
    let (dev, part) = (shm.path().join("sdb").display().to_string(), shm.path().join("sdb1").display().to_string());
    for f in [&dev, &part] {
        std::fs::write(f, "").unwrap();
    }
    let root = fake.dir().join("DNA");
    std::fs::create_dir(&root).unwrap();
    let size = tc.draw(gs::integers::<u64>().min_value(1 << 30).max_value(1 << 40));
    let psize = size - (1 << 20);
    let listing = |label: &str, psize: u64, size: u64| {
        let volume = json!({ "name": "sdb1", "path": part, "type": "part", "size": psize, "fstype": "vfat",
                "fsver": "FAT32", "label": label, "mountpoint": root });
        json!({ "blockdevices": [{ "name": "sdb", "path": dev, "type": "disk", "tran": "usb", "pttype": "dos",
                "model": "Stick", "size": size, "children": [volume] }] })
    };
    // The disk as lsblk describes it from the listing above.
    let disk = Disk {
        id: dev.clone(),
        device: dev.clone(),
        model: "Stick".into(),
        serial: None,
        size,
        sector_size: 512,
        bus: Some("USB".into()),
        removable: true,
        internal: false,
        system: false,
        virtual_disk: false,
        read_only: false,
        scheme: Some(Scheme::Mbr),
        volumes: vec![Volume {
            id: part.clone(),
            size: psize,
            filesystem: Some(FsKind::Fat32),
            filesystem_name: Some("vfat".into()),
            label: Some("DNA".into()),
            mount: Some(root.clone()),
            cluster_size: None,
            uuid: None,
        }],
    };
    let filesystem = tc.draw(gs::sampled_from(vec![Filesystem::Fat32, Filesystem::Exfat]));
    let check_capacity = tc.draw(gs::weighted_booleans(0.2));
    let opts = FormatOptions { filesystem: Some(filesystem), check_capacity, ..FormatOptions::default() };
    // What's mounted on the disk, for the plan's steps and the format's.
    fake.lsblk(Some(&dev), &listing("DNA", psize, size));
    let planned = plan(disk.clone(), &opts, None, &[], &Ctx::silent()).unwrap();
    assert_eq!(planned.keeps_name, None, "Linux mounts nothing to keep it on");

    // What lsblk says just before the erase.
    let change = tc.draw(gs::sampled_from(vec!["nothing", "label", "volume size", "disk"]));
    let label = if change == "label" { "OTHER" } else { "DNA" };
    let now_psize = if change == "volume size" { psize - 512 } else { psize };
    let now_size = if change == "disk" { size + 512 } else { size };
    fake.lsblk(None, &listing(label, now_psize, now_size));
    let now = disk_by_id(&dev).unwrap().unwrap();
    assert_eq!(now.identity() == disk.identity(), change == "nothing", "{change}");
    let root_str = root.display().to_string();
    match check_capacity {
        true => fake.refuse("umount", &[&root_str], "umount: target is busy."),
        false => fake.answer("umount", &[&root_str], ""),
    }
    let spc = (planned.cluster_size / 512).to_string();
    let cluster = planned.cluster_size.to_string();
    let mkfs = match filesystem {
        Filesystem::Fat32 => Cmd::new("mkfs.vfat", &["-F", "32", "-s", &spc, "-n", &planned.label, &part]),
        _ => Cmd::new("mkfs.exfat", &["-c", &cluster, "-L", &planned.label, &part]),
    };
    let sfdisk = ["--wipe", "always", "--wipe-partitions", "always", dev.as_str()];
    let quiet: [(&str, &[&str]); 3] = [("sfdisk", &sfdisk), ("udevadm", &["settle"]), ("sync", &[])];
    for (program, args) in quiet {
        fake.answer(program, args, "");
    }
    fake.answer(&mkfs.program, &mkfs.args.iter().map(String::as_str).collect::<Vec<_>>(), "");

    let applied = apply(disk, &planned.identity, &opts, None, &[], &Ctx::silent());
    let ran: Vec<String> = fake
        .calls()
        .iter()
        .filter(|c| !["lsblk", "udevadm", "sync"].contains(&c.program.as_str()))
        .map(Cmd::display)
        .collect();
    let erased = ran.iter().any(|c| c.starts_with("sfdisk"));
    if check_capacity {
        assert!(matches!(&applied, Err(Error::External { message, .. }) if message.contains("busy")), "{applied:?}");
        assert_eq!(ran, [format!("umount {root_str}")]);
        return;
    }
    if change != "nothing" {
        assert!(matches!(applied, Err(Error::StalePlan(_))), "{change}: {applied:?}");
        assert!(!erased, "{change}");
        return;
    }
    let report = applied.unwrap();
    assert_eq!(ran, planned.steps);
    assert_eq!((report.disk.as_str(), report.label.as_str()), (dev.as_str(), planned.label.as_str()));
    assert_eq!((report.mount, report.drive_name, report.filesystem), (None, None, filesystem));
}
