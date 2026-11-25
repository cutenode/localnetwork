//! The FAT32 formatter's layouts, and volumes it writes checked with an independent FAT
//! implementation.

use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use loclib_format::Error;
use loclib_format::fat32::{self, Layout, MAX_CLUSTER_SIZE, MAX_CLUSTERS, MIN_CLUSTERS};
use loclib_progress::Ctx;
use rustc_hash::FxHashMap;

const MB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;

/// A disk of `len` bytes that keeps only the chunks written to and reads zeros elsewhere: room for
/// a volume of many gigabytes in a test, without a file that takes the room.
struct Sparse {
    len: u64,
    at: u64,
    chunks: FxHashMap<u64, Vec<u8>>,
}

const CHUNK: u64 = 1 << 16;

impl Sparse {
    fn new(len: u64) -> Sparse {
        Sparse { len, at: 0, chunks: FxHashMap::default() }
    }

    /// Bytes from where it is to the end of its chunk or of the disk, at most `want`.
    fn span(&self, want: usize) -> usize {
        let to_chunk_end = CHUNK - self.at % CHUNK;
        want.min(to_chunk_end.min(self.len.saturating_sub(self.at)) as usize)
    }
}

impl Read for Sparse {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.span(buf.len());
        let offset = (self.at % CHUNK) as usize;
        match self.chunks.get(&(self.at / CHUNK)) {
            Some(chunk) => buf[..n].copy_from_slice(&chunk[offset..offset + n]),
            None => buf[..n].fill(0),
        }
        self.at += n as u64;
        Ok(n)
    }
}

impl Write for Sparse {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.span(buf.len());
        if n == 0 {
            return Ok(0);
        }
        let offset = (self.at % CHUNK) as usize;
        let chunk = self.chunks.entry(self.at / CHUNK).or_insert_with(|| vec![0; CHUNK as usize]);
        chunk[offset..offset + n].copy_from_slice(&buf[..n]);
        self.at += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for Sparse {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let at = match to {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::End(d) => self.len.checked_add_signed(d),
            SeekFrom::Current(d) => self.at.checked_add_signed(d),
        };
        self.at = at.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "before the start of the disk"))?;
        Ok(self.at)
    }
}

/// A `size`-byte volume formatted with `cluster`-byte clusters and `label`, and its layout.
fn formatted(size: u64, cluster: u32, label: &str) -> (Sparse, Layout) {
    let layout = Layout::new(size, 512, cluster, 2048).unwrap();
    let mut disk = Sparse::new(size);
    fat32::write(&mut disk, 0, &layout, label, 0x1234_5678, &Ctx::silent()).unwrap();
    // fatfs reads the boot sector from where the disk is.
    disk.rewind().unwrap();
    (disk, layout)
}

#[test]
fn fatfs_reads_what_we_write() {
    for (size, cluster) in [(40 * MB, 512), (300 * MB, 4096), (2100 * MB, 32768)] {
        let (disk, layout) = formatted(size, cluster, "Club Drive");
        let fs = fatfs::FileSystem::new(disk, fatfs::FsOptions::new()).unwrap();
        assert_eq!(fs.fat_type(), fatfs::FatType::Fat32, "{size}");
        assert_eq!(fs.cluster_size(), cluster);
        assert_eq!(fs.volume_id(), 0x1234_5678);
        assert_eq!(fs.volume_label(), "CLUB DRIVE");
        assert_eq!(fs.read_volume_label_from_root_dir().unwrap().as_deref(), Some("CLUB DRIVE"));
        let stats = fs.stats().unwrap();
        assert_eq!(stats.total_clusters(), layout.clusters);
        assert_eq!(stats.free_clusters(), layout.clusters - 1, "only the root directory is in use");

        // Usable: write a file bigger than a cluster into a folder, then read it back.
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        {
            let root = fs.root_dir();
            let dir = root.create_dir("PIONEER").unwrap();
            dir.create_file("export.pdb").unwrap().write_all(&data).unwrap();
            let mut back = Vec::new();
            root.open_file("PIONEER/export.pdb").unwrap().read_to_end(&mut back).unwrap();
            assert_eq!(back, data);
        }
        fs.unmount().unwrap();
    }
}

#[test]
fn boot_sector_and_backups() {
    let (mut disk, layout) = formatted(300 * MB, 4096, "DJ");
    let mut sector = |n: u64| {
        let mut b = [0u8; 512];
        disk.seek(SeekFrom::Start(n * 512)).unwrap();
        disk.read_exact(&mut b).unwrap();
        b
    };
    let boot = sector(0);
    assert_eq!(&boot[510..], &[0x55, 0xAA]);
    assert_eq!(&boot[3..11], b"MSWIN4.1");
    assert_eq!(&boot[82..90], b"FAT32   ");
    assert_eq!(u32::from_le_bytes(boot[28..32].try_into().unwrap()), 2048, "hidden sectors");
    assert_eq!(sector(6), boot, "backup boot sector");
    assert_eq!(sector(7), sector(1), "backup FSInfo");
    // Clusters start on a cluster boundary.
    assert_eq!(layout.data_start() % u64::from(layout.sectors_per_cluster), 0);
}

#[test]
fn nothing_is_written_past_the_volume() {
    let size = 64 * MB;
    // The volume sits in the middle of a larger "disk" full of 0xEE.
    let mut disk = Cursor::new(vec![0xEEu8; (size + 4 * MB) as usize]);
    let layout = Layout::new(size, 512, 512, 2048).unwrap();
    fat32::write(&mut disk, 2 * MB, &layout, "X", 1, &Ctx::silent()).unwrap();
    let after = disk.into_inner();
    assert!(after[..(2 * MB) as usize].iter().all(|&b| b == 0xEE), "before the volume");
    assert!(after[(2 * MB + size) as usize..].iter().all(|&b| b == 0xEE), "after the volume");
}

// -----------------------------------------------------------------------------------------
// Properties
// -----------------------------------------------------------------------------------------

fn sector_size(tc: &TestCase) -> u32 {
    tc.draw(gs::sampled_from(vec![512u32, 1024, 2048, 4096]))
}

/// A cluster size FAT32 accepts with `bps`-byte sectors.
fn cluster_size(tc: &TestCase, bps: u32) -> u32 {
    let shift =
        tc.draw(gs::integers::<u32>().min_value(bps.trailing_zeros()).max_value(MAX_CLUSTER_SIZE.trailing_zeros()));
    1 << shift
}

/// Every layout the planner accepts is self-consistent: clusters start on a cluster boundary, the
/// FAT has an entry for every cluster (plus the two reserved ones) and is at most one sector
/// larger than the smallest that does (the planner rounds a fractional cluster count up),
/// everything fits in the volume, the cluster count makes it FAT32, and the boot sector's 16-
/// and 8-bit fields can hold the values.
#[hegel::test(test_cases = 1000)]
fn prop_layouts_are_consistent(tc: TestCase) {
    let bps = sector_size(&tc);
    let cluster = cluster_size(&tc, bps);
    let volume = tc.draw(hegel::one_of!(
        gs::integers::<u64>().max_value(20 << 40),
        gs::integers::<u64>().min_value(64 << 20).max_value(2 << 40),
    ));
    let hidden = tc.draw(gs::integers::<u32>());
    let Ok(l) = Layout::new(volume, bps, cluster, hidden) else { return };
    let (spc, eps) = (u64::from(l.sectors_per_cluster), u64::from(bps / 4));
    let (fat, clusters) = (u64::from(l.fat_sectors), u64::from(l.clusters));
    assert_eq!(l.cluster_size(), cluster);
    assert_eq!(u64::from(l.total_sectors), volume / u64::from(bps));
    assert_eq!(l.data_start() % spc, 0, "data region is cluster-aligned");
    assert!(fat * eps >= clusters + 2, "FAT covers every cluster");
    // Two sectors less could not cover what the remaining space holds (before alignment padding).
    if let Some(smaller) = fat.checked_sub(2) {
        let unpadded = (u64::from(l.total_sectors) - 32 - 2 * smaller) / spc;
        assert!(smaller * eps < unpadded + 2, "FAT is within a sector of minimal: {l:?}");
    }
    assert!(l.data_start() + clusters * spc <= u64::from(l.total_sectors), "fits the volume");
    assert!(u64::from(l.total_sectors) - (l.data_start() + clusters * spc) < spc, "no whole cluster wasted");
    assert!((MIN_CLUSTERS..=MAX_CLUSTERS).contains(&clusters));
    assert!(l.reserved_sectors >= 32 && l.reserved_sectors <= u32::from(u16::MAX));
    assert!(l.sectors_per_cluster <= u32::from(u8::MAX));
    assert_eq!(l.hidden_sectors, hidden);
    assert_eq!(l.capacity(), clusters * u64::from(cluster));
}

/// The default cluster size works for every drive between 1 GiB and FAT32's 2 TiB limit.
#[hegel::test(test_cases = 500)]
fn prop_default_cluster_size_always_fits(tc: TestCase) {
    let volume = tc.draw(gs::integers::<u64>().min_value(GIB).max_value((2 << 40) - 512));
    let cluster = fat32::default_cluster_size(volume);
    let l = Layout::new(volume, 512, cluster, 2048).unwrap();
    assert!(u64::from(l.clusters) >= MIN_CLUSTERS);
}

/// Formatted volumes open as FAT32 in `fatfs` with the right label and cluster size, report
/// every cluster but the root directory's free, and hold a file that reads back.
#[hegel::test(test_cases = 10)]
fn prop_fatfs_reads_formatted_volumes(tc: TestCase) {
    let cluster = cluster_size(&tc, 512);
    let clusters = tc.draw(gs::integers::<u64>().min_value(MIN_CLUSTERS + 64).max_value(300_000));
    let slack = tc.draw(gs::integers::<u64>().max_value(u64::from(cluster) * 3));
    let label = tc.draw(gs::text().max_size(14));
    let volume_id = tc.draw(gs::integers::<u32>());
    // Room for the clusters plus the reserved area and FATs.
    let size = clusters * u64::from(cluster) + 64 * 512 + 2 * (clusters * 4).div_ceil(512) * 512 + slack;
    let layout = Layout::new(size, 512, cluster, 2048).unwrap();

    let mut disk = Sparse::new(size);
    fat32::write(&mut disk, 0, &layout, &label, volume_id, &Ctx::silent()).unwrap();
    disk.rewind().unwrap();

    let fs = fatfs::FileSystem::new(disk, fatfs::FsOptions::new()).unwrap();
    assert_eq!(fs.fat_type(), fatfs::FatType::Fat32);
    assert_eq!(fs.cluster_size(), cluster);
    assert_eq!(fs.volume_id(), volume_id);
    let expected = fat32::sanitize_label(&label, "NO NAME");
    assert_eq!(fs.volume_label().trim_end(), expected);
    assert_eq!(fs.read_volume_label_from_root_dir().unwrap().as_deref().map(str::trim_end), Some(expected.as_str()));
    let stats = fs.stats().unwrap();
    assert_eq!(stats.total_clusters(), layout.clusters);
    assert_eq!(stats.free_clusters(), layout.clusters - 1);

    let data: Vec<u8> = (0..3 * cluster + 17).map(|i| (i % 251) as u8).collect();
    {
        let root = fs.root_dir();
        root.create_dir("PIONEER").unwrap().create_file("export.pdb").unwrap().write_all(&data).unwrap();
        let mut back = Vec::new();
        root.open_file("PIONEER/export.pdb").unwrap().read_to_end(&mut back).unwrap();
        assert_eq!(back, data);
    }
    fs.unmount().unwrap();
}

/// A layout is refused, as invalid, for a sector size other than the four drives use, and for
/// a cluster size that isn't a power of two from the sector size to 32 KB, whatever the volume.
#[hegel::test(test_cases = 300)]
fn prop_layouts_refuse_sectors_and_clusters_fat32_cant_take(tc: TestCase) {
    let volume = tc.draw(gs::integers::<u64>().min_value(64 << 20).max_value(2 << 40));
    let hidden = tc.draw(gs::integers::<u32>());
    let good_bps = sector_size(&tc);
    let bps = tc.draw(hegel::one_of!(gs::just(good_bps), gs::integers::<u32>().max_value(1 << 16)));
    let cluster = tc.draw(hegel::one_of!(
        gs::integers::<u32>().max_value(20).map(|s| 1u32 << s),
        gs::integers::<u32>().max_value(1 << 20),
    ));
    let sector_ok = [512, 1024, 2048, 4096].contains(&bps);
    let cluster_ok = cluster.is_power_of_two() && cluster >= bps && cluster <= 32 << 10;
    match Layout::new(volume, bps, cluster, hidden) {
        Ok(l) => {
            assert!(sector_ok && cluster_ok, "{bps}-byte sectors, {cluster}-byte clusters: {l:?}");
            assert_eq!((l.bytes_per_sector, l.cluster_size()), (bps, cluster));
        }
        Err(e) => {
            assert!(matches!(e, Error::Invalid(_)), "{e}");
            let msg = e.to_string();
            if !sector_ok {
                assert!(msg.contains(&format!("unsupported sector size {bps}")), "{msg}");
            } else if !cluster_ok {
                assert!(msg.contains("power of two") && msg.ends_with(&format!("not {cluster}")), "{msg}");
            }
        }
    }
}

/// With a sector and a cluster size FAT32 takes, a volume is refused, saying which limit it
/// breaks, when it has more sectors than FAT32 counts, when it can't hold FAT32's fewest
/// clusters, and when it would hold twice its most. A volume that's taken holds a cluster count
/// FAT32 allows, and no more than its size.
#[hegel::test(test_cases = 500)]
fn prop_layouts_refuse_volumes_fat32_cant_count(tc: TestCase) {
    let bps = sector_size(&tc);
    let cluster = cluster_size(&tc, bps);
    let volume = tc.draw(hegel::one_of!(
        gs::integers::<u64>().max_value(u64::from(cluster) * MIN_CLUSTERS),
        gs::integers::<u64>().min_value(u64::from(cluster) * 2 * MAX_CLUSTERS).max_value(1 << 50),
        gs::integers::<u64>().max_value(1 << 50),
    ));
    let hidden = tc.draw(gs::integers::<u32>());
    let refused = match Layout::new(volume, bps, cluster, hidden) {
        Ok(l) => {
            assert!((MIN_CLUSTERS..=MAX_CLUSTERS).contains(&u64::from(l.clusters)), "{l:?}");
            assert!(l.capacity() <= volume, "{l:?}");
            None
        }
        Err(e) => {
            assert!(matches!(e, Error::Invalid(_)), "{e}");
            Some(e.to_string())
        }
    };
    let clusters_at_most = volume / u64::from(cluster);
    let limit = if volume / u64::from(bps) > u64::from(u32::MAX) {
        Some("limited to 2^32 sectors")
    } else if clusters_at_most < MIN_CLUSTERS {
        Some("too small for FAT32")
    } else if clusters_at_most >= 2 * MAX_CLUSTERS {
        Some("use larger clusters")
    } else {
        None
    };
    if let Some(limit) = limit {
        let msg = refused.unwrap_or_else(|| panic!("{volume} bytes, {cluster}-byte clusters: taken"));
        assert!(msg.contains(limit), "{msg}");
    }
}
