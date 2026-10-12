//! Which drive layouts rekordbox players read, from AlphaTheta's manuals and Help Center
//! (checked October 2026):
//!
//! * **File systems.** Every CDJ/XDJ/OPUS/OMNIS reads FAT16, FAT32 and HFS+. exFAT only on
//!   newer units (rekordbox 6.6.2+, some needing a firmware update; see [`EXFAT`]). NTFS is
//!   explicitly unsupported; APFS isn't listed anywhere. The DJM-V10 and DJM-A9 use USB for
//!   settings and firmware and need FAT32 on MBR.
//! * **Partitions.** "Does not support the GUID Partition Map" (CDJ-3000, CDJ-3000X,
//!   CDJ-2000NXS2, XDJ-XZ, XDJ-RX3, XDJ-AZ, OPUS-QUAD, OMNIS-DUO...): MBR it is, so the practical
//!   ceiling is MBR's 2 TiB. Players read the first partition unless another holds a rekordbox
//!   library (the CDJ-2000NXS2, CDJ-2000NXS, XDJ-XZ, OPUS-QUAD and OMNIS-DUO manuals put that one
//!   first); with just one partition there's no doubt which.
//! * **Firmware updates** need a FAT/FAT32 drive on almost every unit.
//! * **Cluster size.** No official guidance, and no measured effect on loading or browsing.
//!   It only changes how much space small files take (rekordbox writes three analysis files
//!   per track), so the defaults Windows and macOS pick are used; never above 32 KB on FAT32,
//!   which many FAT implementations can't handle.
//! * **HFS+** works, but rekordbox for Windows can't export to it, case-sensitive HFS+ may not be
//!   recognised, and users report slower mounting than FAT32.
//! * AlphaTheta's own SanDisk DJ drives (up to 1 TB) ship as FAT32.
//!
//! Hence the default: one MBR partition from 1 MiB, FAT32, Windows' default cluster size.

use schemars::JsonSchema;
use serde::Serialize;

use super::{Disk, Filesystem, FsKind, Scheme};

pub const DEFAULT_FILESYSTEM: Filesystem = Filesystem::Fat32;

/// The largest disk an MBR partition table can fully address (2 TiB with 512-byte sectors).
pub const MBR_MAX: u64 = 1 << 41;

/// Equipment that reads exFAT, with the firmware it needs (rekordbox FAQ "Which DJ equipment
/// supports exFAT", rekordbox 6.6.2 release notes).
pub const EXFAT: &[&str] = &[
    "CDJ-3000 (firmware 1.20+)",
    "CDJ-3000X",
    "CDJ-1500X",
    "OPUS-QUAD",
    "OMNIS-DUO",
    "XDJ-AZ",
    "XDJ-AN",
    "XDJ-XZ (firmware 1.23+)",
    "XDJ-RX3 (firmware 1.11+)",
];

/// Equipment that doesn't read exFAT.
pub const NO_EXFAT: &[&str] = &[
    "CDJ-2000NXS2",
    "CDJ-2000NXS",
    "CDJ-900NXS",
    "CDJ-2000",
    "CDJ-900",
    "XDJ-1000MK2",
    "XDJ-700",
    "XDJ-RX2",
    "XDJ-RR",
    "DJM-V10/DJM-A9 (settings, firmware)",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Every rekordbox player.
    AllPlayers,
    /// Newer players only.
    SomePlayers,
    /// Untested layouts players may or may not read.
    Unknown,
    NoPlayers,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Compatibility {
    pub verdict: Verdict,
    pub summary: String,
    /// Equipment that can't use it (when only some can).
    pub unsupported: Vec<String>,
    /// Caveats worth knowing.
    pub notes: Vec<String>,
}

impl Compatibility {
    fn new(verdict: Verdict, summary: impl Into<String>) -> Self {
        Compatibility { verdict, summary: summary.into(), unsupported: Vec::new(), notes: Vec::new() }
    }

    fn note(mut self, n: impl Into<String>) -> Self {
        self.notes.push(n.into());
        self
    }
}

/// How players get on with a volume of `fs` in a partition table of `scheme`.
pub fn assess(scheme: Option<Scheme>, fs: Option<FsKind>, size: u64) -> Compatibility {
    let mut c = match fs {
        None => return Compatibility::new(Verdict::NoPlayers, "no file system players can read"),
        Some(FsKind::Fat32) => Compatibility::new(Verdict::AllPlayers, "every player"),
        Some(FsKind::Fat16) => Compatibility::new(Verdict::AllPlayers, "every player")
            .note("DJM-V10/DJM-A9 settings and firmware need FAT32"),
        Some(FsKind::Exfat) => {
            let mut c = Compatibility::new(Verdict::SomePlayers, format!("exFAT: only {}", EXFAT.join(", ")));
            c.unsupported = NO_EXFAT.iter().map(|s| s.to_string()).collect();
            c.note("exFAT needs rekordbox 6.6.2 or later to export")
                .note("exFAT drives can't update player firmware (except on the CDJ-3000X)")
        }
        Some(FsKind::Hfsplus) => Compatibility::new(Verdict::AllPlayers, "every player (HFS+)")
            .note("rekordbox for Windows can't export to HFS+, and Windows can't read it")
            .note("players have been reported to mount HFS+ more slowly than FAT32")
            .note("DJM-V10/DJM-A9 settings and firmware need FAT32"),
        Some(FsKind::Fat12) => return Compatibility::new(Verdict::NoPlayers, "FAT12 isn't supported by players"),
        Some(FsKind::Ntfs) => return Compatibility::new(Verdict::NoPlayers, "NTFS isn't supported by any player"),
        Some(FsKind::Apfs) => return Compatibility::new(Verdict::NoPlayers, "APFS isn't supported by any player"),
        Some(FsKind::Other) => return Compatibility::new(Verdict::NoPlayers, "players can't read this file system"),
    };
    match scheme {
        Some(Scheme::Mbr) => {}
        Some(Scheme::Gpt) => {
            c = Compatibility::new(
                Verdict::NoPlayers,
                "GUID partition table: AlphaTheta doesn't support it (some newer players read it anyway); reformat with MBR",
            );
        }
        Some(Scheme::Apm | Scheme::Other) => {
            c = Compatibility::new(Verdict::NoPlayers, "players need an MBR partition table");
        }
        None => {
            c.verdict = Verdict::Unknown;
            c.summary =
                format!("{}, but without a partition table, which players aren't documented to read", c.summary);
        }
    }
    if size > MBR_MAX {
        c.notes.push("players can't use more than 2 TiB".into());
    }
    c
}

/// How players get on with a disk as it is: they read its first partition (unless another holds
/// a rekordbox library).
pub fn assess_disk(disk: &Disk) -> Compatibility {
    let Some(first) = disk.volumes.first() else {
        return Compatibility::new(Verdict::NoPlayers, "no partitions");
    };
    let mut c = assess(disk.scheme, first.filesystem, first.size);
    if first.filesystem == Some(FsKind::Hfsplus)
        && first.filesystem_name.as_deref().is_some_and(|n| n.to_lowercase().contains("case-sensitive"))
    {
        c.verdict = Verdict::Unknown;
        c.notes.push("case-sensitive HFS+ may not be recognised".into());
    }
    if disk.volumes.len() > 1 && c.verdict != Verdict::NoPlayers {
        c.notes.push(format!(
            "players read the first partition ({}) unless another holds a rekordbox library",
            first.label.as_deref().unwrap_or(&first.id)
        ));
    }
    c
}

#[cfg(test)]
mod tests;
