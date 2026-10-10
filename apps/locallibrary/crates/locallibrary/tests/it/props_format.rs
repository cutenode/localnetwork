//! Properties of the FAT32 formatter's layout, and a few volumes checked with an independent FAT
//! implementation.

use std::fs::OpenOptions;
use std::io::{Read, Write};

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::format::fat32::{self, Layout, MAX_CLUSTER_SIZE, MAX_CLUSTERS, MIN_CLUSTERS};
use locallibrary::progress::Ctx;

const GIB: u64 = 1 << 30;

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
fn layouts_are_consistent(tc: TestCase) {
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
fn default_cluster_size_always_fits(tc: TestCase) {
    let volume = tc.draw(gs::integers::<u64>().min_value(GIB).max_value((2 << 40) - 512));
    let cluster = fat32::default_cluster_size(volume);
    let l = Layout::new(volume, 512, cluster, 2048).unwrap();
    assert!(u64::from(l.clusters) >= MIN_CLUSTERS);
}

/// Formatted volumes (sparse files) open as FAT32 in `fatfs` with the right label and cluster
/// size, report every cluster but the root directory's free, and hold a file that reads back.
#[hegel::test(test_cases = 10)]
fn fatfs_reads_formatted_volumes(tc: TestCase) {
    let cluster = cluster_size(&tc, 512);
    let clusters = tc.draw(gs::integers::<u64>().min_value(MIN_CLUSTERS + 64).max_value(300_000));
    let slack = tc.draw(gs::integers::<u64>().max_value(u64::from(cluster) * 3));
    let label = tc.draw(gs::text().max_size(14));
    let volume_id = tc.draw(gs::integers::<u32>());
    // Room for the clusters plus the reserved area and FATs.
    let size = clusters * u64::from(cluster) + 64 * 512 + 2 * (clusters * 4).div_ceil(512) * 512 + slack;
    let layout = Layout::new(size, 512, cluster, 2048).unwrap();

    let file = tempfile::NamedTempFile::new().unwrap();
    file.as_file().set_len(size).unwrap();
    let mut f = OpenOptions::new().read(true).write(true).open(file.path()).unwrap();
    fat32::write(&mut f, 0, &layout, &label, volume_id, &Ctx::silent()).unwrap();
    drop(f);

    let f = OpenOptions::new().read(true).write(true).open(file.path()).unwrap();
    let fs = fatfs::FileSystem::new(f, fatfs::FsOptions::new()).unwrap();
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
