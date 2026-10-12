//! Linux: `lsblk` to find disks, `sfdisk` and `mkfs.*` to format them (as root).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{Cmd, Disk, Filesystem, FsKind, PARTITION_START, Scheme, Target, Volume, json};
use crate::progress::{Ctx, Phase};
use crate::{Error, Result};

/// Running as root, there's no desktop session to mount the new volume for.
pub const MOUNTS_AFTER_FORMAT: bool = false;

const COLUMNS: &str =
    "NAME,PATH,TYPE,SIZE,RM,HOTPLUG,RO,TRAN,VENDOR,MODEL,SERIAL,FSTYPE,LABEL,UUID,MOUNTPOINT,PTTYPE,LOG-SEC";

fn lsblk(device: Option<&str>) -> Result<Value> {
    // FSVER (FAT12/16/32) needs util-linux 2.35+; fall back without it.
    let with_ver = format!("{COLUMNS},FSVER");
    let run = |cols: &str| {
        let mut args = vec!["--json", "--bytes", "--output", cols];
        args.extend(device);
        Cmd::new("lsblk", &args).run()
    };
    let text = run(&with_ver).or_else(|_| run(COLUMNS))?;
    serde_json::from_str(&text).map_err(|e| Error::parse("lsblk output", e))
}

pub fn list() -> Result<Vec<Disk>> {
    let all = lsblk(None)?;
    Ok(json::array(all.get("blockdevices")).into_iter().filter_map(parse_disk).collect())
}

pub fn by_id(spec: &str) -> Result<Option<Disk>> {
    if !spec.starts_with("/dev/") {
        return Ok(None);
    }
    // A partition names its disk.
    Ok(disk_holding(&lsblk(None)?, &|n| json::str(n, "path").as_deref() == Some(spec)))
}

pub fn by_mount(path: &Path) -> Result<Disk> {
    let want = path.to_string_lossy();
    disk_holding(&lsblk(None)?, &|n| json::str(n, "mountpoint").as_deref() == Some(&*want))
        .ok_or_else(|| Error::invalid(format!("{} is not the root of a mounted volume", path.display())))
}

/// The disk in `lsblk` output `all` that is, or holds, a device `pred` picks.
fn disk_holding(all: &Value, pred: &dyn Fn(&Value) -> bool) -> Option<Disk> {
    json::array(all.get("blockdevices")).into_iter().find(|d| contains(d, pred)).and_then(parse_disk)
}

fn contains(node: &Value, pred: &dyn Fn(&Value) -> bool) -> bool {
    pred(node) || json::array(node.get("children")).into_iter().any(|c| contains(c, pred))
}

/// Mount points under `node`, deepest first (the order to unmount them in).
fn mounts(node: &Value, out: &mut Vec<(String, String)>) {
    for c in json::array(node.get("children")) {
        mounts(c, out);
    }
    if let (Some(path), Some(m)) = (json::str(node, "path"), json::str(node, "mountpoint")) {
        out.push((path, m));
    }
}

/// Mount points that mean "this disk runs the system".
const SYSTEM_MOUNTS: [&str; 7] = ["/", "/boot", "/boot/efi", "/efi", "/usr", "/var", "[SWAP]"];

pub(super) fn parse_disk(d: &Value) -> Option<Disk> {
    if json::str(d, "type").as_deref() != Some("disk") {
        return None;
    }
    let name = json::str(d, "name").unwrap_or_default();
    let path = json::str(d, "path").unwrap_or_else(|| format!("/dev/{name}"));
    let mut all = Vec::new();
    mounts(d, &mut all);
    let system = all.iter().any(|(_, m)| SYSTEM_MOUNTS.contains(&m.as_str()));
    let tran = json::str(d, "tran");
    let removable = json::bool(d, "rm")
        || json::bool(d, "hotplug")
        || matches!(tran.as_deref(), Some("usb" | "mmc"))
        || name.starts_with("mmcblk");
    let parts = json::array(d.get("children"));
    let mut volumes: Vec<Volume> =
        parts.iter().filter(|p| json::str(p, "type").as_deref() == Some("part")).map(|p| volume(p)).collect();
    if parts.is_empty() && json::str(d, "fstype").is_some() {
        volumes.push(volume(d));
    }
    let model = [json::str(d, "vendor"), json::str(d, "model")].into_iter().flatten().collect::<Vec<_>>().join(" ");
    Some(Disk {
        id: path.clone(),
        device: path,
        model,
        serial: json::str(d, "serial"),
        size: json::u64(d, "size").unwrap_or(0),
        sector_size: json::u64(d, "log-sec").unwrap_or(512) as u32,
        bus: tran.map(|t| t.to_uppercase()),
        removable,
        internal: !removable,
        system,
        virtual_disk: ["loop", "zram", "ram", "nbd", "dm-", "md"].iter().any(|p| name.starts_with(p)),
        read_only: json::bool(d, "ro"),
        scheme: match json::str(d, "pttype").as_deref() {
            Some("dos") => Some(Scheme::Mbr),
            Some("gpt") => Some(Scheme::Gpt),
            Some("mac") => Some(Scheme::Apm),
            Some(_) => Some(Scheme::Other),
            None => None,
        },
        volumes,
    })
}

fn volume(p: &Value) -> Volume {
    let fstype = json::str(p, "fstype");
    let filesystem = fstype.as_deref().map(|t| match t {
        "vfat" => match json::str(p, "fsver").as_deref() {
            Some("FAT32") => FsKind::Fat32,
            Some("FAT16") => FsKind::Fat16,
            Some("FAT12") => FsKind::Fat12,
            // Without FSVER: assume FAT32, by far the most common on drives this size.
            _ => FsKind::Fat32,
        },
        "exfat" => FsKind::Exfat,
        "hfsplus" => FsKind::Hfsplus,
        "apfs" => FsKind::Apfs,
        "ntfs" | "ntfs3" => FsKind::Ntfs,
        _ => FsKind::Other,
    });
    Volume {
        id: json::str(p, "path").unwrap_or_default(),
        size: json::u64(p, "size").unwrap_or(0),
        filesystem,
        filesystem_name: fstype,
        label: json::str(p, "label"),
        mount: json::str(p, "mountpoint").map(PathBuf::from),
        cluster_size: None,
        uuid: json::str(p, "uuid"),
    }
}

pub fn supports(_: Filesystem) -> Result<()> {
    Ok(())
}

/// `/dev/sdb` -> `/dev/sdb1`, `/dev/mmcblk0` -> `/dev/mmcblk0p1`.
fn first_partition(disk: &str) -> String {
    if disk.ends_with(|c: char| c.is_ascii_digit()) { format!("{disk}p1") } else { format!("{disk}1") }
}

/// Unmount, partition, and make the file system.
fn commands(disk: &Disk, t: &Target, mounted: &[(String, String)]) -> Vec<Cmd> {
    let mut out: Vec<Cmd> =
        mounted.iter().filter(|(_, m)| m != "[SWAP]").map(|(_, m)| Cmd::new("umount", &[m.as_str()])).collect();
    let start = PARTITION_START / u64::from(disk.sector_size);
    let kind = match t.filesystem {
        Filesystem::Fat32 => "c", // W95 FAT32 (LBA)
        Filesystem::Exfat => "7", // HPFS/NTFS/exFAT
        Filesystem::Hfsplus => "af",
    };
    out.push(
        Cmd::new("sfdisk", &["--wipe", "always", "--wipe-partitions", "always", &disk.device])
            .input(format!("label: dos\nstart={start}, type={kind}\n")),
    );
    let part = first_partition(&disk.device);
    let cluster = t.cluster_size.to_string();
    let spc = (t.cluster_size / disk.sector_size).to_string();
    out.push(match t.filesystem {
        Filesystem::Fat32 => Cmd::new("mkfs.vfat", &["-F", "32", "-s", &spc, "-n", &t.label, &part]),
        Filesystem::Exfat => Cmd::new("mkfs.exfat", &["-c", &cluster, "-L", &t.label, &part]),
        Filesystem::Hfsplus => Cmd::new("mkfs.hfsplus", &["-v", &t.label, &part]),
    });
    out
}

fn mounted(disk: &Disk) -> Result<Vec<(String, String)>> {
    let tree = lsblk(Some(&disk.device))?;
    let mut out = Vec::new();
    for d in json::array(tree.get("blockdevices")) {
        mounts(d, &mut out);
    }
    Ok(out)
}

pub fn steps(disk: &Disk, t: &Target) -> Result<Vec<String>> {
    Ok(commands(disk, t, &mounted(disk)?).iter().map(Cmd::display).collect())
}

pub fn format(disk: &Disk, t: &Target, ctx: &Ctx<'_>) -> Result<Option<PathBuf>> {
    super::require_write_access(&disk.device, "run it as an administrator")?;
    let cmds = commands(disk, t, &mounted(disk)?);
    ctx.phase_start(Phase::Format, cmds.len() as u64, 0);
    let part = first_partition(&disk.device);
    for c in cmds {
        if c.program.starts_with("mkfs") {
            // The kernel creates the partition's device node asynchronously.
            let _ = Cmd::new("udevadm", &["settle"]).run();
            let deadline = Instant::now() + Duration::from_secs(10);
            while !Path::new(&part).exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        ctx.emit(crate::progress::Event::Item { phase: Phase::Format, path: c.display(), bytes: 0 });
        c.run()?;
    }
    let _ = Cmd::new("sync", &[]).run();
    ctx.phase_end(Phase::Format);
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn usb_drive(old_lsblk: bool) -> Value {
        let (t, f) = if old_lsblk { (json!("1"), json!("0")) } else { (json!(true), json!(false)) };
        json!({
            "name": "sdb", "path": "/dev/sdb", "type": "disk", "size": 64160400896u64, "rm": t, "hotplug": t, "ro": f,
            "tran": "usb", "vendor": "Samsung ", "model": "Flash Drive FIT ", "serial": "0123", "fstype": null,
            "label": null, "uuid": null, "mountpoint": null, "pttype": "dos", "log-sec": 512,
            "children": [{
                "name": "sdb1", "path": "/dev/sdb1", "type": "part", "size": 64159352320u64, "rm": t, "hotplug": t,
                "ro": f, "fstype": "vfat", "fsver": "FAT32", "label": "KEYCHAIN", "uuid": "1234-ABCD",
                "mountpoint": "/media/dj/KEYCHAIN", "pttype": "dos", "log-sec": 512
            }]
        })
    }

    #[test]
    fn parses_lsblk_old_and_new() {
        for old in [false, true] {
            let d = parse_disk(&usb_drive(old)).unwrap();
            assert_eq!(d.device, "/dev/sdb");
            assert_eq!(d.model, "Samsung Flash Drive FIT");
            assert!(d.removable && !d.internal && !d.system && !d.read_only && !d.virtual_disk);
            assert_eq!(d.scheme, Some(Scheme::Mbr));
            assert_eq!(d.volumes[0].filesystem, Some(FsKind::Fat32));
            assert_eq!(d.volumes[0].mount.as_deref(), Some(Path::new("/media/dj/KEYCHAIN")));
        }
    }

    #[test]
    fn system_disks_are_flagged() {
        let mut v = usb_drive(false);
        v["rm"] = json!(false);
        v["hotplug"] = json!(false);
        v["tran"] = json!("nvme");
        v["children"][0]["mountpoint"] = json!("/");
        let d = parse_disk(&v).unwrap();
        assert!(d.system && d.internal);
    }

    #[test]
    fn format_commands() {
        let d = parse_disk(&usb_drive(false)).unwrap();
        let mounted = vec![("/dev/sdb1".to_string(), "/media/dj/KEYCHAIN".to_string())];
        let t = Target {
            filesystem: Filesystem::Fat32,
            label: "KEYCHAIN".into(),
            cluster_size: 32768,
            default_cluster: true,
        };
        let cmds = commands(&d, &t, &mounted);
        let shown: Vec<String> = cmds.iter().map(Cmd::display).collect();
        assert_eq!(
            shown,
            [
                "umount /media/dj/KEYCHAIN",
                "sfdisk --wipe always --wipe-partitions always /dev/sdb <<< (script)",
                "mkfs.vfat -F 32 -s 64 -n KEYCHAIN /dev/sdb1",
            ]
        );
        assert_eq!(cmds[1].input.as_deref(), Some("label: dos\nstart=2048, type=c\n"));
        let t =
            Target { filesystem: Filesystem::Exfat, label: "Club".into(), cluster_size: 131072, default_cluster: true };
        assert_eq!(commands(&d, &t, &[])[1].display(), "mkfs.exfat -c 131072 -L Club /dev/sdb1");
    }

    #[test]
    fn partition_names() {
        assert_eq!(first_partition("/dev/sdb"), "/dev/sdb1");
        assert_eq!(first_partition("/dev/mmcblk0"), "/dev/mmcblk0p1");
        assert_eq!(first_partition("/dev/nvme1n1"), "/dev/nvme1n1p1");
    }

    /// `lsblk --json --bytes --output <COLUMNS>,FSVER` on a laptop (util-linux 2.39), trimmed: a
    /// snap's loop device, a USB stick, a stick formatted without a partition table, an SD card,
    /// the DVD drive and the system disk.
    const LAPTOP: &str = r#"{"blockdevices": [
      {"name":"loop0", "path":"/dev/loop0", "type":"loop", "size":77881344, "rm":false, "hotplug":false, "ro":true, "tran":null, "vendor":null, "model":null, "serial":null, "fstype":"squashfs", "label":null, "uuid":null, "mountpoint":"/snap/core22/1380", "pttype":null, "log-sec":512, "fsver":"4.0"},
      {"name":"sda", "path":"/dev/sda", "type":"disk", "size":61530439680, "rm":true, "hotplug":true, "ro":false, "tran":"usb", "vendor":"SanDisk ", "model":"Ultra Fit       ", "serial":"4C530001230617114391", "fstype":null, "label":null, "uuid":null, "mountpoint":null, "pttype":"dos", "log-sec":512, "fsver":null,
        "children": [
          {"name":"sda1", "path":"/dev/sda1", "type":"part", "size":61529391104, "rm":true, "hotplug":true, "ro":false, "tran":null, "vendor":null, "model":null, "serial":null, "fstype":"exfat", "label":"REKORDBOX", "uuid":"64F6-2F12", "mountpoint":"/media/sam/REKORDBOX", "pttype":"dos", "log-sec":512, "fsver":"1.0"}
        ]},
      {"name":"sdb", "path":"/dev/sdb", "type":"disk", "size":8053063680, "rm":true, "hotplug":true, "ro":false, "tran":"usb", "vendor":"Generic", "model":"Flash Disk", "serial":"FBB1A2C3", "fstype":"vfat", "label":"STICK", "uuid":"0E4A-77C1", "mountpoint":"/media/sam/STICK", "pttype":null, "log-sec":512, "fsver":"FAT32"},
      {"name":"mmcblk0", "path":"/dev/mmcblk0", "type":"disk", "size":31914983424, "rm":false, "hotplug":false, "ro":false, "tran":null, "vendor":null, "model":null, "serial":"0x1234abcd", "fstype":null, "label":null, "uuid":null, "mountpoint":null, "pttype":"dos", "log-sec":512, "fsver":null,
        "children": [
          {"name":"mmcblk0p1", "path":"/dev/mmcblk0p1", "type":"part", "size":31913934848, "rm":false, "hotplug":false, "ro":false, "tran":null, "vendor":null, "model":null, "serial":null, "fstype":"vfat", "label":"CARD", "uuid":"1A2B-3C4D", "mountpoint":"/media/sam/CARD", "pttype":"dos", "log-sec":512, "fsver":"FAT16"}
        ]},
      {"name":"sr0", "path":"/dev/sr0", "type":"rom", "size":1073741312, "rm":true, "hotplug":true, "ro":false, "tran":"sata", "vendor":"HL-DT-ST", "model":"DVDRAM GU90N", "serial":"KZPH1234", "fstype":null, "label":null, "uuid":null, "mountpoint":null, "pttype":null, "log-sec":2048, "fsver":null},
      {"name":"nvme0n1", "path":"/dev/nvme0n1", "type":"disk", "size":512110190592, "rm":false, "hotplug":false, "ro":false, "tran":"nvme", "vendor":null, "model":"Samsung SSD 980 PRO 512GB", "serial":"S5GXNX0T123456", "fstype":null, "label":null, "uuid":null, "mountpoint":null, "pttype":"gpt", "log-sec":512, "fsver":null,
        "children": [
          {"name":"nvme0n1p1", "path":"/dev/nvme0n1p1", "type":"part", "size":536870912, "rm":false, "hotplug":false, "ro":false, "tran":"nvme", "vendor":null, "model":null, "serial":null, "fstype":"vfat", "label":null, "uuid":"9C1E-0B2A", "mountpoint":"/boot/efi", "pttype":"gpt", "log-sec":512, "fsver":"FAT32"},
          {"name":"nvme0n1p2", "path":"/dev/nvme0n1p2", "type":"part", "size":494927527936, "rm":false, "hotplug":false, "ro":false, "tran":"nvme", "vendor":null, "model":null, "serial":null, "fstype":"ext4", "label":null, "uuid":"3f5c0a8e-2b1d-4c6e-9f7a-8b9c0d1e2f3a", "mountpoint":"/", "pttype":"gpt", "log-sec":512, "fsver":"1.0"},
          {"name":"nvme0n1p3", "path":"/dev/nvme0n1p3", "type":"part", "size":16645095424, "rm":false, "hotplug":false, "ro":false, "tran":"nvme", "vendor":null, "model":null, "serial":null, "fstype":"swap", "label":null, "uuid":"b7d2e1f0-1a2b-4c3d-8e9f-0a1b2c3d4e5f", "mountpoint":"[SWAP]", "pttype":"gpt", "log-sec":512, "fsver":"1"}
        ]}
    ]}"#;

    fn laptop() -> Value {
        serde_json::from_str(LAPTOP).unwrap()
    }

    #[test]
    fn a_laptops_disks_are_read_from_lsblk() {
        let all = laptop();
        let disks: Vec<Disk> = json::array(all.get("blockdevices")).into_iter().filter_map(parse_disk).collect();
        let ids: Vec<&str> = disks.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["/dev/sda", "/dev/sdb", "/dev/mmcblk0", "/dev/nvme0n1"], "no loop or DVD drive");

        let (stick, bare, card, system) = (&disks[0], &disks[1], &disks[2], &disks[3]);
        assert_eq!((stick.model.as_str(), stick.bus.as_deref()), ("SanDisk Ultra Fit", Some("USB")));
        assert_eq!(stick.volumes[0].filesystem, Some(FsKind::Exfat));
        assert_eq!(stick.problem(&[]), None);

        // A file system on the whole disk is its one volume.
        assert_eq!((bare.scheme, bare.volumes.len()), (None, 1));
        assert_eq!((bare.volumes[0].id.as_str(), bare.volumes[0].filesystem), ("/dev/sdb", Some(FsKind::Fat32)));

        // A card in a built-in reader says neither removable nor hot-pluggable: its name does.
        assert!(card.removable && !card.internal);
        assert_eq!(card.volumes[0].filesystem, Some(FsKind::Fat16));

        assert!(system.system && system.internal && !system.removable);
        assert_eq!(system.scheme, Some(Scheme::Gpt));
        let kinds: Vec<Option<FsKind>> = system.volumes.iter().map(|v| v.filesystem).collect();
        assert_eq!(kinds, [Some(FsKind::Fat32), Some(FsKind::Other), Some(FsKind::Other)]);
        assert_eq!(system.problem(&[]).as_deref(), Some("it holds the running operating system"));
    }

    #[test]
    fn a_partition_or_a_mount_point_finds_its_disk() {
        let all = laptop();
        let by_path = |p: &str| disk_holding(&all, &|n| json::str(n, "path").as_deref() == Some(p)).map(|d| d.id);
        assert_eq!(by_path("/dev/sda1").as_deref(), Some("/dev/sda"));
        assert_eq!(by_path("/dev/sda").as_deref(), Some("/dev/sda"));
        assert_eq!(by_path("/dev/loop0"), None, "not a disk");
        assert_eq!(by_path("/dev/sdz"), None);
        let at = |m: &str| disk_holding(&all, &|n| json::str(n, "mountpoint").as_deref() == Some(m)).map(|d| d.id);
        assert_eq!(at("/media/sam/CARD").as_deref(), Some("/dev/mmcblk0"));
        assert_eq!(at("/media/sam"), None);
    }

    /// Partitions are unmounted deepest first, swap is left alone, and the partition starts at
    /// 1 MiB whatever the sector size.
    #[test]
    fn unmounts_then_partitions_a_4k_disk() {
        let all = laptop();
        let system = json::array(all.get("blockdevices"))[5];
        let mut mounted = Vec::new();
        mounts(system, &mut mounted);
        assert_eq!(mounted[0], ("/dev/nvme0n1p1".to_string(), "/boot/efi".to_string()));
        assert_eq!(mounted.len(), 3);
        let disk = Disk { sector_size: 4096, ..parse_disk(system).unwrap() };
        let t =
            Target { filesystem: Filesystem::Hfsplus, label: "Club".into(), cluster_size: 4096, default_cluster: true };
        let cmds = commands(&disk, &t, &mounted);
        let shown: Vec<String> = cmds.iter().map(Cmd::display).collect();
        assert_eq!(
            shown,
            [
                "umount /boot/efi",
                "umount /",
                "sfdisk --wipe always --wipe-partitions always /dev/nvme0n1 <<< (script)",
                "mkfs.hfsplus -v Club /dev/nvme0n1p1",
            ]
        );
        assert_eq!(cmds[2].input.as_deref(), Some("label: dos\nstart=256, type=af\n"));
    }

    #[test]
    fn file_systems_and_partition_tables_are_told_apart() {
        let fs = |fstype: &str, fsver: Option<&str>| volume(&json!({ "fstype": fstype, "fsver": fsver })).filesystem;
        assert_eq!(fs("vfat", Some("FAT12")), Some(FsKind::Fat12));
        assert_eq!(fs("vfat", None), Some(FsKind::Fat32), "lsblk before 2.35 has no FSVER");
        assert_eq!(fs("hfsplus", None), Some(FsKind::Hfsplus));
        assert_eq!(fs("apfs", None), Some(FsKind::Apfs));
        assert_eq!(fs("ntfs3", None), Some(FsKind::Ntfs));
        let mut d = usb_drive(false);
        for (pttype, scheme) in [("mac", Some(Scheme::Apm)), ("atari", Some(Scheme::Other))] {
            d["pttype"] = json!(pttype);
            assert_eq!(parse_disk(&d).unwrap().scheme, scheme, "{pttype}");
        }
    }

    /// Only device paths are asked of lsblk.
    #[test]
    fn other_names_are_not_disks() {
        for spec in ["Backup", "disk4", "sdb", "PhysicalDrive2"] {
            assert!(by_id(spec).unwrap().is_none(), "{spec}");
        }
    }

    // -----------------------------------------------------------------------------------------
    // Properties
    // -----------------------------------------------------------------------------------------

    use hegel::generators as gs;
    use hegel::{Generator, TestCase};

    /// A flag as lsblk writes it: a boolean, or "1"/"0" before util-linux 2.33.
    #[hegel::test_helper]
    fn flag(tc: &TestCase, b: bool) -> Value {
        if tc.draw(gs::booleans()) { json!(b) } else { json!(if b { "1" } else { "0" }) }
    }

    /// `fstype` and `fsver` as lsblk reports them, and what they mean.
    const FILESYSTEMS: [(&str, Option<&str>, FsKind); 11] = [
        ("vfat", Some("FAT32"), FsKind::Fat32),
        ("vfat", Some("FAT16"), FsKind::Fat16),
        ("vfat", Some("FAT12"), FsKind::Fat12),
        ("vfat", None, FsKind::Fat32),
        ("exfat", Some("1.0"), FsKind::Exfat),
        ("hfsplus", None, FsKind::Hfsplus),
        ("apfs", None, FsKind::Apfs),
        ("ntfs", None, FsKind::Ntfs),
        ("ntfs3", None, FsKind::Ntfs),
        ("ext4", Some("1.0"), FsKind::Other),
        ("swap", Some("1"), FsKind::Other),
    ];
    const MOUNTS: [&str; 7] = ["/media/dj/A", "/media/dj/B", "/mnt/usb", "/", "/boot/efi", "/var", "[SWAP]"];

    /// A partition (or something on one, like a LUKS mapping) and what reading it gives.
    #[hegel::test_helper]
    fn node(tc: &TestCase, path: &str, part: bool, nested: bool) -> (Value, Option<Volume>, Vec<String>) {
        let mut v = json!({ "name": path.trim_start_matches("/dev/"), "path": path,
            "type": if part { "part" } else { "crypt" }, "size": tc.draw(gs::integers::<u64>()) });
        let fs = if tc.draw(gs::booleans()) {
            Some(tc.draw(gs::sampled_from(FILESYSTEMS.to_vec()).print_as_debug()))
        } else {
            None
        };
        if let Some((t, ver, _)) = fs {
            v["fstype"] = json!(t);
            v["fsver"] = json!(ver);
        }
        for key in ["label", "uuid"] {
            if tc.draw(gs::booleans()) {
                v[key] = json!(format!("{key}-{}", tc.draw(gs::integers::<u8>())));
            }
        }
        let mut mounts = Vec::new();
        if tc.draw(gs::booleans()) {
            let m = tc.draw(gs::sampled_from(MOUNTS.to_vec()));
            v["mountpoint"] = json!(m);
            mounts.push(m.to_string());
        }
        if nested && tc.draw(gs::weighted_booleans(0.2)) {
            let (child, _, inner) = node(tc, &format!("/dev/mapper/{}", v["name"].as_str().unwrap()), false, false);
            v["children"] = json!([child]);
            mounts.splice(0..0, inner);
        }
        let volume = part.then(|| Volume {
            id: path.into(),
            size: v["size"].as_u64().unwrap(),
            filesystem: fs.map(|f| f.2),
            filesystem_name: fs.map(|f| f.0.to_string()),
            label: v.get("label").and_then(Value::as_str).map(String::from),
            mount: v.get("mountpoint").and_then(Value::as_str).map(PathBuf::from),
            cluster_size: None,
            uuid: v.get("uuid").and_then(Value::as_str).map(String::from),
        });
        (v, volume, mounts)
    }

    /// A block device as `lsblk --json` lists it, with the [`Disk`] it should read as (`None` for
    /// what isn't a disk) and its mount points, deepest first.
    #[hegel::test_helper]
    fn block_device(tc: &TestCase, name: &str) -> (Value, Option<Disk>, Vec<String>) {
        let kind = tc.draw(gs::sampled_from(vec!["disk", "disk", "disk", "loop", "rom"]));
        let (rm, hotplug, ro) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()), tc.draw(gs::booleans()));
        let tran = tc.draw(gs::sampled_from(vec![None, Some("usb"), Some("mmc"), Some("sata"), Some("nvme")]));
        let pttype = tc.draw(
            gs::sampled_from(vec![
                (None, None),
                (Some("dos"), Some(Scheme::Mbr)),
                (Some("gpt"), Some(Scheme::Gpt)),
                (Some("mac"), Some(Scheme::Apm)),
                (Some("atari"), Some(Scheme::Other)),
            ])
            .print_as_debug(),
        );
        let size = tc.draw(gs::integers::<u64>());
        let mut d = json!({ "name": name, "type": kind, "size": size, "rm": flag(tc, rm), "hotplug": flag(tc, hotplug),
            "ro": flag(tc, ro), "tran": tran, "pttype": pttype.0 });
        let path = format!("/dev/{name}");
        // Older lsblk has no PATH column.
        let has_path = tc.draw(gs::booleans());
        if has_path {
            d["path"] = json!(path);
        }
        let vendor = tc.draw(gs::sampled_from(vec![None, Some("SanDisk "), Some("Generic")]));
        let model = tc.draw(gs::sampled_from(vec![None, Some("Ultra Fit   "), Some("Flash Disk")]));
        d["vendor"] = json!(vendor);
        d["model"] = json!(model);
        let serial = tc.draw(gs::sampled_from(vec![None, Some("4C530001"), Some("  ")]));
        d["serial"] = json!(serial);
        let sector = tc.draw(gs::sampled_from(vec![None, Some(512u64), Some(4096)]));
        if let Some(s) = sector {
            d["log-sec"] = json!(s);
        }
        let mut volumes = Vec::new();
        let mut mounts = Vec::new();
        let n = tc.draw(gs::integers::<usize>().max_value(3));
        let mut children = Vec::new();
        for i in 1..=n {
            let sep = if name.ends_with(|c: char| c.is_ascii_digit()) { "p" } else { "" };
            let (child, volume, inner) = node(tc, &format!("{path}{sep}{i}"), true, true);
            children.push(child);
            volumes.extend(volume);
            mounts.extend(inner);
        }
        if n > 0 {
            d["children"] = json!(children);
        } else if tc.draw(gs::booleans()) {
            // A file system on the whole disk.
            let (whole, volume, inner) = node(tc, &path, true, false);
            for key in ["fstype", "fsver", "label", "uuid", "mountpoint"] {
                if let Some(v) = whole.get(key) {
                    d[key] = v.clone();
                }
            }
            // Without a PATH column (lsblk refuses COLUMNS then anyway) the volume has no id.
            let id = if has_path { path.clone() } else { String::new() };
            volumes.push(Volume { size, id, ..volume.unwrap() });
            // Mount points are only told apart by path.
            mounts.extend(inner.into_iter().filter(|_| has_path));
            if d.get("fstype").is_none() {
                volumes.clear();
            }
        }
        if kind != "disk" {
            return (d, None, mounts);
        }
        let removable = rm || hotplug || matches!(tran, Some("usb" | "mmc")) || name.starts_with("mmcblk");
        let model = [vendor, model].into_iter().flatten().map(str::trim).collect::<Vec<_>>().join(" ");
        let disk = Disk {
            id: path.clone(),
            device: path,
            model,
            serial: serial.map(str::trim).filter(|s| !s.is_empty()).map(String::from),
            size,
            sector_size: sector.unwrap_or(512) as u32,
            bus: tran.map(str::to_uppercase),
            removable,
            internal: !removable,
            system: mounts.iter().any(|m| ["/", "/boot/efi", "/var", "[SWAP]"].contains(&m.as_str())),
            virtual_disk: ["loop", "zram", "dm-", "md", "nbd"].iter().any(|p| name.starts_with(p)),
            read_only: ro,
            scheme: pttype.1,
            volumes,
        };
        (d, Some(disk), mounts)
    }

    /// `lsblk --json` output reads as the disks in it, field for field: partitions are the
    /// volumes (or the disk itself, when it holds a file system directly), a mount point of the
    /// system anywhere under a disk makes it a system disk, and what isn't a disk is left out.
    /// Every partition and mount point finds its disk.
    #[hegel::test(test_cases = 300)]
    fn lsblk_listings_read_back(tc: TestCase) {
        let names = gs::sampled_from(vec!["sdb", "sdc", "mmcblk0", "nvme0n1", "loop3", "dm-1", "md0", "zram0", "nbd0"]);
        let names: Vec<&str> = tc.draw(gs::vecs(names).unique(true).max_size(3));
        let devices: Vec<(Value, Option<Disk>, Vec<String>)> = names.iter().map(|n| block_device(&tc, n)).collect();
        let all = json!({ "blockdevices": devices.iter().map(|d| d.0.clone()).collect::<Vec<_>>() });
        let got: Vec<Disk> = json::array(all.get("blockdevices")).into_iter().filter_map(parse_disk).collect();
        let want: Vec<&Disk> = devices.iter().filter_map(|d| d.1.as_ref()).collect();
        assert_eq!(serde_json::to_value(&got).unwrap(), serde_json::to_value(&want).unwrap());

        for (v, disk, want_mounts) in &devices {
            let mut found = Vec::new();
            mounts(v, &mut found);
            assert_eq!(found.iter().map(|(_, m)| m.clone()).collect::<Vec<_>>(), *want_mounts, "deepest first");
            let Some(disk) = disk else { continue };
            let own = v.get("path").map(|_| disk.id.as_str());
            let parts = disk.volumes.iter().map(|v| v.id.as_str()).filter(|p| !p.is_empty() && Some(*p) != own);
            for p in parts.chain(own) {
                let at = disk_holding(&all, &|n| json::str(n, "path").as_deref() == Some(p));
                assert_eq!(at.map(|d| d.id), Some(disk.id.clone()), "{p}");
            }
        }
    }

    /// Formatting unmounts everything but swap, deepest first, partitions from 1 MiB whatever the
    /// sector size with the type code of the file system, and makes the file system on the first
    /// partition with the cluster size asked for.
    #[hegel::test(test_cases = 300)]
    fn format_commands_follow_the_target(tc: TestCase) {
        let name = tc.draw(gs::sampled_from(vec!["sdb", "mmcblk0", "nvme0n1"]));
        let (_, disk, mounted) = block_device(&tc, name);
        let Some(disk) = disk else { return };
        let mounted: Vec<(String, String)> = mounted.into_iter().map(|m| (String::new(), m)).collect();
        let filesystem =
            tc.draw(gs::sampled_from(vec![Filesystem::Fat32, Filesystem::Exfat, Filesystem::Hfsplus]).print_as_debug());
        let cluster_size = disk.sector_size << tc.draw(gs::integers::<u32>().max_value(6));
        let label = filesystem.label(&tc.draw(gs::text().max_size(16)));
        let t = Target { filesystem, label: label.clone(), cluster_size, default_cluster: tc.draw(gs::booleans()) };
        let cmds = commands(&disk, &t, &mounted);
        let unmounts: Vec<&str> = mounted.iter().map(|(_, m)| m.as_str()).filter(|m| *m != "[SWAP]").collect();
        let (umount, rest) = cmds.split_at(unmounts.len());
        assert!(umount.iter().zip(&unmounts).all(|(c, m)| c.program == "umount" && c.args == [*m]));
        let [sfdisk, mkfs] = rest else { panic!("{cmds:?}") };
        assert_eq!(sfdisk.args.last(), Some(&disk.device));
        let kind = ["c", "7", "af"][filesystem as usize];
        let start = (1 << 20) / disk.sector_size;
        assert_eq!(sfdisk.input.as_deref(), Some(format!("label: dos\nstart={start}, type={kind}\n").as_str()));
        let part = mkfs.args.last().unwrap();
        assert!(part.starts_with(&disk.device) && part.ends_with('1') && part.len() <= disk.device.len() + 2);
        assert!(mkfs.args.contains(&label));
        let size_arg = match filesystem {
            Filesystem::Fat32 => Some((cluster_size / disk.sector_size).to_string()),
            Filesystem::Exfat => Some(cluster_size.to_string()),
            Filesystem::Hfsplus => None,
        };
        assert!(size_arg.is_none_or(|s| mkfs.args.contains(&s)), "{mkfs:?}");
        assert!(by_id(&disk.device.replace("/dev/", "dev/")).unwrap().is_none());
    }
}
