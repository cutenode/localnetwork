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
    let t =
        Target { filesystem: Filesystem::Exfat, label: "DJ's USB".into(), cluster_size: 32768, default_cluster: false };
    let s = partition_script(&d, &t);
    assert!(s.contains("$n = 2\n"));
    // The disk under that number is the one planned, and not the system's, before it's cleared.
    let check = "if ([uint64]$d.Size -ne 64023257088 -or ([string]$d.SerialNumber).Trim().Trim([char]0) -ne '4C53')";
    let (checked, cleared) = (s.find(check).unwrap(), s.find("Clear-Disk").unwrap());
    assert!(checked < cleared && s.find("$d.IsBoot -or $d.IsSystem").unwrap() < cleared, "{s}");
    assert!(s.contains("-MbrType IFS"));
    assert!(s.contains("-NewFileSystemLabel 'DJ''s USB' -AllocationUnitSize 32768"));
    let t = Target { filesystem: Filesystem::Fat32, label: "USB".into(), cluster_size: 32768, default_cluster: true };
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
    let t = Target { filesystem: Filesystem::Exfat, label: "USB".into(), cluster_size: 131072, default_cluster: true };
    assert_eq!(
        steps(&d, &t).unwrap(),
        [
            "PowerShell: Clear-Disk, Initialize-Disk -PartitionStyle MBR, New-Partition -UseMaximumSize, Format-Volume -FileSystem exFAT -AllocationUnitSize 131072 on disk 1"
        ]
    );
    let t = Target { filesystem: Filesystem::Fat32, label: "USB".into(), cluster_size: 32768, default_cluster: true };
    let s = steps(&d, &t).unwrap();
    assert!(s[0].ends_with("Format-Volume -FileSystem exFAT on disk 1"), "{s:?}");
    assert_eq!(s[1], "write FAT32 (32 KB clusters) over the new volume");
    assert!(supports(Filesystem::Fat32).is_ok() && supports(Filesystem::Exfat).is_ok());
}

// -----------------------------------------------------------------------------------------
// Properties
// -----------------------------------------------------------------------------------------

use hegel::generators as gs;
use hegel::{Generator, TestCase};

/// `FileSystem` as Get-Volume says it, and what it means.
const FILESYSTEMS: [(&str, Option<FsKind>); 6] = [
    ("FAT32", Some(FsKind::Fat32)),
    ("FAT", Some(FsKind::Fat16)),
    ("exFAT", Some(FsKind::Exfat)),
    ("NTFS", Some(FsKind::Ntfs)),
    ("ReFS", Some(FsKind::Other)),
    ("", None),
];

/// One of `items` as PowerShell prints an array: as it is, unwrapped when there's one, or in
/// Windows PowerShell 5.1's `{"value": [...], "Count": n}`.
#[hegel::test_helper]
fn ps_array(tc: &TestCase, items: Vec<Value>) -> Value {
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 if items.len() == 1 => items[0].clone(),
        1 => json!({ "value": items, "Count": items.len() }),
        _ => Value::Array(items),
    }
}

/// What [`LIST_SCRIPT`] prints reads as the disks in it, field for field: a USB, SD or MMC
/// bus makes a disk removable, a virtual one is a virtual disk, partitions are volumes with a
/// drive letter as their mount point, and a cluster size of 0 is none.
#[hegel::test(test_cases = 300)]
fn get_disk_listings_read_back(tc: TestCase) {
    let mut listed = Vec::new();
    let mut want = Vec::new();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
        let number = tc.draw(gs::integers::<u32>().max_value(20));
        let bus = tc.draw(gs::sampled_from(vec!["USB", "SD", "MMC", "SATA", "NVMe", "File Backed Virtual", "Virtual"]));
        let (boot, system, ro) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()), tc.draw(gs::booleans()));
        let style = tc.draw(
            gs::sampled_from(vec![
                (Some("MBR"), Some(Scheme::Mbr)),
                (Some("GPT"), Some(Scheme::Gpt)),
                (Some("RAW"), None),
                (None, None),
                (Some("Unknown"), Some(Scheme::Other)),
            ])
            .print_as_debug(),
        );
        let (size, sector) = (tc.draw(gs::integers::<u64>()), tc.draw(gs::sampled_from(vec![512u64, 4096])));
        let name = tc.draw(gs::sampled_from(vec!["SanDisk Ultra", "Msft Virtual Disk", ""]));
        let serial = tc.draw(gs::sampled_from(vec!["4C530001", "", " 000819 "]));
        let mut parts = Vec::new();
        let mut volumes = Vec::new();
        for _ in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
            let pn = tc.draw(gs::integers::<u32>().max_value(8));
            let (fs, kind) = tc.draw(gs::sampled_from(FILESYSTEMS.to_vec()).print_as_debug());
            let letter = tc.draw(gs::sampled_from(vec!["E", "g", "", "\u{0}"]));
            let cluster = tc.draw(gs::sampled_from(vec![None, Some(0u64), Some(4096), Some(131072)]));
            let psize = tc.draw(gs::integers::<u64>());
            let label = tc.draw(gs::sampled_from(vec!["", "REKORDBOX", "My Passport"]));
            parts.push(json!({ "Number": pn, "Offset": 1048576, "Size": psize, "DriveLetter": letter,
                    "Guid": "", "FileSystem": fs, "Label": label, "AllocationUnitSize": cluster }));
            let letter = letter.chars().next().filter(char::is_ascii_alphabetic);
            volumes.push(Volume {
                id: format!("{number}:{pn}"),
                size: psize,
                filesystem: kind,
                filesystem_name: (!fs.is_empty()).then(|| fs.to_string()),
                label: (!label.is_empty()).then(|| label.to_string()),
                mount: letter.map(|l| PathBuf::from(format!("{l}:\\"))),
                cluster_size: cluster.filter(|&c| c > 0),
                uuid: None,
            });
        }
        listed.push(json!({ "Number": number, "FriendlyName": name, "SerialNumber": serial, "Size": size,
                "LogicalSectorSize": sector, "BusType": bus, "IsBoot": boot, "IsSystem": system, "IsReadOnly": ro,
                "PartitionStyle": style.0, "Partitions": ps_array(&tc, parts) }));
        let removable = matches!(bus, "USB" | "SD" | "MMC");
        let virtual_disk = bus.contains("Virtual");
        want.push(Disk {
            id: format!("PhysicalDrive{number}"),
            device: format!(r"\\.\PhysicalDrive{number}"),
            model: name.into(),
            serial: (!serial.trim().is_empty()).then(|| serial.trim().to_string()),
            size,
            sector_size: sector as u32,
            bus: Some(bus.into()),
            removable: removable || virtual_disk,
            internal: !removable && !virtual_disk,
            system: boot || system,
            virtual_disk,
            read_only: ro,
            scheme: style.1,
            volumes,
        });
    }
    let text = format!("{}\r\n", serde_json::to_string(&ps_array(&tc, listed)).unwrap());
    let got = parse_disks(&text).unwrap();
    assert_eq!(serde_json::to_value(&got).unwrap(), serde_json::to_value(&want).unwrap());
    let cut = tc.draw(gs::integers::<usize>().max_value(text.trim_end().len().saturating_sub(1)));
    if text.trim_end().len() > 2 && cut > 0 {
        assert!(matches!(parse_disks(&text[..cut]), Err(Error::Parse { .. })), "{:?}", &text[..cut]);
    }
}

/// Only a drive's root (`E:`, `e:\`, `\\?\E:/`) has a drive letter, upper-cased; nothing
/// below a root or outside Windows paths does.
#[hegel::test(test_cases = 300)]
fn only_drive_roots_have_a_letter(tc: TestCase) {
    let letter = tc.draw(gs::sampled_from(('a'..='z').chain('A'..='Z').collect::<Vec<char>>()));
    let prefix = tc.draw(gs::sampled_from(vec!["", r"\\?\"]));
    let seps: String = tc.draw(gs::text().alphabet("\\/").max_size(3));
    let root = format!("{prefix}{letter}:{seps}");
    assert_eq!(drive_letter(Path::new(&root)), Some(letter.to_ascii_uppercase()));
    assert_eq!(root_letter(Path::new(&root)).unwrap(), letter.to_ascii_uppercase());
    let below: String =
        tc.draw(gs::text().min_size(1).max_size(8).filter(|s: &String| !s.chars().all(|c| c == '\\' || c == '/')));
    let deeper = format!("{root}{below}");
    assert_eq!(drive_letter(Path::new(&deeper)), None, "{deeper:?}");
    assert!(matches!(root_letter(Path::new(&deeper)), Err(Error::Invalid(_))));
    let other = tc.draw(gs::text().max_size(8).filter(|s: &String| s.chars().nth(1) != Some(':')));
    assert_eq!(drive_letter(Path::new(&other)), None, "{other:?}");
}

/// A PowerShell single-quoted string reads back as exactly the text quoted, whatever it holds.
#[hegel::test(test_cases = 300)]
fn powershell_quoting_reads_back(tc: TestCase) {
    let s = tc.draw(hegel::one_of!(gs::text().max_size(20), gs::text().alphabet("a'’ $`\"").max_size(10)));
    let q = quote(&s);
    let inner = q.strip_prefix('\'').and_then(|q| q.strip_suffix('\'')).unwrap();
    // Inside, a quote only ever comes doubled.
    assert!(inner.split("''").all(|part| !part.contains('\'')), "{q}");
    assert_eq!(inner.replace("''", "'"), s);
}

/// Scripts reach PowerShell as base64 of UTF-16LE, with progress output silenced first.
#[hegel::test(test_cases = 100)]
fn scripts_reach_powershell_intact(tc: TestCase) {
    let script = tc.draw(gs::text().max_size(60));
    let c = powershell(&script);
    assert_eq!(c.args[..5], ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand"]);
    let bytes = BASE64_STANDARD.decode(&c.args[5]).unwrap();
    let units: Vec<u16> = bytes.chunks(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
    let decoded = String::from_utf16(&units).unwrap();
    assert_eq!(decoded, format!("$ProgressPreference = 'SilentlyContinue'\n{script}"));
}

/// The partitioning script works on the planned disk's number, refuses a disk of another size
/// or serial number before clearing anything, and labels the volume with the quoted label;
/// the steps say the same, and FAT32 is written over the placeholder exFAT volume.
#[hegel::test(test_cases = 200)]
fn partition_scripts_follow_the_plan(tc: TestCase) {
    let number = tc.draw(gs::integers::<u32>().max_value(30));
    let disk = Disk {
        id: format!("PhysicalDrive{number}"),
        serial: tc.draw(gs::sampled_from(vec![None, Some("4C53".to_string()), Some("O'Neil".to_string())])),
        size: tc.draw(gs::integers::<u64>()),
        ..parse_disk(&json!({ "Number": number, "BusType": "USB" }))
    };
    let filesystem = tc.draw(gs::sampled_from(vec![Filesystem::Fat32, Filesystem::Exfat]).print_as_debug());
    let label = filesystem.label(&tc.draw(gs::text().alphabet("ab 'é").max_size(12)));
    let cluster_size = 1 << tc.draw(gs::integers::<u32>().min_value(9).max_value(15));
    let t = Target { filesystem, label: label.clone(), cluster_size, default_cluster: tc.draw(gs::booleans()) };
    let s = partition_script(&disk, &t);
    assert!(s.contains(&format!("$n = {number}\n")));
    let check = format!(
        "if ([uint64]$d.Size -ne {} -or ([string]$d.SerialNumber).Trim().Trim([char]0) -ne {})",
        disk.size,
        quote(disk.serial.as_deref().unwrap_or(""))
    );
    let (checked, cleared) = (s.find(&check).unwrap(), s.find("Clear-Disk").unwrap());
    assert!(checked < cleared && s.find("$d.IsBoot -or $d.IsSystem").unwrap() < cleared);
    let format_volume = format!("-NewFileSystemLabel {}", quote(&label));
    assert!(s.contains(&format_volume), "{s}");
    let sized = format!("-AllocationUnitSize {cluster_size}");
    let steps = steps(&disk, &t).unwrap();
    assert!(steps[0].ends_with(&format!("on disk {number}")));
    match filesystem {
        Filesystem::Fat32 => {
            assert!(s.contains("-MbrType FAT32") && !s.contains(&sized));
            assert_eq!(steps[1], format!("write FAT32 ({} KB clusters) over the new volume", cluster_size / 1024));
        }
        _ => {
            assert!(s.contains("-MbrType IFS") && s.contains(&sized) && steps[0].contains(&sized));
            assert_eq!(steps.len(), 1);
        }
    }
    assert!(supports(filesystem).is_ok());
    let spec =
        tc.draw(gs::sampled_from(vec!["PhysicalDrive", r"\\.\PHYSICALDRIVE", "PhysicalDrive-1", "PhysicalDrive 2"]));
    assert!(matches!(by_id(spec), Err(Error::Invalid(_))), "{spec}");
}

/// Only a drive's root names a disk to look for: any other path is refused as invalid, naming
/// it, before PowerShell is asked anything. Of the file systems, Windows creates all but HFS+,
/// and outside Windows FAT32 can't be written over a volume at all.
#[hegel::test(test_cases = 200)]
fn paths_and_file_systems_windows_cant_format_are_refused_first(tc: TestCase) {
    let path = tc.draw(hegel::one_of!(
        gs::text().max_size(12),
        gs::text().alphabet("E:\\/ab?").max_size(6),
        gs::sampled_from(vec!["/Volumes/DNA", "E:\\Music", "\\\\server\\share"]).map(String::from),
    ));
    tc.assume(drive_letter(Path::new(&path)).is_none());
    match by_mount(Path::new(&path)) {
        Err(Error::Invalid(m)) => assert!(m.starts_with(&Path::new(&path).display().to_string()), "{m}"),
        other => panic!("{path:?}: {:?}", other.map(|d| d.id)),
    }
    let fs =
        tc.draw(gs::sampled_from(vec![Filesystem::Fat32, Filesystem::Exfat, Filesystem::Hfsplus]).print_as_debug());
    assert_eq!(supports(fs).is_err(), fs == Filesystem::Hfsplus);
    if let Err(e) = supports(fs) {
        assert_eq!(e.code(), "invalid");
    }
    #[cfg(not(windows))]
    {
        let layout = fat32::Layout::new(64 << 30, 512, 32 << 10, 2048).unwrap();
        let letter = tc.draw(gs::sampled_from(('A'..='Z').collect::<Vec<char>>()));
        let e = write_fat32(letter, &layout, "REKORDBOX", &Ctx::silent()).unwrap_err();
        assert_eq!(e.code(), "invalid", "{e}");
    }
}
