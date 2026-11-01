use super::*;
use serde_json::json;

/// `diskutil info -plist`/`list -plist` of a 128 GB USB drive, trimmed.
fn usb_drive() -> (Value, Value, FxHashMap<String, Value>) {
    let info = json!({
        "DeviceIdentifier": "disk4", "DeviceNode": "/dev/disk4", "Content": "FDisk_partition_scheme",
        "IORegistryEntryName": "DJ TechTools Media", "MediaName": "TechTools", "Size": 127984988672u64,
        "DeviceBlockSize": 512, "BusProtocol": "USB", "Internal": false, "RemovableMediaOrExternalDevice": true,
        "VirtualOrPhysical": "Physical", "WritableMedia": true, "WholeDisk": true
    });
    let listing = json!({ "AllDisksAndPartitions": [{
        "DeviceIdentifier": "disk4", "Content": "FDisk_partition_scheme", "Size": 127984988672u64,
        "Partitions": [{ "DeviceIdentifier": "disk4s1", "Content": "Windows_FAT_32", "Size": 127984972288u64,
            "VolumeName": "DNA", "MountPoint": "/Volumes/DNA", "VolumeUUID": "12345678-9ABC-3DEF-8123-456789ABCDEF" }]
    }]});
    let part = json!({
        "DeviceIdentifier": "disk4s1", "FilesystemType": "msdos", "FilesystemName": "MS-DOS FAT32",
        "VolumeAllocationBlockSize": 32768, "VolumeName": "DNA", "MountPoint": "/Volumes/DNA"
    });
    (info, listing, FxHashMap::from_iter([("disk4s1".to_string(), part)]))
}

#[test]
fn parses_a_usb_drive() {
    let (info, listing, parts) = usb_drive();
    let d = parse_disk(&info, &listing, &parts, &["disk0".into()]);
    assert_eq!(d.id, "disk4");
    assert_eq!(d.model, "DJ TechTools");
    assert_eq!(d.scheme, Some(Scheme::Mbr));
    assert!(d.removable && !d.internal && !d.system && !d.virtual_disk && !d.read_only);
    assert_eq!(d.volumes.len(), 1);
    let v = &d.volumes[0];
    assert_eq!(v.filesystem, Some(FsKind::Fat32));
    assert_eq!(v.cluster_size, Some(32768));
    assert_eq!(v.mount.as_deref(), Some(Path::new("/Volumes/DNA")));
    assert_eq!(d.problem(&[]), None);
    assert!(parse_disk(&info, &listing, &parts, &["disk4".into()]).system);
}

#[test]
fn disk_images_are_virtual() {
    let (mut info, listing, parts) = usb_drive();
    info["BusProtocol"] = json!("Disk Image");
    info["VirtualOrPhysical"] = json!("Virtual");
    assert!(parse_disk(&info, &listing, &parts, &[]).virtual_disk);
}

#[test]
fn erase_commands() {
    let (info, listing, parts) = usb_drive();
    let d = parse_disk(&info, &listing, &parts, &[]);
    let t = Target { filesystem: Filesystem::Fat32, label: "DNA".into(), cluster_size: 32768, default_cluster: true };
    assert_eq!(steps(&d, &t).unwrap(), [r#"diskutil eraseDisk "MS-DOS FAT32" DNA MBR disk4"#]);
    let t =
        Target { filesystem: Filesystem::Exfat, label: "My USB".into(), cluster_size: 32768, default_cluster: false };
    assert_eq!(
        steps(&d, &t).unwrap(),
        [
            r#"diskutil eraseDisk ExFAT "My USB" MBR disk4"#,
            "diskutil unmountDisk disk4",
            r#"newfs_exfat -R -b 32768 -v "My USB" /dev/rdisk4s1"#,
            "diskutil mountDisk disk4",
        ]
    );
}

/// `ioreg -r -c IOUSBHostDevice -l -w0`, trimmed: a hub with a stick behind it (listed again
/// on its own), and a stick plugged in directly.
const IOREG: &str = r#"+-o USB3.0 Hub@01100000  <class IOUSBHostDevice, id 0x1000>
  | {
  |   "USB Serial Number" = "HUB0001"
  | }
  | 
  +-o Cruzer@01110000  <class IOUSBHostDevice, id 0x1001>
    | {
    |   "kUSBSerialNumberString" = "4C530001123456789012"
    |   "USB Serial Number" = "4C530001123456789012"
    | }
    | 
    +-o Cruzer Media  <class IOMedia, id 0x1002>
      | {
      |   "BSD Name" = "disk4"
      | }
      | 
      +-o DNA@1  <class IOMedia, id 0x1003>
          {
            "BSD Name" = "disk4s1"
          }
          
+-o Cruzer@01110000  <class IOUSBHostDevice, id 0x1001>
  | {
  |   "kUSBSerialNumberString" = "4C530001123456789012"
  |   "USB Serial Number" = "4C530001123456789012"
  | }
  | 
  +-o Cruzer Media  <class IOMedia, id 0x1002>
    | {
    |   "BSD Name" = "disk4"
    | }
    | 
    +-o DNA@1  <class IOMedia, id 0x1003>
        {
          "BSD Name" = "disk4s1"
        }
        
+-o DJ Stick@00200000  <class IOUSBHostDevice, id 0x2000>
  | {
  |   "USB Serial Number" = "DJ0002"
  | }
  | 
  +-o DJ Stick Media  <class IOMedia, id 0x2001>
      {
        "BSD Name" = "disk40"
      }
"#;

/// Regression: no disk had a serial number on macOS, so two sticks of one model and size
/// looked the same.
#[test]
fn a_usb_disks_serial_comes_from_the_ioregistry() {
    assert_eq!(usb_serial(IOREG, "disk4").as_deref(), Some("4C530001123456789012"));
    assert_eq!(usb_serial(IOREG, "disk40").as_deref(), Some("DJ0002"));
    assert_eq!(usb_serial(IOREG, "disk4s1").as_deref(), Some("4C530001123456789012"));
    assert_eq!(usb_serial(IOREG, "disk5"), None);
    assert_eq!(usb_serial("", "disk4"), None);
}

#[test]
fn whole_disk_of_a_partition() {
    assert_eq!(whole_of("disk0s2"), "disk0");
    assert_eq!(whole_of("disk12s1"), "disk12");
    assert_eq!(whole_of("disk3"), "disk3");
}

/// An external SSD as macOS formats it: GUID, an EFI partition, and an APFS container whose
/// volume (and mount point) is on the synthesized disk6. `diskutil info`/`list -plist`, trimmed.
#[test]
fn apfs_volumes_come_from_their_container() {
    let info = json!({
        "DeviceIdentifier": "disk5", "DeviceNode": "/dev/disk5", "Content": "GUID_partition_scheme",
        "IORegistryEntryName": "Samsung PSSD T7 Media", "Size": 1000204886016u64, "DeviceBlockSize": 512,
        "BusProtocol": "USB", "Internal": false, "RemovableMediaOrExternalDevice": true,
        "VirtualOrPhysical": "Physical", "WritableMedia": true
    });
    let listing = json!({ "AllDisksAndPartitions": [{
        "DeviceIdentifier": "disk5", "Content": "GUID_partition_scheme", "Size": 1000204886016u64,
        "Partitions": [
            { "DeviceIdentifier": "disk5s1", "Content": "EFI", "Size": 209715200, "VolumeName": "EFI",
              "DiskUUID": "A1B2C3D4-0000-4000-8000-000000000001" },
            { "DeviceIdentifier": "disk5s2", "Content": "Apple_APFS", "Size": 999995129856u64,
              "DiskUUID": "A1B2C3D4-0000-4000-8000-000000000002" }
        ]
    }]});
    let parts = FxHashMap::from_iter([
        (
            "disk5s1".to_string(),
            json!({ "FilesystemType": "msdos", "FilesystemName": "MS-DOS FAT32", "VolumeAllocationBlockSize": 512,
                    "DiskUUID": "A1B2C3D4-0000-4000-8000-000000000001" }),
        ),
        ("disk5s2".to_string(), json!({ "Content": "Apple_APFS", "APFSContainerReference": "disk6" })),
        (
            "disk5s2/apfs".to_string(),
            json!({ "AllDisksAndPartitions": [{
                "DeviceIdentifier": "disk6", "Size": 999995129856u64,
                "APFSPhysicalStores": [{ "DeviceIdentifier": "disk5s2" }],
                "APFSVolumes": [{ "DeviceIdentifier": "disk6s1", "VolumeName": "Samples",
                    "MountPoint": "/Volumes/Samples", "CapacityInUse": 421507072000u64, "Size": 999995129856u64,
                    "VolumeUUID": "0F1E2D3C-4B5A-4978-8695-A4B3C2D1E0F9" }]
            }]}),
        ),
    ]);
    let d = parse_disk(&info, &listing, &parts, &["disk3".into()]);
    assert_eq!((d.model.as_str(), d.scheme), ("Samsung PSSD T7", Some(Scheme::Gpt)));
    let ids: Vec<&str> = d.volumes.iter().map(|v| v.id.as_str()).collect();
    assert_eq!(ids, ["disk5s1", "disk5s2", "disk6s1"]);
    assert_eq!(d.volumes[0].uuid.as_deref(), Some("A1B2C3D4-0000-4000-8000-000000000001"));
    // The container's partition has no file system of its own; the volume in it does.
    assert_eq!((d.volumes[1].filesystem, d.volumes[1].filesystem_name.as_deref()), (None, Some("Apple_APFS")));
    let apfs = &d.volumes[2];
    assert_eq!((apfs.filesystem, apfs.label.as_deref()), (Some(FsKind::Apfs), Some("Samples")));
    assert_eq!((apfs.size, apfs.mount.as_deref()), (421507072000, Some(Path::new("/Volumes/Samples"))));
    // Players read the first partition, and don't read GUID partition tables.
    assert_eq!(crate::format::players::assess_disk(&d).verdict, crate::format::Verdict::NoPlayers);
}

/// An SD card with its lock switch on, formatted FAT16 without a partition table: the file
/// system is on the whole disk, which `list` shows without partitions.
#[test]
fn a_locked_card_without_a_partition_table_is_one_volume() {
    let info = json!({
        "DeviceIdentifier": "disk7", "DeviceNode": "/dev/disk7", "Content": "DOS_FAT_16", "MediaName": "SD Card Reader",
        "Size": 1973420032u64, "DeviceBlockSize": 512, "BusProtocol": "Secure Digital", "Internal": false,
        "Removable": true, "VirtualOrPhysical": "Physical", "WritableMedia": false, "FilesystemType": "msdos",
        "FilesystemName": "MS-DOS FAT16", "VolumeName": "CARD", "MountPoint": "/Volumes/CARD",
        "VolumeAllocationBlockSize": 32768, "VolumeUUID": "5F8C1A2B-3D4E-3F60-9A7B-C8D9E0F1A2B3"
    });
    let listing = json!({ "AllDisksAndPartitions": [{
        "DeviceIdentifier": "disk7", "Content": "DOS_FAT_16", "Size": 1973420032u64, "VolumeName": "CARD",
        "MountPoint": "/Volumes/CARD"
    }]});
    let d = parse_disk(&info, &listing, &FxHashMap::default(), &[]);
    assert_eq!(d.model, "SD Card Reader", "the media name, without an IORegistry name");
    assert_eq!(d.scheme, None);
    assert!(d.read_only && d.removable);
    assert_eq!(d.problem(&[]).as_deref(), Some("it's write-protected"));
    assert_eq!(d.volumes.len(), 1);
    let v = &d.volumes[0];
    assert_eq!((v.id.as_str(), v.filesystem, v.label.as_deref()), ("disk7", Some(FsKind::Fat16), Some("CARD")));
    assert_eq!((v.cluster_size, v.mount.as_deref()), (Some(32768), Some(Path::new("/Volumes/CARD"))));
}

#[test]
fn partition_schemes_are_told_by_content() {
    let (mut info, listing, parts) = usb_drive();
    for (content, scheme) in [
        ("GUID_partition_scheme", Some(Scheme::Gpt)),
        ("Apple_partition_scheme", Some(Scheme::Apm)),
        ("Sun_partition_scheme", Some(Scheme::Other)),
    ] {
        info["Content"] = json!(content);
        assert_eq!(parse_disk(&info, &listing, &parts, &[]).scheme, scheme, "{content}");
    }
    info.as_object_mut().unwrap().remove("Content");
    assert_eq!(parse_disk(&info, &listing, &parts, &[]).scheme, None);
}

#[test]
fn file_systems_are_told_by_type_and_name() {
    for (kind, name, want) in [
        ("msdos", "MS-DOS FAT16", FsKind::Fat16),
        ("msdos", "MS-DOS FAT12", FsKind::Fat12),
        ("msdos", "MS-DOS", FsKind::Other),
        ("exfat", "ExFAT", FsKind::Exfat),
        ("hfs", "Mac OS Extended (Case-sensitive)", FsKind::Hfsplus),
        ("apfs", "APFS", FsKind::Apfs),
        ("ntfs", "Windows NT File System (NTFS)", FsKind::Ntfs),
        ("udf", "Universal Disk Format (UDF)", FsKind::Other),
    ] {
        let (k, n) = filesystem(&json!({ "FilesystemType": kind, "FilesystemName": name }), &Value::Null);
        assert_eq!((k, n.as_deref()), (Some(want), Some(name)));
    }
    // No file system: the partition's content names it.
    let part = json!({ "Content": "Microsoft Basic Data" });
    assert_eq!(filesystem(&Value::Null, &part), (None, Some("Microsoft Basic Data".into())));
}

#[test]
fn hfs_and_fat32_with_a_chosen_cluster_size_are_planned() {
    let (info, listing, parts) = usb_drive();
    let d = parse_disk(&info, &listing, &parts, &[]);
    let t = Target { filesystem: Filesystem::Hfsplus, label: "DNA".into(), cluster_size: 4096, default_cluster: true };
    assert_eq!(steps(&d, &t).unwrap(), ["diskutil eraseDisk HFS+ DNA MBR disk4"]);
    let t = Target { filesystem: Filesystem::Fat32, label: "DNA".into(), cluster_size: 16384, default_cluster: false };
    assert_eq!(
        steps(&d, &t).unwrap(),
        [
            r#"diskutil eraseDisk "MS-DOS FAT32" DNA MBR disk4"#,
            "diskutil unmountDisk disk4",
            "newfs_msdos -F 32 -b 16384 -v DNA /dev/rdisk4s1",
            "diskutil mountDisk disk4",
        ]
    );
}

/// Only names shaped like `disk4` (or `/dev/disk4`, `/dev/rdisk4s1`) are asked of diskutil.
#[test]
fn other_names_are_not_disks() {
    for spec in ["Backup", "/dev/sdb", "disks", "/Volumes/DNA", "PhysicalDrive2"] {
        assert!(by_id(spec).unwrap().is_none(), "{spec}");
    }
}

// -----------------------------------------------------------------------------------------
// Properties
// -----------------------------------------------------------------------------------------

use hegel::generators as gs;
use hegel::{Generator, TestCase};

/// A value as diskutil (through plutil) writes it, or as a string, which the readers take too.
#[hegel::test_helper]
fn num(tc: &TestCase, n: u64) -> Value {
    if tc.draw(gs::weighted_booleans(0.2)) { json!(n.to_string()) } else { json!(n) }
}

/// A flag as diskutil writes it, or as `1`/`0` or "true"/"false".
#[hegel::test_helper]
fn flag(tc: &TestCase, b: bool) -> Value {
    match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => json!(u8::from(b)),
        1 => json!(if b { "true" } else { "false" }),
        _ => json!(b),
    }
}

/// Text diskutil might report: trimmed, never empty.
#[hegel::test_helper]
fn word(tc: &TestCase) -> String {
    let s: String = tc.draw(gs::text().alphabet("abcXYZ 09-_.éß日").min_size(1).max_size(12));
    let s = s.trim();
    if s.is_empty() { "x".into() } else { s.into() }
}

/// What a file system's `FilesystemType`/`FilesystemName` say, and what they mean.
const FILESYSTEMS: [(&str, &str, FsKind); 9] = [
    ("msdos", "MS-DOS FAT32", FsKind::Fat32),
    ("msdos", "MS-DOS FAT16", FsKind::Fat16),
    ("msdos", "MS-DOS FAT12", FsKind::Fat12),
    ("msdos", "MS-DOS", FsKind::Other),
    ("exfat", "ExFAT", FsKind::Exfat),
    ("hfs", "Mac OS Extended (Journaled)", FsKind::Hfsplus),
    ("apfs", "APFS", FsKind::Apfs),
    ("ntfs", "Windows NT File System (NTFS)", FsKind::Ntfs),
    ("udf", "Universal Disk Format (UDF)", FsKind::Other),
];

/// A disk as `parse_disk` should read it, rendered as `diskutil info`/`list -plist` (and
/// `info` of each partition and APFS container) with the spellings the tools vary between,
/// reads back as that disk, field for field.
#[hegel::test(test_cases = 300)]
fn diskutil_listings_read_back(tc: TestCase) {
    let n = tc.draw(gs::integers::<u8>().max_value(40));
    let id = format!("disk{n}");
    // Products can be named "... Media" too.
    let model = if tc.draw(gs::weighted_booleans(0.2)) { format!("{} Media", word(&tc)) } else { word(&tc) };
    let size = tc.draw(gs::integers::<u64>());
    let sector = tc.draw(gs::sampled_from(vec![512u64, 4096]));
    let (removable, internal, virt, writable) =
        (tc.draw(gs::booleans()), tc.draw(gs::booleans()), tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    let scheme = tc.draw(gs::sampled_from(vec![
        ("FDisk_partition_scheme", Some(Scheme::Mbr)),
        ("GUID_partition_scheme", Some(Scheme::Gpt)),
        ("Apple_partition_scheme", Some(Scheme::Apm)),
        ("Sun_partition_scheme", Some(Scheme::Other)),
    ]));
    let mut info = json!({
        "DeviceIdentifier": id, "Content": scheme.0, "Size": num(&tc, size), "DeviceBlockSize": num(&tc, sector),
        "Internal": flag(&tc, internal), "WritableMedia": flag(&tc, writable),
    });
    // The model is IOKit's name for the media, else diskutil's media name.
    if tc.draw(gs::booleans()) {
        info["IORegistryEntryName"] = json!(format!("{model} Media"));
    } else {
        info["MediaName"] = json!(model);
    }
    let removable_key = tc.draw(gs::sampled_from(vec!["RemovableMediaOrExternalDevice", "Removable"]));
    info[removable_key] = flag(&tc, removable);
    if virt {
        if tc.draw(gs::booleans()) {
            info["VirtualOrPhysical"] = json!("Virtual");
        } else {
            info["BusProtocol"] = json!("Disk Image");
        }
    } else {
        info["VirtualOrPhysical"] = json!("Physical");
        info["BusProtocol"] = json!("USB");
    }

    let mut want = Vec::new();
    let mut partitions = Vec::new();
    let mut parts = FxHashMap::default();
    for i in 1..=tc.draw(gs::integers::<usize>().max_value(3)) {
        let pid = format!("{id}s{i}");
        let psize = tc.draw(gs::integers::<u64>());
        let (fs_type, fs_name, kind) = tc.draw(gs::sampled_from(FILESYSTEMS.to_vec()));
        let formatted = tc.draw(gs::booleans());
        let label = if tc.draw(gs::booleans()) { Some(word(&tc)) } else { None };
        let mount = label.as_ref().filter(|_| tc.draw(gs::booleans())).map(|l| format!("/Volumes/{l}"));
        let uuid = if tc.draw(gs::booleans()) { Some(word(&tc)) } else { None };
        let cluster = tc.draw(gs::integers::<u64>().max_value(1 << 20));
        let mut p = json!({ "DeviceIdentifier": pid, "Content": "Windows_FAT_32", "Size": num(&tc, psize) });
        let mut pinfo = json!({ "DeviceIdentifier": pid, "VolumeAllocationBlockSize": num(&tc, cluster) });
        if formatted {
            pinfo["FilesystemType"] = json!(fs_type);
            pinfo["FilesystemName"] = json!(fs_name);
        }
        // The label and uuid are in either listing; the partition's own wins.
        if let Some(l) = &label {
            let key = if tc.draw(gs::booleans()) { &mut p } else { &mut pinfo };
            key["VolumeName"] = json!(l);
        }
        if let Some(m) = &mount {
            p["MountPoint"] = json!(m);
        }
        if let Some(u) = &uuid {
            if tc.draw(gs::booleans()) {
                p["VolumeUUID"] = json!(u);
            } else {
                pinfo["DiskUUID"] = json!(u);
            }
        }
        want.push(Volume {
            id: pid.clone(),
            size: psize,
            filesystem: formatted.then_some(kind),
            filesystem_name: Some(if formatted { fs_name.into() } else { "Windows_FAT_32".into() }),
            label,
            mount: mount.map(PathBuf::from),
            cluster_size: Some(cluster),
            uuid,
        });
        // Sometimes an APFS container, whose volumes follow its partition.
        if tc.draw(gs::weighted_booleans(0.3)) {
            let container = format!("disk{}", 100 + i);
            pinfo["APFSContainerReference"] = json!(container);
            let mut volumes = Vec::new();
            for j in 1..=tc.draw(gs::integers::<usize>().max_value(2)) {
                let vid = format!("{container}s{j}");
                let (used, total) = (tc.draw(gs::integers::<u64>()), tc.draw(gs::integers::<u64>()));
                let in_use = tc.draw(gs::booleans());
                let name = word(&tc);
                let mut v = json!({ "DeviceIdentifier": vid, "Size": total, "VolumeName": name,
                        "MountPoint": format!("/Volumes/{name}"), "VolumeUUID": format!("{name}-uuid") });
                if in_use {
                    v["CapacityInUse"] = json!(used);
                }
                volumes.push(v);
                want.push(Volume {
                    id: vid,
                    size: if in_use { used } else { total },
                    filesystem: Some(FsKind::Apfs),
                    filesystem_name: Some("APFS".into()),
                    label: Some(name.clone()),
                    mount: Some(PathBuf::from(format!("/Volumes/{name}"))),
                    cluster_size: None,
                    uuid: Some(format!("{name}-uuid")),
                });
            }
            let c = json!({ "AllDisksAndPartitions": [{ "DeviceIdentifier": container, "APFSVolumes": volumes }] });
            parts.insert(format!("{pid}/apfs"), c);
        }
        partitions.push(p);
        parts.insert(pid, pinfo);
    }
    let listing = json!({ "AllDisksAndPartitions": [{ "DeviceIdentifier": id, "Partitions": partitions }] });
    let boot: Vec<String> =
        tc.draw(gs::vecs(gs::integers::<u8>().max_value(40).map(|n| format!("disk{n}"))).max_size(2));

    let got = parse_disk(&info, &listing, &parts, &boot);
    let expected = Disk {
        device: format!("/dev/{id}"),
        model,
        serial: None,
        size,
        sector_size: sector as u32,
        bus: (!virt || info.get("BusProtocol").is_some()).then(|| if virt { "Disk Image" } else { "USB" }.into()),
        removable,
        internal,
        system: boot.contains(&id),
        virtual_disk: virt,
        read_only: !writable,
        scheme: scheme.1,
        volumes: want,
        id,
    };
    assert_eq!(serde_json::to_value(&got).unwrap(), serde_json::to_value(&expected).unwrap());
}

/// A file system written straight onto a disk without a partition table is its one volume.
#[hegel::test(test_cases = 100)]
fn a_disk_without_partitions_is_its_own_volume(tc: TestCase) {
    let (fs_type, fs_name, kind) = tc.draw(gs::sampled_from(FILESYSTEMS.to_vec()));
    let (size, label) = (tc.draw(gs::integers::<u64>()), word(&tc));
    let content = tc.draw(gs::sampled_from(vec!["DOS_FAT_32", "Apple_HFS"]));
    let info = json!({ "DeviceIdentifier": "disk7", "Content": content, "Size": size, "FilesystemType": fs_type,
            "FilesystemName": fs_name, "VolumeName": label, "TotalSize": 1 });
    let listing = json!({ "AllDisksAndPartitions": [{ "DeviceIdentifier": "disk7", "Content": content }] });
    let d = parse_disk(&info, &listing, &FxHashMap::default(), &[]);
    assert_eq!(d.scheme, None, "no partition table");
    assert_eq!(d.volumes.len(), 1);
    let v = &d.volumes[0];
    assert_eq!((v.id.as_str(), v.size, v.filesystem, v.label.as_deref()), ("disk7", size, Some(kind), Some(&*label)));
    assert_eq!(d.size, size);
    // Without a file system there is nothing to list.
    let mut bare = info.clone();
    bare.as_object_mut().unwrap().remove("FilesystemType");
    assert!(parse_disk(&bare, &listing, &FxHashMap::default(), &[]).volumes.is_empty());
}

/// A USB device in the IORegistry: a hub (devices behind it) or a stick (disks in it).
#[derive(Debug, Clone)]
struct UsbDevice {
    serial: Option<String>,
    hub: Vec<UsbDevice>,
    disks: Vec<u32>,
}

#[hegel::test_helper]
fn usb_device(tc: &TestCase, depth: usize, next_disk: &mut u32) -> UsbDevice {
    let serial = if tc.draw(gs::weighted_booleans(0.85)) {
        Some(tc.draw(gs::text().alphabet("0123456789ABCDEF").min_size(1).max_size(12)))
    } else {
        None
    };
    if depth < 2 && tc.draw(gs::weighted_booleans(0.3)) {
        let n = tc.draw(gs::integers::<usize>().max_value(3));
        return UsbDevice {
            serial,
            hub: (0..n).map(|_| usb_device(tc, depth + 1, next_disk)).collect(),
            disks: vec![],
        };
    }
    let n = tc.draw(gs::integers::<u8>().max_value(2));
    let disks = (0..n).map(|_| {
        // Skip ahead now and then, so that disk4 and disk40 both exist.
        *next_disk += 1 + tc.draw(gs::integers::<u32>().max_value(1)) * 35;
        *next_disk
    });
    UsbDevice { serial, hub: vec![], disks: disks.collect() }
}

/// `ioreg -r -c IOUSBHostDevice -l -w0` prints each device's tree, nested ones inside their
/// hub's and again on their own.
fn render(d: &UsbDevice, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    out.push_str(&format!("{pad}+-o Device@{depth}  <class IOUSBHostDevice>\n{pad}  | {{\n"));
    out.push_str(&format!("{pad}  |   \"idVendor\" = 1234\n"));
    if let Some(s) = &d.serial {
        out.push_str(&format!("{pad}  |   \"kUSBSerialNumberString\" = \"{s}\"\n"));
        out.push_str(&format!("{pad}  |   \"USB Serial Number\" = \"{s}\"\n"));
    }
    out.push_str(&format!("{pad}  | }}\n"));
    for h in &d.hub {
        render(h, depth + 1, out);
    }
    for n in &d.disks {
        out.push_str(&format!(
            "{pad}  +-o Media  <class IOMedia>\n{pad}    | {{\n{pad}    |   \"BSD Name\" = \"disk{n}\"\n"
        ));
        out.push_str(&format!("{pad}    | }}\n{pad}    +-o Part@1  <class IOMedia>\n"));
        out.push_str(&format!("{pad}        {{\n{pad}          \"BSD Name\" = \"disk{n}s1\"\n{pad}        }}\n"));
    }
}

fn all_devices<'a>(d: &'a UsbDevice, out: &mut Vec<&'a UsbDevice>) {
    out.push(d);
    for h in &d.hub {
        all_devices(h, out);
    }
}

/// A disk's serial number is the one of the USB device it is in (not of a hub it is behind,
/// nor of another disk whose number starts the same), for the disk and its partitions; a disk
/// on no USB device, or on one without a serial number, has none.
#[hegel::test(test_cases = 300)]
fn usb_serial_numbers_come_from_the_disks_own_device(tc: TestCase) {
    let mut next = 0u32;
    let roots: Vec<UsbDevice> =
        (0..tc.draw(gs::integers::<usize>().max_value(3))).map(|_| usb_device(&tc, 0, &mut next)).collect();
    let mut devices = Vec::new();
    for r in &roots {
        all_devices(r, &mut devices);
    }
    let mut ioreg = String::new();
    for d in &devices {
        if !ioreg.is_empty() {
            ioreg.push('\n');
        }
        render(d, 0, &mut ioreg);
    }
    for d in &devices {
        for n in &d.disks {
            let want = d.serial.as_deref();
            assert_eq!(usb_serial(&ioreg, &format!("disk{n}")).as_deref(), want, "disk{n} in\n{ioreg}");
            assert_eq!(usb_serial(&ioreg, &format!("disk{n}s1")).as_deref(), want);
        }
    }
    assert_eq!(usb_serial(&ioreg, &format!("disk{}", next + 1)), None);
}

/// A partition's whole disk is its name up to the slice, and only names shaped like a disk are
/// asked of diskutil.
#[hegel::test(test_cases = 200)]
fn partitions_name_their_disk(tc: TestCase) {
    let (n, s) = (tc.draw(gs::integers::<u32>()), tc.draw(gs::integers::<u32>()));
    assert_eq!(whole_of(&format!("disk{n}s{s}")), format!("disk{n}"));
    assert_eq!(whole_of(&format!("disk{n}")), format!("disk{n}"));
    let spec = tc.draw(gs::text().max_size(12).filter(|s: &String| !s.contains(|c: char| c.is_ascii_digit())));
    assert!(by_id(&spec).unwrap().is_none(), "{spec:?}");
}

/// Erasing is one `eraseDisk` with the file system, label and MBR; a chosen cluster size makes
/// the file system again in the new partition, unmounted, with that size.
#[hegel::test(test_cases = 200)]
fn erase_commands_follow_the_target(tc: TestCase) {
    let (info, listing, parts) = usb_drive();
    let disk = parse_disk(&info, &listing, &parts, &[]);
    let filesystem = tc.draw(gs::sampled_from(vec![Filesystem::Fat32, Filesystem::Exfat, Filesystem::Hfsplus]));
    let label = filesystem.label(&tc.draw(gs::text().max_size(20)));
    let cluster_size = 1 << tc.draw(gs::integers::<u32>().min_value(9).max_value(17));
    let default_cluster = filesystem == Filesystem::Hfsplus || tc.draw(gs::booleans());
    let t = Target { filesystem, label: label.clone(), cluster_size, default_cluster };
    let cmds = commands(&disk, &t);
    let personality = ["MS-DOS FAT32", "ExFAT", "HFS+"][filesystem as usize];
    assert_eq!(cmds[0].args, ["eraseDisk", personality, &label, "MBR", "disk4"]);
    assert_eq!(steps(&disk, &t).unwrap(), cmds.iter().map(Cmd::display).collect::<Vec<_>>());
    if default_cluster {
        assert_eq!(cmds.len(), 1);
        return;
    }
    assert_eq!(cmds.len(), 4);
    assert_eq!(cmds[1], Cmd::new("diskutil", &["unmountDisk", "disk4"]));
    assert_eq!(cmds[3], Cmd::new("diskutil", &["mountDisk", "disk4"]));
    let make = &cmds[2];
    let i = make.args.iter().position(|a| a == "-b").unwrap();
    assert_eq!(make.args[i + 1], cluster_size.to_string());
    assert_eq!(make.args.last().map(String::as_str), Some("/dev/rdisk4s1"));
    assert!(make.args.contains(&label));
}

/// A disk's partition scheme is what its `Content` names, whatever partitions it lists: none
/// without a `Content`, and for a name diskutil uses for no scheme, another scheme only when it
/// has a partition table to show for it.
#[hegel::test(test_cases = 200)]
fn partition_schemes_follow_the_disks_content(tc: TestCase) {
    let content = tc.draw(gs::sampled_from(vec![
        (None, None),
        (Some("FDisk_partition_scheme"), Some(Scheme::Mbr)),
        (Some("GUID_partition_scheme"), Some(Scheme::Gpt)),
        (Some("Apple_partition_scheme"), Some(Scheme::Apm)),
        (Some("Sun_partition_scheme"), Some(Scheme::Other)),
        (Some("DOS_FAT_32"), Some(Scheme::Other)),
    ]));
    let partitions = tc.draw(gs::integers::<usize>().max_value(3));
    let table = tc.draw(gs::booleans()) || partitions > 0;
    let mut info = json!({ "DeviceIdentifier": "disk4", "Size": tc.draw(gs::integers::<u64>()) });
    let mut entry = json!({ "DeviceIdentifier": "disk4" });
    if let Some(c) = content.0 {
        info["Content"] = json!(c);
        entry["Content"] = json!(c);
    }
    if table {
        entry["Partitions"] = (1..=partitions)
            .map(|i| json!({ "DeviceIdentifier": format!("disk4s{i}"), "Content": "Windows_FAT_32", "Size": 1 << 30 }))
            .collect();
    }
    let listing = json!({ "AllDisksAndPartitions": [entry] });
    let d = parse_disk(&info, &listing, &FxHashMap::default(), &[]);
    let want = match content.1 {
        Some(Scheme::Other) if !table => None,
        s => s,
    };
    assert_eq!(d.scheme, want);
    assert_eq!(d.volumes.len(), partitions);
}

// -----------------------------------------------------------------------------------------
// Through diskutil: a fake one that answers only what it was told the disks are
// -----------------------------------------------------------------------------------------

#[cfg(unix)]
use crate::format::tests::fake_tools::FakeTools;

#[cfg(unix)]
const PLUTIL: [&str; 6] = ["-convert", "json", "-o", "-", "--", "-"];
#[cfg(unix)]
const IOREG_ARGS: [&str; 5] = ["-r", "-c", "IOUSBHostDevice", "-l", "-w0"];

/// A disk as diskutil describes it, to tell a fake diskutil.
#[cfg(unix)]
#[derive(Debug, Clone)]
struct Described {
    id: String,
    info: Value,
    listing: Value,
    /// `info` of each partition, and whether diskutil answers for it.
    parts: Vec<(String, Value, bool)>,
    /// Each APFS container: the partition holding it, its identifier, its `list`, and whether
    /// diskutil answers for it.
    containers: Vec<(String, String, Value, bool)>,
    serial: Option<String>,
}

#[cfg(unix)]
#[hegel::test_helper]
fn described(tc: &TestCase, n: u32) -> Described {
    let id = format!("disk{n}");
    let info = json!({
        "DeviceIdentifier": id, "ParentWholeDisk": id, "Content": "FDisk_partition_scheme", "MediaName": word(tc),
        "Size": tc.draw(gs::integers::<u64>().min_value(1 << 26).max_value(1 << 41)), "DeviceBlockSize": 512,
        "Internal": tc.draw(gs::weighted_booleans(0.2)), "RemovableMediaOrExternalDevice": tc.draw(gs::weighted_booleans(0.8)),
        "VirtualOrPhysical": "Physical", "BusProtocol": "USB", "WritableMedia": true,
    });
    let (mut partitions, mut parts, mut containers) = (Vec::new(), Vec::new(), Vec::new());
    for i in 1..=tc.draw(gs::integers::<u32>().max_value(2)) {
        let pid = format!("{id}s{i}");
        let (fs_type, fs_name, _) = tc.draw(gs::sampled_from(FILESYSTEMS.to_vec()));
        let label = word(tc);
        let size = tc.draw(gs::integers::<u64>().max_value(1 << 40));
        let mut p = json!({ "DeviceIdentifier": pid, "Content": "Windows_FAT_32", "Size": size, "VolumeName": label,
                "VolumeUUID": format!("{pid}-uuid") });
        let mut pinfo = json!({ "DeviceIdentifier": pid, "ParentWholeDisk": id, "FilesystemType": fs_type,
                "FilesystemName": fs_name, "VolumeName": label, "VolumeAllocationBlockSize": 32768 });
        if tc.draw(gs::booleans()) {
            let m = format!("/Volumes/{pid}");
            p["MountPoint"] = json!(m);
            pinfo["MountPoint"] = json!(m);
        }
        if tc.draw(gs::weighted_booleans(0.25)) {
            let container = format!("disk{}", 100 + n * 4 + i);
            pinfo["APFSContainerReference"] = json!(container);
            let vid = format!("{container}s1");
            let volumes = json!([{ "DeviceIdentifier": vid, "Size": 1 << 30, "VolumeName": word(tc),
                    "MountPoint": format!("/Volumes/{vid}"), "VolumeUUID": format!("{vid}-uuid") }]);
            let c = json!({ "AllDisksAndPartitions": [{ "DeviceIdentifier": container, "APFSVolumes": volumes }] });
            containers.push((pid.clone(), container, c, tc.draw(gs::weighted_booleans(0.9))));
        }
        partitions.push(p);
        parts.push((pid, pinfo, tc.draw(gs::weighted_booleans(0.9))));
    }
    let listing = json!({ "AllDisksAndPartitions": [{ "DeviceIdentifier": id, "Content": "FDisk_partition_scheme",
            "Partitions": partitions }] });
    let serial = tc
        .draw(gs::weighted_booleans(0.7))
        .then(|| tc.draw(gs::text().alphabet("0123456789ABCDEF").min_size(1).max_size(12)));
    Described { id, info, listing, parts, containers, serial }
}

#[cfg(unix)]
impl Described {
    /// What diskutil says about the partitions, as [`parse_disk`] takes it.
    fn answered_parts(&self) -> FxHashMap<String, Value> {
        let mut out: FxHashMap<String, Value> =
            self.parts.iter().filter(|p| p.2).map(|(pid, info, _)| (pid.clone(), info.clone())).collect();
        for (pid, _, c, answered) in &self.containers {
            if *answered && out.contains_key(pid) {
                out.insert(format!("{pid}/apfs"), c.clone());
            }
        }
        out
    }

    /// The disk, as diskutil's answers about it and its USB device read.
    fn disk(&self, boot: &[String], usb: bool) -> Disk {
        let mut d = parse_disk(&self.info, &self.listing, &self.answered_parts(), boot);
        d.serial = self.serial.clone().filter(|_| usb);
        d
    }

    /// Its USB device, as ioreg prints it.
    fn ioreg(&self) -> String {
        let n = self.id.trim_start_matches("disk").parse().unwrap();
        let mut out = String::new();
        render(&UsbDevice { serial: self.serial.clone(), hub: vec![], disks: vec![n] }, 0, &mut out);
        out
    }

    /// Tell `fake` about the disk: by identifier, and each volume diskutil can describe by its
    /// mount point too (an APFS volume's says which container it's in, the container which
    /// partition holds it).
    fn tell(&self, fake: &mut FakeTools) {
        fake.answer("diskutil", &["info", "-plist", &self.id], &self.info.to_string());
        fake.answer("diskutil", &["list", "-plist", &self.id], &self.listing.to_string());
        let parts = self.answered_parts();
        for (pid, info, answered) in &self.parts {
            if !answered {
                fake.refuse("diskutil", &["info", "-plist", pid], &format!("Could not find disk: {pid}"));
                continue;
            }
            fake.answer("diskutil", &["info", "-plist", pid], &info.to_string());
            if let Some(m) = json::str(info, "MountPoint") {
                fake.answer("diskutil", &["info", "-plist", &m], &info.to_string());
            }
        }
        for (pid, container, c, _) in &self.containers {
            if !parts.contains_key(&format!("{pid}/apfs")) {
                fake.refuse("diskutil", &["list", "-plist", container], &format!("Could not find disk: {container}"));
                continue;
            }
            fake.answer("diskutil", &["list", "-plist", container], &c.to_string());
            let stores = json!({ "DeviceIdentifier": container, "APFSPhysicalStores": [{ "APFSPhysicalStore": pid }] });
            fake.answer("diskutil", &["info", "-plist", container], &stores.to_string());
            let volume = &c["AllDisksAndPartitions"][0]["APFSVolumes"][0];
            let m = json::str(volume, "MountPoint").unwrap();
            let vinfo = json!({ "DeviceIdentifier": volume["DeviceIdentifier"], "MountPoint": m,
                    "ParentWholeDisk": container });
            fake.answer("diskutil", &["info", "-plist", &m], &vinfo.to_string());
        }
    }
}

/// The running system as `diskutil info /` describes it: on the APFS container in `store`.
#[cfg(unix)]
fn root_volume(store: &str) -> String {
    json!({ "MountPoint": "/", "ParentWholeDisk": "disk1", "APFSPhysicalStores": [{ "APFSPhysicalStore": store }] })
        .to_string()
}

/// Listing asks diskutil for the external physical disks and describes each: what comes back is
/// every listed disk as diskutil's answers about it read (partitions it can't describe left
/// bare), with its USB device's serial number when ioreg answers, and marked as the system's
/// when the running system lives on it. Listing only reads: diskutil is asked nothing but `info`
/// and `list`. A listed disk diskutil then can't describe fails the listing, in its own words.
#[cfg(unix)]
#[hegel::test(test_cases = 15)]
fn the_disks_listed_are_the_ones_diskutil_describes(tc: TestCase) {
    let numbers: Vec<u32> =
        tc.draw(gs::vecs(gs::integers::<u32>().min_value(2).max_value(40)).max_size(2).unique(true));
    let disks: Vec<Described> = numbers.iter().map(|&n| described(&tc, n)).collect();
    let mut fake = FakeTools::new();
    fake.echo("plutil", &PLUTIL);
    let usb = tc.draw(gs::weighted_booleans(0.8));
    if usb {
        fake.answer("ioreg", &IOREG_ARGS, &disks.iter().map(Described::ioreg).collect::<Vec<_>>().join("\n"));
    } else {
        fake.refuse("ioreg", &IOREG_ARGS, "ioreg: no such class");
    }
    // The system runs from an internal disk, or now and then from one of the listed ones.
    let booted = (!disks.is_empty() && tc.draw(gs::weighted_booleans(0.3)))
        .then(|| tc.draw(gs::integers::<usize>().max_value(disks.len() - 1)));
    let store = booted.map_or("disk0s2".to_string(), |i| format!("{}s2", disks[i].id));
    let boot = if tc.draw(gs::weighted_booleans(0.9)) {
        fake.answer("diskutil", &["info", "-plist", "/"], &root_volume(&store));
        vec![whole_of(&store), "disk1".to_string()]
    } else {
        fake.refuse("diskutil", &["info", "-plist", "/"], "Could not find disk: /");
        Vec::new()
    };
    let ids: Vec<&str> = disks.iter().map(|d| d.id.as_str()).collect();
    fake.answer("diskutil", &["list", "-plist", "external", "physical"], &json!({ "WholeDisks": ids }).to_string());
    // Now and then a disk goes between being listed and being described.
    let gone = if disks.is_empty() || !tc.draw(gs::weighted_booleans(0.15)) {
        None
    } else {
        Some(tc.draw(gs::integers::<usize>().max_value(disks.len() - 1)))
    };
    for (i, d) in disks.iter().enumerate() {
        if gone == Some(i) {
            fake.refuse("diskutil", &["info", "-plist", &d.id], &format!("Could not find disk: {}", d.id));
        } else {
            d.tell(&mut fake);
        }
    }

    let listed = list();
    for c in fake.calls() {
        assert!(c.program != "diskutil" || ["info", "list"].contains(&c.args[0].as_str()), "{c:?}");
    }
    if let Some(i) = gone {
        match listed {
            Err(Error::External { message, .. }) => {
                assert_eq!(message, format!("Could not find disk: {}", disks[i].id))
            }
            other => panic!("{other:?}"),
        }
        return;
    }
    let listed = listed.unwrap();
    let want: Vec<Disk> = disks.iter().map(|d| d.disk(&boot, usb)).collect();
    assert_eq!(serde_json::to_value(&listed).unwrap(), serde_json::to_value(&want).unwrap());
    if let (Some(i), false) = (booted, boot.is_empty()) {
        assert!(listed[i].system && listed[i].problem(&[]).is_some());
    }
    // On a Mac these are the disks offered for formatting: the external and removable ones.
    // (Listing again takes as long again, so only for a single disk.)
    #[cfg(target_os = "macos")]
    if disks.len() == 1 {
        let views = crate::format::list(&[]).unwrap();
        let offered: Vec<&str> = views.iter().map(|v| v.disk.id.as_str()).collect();
        let external: Vec<&str> = want.iter().filter(|d| d.removable && !d.internal).map(|d| d.id.as_str()).collect();
        assert_eq!(offered, external);
        for v in views {
            assert_eq!((v.identity, v.problem), (v.disk.identity(), v.disk.problem(&[])));
        }
    }
}

/// Every name of a disk finds that disk, as diskutil describes it: its identifier as given, as
/// a device and as a raw device; each partition diskutil knows; and the mount point of each
/// volume on it diskutil can describe, an APFS volume's through its container's physical store. A folder inside a
/// volume is refused as not a volume's root, and a path diskutil knows nothing of as not a
/// mounted volume.
#[cfg(unix)]
#[hegel::test(test_cases = 30)]
fn every_name_of_a_disk_finds_that_disk(tc: TestCase) {
    let d = described(&tc, tc.draw(gs::integers::<u32>().min_value(2).max_value(40)));
    let mut fake = FakeTools::new();
    fake.echo("plutil", &PLUTIL);
    fake.answer("ioreg", &IOREG_ARGS, &d.ioreg());
    fake.answer("diskutil", &["info", "-plist", "/"], &root_volume("disk0s2"));
    d.tell(&mut fake);
    let want = d.disk(&["disk0".into(), "disk1".into()], true);
    let id = &d.id;
    let mut names: Vec<String> = vec![id.clone(), format!("/dev/{id}"), format!("/dev/r{id}")];
    names.extend(d.parts.iter().filter(|p| p.2).map(|p| p.0.clone()));
    // Each volume diskutil can describe: a partition it can't, it can't find by mount point either.
    let parts = d.answered_parts();
    let described = want.volumes.iter().filter(|v| parts.contains_key(&v.id) || !v.id.starts_with(&format!("{id}s")));
    let mounts: Vec<String> = described.filter_map(|v| v.mount.as_ref()).map(|m| m.display().to_string()).collect();
    names.extend(mounts.iter().cloned());
    names.push("/Volumes/elsewhere".into());
    if let Some(m) = mounts.first() {
        let inner = format!("{m}/Contents");
        let holder = json!({ "MountPoint": m, "ParentWholeDisk": id });
        fake.answer("diskutil", &["info", "-plist", &inner], &holder.to_string());
        names.push(inner);
    }
    let name = tc.draw(gs::sampled_from(names));
    let found =
        if name.starts_with("/Volumes/") { by_mount(Path::new(&name)) } else { by_id(&name).map(Option::unwrap) };
    match found {
        Ok(disk) => {
            assert!(!name.ends_with("/Contents") && name != "/Volumes/elsewhere", "{name}");
            assert_eq!(serde_json::to_value(&disk).unwrap(), serde_json::to_value(&want).unwrap(), "{name}");
        }
        Err(Error::Invalid(m)) => {
            assert!(name.ends_with("/Contents") && m.contains("is not the root of a volume"), "{m}")
        }
        Err(Error::NotFound(m)) => {
            assert!(name == "/Volumes/elsewhere" && m.contains("is not a mounted volume"), "{m}")
        }
        Err(e) => panic!("{name}: {e}"),
    }
}

/// Formatting is the planned erase, then a fresh look at the disk: the new volume is mounted
/// where diskutil now says (nowhere when it isn't). When the erase fails, diskutil's own words
/// come back, and the disk is asked nothing more.
#[cfg(unix)]
#[hegel::test(test_cases = 20)]
fn formatting_erases_then_finds_the_new_volume(tc: TestCase) {
    let d = described(&tc, tc.draw(gs::integers::<u32>().min_value(2).max_value(40)));
    let disk = d.disk(&[], false);
    let filesystem = tc.draw(gs::sampled_from(vec![Filesystem::Fat32, Filesystem::Exfat, Filesystem::Hfsplus]));
    let label = filesystem.label(&word(&tc));
    let cluster_size = filesystem.default_cluster_size(disk.partition_size());
    let t = Target { filesystem, label: label.clone(), cluster_size, default_cluster: true };
    let erase = commands(&disk, &t).remove(0);
    let args: Vec<&str> = erase.args.iter().map(String::as_str).collect();
    let mut fake = FakeTools::new();
    fake.echo("plutil", &PLUTIL);
    let fails = tc.draw(gs::weighted_booleans(0.2));
    let mount = tc.draw(gs::booleans()).then(|| format!("/Volumes/{label}"));
    if fails {
        fake.refuse("diskutil", &args, "Error: -69877: Couldn't open device");
    } else {
        fake.then_after("diskutil", &args);
        let pid = format!("{}s1", d.id);
        let mut part = json!({ "DeviceIdentifier": pid, "Content": "Windows_FAT_32", "Size": disk.partition_size(),
                "VolumeName": label });
        if let Some(m) = &mount {
            part["MountPoint"] = json!(m);
        }
        let listing = json!({ "AllDisksAndPartitions": [{ "DeviceIdentifier": d.id, "Partitions": [part] }] });
        fake.answer("diskutil", &["info", "-plist", &d.id], &d.info.to_string());
        fake.answer("diskutil", &["list", "-plist", &d.id], &listing.to_string());
        fake.answer("diskutil", &["info", "-plist", &pid], &json!({ "DeviceIdentifier": pid }).to_string());
    }

    let formatted = format(&disk, &t, &Ctx::silent());
    let asked: Vec<Cmd> = fake.calls().into_iter().filter(|c| c.program == "diskutil").collect();
    assert_eq!(asked[0], erase);
    if fails {
        assert!(matches!(&formatted, Err(Error::External { message, .. }) if message.contains("Couldn't open device")));
        assert_eq!(asked.len(), 1);
        return;
    }
    assert_eq!(formatted.unwrap(), mount.map(PathBuf::from));
    assert!(asked[1..].iter().all(|c| ["info", "list"].contains(&c.args[0].as_str())), "{asked:?}");
}

/// Applying a plan formats the disk it was made for and no other: diskutil is asked about the
/// disk again just before the erase, and when what is on it changed since the plan (its volume
/// relabelled or resized, another disk under its identifier) nothing is erased and the plan is
/// stale. Otherwise the planned erase runs, and the drive that was on the disk keeps its name and
/// identity on the new volume.
#[cfg(target_os = "macos")]
#[hegel::test(test_cases = 15)]
fn a_plan_formats_only_the_disk_it_was_made_for(tc: TestCase) {
    use crate::device::Device;
    use crate::format::{FormatOptions, apply, plan};
    let mut fake = FakeTools::new();
    fake.echo("plutil", &PLUTIL);
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
    let parts = FxHashMap::from_iter([(pid.clone(), pinfo.clone())]);
    let disk = parse_disk(&serde_json::from_str(&info(size)).unwrap(), &listing("DNA", psize, &old), &parts, &[]);
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
    let target = Target {
        filesystem: planned.filesystem,
        label: planned.label.clone(),
        cluster_size: planned.cluster_size,
        default_cluster: true,
    };
    let erase = commands(&disk, &target).remove(0);
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
