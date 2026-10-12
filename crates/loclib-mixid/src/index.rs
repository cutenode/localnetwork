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

impl LibraryIndex {
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
pub(crate) mod tests {
    use super::*;

    #[test]
    fn prints_round_trip() {
        let p = TrackPrint {
            duration_secs: 312.5,
            peaks: vec![Peak { t: 0, f: 3 }, Peak { t: 0, f: 200 }, Peak { t: 130, f: 7 }, Peak { t: 70_000, f: 255 }],
        };
        let bytes = p.encode();
        assert_eq!(TrackPrint::decode(&bytes), Some(p.clone()));
        assert_eq!(TrackPrint::decode(&bytes[..bytes.len() - 1]), None);
    }

    /// Prints kept on disk at this version still read: the format is these bytes exactly. Those
    /// of another version, older or newer, don't.
    #[test]
    fn the_format_is_stable() {
        let p = TrackPrint { duration_secs: 1.5, peaks: vec![Peak { t: 2, f: 9 }, Peak { t: 202, f: 40 }] };
        let bytes = [b"LLFP".as_slice(), &[2, 0], &1.5f32.to_le_bytes(), &[2, 0, 0, 0], &[2, 9, 200, 1, 40]].concat();
        assert_eq!(p.encode(), bytes);
        assert_eq!(TrackPrint::decode(&bytes), Some(p));
        for other in [1u8, 3] {
            let other = [&bytes[..4], &[other, 0], &bytes[6..]].concat();
            assert_eq!(TrackPrint::decode(&other), None);
        }
    }

    #[test]
    fn index_finds_hashes() {
        let peaks: Vec<Peak> = (0..50).map(|i| Peak { t: i * 5, f: (10 + (i * 37) % 200) as u8 }).collect();
        let idx = LibraryIndex::build(vec![(42, TrackPrint { duration_secs: 6.0, peaks: peaks.clone() })]);
        let lm = fingerprint::landmarks(&peaks, LIBRARY_FANOUT)[5];
        let hits = idx.bucket(lm.hash());
        assert!(hits.iter().any(|&e| (e >> 32) == 0 && (e as u32) == lm.t));
        assert_eq!(idx.track_ids, vec![42]);
    }

    use hegel::TestCase;
    use hegel::generators as gs;

    /// Peaks in (frame, bin) order, as [`Analyzer::finish`] gives them, up to `max_gap` frames
    /// apart.
    #[hegel::test_helper]
    pub(crate) fn sorted_peaks(tc: &TestCase, max_gap: u32, max_len: usize) -> Vec<Peak> {
        let steps = tc.draw(
            gs::vecs(gs::tuples!(gs::integers::<u32>().max_value(max_gap), gs::integers::<u8>())).max_size(max_len),
        );
        let mut t = 0u32;
        let mut peaks: Vec<Peak> = steps
            .into_iter()
            .map(|(d, f)| {
                t = t.saturating_add(d);
                Peak { t, f }
            })
            .collect();
        peaks.sort_unstable();
        peaks
    }

    /// Any print reads back as it was, and a print cut short or with a byte to spare doesn't
    /// read at all.
    #[hegel::test(test_cases = 300)]
    fn any_print_round_trips(tc: TestCase) {
        let peaks = sorted_peaks(&tc, 1 << 20, 40);
        let duration_secs = tc.draw(gs::floats::<f32>().allow_nan(false));
        let p = TrackPrint { duration_secs, peaks };
        let bytes = p.encode();
        assert_eq!(TrackPrint::decode(&bytes), Some(p));
        let cut = tc.draw(gs::integers::<usize>().max_value(bytes.len() - 1));
        assert_eq!(TrackPrint::decode(&bytes[..cut]), None);
        let extra = tc.draw(gs::integers::<u8>());
        assert_eq!(TrackPrint::decode(&[&bytes[..], &[extra]].concat()), None);
    }

    /// Bytes from anywhere (a damaged cache file) never panic the decoder, and whatever it reads
    /// from them is a print in frame order that writes and reads back the same.
    #[hegel::test(test_cases = 300)]
    fn any_bytes_decode_safely(tc: TestCase) {
        let header = tc.draw(gs::booleans());
        let body = tc.draw(gs::binary().max_size(64));
        let n = tc.draw(gs::integers::<u32>().max_value(40));
        let bytes = if header {
            [b"LLFP".as_slice(), &VERSION.to_le_bytes(), &1.0f32.to_le_bytes(), &n.to_le_bytes(), &body].concat()
        } else {
            body
        };
        if let Some(p) = TrackPrint::decode(&bytes) {
            assert!(p.peaks.is_sorted_by_key(|p| p.t), "{p:?}");
            assert_eq!(TrackPrint::decode(&p.encode()).map(|q| q.encode()), Some(p.encode()));
        }
    }

    /// Every landmark of every track is in the bucket of its hash, as (slot, frame), and the
    /// index holds nothing else: a small library has no hash common enough to drop.
    #[hegel::test(test_cases = 25)]
    fn the_index_holds_every_landmark(tc: TestCase) {
        let n = tc.draw(gs::integers::<usize>().max_value(4));
        let prints: Vec<(u32, TrackPrint)> = (0..n)
            .map(|i| (i as u32 * 10 + 1, TrackPrint { duration_secs: 30.0, peaks: sorted_peaks(&tc, 6, 60) }))
            .collect();
        let index = LibraryIndex::build(prints.clone());
        let mut total = 0;
        for (slot, (_, p)) in prints.iter().enumerate() {
            for lm in fingerprint::landmarks(&p.peaks, LIBRARY_FANOUT) {
                let entry = ((slot as u64) << 32) | lm.t as u64;
                assert!(index.bucket(lm.hash()).contains(&entry), "{lm:?} of slot {slot}");
                total += 1;
            }
        }
        assert_eq!(index.hashes(), total);
        assert_eq!(index.track_ids, prints.iter().map(|(id, _)| *id).collect::<Vec<_>>());
        assert_eq!(index.durations, vec![30.0; n]);
    }
}
