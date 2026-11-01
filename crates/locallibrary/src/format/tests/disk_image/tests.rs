use super::*;
use crate::Error;
use crate::device::Device;
use crate::format::{Filesystem, FormatOptions, FsKind, Scheme, apply, disk_by_id, plan};
use crate::progress::Ctx;
use crate::store::Store;

/// The disk `image` is now, checked to be it.
fn mine(image: &TestImage) -> Disk {
    // lsblk reads what udev has found on the disk: wait for it to catch up with the format.
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = Cmd::new("udevadm", &["settle"]).run();
    let disk = disk_by_id(&image.id).unwrap().unwrap_or_else(|| panic!("{} is gone", image.id));
    image.assert_mine(&disk);
    disk
}

fn volumes(disk: &Disk) -> Vec<(Option<FsKind>, Option<&str>)> {
    disk.volumes.iter().map(|v| (v.filesystem, v.label.as_deref())).collect()
}

/// An image is detached when its guard goes, also when the test holding it panics, and the
/// format code stops taking it for the test's image.
#[test]
fn an_image_is_detached_when_its_test_ends() {
    if skipped() {
        return;
    }
    let image = TestImage::attach(8);
    image.admit();
    assert!(super::super::is_test_image(&image.id));
    let file = image.file.clone();
    drop(image);
    assert_eq!(os::attached(&file), None);
    super::super::TEST_IMAGE.with_borrow(|t| assert_eq!(t, &None));

    let attached = std::cell::RefCell::new(None);
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let image = TestImage::attach(8);
        *attached.borrow_mut() = Some(image.file.clone());
        panic!("the test failed");
    }));
    assert!(failed.is_err());
    assert_eq!(os::attached(&attached.into_inner().unwrap()), None);
}

/// A disk image is refused like any other, until the test lets its own through. Then
/// `format::apply` makes one FAT32 partition on MBR as planned, refuses the plan once the disk has
/// changed, and formats it again as exFAT. Where the system mounts the new volume, a drive adopted
/// on it keeps its name and identity.
#[test]
fn formats_the_image_it_attached_as_planned() {
    if skipped() {
        return;
    }
    // FAT32 with the default 4 KB clusters needs about 256 MiB.
    let image = TestImage::attach(300);
    let ctx = Ctx::silent();
    let store_dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(store_dir.path().join("store")).unwrap();
    let protected = vec![store.root().to_path_buf()];

    // Linux doesn't take a loop device for a disk at all.
    let unadmitted = disk_by_id(&image.id).unwrap();
    assert_eq!(unadmitted.is_none(), cfg!(all(unix, not(target_os = "macos"))));
    if let Some(disk) = unadmitted {
        image.assert_mine(&disk);
        let refused = plan(disk, &FormatOptions::default(), Some(&store), &protected, &ctx).unwrap_err();
        assert!(
            matches!(&refused, Error::Invalid(m) if m.ends_with("it's a disk image or virtual disk")),
            "{refused:?}"
        );
    }

    image.admit();
    let label = unique_label('F');
    let fat32 = FormatOptions { label: Some(label.clone()), ..FormatOptions::default() };
    let planned = plan(mine(&image), &fat32, Some(&store), &protected, &ctx).unwrap();
    assert_eq!((planned.filesystem, planned.label.as_str()), (Filesystem::Fat32, label.as_str()));
    image.assert_mine(&planned.disk);
    let report = apply(planned.disk.clone(), &planned.identity, &fat32, Some(&store), &protected, &ctx).unwrap();
    assert_eq!((report.disk.as_str(), report.filesystem), (image.id.as_str(), Filesystem::Fat32));
    let formatted = mine(&image);
    assert_eq!(formatted.scheme, Some(Scheme::Mbr));
    assert_eq!(volumes(&formatted), [(Some(FsKind::Fat32), Some(label.as_str()))]);
    assert_eq!(report.mount, formatted.volumes[0].mount);

    let stale = apply(formatted.clone(), &planned.identity, &fat32, Some(&store), &protected, &ctx).unwrap_err();
    assert!(matches!(stale, Error::StalePlan(_)), "{stale:?}");

    let kept = report.mount.as_ref().map(|m| {
        let mut d = Device::open(m).unwrap();
        let name = unique_label('K');
        d.adopt(Some(&name)).unwrap();
        crate::sync::register_drive(&store, &d, None).unwrap();
        (name, d.id().unwrap())
    });
    let exfat = FormatOptions { filesystem: Some(Filesystem::Exfat), ..FormatOptions::default() };
    let planned = plan(mine(&image), &exfat, Some(&store), &protected, &ctx).unwrap();
    assert_eq!(planned.keeps_name, kept.as_ref().map(|(name, _)| name.clone()));
    image.assert_mine(&planned.disk);
    let report = apply(planned.disk.clone(), &planned.identity, &exfat, Some(&store), &protected, &ctx).unwrap();
    assert_eq!(report.drive_name, planned.keeps_name);
    assert_eq!(volumes(&mine(&image)), [(Some(FsKind::Exfat), Some(planned.label.as_str()))]);
    if let (Some(m), Some((_, id))) = (&report.mount, &kept) {
        assert_eq!(Device::open(m).unwrap().id(), Some(*id));
    }
}

/// Ejecting a drive on one partition of a two-partition image is refused while the store is on
/// the other, which ejecting would unmount, and nothing runs. Otherwise the plan names the other
/// volume, and applying it unmounts both and detaches the image.
#[cfg(target_os = "macos")]
#[test]
fn ejecting_a_drive_unmounts_the_other_volume_of_its_disk() {
    use crate::api::{EjectDrives, drive_eject_plan};
    if skipped() {
        return;
    }
    let image = TestImage::attach(64);
    image.assert_mine(&mine(&image));
    let (a, b) = (unique_label('A'), unique_label('B'));
    Cmd::new("diskutil", &["partitionDisk", &image.id, "2", "MBR", "ExFAT", &a, "32M", "ExFAT", &b, "R"])
        .run()
        .unwrap();
    let disk = mine(&image);
    let mount = |label: &str| {
        disk.volumes.iter().find(|v| v.label.as_deref() == Some(label)).and_then(|v| v.mount.clone()).unwrap()
    };
    let (on_a, on_b) = (mount(&a), mount(&b));
    let mut d = Device::open(&on_a).unwrap();
    let name = unique_label('D');
    d.adopt(Some(&name)).unwrap();
    let req = EjectDrives { drives: vec![on_a.display().to_string()] };

    let refused = drive_eject_plan(Some(&on_b.join("store")), &req).unwrap_err();
    let unmounts = format!("would unmount {}, which holds", on_b.display());
    assert!(matches!(&refused, Error::Invalid(m) if m.contains(&unmounts)), "{refused:?}");
    assert!(on_a.is_dir() && on_b.is_dir());

    let store_dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(store_dir.path().join("store")).unwrap();
    crate::sync::register_drive(&store, &d, None).unwrap();
    let planned = drive_eject_plan(Some(store.root()), &req).unwrap();
    assert_eq!(planned.plan.drives[0].also_unmounts, std::slice::from_ref(&on_b));
    let summary = format!("eject {name}: unmount it (and {}, on the same disk) and power it off", on_b.display());
    assert_eq!(planned.summary, [summary]);
    image.assert_mine(&crate::format::disk_at(&on_a).unwrap());
    let reports = planned.apply(Some(&store), &Ctx::silent()).unwrap();
    assert_eq!(reports[0].commands, planned.plan.drives[0].commands);
    assert!(reports[0].powered_off);
    assert!(!on_a.exists() && !on_b.exists());
    assert_eq!(image.attached(), None);
    assert_eq!(store.drives().unwrap().shift_remove(&d.id().unwrap()).unwrap().log.clean_ejects, 1);
}
