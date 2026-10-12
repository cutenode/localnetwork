//! Catching fake-capacity drives before a library is written to them.
//!
//! A fake stick reports a large size (often 1–2 TB) and quietly wraps writes past its real
//! capacity onto what it already holds, or drops them. A library on it works until it grows past
//! the real size, then breaks. In 2023 GRC bought 12 cheap 1–2 TB sticks on Amazon; all 12 were
//! fake, typically with about 64 GB that worked.
//!
//! The probe follows f3probe's idea. It writes a distinct 1 MiB pattern to regions spread over
//! the whole disk, highest first, so on a drive that wraps, the low regions written last
//! overwrite the high ones that alias them. It then reads every region back (from the device,
//! not the operating system's cache) and checks each pattern. A real drive returns them all; a
//! fake one returns other regions' patterns or nothing past its real size.
//!
//! It overwrites data at every region, so it only runs as part of a format, which erases the
//! disk anyway. The first region (the partition table and the boot sector) is wiped before
//! anything else is written: a probe that stops part way (cancelled, an I/O error) leaves a disk
//! that no longer mounts and has to be formatted. A check over the free space of a drive that
//! already holds a library is unsafe on exactly the drives it is meant to catch (the test writes
//! land on the library).

use std::io;

use schemars::JsonSchema;
use serde::Serialize;

use crate::progress::{Ctx, Event, Phase};
use crate::{Error, Result};

const MAGIC: &[u8; 8] = b"LLCAPCHK";
pub const REGION: usize = 1 << 20;
const MAX_REGIONS: u64 = 256;

/// Raw access to a whole disk.
pub trait RawDisk {
    fn write_at(&mut self, offset: u64, buf: &[u8]) -> io::Result<()>;
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()>;
    /// Make the writes reach the device, and make the next reads come from it.
    fn flush(&mut self) -> io::Result<()>;
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProbeReport {
    /// The size the disk reports.
    pub claimed: u64,
    pub regions: usize,
    pub failed: usize,
    /// Every region below this offset held its data (`None`: all did).
    pub works_up_to: Option<u64>,
    /// Data that came back at the wrong place, the signature of a drive that wraps writes
    /// around. At most 8.
    pub aliases: Vec<Alias>,
}

crate::schema::labelled_tuple! {
    /// Where a region was written and where its data was read back: `[written_at, read_at]`,
    /// byte offsets.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Alias {
        pub written_at: u64,
        pub read_at: u64,
    }
}

impl ProbeReport {
    pub fn is_fake(&self) -> bool {
        self.failed > 0
    }
}

/// The pattern for the region at `offset`: the magic, the offset, then pseudo-random bytes.
fn pattern(offset: u64, nonce: u64, buf: &mut [u8]) {
    buf[..8].copy_from_slice(MAGIC);
    buf[8..16].copy_from_slice(&offset.to_le_bytes());
    buf[16..24].copy_from_slice(&nonce.to_le_bytes());
    let mut x = offset ^ nonce ^ 0x9E37_79B9_7F4A_7C15;
    for chunk in buf[24..].chunks_mut(8) {
        // xorshift64
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        chunk.copy_from_slice(&x.to_le_bytes()[..chunk.len()]);
    }
}

/// How many regions [`probe`] tests on a disk of `size` bytes.
pub fn region_count(size: u64, align: u32) -> usize {
    regions(size, u64::from(align.max(512))).len()
}

/// Region offsets: spread evenly from the start to the last whole region, `align`-aligned, at
/// least a region apart (`size` is whole sectors). Overlapping regions would fail on a real
/// disk: the lower one, written later, overwrites the higher one's pattern.
fn regions(size: u64, align: u64) -> Vec<u64> {
    let region = REGION as u64;
    if size < region {
        return Vec::new();
    }
    let last = (size - region) / align * align;
    let n = (size / region).clamp(1, MAX_REGIONS);
    if n == 1 {
        // Room for one region only: the last one would overlap it.
        return vec![0];
    }
    // The i-th of n - 1 equal steps, so that the last region is exactly `last`.
    (0..n).map(|i| (u128::from(last) * u128::from(i) / u128::from(n - 1)) as u64 / align * align).collect()
}

/// Write a pattern to regions across `size` bytes of `disk`, highest first, then read them all
/// back. `align` is the disk's sector size. The first region is wiped before anything else.
pub fn probe(disk: &mut dyn RawDisk, size: u64, align: u32, ctx: &Ctx<'_>) -> Result<ProbeReport> {
    let offsets = regions(size, u64::from(align.max(512)));
    let mut nonce_bytes = [0u8; 8];
    getrandom::fill(&mut nonce_bytes).map_err(|e| Error::invalid(format!("no random numbers: {e}")))?;
    let nonce = u64::from_le_bytes(nonce_bytes);
    let io = |e: io::Error| Error::Io { path: "disk".into(), source: e };
    let mut buf = vec![0u8; REGION];
    if !offsets.is_empty() {
        disk.write_at(0, &buf).map_err(io)?;
        disk.flush().map_err(io)?;
    }

    let total = offsets.len() as u64 * REGION as u64;
    ctx.phase_start(Phase::Write, offsets.len() as u64, total);
    for &off in offsets.iter().rev() {
        ctx.check()?;
        pattern(off, nonce, &mut buf);
        disk.write_at(off, &buf).map_err(io)?;
        ctx.emit(Event::Bytes { phase: Phase::Write, bytes: REGION as u64 });
    }
    disk.flush().map_err(io)?;
    ctx.phase_end(Phase::Write);

    let mut report =
        ProbeReport { claimed: size, regions: offsets.len(), failed: 0, works_up_to: None, aliases: Vec::new() };
    let mut want = vec![0u8; REGION];
    ctx.phase_start(Phase::Verify, offsets.len() as u64, total);
    for &off in &offsets {
        ctx.check()?;
        pattern(off, nonce, &mut want);
        let read = disk.read_at(off, &mut buf).is_ok();
        if !(read && buf == want) {
            report.failed += 1;
            report.works_up_to.get_or_insert(off);
            // A failed read leaves an earlier region's data in the buffer: that's no alias.
            if read && &buf[..8] == MAGIC && buf[16..24] == nonce.to_le_bytes() && report.aliases.len() < 8 {
                let written_at = u64::from_le_bytes(buf[8..16].try_into().unwrap());
                if written_at != off {
                    report.aliases.push(Alias { written_at, read_at: off });
                }
            }
        }
        ctx.emit(Event::Bytes { phase: Phase::Verify, bytes: REGION as u64 });
    }
    ctx.phase_end(Phase::Verify);
    Ok(report)
}

/// A raw whole-disk device opened for the probe.
pub struct DeviceFile {
    file: std::fs::File,
}

impl DeviceFile {
    /// Open `path` (macOS: the unbuffered `/dev/rdiskN`; Linux: `/dev/sdX`) for reading and
    /// writing. All its volumes must be unmounted.
    pub fn open(path: &str) -> Result<DeviceFile> {
        let file = std::fs::OpenOptions::new().read(true).write(true).open(path).map_err(|e| match e.kind() {
            io::ErrorKind::PermissionDenied => {
                Error::Permission(format!("checking the capacity of {path} needs administrator rights"))
            }
            _ => Error::Io { path: path.into(), source: e },
        })?;
        Ok(DeviceFile { file })
    }
}

impl RawDisk for DeviceFile {
    fn write_at(&mut self, offset: u64, buf: &[u8]) -> io::Result<()> {
        use std::io::{Seek, SeekFrom, Write};
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(buf)
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        use std::io::{Read, Seek, SeekFrom};
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        // macOS's raw /dev/rdisk devices aren't buffered and refuse fsync (ENOTTY).
        match self.file.sync_all() {
            Err(e) if cfg!(target_os = "macos") && e.raw_os_error() == Some(25) => {}
            other => other?,
        }
        // Linux caches block devices: drop clean pages so the reads come from the drive. (macOS's
        // raw /dev/rdisk devices aren't cached.) Formatting runs as root on Linux.
        #[cfg(all(unix, not(target_os = "macos")))]
        std::fs::write("/proc/sys/vm/drop_caches", b"1")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A disk that keeps only as many bytes as `data` holds and wraps every address past them.
    struct FakeDisk {
        data: Vec<u8>,
    }

    impl RawDisk for FakeDisk {
        fn write_at(&mut self, offset: u64, buf: &[u8]) -> io::Result<()> {
            let n = self.data.len() as u64;
            for (i, b) in buf.iter().enumerate() {
                self.data[((offset + i as u64) % n) as usize] = *b;
            }
            Ok(())
        }
        fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
            let n = self.data.len() as u64;
            for (i, b) in buf.iter_mut().enumerate() {
                *b = self.data[((offset + i as u64) % n) as usize];
            }
            Ok(())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    const MIB: u64 = 1 << 20;

    #[test]
    fn a_real_disk_passes() {
        let mut d = FakeDisk { data: vec![0; (64 * MIB) as usize] };
        let r = probe(&mut d, 64 * MIB, 512, &Ctx::silent()).unwrap();
        assert!(!r.is_fake(), "{r:?}");
        assert_eq!(r.regions, 64);
    }

    #[test]
    fn a_wrapping_disk_is_caught_and_its_real_size_shown() {
        // Claims 256 MiB, holds 64.
        let mut d = FakeDisk { data: vec![0; (64 * MIB) as usize] };
        let r = probe(&mut d, 256 * MIB, 512, &Ctx::silent()).unwrap();
        assert!(r.is_fake());
        let works = r.works_up_to.unwrap();
        assert!((60 * MIB..=64 * MIB).contains(&works), "{works}");
        // Regions above 64 MiB read back the low regions written over them.
        assert!(
            r.aliases.iter().all(|a| a.read_at > a.written_at && (a.read_at - a.written_at) % (64 * MIB) == 0),
            "{:?}",
            r.aliases
        );
    }

    /// Fails every write after the first `ok` (the wipe counts as one).
    struct FailingDisk {
        inner: FakeDisk,
        ok: usize,
    }

    impl RawDisk for FailingDisk {
        fn write_at(&mut self, offset: u64, buf: &[u8]) -> io::Result<()> {
            if self.ok == 0 {
                return Err(io::Error::other("unplugged"));
            }
            self.ok -= 1;
            self.inner.write_at(offset, buf)
        }
        fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
            self.inner.read_at(offset, buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Regression: the first region (partition table, boot sector) was written last, so a probe
    /// stopped part way left the old volume mounting with damaged files.
    #[test]
    fn a_probe_stopped_part_way_leaves_a_disk_that_no_longer_mounts() {
        let mut data = vec![0x55; (16 * MIB) as usize];
        data[510..512].copy_from_slice(&[0x55, 0xAA]);
        let mut d = FailingDisk { inner: FakeDisk { data }, ok: 3 };
        assert!(probe(&mut d, 16 * MIB, 512, &Ctx::silent()).is_err());
        assert!(d.inner.data[..REGION].iter().all(|&b| b == 0), "the boot sector is gone");
    }

    /// A disk that keeps only as many bytes as `data` holds, drops writes past them and fails
    /// reads there, as many fake sticks do once their real capacity is used up.
    struct DroppingDisk {
        data: Vec<u8>,
    }

    impl RawDisk for DroppingDisk {
        fn write_at(&mut self, offset: u64, buf: &[u8]) -> io::Result<()> {
            if let Some(dst) = self.data.get_mut(offset as usize..offset as usize + buf.len()) {
                dst.copy_from_slice(buf);
            }
            Ok(())
        }
        fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
            let src =
                self.data.get(offset as usize..offset as usize + buf.len()).ok_or(io::ErrorKind::UnexpectedEof)?;
            buf.copy_from_slice(src);
            Ok(())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_disk_that_fails_reads_past_its_real_size_is_caught() {
        let mut d = DroppingDisk { data: vec![0; (32 * MIB) as usize] };
        let r = probe(&mut d, 128 * MIB, 512, &Ctx::silent()).unwrap();
        assert!(r.is_fake());
        assert_eq!(r.failed, r.regions - 32, "{r:?}");
        assert!((31 * MIB..=32 * MIB).contains(&r.works_up_to.unwrap()), "{r:?}");
        assert!(r.aliases.is_empty(), "nothing came back from the wrong place");
    }

    /// A disk smaller than one test region has nothing to test, and nothing is written to it.
    #[test]
    fn a_disk_smaller_than_a_region_is_left_alone() {
        let mut d = FailingDisk { inner: FakeDisk { data: vec![0; REGION / 2] }, ok: 0 };
        let r = probe(&mut d, REGION as u64 / 2, 512, &Ctx::silent()).unwrap();
        assert_eq!((r.regions, r.failed, r.works_up_to), (0, 0, None));
        assert_eq!(region_count(REGION as u64 / 2, 512), 0);
    }

    /// The probe reads back from the device file it wrote, on a file standing in for a disk.
    /// Not on Linux, where flushing drops the kernel's caches, which only root may.
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    #[test]
    fn a_device_file_is_probed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rdisk4");
        std::fs::File::create(&path).unwrap().set_len(8 * MIB).unwrap();
        let mut dev = DeviceFile::open(&path.to_string_lossy()).unwrap();
        let r = probe(&mut dev, 8 * MIB, 512, &Ctx::silent()).unwrap();
        assert!(!r.is_fake() && r.regions == 8, "{r:?}");
        let err = DeviceFile::open(&dir.path().join("rdisk5").to_string_lossy()).err().unwrap();
        assert_eq!(err.code(), "io");
    }

    #[test]
    fn regions_cover_the_disk() {
        let r = regions(10 * MIB, 512);
        assert_eq!(r.first(), Some(&0));
        assert_eq!(r.last(), Some(&(9 * MIB)));
        assert_eq!(r.len(), 10);
        assert!(regions(MIB - 1, 512).is_empty());
        assert!(regions(1 << 40, 4096).len() <= MAX_REGIONS as usize + 1);
    }

    // -----------------------------------------------------------------------------------------
    // Properties
    // -----------------------------------------------------------------------------------------

    use hegel::generators as gs;
    use hegel::{Generator, TestCase};
    use std::sync::Mutex;

    /// How a simulated disk treats addresses past its real capacity.
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Past {
        /// They wrap around onto the start.
        Wrap,
        /// Writes vanish and reads come back as zeros.
        Drop,
        /// Reads fail.
        Fail,
    }

    /// A disk that really holds `data.len()` bytes, copying a run at a time (byte by byte is
    /// too slow for tests that write many megabytes).
    struct Simulated {
        data: Vec<u8>,
        past: Past,
    }

    impl Simulated {
        /// The runs of `len` bytes from `offset`: (position in the buffer, address on the disk or
        /// `None` past it, length).
        fn runs(&self, offset: u64, len: usize) -> Vec<(usize, Option<usize>, usize)> {
            let real = self.data.len();
            let mut out = Vec::new();
            let mut pos = 0;
            while pos < len {
                let at = offset as usize + pos;
                let (addr, room) = match self.past {
                    Past::Wrap => (Some(at % real), real - at % real),
                    _ if at < real => (Some(at), real - at),
                    _ => (None, len - pos),
                };
                let n = room.min(len - pos);
                out.push((pos, addr, n));
                pos += n;
            }
            out
        }
    }

    impl RawDisk for Simulated {
        fn write_at(&mut self, offset: u64, buf: &[u8]) -> io::Result<()> {
            for (pos, addr, n) in self.runs(offset, buf.len()) {
                if let Some(a) = addr {
                    self.data[a..a + n].copy_from_slice(&buf[pos..pos + n]);
                }
            }
            Ok(())
        }
        fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
            for (pos, addr, n) in self.runs(offset, buf.len()) {
                match addr {
                    Some(a) => buf[pos..pos + n].copy_from_slice(&self.data[a..a + n]),
                    None if self.past == Past::Fail => return Err(io::ErrorKind::UnexpectedEof.into()),
                    None => buf[pos..pos + n].fill(0),
                }
            }
            Ok(())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Test regions start at 0 and end at the last whole region, are sector-aligned, never
    /// overlap (a region written over by the next would read back wrong on a real disk), and
    /// number one per MiB up to 256.
    #[hegel::test(test_cases = 500)]
    fn regions_are_aligned_and_apart(tc: TestCase) {
        let align = tc.draw(gs::sampled_from(vec![512u64, 4096]));
        let sectors = tc.draw(hegel::one_of!(
            gs::integers::<u64>().max_value(80 * MIB / 512),
            gs::integers::<u64>().max_value((4u64 << 40) / 512),
        ));
        let size = sectors * align;
        let r = regions(size, align);
        assert_eq!(r.len(), region_count(size, align as u32));
        if size < MIB {
            assert!(r.is_empty());
            return;
        }
        let last = if size < 2 * MIB { 0 } else { (size - MIB) / align * align };
        assert_eq!((r.first(), r.last()), (Some(&0), Some(&last)));
        assert!(r.iter().all(|o| o % align == 0 && o + MIB <= size));
        assert!(r.windows(2).all(|w| w[1] >= w[0] + MIB), "regions overlap on a {size}-byte disk: {r:?}");
        assert_eq!(r.len() as u64, (size / MIB).clamp(1, MAX_REGIONS));
    }

    /// A disk is fake exactly when it holds less than it claims: every region wholly within its
    /// real capacity reads back and every other one doesn't, so it works up to the first region
    /// past that. Data read back from the wrong place came from the place it wraps onto, and is
    /// reported as its schema says. Progress counts every region written and verified.
    #[hegel::test(test_cases = 25)]
    fn probes_find_the_real_capacity(tc: TestCase) {
        let align = tc.draw(gs::sampled_from(vec![512u64, 4096]));
        let claimed = tc.draw(gs::integers::<u64>().min_value(1).max_value(12 * MIB / align)) * align;
        let fake = tc.draw(gs::booleans());
        let real =
            if fake { tc.draw(gs::integers::<u64>().min_value(1).max_value(claimed / align)) * align } else { claimed };
        let past = tc.draw(gs::sampled_from(vec![Past::Wrap, Past::Drop, Past::Fail]).print_as_debug());
        let mut disk = Simulated { data: vec![0x5A; real as usize], past };

        let bytes = Mutex::new((0u64, 0u64));
        let count = |e: &Event| {
            let mut b = bytes.lock().unwrap();
            match e {
                Event::Bytes { phase: Phase::Write, bytes } => b.0 += bytes,
                Event::Bytes { phase: Phase::Verify, bytes } => b.1 += bytes,
                _ => {}
            }
        };
        let r = probe(&mut disk, claimed, align as u32, &Ctx::new(&count, Default::default())).unwrap();

        let offsets = regions(claimed, align);
        let bad: Vec<u64> = offsets.iter().copied().filter(|o| o + MIB > real).collect();
        assert_eq!((r.claimed, r.regions), (claimed, offsets.len()));
        assert_eq!(r.failed, bad.len(), "{real} of {claimed} ({past:?}): {r:?}");
        assert_eq!(r.works_up_to, bad.first().copied());
        assert_eq!(r.is_fake(), !bad.is_empty());
        assert!(bad.is_empty() || real < claimed);
        assert!(r.aliases.len() <= 8);
        for a in &r.aliases {
            assert_eq!(past, Past::Wrap, "only a wrapping disk returns another region's data");
            assert!(offsets.contains(&a.written_at) && a.read_at > a.written_at);
            assert_eq!((a.read_at - a.written_at) % real, 0, "{a:?}");
        }
        let total = offsets.len() as u64 * MIB;
        assert_eq!(*bytes.lock().unwrap(), (total, total));
        // Each alias is sent as the `[written_at, read_at]` pair its schema describes.
        let sent = serde_json::to_value(&r).unwrap();
        let pairs: Vec<[u64; 2]> = r.aliases.iter().map(|a| [a.written_at, a.read_at]).collect();
        assert_eq!(sent["aliases"], serde_json::json!(pairs));
        let schema = serde_json::to_value(schemars::schema_for!(Alias)).unwrap();
        let titles: Vec<&str> =
            schema["prefixItems"].as_array().unwrap().iter().map(|i| i["title"].as_str().unwrap()).collect();
        assert_eq!(
            (titles, &schema["minItems"], &schema["maxItems"]),
            (vec!["written_at", "read_at"], &serde_json::json!(2), &serde_json::json!(2))
        );
        // The first region was wiped and then written last, so it holds its own pattern.
        if !offsets.is_empty() && real >= MIB {
            assert_eq!(&disk.data[..8], MAGIC);
        }
    }

    /// A cancelled probe stops before writing a single pattern, with the boot region already
    /// wiped, and reports the cancellation.
    #[hegel::test(test_cases = 20)]
    fn a_cancelled_probe_stops_after_the_wipe(tc: TestCase) {
        let size = tc.draw(gs::integers::<u64>().min_value(1).max_value(4)) * MIB;
        let mut disk = Simulated { data: vec![0xFF; size as usize], past: Past::Wrap };
        let cancel = crate::progress::CancelToken::new();
        cancel.cancel();
        let err = probe(&mut disk, size, 512, &Ctx::new(&crate::progress::NoProgress, cancel)).unwrap_err();
        assert_eq!(err.code(), "cancelled");
        assert!(disk.data[..REGION].iter().all(|&b| b == 0) && disk.data[REGION..].iter().all(|&b| b == 0xFF));
    }

    /// A file standing in for a disk of whole sectors probes as real, reading back from the file
    /// it wrote; a missing one can't be opened. Not on Linux, where flushing drops the kernel's
    /// caches, which only root may.
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    #[hegel::test(test_cases = 10)]
    fn device_files_probe_as_real(tc: TestCase) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rdisk4");
        let size = tc.draw(gs::integers::<u64>().min_value(1).max_value(6 * MIB / 512)) * 512;
        std::fs::File::create(&path).unwrap().set_len(size).unwrap();
        let mut dev = DeviceFile::open(&path.to_string_lossy()).unwrap();
        let r = probe(&mut dev, size, 512, &Ctx::silent()).unwrap();
        assert!(!r.is_fake() && r.regions == region_count(size, 512), "{r:?}");
        assert_eq!(std::fs::metadata(&path).unwrap().len(), size, "nothing written past the end");
        let err = DeviceFile::open(&dir.path().join("rdisk5").to_string_lossy()).err().unwrap();
        assert_eq!(err.code(), "io");
    }
}
