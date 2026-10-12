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
fn only_sector_sizes_disks_use_are_taken() {
    for bps in [512, 4096] {
        assert_eq!(Layout::new(64 * GB, bps, 32768, 0).unwrap().bytes_per_sector, bps);
    }
    let err = Layout::new(64 * GB, 520, 32768, 0).unwrap_err();
    assert_eq!(err.to_string(), "unsupported sector size 520");
}

/// The progress a format reports adds up to what it said it would write.
#[test]
fn progress_adds_up_to_the_bytes_written() {
    let layout = Layout::new(64 << 20, 512, 512, 0).unwrap();
    let reported = std::sync::atomic::AtomicU64::new(0);
    let count = |e: &Event| {
        if let Event::Bytes { phase: Phase::Format, bytes } = e {
            reported.fetch_add(*bytes, std::sync::atomic::Ordering::Relaxed);
        }
    };
    let mut volume = std::io::Cursor::new(Vec::new());
    write(&mut volume, 0, &layout, "USB", 1, &Ctx::new(&count, Default::default())).unwrap();
    assert_eq!(reported.into_inner(), layout.bytes_written());
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

// -----------------------------------------------------------------------------------------
// Properties
// -----------------------------------------------------------------------------------------

use hegel::TestCase;
use hegel::generators as gs;

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// A written volume's boot sector describes its layout (and its backup is the same), the
/// FSInfo sectors count every cluster but the root's free, both FATs start with the media
/// byte and the root's end of chain, and the root directory holds the label. Nothing outside
/// the volume is touched, and progress adds up to what was written. Cancelled, it leaves no
/// boot sector.
#[hegel::test(test_cases = 30)]
fn written_volumes_describe_their_layout(tc: TestCase) {
    let bps = tc.draw(gs::sampled_from(vec![512u32, 1024, 2048, 4096]));
    let shift = tc.draw(gs::integers::<u32>().min_value(bps.trailing_zeros()).max_value(15));
    let cluster = 1u32 << shift;
    let clusters = tc.draw(gs::integers::<u64>().min_value(MIN_CLUSTERS + 100).max_value(MIN_CLUSTERS + 20_000));
    let size = clusters * u64::from(cluster) + 1024 * u64::from(bps);
    let hidden = tc.draw(gs::integers::<u32>());
    let l = Layout::new(size, bps, cluster, hidden).unwrap();
    let label = tc.draw(gs::text().max_size(14));
    let volume_id = tc.draw(gs::integers::<u32>());
    let start = u64::from(bps) * tc.draw(gs::integers::<u64>().max_value(3));
    let cancelled = tc.draw(gs::weighted_booleans(0.2));

    let reported = std::sync::atomic::AtomicU64::new(0);
    let count = |e: &Event| {
        if let Event::Bytes { phase: Phase::Format, bytes } = e {
            reported.fetch_add(*bytes, std::sync::atomic::Ordering::Relaxed);
        }
    };
    let cancel = crate::progress::CancelToken::new();
    if cancelled {
        cancel.cancel();
    }
    let mut dev = std::io::Cursor::new(vec![0xEEu8; start as usize]);
    let done = write(&mut dev, start, &l, &label, volume_id, &Ctx::new(&count, cancel));
    let img = dev.into_inner();
    assert!(img[..start as usize].iter().all(|&b| b == 0xEE), "before the volume");
    let v = &img[start as usize..];
    let sector = bps as usize;
    if cancelled {
        assert!(done.is_err() && v[510..512] != [0x55, 0xAA], "a cancelled format looks like a volume");
        return;
    }
    done.unwrap();
    assert_eq!(reported.into_inner(), l.bytes_written());
    assert_eq!(v.len() as u64, (l.data_start() * u64::from(bps)) + u64::from(cluster), "ends after the root");

    let boot = &v[..sector];
    assert_eq!(
        (le16(boot, 11), u32::from(boot[13]), u32::from(le16(boot, 14))),
        (bps as u16, l.sectors_per_cluster, l.reserved_sectors)
    );
    assert_eq!((boot[16], boot[21], le32(boot, 28), le32(boot, 32)), (2, MEDIA, hidden, l.total_sectors));
    assert_eq!((le32(boot, 36), le32(boot, 44), le16(boot, 48), le16(boot, 50)), (l.fat_sectors, ROOT_CLUSTER, 1, 6));
    assert_eq!((le32(boot, 67), &boot[71..82], &boot[82..90]), (volume_id, &label_bytes(&label)[..], &b"FAT32   "[..]));
    assert_eq!(&boot[510..512], [0x55, 0xAA]);
    assert_eq!(&v[6 * sector..7 * sector], boot, "backup boot sector");
    for info in [&v[sector..2 * sector], &v[7 * sector..8 * sector]] {
        assert_eq!((le32(info, 0), le32(info, 484), le32(info, 508)), (0x4161_5252, 0x6141_7272, 0xAA55_0000));
        assert_eq!((le32(info, 488), le32(info, 492)), (l.clusters - 1, ROOT_CLUSTER + 1));
    }
    let fats = (0..2).map(|n| (u64::from(l.reserved_sectors) + n * u64::from(l.fat_sectors)) as usize * sector);
    for at in fats {
        assert_eq!((le32(v, at), le32(v, at + 4), le32(v, at + 8)), (0x0FFF_FFF8, 0x0FFF_FFFF, 0x0FFF_FFFF));
        assert!(v[at + 12..at + l.fat_sectors as usize * sector].iter().all(|&b| b == 0), "the rest is free");
    }
    let root = &v[l.data_start() as usize * sector..];
    assert_eq!((&root[..11], root[11]), (&label_bytes(&label)[..], 0x08));
}

/// Timestamps keep the date and the time to two seconds, years clamped to what FAT holds
/// (1980 to 2107).
#[hegel::test(test_cases = 300)]
fn timestamps_keep_the_date_and_time(tc: TestCase) {
    let secs = tc.draw(gs::integers::<i64>().min_value(-2_000_000_000).max_value(8_000_000_000));
    let t = chrono::DateTime::from_timestamp(secs, 0).unwrap().naive_utc();
    let (date, time) = fat_timestamp(t);
    let year = i32::from(date >> 9) + 1980;
    assert_eq!(year, t.year().clamp(1980, 2107));
    assert_eq!((u32::from(date >> 5 & 0xF), u32::from(date & 0x1F)), (t.month(), t.day()));
    let hms = (u32::from(time >> 11), u32::from(time >> 5 & 0x3F), u32::from(time & 0x1F) * 2);
    assert_eq!(hms, (t.hour(), t.minute(), t.second() / 2 * 2));
}
