//! Windows: the Storage cmdlets (PowerShell) partition the disk and create exFAT. FAT32 is
//! written by [`super::fat32`], because Windows refuses to create FAT32 volumes above 32 GB.
//! Needs an elevated (administrator) process.

use std::path::{Path, PathBuf};

use base64::prelude::*;
use serde_json::Value;

use super::{Cmd, Disk, Filesystem, FsKind, Scheme, Target, Volume, fat32, json};
use crate::progress::{Ctx, Phase};
use crate::{Error, Result};

/// The new volume gets a drive letter.
pub const MOUNTS_AFTER_FORMAT: bool = true;

const LIST_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$disks = @(Get-Disk | ForEach-Object {
  $d = $_
  $parts = @(Get-Partition -DiskNumber $d.Number -ErrorAction SilentlyContinue | ForEach-Object {
    $v = $null
    try { $v = $_ | Get-Volume -ErrorAction Stop } catch {}
    [pscustomobject]@{
      Number = $_.PartitionNumber; Offset = $_.Offset; Size = $_.Size; DriveLetter = [string]$_.DriveLetter
      Guid = [string]$_.Guid; FileSystem = [string]$v.FileSystem; Label = [string]$v.FileSystemLabel
      AllocationUnitSize = $v.AllocationUnitSize
    }
  })
  [pscustomobject]@{
    Number = $d.Number; FriendlyName = [string]$d.FriendlyName; SerialNumber = [string]$d.SerialNumber
    Size = $d.Size; LogicalSectorSize = $d.LogicalSectorSize; BusType = [string]$d.BusType
    IsBoot = [bool]$d.IsBoot; IsSystem = [bool]$d.IsSystem; IsReadOnly = [bool]$d.IsReadOnly
    PartitionStyle = [string]$d.PartitionStyle; Partitions = $parts
  }
})
ConvertTo-Json -InputObject $disks -Depth 5 -Compress
"#;

/// Run a script with Windows PowerShell. `-EncodedCommand` (UTF-16LE, base64) avoids both
/// quoting and `-Command -`'s line-by-line reading of standard input.
fn powershell(script: &str) -> Cmd {
    let script = format!("$ProgressPreference = 'SilentlyContinue'\n{script}");
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = BASE64_STANDARD.encode(&utf16);
    Cmd::new(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &encoded],
    )
}

fn all_disks() -> Result<Vec<Disk>> {
    parse_disks(&powershell(LIST_SCRIPT).run()?)
}

/// The disks in what [`LIST_SCRIPT`] prints.
fn parse_disks(text: &str) -> Result<Vec<Disk>> {
    let v: Value = serde_json::from_str(text.trim()).map_err(|e| Error::parse("Get-Disk output", e))?;
    Ok(json::array(Some(&v)).into_iter().map(parse_disk).collect())
}

pub fn list() -> Result<Vec<Disk>> {
    all_disks()
}

pub fn by_id(spec: &str) -> Result<Option<Disk>> {
    let s = spec.trim_start_matches(r"\\.\");
    let Some(n) = s.strip_prefix("PhysicalDrive").or_else(|| s.strip_prefix("PHYSICALDRIVE")) else { return Ok(None) };
    let n: u32 = n.parse().map_err(|_| Error::invalid(format!("not a disk: {spec:?}")))?;
    Ok(all_disks()?.into_iter().find(|d| d.id == format!("PhysicalDrive{n}")))
}

pub fn by_mount(path: &Path) -> Result<Disk> {
    let letter = root_letter(path)?;
    all_disks()?
        .into_iter()
        .find(|d| d.volumes.iter().any(|v| v.mount.as_deref().and_then(drive_letter) == Some(letter)))
        .ok_or_else(|| Error::NotFound(format!("the disk holding {}", path.display())))
}

/// The letter of the drive whose root is `root` (`E:\` -> `E`), refusing any other path.
pub(crate) fn root_letter(root: &Path) -> Result<char> {
    drive_letter(root)
        .ok_or_else(|| Error::invalid(format!("{} is not the root of a drive (like E:\\)", root.display())))
}

/// `E:\` -> `E` (only for drive roots).
fn drive_letter(path: &Path) -> Option<char> {
    let s = path.to_string_lossy();
    let s = s.trim_start_matches(r"\\?\");
    let mut c = s.chars();
    let letter = c.next().filter(char::is_ascii_alphabetic)?;
    (c.next() == Some(':') && c.all(|c| c == '\\' || c == '/')).then(|| letter.to_ascii_uppercase())
}

pub(super) fn parse_disk(d: &Value) -> Disk {
    let number = json::u64(d, "Number").unwrap_or(u64::MAX);
    let bus = json::str(d, "BusType");
    let removable = matches!(bus.as_deref(), Some("USB" | "SD" | "MMC"));
    let virtual_disk = matches!(bus.as_deref(), Some("File Backed Virtual" | "Virtual"));
    let volumes = json::array(d.get("Partitions"))
        .into_iter()
        .map(|p| {
            let letter = json::str(p, "DriveLetter").and_then(|l| l.chars().next()).filter(char::is_ascii_alphabetic);
            let fs = json::str(p, "FileSystem");
            Volume {
                id: format!("{number}:{}", json::u64(p, "Number").unwrap_or(0)),
                size: json::u64(p, "Size").unwrap_or(0),
                filesystem: fs.as_deref().map(|f| match f {
                    "FAT32" => FsKind::Fat32,
                    "FAT" => FsKind::Fat16,
                    "exFAT" => FsKind::Exfat,
                    "NTFS" => FsKind::Ntfs,
                    _ => FsKind::Other,
                }),
                filesystem_name: fs,
                label: json::str(p, "Label"),
                mount: letter.map(|l| PathBuf::from(format!("{l}:\\"))),
                cluster_size: json::u64(p, "AllocationUnitSize").filter(|&n| n > 0),
                uuid: json::str(p, "Guid"),
            }
        })
        .collect();
    Disk {
        id: format!("PhysicalDrive{number}"),
        device: format!(r"\\.\PhysicalDrive{number}"),
        model: json::str(d, "FriendlyName").unwrap_or_default(),
        serial: json::str(d, "SerialNumber"),
        size: json::u64(d, "Size").unwrap_or(0),
        sector_size: json::u64(d, "LogicalSectorSize").unwrap_or(512) as u32,
        removable: removable || virtual_disk,
        internal: !removable && !virtual_disk,
        system: json::bool(d, "IsBoot") || json::bool(d, "IsSystem"),
        virtual_disk,
        read_only: json::bool(d, "IsReadOnly"),
        bus,
        scheme: match json::str(d, "PartitionStyle").as_deref() {
            Some("MBR") => Some(Scheme::Mbr),
            Some("GPT") => Some(Scheme::Gpt),
            Some("RAW") | None => None,
            Some(_) => Some(Scheme::Other),
        },
        volumes,
    }
}

pub fn supports(fs: Filesystem) -> Result<()> {
    if fs == Filesystem::Hfsplus {
        return Err(Error::invalid("Windows can't create HFS+ volumes; use FAT32 or exFAT, or format on a Mac"));
    }
    Ok(())
}

/// `s` as a PowerShell string that takes it as it is.
pub(crate) fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Repartition (one MBR partition) and create exFAT; print the new partition as JSON. For a
/// FAT32 target the exFAT volume is a placeholder that gives the partition a drive letter
/// without Windows offering to format a RAW volume; it is replaced right after. First, the disk
/// under that number must still be the planned one (its size and serial) and not the system's.
fn partition_script(disk: &Disk, t: &Target) -> String {
    let n = disk.id.trim_start_matches("PhysicalDrive");
    let (mbr_type, cluster) = match t.filesystem {
        Filesystem::Fat32 => ("FAT32", String::new()),
        _ => ("IFS", format!(" -AllocationUnitSize {}", t.cluster_size)),
    };
    format!(
        r#"$ErrorActionPreference = 'Stop'
$n = {n}
$d = Get-Disk -Number $n
if ($d.IsBoot -or $d.IsSystem) {{ throw 'the disk holds the running system' }}
if ([uint64]$d.Size -ne {size} -or ([string]$d.SerialNumber).Trim().Trim([char]0) -ne {serial}) {{ throw 'this is not the disk that was planned' }}
if ($d.IsReadOnly) {{ throw 'the disk is write-protected' }}
if ($d.IsOffline) {{ Set-Disk -Number $n -IsOffline $false }}
if ($d.PartitionStyle -ne 'RAW') {{ Clear-Disk -Number $n -RemoveData -RemoveOEM -Confirm:$false }}
Initialize-Disk -Number $n -PartitionStyle MBR
$p = New-Partition -DiskNumber $n -UseMaximumSize -MbrType {mbr_type}
$null = Format-Volume -Partition $p -FileSystem exFAT -NewFileSystemLabel {label}{cluster} -Force -Confirm:$false
$p = Get-Partition -DiskNumber $n -PartitionNumber $p.PartitionNumber
if ([int]$p.DriveLetter -eq 0) {{ $p | Add-PartitionAccessPath -AssignDriveLetter; $p = Get-Partition -DiskNumber $n -PartitionNumber $p.PartitionNumber }}
ConvertTo-Json -Compress -InputObject ([pscustomobject]@{{ DriveLetter = [string]$p.DriveLetter; Offset = $p.Offset; Size = $p.Size }})
"#,
        label = quote(&t.label),
        size = disk.size,
        serial = quote(disk.serial.as_deref().unwrap_or("")),
    )
}

pub fn steps(disk: &Disk, t: &Target) -> Result<Vec<String>> {
    let mut out = vec![format!(
        "PowerShell: Clear-Disk, Initialize-Disk -PartitionStyle MBR, New-Partition -UseMaximumSize, Format-Volume -FileSystem exFAT{} on disk {}",
        if t.filesystem == Filesystem::Exfat {
            format!(" -AllocationUnitSize {}", t.cluster_size)
        } else {
            String::new()
        },
        disk.id.trim_start_matches("PhysicalDrive")
    )];
    if t.filesystem == Filesystem::Fat32 {
        out.push(format!("write FAT32 ({} KB clusters) over the new volume", t.cluster_size / 1024));
    }
    Ok(out)
}

pub fn format(disk: &Disk, t: &Target, ctx: &Ctx<'_>) -> Result<Option<PathBuf>> {
    super::require_write_access(&disk.device, "run it as an administrator")?;
    ctx.phase_start(Phase::Format, 0, 0);
    let out = powershell(&partition_script(disk, t)).run()?;
    ctx.phase_end(Phase::Format);
    let p: Value = serde_json::from_str(out.trim()).map_err(|e| Error::parse("New-Partition output", e))?;
    let letter =
        json::str(&p, "DriveLetter").and_then(|l| l.chars().next()).filter(char::is_ascii_alphabetic).ok_or_else(
            || Error::External { command: "Add-PartitionAccessPath".into(), message: "no drive letter".into() },
        )?;
    if t.filesystem == Filesystem::Fat32 {
        let offset = json::u64(&p, "Offset").unwrap_or(super::PARTITION_START);
        let size = json::u64(&p, "Size").unwrap_or_else(|| disk.partition_size());
        let layout =
            fat32::Layout::new(size, disk.sector_size, t.cluster_size, (offset / u64::from(disk.sector_size)) as u32)?;
        ctx.phase_start(Phase::Format, 0, layout.bytes_written());
        write_fat32(letter, &layout, &t.label, ctx)?;
        ctx.phase_end(Phase::Format);
    }
    let root = PathBuf::from(format!("{letter}:\\"));
    // The first access mounts the new file system.
    let _ = std::fs::read_dir(&root);
    Ok(Some(root))
}

#[cfg(windows)]
fn write_fat32(letter: char, layout: &fat32::Layout, label: &str, ctx: &Ctx<'_>) -> Result<()> {
    use std::os::windows::io::AsRawHandle;

    let path = format!(r"\\.\{letter}:");
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| Error::Io { path: path.clone().into(), source: e })?;
    // Explorer and indexers open new volumes briefly; retry the lock for a few seconds.
    let mut tries = 0;
    while let Err(e) = ffi::ioctl(f.as_raw_handle(), ffi::FSCTL_LOCK_VOLUME) {
        tries += 1;
        if tries == 40 {
            return Err(Error::External {
                command: format!("locking {path}"),
                message: format!("{e} (close programs using the drive)"),
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    ffi::ioctl(f.as_raw_handle(), ffi::FSCTL_DISMOUNT_VOLUME)
        .map_err(|e| Error::Io { path: path.clone().into(), source: e })?;
    let volume_id = u32::from_le_bytes(uuid::Uuid::new_v4().as_bytes()[..4].try_into().expect("4 bytes"));
    fat32::write(&mut f, 0, layout, label, volume_id, ctx)?;
    f.sync_all().map_err(|e| Error::Io { path: path.into(), source: e })?;
    // Closing the handle unlocks the volume; Windows mounts it as FAT32 on the next access.
    Ok(())
}

#[cfg(not(windows))]
fn write_fat32(_: char, _: &fat32::Layout, _: &str, _: &Ctx<'_>) -> Result<()> {
    Err(Error::invalid("raw volume access is only implemented on Windows"))
}

#[cfg(windows)]
mod ffi {
    use std::ffi::c_void;

    pub const FSCTL_LOCK_VOLUME: u32 = 0x0009_0018;
    pub const FSCTL_DISMOUNT_VOLUME: u32 = 0x0009_0020;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn DeviceIoControl(
            device: *mut c_void,
            code: u32,
            in_buffer: *const c_void,
            in_size: u32,
            out_buffer: *mut c_void,
            out_size: u32,
            returned: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
    }

    pub fn ioctl(handle: *mut c_void, code: u32) -> std::io::Result<()> {
        let mut returned = 0u32;
        // SAFETY: a valid handle owned by the caller, no buffers, synchronous call.
        let ok = unsafe {
            DeviceIoControl(
                handle,
                code,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok != 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn listing() -> Value {
        json!([
            { "Number": 0, "FriendlyName": "Samsung SSD 990", "Size": 1000204886016u64, "LogicalSectorSize": 512,
              "BusType": "NVMe", "IsBoot": true, "IsSystem": true, "IsReadOnly": false, "PartitionStyle": "GPT",
              "Partitions": [{ "Number": 3, "Offset": 290455552, "Size": 999000000000u64, "DriveLetter": "C",
                               "FileSystem": "NTFS", "Label": "", "AllocationUnitSize": 4096 }] },
            // PowerShell 5.1's array wrapper, a partition without a letter (DriveLetter is NUL).
            { "Number": 2, "FriendlyName": "SanDisk Ultra", "SerialNumber": "4C53", "Size": 64023257088u64,
              "LogicalSectorSize": 512, "BusType": "USB", "IsBoot": false, "IsSystem": false, "IsReadOnly": false,
              "PartitionStyle": "MBR",
              "Partitions": { "value": [{ "Number": 1, "Offset": 1048576, "Size": 64022208512u64, "DriveLetter": "\u{0}",
                                          "FileSystem": "exFAT", "Label": "USB", "AllocationUnitSize": 131072 }], "Count": 1 } }
        ])
    }

    #[test]
    fn parses_get_disk() {
        let v = listing();
        let disks: Vec<Disk> = json::array(Some(&v)).into_iter().map(parse_disk).collect();
        assert!(disks[0].system && disks[0].internal);
        assert_eq!(disks[0].volumes[0].mount.as_deref(), Some(Path::new("C:\\")));
        let usb = &disks[1];
        assert_eq!(usb.id, "PhysicalDrive2");
        assert_eq!(usb.device, r"\\.\PhysicalDrive2");
        assert!(usb.removable && !usb.internal && !usb.system);
        assert_eq!(usb.volumes.len(), 1);
        assert_eq!(usb.volumes[0].mount, None);
        assert_eq!(usb.volumes[0].filesystem, Some(FsKind::Exfat));
        assert_eq!(usb.volumes[0].cluster_size, Some(131072));
    }

    #[test]
    fn drive_letters() {
        assert_eq!(drive_letter(Path::new("E:\\")), Some('E'));
        assert_eq!(drive_letter(Path::new(r"\\?\e:\")), Some('E'));
        assert_eq!(drive_letter(Path::new("E:\\Music")), None);
        assert_eq!(drive_letter(Path::new("/Volumes/E")), None);
    }

    #[test]
    fn partition_script_quotes_the_label() {
        let v = listing();
        let d = parse_disk(json::array(Some(&v))[1]);
        let t = Target {
            filesystem: Filesystem::Exfat,
            label: "DJ's USB".into(),
            cluster_size: 32768,
            default_cluster: false,
        };
        let s = partition_script(&d, &t);
        assert!(s.contains("$n = 2\n"));
        // The disk under that number is the one planned, and not the system's, before it's cleared.
        let check =
            "if ([uint64]$d.Size -ne 64023257088 -or ([string]$d.SerialNumber).Trim().Trim([char]0) -ne '4C53')";
        let (checked, cleared) = (s.find(check).unwrap(), s.find("Clear-Disk").unwrap());
        assert!(checked < cleared && s.find("$d.IsBoot -or $d.IsSystem").unwrap() < cleared, "{s}");
        assert!(s.contains("-MbrType IFS"));
        assert!(s.contains("-NewFileSystemLabel 'DJ''s USB' -AllocationUnitSize 32768"));
        let t =
            Target { filesystem: Filesystem::Fat32, label: "USB".into(), cluster_size: 32768, default_cluster: true };
        assert!(partition_script(&d, &t).contains("-MbrType FAT32"));
        assert!(supports(Filesystem::Hfsplus).is_err());
    }

    /// PowerShell reads `-EncodedCommand` as base64 of UTF-16LE (the value here is Python's
    /// `base64.b64encode(script.encode("utf-16-le"))`).
    #[test]
    fn scripts_go_to_powershell_encoded() {
        let c = powershell("Get-Disk");
        assert_eq!(c.program, "powershell");
        assert_eq!(
            c.args,
            [
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-EncodedCommand",
                "JABQAHIAbwBnAHIAZQBzAHMAUAByAGUAZgBlAHIAZQBuAGMAZQAgAD0AIAAnAFMAaQBsAGUAbgB0AGwAeQBDAG8AbgB0AGkAbgB1AGUAJwAKAEcAZQB0AC0ARABpAHMAawA="
            ]
        );
    }

    /// What `LIST_SCRIPT` prints on a desktop with a card reader, an attached VHD and a USB hard
    /// drive (Windows PowerShell 5.1, with its trailing newline).
    const DESKTOP: &str = concat!(
        r#"[{"Number":1,"FriendlyName":"Generic STORAGE DEVICE","SerialNumber":"000000000819","Size":31914983424,"LogicalSectorSize":512,"BusType":"SD","IsBoot":false,"IsSystem":false,"IsReadOnly":false,"PartitionStyle":"MBR","Partitions":[{"Number":1,"Offset":4194304,"Size":31910789120,"DriveLetter":"F","Guid":"","FileSystem":"FAT32","Label":"REKORDBOX","AllocationUnitSize":32768}]},"#,
        r#"{"Number":3,"FriendlyName":"Msft Virtual Disk","SerialNumber":"","Size":1073741824,"LogicalSectorSize":512,"BusType":"File Backed Virtual","IsBoot":false,"IsSystem":false,"IsReadOnly":true,"PartitionStyle":"RAW","Partitions":[]},"#,
        r#"{"Number":4,"FriendlyName":"WD My Passport 25E2","SerialNumber":"575836314134","Size":2000365289472,"LogicalSectorSize":512,"BusType":"USB","IsBoot":false,"IsSystem":false,"IsReadOnly":false,"PartitionStyle":"GPT","Partitions":[{"Number":1,"Offset":17408,"Size":16759808,"DriveLetter":"\u0000","Guid":"{0a1b2c3d-0000-4000-8000-000000000001}","FileSystem":"","Label":"","AllocationUnitSize":null},{"Number":2,"Offset":16777216,"Size":2000348446720,"DriveLetter":"G","Guid":"{0a1b2c3d-0000-4000-8000-000000000002}","FileSystem":"NTFS","Label":"My Passport","AllocationUnitSize":4096}]}]"#,
        "\r\n"
    );

    #[test]
    fn a_desktops_disks_are_read_from_get_disk() {
        let disks = parse_disks(DESKTOP).unwrap();
        let ids: Vec<&str> = disks.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["PhysicalDrive1", "PhysicalDrive3", "PhysicalDrive4"]);

        let card = &disks[0];
        assert!(card.removable && card.problem(&[]).is_none());
        assert_eq!((card.scheme, card.volumes[0].filesystem), (Some(Scheme::Mbr), Some(FsKind::Fat32)));
        assert_eq!(card.volumes[0].mount.as_deref(), Some(Path::new("F:\\")));

        let vhd = &disks[1];
        assert!(vhd.virtual_disk && vhd.read_only && vhd.volumes.is_empty());
        assert_eq!(vhd.scheme, None, "RAW: not initialised");

        let passport = &disks[2];
        assert_eq!(passport.scheme, Some(Scheme::Gpt));
        // The reserved partition has no file system, letter or cluster size.
        let reserved = &passport.volumes[0];
        assert_eq!((reserved.filesystem, &reserved.mount, reserved.cluster_size), (None, &None, None));
        assert_eq!(passport.volumes[1].filesystem, Some(FsKind::Ntfs));
        assert_eq!(passport.volumes[1].id, "4:2");

        assert!(matches!(parse_disks("Get-Disk : Access denied"), Err(Error::Parse { .. })));
    }

    #[test]
    fn other_file_systems_and_partition_styles_are_told_apart() {
        let d = parse_disk(&json!({
            "Number": 5, "BusType": "USB", "PartitionStyle": "Unknown",
            "Partitions": [{ "Number": 1, "FileSystem": "FAT" }, { "Number": 2, "FileSystem": "exFAT" }, { "Number": 3, "FileSystem": "ReFS" }]
        }));
        let kinds: Vec<Option<FsKind>> = d.volumes.iter().map(|v| v.filesystem).collect();
        assert_eq!(kinds, [Some(FsKind::Fat16), Some(FsKind::Exfat), Some(FsKind::Other)]);
        assert_eq!(d.scheme, Some(Scheme::Other));
    }

    /// Only `PhysicalDriveN` (or `\\.\PhysicalDriveN`) names a disk, and only a drive's root has
    /// one; neither runs PowerShell to say no.
    #[test]
    fn disks_are_named_by_number() {
        for spec in ["Backup", "disk4", "/dev/sdb"] {
            assert!(by_id(spec).unwrap().is_none(), "{spec}");
        }
        assert!(matches!(by_id(r"\\.\PhysicalDriveX"), Err(Error::Invalid(_))));
        assert!(by_mount(Path::new("/Volumes/DNA")).is_err());
    }

    #[test]
    fn each_file_system_is_planned_in_its_own_steps() {
        let d = parse_disks(DESKTOP).unwrap().remove(0);
        let t =
            Target { filesystem: Filesystem::Exfat, label: "USB".into(), cluster_size: 131072, default_cluster: true };
        assert_eq!(
            steps(&d, &t).unwrap(),
            [
                "PowerShell: Clear-Disk, Initialize-Disk -PartitionStyle MBR, New-Partition -UseMaximumSize, Format-Volume -FileSystem exFAT -AllocationUnitSize 131072 on disk 1"
            ]
        );
        let t =
            Target { filesystem: Filesystem::Fat32, label: "USB".into(), cluster_size: 32768, default_cluster: true };
        let s = steps(&d, &t).unwrap();
        assert!(s[0].ends_with("Format-Volume -FileSystem exFAT on disk 1"), "{s:?}");
        assert_eq!(s[1], "write FAT32 (32 KB clusters) over the new volume");
        assert!(supports(Filesystem::Fat32).is_ok() && supports(Filesystem::Exfat).is_ok());
    }
}
