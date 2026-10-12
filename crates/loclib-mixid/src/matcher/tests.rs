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
    // With enough pairs the offset counts as measured; another track shares next to nothing.
    assert!(ab.pairs < ALIGN_PAIRS || ab.is_precise(), "{ab:?}");
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
