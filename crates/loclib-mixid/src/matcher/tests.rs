use super::*;
use crate::audio::Resampler;
use crate::fingerprint::{Analyzer, LIBRARY, QUERY, RATE};
use crate::index::TrackPrint;
use crate::test_util::synth;

fn peaks(x: &[f32], density: fingerprint::Density) -> (Vec<Peak>, u32) {
    let mut a = Analyzer::new(density, false);
    a.push(x);
    let frames = a.frames();
    (a.finish().0, frames)
}

/// `x` played `speed` times as fast, pitch and all (a turntable, or a CDJ without key lock).
fn faster(x: &[f32], speed: f64) -> Vec<f32> {
    let mut r = Resampler::new((RATE as f64 * speed).round() as u32, RATE);
    let mut out = Vec::new();
    r.push(x, &mut |s| out.extend_from_slice(s));
    r.finish(&mut |s| out.extend_from_slice(s));
    out
}

/// A chain of track 1 from `from` to `to` seconds into the mix, played at `speed` from `at`
/// seconds into the track: a detection per slice, its anchors along that line.
fn chain_at(variants: &[Variant], from: f64, to: f64, at: f64, speed: f64) -> Chain {
    let off = |v: &Variant| (v.ratio - speed).abs();
    let variant = (0..variants.len()).min_by(|&a, &b| off(&variants[a]).total_cmp(&off(&variants[b]))).unwrap();
    let (start, end) = ((from * FRAMES_PER_SEC) as u32, (to * FRAMES_PER_SEC) as u32);
    let detections = (start / SLICE..end / SLICE)
        .map(|slice| {
            let pairs = (slice * SLICE..(slice + 1) * SLICE)
                .step_by(8)
                .filter(|t| (start..end).contains(t))
                .map(|t| (t, (at * FRAMES_PER_SEC + speed * (t - start) as f64) as u32))
                .collect();
            Detection { slot: 1, slice, variant: variant as u16, votes: 20, stretch_votes: 0, pitch_votes: 20, pairs }
        })
        .collect();
    Chain { slot: 1, detections }
}

/// [`chain_at`], of track `slot`, with each detection's votes split `stretch`/`pitch`
/// between key-locked and pitched speeds.
fn chain_of(slot: u32, chain: Chain, stretch: u32, pitch: u32) -> Chain {
    let detections = chain
        .detections
        .into_iter()
        .map(|d| Detection { slot, stretch_votes: stretch, pitch_votes: pitch, ..d })
        .collect();
    Chain { slot, detections }
}

/// A breakdown the mix drops out over splits a track into two chains on one line: one play.
/// A track heard again minutes later is another play; a weaker match of its own material
/// elsewhere in the track, alongside the real one (a repeated chorus), is no play at all.
#[test]
fn chains_become_plays() {
    let variants = fingerprint::variants(0.08);
    let at = |from, to, track| chain_at(&variants, from, to, track, 1.0);
    // (plays, and each one's lines and jumps)
    let shapes = |found: &[Found]| (found.len(), found.iter().map(|f| (f.fits.len(), f.jumps)).collect::<Vec<_>>());
    let quiet = plays(vec![at(0.0, 30.0, 10.0), at(36.0, 70.0, 46.0)], &variants);
    assert_eq!(shapes(&quiet), (1, vec![(1, 0)]), "{quiet:#?}");
    assert!(quiet[0].start < 1.0 && quiet[0].end > 66.0, "{quiet:#?}");
    let again = plays(vec![at(0.0, 40.0, 10.0), at(100.0, 140.0, 10.0)], &variants);
    assert_eq!(shapes(&again), (2, vec![(1, 0), (1, 0)]), "{again:#?}");
    let chorus = plays(vec![at(0.0, 60.0, 10.0), at(20.0, 40.0, 90.0)], &variants);
    assert_eq!(shapes(&chorus), (1, vec![(1, 0)]), "{chorus:#?}");
    // A slice or two is what a similar groove at the same tempo gives.
    assert!(plays(vec![at(0.0, 2.0, 10.0)], &variants).is_empty());
}

/// Key lock is told by which speeds voted most: time-stretched ones (on), pitched ones
/// (off), and neither by much, or barely off 1.0, can't be told.
#[test]
fn key_lock_is_told_by_the_votes() {
    let variants = fingerprint::variants(0.08);
    let lock = |speed, stretch, pitch| {
        let chain = chain_of(1, chain_at(&variants, 0.0, 40.0, 10.0, speed), stretch, pitch);
        plays(vec![chain], &variants)[0].key_lock
    };
    assert_eq!(lock(1.04, 20, 0), Some(true));
    assert_eq!(lock(1.04, 0, 20), Some(false));
    assert_eq!(lock(1.04, 20, 19), None);
    assert_eq!(lock(1.0, 20, 0), None);
}

/// Of plays of different tracks over one stretch: a duplicate file is named as the same
/// audio, a track whose material is heard inside another's as an alternate, a barely-there
/// one not at all, and one heard only underneath the tracks around it is dropped.
#[test]
fn overlapping_plays_are_resolved_to_the_strongest() {
    let variants = fingerprint::variants(0.08);
    let found = |slot, from, to, votes: u32| {
        let mut c = chain_of(slot, chain_at(&variants, from, to, 10.0, 1.0), 0, 0);
        c.detections.iter_mut().for_each(|d| d.votes = votes);
        plays(vec![c], &variants).remove(0)
    };
    let resolved = resolve(vec![
        found(1, 0.0, 60.0, 20),
        found(2, 0.0, 60.0, 20),
        found(3, 10.0, 40.0, 10),
        found(4, 0.0, 60.0, 4),
        found(5, 60.0, 120.0, 20),
        found(6, 30.0, 90.0, 4),
    ]);
    let kept: Vec<(u32, &[(u32, bool)])> = resolved.iter().map(|f| (f.slot, f.alternates.as_slice())).collect();
    assert_eq!(kept, [(1, &[(2, true), (3, false)][..]), (5, &[][..])]);
}

#[test]
fn nothing_is_found_outside_the_speeds_that_can_be_searched() {
    let index = LibraryIndex::build(Vec::new());
    let err = find(&index, &[], 0, 0.3, None, &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("tempo range 0.3"), "{err}");
    assert_eq!(coverage(&[], 0.0), 0.0);
    assert_eq!(gaps(&[], 90.0, 30.0), [(0.0, 90.0)]);
}

/// A tempo range that isn't a number from 0 to 0.25 (NaN, infinite, negative) is refused
/// like one that's too wide, rather than searched.
#[test]
fn tempo_ranges_that_arent_numbers_are_refused() {
    let index = LibraryIndex::build(Vec::new());
    for range in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01, 0.2501] {
        let err = find(&index, &[], 0, range, None, &Ctx::silent()).unwrap_err();
        assert!(matches!(&err, Error::Invalid(m) if m.contains("between 0 and 0.25")), "{range}: {err}");
    }
    for range in [0.0, 0.08, 0.25] {
        assert!(find(&index, &[], 0, range, None, &Ctx::silent()).is_ok(), "{range}");
    }
}

#[test]
fn a_pitch_ride_carries_on_where_a_cue_jump_jumps() {
    let variants = fingerprint::variants(0.08);
    // 40 s at +0%, then the pitch fader up to +6%: the track carries on from where it was.
    let ride =
        plays(vec![chain_at(&variants, 0.0, 40.0, 10.0, 1.0), chain_at(&variants, 40.0, 80.0, 50.0, 1.06)], &variants);
    assert_eq!(ride.iter().map(|p| (p.fits.len(), p.jumps)).collect::<Vec<_>>(), [(2, 0)], "{ride:#?}");
    // At one speed, but 20 s further on in the track after 40 s: a cue jump.
    let skip =
        plays(vec![chain_at(&variants, 0.0, 40.0, 10.0, 1.0), chain_at(&variants, 40.0, 80.0, 70.0, 1.0)], &variants);
    assert_eq!(skip.iter().map(|p| (p.fits.len(), p.jumps)).collect::<Vec<_>>(), [(2, 1)], "{skip:#?}");
}

/// `x` at the fingerprint rate as a library print.
fn print(x: &[f32]) -> TrackPrint {
    TrackPrint { duration_secs: x.len() as f32 / RATE as f32, peaks: peaks(x, LIBRARY).0 }
}

/// A copy shifted by an encoder's delay (26 ms) is the whole of the original, and its offset
/// is measured to within a few milliseconds. An edit with 16 s cut out of its middle shares
/// only part of it at any one offset, a copy 0.5% fast drifts, and another track shares
/// nothing.
#[test]
fn copies_line_up_and_edits_dont() {
    let secs = |s: f64| (s * RATE as f64) as usize;
    let x = synth(120.0, 7919);
    let original = print(&x);
    let delayed: Vec<f32> = std::iter::repeat_n(0.0, secs(0.026)).chain(x.iter().map(|s| s * 0.7)).collect();
    let copy = align(&original, &print(&delayed)).unwrap();
    assert!((copy.offset - 0.026).abs() < 0.005 && copy.is_precise(), "{copy:?}");
    assert!(copy.coverage.0 > 0.95 && copy.coverage.1 > 0.95, "{copy:?}");
    let back = align(&print(&delayed), &original).unwrap();
    assert!((back.offset + copy.offset).abs() < 0.005, "{back:?}");

    let edit = print(&[&x[..secs(50.0)], &x[secs(66.0)..]].concat());
    let cut = align(&original, &edit).unwrap();
    assert!(cut.coverage.0 < 0.7 && cut.coverage.1 < 0.8, "{cut:?}");

    let fast = align(&original, &print(&faster(&x, 1.005))).unwrap();
    assert!(!fast.is_precise() && fast.coverage.0 < 0.8, "{fast:?}");
    assert!(align(&original, &print(&synth(120.0, 3 * 7919))).is_none_or(|a| a.coverage.0 < 0.1));
}

/// A copy shifted by part of a frame lines up at that fraction: the mean doesn't lean towards
/// a whole frame. Over landmark pairs it did, by up to a quarter of one (5 ms here), since a
/// pair matches only when both its peaks moved into the next frame or neither did.
#[test]
fn a_fraction_of_a_frame_is_measured() {
    let x = synth(30.0, 5);
    let original = print(&x);
    for delay in [40, 71, 80, 180] {
        let copy: Vec<f32> = std::iter::repeat_n(0.0, delay).chain(x.iter().copied()).collect();
        let truth = delay as f64 / RATE as f64;
        let a = align(&original, &print(&copy)).unwrap();
        assert!((a.offset - truth).abs() < 0.0025 && a.is_precise(), "{truth}: {a:?}");
    }
}

/// A copy of a short track late by any part of a frame has its offset measured. At half a
/// frame it keeps under half its landmark pairs, since each of a pair's peaks moves into the next
/// frame or not on its own, so counting pairs called 20 s copies unmeasured.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
#[hegel::explicit_test_case(seed = 1u32, delay = 99usize)]
fn a_short_copy_late_by_part_of_a_frame_is_measured(tc: TestCase) {
    let seed = tc.draw(gs::integers::<u32>().min_value(1).max_value(10_000));
    // Up to a frame: 256 samples.
    let delay = tc.draw(gs::integers::<usize>().max_value(256));
    let x = synth(20.0, seed);
    let copy: Vec<f32> = std::iter::repeat_n(0.0, delay).chain(x.iter().copied()).collect();
    let truth = delay as f64 / RATE as f64;
    let a = align(&print(&x), &print(&copy)).unwrap();
    assert!((a.offset - truth).abs() < 0.004 && a.is_precise(), "{truth}: {a:?}");
}

/// Which kind of speed voted more by 15% is the same however many detections share the votes:
/// 20 to 23 is even, over 7 slices or 20.
#[test]
fn key_lock_doesnt_depend_on_how_many_slices_voted() {
    let variants = fingerprint::variants(0.08);
    for (to, stretch, pitch, lock) in [
        (30.0, 20, 23, None),
        (40.0, 20, 23, None),
        (80.0, 20, 23, None),
        (40.0, 20, 24, Some(false)),
        (40.0, 24, 20, Some(true)),
    ] {
        let chain = chain_of(1, chain_at(&variants, 0.0, to, 10.0, 1.04), stretch, pitch);
        let found = plays(vec![chain], &variants);
        assert_eq!(found.first().map(|f| f.key_lock), Some(lock), "{to} s, {stretch} to {pitch}");
    }
}

#[test]
fn finds_tracks_through_a_mix() {
    let secs = |s: f64| (s * RATE as f64) as usize;
    let tracks: Vec<Vec<f32>> = (1..=4).map(|seed| synth(80.0, seed * 7919)).collect();
    let prints = tracks
        .iter()
        .enumerate()
        .map(|(i, x)| (i as u32 + 1, TrackPrint { duration_secs: 80.0, peaks: peaks(x, LIBRARY).0 }))
        .collect();
    let index = LibraryIndex::build(prints);

    // Track 1 from the top for 50 s; track 3 from its 10 s mark, 4% fast, crossfaded in over
    // the last 10 s of track 1 and playing for 60 s. Track 2 and 4 never play.
    let a = &tracks[0][..secs(50.0)];
    let b = faster(&tracks[2][secs(10.0)..], 1.04);
    let b = &b[..secs(60.0)];
    let start_b = secs(40.0);
    let mut mix = vec![0.0f32; start_b + b.len()];
    for (i, s) in a.iter().enumerate() {
        let fade = if i >= start_b { 1.0 - (i - start_b) as f32 / secs(10.0) as f32 } else { 1.0 };
        mix[i] += s * fade;
    }
    for (i, s) in b.iter().enumerate() {
        let fade = (i as f32 / secs(10.0) as f32).min(1.0);
        mix[start_b + i] += s * fade * 0.8;
    }

    let (q, frames) = peaks(&mix, QUERY);
    let found = find(&index, &q, frames, 0.08, None, &Ctx::silent()).unwrap();
    let slots: Vec<u32> = found.iter().map(|f| f.slot).collect();
    assert_eq!(slots, vec![0, 2], "{found:#?}");
    let (first, second) = (&found[0], &found[1]);
    assert!(first.start < 3.0 && (45.0..52.0).contains(&first.end), "{first:#?}");
    assert!((first.tempo - 1.0).abs() < 0.005);
    assert!((36.0..46.0).contains(&second.start), "{second:#?}");
    assert!((second.tempo - 1.04).abs() < 0.005, "tempo {}", second.tempo);
    assert_eq!(second.key_lock, Some(false));
    // It came in 10 s into the track.
    let at_start = second.track_start - (second.start - 40.0) * second.tempo;
    assert!((at_start - 10.0).abs() < 1.5, "track start {}", second.track_start);
    assert!(first.end > second.start, "the crossfade should overlap");
}

use hegel::TestCase;
use hegel::generators as gs;

/// A play of `slot` from `start` to `end` mix seconds with `votes`, as [`plays`] would make
/// it.
fn heard(slot: u32, start: f64, end: f64, votes: u32) -> Found {
    Found {
        slot,
        start,
        end,
        track_start: 0.0,
        track_end: end - start,
        tempo: 1.0,
        key_lock: None,
        votes,
        strongest: votes,
        heard: 10,
        spanned: 10,
        presence: Vec::new(),
        jumps: 0,
        alternates: Vec::new(),
        fits: Vec::new(),
    }
}

/// How far a play's tempo can be from the truth when measured through whole frames, over its
/// shortest line: anchors land up to a frame early, which tips a least-squares line over `n`
/// frames by up to `1.5 / n` (the early half all a frame out, the late half not), with room
/// for the stray anchors dropped before it's fitted again.
fn tempo_error(f: &Found) -> f64 {
    f.fits.iter().map(|fit| 2.0 / (fit.end - fit.start)).fold(0.0, f64::max)
}

#[hegel::test_helper]
fn plays_within(tc: &TestCase, duration: f64) -> Vec<Found> {
    let spans = tc.draw(
        gs::vecs(gs::tuples!(
            gs::floats::<f64>().min_value(0.0).max_value(duration),
            gs::floats::<f64>().min_value(0.0).max_value(duration),
            gs::integers::<u32>().min_value(1).max_value(2000),
        ))
        .max_size(8),
    );
    spans.into_iter().enumerate().map(|(i, (a, b, votes))| heard(i as u32, a.min(b), a.max(b), votes)).collect()
}

/// The gaps are the stretches of the mix no play covers, in order: with the plays they make
/// up the whole mix, the coverage is what they leave, and a minimum length only drops the
/// shorter ones.
#[hegel::test(test_cases = 300)]
fn gaps_and_plays_make_up_the_mix(tc: TestCase) {
    let duration = tc.draw(gs::floats::<f64>().min_value(1.0).max_value(7200.0));
    let found = plays_within(&tc, duration);
    let all = gaps(&found, duration, 0.0);
    assert!(all.windows(2).all(|w| w[0].1 <= w[1].0), "{all:?}");
    for (s, e) in &all {
        assert!(s <= e && found.iter().all(|f| f.end <= *s || f.start >= *e), "({s}, {e}) is played");
    }
    // The plays' union, merged independently.
    let mut spans: Vec<(f64, f64)> = found.iter().map(|f| (f.start, f.end)).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut union = 0.0;
    let mut at = 0.0f64;
    for (s, e) in spans {
        union += (e - s.max(at)).max(0.0);
        at = at.max(e);
    }
    let gap: f64 = all.iter().map(|(s, e)| e - s).sum();
    assert!((gap + union - duration).abs() < 1e-6 * duration, "{gap} + {union} != {duration}");
    let c = coverage(&found, duration);
    assert!((0.0..=1.0).contains(&c) && (c - union / duration).abs() < 1e-9, "{c}");
    let min = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(duration));
    let long: Vec<(f64, f64)> = all.iter().copied().filter(|(s, e)| e - s >= min).collect();
    assert_eq!(gaps(&found, duration, min), long);
}

/// Resolving keeps the strongest play and every play nothing else overlaps, in mix order;
/// what it names alongside a play are other tracks, each once.
#[hegel::test(test_cases = 300)]
fn resolving_keeps_the_strongest_and_the_unopposed(tc: TestCase) {
    let found = plays_within(&tc, 600.0);
    let resolved = resolve(found.clone());
    assert!(resolved.windows(2).all(|w| w[0].start <= w[1].start));
    let kept = |f: &Found| resolved.iter().any(|k| k.slot == f.slot);
    if let Some(top) = found.iter().max_by(|a, b| a.votes.cmp(&b.votes).then(b.slot.cmp(&a.slot))) {
        assert!(kept(top), "the strongest, {top:?}");
    }
    for f in &found {
        let alone = found.iter().all(|g| g.slot == f.slot || f.overlap(g) <= 0.0);
        assert!(!alone || kept(f), "{f:?} overlaps nothing but was dropped");
    }
    for k in &resolved {
        let mut named: Vec<u32> = k.alternates.iter().map(|a| a.0).collect();
        assert!(!named.contains(&k.slot));
        named.sort_unstable();
        named.dedup();
        assert_eq!(named.len(), k.alternates.len(), "{:?}", k.alternates);
    }
}

/// A track heard along any line (from any point in the mix, at any point in the track, at
/// any speed searched) is one play along that line: its times, speed and key lock are where
/// its anchors put them, and its confidence and pitch follow from them.
#[hegel::test(test_cases = 150)]
fn a_chain_along_any_line_is_one_play(tc: TestCase) {
    let variants = fingerprint::variants(0.08);
    let from = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(600.0));
    let to = from + tc.draw(gs::floats::<f64>().min_value(24.0).max_value(240.0));
    let at = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(300.0));
    let speed = tc.draw(gs::floats::<f64>().min_value(0.93).max_value(1.07));
    let (stretch, pitch) = (tc.draw(gs::integers::<u32>().max_value(40)), tc.draw(gs::integers::<u32>().max_value(40)));
    let mut c = chain_of(1, chain_at(&variants, from, to, at, speed), stretch, pitch);
    // A slice the line only grazes gets no anchors from `chain_at`; the matcher never makes one.
    c.detections.retain(|d| !d.pairs.is_empty());
    // Its detections, chained again, are the one chain.
    let rechained = chain(c.detections.clone(), &variants);
    assert_eq!(rechained.len(), 1);
    assert_eq!(rechained[0].detections.len(), c.detections.len());

    let found = plays(rechained, &variants);
    assert_eq!(found.len(), 1, "{found:#?}");
    let f = &found[0];
    let last = ((to * FRAMES_PER_SEC) as u32 / SLICE * SLICE) as f64 / FRAMES_PER_SEC;
    assert!((f.start - from).abs() < 0.4 && (f.end - last).abs() < 0.4, "{from}..{last}: {f:#?}");
    assert!((f.tempo - speed).abs() <= tempo_error(f), "{speed}: {}", f.tempo);
    // The track, `at` seconds in at `from`, is where the line puts it when first and last heard.
    let track_at = |t: f64| at + speed * (t - from);
    assert!((f.track_start - track_at(f.start)).abs() < 0.05, "{}: {f:#?}", track_at(f.start));
    assert!((f.track_end - track_at(f.end)).abs() < 0.05, "{}: {f:#?}", track_at(f.end));
    let lock = if (f.tempo - 1.0).abs() < 0.006 {
        None
    } else if stretch * 20 > pitch * 23 {
        Some(true)
    } else if pitch * 20 > stretch * 23 {
        Some(false)
    } else {
        None
    };
    assert_eq!(f.key_lock, lock);
    assert!((0.0..=1.0).contains(&f.confidence()));
    let semitones = if lock == Some(true) { 0.0 } else { 12.0 * f.tempo.log2() };
    assert_eq!(f.semitones(), semitones);
    assert_eq!((f.jumps, f.fits.len()), (0, 1));
}

/// A copy of a track delayed by any amount and at any level lines up with it at that delay,
/// heard throughout, and the other way round at minus that delay.
#[hegel::test(test_cases = 3, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn copies_line_up_at_any_delay(tc: TestCase) {
    let x = synth(30.0, tc.draw(gs::integers::<u32>().min_value(1)));
    let delay = tc.draw(gs::integers::<usize>().max_value(2 * RATE as usize));
    let gain = tc.draw(gs::floats::<f32>().min_value(0.2).max_value(1.0));
    let copy: Vec<f32> = std::iter::repeat_n(0.0, delay).chain(x.iter().map(|s| s * gain)).collect();
    let (a, b) = (print(&x), print(&copy));
    let truth = delay as f64 / RATE as f64;
    let ab = align(&a, &b).unwrap();
    assert!((ab.offset - truth).abs() < 0.005, "{truth}: {ab:?}");
    assert!(ab.coverage.0 > 0.9 && ab.coverage.1 > 0.9 && ab.drift.abs() < 0.004, "{ab:?}");
    let ba = align(&b, &a).unwrap();
    assert!((ba.offset + truth).abs() < 0.005, "{truth}: {ba:?}");
    // With enough shifts the offset counts as measured; another track shares next to nothing.
    assert!(ab.shifts < ALIGN_SHIFTS || ab.is_precise(), "{ab:?}");
    let other = print(&synth(30.0, tc.draw(gs::integers::<u32>().min_value(1)) ^ 0x5555));
    assert!(align(&a, &other).is_none_or(|o| o.coverage.0 < 0.1 && !o.is_precise()));
}

/// Four tracks of 40 s, and the index of them.
fn library() -> &'static (Vec<Vec<f32>>, LibraryIndex) {
    static LIBRARY: std::sync::OnceLock<(Vec<Vec<f32>>, LibraryIndex)> = std::sync::OnceLock::new();
    LIBRARY.get_or_init(|| {
        let tracks: Vec<Vec<f32>> = (1..=4).map(|seed| synth(40.0, seed * 7919)).collect();
        let index = LibraryIndex::build(tracks.iter().enumerate().map(|(i, x)| (i as u32, print(x))).collect());
        (tracks, index)
    })
}

/// Whether the mix holds a slice's worth of votes for any of the `heard` tracks, (slot, start, end,
/// from) in seconds, outside where it was put but on its line: `SLICE_VOTES` landmarks within a
/// slice that land within 4 frames of where the line puts the track, as the fit keeps them, each
/// counted once however many speeds find it. Two tracks share a motif now and then, and one played
/// where another's line carries on is that track heard for a moment longer, which no matcher can
/// tell otherwise.
fn carries_on(index: &LibraryIndex, peaks: &[Peak], heard: &[(u32, f64, f64, f64)]) -> bool {
    let landmarks = fingerprint::landmarks(peaks, QUERY_FANOUT);
    let variants = fingerprint::variants(0.02);
    heard.iter().any(|&(slot, start, end, from)| {
        let on_line: Vec<u32> = landmarks
            .iter()
            .filter(|lm| !(start..=end).contains(&(lm.t as f64 / FRAMES_PER_SEC)))
            .filter(|lm| {
                let expect = lm.t as f64 + (from - start) * FRAMES_PER_SEC;
                variants.iter().filter_map(|v| v.original_hash(lm)).any(|h| {
                    index
                        .bucket(h)
                        .iter()
                        .any(|&e| (e >> 32) as u32 == slot && ((e as u32) as f64 - expect).abs() <= 4.0)
                })
            })
            .map(|lm| lm.t)
            .collect();
        on_line.windows(SLICE_VOTES as usize).any(|w| w[w.len() - 1] - w[0] < SLICE)
    })
}

/// A set stitched from any of the library's tracks, each from any point in it, at any level,
/// after any silence and with any gap before the next, is heard as those tracks in that
/// order, each where it was put in the set and from where it was taken in the track.
#[hegel::test(test_cases = 4, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_stitched_set_is_heard_as_its_tracks(tc: TestCase) {
    let (tracks, index) = library();
    let secs = |s: f64| (s * RATE as f64) as usize;
    let order =
        tc.draw(gs::samples((0..tracks.len()).collect::<Vec<_>>()).without_replacement().min_size(2).max_size(3));
    let mut mix = vec![0.0f32; secs(tc.draw(gs::floats::<f64>().min_value(0.0).max_value(6.0)))];
    let mut truth = Vec::new();
    for &i in &order {
        mix.resize(mix.len() + secs(tc.draw(gs::floats::<f64>().min_value(0.0).max_value(3.0))), 0.0);
        let from = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(10.0));
        let len = tc.draw(gs::floats::<f64>().min_value(26.0).max_value(30.0));
        let gain = tc.draw(gs::floats::<f32>().min_value(0.3).max_value(1.0));
        let start = mix.len() as f64 / RATE as f64;
        mix.extend(tracks[i][secs(from)..secs(from + len)].iter().map(|s| s * gain));
        truth.push((i as u32, start, start + len, from));
    }
    let (q, frames) = peaks(&mix, QUERY);
    tc.assume(!carries_on(index, &q, &truth));
    let wide = tc.draw(gs::floats::<f64>().min_value(0.2501).max_value(1.0));
    assert!(matches!(find(index, &q, frames, wide, None, &Ctx::silent()), Err(Error::Invalid(_))));
    let found = find(index, &q, frames, 0.02, None, &Ctx::silent()).unwrap();
    let slots: Vec<u32> = found.iter().map(|f| f.slot).collect();
    assert_eq!(slots, truth.iter().map(|t| t.0).collect::<Vec<_>>(), "{found:#?}");
    for (f, (_, start, end, from)) in found.iter().zip(&truth) {
        assert!((f.start - start).abs() < 2.0 && (f.end - end).abs() < 2.0, "{start}..{end}: {f:#?}");
        assert!((f.track_start - from).abs() < 2.0 && (f.tempo - 1.0).abs() < 0.005, "from {from}: {f:#?}");
    }
}

/// A track heard in pieces (split by breakdowns, looped or cue-jumped further on, or heard
/// again minutes later) is one play for each run of pieces less than 24 s apart: one line for
/// the pieces that carry on where the last left off, another after each jump, and the jumps
/// counted.
#[hegel::test(test_cases = 150)]
fn a_track_heard_in_pieces_is_a_play_for_each_stretch(tc: TestCase) {
    let variants = fingerprint::variants(0.08);
    let speed = tc.draw(gs::floats::<f64>().min_value(0.95).max_value(1.05));
    let (mut from, mut at) = (tc.draw(gs::floats::<f64>().min_value(0.0).max_value(100.0)), 30.0);
    // Per piece: its chain, then whether it starts a play and whether it jumped.
    let mut pieces: Vec<(Chain, bool, bool)> = Vec::new();
    for i in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        // The matcher hears in slices (~4 s), and a slice a piece only partly fills may go
        // unheard, so a gap can read up to a slice longer at each end: short ones are kept
        // that far under the play gap, or which side of it they fall on isn't the matcher's
        // to tell.
        let short = PLAY_GAP - 2.0 * f64::from(SLICE) / FRAMES_PER_SEC;
        let gap = tc.draw(hegel::one_of!(
            gs::floats::<f64>().min_value(2.0).max_value(short),
            gs::floats::<f64>().min_value(30.0).max_value(90.0)
        ));
        let jump = i > 0 && tc.draw(gs::booleans());
        if i > 0 {
            at += speed * gap + if jump { tc.draw(gs::floats::<f64>().min_value(20.0).max_value(60.0)) } else { 0.0 };
            from += gap;
        }
        let len = tc.draw(gs::floats::<f64>().min_value(24.0).max_value(60.0));
        let mut c = chain_at(&variants, from, from + len, at, speed);
        c.detections.retain(|d| !d.pairs.is_empty());
        pieces.push((c, i == 0 || gap > PLAY_GAP, jump));
        from += len;
        at += speed * len;
    }
    let mut expected: Vec<(usize, u32)> = Vec::new();
    for (_, starts, jump) in &pieces {
        match expected.last_mut() {
            Some(last) if !starts => {
                last.0 += usize::from(*jump);
                last.1 += u32::from(*jump);
            }
            _ => expected.push((1, 0)),
        }
    }
    let mut found = plays(pieces.into_iter().map(|p| p.0).collect(), &variants);
    found.sort_by(|a, b| a.start.total_cmp(&b.start));
    let shapes: Vec<(usize, u32)> = found.iter().map(|f| (f.fits.len(), f.jumps)).collect();
    assert_eq!(shapes, expected, "{found:#?}");
    assert!(found.iter().all(|f| (f.tempo - speed).abs() <= tempo_error(f)), "{speed}: {found:#?}");
}

/// What tells two detections apart, for comparing lists of them.
fn key(d: &Detection) -> (u32, u32, u16, u32, Vec<(u32, u32)>) {
    (d.slot, d.slice, d.variant, d.votes, d.pairs.clone())
}

/// However detections fall (any tracks and slices, anchors on any of a few lines or none, in
/// the order `detect` gives them), chaining puts each in exactly one chain of its own track, and
/// a chain moves on through the mix: one detection a slice, never more than a few slices apart.
#[hegel::test(test_cases = 300)]
fn chaining_puts_every_detection_in_one_chain(tc: TestCase) {
    let variants = fingerprint::variants(0.08);
    let mut detections: Vec<Detection> = (0..tc.draw(gs::integers::<usize>().max_value(30)))
        .map(|_| {
            let slice = tc.draw(gs::integers::<u32>().max_value(16));
            let line = tc.draw(gs::sampled_from(vec![0u32, 400, 2000]));
            let pairs = tc.draw(
                gs::vecs(gs::tuples!(gs::integers::<u32>().max_value(SLICE - 1), gs::integers::<u32>().max_value(20)))
                    .max_size(5),
            );
            Detection {
                slot: tc.draw(gs::integers::<u32>().max_value(2)),
                slice,
                variant: tc.draw(gs::integers::<u16>().max_value(variants.len() as u16 - 1)),
                votes: tc.draw(gs::integers::<u32>().min_value(1).max_value(50)),
                stretch_votes: 0,
                pitch_votes: 0,
                pairs: pairs
                    .into_iter()
                    .map(|(t, jitter)| (slice * SLICE + t, line + slice * SLICE + t + jitter))
                    .collect(),
            }
        })
        .collect();
    detections.sort_by_key(|d| (d.slot, d.slice, std::cmp::Reverse(d.votes)));
    let chains = chain(detections.clone(), &variants);
    assert!(chains.windows(2).all(|w| w[0].slot <= w[1].slot));
    for c in &chains {
        assert!(c.detections.iter().all(|d| d.slot == c.slot));
        let slices: Vec<u32> = c.detections.iter().map(|d| d.slice).collect();
        assert!(slices.windows(2).all(|w| (1..=CHAIN_GAP).contains(&(w[1] - w[0]))), "{slices:?}");
    }
    let mut chained: Vec<_> = chains.iter().flat_map(|c| &c.detections).map(key).collect();
    let mut given: Vec<_> = detections.iter().map(key).collect();
    chained.sort();
    given.sort();
    assert_eq!(chained, given);
}

/// The speed searched nearest `speed`.
fn nearest(variants: &[Variant], speed: f64) -> usize {
    let off = |v: &Variant| (v.ratio - speed).abs();
    (0..variants.len()).min_by(|&a, &b| off(&variants[a]).total_cmp(&off(&variants[b]))).unwrap()
}

/// One stretch of a track at a speed searched is a play once it has 60 votes (a chain needs
/// 30 and three slices, or 60 in fewer, so that's all it takes). Its speed is the one searched
/// when its anchors span under 8 s, too short for a slope, and when they lie along a line over
/// 2% off it (the fit locked onto noise); otherwise it's the slope they measure.
#[hegel::test(test_cases = 300)]
fn a_stretch_is_a_play_with_votes_enough(tc: TestCase) {
    let variants = fingerprint::variants(0.08);
    let from = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(300.0));
    let len = tc.draw(gs::floats::<f64>().min_value(4.0).max_value(60.0));
    let speed = tc.draw(gs::floats::<f64>().min_value(0.96).max_value(1.04));
    let at = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(200.0));
    let mut c = chain_at(&variants, from, from + len, at, speed);
    c.detections.retain(|d| !d.pairs.is_empty());
    tc.assume(!c.detections.is_empty());
    // Searched at a speed far from the one it plays at.
    let off = tc.draw(gs::booleans());
    let searched = if off {
        nearest(&variants, if speed < 1.0 { speed + 0.03 } else { speed - 0.03 })
    } else {
        c.detections[0].variant as usize
    };
    for d in &mut c.detections {
        d.votes = tc.draw(gs::integers::<u32>().min_value(1).max_value(40));
        d.variant = searched as u16;
    }
    let votes: u32 = c.detections.iter().map(|d| d.votes).sum();
    let ts: Vec<u32> = c.detections.iter().flat_map(|d| d.pairs.iter().map(|p| p.0)).collect();
    let span = f64::from(ts.iter().max().unwrap() - ts.iter().min().unwrap());
    let found = plays(vec![c], &variants);
    assert_eq!(found.len(), usize::from(votes >= PLAY_VOTES), "{votes} votes: {found:#?}");
    let Some(f) = found.first() else { return };
    let guess = variants[searched].ratio;
    // Where its anchors put the track, at the speed searched, when that's the one it plays at.
    let track_at = |t: f64| at + speed * (t - from);
    if !off && span < 8.0 * FRAMES_PER_SEC {
        assert!((f.track_start - track_at(f.start)).abs() < 0.1, "{}: {f:#?}", track_at(f.start));
    }
    if off || span < 8.0 * FRAMES_PER_SEC {
        // (A mean over the play's chains, weighted by their votes: rounding aside, the speed.)
        assert!(
            (f.tempo - guess).abs() < 1e-12,
            "over {span} frames, searched at {guess}, playing at {speed}: {}",
            f.tempo
        );
        assert!(!f.fits[0].measured);
    } else {
        assert!((f.tempo - speed).abs() <= tempo_error(f), "{speed}: {}", f.tempo);
    }
}

/// A stretch found twice, through two neighbouring speeds (two chains on one line, sharing
/// some slices), is one play along that line: its votes counted once a slice (the stronger
/// of the two there), its slices each heard once.
#[hegel::test(test_cases = 200)]
fn a_stretch_found_twice_is_one_play(tc: TestCase) {
    let variants = fingerprint::variants(0.08);
    let from = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(100.0));
    let len = tc.draw(gs::floats::<f64>().min_value(30.0).max_value(80.0));
    let speed = tc.draw(gs::floats::<f64>().min_value(0.97).max_value(1.03));
    let at = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(100.0));
    let mut a = chain_at(&variants, from, from + len, at, speed);
    a.detections.retain(|d| !d.pairs.is_empty());
    let mut b = a.clone();
    // The next speed searched of the other kind (key lock or not), as close as it comes.
    let other = (0..variants.len())
        .filter(|&v| v != a.detections[0].variant as usize && (variants[v].ratio - speed).abs() < 0.008)
        .min_by(|&x, &y| (variants[x].ratio - speed).abs().total_cmp(&(variants[y].ratio - speed).abs()))
        .unwrap();
    let n = b.detections.len();
    let first = tc.draw(gs::integers::<usize>().max_value(n - 3));
    let last = tc.draw(gs::integers::<usize>().min_value(first + 3).max_value(n));
    b.detections = b.detections[first..last].to_vec();
    for d in &mut a.detections {
        d.votes = tc.draw(gs::integers::<u32>().min_value(10).max_value(40));
    }
    for d in &mut b.detections {
        d.votes = tc.draw(gs::integers::<u32>().min_value(10).max_value(40));
        d.variant = other as u16;
        // Anchors a frame off here and there: the other speed rounds differently.
        let nudge = tc.draw(gs::booleans());
        d.pairs.iter_mut().step_by(3).for_each(|p| p.1 += u32::from(nudge));
    }
    let mut best: FxHashMap<u32, u32> = FxHashMap::default();
    for d in a.detections.iter().chain(&b.detections) {
        let v = best.entry(d.slice).or_default();
        *v = (*v).max(d.votes);
    }
    let found = plays(vec![a, b], &variants);
    assert_eq!(found.len(), 1, "{found:#?}");
    let f = &found[0];
    assert_eq!((f.fits.len(), f.jumps), (1, 0));
    assert_eq!((f.votes, f.heard), (best.values().sum::<u32>(), best.len() as u32));
}

/// One track can't be in two places at once: of two stretches of it heard at the same time
/// from different places in it (a chorus coming round again), the weaker is dropped when it
/// overlaps the stronger by half its length, or by over 6 s with under 60% of its votes. One
/// heard just after it, or overlapping it for a moment with nearly as many votes, is kept: a
/// jump to another place in the track.
#[hegel::test(test_cases = 200)]
fn a_track_isnt_heard_in_two_places_at_once(tc: TestCase) {
    let variants = fingerprint::variants(0.08);
    // 15 slices of 20 votes.
    let strong = chain_at(&variants, 0.0, 60.0, 10.0, 1.0);
    let len = tc.draw(gs::floats::<f64>().min_value(24.0).max_value(36.0));
    let (kind, from, votes) = match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => ("inside", tc.draw(gs::floats::<f64>().min_value(2.0).max_value(58.0 - len)), 30),
        1 => ("ghost", 60.0 - tc.draw(gs::floats::<f64>().min_value(8.0).max_value(0.45 * len)), 5),
        2 => ("after", 60.0 + tc.draw(gs::floats::<f64>().min_value(6.0).max_value(18.0)), 5),
        _ => ("brief", 60.0 - tc.draw(gs::floats::<f64>().min_value(0.5).max_value(4.0)), 35),
    };
    let mut weak = chain_at(&variants, from, from + len, 150.0, 1.0);
    weak.detections.retain(|d| !d.pairs.is_empty());
    weak.detections.iter_mut().for_each(|d| d.votes = votes);
    let found = plays(vec![strong, weak], &variants);
    let fits: usize = found.iter().map(|f| f.fits.len()).sum();
    let kept = matches!(kind, "after" | "brief");
    assert_eq!((found.len(), fits), (1, 1 + usize::from(kept)), "{kind} from {from} for {len}: {found:#?}");
    assert_eq!(found[0].jumps, u32::from(kept));
}

/// Traced along its line, a play of a library track in the mix grows to where the track is
/// heard, from the stretch detection found, and where the track is follows the line; its
/// presence runs from 0 to 1. It doesn't grow over a gap of silence into where the track comes
/// back further on. That's so whatever the key lock reading, and however its line is split
/// up: a line cut into pieces traces as the whole of it.
#[hegel::test(test_cases = 3, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn tracing_follows_a_play_along_its_line(tc: TestCase) {
    let (tracks, index) = library();
    let variants = fingerprint::variants(0.02);
    let secs = |s: f64| (s * RATE as f64) as usize;
    let slot = tc.draw(gs::integers::<usize>().max_value(tracks.len() - 1));
    // All of it within the track's 40 s.
    let from = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(4.0));
    let len = tc.draw(gs::floats::<f64>().min_value(20.0).max_value(24.0));
    let lead = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(4.0));
    let mut mix = vec![0.0f32; secs(lead)];
    mix.extend_from_slice(&tracks[slot][secs(from)..secs(from + len)]);
    // Then silence, over the one quiet stretch the trace forgives, and the track again where
    // it would have been.
    let gap = tc.draw(gs::floats::<f64>().min_value(6.0).max_value(8.0));
    mix.resize(mix.len() + secs(gap), 0.0);
    mix.extend_from_slice(&tracks[slot][secs(from + len + gap)..secs(from + len + gap + 4.0)]);
    let (q, _) = peaks(&mix, QUERY);
    let landmarks = fingerprint::landmarks(&q, QUERY_FANOUT);
    // Detection found the middle of it, on the line the track plays along.
    let start = lead + tc.draw(gs::floats::<f64>().min_value(2.0).max_value(6.0));
    let end = lead + len - tc.draw(gs::floats::<f64>().min_value(2.0).max_value(6.0));
    let line = |a: f64, b: f64| Fit {
        ratio: 1.0,
        intercept: (from - lead) * FRAMES_PER_SEC,
        start: a * FRAMES_PER_SEC,
        end: b * FRAMES_PER_SEC,
        measured: true,
    };
    let mut cuts = tc.draw(gs::vecs(gs::floats::<f64>().min_value(start).max_value(end)).min_size(1).max_size(3));
    cuts.sort_by(f64::total_cmp);
    let ends: Vec<f64> = std::iter::once(start).chain(cuts).chain([end]).collect();
    for key_lock in [None, Some(true), Some(false)] {
        let traced = |fits: Vec<Fit>| {
            let mut f = Found { key_lock, fits, ..heard(slot as u32, start, end, 200) };
            trace(index, &landmarks, &variants, &mut f);
            f
        };
        let whole = traced(vec![line(start, end)]);
        let pieces = traced(ends.windows(2).map(|w| line(w[0], w[1])).collect());
        assert_eq!(
            (pieces.start, pieces.end, pieces.track_start, pieces.track_end, &pieces.presence),
            (whole.start, whole.end, whole.track_start, whole.track_end, &whole.presence),
            "{key_lock:?}, {ends:?}"
        );
        let f = whole;
        let heard = (f.start - lead).abs() < 2.0 && (f.end - lead - len).abs() < 2.0;
        assert!(heard, "{key_lock:?}: {lead}..{}: {f:#?}", lead + len);
        assert!((f.track_start - (from + f.start - lead)).abs() < 0.05, "{key_lock:?}: {f:#?}");
        assert!((f.track_end - (from + f.end - lead)).abs() < 0.05, "{key_lock:?}: {f:#?}");
        assert!(f.presence.iter().all(|p| (0.0..=1.0).contains(p)) && f.presence.contains(&1.0), "{:?}", f.presence);
    }
}

/// A cancelled search stops as cancelled, whatever it searches for and in.
#[hegel::test(test_cases = 20)]
fn a_cancelled_search_is_cancelled(tc: TestCase) {
    let prints = (0..tc.draw(gs::integers::<u32>().max_value(3)))
        .map(|i| (i, TrackPrint { duration_secs: 30.0, peaks: crate::index::tests::sorted_peaks(&tc, 6, 300) }))
        .collect();
    let index = &LibraryIndex::build(prints);
    let peaks = crate::index::tests::sorted_peaks(&tc, 6, 300);
    let frames = peaks.last().map_or(0, |p| p.t) + tc.draw(gs::integers::<u32>().min_value(1).max_value(2000));
    let ctx = Ctx::silent();
    ctx.cancel.cancel();
    let range = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(0.25));
    let found = find(index, &peaks, frames, range, None, &ctx);
    assert!(matches!(found, Err(Error::Cancelled(_))), "{found:?}");
}

/// A mix of no length has nothing to cover, whatever was found in it.
#[hegel::test(test_cases = 50)]
fn a_mix_of_no_length_is_not_covered(tc: TestCase) {
    let found = plays_within(&tc, 600.0);
    let duration = tc.draw(gs::floats::<f64>().min_value(-600.0).max_value(0.0));
    assert_eq!(coverage(&found, duration), 0.0);
}

/// A stretch of a track that recurs in it (a loop, a repeated bar), heard once in the mix,
/// matches it as strongly at every recurrence: a slice keeps three of them at most, each
/// with votes enough, at its own place in the track, the strongest first.
#[hegel::test(test_cases = 25)]
fn a_recurring_stretch_is_heard_three_ways_at_most(tc: TestCase) {
    let period = tc.draw(gs::integers::<u32>().min_value(70).max_value(120));
    let mut pattern: Vec<Peak> = tc
        .draw(
            gs::vecs(gs::tuples!(
                gs::integers::<u32>().max_value(period - 1),
                gs::integers::<u8>().min_value(8).max_value(200)
            ))
            .min_size(20)
            .max_size(40),
        )
        .into_iter()
        .map(|(t, f)| Peak { t, f })
        .collect();
    pattern.sort_unstable();
    pattern.dedup();
    let repeats = tc.draw(gs::integers::<u32>().min_value(1).max_value(5));
    let track: Vec<Peak> =
        (0..repeats).flat_map(|k| pattern.iter().map(move |p| Peak { t: p.t + k * period, ..*p })).collect();
    let print = TrackPrint { duration_secs: (repeats * period) as f32 / FRAMES_PER_SEC as f32, peaks: track };
    let index = LibraryIndex::build(vec![(7, print)]);
    // Within one slice of the mix.
    let at = SLICE + tc.draw(gs::integers::<u32>().max_value(SLICE - period));
    let mix: Vec<Peak> = pattern.iter().map(|p| Peak { t: p.t + at, ..*p }).collect();
    let variants = fingerprint::variants(0.0);
    let found = detect(&index, &mix, 3 * SLICE, &variants, &Ctx::silent()).unwrap();
    tc.assume(fingerprint::landmarks(&pattern, crate::index::LIBRARY_FANOUT).len() >= 2 * SLICE_VOTES as usize);
    assert!(found.iter().all(|d| d.slot == 0 && d.slice == 1 && d.votes >= SLICE_VOTES), "{found:#?}");
    assert_eq!(found.len(), repeats.min(3) as usize, "{found:#?}");
    assert!(found.windows(2).all(|w| w[0].votes >= w[1].votes));
    let mut places: Vec<f64> = found.iter().map(|d| d.centre().1 - d.centre().0).collect();
    places.sort_by(f64::total_cmp);
    assert!(places.windows(2).all(|w| w[1] - w[0] > 1.5 * FRAMES_PER_SEC), "{places:?}");
}

/// A print lines up with itself within a frame of no offset, with enough of it in common, and
/// with an empty print not at all, either way round.
#[hegel::test(test_cases = 200)]
fn a_print_lines_up_with_itself_and_not_with_nothing(tc: TestCase) {
    let mut peaks = crate::index::tests::sorted_peaks(&tc, 6, 200);
    // Peaks in pairs a frame apart, as a sustained note gives: then no peak has one match
    // alone, and the offset comes from the landmarks.
    if tc.draw(gs::booleans()) {
        peaks = peaks.iter().flat_map(|p| [*p, Peak { t: p.t + 1, ..*p }]).collect();
        peaks.sort_unstable();
        peaks.dedup();
    }
    let a = TrackPrint { duration_secs: 60.0, peaks };
    let empty = TrackPrint { duration_secs: 0.0, peaks: Vec::new() };
    assert_eq!(align(&a, &empty), None);
    assert_eq!(align(&empty, &a), None);
    let shared = fingerprint::landmarks(&a.peaks, crate::index::LIBRARY_FANOUT).len();
    let Some(itself) = align(&a, &a) else { return assert!(shared < SLICE_VOTES as usize, "{shared} landmarks") };
    assert!(itself.offset.abs() <= 1.0 / FRAMES_PER_SEC && itself.pairs >= SLICE_VOTES as usize, "{itself:?}");
    assert!([itself.coverage.0, itself.coverage.1].iter().all(|c| (0.0..=1.0).contains(c)), "{itself:?}");
}

/// A vote of other audio that lands on a chain's line, as one can up to a slice past where the
/// track is heard, isn't where the track came in or went out: along a track every anchor has
/// another within a second.
#[hegel::test(test_cases = 100)]
fn a_stray_vote_on_a_chains_line_isnt_where_it_ends(tc: TestCase) {
    let variants = fingerprint::variants(0.08);
    let from = tc.draw(gs::floats::<f64>().min_value(10.0).max_value(600.0));
    let to = from + tc.draw(gs::floats::<f64>().min_value(24.0).max_value(240.0));
    let at = tc.draw(gs::floats::<f64>().min_value(10.0).max_value(300.0));
    let speed = tc.draw(gs::floats::<f64>().min_value(0.93).max_value(1.07));
    let mut c = chain_at(&variants, from, to, at, speed);
    c.detections.retain(|d| !d.pairs.is_empty());
    let clean = fit(&c, &variants);
    // From over a second to a slice past the first or last anchor, off the line by as much as
    // the fit keeps.
    let slice = f64::from(SLICE) / FRAMES_PER_SEC;
    let away = tc.draw(gs::floats::<f64>().min_value(EDGE_GAP + 0.05).max_value(slice)) * FRAMES_PER_SEC;
    let off = tc.draw(gs::floats::<f64>().min_value(-2.4).max_value(2.4));
    let after = tc.draw(gs::booleans());
    let t = if after { clean.end + away } else { clean.start - away };
    let stray = (t.round() as u32, (clean.track_frame(t) + off).round() as u32);
    let edge = if after { c.detections.last_mut() } else { c.detections.first_mut() };
    edge.expect("a chain of 24 s has anchors").pairs.push(stray);
    let strayed = fit(&c, &variants);
    assert_eq!((strayed.start, strayed.end), (clean.start, clean.end), "{stray:?}: {clean:?} {strayed:?}");
}

/// Past where a track ends, a bin is heard from two matches that share no peak. Landmarks that
/// share one, or the same peaks a frame apart (an onset spans two frames), are one match of a few
/// peaks, which a motif two tracks share gives too: counted apiece, they carried a play seconds on
/// into the next track.
#[hegel::test(test_cases = 100)]
fn tracing_hears_a_bin_from_matches_that_share_no_peak(tc: TestCase) {
    // A track with a peak every 10 frames, each paired in the index with the next.
    let (base, turn) = (
        tc.draw(gs::integers::<u8>().min_value(20).max_value(200)),
        tc.draw(gs::integers::<usize>().min_value(1).max_value(30)),
    );
    let peaks: Vec<Peak> = (0..120).map(|i| Peak { t: 10 * i as u32, f: base + (i * turn % 31) as u8 }).collect();
    let print = TrackPrint { duration_secs: 30.0, peaks: peaks.clone() };
    let index = LibraryIndex::build(vec![(0, print)]);
    let pair = |a: Peak, b: Peak| Landmark { t: a.t, f1: a.f, f2: b.f, dt: (b.t - a.t) as u8 };
    // The mix plays it from the top until bin `last` ends, heard in pairs of peaks of their own.
    let step = PRESENCE_STEP * FRAMES_PER_SEC;
    let last = tc.draw(gs::integers::<u32>().min_value(4).max_value(7));
    let gone = f64::from(last + 1) * step;
    let mut landmarks: Vec<Landmark> =
        peaks.chunks(2).filter(|p| f64::from(p[1].t) < gone).map(|p| pair(p[0], p[1])).collect();
    // Then two landmarks of it in the next bin, where its line puts them.
    let k = (gone / 10.0).ceil() as usize + tc.draw(gs::integers::<usize>().max_value(3));
    let apart = tc.draw(gs::integers::<u8>().max_value(2));
    let second = match apart {
        // Sharing a peak.
        0 => pair(peaks[k + 1], peaks[k + 2]),
        // The same peaks a frame later.
        1 => Landmark { t: peaks[k].t + 1, ..pair(peaks[k], peaks[k + 1]) },
        // Peaks of their own.
        _ => pair(peaks[k + 3], peaks[k + 4]),
    };
    landmarks.extend([pair(peaks[k], peaks[k + 1]), second]);
    let end = (f64::from(last) * step + 10.0) / FRAMES_PER_SEC;
    let fit = Fit { ratio: 1.0, intercept: 0.0, start: FRAMES_PER_SEC, end: end * FRAMES_PER_SEC, measured: true };
    let mut f = Found { fits: vec![fit], ..heard(0, 1.0, end, 200) };
    trace(&index, &landmarks, &fingerprint::variants(0.02), &mut f);
    if apart == 2 {
        assert!((f.end * FRAMES_PER_SEC).round() >= f64::from(second.t), "{second:?}: {f:#?}");
    } else {
        assert!(f.end * FRAMES_PER_SEC < gone, "{second:?}: {f:#?}");
    }
}

/// Every vote of a cell with enough of them is kept, whichever places its two bins fall in,
/// in key order; nearly all strays, far from any other vote, are dropped before the sort.
#[test]
fn ballots_keep_the_votes_cells_need() {
    let vote = |slot: u64, bin: u64, t| Vote { key: (slot << 32) | (3 << 24) | bin, t, tau: 0 };
    // Track 1: 3 + 3 votes over a cell starting on an even bin, then 5 + 1 over one starting
    // on an odd bin (the lone vote is only in the cell before it). Track 2: strays.
    let cells: Vec<Vote> = [(1000, 3), (1001, 3), (2001, 5), (2002, 1)]
        .into_iter()
        .flat_map(|(bin, n)| (0..n).map(move |t| vote(1, bin, t)))
        .collect();
    let strays = (0..100).map(|i| vote(2, 5000 + 10 * i, i as u32));
    let mut ballots = Ballots { cast: strays.chain(cells.iter().copied()).rev().collect(), ..Ballots::default() };
    ballots.keep();
    let kept = &ballots.kept;
    assert!(kept.is_sorted_by_key(|v| v.key));
    assert!(cells.iter().all(|c| kept.iter().any(|k| (k.key, k.t) == (c.key, c.t))));
    assert!(kept.len() < cells.len() + 10, "{} kept", kept.len());
}
