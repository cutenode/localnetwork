use std::path::{Path, PathBuf};

use loclib_equipment::FsKind;
use serde::{Deserialize, Serialize};

/// Where the partition starts: 1 MiB, the alignment every modern tool uses.
pub const PARTITION_START: u64 = 1 << 20;

/// A disk's partition table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    /// Master Boot Record ("FDisk", "dos").
    Mbr,
    /// GUID partition table.
    Gpt,
    /// Apple partition map.
    Apm,
    /// A partition table none of the above.
    Other,
}

/// One partition (or, for disks without a partition table, the disk itself).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Volume {
    /// OS identifier: `disk4s1`, `/dev/sdb1`, `2:1` (disk:partition on Windows).
    pub id: String,
    /// Bytes.
    pub size: u64,
    /// The file system on it, when there is one the OS recognises.
    pub filesystem: Option<FsKind>,
    /// The file system as the OS names it ("MS-DOS FAT32", "vfat", "exFAT"...).
    pub filesystem_name: Option<String>,
    /// The volume label.
    pub label: Option<String>,
    /// Where it's mounted.
    pub mount: Option<PathBuf>,
    /// Bytes per cluster, where the OS says.
    pub cluster_size: Option<u64>,
    /// The volume's or partition's UUID (a FAT serial number, a GPT GUID...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
}

/// A whole disk, as the OS's disk tools describe it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Disk {
    /// OS identifier: `disk4` (macOS), `/dev/sdb` (Linux), `PhysicalDrive2` (Windows).
    pub id: String,
    /// Whole-disk device path.
    pub device: String,
    /// Vendor and product, as the OS names them.
    pub model: String,
    /// The serial number, where the OS can tell (on macOS, the USB device's).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    /// Bytes.
    pub size: u64,
    /// Bytes per logical sector.
    pub sector_size: u32,
    /// "USB", "SD"...
    pub bus: Option<String>,
    /// External, removable or hot-pluggable.
    pub removable: bool,
    /// Built into the computer.
    pub internal: bool,
    /// Holds the running operating system.
    pub system: bool,
    /// A disk image, loop device or virtual disk.
    pub virtual_disk: bool,
    /// Write-protected (an SD card's lock switch, say).
    pub read_only: bool,
    /// `None` when the disk has no partition table.
    pub scheme: Option<Scheme>,
    /// Its partitions, and on macOS the APFS volumes in them.
    pub volumes: Vec<Volume>,
}

impl Disk {
    /// Fingerprint of what is on the disk, for confirming that the disk being formatted is the
    /// one whose plan was shown. Mount points aren't included: they come and go.
    pub fn identity(&self) -> String {
        self.fingerprint(true)
    }

    /// [`Self::identity`], or with `volumes` false, of the disk alone (what a capacity test,
    /// which overwrites the partition table, leaves as it was).
    pub fn fingerprint(&self, volumes: bool) -> String {
        let mut h = blake3::Hasher::new();
        for s in [&self.id, &self.model, self.serial.as_deref().unwrap_or("")] {
            h.update(s.as_bytes());
            h.update(&[0]);
        }
        h.update(&self.size.to_le_bytes());
        for v in self.volumes.iter().filter(|_| volumes) {
            for s in [&v.id, v.uuid.as_deref().unwrap_or(""), v.label.as_deref().unwrap_or("")] {
                h.update(s.as_bytes());
                h.update(&[0]);
            }
            h.update(&v.size.to_le_bytes());
            h.update(format!("{:?}", v.filesystem).as_bytes());
        }
        h.finalize().to_hex()[..16].to_string()
    }

    /// Why this disk must not be formatted, if it mustn't: it holds the running system or one of
    /// `protected`, or it's internal, virtual or write-protected.
    pub fn problem(&self, protected: &[PathBuf]) -> Option<String> {
        if self.system {
            return Some("it holds the running operating system".into());
        }
        // The disk image a test attached is let through, and nothing else: it's virtual, and on
        // Linux (a loop device) not removable either.
        let test_image = self.virtual_disk && is_test_image(&self.id);
        if (self.internal || !self.removable) && !test_image {
            return Some("it's an internal disk (only external and removable drives can be formatted)".into());
        }
        if self.virtual_disk && !test_image {
            return Some("it's a disk image or virtual disk".into());
        }
        if self.read_only {
            return Some("it's write-protected".into());
        }
        if self.volumes.iter().any(|v| v.mount.as_deref() == Some(Path::new("/"))) {
            return Some("it holds the running operating system".into());
        }
        self.holding(protected).map(|(_, p)| format!("it holds {}", p.display()))
    }

    /// The first of `paths` on one of this disk's mounted volumes, and that volume's mount point.
    pub fn holding<'a>(&'a self, paths: &'a [PathBuf]) -> Option<(&'a Path, &'a PathBuf)> {
        let mounts = self.volumes.iter().filter_map(|v| v.mount.as_deref());
        mounts.flat_map(|m| paths.iter().filter(move |p| p.starts_with(m)).map(move |p| (m, p))).next()
    }

    /// Bytes available for the partition: the disk minus the 1 MiB lead-in.
    pub fn partition_size(&self) -> u64 {
        let usable = self.size.saturating_sub(PARTITION_START);
        usable - usable % u64::from(self.sector_size.max(512))
    }
}

/// Whether `id` names the disk image this test thread attached ([`crate::test_util::TEST_IMAGE`]).
/// Never, outside tests.
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn is_test_image(id: &str) -> bool {
    crate::test_util::TEST_IMAGE.with_borrow(|t| t.as_deref() == Some(id))
}

#[cfg(not(any(test, feature = "test-util")))]
pub(crate) fn is_test_image(_: &str) -> bool {
    false
}
