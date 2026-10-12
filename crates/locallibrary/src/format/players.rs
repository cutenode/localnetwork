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
mod tests {
    use super::*;

    #[test]
    fn verdicts() {
        let mbr = Some(Scheme::Mbr);
        assert_eq!(assess(mbr, Some(FsKind::Fat32), 1 << 40).verdict, Verdict::AllPlayers);
        let exfat = assess(mbr, Some(FsKind::Exfat), 1 << 40);
        assert_eq!(exfat.verdict, Verdict::SomePlayers);
        assert!(exfat.unsupported.iter().any(|p| p == "CDJ-2000NXS2"));
        assert_eq!(assess(Some(Scheme::Gpt), Some(FsKind::Fat32), 1 << 30).verdict, Verdict::NoPlayers);
        assert_eq!(assess(mbr, Some(FsKind::Ntfs), 1 << 30).verdict, Verdict::NoPlayers);
        assert_eq!(assess(None, Some(FsKind::Fat32), 1 << 30).verdict, Verdict::Unknown);
    }

    #[test]
    fn players_read_the_first_partition() {
        let mut d = crate::format::tests::sample_disk();
        let mut second = d.volumes[0].clone();
        second.filesystem = Some(FsKind::Ntfs);
        d.volumes.push(second);
        let c = assess_disk(&d);
        assert_eq!(c.verdict, Verdict::AllPlayers);
        assert!(c.notes.iter().any(|n| n.contains("first partition")));
        d.volumes.reverse();
        assert_eq!(assess_disk(&d).verdict, Verdict::NoPlayers);
    }

    #[test]
    fn each_file_system_gets_its_verdict() {
        let mbr = Some(Scheme::Mbr);
        let fat16 = assess(mbr, Some(FsKind::Fat16), 1 << 30);
        assert_eq!(fat16.verdict, Verdict::AllPlayers);
        assert!(fat16.notes.iter().any(|n| n.contains("need FAT32")), "{:?}", fat16.notes);
        let hfs = assess(mbr, Some(FsKind::Hfsplus), 1 << 30);
        assert_eq!(hfs.verdict, Verdict::AllPlayers);
        assert!(hfs.notes.iter().any(|n| n.contains("rekordbox for Windows")), "{:?}", hfs.notes);
        for fs in [None, Some(FsKind::Fat12), Some(FsKind::Apfs), Some(FsKind::Other)] {
            assert_eq!(assess(mbr, fs, 1 << 30).verdict, Verdict::NoPlayers, "{fs:?}");
        }
    }

    /// The partition table only matters once the file system is one players read.
    #[test]
    fn partition_tables_and_size_matter_once_the_file_system_is_read() {
        for scheme in [Scheme::Apm, Scheme::Other] {
            let c = assess(Some(scheme), Some(FsKind::Fat32), 1 << 30);
            assert_eq!((c.verdict, c.summary.as_str()), (Verdict::NoPlayers, "players need an MBR partition table"));
        }
        let bare = assess(None, Some(FsKind::Exfat), 1 << 30);
        assert!(
            bare.summary.starts_with("exFAT: only CDJ-3000")
                && bare.summary.ends_with("without a partition table, which players aren't documented to read")
        );
        let big = assess(Some(Scheme::Mbr), Some(FsKind::Fat32), MBR_MAX + 1);
        assert_eq!(big.notes, ["players can't use more than 2 TiB"]);
    }

    #[test]
    fn disks_without_partitions_or_with_case_sensitive_hfs_are_doubted() {
        let mut d = crate::format::tests::sample_disk();
        d.volumes[0].filesystem = Some(FsKind::Hfsplus);
        d.volumes[0].filesystem_name = Some("Mac OS Extended (Case-sensitive)".into());
        let c = assess_disk(&d);
        assert_eq!(c.verdict, Verdict::Unknown);
        assert!(c.notes.iter().any(|n| n == "case-sensitive HFS+ may not be recognised"));
        d.volumes[0].filesystem_name = Some("Mac OS Extended".into());
        assert_eq!(assess_disk(&d).verdict, Verdict::AllPlayers);
        d.volumes.clear();
        let none = assess_disk(&d);
        assert_eq!((none.verdict, none.summary.as_str()), (Verdict::NoPlayers, "no partitions"));
    }

    // -----------------------------------------------------------------------------------------
    // Properties
    // -----------------------------------------------------------------------------------------

    use hegel::generators as gs;
    use hegel::{Generator, TestCase};

    const SCHEMES: [Option<Scheme>; 5] =
        [None, Some(Scheme::Mbr), Some(Scheme::Gpt), Some(Scheme::Apm), Some(Scheme::Other)];

    /// Every layout gets a verdict: no player reads a file system they don't support or anything
    /// but an MBR partition table; FAT and HFS+ on MBR work everywhere, exFAT on some players (and
    /// the rest are named); without a partition table it's unknown. Only a readable layout
    /// warns about size past 2 TiB.
    #[hegel::test(test_cases = 300)]
    fn every_layout_gets_its_verdict(tc: TestCase) {
        let scheme = tc.draw(gs::sampled_from(SCHEMES.to_vec()).print_as_debug());
        let all = crate::format::tests::FS_KINDS.map(Some);
        let fs = tc.draw(gs::sampled_from([&[None][..], &all[..]].concat()).print_as_debug());
        let size = tc.draw(hegel::one_of!(
            gs::integers::<u64>(),
            gs::integers::<u64>().min_value(MBR_MAX - 2).max_value(MBR_MAX + 2)
        ));
        let c = assess(scheme, fs, size);
        let readable = matches!(fs, Some(FsKind::Fat32 | FsKind::Fat16 | FsKind::Hfsplus | FsKind::Exfat));
        let want = match (readable, scheme) {
            (false, _) | (true, Some(Scheme::Gpt | Scheme::Apm | Scheme::Other)) => Verdict::NoPlayers,
            (true, None) => Verdict::Unknown,
            (true, Some(Scheme::Mbr)) if fs == Some(FsKind::Exfat) => Verdict::SomePlayers,
            (true, Some(Scheme::Mbr)) => Verdict::AllPlayers,
        };
        assert_eq!(c.verdict, want, "{scheme:?} {fs:?}");
        assert!(!c.summary.is_empty());
        let exfat_named = c.unsupported == NO_EXFAT && c.summary.contains(EXFAT[0]);
        assert_eq!(exfat_named, fs == Some(FsKind::Exfat) && scheme.is_none_or(|s| s == Scheme::Mbr));
        if c.verdict != Verdict::SomePlayers {
            assert!(c.unsupported.is_empty() || exfat_named);
        }
        let size_note = c.notes.iter().any(|n| n.contains("2 TiB"));
        assert_eq!(size_note, readable && size > MBR_MAX);
        assert_eq!(c.summary.contains("without a partition table"), readable && scheme.is_none());
    }

    /// Players read a disk's first partition: the disk gets that partition's verdict, doubted for
    /// case-sensitive HFS+, with a note naming it when there are others; a disk without
    /// partitions works on none.
    #[hegel::test(test_cases = 300)]
    fn disks_are_judged_by_their_first_partition(tc: TestCase) {
        let disk = crate::format::tests::any_disk(&tc);
        let c = assess_disk(&disk);
        let Some(first) = disk.volumes.first() else {
            assert_eq!((c.verdict, c.summary.as_str()), (Verdict::NoPlayers, "no partitions"));
            return;
        };
        let alone = assess(disk.scheme, first.filesystem, first.size);
        let case_sensitive = first.filesystem == Some(FsKind::Hfsplus)
            && first.filesystem_name.as_deref().is_some_and(|n| n.to_lowercase().contains("case-sensitive"));
        let verdict = if case_sensitive { Verdict::Unknown } else { alone.verdict };
        assert_eq!(c.verdict, verdict);
        assert_eq!(c.summary, alone.summary);
        assert!(c.notes.starts_with(&alone.notes));
        let named = c.notes.iter().any(|n| n.starts_with("players read the first partition"));
        assert_eq!(named, disk.volumes.len() > 1 && verdict != Verdict::NoPlayers);
    }
}
