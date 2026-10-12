//! macOS: `diskutil` (no administrator rights needed for external disks).

use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{Cmd, Disk, Filesystem, FsKind, Scheme, Target, Volume, json};
use crate::progress::{Ctx, Phase};
use crate::{Error, Result};

/// `diskutil eraseDisk` mounts the new volume.
pub const MOUNTS_AFTER_FORMAT: bool = true;

/// `diskutil <args> -plist`, as JSON.
fn diskutil(args: &[&str]) -> Result<Value> {
    let mut full = vec![args[0], "-plist"];
    full.extend_from_slice(&args[1..]);
    let xml = Cmd::new("diskutil", &full).run()?;
    let text = Cmd::new("plutil", &["-convert", "json", "-o", "-", "--", "-"]).input(xml).run()?;
    serde_json::from_str(&text).map_err(|e| Error::parse("diskutil output", e))
}

pub fn list() -> Result<Vec<Disk>> {
    let filter: &[&str] = if super::virtual_disks_allowed() { &["external"] } else { &["external", "physical"] };
    let mut args = vec!["list"];
    args.extend_from_slice(filter);
    let listing = diskutil(&args)?;
    let boot = boot_disks();
    let usb = usb_devices();
    json::array(listing.get("WholeDisks"))
        .into_iter()
        .filter_map(Value::as_str)
        .map(|id| describe(id, &boot, &usb))
        .collect()
}

pub fn by_id(spec: &str) -> Result<Option<Disk>> {
    let id = spec.trim_start_matches("/dev/").trim_start_matches('r');
    let is_disk = id.strip_prefix("disk").is_some_and(|n| n.starts_with(|c: char| c.is_ascii_digit()));
    if !is_disk {
        return Ok(None);
    }
    let info = diskutil(&["info", id])?;
    let whole = json::str(&info, "ParentWholeDisk").unwrap_or_else(|| id.to_string());
    describe(&whole, &boot_disks(), &usb_devices()).map(Some)
}

pub fn by_mount(path: &Path) -> Result<Disk> {
    let info = diskutil(&["info", &path.to_string_lossy()])
        .map_err(|_| Error::NotFound(format!("{} is not a mounted volume", path.display())))?;
    if json::str(&info, "MountPoint").as_deref() != Some(&*path.to_string_lossy()) {
        return Err(Error::invalid(format!("{} is not the root of a volume", path.display())));
    }
    let whole =
        json::str(&info, "ParentWholeDisk").ok_or_else(|| Error::NotFound(format!("disk of {}", path.display())))?;
    let (boot, usb) = (boot_disks(), usb_devices());
    // An APFS volume lives on a synthesized disk; the physical disk is its container's store.
    let info = diskutil(&["info", &whole])?;
    if let Some(store) = json::array(info.get("APFSPhysicalStores")).first() {
        let part = json::str(store, "APFSPhysicalStore").unwrap_or_default();
        return describe(&whole_of(&part), &boot, &usb);
    }
    describe(&whole, &boot, &usb)
}

/// Physical disks the running system lives on.
fn boot_disks() -> Vec<String> {
    let Ok(root) = diskutil(&["info", "/"]) else { return Vec::new() };
    let mut out: Vec<String> = json::array(root.get("APFSPhysicalStores"))
        .into_iter()
        .filter_map(|s| json::str(s, "APFSPhysicalStore"))
        .map(|p| whole_of(&p))
        .collect();
    out.extend(json::str(&root, "ParentWholeDisk"));
    out
}

/// The USB devices in the IORegistry, each with what's connected behind it, as `ioreg` prints
/// them: where a disk's serial number is (diskutil doesn't say). Empty if ioreg can't run.
fn usb_devices() -> String {
    Cmd::new("ioreg", &["-r", "-c", "IOUSBHostDevice", "-l", "-w0"]).run().unwrap_or_default()
}

/// The serial number of the USB device the disk `id` (`disk4`) is on, from [`usb_devices`]: the
/// innermost device whose tree holds the disk (a hub's tree holds the devices behind it). `None`
/// for a disk that isn't USB, or a device that has none.
fn usb_serial(ioreg: &str, id: &str) -> Option<String> {
    let media = format!("\"BSD Name\" = \"{id}\"");
    let property = |line: &str| line.trim_start_matches(['|', ' ']).to_string();
    ioreg
        .split("\n+-o ")
        .filter(|tree| tree.lines().any(|l| property(l) == media))
        .min_by_key(|tree| tree.len())?
        .lines()
        .find_map(|l| property(l).strip_prefix("\"USB Serial Number\" = \"")?.strip_suffix('"').map(String::from))
        .filter(|s| !s.is_empty())
}

/// `disk4s1` -> `disk4`.
fn whole_of(part: &str) -> String {
    match part.strip_prefix("disk").and_then(|rest| rest.find('s')) {
        Some(i) => part[..4 + i].to_string(),
        None => part.to_string(),
    }
}

fn describe(id: &str, boot: &[String], usb: &str) -> Result<Disk> {
    let info = diskutil(&["info", id])?;
    let listing = diskutil(&["list", id])?;
    let mut parts = FxHashMap::default();
    let entry = json::array(listing.get("AllDisksAndPartitions")).into_iter().next().cloned().unwrap_or(Value::Null);
    for p in json::array(entry.get("Partitions")) {
        if let Some(pid) = json::str(p, "DeviceIdentifier")
            && let Ok(pinfo) = diskutil(&["info", &pid])
        {
            // APFS containers: their volumes (and mount points) are on a synthesized disk.
            if let Some(container) = json::str(&pinfo, "APFSContainerReference")
                && let Ok(c) = diskutil(&["list", &container])
            {
                parts.insert(format!("{pid}/apfs"), c);
            }
            parts.insert(pid, pinfo);
        }
    }
    let mut disk = parse_disk(&info, &listing, &parts, boot);
    disk.serial = usb_serial(usb, id);
    Ok(disk)
}

/// Build a [`Disk`] from `diskutil info` of the whole disk, `diskutil list` of it, and `info`
/// of each partition (keyed by identifier; `<id>/apfs` holds `list` of an APFS container). The
/// serial number isn't there ([`usb_serial`] has it).
pub(super) fn parse_disk(info: &Value, listing: &Value, parts: &FxHashMap<String, Value>, boot: &[String]) -> Disk {
    let id = json::str(info, "DeviceIdentifier").unwrap_or_default();
    let model = json::str(info, "IORegistryEntryName")
        // IOKit names the media "<product> Media" once; a product can end in " Media" itself.
        .map(|n| n.strip_suffix(" Media").unwrap_or(&n).to_string())
        .or_else(|| json::str(info, "MediaName"))
        .unwrap_or_default();
    let entry = json::array(listing.get("AllDisksAndPartitions")).into_iter().next().cloned().unwrap_or(Value::Null);
    let mut volumes = Vec::new();
    for p in json::array(entry.get("Partitions")) {
        let pid = json::str(p, "DeviceIdentifier").unwrap_or_default();
        let pinfo = parts.get(&pid).unwrap_or(&Value::Null);
        let (filesystem, filesystem_name) = filesystem(pinfo, p);
        volumes.push(Volume {
            id: pid.clone(),
            size: json::u64(p, "Size").unwrap_or(0),
            filesystem,
            filesystem_name,
            label: json::str(p, "VolumeName").or_else(|| json::str(pinfo, "VolumeName")),
            mount: json::str(p, "MountPoint").map(PathBuf::from),
            cluster_size: json::u64(pinfo, "VolumeAllocationBlockSize"),
            uuid: json::str(p, "VolumeUUID").or_else(|| json::str(pinfo, "DiskUUID")),
        });
        if let Some(c) = parts.get(&format!("{pid}/apfs")) {
            for e in json::array(c.get("AllDisksAndPartitions")) {
                for v in json::array(e.get("APFSVolumes")) {
                    volumes.push(Volume {
                        id: json::str(v, "DeviceIdentifier").unwrap_or_default(),
                        size: json::u64(v, "CapacityInUse").or_else(|| json::u64(v, "Size")).unwrap_or(0),
                        filesystem: Some(FsKind::Apfs),
                        filesystem_name: Some("APFS".into()),
                        label: json::str(v, "VolumeName"),
                        mount: json::str(v, "MountPoint").map(PathBuf::from),
                        cluster_size: None,
                        uuid: json::str(v, "VolumeUUID"),
                    });
                }
            }
        }
    }
    // A file system directly on the disk, without a partition table.
    if volumes.is_empty() && json::str(info, "FilesystemType").is_some() {
        let (filesystem, filesystem_name) = filesystem(info, &Value::Null);
        volumes.push(Volume {
            id: id.clone(),
            size: json::u64(info, "Size").unwrap_or(0),
            filesystem,
            filesystem_name,
            label: json::str(info, "VolumeName"),
            mount: json::str(info, "MountPoint").map(PathBuf::from),
            cluster_size: json::u64(info, "VolumeAllocationBlockSize"),
            uuid: json::str(info, "VolumeUUID"),
        });
    }
    let bus = json::str(info, "BusProtocol");
    Disk {
        device: json::str(info, "DeviceNode").unwrap_or_else(|| format!("/dev/{id}")),
        model,
        serial: None,
        size: json::u64(info, "Size").or_else(|| json::u64(info, "TotalSize")).unwrap_or(0),
        sector_size: json::u64(info, "DeviceBlockSize").unwrap_or(512) as u32,
        removable: json::bool(info, "RemovableMediaOrExternalDevice") || json::bool(info, "Removable"),
        internal: json::bool(info, "Internal"),
        system: boot.contains(&id),
        virtual_disk: json::str(info, "VirtualOrPhysical").as_deref() == Some("Virtual")
            || bus.as_deref() == Some("Disk Image"),
        read_only: info.get("WritableMedia").is_some() && !json::bool(info, "WritableMedia"),
        bus,
        scheme: match json::str(info, "Content").as_deref() {
            Some("FDisk_partition_scheme") => Some(Scheme::Mbr),
            Some("GUID_partition_scheme") => Some(Scheme::Gpt),
            Some("Apple_partition_scheme") => Some(Scheme::Apm),
            None => None,
            Some(_) if entry.get("Partitions").is_none() => None,
            Some(_) => Some(Scheme::Other),
        },
        volumes,
        id,
    }
}

fn filesystem(info: &Value, part: &Value) -> (Option<FsKind>, Option<String>) {
    let name = json::str(info, "FilesystemName").or_else(|| json::str(part, "Content"));
    let kind = json::str(info, "FilesystemType").map(|t| match t.as_str() {
        "msdos" => match name.as_deref() {
            Some(n) if n.contains("FAT32") => FsKind::Fat32,
            Some(n) if n.contains("FAT16") => FsKind::Fat16,
            Some(n) if n.contains("FAT12") => FsKind::Fat12,
            _ => FsKind::Other,
        },
        "exfat" => FsKind::Exfat,
        "hfs" => FsKind::Hfsplus,
        "apfs" => FsKind::Apfs,
        "ntfs" => FsKind::Ntfs,
        _ => FsKind::Other,
    });
    (kind, name)
}

pub fn supports(_: Filesystem) -> Result<()> {
    Ok(())
}

fn commands(disk: &Disk, t: &Target) -> Vec<Cmd> {
    let personality = match t.filesystem {
        Filesystem::Fat32 => "MS-DOS FAT32",
        Filesystem::Exfat => "ExFAT",
        // Not journaled: players don't replay a journal.
        Filesystem::Hfsplus => "HFS+",
    };
    let mut out = vec![Cmd::new("diskutil", &["eraseDisk", personality, &t.label, "MBR", &disk.id])];
    if !t.default_cluster {
        // diskutil can't choose a cluster size: recreate the file system in the new partition.
        let raw = format!("/dev/r{}s1", disk.id);
        let bytes = t.cluster_size.to_string();
        out.push(Cmd::new("diskutil", &["unmountDisk", &disk.id]));
        out.push(match t.filesystem {
            Filesystem::Fat32 => Cmd::new("newfs_msdos", &["-F", "32", "-b", &bytes, "-v", &t.label, &raw]),
            _ => Cmd::new("newfs_exfat", &["-R", "-b", &bytes, "-v", &t.label, &raw]),
        });
        out.push(Cmd::new("diskutil", &["mountDisk", &disk.id]));
    }
    out
}

pub fn steps(disk: &Disk, t: &Target) -> Result<Vec<String>> {
    Ok(commands(disk, t).iter().map(Cmd::display).collect())
}

pub fn format(disk: &Disk, t: &Target, ctx: &Ctx<'_>) -> Result<Option<PathBuf>> {
    if !t.default_cluster {
        super::require_write_access(
            &format!("/dev/r{}", disk.id),
            "run it as an administrator, or leave the cluster size at its default",
        )?;
    }
    ctx.phase_start(Phase::Format, 0, 0);
    for c in commands(disk, t) {
        c.run()?;
    }
    ctx.phase_end(Phase::Format);
    let after = describe(&disk.id, &[], "")?;
    Ok(after.volumes.into_iter().find_map(|v| v.mount))
}

#[cfg(test)]
mod tests {
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
        let t =
            Target { filesystem: Filesystem::Fat32, label: "DNA".into(), cluster_size: 32768, default_cluster: true };
        assert_eq!(steps(&d, &t).unwrap(), [r#"diskutil eraseDisk "MS-DOS FAT32" DNA MBR disk4"#]);
        let t = Target {
            filesystem: Filesystem::Exfat,
            label: "My USB".into(),
            cluster_size: 32768,
            default_cluster: false,
        };
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
        let t =
            Target { filesystem: Filesystem::Hfsplus, label: "DNA".into(), cluster_size: 4096, default_cluster: true };
        assert_eq!(steps(&d, &t).unwrap(), ["diskutil eraseDisk HFS+ DNA MBR disk4"]);
        let t =
            Target { filesystem: Filesystem::Fat32, label: "DNA".into(), cluster_size: 16384, default_cluster: false };
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
        let scheme = tc.draw(
            gs::sampled_from(vec![
                ("FDisk_partition_scheme", Some(Scheme::Mbr)),
                ("GUID_partition_scheme", Some(Scheme::Gpt)),
                ("Apple_partition_scheme", Some(Scheme::Apm)),
                ("Sun_partition_scheme", Some(Scheme::Other)),
            ])
            .print_as_debug(),
        );
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
            let (fs_type, fs_name, kind) = tc.draw(gs::sampled_from(FILESYSTEMS.to_vec()).print_as_debug());
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
        let (fs_type, fs_name, kind) = tc.draw(gs::sampled_from(FILESYSTEMS.to_vec()).print_as_debug());
        let (size, label) = (tc.draw(gs::integers::<u64>()), word(&tc));
        let content = tc.draw(gs::sampled_from(vec!["DOS_FAT_32", "Apple_HFS"]));
        let info = json!({ "DeviceIdentifier": "disk7", "Content": content, "Size": size, "FilesystemType": fs_type,
            "FilesystemName": fs_name, "VolumeName": label, "TotalSize": 1 });
        let listing = json!({ "AllDisksAndPartitions": [{ "DeviceIdentifier": "disk7", "Content": content }] });
        let d = parse_disk(&info, &listing, &FxHashMap::default(), &[]);
        assert_eq!(d.scheme, None, "no partition table");
        assert_eq!(d.volumes.len(), 1);
        let v = &d.volumes[0];
        assert_eq!(
            (v.id.as_str(), v.size, v.filesystem, v.label.as_deref()),
            ("disk7", size, Some(kind), Some(&*label))
        );
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
        let filesystem =
            tc.draw(gs::sampled_from(vec![Filesystem::Fat32, Filesystem::Exfat, Filesystem::Hfsplus]).print_as_debug());
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
}
