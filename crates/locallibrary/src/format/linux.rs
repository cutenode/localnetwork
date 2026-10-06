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
    let all = lsblk(None)?;
    let disks: Vec<&Value> = json::array(all.get("blockdevices"));
    // A partition names its disk.
    let top = disks.into_iter().find(|d| contains(d, &|n| json::str(n, "path").as_deref() == Some(spec)));
    Ok(top.and_then(parse_disk))
}

pub fn by_mount(path: &Path) -> Result<Disk> {
    let all = lsblk(None)?;
    let want = path.to_string_lossy();
    json::array(all.get("blockdevices"))
        .into_iter()
        .find(|d| contains(d, &|n| json::str(n, "mountpoint").as_deref() == Some(&*want)))
        .and_then(parse_disk)
        .ok_or_else(|| Error::invalid(format!("{} is not the root of a mounted volume", path.display())))
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
}
