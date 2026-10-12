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
    let steps = tc
        .draw(gs::vecs(gs::tuples!(gs::integers::<u32>().max_value(max_gap), gs::integers::<u8>())).max_size(max_len));
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

/// A print's frames as its bytes spell them: the header, then each peak's varint frame delta
/// and bin.
fn spelled(deltas: &[u32]) -> Vec<u8> {
    let mut out =
        [b"LLFP".as_slice(), &VERSION.to_le_bytes(), &2.0f32.to_le_bytes(), &(deltas.len() as u32).to_le_bytes()]
            .concat();
    for &d in deltas {
        let mut d = d;
        while d >= 0x80 {
            out.push((d & 0x7f) as u8 | 0x80);
            d >>= 7;
        }
        out.extend_from_slice(&[d as u8, 9]);
    }
    out
}

/// Frames are counted in 32 bits: a print whose deltas add up past that doesn't read, and one
/// that reaches the last frame does.
#[hegel::test(test_cases = 200)]
fn frames_past_32_bits_dont_read(tc: TestCase) {
    let first = tc.draw(gs::integers::<u32>());
    let room = u32::MAX - first;
    let over = tc.draw(gs::integers::<u32>().min_value(room.saturating_add(1)));
    if room < u32::MAX {
        assert_eq!(TrackPrint::decode(&spelled(&[first, over])), None, "{first} then {over}");
    }
    let last = TrackPrint::decode(&spelled(&[first, room])).unwrap();
    assert_eq!(last.peaks.iter().map(|p| p.t).collect::<Vec<_>>(), [first, u32::MAX]);
}

/// A hash so common it says nothing (a bare pattern repeated over and over, more than 512
/// times in a small library) is left out of the index; every other landmark is in it.
#[hegel::test(test_cases = 30)]
fn hashes_too_common_to_tell_tracks_apart_are_left_out(tc: TestCase) {
    let step = tc.draw(gs::integers::<u32>().min_value(2).max_value(20));
    let f = tc.draw(gs::integers::<u8>().min_value(10).max_value(200));
    // The same bin every `step` frames: each landmark is one of a few hashes, over and over.
    let repeats = tc.draw(hegel::one_of!(gs::just(600), gs::integers::<u32>().min_value(1).max_value(700)));
    let mut peaks: Vec<Peak> = (0..repeats).map(|i| Peak { t: i * step, f }).collect();
    let other = sorted_peaks(&tc, 6, 60);
    peaks.extend(other.iter().map(|p| Peak { t: p.t + repeats * step + 100, ..*p }));
    let index = LibraryIndex::build(vec![(1, TrackPrint { duration_secs: 60.0, peaks: peaks.clone() })]);
    let mut counts: rustc_hash::FxHashMap<u32, usize> = rustc_hash::FxHashMap::default();
    let landmarks = fingerprint::landmarks(&peaks, LIBRARY_FANOUT);
    for lm in &landmarks {
        *counts.entry(lm.hash()).or_default() += 1;
    }
    for lm in &landmarks {
        let kept = index.bucket(lm.hash()).contains(&u64::from(lm.t));
        assert_eq!(kept, counts[&lm.hash()] <= 512, "{lm:?}, one of {}", counts[&lm.hash()]);
    }
    assert_eq!(index.hashes(), counts.values().filter(|&&n| n <= 512).sum::<usize>());
}
