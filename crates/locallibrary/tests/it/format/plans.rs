//! Planning a format of a disk whose volume is a drive folder: what it erases, what it warns about
//! and what the drive keeps, and applying stopping before it asks anything of the disk. Not on
//! Linux, where a plan asks lsblk what is mounted on the disk.
#![cfg(any(target_os = "macos", windows))]

use std::path::Path;

use crate::common;

use locallibrary::device::Device;
use locallibrary::error::Reason;
use locallibrary::fleet::{self, Role};
use locallibrary::format::{self, Disk, Filesystem, FormatOptions, FormatPlan, FsKind, Scheme, Verdict, Volume};
use locallibrary::progress::{CancelToken, Ctx, NoProgress};
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::{Error, Result};

/// A 64 GB USB stick whose one FAT32 volume is mounted at `mount`. No disk answers to its
/// identifier, so a format that got as far as looking for it again would find nothing.
fn stick(mount: Option<&Path>) -> Disk {
    Disk {
        id: "disk999".into(),
        device: "/dev/disk999".into(),
        model: "Cruzer Glide".into(),
        serial: Some("4C530001".into()),
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
            id: "disk999s1".into(),
            size: 63_998_951_424,
            filesystem: Some(FsKind::Fat32),
            filesystem_name: Some("MS-DOS FAT32".into()),
            label: Some("STICK".into()),
            mount: mount.map(Path::to_path_buf),
            cluster_size: Some(32768),
            uuid: Some("1234-ABCD".into()),
        }],
    }
}

fn plan(disk: Disk, opts: &FormatOptions, store: Option<&Store>) -> Result<FormatPlan> {
    format::plan(disk, opts, store, &[], &Ctx::silent())
}

/// A store holding the demo drive `Main` (adopted, and pulled).
fn pulled(dir: &Path) -> (Store, Device) {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    let mut main = Device::open(common::demo_drive(dir, "Main")).unwrap();
    main.adopt(Some("Main")).unwrap();
    let opts = PullOptions { library: None, message: Some("before formatting".into()), force: false };
    sync::pull(&store, &mut main, &opts, &Ctx::silent()).unwrap();
    (store, main)
}

#[test]
fn a_library_that_isnt_in_a_store_is_lost() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let p = plan(stick(Some(&root)), &FormatOptions::default(), None).unwrap();
    let erased = &p.erases[0];
    assert_eq!((erased.tracks, erased.in_store, erased.drive_name.as_deref()), (Some(2), None, None));
    assert!(
        p.warnings.iter().any(|w| w.ends_with("holds a rekordbox library (2 tracks). Everything on it will be lost.")),
        "{:?}",
        p.warnings
    );
    // FAT32 on MBR, named as the volume was, confirmed by the disk's identifier.
    assert_eq!((p.filesystem, p.scheme, p.cluster_size), (Filesystem::Fat32, Scheme::Mbr, 32768));
    assert_eq!((p.label.as_str(), p.keeps_name.as_deref(), p.confirmation()), ("STICK", None, "disk999"));
    assert_eq!(p.compatibility.verdict, Verdict::AllPlayers);
    assert!(p.compatibility.notes.iter().any(|n| n == "FAT32 can't hold files of 4 GB or more"));
    assert!(!p.steps.is_empty());
}

#[test]
fn a_pulled_library_is_safe_and_the_drive_keeps_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let (store, main) = pulled(dir.path());
    fleet::set_role(&store, main.id().unwrap(), Some(Role::Backup)).unwrap();
    let p = plan(stick(Some(main.root())), &FormatOptions::default(), Some(&store)).unwrap();
    let erased = &p.erases[0];
    assert_eq!(
        (erased.in_store, erased.drive_name.as_deref(), erased.role),
        (Some(true), Some("Main"), Some(Role::Backup))
    );
    assert!(!p.warnings.iter().any(|w| w.contains("rekordbox library")), "{:?}", p.warnings);
    assert_eq!((p.keeps_name.as_deref(), p.confirmation(), p.label.as_str()), (Some("Main"), "Main", "MAIN"));

    // A track exported since the pull isn't in the store.
    common::export_new_track(main.root(), "Artist", "Unsaved");
    let p = plan(stick(Some(main.root())), &FormatOptions::default(), Some(&store)).unwrap();
    assert_eq!(p.erases[0].in_store, Some(false));
    assert!(
        p.warnings.iter().any(|w| w.ends_with("(3 tracks) that isn't in the store. Pull it first to keep a copy.")),
        "{:?}",
        p.warnings
    );
    let named = FormatOptions { label: Some("Club drive".into()), ..Default::default() };
    assert_eq!(plan(stick(Some(main.root())), &named, Some(&store)).unwrap().label, "CLUB DRIVE");
}

#[test]
fn archive_and_retired_drives_are_kept_as_they_are() {
    let dir = tempfile::tempdir().unwrap();
    let (store, main) = pulled(dir.path());
    let id = main.id().unwrap();
    fleet::set_role(&store, id, Some(Role::Archive)).unwrap();
    let refused = plan(stick(Some(main.root())), &FormatOptions::default(), Some(&store)).unwrap_err();
    assert_eq!(refused.reason(), Some(Reason::Archive), "{refused}");
    locallibrary::op::allow_archive_writes(true);
    let allowed = plan(stick(Some(main.root())), &FormatOptions::default(), Some(&store));
    locallibrary::op::allow_archive_writes(false);
    assert_eq!(allowed.unwrap().erases[0].role, Some(Role::Archive));

    fleet::set_role(&store, id, None).unwrap();
    fleet::retire(&store, id, false).unwrap();
    let refused = plan(stick(Some(main.root())), &FormatOptions::default(), Some(&store)).unwrap_err();
    assert_eq!(refused.reason(), Some(Reason::Retired), "{refused}");
}

/// A volume that isn't mounted can't be looked into: the plan says it goes too (an EFI partition
/// goes without saying).
#[test]
fn unmounted_volumes_are_erased_too() {
    let mut disk = stick(None);
    let mut efi = disk.volumes[0].clone();
    (efi.id, efi.label, efi.size) = ("disk999s2".into(), Some("EFI".into()), 209_715_200);
    disk.volumes.push(efi);
    let p = plan(disk, &FormatOptions::default(), None).unwrap();
    let unmounted: Vec<&String> = p.warnings.iter().filter(|w| w.contains("isn't mounted")).collect();
    assert_eq!(unmounted, ["STICK (64.0 GB) isn't mounted, so its contents can't be shown; it will be erased too."]);
    assert_eq!(p.erases.len(), 2);
}

#[test]
fn exfat_only_works_on_newer_players() {
    let opts = FormatOptions { filesystem: Some(Filesystem::Exfat), ..Default::default() };
    let p = plan(stick(None), &opts, None).unwrap();
    assert_eq!((p.filesystem, p.cluster_size), (Filesystem::Exfat, 128 * 1024));
    assert_eq!(p.compatibility.verdict, Verdict::SomePlayers);
    assert!(p.warnings.iter().any(|w| w == "this drive won't work on every player (see above)"), "{:?}", p.warnings);
}

/// macOS formats HFS+ and tests a drive's capacity first; Windows does neither.
#[test]
fn hfs_and_the_capacity_check_where_they_are_available() {
    let hfs = FormatOptions { filesystem: Some(Filesystem::Hfsplus), ..Default::default() };
    let checked = FormatOptions { check_capacity: true, ..Default::default() };
    if cfg!(windows) {
        assert!(plan(stick(None), &hfs, None).unwrap_err().to_string().contains("can't create HFS+"));
        assert!(plan(stick(None), &checked, None).unwrap_err().to_string().contains("isn't available on Windows"));
        return;
    }
    assert_eq!(plan(stick(None), &hfs, None).unwrap().cluster_size, 4096);
    let p = plan(stick(None), &checked, None).unwrap();
    assert!(p.steps[0].starts_with("test the real capacity: write and read back "), "{:?}", p.steps);
    assert!(p.steps[1].starts_with("diskutil eraseDisk"), "{:?}", p.steps);
}

/// Formatting waits for nothing: it is refused while a locallibrary operation is writing to a
/// drive on the disk, and stops when cancelled, both before the disk is asked anything.
#[test]
fn applying_stops_before_the_disk_is_touched() {
    let dir = tempfile::tempdir().unwrap();
    let (store, main) = pulled(dir.path());
    let disk = stick(Some(main.root()));
    let identity = disk.identity();
    let opts = FormatOptions::default();
    let apply = |ctx: &Ctx<'_>| format::apply(disk.clone(), &identity, &opts, Some(&store), &[], ctx);

    let syncing = main.lock().unwrap();
    assert!(matches!(apply(&Ctx::silent()), Err(Error::Locked(_))));
    drop(syncing);

    let cancel = CancelToken::new();
    cancel.cancel();
    assert!(matches!(apply(&Ctx::new(&NoProgress, cancel)), Err(Error::Cancelled)));
    assert!(Device::open(main.root()).unwrap().has_export(), "nothing was erased");
}
