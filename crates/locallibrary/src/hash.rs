//! BLAKE3 content hashes, used for blob addressing, change detection and fingerprints.

use std::fmt;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::IoContext;
use crate::progress::{Ctx, Event, Phase};
use crate::{Error, Result};

const BUF_SIZE: usize = 1 << 20;
/// Buffers read ahead of the writes in [`copy_reading_ahead`].
const READ_AHEAD: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hash([u8; 32]);

impl Hash {
    pub fn of(bytes: &[u8]) -> Self {
        Hash(*blake3::hash(bytes).as_bytes())
    }

    pub fn to_hex(&self) -> String {
        blake3::Hash::from_bytes(self.0).to_hex().to_string()
    }

    /// First 12 hex characters, for display.
    pub fn short(&self) -> String {
        self.to_hex()[..12].to_string()
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl From<blake3::Hash> for Hash {
    fn from(h: blake3::Hash) -> Self {
        Hash(*h.as_bytes())
    }
}

impl FromStr for Hash {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        blake3::Hash::from_hex(s).map(Hash::from).map_err(|_| Error::invalid(format!("invalid hash {s:?}")))
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash({})", self.short())
    }
}

impl Serialize for Hash {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// A hash is 64 lowercase hex characters.
impl schemars::JsonSchema for Hash {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Hash".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        "locallibrary::hash::Hash".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "A BLAKE3 hash, as 64 lowercase hex characters.",
            "pattern": "^[0-9a-f]{64}$",
        })
    }
}

/// Hash a file, reporting bytes to `ctx` under `phase` and honouring cancellation.
pub fn hash_file(path: &Path, ctx: &Ctx<'_>, phase: Phase) -> Result<Hash> {
    let mut file = File::open(path).at(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; BUF_SIZE];
    loop {
        ctx.check()?;
        let n = file.read(&mut buf).at(path)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        ctx.emit(Event::Bytes { phase, bytes: n as u64 });
    }
    Ok(hasher.finalize().into())
}

/// Stream `src` into `dst` while hashing it. Returns the hash and number of bytes copied. A file
/// bigger than a buffer is read and hashed on a thread of its own, so a drive that's busy writing
/// never waits for the next buffer.
pub fn copy_hashing(
    src: &Path,
    dst: &mut impl Write,
    dst_path: &Path,
    ctx: &Ctx<'_>,
    phase: Phase,
) -> Result<(Hash, u64)> {
    let mut file = File::open(src).at(src)?;
    if file.metadata().at(src)?.len() > BUF_SIZE as u64 {
        return copy_reading_ahead(file, src, dst, dst_path, ctx, phase);
    }
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; BUF_SIZE];
    let mut total = 0u64;
    loop {
        ctx.check()?;
        let n = file.read(&mut buf).at(src)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        dst.write_all(&buf[..n]).at(dst_path)?;
        total += n as u64;
        ctx.emit(Event::Bytes { phase, bytes: n as u64 });
    }
    Ok((hasher.finalize().into(), total))
}

/// [`copy_hashing`] with `file` read and hashed ahead of the writes, at most [`READ_AHEAD`]
/// buffers. A failed write or a cancel drops the channel, which stops the reader.
fn copy_reading_ahead(
    mut file: File,
    src: &Path,
    dst: &mut impl Write,
    dst_path: &Path,
    ctx: &Ctx<'_>,
    phase: Phase,
) -> Result<(Hash, u64)> {
    let (full, filled) = std::sync::mpsc::sync_channel(READ_AHEAD);
    let (empty, spare) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::scope(|s| {
        let reader = s.spawn(move || {
            let mut hasher = blake3::Hasher::new();
            let mut buf = vec![0u8; BUF_SIZE];
            loop {
                match file.read(&mut buf) {
                    Ok(0) => return Some(hasher.finalize()),
                    Ok(n) => {
                        hasher.update(&buf[..n]);
                        let next = spare.try_recv().unwrap_or_else(|_| vec![0u8; BUF_SIZE]);
                        full.send(Ok((std::mem::replace(&mut buf, next), n))).ok()?;
                    }
                    Err(e) => {
                        let _ = full.send(Err(e));
                        return None;
                    }
                }
            }
        });
        let mut total = 0u64;
        for read in filled {
            let (buf, n): (Vec<u8>, usize) = read.at(src)?;
            ctx.check()?;
            dst.write_all(&buf[..n]).at(dst_path)?;
            total += n as u64;
            ctx.emit(Event::Bytes { phase, bytes: n as u64 });
            let _ = empty.send(buf);
        }
        let hash = reader.join().expect("the reader doesn't panic").expect("it ended by finishing the file");
        Ok((hash.into(), total))
    })
}

#[cfg(test)]
mod tests;
