#![doc = include_str!("../README.md")]

use std::io;
use std::path::Path;

use loclib_progress::{Ctx, Event, Phase};
use serde::{Serialize, Serializer};

mod error;

pub use error::{Error, Result};

const MAGIC: &[u8; 8] = b"LLCAPCHK";
/// The size of each test region, and of the wipe at the start of the disk.
pub const REGION: usize = 1 << 20;
const MAX_REGIONS: u64 = 256;

/// Raw access to a whole disk, which [`probe`] overwrites. [`DeviceFile`] is a real device; an
/// implementation of your own stands in for one (in memory, in a test).
pub trait RawDisk {
    fn write_at(&mut self, offset: u64, buf: &[u8]) -> io::Result<()>;
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()>;
    /// Make the writes reach the device, and make the next reads come from it.
    fn flush(&mut self) -> io::Result<()>;
}

/// What [`probe`] found.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ProbeReport {
    /// The size the disk reports.
    pub claimed: u64,
    /// How many regions were tested.
    pub regions: usize,
    /// How many of them didn't read back what was written there.
    pub failed: usize,
    /// Every region below this offset held its data (`None`: all did).
    pub works_up_to: Option<u64>,
    /// Data that came back at the wrong place, the signature of a drive that wraps writes
    /// around. At most 8.
    pub aliases: Vec<Alias>,
}

/// Where a region was written and where its data was read back: `[written_at, read_at]`,
/// byte offsets. It serialises as that array, and its schema names each element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alias {
    pub written_at: u64,
    pub read_at: u64,
}

impl Serialize for Alias {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (self.written_at, self.read_at).serialize(serializer)
    }
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for Alias {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Alias".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::Alias").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut offset = |title: &str| {
            let mut item = generator.subschema_for::<u64>();
            item.insert("title".into(), title.into());
            item
        };
        let items = [offset("written_at"), offset("read_at")];
        schemars::json_schema!({ "type": "array", "prefixItems": items, "minItems": 2, "maxItems": 2 })
    }
}

impl ProbeReport {
    /// Whether any region failed to keep its data.
    pub fn is_fake(&self) -> bool {
        self.failed > 0
    }
}

/// The pattern for the region at `offset`: the magic, the offset, the nonce, then pseudo-random
/// bytes, so no two regions or runs look alike. Also fresh data for timing writes that a
/// controller can't compress or skip.
///
/// # Panics
///
/// If `buf` is shorter than 24 bytes.
pub fn pattern(offset: u64, nonce: u64, buf: &mut [u8]) {
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
///
/// This destroys what `disk` holds: the first region and every test region are overwritten, and
/// a disk with a file system on it no longer mounts afterwards. Nothing at or past `size` is
/// touched, and a disk smaller than one region isn't written at all.
pub fn probe(disk: &mut dyn RawDisk, size: u64, align: u32, ctx: &Ctx<'_>) -> Result<ProbeReport> {
    let offsets = regions(size, u64::from(align.max(512)));
    let mut nonce_bytes = [0u8; 8];
    getrandom::fill(&mut nonce_bytes).map_err(Error::Random)?;
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
    /// Open the whole-disk device at `path` (macOS: the unbuffered `/dev/rdiskN`; Linux:
    /// `/dev/sdX`) for reading and writing, so that [`probe`] can overwrite it.
    ///
    /// Probing what this returns destroys everything on the disk. Unmount all its volumes first,
    /// and make sure `path` is the disk you mean to erase: nothing here checks. A path that
    /// doesn't exist is an error rather than a new file.
    pub fn open_to_overwrite(path: impl AsRef<Path>) -> Result<DeviceFile> {
        let path = path.as_ref();
        let file = std::fs::OpenOptions::new().read(true).write(true).open(path).map_err(|e| match e.kind() {
            io::ErrorKind::PermissionDenied => Error::Permission { path: path.into() },
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
