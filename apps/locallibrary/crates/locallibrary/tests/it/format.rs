//! The FAT32 formatter, checked with an independent FAT implementation.

mod plans;

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};

use locallibrary::format::fat32::{self, Layout};
use locallibrary::progress::Ctx;

const MB: u64 = 1 << 20;

fn formatted(size: u64, cluster: u32, label: &str) -> (tempfile::NamedTempFile, Layout) {
    let file = tempfile::NamedTempFile::new().unwrap();
    file.as_file().set_len(size).unwrap();
    let layout = Layout::new(size, 512, cluster, 2048).unwrap();
    let mut f = OpenOptions::new().read(true).write(true).open(file.path()).unwrap();
    fat32::write(&mut f, 0, &layout, label, 0x1234_5678, &Ctx::silent()).unwrap();
    (file, layout)
}

#[test]
fn fatfs_reads_what_we_write() {
    for (size, cluster) in [(40 * MB, 512), (300 * MB, 4096), (2100 * MB, 32768)] {
        let (file, layout) = formatted(size, cluster, "Club Drive");
        let f = OpenOptions::new().read(true).write(true).open(file.path()).unwrap();
        let fs = fatfs::FileSystem::new(f, fatfs::FsOptions::new()).unwrap();
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
    let (file, layout) = formatted(300 * MB, 4096, "DJ");
    let mut f = std::fs::File::open(file.path()).unwrap();
    let mut sector = |n: u64| {
        let mut b = [0u8; 512];
        f.seek(SeekFrom::Start(n * 512)).unwrap();
        f.read_exact(&mut b).unwrap();
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
    let file = tempfile::NamedTempFile::new().unwrap();
    // The volume sits in the middle of a larger "disk" full of 0xEE.
    let disk = vec![0xEEu8; (size + 4 * MB) as usize];
    std::fs::write(file.path(), &disk).unwrap();
    let layout = Layout::new(size, 512, 512, 2048).unwrap();
    let mut f = OpenOptions::new().read(true).write(true).open(file.path()).unwrap();
    fat32::write(&mut f, 2 * MB, &layout, "X", 1, &Ctx::silent()).unwrap();
    let after = std::fs::read(file.path()).unwrap();
    assert!(after[..(2 * MB) as usize].iter().all(|&b| b == 0xEE), "before the volume");
    assert!(after[(2 * MB + size) as usize..].iter().all(|&b| b == 0xEE), "after the volume");
}
