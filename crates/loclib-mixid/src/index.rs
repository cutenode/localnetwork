//! Tracks' fingerprints, and the hash index built from them.
//!
//! Fingerprinting decodes every track once, which takes minutes for a few thousand tracks, so a
//! program keeps the prints ([`TrackPrint::encode`] is a compact format for that) and builds the
//! index from them in seconds.

use std::path::Path;

use loclib_progress::Ctx;

use crate::Result;
use crate::audio;
use crate::fingerprint::{self, Analyzer, HASH_BITS, LIBRARY, Peak};

/// Landmarks per library peak.
pub(crate) const LIBRARY_FANOUT: usize = 3;

/// A track's fingerprint.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackPrint {
    pub duration_secs: f32,
    pub peaks: Vec<Peak>,
}

const MAGIC: &[u8; 4] = b"LLFP";
/// The version of [`TrackPrint::encode`]'s format. Bump when anything that changes peaks changes.
/// 2: the resampler keeps its position exactly, so outputs on a tie between kernel phases (and
/// the peaks they make) no longer depend on how decoding chunked the audio.
pub const VERSION: u16 = 2;

impl TrackPrint {
    /// `LLFP`, version, duration, peak count, then per peak a varint frame delta and the bin.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.peaks.len() * 2);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&self.duration_secs.to_le_bytes());
        out.extend_from_slice(&(self.peaks.len() as u32).to_le_bytes());
        let mut last = 0u32;
        for p in &self.peaks {
            let mut d = p.t - last;
            last = p.t;
            loop {
                let byte = (d & 0x7f) as u8;
                d >>= 7;
                if d == 0 {
                    out.push(byte);
                    break;
                }
                out.push(byte | 0x80);
            }
            out.push(p.f);
        }
        out
    }

    /// [`TrackPrint::encode`]'s bytes read back: `None` for anything else, or another
    /// [`VERSION`].
    pub fn decode(bytes: &[u8]) -> Option<TrackPrint> {
        if bytes.get(..4)? != MAGIC || u16::from_le_bytes(bytes.get(4..6)?.try_into().ok()?) != VERSION {
            return None;
        }
        let duration_secs = f32::from_le_bytes(bytes.get(6..10)?.try_into().ok()?);
        let n = u32::from_le_bytes(bytes.get(10..14)?.try_into().ok()?) as usize;
        let mut peaks = Vec::with_capacity(n.min(bytes.len()));
        let mut i = 14;
        let mut t = 0u32;
        for _ in 0..n {
            let mut d = 0u32;
            let mut shift = 0;
            loop {
                let b = *bytes.get(i)?;
                i += 1;
                d |= ((b & 0x7f) as u32).checked_shl(shift)?;
                if b & 0x80 == 0 {
                    break;
                }
                shift += 7;
            }
            t = t.checked_add(d)?;
            peaks.push(Peak { t, f: *bytes.get(i)? });
            i += 1;
        }
        (i == bytes.len()).then_some(TrackPrint { duration_secs, peaks })
    }
}

/// Decode an audio file and fingerprint it.
pub fn fingerprint_file(path: &Path, ctx: &Ctx<'_>) -> Result<TrackPrint> {
    let mut a = Analyzer::new(LIBRARY, false);
    let format = audio::decode(path, fingerprint::RATE, ctx, &mut |s| a.push(s))?;
    let (peaks, _) = a.finish();
    Ok(TrackPrint { duration_secs: format.duration_secs as f32, peaks })
}

/// Inverted index from landmark hash to (track, frame) occurrences.
pub struct LibraryIndex {
    /// Library track id of each indexed track.
    pub track_ids: Vec<u32>,
    pub durations: Vec<f32>,
    offsets: Vec<u32>,
    /// `(slot << 32) | frame`, grouped by hash.
    entries: Vec<u64>,
}

const INDEX_MAGIC: &[u8; 4] = b"LLIX";
/// The version of [`LibraryIndex::encode`]'s format. Bump when it changes, or when
/// [`LibraryIndex::build`] makes another index of the same prints (landmarks, the cap on common
/// hashes): a kept index is only good while both match.
pub const INDEX_VERSION: u16 = 1;

impl LibraryIndex {
    /// `LLIX`, [`VERSION`], [`INDEX_VERSION`], then each array's length and values, little-endian.
    pub fn encode(&self) -> Vec<u8> {
        let (ids, durations, offsets, entries) =
            (self.track_ids.len(), self.durations.len(), self.offsets.len(), self.entries.len());
        let mut out = Vec::with_capacity(40 + ids * 4 + durations * 4 + offsets * 4 + entries * 8);
        out.extend_from_slice(INDEX_MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&INDEX_VERSION.to_le_bytes());
        out.extend((ids as u64).to_le_bytes().into_iter().chain(self.track_ids.iter().flat_map(|v| v.to_le_bytes())));
        out.extend(
            (durations as u64).to_le_bytes().into_iter().chain(self.durations.iter().flat_map(|v| v.to_le_bytes())),
        );
        out.extend((offsets as u64).to_le_bytes().into_iter().chain(self.offsets.iter().flat_map(|v| v.to_le_bytes())));
        out.extend((entries as u64).to_le_bytes().into_iter().chain(self.entries.iter().flat_map(|v| v.to_le_bytes())));
        out
    }

    /// [`LibraryIndex::encode`]'s bytes read back: `None` for anything else, another [`VERSION`]
    /// or [`INDEX_VERSION`], or an index whose buckets don't fit its entries.
    pub fn decode(bytes: &[u8]) -> Option<LibraryIndex> {
        let (head, mut rest) = bytes.split_at_checked(8)?;
        if &head[..4] != INDEX_MAGIC || head[4..6] != VERSION.to_le_bytes() || head[6..8] != INDEX_VERSION.to_le_bytes()
        {
            return None;
        }
        fn array<const N: usize, T>(rest: &mut &[u8], from: fn([u8; N]) -> T) -> Option<Vec<T>> {
            let (len, tail) = rest.split_at_checked(8)?;
            let len = usize::try_from(u64::from_le_bytes(len.try_into().ok()?)).ok()?;
            let (values, tail) = tail.split_at_checked(len.checked_mul(N)?)?;
            *rest = tail;
            Some(values.chunks_exact(N).map(|c| from(c.try_into().expect("N bytes"))).collect())
        }
        let track_ids = array(&mut rest, u32::from_le_bytes)?;
        let durations = array(&mut rest, f32::from_le_bytes)?;
        let offsets = array(&mut rest, u32::from_le_bytes)?;
        let entries = array(&mut rest, u64::from_le_bytes)?;
        let fits = rest.is_empty()
            && track_ids.len() == durations.len()
            && offsets.len() == (1usize << HASH_BITS) + 1
            && offsets.windows(2).all(|w| w[0] <= w[1])
            && offsets.last().is_some_and(|&end| end as usize == entries.len())
            && entries.iter().all(|e| ((e >> 32) as usize) < track_ids.len());
        fits.then_some(LibraryIndex { track_ids, durations, offsets, entries })
    }

    pub fn bucket(&self, hash: u32) -> &[u64] {
        let h = hash as usize;
        &self.entries[self.offsets[h] as usize..self.offsets[h + 1] as usize]
    }

    pub fn hashes(&self) -> usize {
        self.entries.len()
    }

    /// Build from fingerprints. Hashes so common they say nothing (a bare kick pattern shared by
    /// thousands of tracks) are dropped. Each track's landmarks are worked out twice, to count
    /// and then to fill: keeping them all between the two would double the memory it takes.
    pub fn build(prints: Vec<(u32, TrackPrint)>) -> LibraryIndex {
        let buckets = 1usize << HASH_BITS;
        let mut counts = vec![0u32; buckets + 1];
        for (_, p) in &prints {
            for lm in fingerprint::landmarks(&p.peaks, LIBRARY_FANOUT) {
                counts[lm.hash() as usize] += 1;
            }
        }
        let total: u64 = counts.iter().map(|&c| c as u64).sum();
        let cap = (total as f64 / buckets as f64 * 64.0).max(512.0) as u32;
        let mut offsets = vec![0u32; buckets + 1];
        let mut sum = 0u32;
        for h in 0..buckets {
            offsets[h] = sum;
            if counts[h] <= cap {
                sum += counts[h];
            }
        }
        offsets[buckets] = sum;
        let mut fill = offsets.clone();
        let mut entries = vec![0u64; sum as usize];
        for (slot, (_, p)) in prints.iter().enumerate() {
            for lm in fingerprint::landmarks(&p.peaks, LIBRARY_FANOUT) {
                let h = lm.hash() as usize;
                if counts[h] <= cap {
                    entries[fill[h] as usize] = ((slot as u64) << 32) | lm.t as u64;
                    fill[h] += 1;
                }
            }
        }
        let (track_ids, durations) = prints.iter().map(|(id, p)| (*id, p.duration_secs)).unzip();
        LibraryIndex { track_ids, durations, offsets, entries }
    }
}

#[cfg(test)]
pub(crate) mod tests;
