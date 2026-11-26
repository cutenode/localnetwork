use loclib_equipment::FsKind;
use serde::{Deserialize, Serialize};

use crate::{Error, Result, fat32};

/// File systems a drive can be formatted with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Filesystem {
    /// FAT32, which every player reads, written by [`fat32`] where the system's tools can't.
    Fat32,
    /// exFAT, for files of 4 GB and more.
    Exfat,
    /// HFS+ (Mac OS Extended), not journaled.
    Hfsplus,
}

impl Filesystem {
    /// The name people know it by: "FAT32", "exFAT", "HFS+".
    pub fn name(self) -> &'static str {
        match self {
            Filesystem::Fat32 => "FAT32",
            Filesystem::Exfat => "exFAT",
            Filesystem::Hfsplus => "HFS+",
        }
    }

    /// The cluster size used unless one is asked for: what Windows and macOS pick by default
    /// for a volume of `volume_bytes`.
    pub fn default_cluster_size(self, volume_bytes: u64) -> u32 {
        const MIB: u64 = 1 << 20;
        match self {
            Filesystem::Fat32 => fat32::default_cluster_size(volume_bytes),
            Filesystem::Exfat if volume_bytes <= 256 * MIB => 4096,
            Filesystem::Exfat if volume_bytes <= 32 * 1024 * MIB => 32 * 1024,
            Filesystem::Exfat => 128 * 1024,
            Filesystem::Hfsplus => 4096,
        }
    }

    /// Refuse a cluster size of `size` bytes this file system can't take on a disk with
    /// `sector`-byte sectors.
    pub fn check_cluster_size(self, size: u32, sector: u32) -> Result<()> {
        let ok = size.is_power_of_two()
            && size >= sector
            && match self {
                Filesystem::Fat32 => size <= fat32::MAX_CLUSTER_SIZE,
                Filesystem::Exfat => size <= 32 << 20,
                Filesystem::Hfsplus => size == 4096,
            };
        if ok {
            return Ok(());
        }
        Err(Error::invalid(match self {
            Filesystem::Fat32 => format!("FAT32 clusters must be a power of two from {sector} bytes to 32 KB"),
            Filesystem::Exfat => format!("exFAT clusters must be a power of two from {sector} bytes to 32 MB"),
            Filesystem::Hfsplus => "HFS+ is always formatted with 4 KB blocks".into(),
        }))
    }

    /// Make `name` a valid volume label for this file system.
    pub fn label(self, name: &str) -> String {
        const FALLBACK: &str = "REKORDBOX";
        match self {
            Filesystem::Fat32 => fat32::sanitize_label(name, FALLBACK),
            Filesystem::Exfat => {
                // The label holds 11 UTF-16 units: a character outside the BMP takes two.
                let mut units = 0;
                let s: String = name
                    .chars()
                    .map(|c| if c.is_control() || "\"*/:<>?\\|".contains(c) { '_' } else { c })
                    .take_while(|c| {
                        units += c.len_utf16();
                        units <= 11
                    })
                    .collect();
                let s = s.trim();
                if s.is_empty() { FALLBACK.into() } else { s.into() }
            }
            Filesystem::Hfsplus => {
                let s: String =
                    name.chars().map(|c| if c == ':' || c.is_control() { '_' } else { c }).take(27).collect();
                let s = s.trim();
                if s.is_empty() { FALLBACK.into() } else { s.into() }
            }
        }
    }
}

impl From<Filesystem> for FsKind {
    fn from(f: Filesystem) -> Self {
        match f {
            Filesystem::Fat32 => FsKind::Fat32,
            Filesystem::Exfat => FsKind::Exfat,
            Filesystem::Hfsplus => FsKind::Hfsplus,
        }
    }
}
