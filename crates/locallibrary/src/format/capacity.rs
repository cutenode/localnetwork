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
mod tests;
