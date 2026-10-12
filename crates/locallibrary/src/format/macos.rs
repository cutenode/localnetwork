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
mod tests;
