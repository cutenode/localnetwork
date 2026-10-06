//! A FAT32 formatter, following Microsoft's FAT specification ("fatgen103").
//!
//! Windows' own tools refuse to create FAT32 volumes larger than 32 GB, yet FAT32 is the one
//! file system every CDJ/XDJ reads, at any size up to 2 TB. Writing the structures directly lets
//! `drive format` produce it on every OS, with a chosen cluster size.
//!
//! Every write is a whole number of sectors at a sector-aligned offset (raw disk handles on
//! Windows require that), and nothing is written outside `[0, total_sectors * sector_size)`
//! relative to the start offset. The boot sector goes last, so an interrupted format never
//! looks like a valid volume.

use std::io::{Seek, SeekFrom, Write};

use chrono::{Datelike, Timelike};
use schemars::JsonSchema;
use serde::Serialize;

use crate::progress::{Ctx, Event, Phase};
use crate::{Error, Result};

/// Fewer clusters than this and the volume is FAT16 by definition.
pub const MIN_CLUSTERS: u64 = 65_525;
/// Highest cluster count FAT32's 28-bit entries can address (minus reserved values).
pub const MAX_CLUSTERS: u64 = 0x0FFF_FFF5;
/// Largest cluster size the specification considers compatible.
pub const MAX_CLUSTER_SIZE: u32 = 32 * 1024;

const RESERVED_SECTORS: u64 = 32;
const NUM_FATS: u64 = 2;
const ROOT_CLUSTER: u32 = 2;
const MEDIA: u8 = 0xF8;
const CHUNK: usize = 1 << 20;

/// Where everything goes on a FAT32 volume of a given size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Layout {
    pub bytes_per_sector: u32,
    pub sectors_per_cluster: u32,
    /// Includes padding that aligns the data region to the cluster size.
    pub reserved_sectors: u32,
    pub fat_sectors: u32,
    pub total_sectors: u32,
    /// Sectors before the volume on the disk (its partition's start LBA).
    pub hidden_sectors: u32,
    pub clusters: u32,
}

impl Layout {
    /// Plan a volume of `volume_bytes` (rounded down to whole sectors).
    pub fn new(volume_bytes: u64, bytes_per_sector: u32, cluster_size: u32, hidden_sectors: u32) -> Result<Layout> {
        if !matches!(bytes_per_sector, 512 | 1024 | 2048 | 4096) {
            return Err(Error::invalid(format!("unsupported sector size {bytes_per_sector}")));
        }
        if !cluster_size.is_power_of_two() || cluster_size < bytes_per_sector || cluster_size > MAX_CLUSTER_SIZE {
            return Err(Error::invalid(format!(
                "FAT32 cluster size must be a power of two between the sector size ({bytes_per_sector} bytes) and 32 KB, not {cluster_size}"
            )));
        }
        let bps = u64::from(bytes_per_sector);
        let spc = u64::from(cluster_size / bytes_per_sector);
        let total = volume_bytes / bps;
        if total > u64::from(u32::MAX) {
            return Err(Error::invalid("FAT32 volumes are limited to 2^32 sectors (2 TiB with 512-byte sectors)"));
        }
        // Smallest FAT that covers every cluster the remaining space could hold: with F sectors
        // per FAT, clusters = (total - reserved - 2F) / spc, and F * bps / 4 >= clusters + 2.
        let entries_per_sector = bps / 4;
        let numerator = (total.saturating_sub(RESERVED_SECTORS) + 2 * spc).max(1);
        let fat = numerator.div_ceil(entries_per_sector * spc + NUM_FATS);
        // Pad the reserved area so clusters start on a cluster boundary. A larger reserved area
        // only shrinks the data region, so the FAT computed above still covers it.
        let data_start = RESERVED_SECTORS + NUM_FATS * fat;
        let reserved = RESERVED_SECTORS + (spc - data_start % spc) % spc;
        let data_start = reserved + NUM_FATS * fat;
        let clusters = total.saturating_sub(data_start) / spc;
        if clusters < MIN_CLUSTERS {
            return Err(Error::invalid(format!(
                "a {} MB volume is too small for FAT32 with {} KB clusters (it would hold {clusters} clusters, FAT32 needs at least {MIN_CLUSTERS}); use smaller clusters",
                volume_bytes / 1_000_000,
                cluster_size / 1024,
            )));
        }
        if clusters > MAX_CLUSTERS {
            return Err(Error::invalid(format!(
                "{} KB clusters are too small for a volume this size; use larger clusters",
                cluster_size / 1024
            )));
        }
        debug_assert!(fat * entries_per_sector >= clusters + 2);
        Ok(Layout {
            bytes_per_sector,
            sectors_per_cluster: spc as u32,
            reserved_sectors: reserved as u32,
            fat_sectors: fat as u32,
            total_sectors: total as u32,
            hidden_sectors,
            clusters: clusters as u32,
        })
    }

    pub fn cluster_size(&self) -> u32 {
        self.bytes_per_sector * self.sectors_per_cluster
    }

    /// First sector of the data region (cluster 2).
    pub fn data_start(&self) -> u64 {
        u64::from(self.reserved_sectors) + NUM_FATS * u64::from(self.fat_sectors)
    }

    /// Bytes of user data the volume can hold.
    pub fn capacity(&self) -> u64 {
        u64::from(self.clusters) * u64::from(self.cluster_size())
    }

    /// Bytes [`write()`] puts on the device (for progress).
    pub fn bytes_written(&self) -> u64 {
        (u64::from(self.reserved_sectors) + NUM_FATS * u64::from(self.fat_sectors)) * u64::from(self.bytes_per_sector)
            + u64::from(self.cluster_size())
    }
}

/// The cluster size Microsoft's formatter picks for a FAT32 volume of this size (and macOS's
/// `newfs_msdos` too): 4 KB up to 8 GB, 8 KB up to 16 GB, 16 KB up to 32 GB, 32 KB above.
pub fn default_cluster_size(volume_bytes: u64) -> u32 {
    const GIB: u64 = 1 << 30;
    match volume_bytes {
        b if b <= 8 * GIB => 4096,
        b if b <= 16 * GIB => 8192,
        b if b <= 32 * GIB => 16384,
        _ => 32768,
    }
}

/// Turn any name into a FAT volume label: at most 11 characters, upper case ASCII, none of
/// the characters FAT forbids. Empty results fall back to `fallback`.
pub fn sanitize_label(name: &str, fallback: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        let c = c.to_ascii_uppercase();
        let ok = c.is_ascii_graphic() || c == ' ';
        let forbidden =
            matches!(c, '"' | '*' | '+' | ',' | '.' | '/' | ':' | ';' | '<' | '=' | '>' | '?' | '[' | '\\' | ']' | '|');
        if !ok || forbidden {
            out.push('_');
        } else {
            out.push(c);
        }
        if out.len() == 11 {
            break;
        }
    }
    let out = out.trim().to_string();
    if out.is_empty() || out.chars().all(|c| c == '_') { fallback.to_string() } else { out }
}

/// Write an empty FAT32 file system for `layout` at `start` (byte offset of the volume in `dev`).
/// The caller makes the result durable (e.g. `File::sync_all`).
pub fn write<W: Write + Seek>(
    dev: &mut W,
    start: u64,
    layout: &Layout,
    label: &str,
    volume_id: u32,
    ctx: &Ctx<'_>,
) -> Result<()> {
    let bps = layout.bytes_per_sector as usize;
    let label = label_bytes(label);
    let io = |e: std::io::Error| Error::Io { path: "volume".into(), source: e };
    let put = |dev: &mut W, sector: u64, bytes: &[u8]| -> Result<()> {
        debug_assert!(bytes.len().is_multiple_of(bps));
        debug_assert!(sector + (bytes.len() / bps) as u64 <= u64::from(layout.total_sectors));
        dev.seek(SeekFrom::Start(start + sector * bps as u64)).map_err(io)?;
        dev.write_all(bytes).map_err(io)?;
        ctx.emit(Event::Bytes { phase: Phase::Format, bytes: bytes.len() as u64 });
        Ok(())
    };

    // Reserved area: blank, so no older file system's boot record survives (exFAT keeps a
    // backup in sectors 12-23).
    put(dev, 0, &vec![0u8; layout.reserved_sectors as usize * bps])?;

    // Both FATs: media descriptor, end-of-chain marker (with the "clean" bits set), and the
    // root directory's single cluster; everything else free.
    let fat_bytes = layout.fat_sectors as usize * bps;
    let zeros = vec![0u8; CHUNK];
    for n in 0..NUM_FATS {
        ctx.check()?;
        let first = u64::from(layout.reserved_sectors) + n * u64::from(layout.fat_sectors);
        let mut head = vec![0u8; bps];
        head[0..4].copy_from_slice(&(0x0FFF_FF00 | u32::from(MEDIA)).to_le_bytes());
        head[4..8].copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes());
        head[8..12].copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes());
        put(dev, first, &head)?;
        let mut done = bps;
        while done < fat_bytes {
            ctx.check()?;
            let n = (fat_bytes - done).min(CHUNK);
            put(dev, first + (done / bps) as u64, &zeros[..n])?;
            done += n;
        }
    }

    // Root directory: one cluster holding the volume label entry.
    let mut root = vec![0u8; layout.cluster_size() as usize];
    root[0..11].copy_from_slice(&label);
    root[11] = 0x08; // ATTR_VOLUME_ID
    let (date, time) = fat_timestamp(chrono::Local::now().naive_local());
    root[22..24].copy_from_slice(&time.to_le_bytes());
    root[24..26].copy_from_slice(&date.to_le_bytes());
    put(dev, layout.data_start(), &root)?;

    // FSInfo and boot sector, each with its backup (sectors 6 and 7).
    let info = fs_info(layout, bps);
    let boot = boot_sector(layout, &label, volume_id, bps);
    put(dev, 7, &info)?;
    put(dev, 6, &boot)?;
    put(dev, 1, &info)?;
    put(dev, 0, &boot)?;
    dev.flush().map_err(io)?;
    Ok(())
}

fn label_bytes(label: &str) -> [u8; 11] {
    let mut out = [b' '; 11];
    for (o, b) in out.iter_mut().zip(sanitize_label(label, "NO NAME").bytes()) {
        *o = b;
    }
    out
}

fn boot_sector(l: &Layout, label: &[u8; 11], volume_id: u32, bps: usize) -> Vec<u8> {
    let mut b = vec![0u8; bps];
    b[0..3].copy_from_slice(&[0xEB, 0x58, 0x90]); // jump over the BPB
    b[3..11].copy_from_slice(b"MSWIN4.1"); // the OEM name the spec recommends for compatibility
    b[11..13].copy_from_slice(&(l.bytes_per_sector as u16).to_le_bytes());
    b[13] = l.sectors_per_cluster as u8;
    b[14..16].copy_from_slice(&(l.reserved_sectors as u16).to_le_bytes());
    b[16] = NUM_FATS as u8;
    // 17..19 root entries, 19..21 16-bit sector count: 0 on FAT32.
    b[21] = MEDIA;
    // 22..24 16-bit FAT size: 0 on FAT32.
    b[24..26].copy_from_slice(&63u16.to_le_bytes()); // sectors per track (nominal)
    b[26..28].copy_from_slice(&255u16.to_le_bytes()); // heads (nominal)
    b[28..32].copy_from_slice(&l.hidden_sectors.to_le_bytes());
    b[32..36].copy_from_slice(&l.total_sectors.to_le_bytes());
    b[36..40].copy_from_slice(&l.fat_sectors.to_le_bytes());
    // 40..42 flags (FAT mirroring on), 42..44 version 0.0.
    b[44..48].copy_from_slice(&ROOT_CLUSTER.to_le_bytes());
    b[48..50].copy_from_slice(&1u16.to_le_bytes()); // FSInfo sector
    b[50..52].copy_from_slice(&6u16.to_le_bytes()); // backup boot sector
    b[64] = 0x80; // drive number
    b[66] = 0x29; // extended boot signature: the next three fields are valid
    b[67..71].copy_from_slice(&volume_id.to_le_bytes());
    b[71..82].copy_from_slice(label);
    b[82..90].copy_from_slice(b"FAT32   ");
    b[90..92].copy_from_slice(&[0xCD, 0x18]); // not bootable: int 18h
    b[510] = 0x55;
    b[511] = 0xAA;
    b
}

fn fs_info(l: &Layout, bps: usize) -> Vec<u8> {
    let mut s = vec![0u8; bps];
    s[0..4].copy_from_slice(&0x4161_5252u32.to_le_bytes());
    s[484..488].copy_from_slice(&0x6141_7272u32.to_le_bytes());
    s[488..492].copy_from_slice(&(l.clusters - 1).to_le_bytes()); // all free but the root's
    s[492..496].copy_from_slice(&(ROOT_CLUSTER + 1).to_le_bytes()); // next free cluster hint
    s[508..512].copy_from_slice(&0xAA55_0000u32.to_le_bytes());
    s
}

fn fat_timestamp(t: chrono::NaiveDateTime) -> (u16, u16) {
    let year = (t.year().clamp(1980, 2107) - 1980) as u16;
    let date = year << 9 | (t.month() as u16) << 5 | t.day() as u16;
    let time = (t.hour() as u16) << 11 | (t.minute() as u16) << 5 | (t.second() / 2) as u16;
    (date, time)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    #[test]
    fn layouts_are_aligned_and_consistent() {
        for (size, cluster) in [(64 * GB, 32768), (128 * GB, 32768), (2_000 * GB, 32768), (16 * GB, 8192)] {
            let l = Layout::new(size, 512, cluster, 2048).unwrap();
            assert_eq!(l.data_start() % u64::from(l.sectors_per_cluster), 0, "{size}");
            assert!(u64::from(l.fat_sectors) * 128 >= u64::from(l.clusters) + 2);
            // The FAT is no more than one sector larger than it needs to be.
            assert!(u64::from(l.fat_sectors - 1) * 128 < u64::from(l.clusters) + 2 + 128);
            assert!(
                l.data_start() + u64::from(l.clusters) * u64::from(l.sectors_per_cluster) <= u64::from(l.total_sectors)
            );
        }
    }

    #[test]
    fn rejects_volumes_fat32_cannot_describe() {
        assert!(Layout::new(GB, 512, 32768, 0).is_err(), "too few clusters");
        assert!(Layout::new(3_000 * GB, 512, 32768, 0).is_err(), "more than 2^32 sectors");
        assert!(Layout::new(64 * GB, 512, 65536, 0).is_err(), "64 KB clusters");
        assert!(Layout::new(64 * GB, 512, 3000, 0).is_err(), "not a power of two");
    }

    #[test]
    fn default_cluster_sizes_match_windows_and_macos() {
        assert_eq!(default_cluster_size(8 << 30), 4096);
        assert_eq!(default_cluster_size(16 << 30), 8192);
        assert_eq!(default_cluster_size(32 << 30), 16384);
        assert_eq!(default_cluster_size(128 * GB), 32768);
    }

    #[test]
    fn labels_follow_fat_rules() {
        assert_eq!(sanitize_label("Club drive", "X"), "CLUB DRIVE");
        assert_eq!(sanitize_label("Rekordbox USB 2026", "X"), "REKORDBOX U");
        assert_eq!(sanitize_label("a.b/c", "X"), "A_B_C");
        assert_eq!(sanitize_label("Tōkyō", "X"), "T_KY_");
        assert_eq!(sanitize_label("...", "REKORDBOX"), "REKORDBOX");
        assert_eq!(sanitize_label("", "REKORDBOX"), "REKORDBOX");
    }
}
