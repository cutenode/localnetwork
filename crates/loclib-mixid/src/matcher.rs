//! Finding library tracks in a mix.
//!
//! 1. **Vote.** The mix is cut into ~4 s slices. Every landmark in a slice is mapped back through
//!    each candidate playback speed ([`Variant`]) and looked up in the library index. Each hit
//!    votes for (track, speed, offset), where the offset is the mix time at which the track's
//!    first frame would have played. A track really playing piles its votes onto one offset; the
//!    best such cell per track, if it has enough votes, is a detection.
//! 2. **Chain.** Detections of a track in nearby slices that agree on where in the track the mix
//!    is (allowing for the speed) join into a chain: one continuous stretch of playback.
//! 3. **Group.** Chains of a track close together in time are one play (a loop or a cue jump
//!    starts a new chain, not a new play).
//! 4. **Resolve.** Two library entries with the same audio (a re-download, a remaster) both match
//!    a play; a remix or the original matches part of one. The stronger match is kept and the
//!    others are reported alongside it.
//! 5. **Refine.** A line fitted through each chain's matching (mix time, track time) pairs gives
//!    the playback speed and where the track came in and went out.

use rustc_hash::FxHashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use loclib_progress::{Ctx, Event, Phase};

use crate::fingerprint::{self, FRAMES_PER_SEC, Landmark, Peak, Variant};
use crate::index::LibraryIndex;
use crate::{Error, Result};

/// Landmarks per mix peak: more than the library's, so its pairs are among them even when the
/// other deck adds peaks in between.
const QUERY_FANOUT: usize = 8;
/// Frames per voting slice (~4 s).
const SLICE: u32 = 172;
/// Offset histogram bin, in frames. Adjacent bins are counted together.
const OFFSET_BIN: f64 = 2.0;
const OFFSET_BIAS: f64 = (1u64 << 23) as f64;
/// Votes a slice needs before it counts as hearing a track.
const SLICE_VOTES: u32 = 6;
/// Slices a chain may skip (a breakdown, the other deck drowning it out).
const CHAIN_GAP: u32 = 4;
/// How far (seconds of track time) a detection may stray from where its chain predicts.
const CHAIN_TOLERANCE: f64 = 0.75;
/// A chain needs this many votes, and this many detections unless it has twice the votes.
const CHAIN_VOTES: u32 = 30;
const CHAIN_DETECTIONS: usize = 3;
/// Chains whose lines agree to within this (seconds of track time) are one playback.
const MERGE_TOLERANCE: f64 = 1.5;
/// A weaker chain overlapping a stronger one by this long (seconds) is a ghost of it.
const GHOST_OVERLAP: f64 = 6.0;
/// Chains of one track this close (seconds) belong to one play.
const PLAY_GAP: f64 = 24.0;
/// Shortest play reported, seconds (shorter ones are a shared sample, not a track).
const PLAY_SECS: f64 = 16.0;
/// Votes a play needs to be reported.
const PLAY_VOTES: u32 = 60;
/// Presence curve resolution, seconds.
pub const PRESENCE_STEP: f64 = 2.0;
/// Speeds whose votes are counted at once. A slice's votes for every speed together run to
/// hundreds of MB per worker on a big library; a few speeds at a time bound that, and as votes
/// for different speeds never meet, nothing else changes.
const VARIANT_CHUNK: usize = 8;

#[derive(Debug, Clone)]
pub struct Detection {
    pub slot: u32,
    pub slice: u32,
    pub variant: u16,
    pub votes: u32,
    /// Best votes among key-locked (time only) and pitched speeds.
    pub stretch_votes: u32,
    pub pitch_votes: u32,
    /// Matching (mix frame, track frame) anchors.
    pub pairs: Vec<(u32, u32)>,
}

impl Detection {
    /// Median mix frame and track frame of the pairs.
    fn centre(&self) -> (f64, f64) {
        let mut t: Vec<u32> = self.pairs.iter().map(|p| p.0).collect();
        let mut tau: Vec<u32> = self.pairs.iter().map(|p| p.1).collect();
        (median_u32(&mut t), median_u32(&mut tau))
    }
}

fn median_u32(v: &mut [u32]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mid = v.len() / 2;
    *v.select_nth_unstable(mid).1 as f64
}

#[derive(Clone, Copy)]
struct Vote {
    /// `(slot << 32) | (variant << 24) | offset bin`.
    key: u64,
    t: u32,
    tau: u32,
}

/// Count votes slice by slice (in parallel) and keep each track's best cell per slice.
pub fn detect(
    index: &LibraryIndex,
    peaks: &[Peak],
    frames: u32,
    variants: &[Variant],
    ctx: &Ctx<'_>,
) -> Result<Vec<Detection>> {
    let landmarks = fingerprint::landmarks(peaks, QUERY_FANOUT);
    let slices = frames.div_ceil(SLICE).max(1);
    let bounds: Vec<usize> = (0..=slices).map(|s| landmarks.partition_point(|l| l.t < s * SLICE)).collect();
    ctx.phase_start(Phase::Match, slices as u64, 0);
    let next = AtomicUsize::new(0);
    let out = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    let result = std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| -> Result<()> {
                    let mut votes = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= slices as usize {
                            return Ok(());
                        }
                        ctx.check()?;
                        let found = slice_detections(
                            index,
                            &landmarks[bounds[i]..bounds[i + 1]],
                            i as u32,
                            variants,
                            &mut votes,
                        );
                        out.lock().unwrap().extend(found);
                        ctx.emit(Event::Item { phase: Phase::Match, path: String::new(), bytes: 0 });
                    }
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("match worker panicked")).collect::<Result<Vec<()>>>()
    });
    ctx.phase_end(Phase::Match);
    result?;
    let mut out = out.into_inner().unwrap();
    out.sort_by_key(|d| (d.slot, d.slice, std::cmp::Reverse(d.votes)));
    Ok(out)
}

fn slice_detections(
    index: &LibraryIndex,
    landmarks: &[Landmark],
    slice: u32,
    variants: &[Variant],
    votes: &mut Vec<Vote>,
) -> Vec<Detection> {
    // Every cell with enough votes is a candidate. Repetitive music gives one track several
    // (the same bars recur), and a few slices later only the real one is still in line.
    struct Cell {
        votes: u32,
        variant: u16,
        pairs: Vec<(u32, u32)>,
    }
    let mut cells: FxHashMap<u32, Vec<Cell>> = FxHashMap::default();
    let mut lock: FxHashMap<u32, (u32, u32)> = FxHashMap::default();
    for (chunk, speeds) in variants.chunks(VARIANT_CHUNK).enumerate() {
        votes.clear();
        for lm in landmarks {
            for (vi, v) in speeds.iter().enumerate().map(|(i, v)| (chunk * VARIANT_CHUNK + i, v)) {
                let Some(h) = v.original_hash(lm) else { continue };
                for &e in index.bucket(h) {
                    let slot = e >> 32;
                    let tau = e as u32;
                    let t0 = lm.t as f64 - tau as f64 / v.ratio;
                    let bin = ((t0 + OFFSET_BIAS) / OFFSET_BIN) as u64 & 0xff_ffff;
                    votes.push(Vote { key: (slot << 32) | ((vi as u64) << 24) | bin, t: lm.t, tau });
                }
            }
        }
        votes.sort_unstable_by_key(|v| v.key);
        let mut i = 0;
        while i < votes.len() {
            let key = votes[i].key;
            let mut j = i + 1;
            while j < votes.len() && votes[j].key == key {
                j += 1;
            }
            // The next offset bin of the same track and speed counts too.
            let mut k = j;
            while k < votes.len() && votes[k].key == key + 1 {
                k += 1;
            }
            let n = (k - i) as u32;
            if n >= SLICE_VOTES {
                let slot = (key >> 32) as u32;
                let variant = ((key >> 24) & 0xff) as u16;
                let pairs = votes[i..k].iter().map(|v| (v.t, v.tau)).collect();
                cells.entry(slot).or_default().push(Cell { votes: n, variant, pairs });
                let l = lock.entry(slot).or_default();
                if variants[variant as usize].pitch {
                    l.1 = l.1.max(n);
                } else {
                    l.0 = l.0.max(n);
                }
            }
            i = j;
        }
    }
    let mut out = Vec::new();
    for (slot, mut cells) in cells {
        cells.sort_by_key(|c| std::cmp::Reverse(c.votes));
        let top = cells[0].votes;
        let (stretch, pitch) = lock[&slot];
        let mut kept: Vec<Detection> = Vec::new();
        for c in cells.into_iter().take_while(|c| c.votes * 5 >= top * 2) {
            let mut pairs = c.pairs;
            pairs.sort_unstable();
            pairs.dedup();
            let d = Detection {
                slot,
                slice,
                variant: c.variant,
                votes: c.votes,
                stretch_votes: stretch,
                pitch_votes: pitch,
                pairs,
            };
            // The same alignment found through a neighbouring speed is not a second candidate:
            // compare where each puts the track at the middle of the slice.
            let mid = (slice as f64 + 0.5) * SLICE as f64;
            let at_mid = |d: &Detection| {
                let (t, tau) = d.centre();
                tau + variants[d.variant as usize].ratio * (mid - t)
            };
            let here = at_mid(&d);
            if kept.iter().any(|k| (at_mid(k) - here).abs() < 1.5 * FRAMES_PER_SEC) {
                continue;
            }
            kept.push(d);
            if kept.len() == 3 {
                break;
            }
        }
        out.extend(kept);
    }
    out
}

/// One continuous stretch of a track's playback.
#[derive(Debug, Clone)]
pub struct Chain {
    pub slot: u32,
    pub detections: Vec<Detection>,
}

impl Chain {
    fn votes(&self) -> u32 {
        self.detections.iter().map(|d| d.votes).sum()
    }

    fn last(&self) -> &Detection {
        self.detections.last().expect("chains are never empty")
    }
}

/// Join each track's detections into chains (`detections` sorted by slot, then slice).
pub fn chain(detections: Vec<Detection>, variants: &[Variant]) -> Vec<Chain> {
    let mut chains: Vec<Chain> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut slot = u32::MAX;
    for d in detections {
        if d.slot != slot {
            slot = d.slot;
            open.clear();
        }
        let (t, tau) = d.centre();
        let fits = |c: &Chain| {
            let last = c.last();
            if last.slice == d.slice || d.slice - last.slice > CHAIN_GAP {
                return None;
            }
            let (lt, ltau) = last.centre();
            let ratio = variants[last.variant as usize].ratio;
            let miss = ((tau - ltau) - ratio * (t - lt)).abs() / FRAMES_PER_SEC;
            (miss <= CHAIN_TOLERANCE).then_some(miss)
        };
        let target = open
            .iter()
            .filter_map(|&i| fits(&chains[i]).map(|m| (i, m)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i);
        match target {
            Some(i) => chains[i].detections.push(d),
            None => {
                open.push(chains.len());
                chains.push(Chain { slot, detections: vec![d] });
            }
        }
    }
    chains
}

/// A straight line `track frame = intercept + ratio * mix frame` through a chain's pairs.
#[derive(Debug, Clone, Copy)]
pub struct Fit {
    pub ratio: f64,
    pub intercept: f64,
    /// Mix frames of the first and last matching pair.
    pub start: f64,
    pub end: f64,
    /// Whether the slope came from the data (else it's the searched speed).
    pub measured: bool,
}

impl Fit {
    pub fn track_frame(&self, mix_frame: f64) -> f64 {
        self.intercept + self.ratio * mix_frame
    }
}

fn fit(chain: &Chain, variants: &[Variant]) -> Fit {
    let mut pairs: Vec<(f64, f64)> =
        chain.detections.iter().flat_map(|d| d.pairs.iter().map(|&(t, tau)| (t as f64, tau as f64))).collect();
    let best = chain.detections.iter().max_by_key(|d| d.votes).expect("chains are never empty");
    let guess = variants[best.variant as usize].ratio;
    let line = |pairs: &[(f64, f64)]| -> (f64, f64) {
        let n = pairs.len() as f64;
        let (mt, mtau) = pairs.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0 / n, a.1 + p.1 / n));
        let (sxy, sxx) =
            pairs.iter().fold((0.0, 0.0), |a, p| (a.0 + (p.0 - mt) * (p.1 - mtau), a.1 + (p.0 - mt).powi(2)));
        let ratio = if sxx > 0.0 { sxy / sxx } else { guess };
        (ratio, mtau - ratio * mt)
    };
    let span = |pairs: &[(f64, f64)]| pairs.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
    // Too short a stretch to measure a slope: trust the searched speed.
    let (lo, hi) = span(&pairs);
    let measurable = hi - lo >= 8.0 * FRAMES_PER_SEC;
    let (mut ratio, mut intercept) = if measurable { line(&pairs) } else { (guess, 0.0) };
    if !measurable {
        let mut offsets: Vec<f64> = pairs.iter().map(|p| p.1 - guess * p.0).collect();
        offsets.sort_by(f64::total_cmp);
        intercept = offsets[offsets.len() / 2];
    }
    // Drop outliers (stray votes that landed in the cell) and refit.
    let mut residuals: Vec<f64> = pairs.iter().map(|p| (p.1 - intercept - ratio * p.0).abs()).collect();
    residuals.sort_by(f64::total_cmp);
    let limit = (residuals[residuals.len() / 2] * 4.0).max(3.0);
    pairs.retain(|p| (p.1 - intercept - ratio * p.0).abs() <= limit);
    if pairs.len() >= 2 && measurable {
        (ratio, intercept) = line(&pairs);
    }
    // A slope far outside the searched speeds means the fit locked onto noise.
    let plausible = (ratio - guess).abs() < 0.02;
    if !plausible {
        ratio = guess;
        let mut offsets: Vec<f64> = pairs.iter().map(|p| p.1 - guess * p.0).collect();
        offsets.sort_by(f64::total_cmp);
        intercept = offsets.get(offsets.len() / 2).copied().unwrap_or(intercept);
    }
    let (start, end) = if pairs.is_empty() { (lo, hi) } else { span(&pairs) };
    Fit { ratio, intercept, start, end, measured: measurable && plausible }
}

/// A track heard in the mix, before library metadata is attached.
#[derive(Debug, Clone)]
pub struct Found {
    /// The track's place in the [`LibraryIndex`]: its id is `index.track_ids[slot]`.
    pub slot: u32,
    /// Mix seconds.
    pub start: f64,
    pub end: f64,
    /// Track seconds at `start` and `end`.
    pub track_start: f64,
    pub track_end: f64,
    /// Playback speed (1.0 as recorded).
    pub tempo: f64,
    /// Whether key lock (master tempo) was on: `None` when the speed is too close to 1.0, or the
    /// votes too even, to tell.
    pub key_lock: Option<bool>,
    pub votes: u32,
    /// Votes of the play's strongest chain.
    pub strongest: u32,
    /// Slices heard, and slices between the first and last.
    pub heard: u32,
    pub spanned: u32,
    /// Matching anchors per [`PRESENCE_STEP`] from `start`, scaled to 0-1.
    pub presence: Vec<f32>,
    /// Loops and cue jumps: neighbouring chains whose track positions don't follow on (see `jumped`).
    pub jumps: u32,
    /// Other library entries matching the same stretch: (slot, same audio?).
    pub alternates: Vec<(u32, bool)>,
    /// The play's chains as lines through (mix frame, track frame), in mix order.
    pub fits: Vec<Fit>,
}

impl Found {
    pub fn duration(&self) -> f64 {
        self.end - self.start
    }

    /// Votes per second heard.
    fn density(&self) -> f64 {
        self.votes as f64 / self.duration().max(4.0)
    }

    fn overlap(&self, other: &Found) -> f64 {
        (self.end.min(other.end) - self.start.max(other.start)).max(0.0)
    }

    /// Pitch change from the speed, in semitones: 0 with key lock.
    pub fn semitones(&self) -> f64 {
        if self.key_lock == Some(true) { 0.0 } else { 12.0 * self.tempo.log2() }
    }

    /// How sure the match is, 0-1: from the votes per slice and how much of its stretch the track
    /// was heard in.
    pub fn confidence(&self) -> f64 {
        let density = self.votes as f64 / self.spanned.max(1) as f64;
        let continuity = self.heard as f64 / self.spanned.max(1) as f64;
        ((1.0 - (-density / 14.0).exp()) * continuity.sqrt()).clamp(0.0, 1.0)
    }
}

/// From chains to plays: drop noise, group, refine, then resolve tracks matching the same audio.
pub fn plays(chains: Vec<Chain>, variants: &[Variant]) -> Vec<Found> {
    let mut by_slot: FxHashMap<u32, Vec<(Chain, Fit)>> = FxHashMap::default();
    for c in chains {
        let votes = c.votes();
        // A lone detection or a weak chain is what a similar groove at the same tempo produces.
        if votes < CHAIN_VOTES || (c.detections.len() < CHAIN_DETECTIONS && votes < 2 * CHAIN_VOTES) {
            continue;
        }
        let f = fit(&c, variants);
        by_slot.entry(c.slot).or_default().push((c, f));
    }
    let mut found = Vec::new();
    for (slot, chains) in by_slot {
        let chains = merge_collinear(chains, variants);
        // Recurring bars (a repeated chorus, a second drop) line up a weaker chain alongside the
        // real one. One track can't be in two places at once: the stronger chain wins.
        let mut chains = chains;
        chains.sort_by_key(|c| std::cmp::Reverse(c.0.votes()));
        let mut kept: Vec<(Chain, Fit)> = Vec::new();
        for (c, f) in chains {
            let len = (f.end - f.start).max(1.0);
            let ghost = kept.iter().any(|(k, kf)| {
                let overlap = f.end.min(kf.end) - f.start.max(kf.start);
                overlap >= 0.5 * len
                    || ((overlap >= 0.3 * len || overlap >= GHOST_OVERLAP * FRAMES_PER_SEC)
                        && (c.votes() as f64) < 0.6 * k.votes() as f64)
            });
            if !ghost {
                kept.push((c, f));
            }
        }
        kept.sort_by(|a, b| a.1.start.total_cmp(&b.1.start));
        let mut group: Vec<(Chain, Fit)> = Vec::new();
        for (c, f) in kept {
            let end = group.iter().map(|g| g.1.end).fold(f64::MIN, f64::max);
            if !group.is_empty() && f.start - end > PLAY_GAP * FRAMES_PER_SEC {
                found.extend(play(slot, std::mem::take(&mut group)));
            }
            group.push((c, f));
        }
        found.extend(play(slot, group));
    }
    found
}

/// Join chains that are the same playback: split by a quiet patch, or found twice through
/// neighbouring speeds. Two chains are one when either's line predicts the other's ends.
fn merge_collinear(chains: Vec<(Chain, Fit)>, variants: &[Variant]) -> Vec<(Chain, Fit)> {
    let mut items = chains;
    let tolerance = MERGE_TOLERANCE * FRAMES_PER_SEC;
    loop {
        items.sort_by_key(|c| std::cmp::Reverse(c.0.votes()));
        let mut join = None;
        'search: for i in 0..items.len() {
            for j in i + 1..items.len() {
                let (a, b) = (&items[i].1, &items[j].1);
                let gap = (a.start.max(b.start) - a.end.min(b.end)).max(0.0);
                if gap > PLAY_GAP * FRAMES_PER_SEC {
                    continue;
                }
                // Predict with the line measured over the longer stretch.
                let (line, other) = if (a.end - a.start) >= (b.end - b.start) { (a, b) } else { (b, a) };
                let fits = [other.start, other.end]
                    .iter()
                    .all(|&t| (line.track_frame(t) - other.track_frame(t)).abs() <= tolerance);
                if fits {
                    join = Some((i, j));
                    break 'search;
                }
            }
        }
        let Some((i, j)) = join else { return items };
        let (other, _) = items.remove(j);
        let (into, _) = &mut items[i];
        for d in other.detections {
            match into.detections.iter_mut().find(|x| x.slice == d.slice) {
                Some(x) => {
                    if d.votes > x.votes {
                        x.votes = d.votes;
                        x.variant = d.variant;
                    }
                    x.pairs.extend(d.pairs);
                    x.pairs.sort_unstable();
                    x.pairs.dedup();
                }
                None => into.detections.push(d),
            }
        }
        into.detections.sort_by_key(|d| d.slice);
        let refit = fit(into, variants);
        items[i].1 = refit;
    }
}

fn play(slot: u32, chains: Vec<(Chain, Fit)>) -> Option<Found> {
    let votes: u32 = chains.iter().map(|(c, _)| c.votes()).sum();
    let heard: u32 = chains.iter().map(|(c, _)| c.detections.len() as u32).sum();
    let strongest = chains.iter().map(|(c, _)| c.votes()).max()?;
    if votes < PLAY_VOTES || strongest < PLAY_VOTES * 3 / 4 {
        return None;
    }
    let (first, last) = (
        chains.iter().min_by(|a, b| a.1.start.total_cmp(&b.1.start))?,
        chains.iter().max_by(|a, b| a.1.end.total_cmp(&b.1.end))?,
    );
    let (start, end) = (first.1.start, last.1.end);
    // Speed: measured slopes weighted by votes, else the searched speeds.
    let measured: Vec<(f64, f64)> =
        chains.iter().filter(|(_, f)| f.measured).map(|(c, f)| (f.ratio, c.votes() as f64)).collect();
    let pool: Vec<(f64, f64)> =
        if measured.is_empty() { chains.iter().map(|(c, f)| (f.ratio, c.votes() as f64)).collect() } else { measured };
    let weight: f64 = pool.iter().map(|p| p.1).sum();
    let tempo = pool.iter().map(|p| p.0 * p.1).sum::<f64>() / weight;

    let (stretch, pitch) = chains
        .iter()
        .flat_map(|(c, _)| &c.detections)
        .fold((0u64, 0u64), |a, d| (a.0 + d.stretch_votes as u64, a.1 + d.pitch_votes as u64));
    let key_lock = if (tempo - 1.0).abs() < 0.006 {
        None
    } else if stretch as f64 > pitch as f64 * 1.15 {
        Some(true)
    } else if pitch as f64 > stretch as f64 * 1.15 {
        Some(false)
    } else {
        None
    };

    let mut fits: Vec<Fit> = chains.iter().map(|(_, f)| *f).collect();
    fits.sort_by(|a, b| a.start.total_cmp(&b.start));
    let first_slice = chains.iter().map(|(c, _)| c.detections[0].slice).min()?;
    let last_slice = chains.iter().map(|(c, _)| c.last().slice).max()?;
    Some(Found {
        strongest,
        slot,
        start: start / FRAMES_PER_SEC,
        end: end / FRAMES_PER_SEC,
        track_start: first.1.track_frame(start).max(0.0) / FRAMES_PER_SEC,
        track_end: last.1.track_frame(end).max(0.0) / FRAMES_PER_SEC,
        tempo,
        key_lock,
        votes,
        heard,
        spanned: last_slice - first_slice + 1,
        presence: Vec::new(),
        jumps: fits.windows(2).filter(|w| jumped(&w[0], &w[1])).count() as u32,
        alternates: Vec::new(),
        fits,
    })
}

/// Whether the track jumps (a loop, a cue jump) between neighbouring chains `a` and `b` of a play.
/// Riding the pitch fader splits a play into chains at different speeds too, but there the track
/// carries on: halfway between them both lines put it in the same place, give or take the speed
/// change over the gap.
fn jumped(a: &Fit, b: &Fit) -> bool {
    let (mid, gap) = ((a.end + b.start) / 2.0, (b.start - a.end).abs());
    let slack = MERGE_TOLERANCE * FRAMES_PER_SEC + (a.ratio - b.ratio).abs() * gap / 2.0;
    (a.track_frame(mid) - b.track_frame(mid)).abs() > slack
}

/// Keep the strongest of plays that are really the same audio, or one track's material heard
/// inside another's (a remix and its original, a shared sample).
pub fn resolve(mut found: Vec<Found>) -> Vec<Found> {
    found.sort_by(|a, b| b.votes.cmp(&a.votes).then(a.slot.cmp(&b.slot)));
    let mut kept: Vec<Found> = Vec::new();
    'next: for f in found {
        for k in &mut kept {
            let overlap = f.overlap(k);
            if overlap <= 0.0 {
                continue;
            }
            let shorter = f.duration().min(k.duration()).max(1.0);
            // A duplicate file matches as strongly as the original; a remix sharing a vocal doesn't.
            let same_audio =
                overlap >= 0.6 * shorter && (f.tempo - k.tempo).abs() < 0.008 && f.density() >= 0.85 * k.density();
            let inside = overlap >= 0.6 * f.duration();
            if same_audio || inside {
                // Barely-there matches under a strong track are noise, not worth naming.
                let worth = same_audio || f.density() >= 0.35 * k.density();
                if worth && !k.alternates.iter().any(|a| a.0 == f.slot) && k.slot != f.slot {
                    k.alternates.push((f.slot, same_audio));
                }
                continue 'next;
            }
        }
        // Heard only underneath stronger tracks on either side: material they share.
        let covered: f64 = kept.iter().map(|k| f.overlap(k)).sum();
        let loudest = kept.iter().filter(|k| f.overlap(k) > 0.0).map(Found::density).fold(0.0, f64::max);
        if covered >= 0.8 * f.duration() && f.density() < 0.5 * loudest {
            continue;
        }
        kept.push(f);
    }
    kept.sort_by(|a, b| a.start.total_cmp(&b.start));
    kept
}

/// Where in the mix no track was found, as (start, end) seconds, ignoring gaps under `min`.
pub fn gaps(found: &[Found], duration: f64, min: f64) -> Vec<(f64, f64)> {
    let mut spans: Vec<(f64, f64)> = found.iter().map(|f| (f.start, f.end)).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut at = 0.0;
    for (s, e) in spans {
        if s - at >= min {
            out.push((at, s));
        }
        at = f64::max(at, e);
    }
    if duration - at >= min {
        out.push((at, duration));
    }
    out
}

/// Fraction of the mix covered by at least one play.
pub fn coverage(found: &[Found], duration: f64) -> f64 {
    if duration <= 0.0 {
        return 0.0;
    }
    let uncovered: f64 = gaps(found, duration, 0.0).iter().map(|(s, e)| e - s).sum();
    (1.0 - uncovered / duration).clamp(0.0, 1.0)
}

/// How far before and after the detected stretch to look for a fading track.
const EXTEND: f64 = 60.0;
/// Track frames a hit may be from where the play's line puts it.
const TRACE_TOLERANCE: f64 = 3.0;

/// Follow a play along its fitted lines: count every landmark that lands where the line says
/// (which needs far fewer votes than finding the track did), extend the play into the fades on
/// either side, and measure how clearly it's heard along the way.
fn trace(index: &LibraryIndex, landmarks: &[Landmark], variants: &[Variant], f: &mut Found) {
    let near: Vec<&Variant> = variants
        .iter()
        .filter(|v| {
            (v.ratio - f.tempo).abs() <= 0.013
                && match f.key_lock {
                    Some(true) => !v.pitch,
                    Some(false) => v.pitch || v.ratio == 1.0,
                    None => true,
                }
        })
        .collect();
    let track_len = index.durations.get(f.slot as usize).copied().unwrap_or(0.0) as f64 * FRAMES_PER_SEC;
    let (start, end) = (f.start * FRAMES_PER_SEC, f.end * FRAMES_PER_SEC);
    let lo = (start - EXTEND * FRAMES_PER_SEC).max(0.0);
    let hi = end + EXTEND * FRAMES_PER_SEC;
    let from = landmarks.partition_point(|l| (l.t as f64) < lo);
    let to = landmarks.partition_point(|l| (l.t as f64) < hi);
    let line = |t: f64| -> &Fit {
        f.fits
            .iter()
            .min_by(|a, b| {
                let d = |x: &Fit| {
                    if t < x.start {
                        x.start - t
                    } else if t > x.end {
                        t - x.end
                    } else {
                        0.0
                    }
                };
                d(a).total_cmp(&d(b))
            })
            .expect("plays have fits")
    };
    let mut hits: Vec<u32> = Vec::new();
    for lm in &landmarks[from..to] {
        let fit = line(lm.t as f64);
        let expect = fit.track_frame(lm.t as f64);
        if expect < -TRACE_TOLERANCE || (track_len > 0.0 && expect > track_len + TRACE_TOLERANCE) {
            continue;
        }
        let hit = near.iter().any(|v| {
            v.original_hash(lm).is_some_and(|h| {
                index
                    .bucket(h)
                    .iter()
                    .any(|&e| (e >> 32) as u32 == f.slot && ((e as u32) as f64 - expect).abs() <= TRACE_TOLERANCE)
            })
        });
        if hit && hits.last() != Some(&lm.t) {
            hits.push(lm.t);
        }
    }

    // Hits per presence bin over the search window.
    let step = PRESENCE_STEP * FRAMES_PER_SEC;
    let bins = ((hi - lo) / step).ceil() as usize;
    let mut counts = vec![0u32; bins];
    for &t in &hits {
        if let Some(c) = counts.get_mut(((t as f64 - lo) / step) as usize) {
            *c += 1;
        }
    }
    let bin_of = |frame: f64| (((frame - lo) / step) as usize).min(bins.saturating_sub(1));
    // Walk outwards while the track is still heard, forgiving one quiet bin.
    let walk = |from: usize, dir: isize| -> usize {
        let (mut at, mut edge, mut quiet) = (from as isize, from, 0);
        loop {
            at += dir;
            if at < 0 || at >= bins as isize {
                return edge;
            }
            if counts[at as usize] >= 2 {
                edge = at as usize;
                quiet = 0;
            } else {
                quiet += 1;
                if quiet > 1 {
                    return edge;
                }
            }
        }
    };
    let first = walk(bin_of(start), -1);
    let last = walk(bin_of(end), 1);
    let in_bins = |t: &&u32| {
        let b = ((**t as f64 - lo) / step) as usize;
        (first..=last).contains(&b)
    };
    let new_start = hits.iter().filter(in_bins).min().map_or(start, |&t| (t as f64).min(start));
    let new_end = hits.iter().filter(in_bins).max().map_or(end, |&t| (t as f64).max(end));

    // Presence: hits per bin from the new start, smoothed, relative to the clearest stretch.
    let n = (((new_end - new_start) / step).ceil() as usize).max(1);
    let mut presence = vec![0f32; n];
    for &t in &hits {
        let t = t as f64;
        if (new_start..=new_end).contains(&t) {
            presence[(((t - new_start) / step) as usize).min(n - 1)] += 1.0;
        }
    }
    let smoothed: Vec<f32> = (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(1), (i + 2).min(n));
            presence[a..b].iter().sum::<f32>() / (b - a) as f32
        })
        .collect();
    let top = smoothed.iter().copied().fold(0.0f32, f32::max).max(1.0);
    f.presence = smoothed.iter().map(|p| ((p / top).min(1.0) * 100.0).round() / 100.0).collect();
    f.start = new_start / FRAMES_PER_SEC;
    f.end = new_end / FRAMES_PER_SEC;
    f.track_start = line(new_start).track_frame(new_start).max(0.0) / FRAMES_PER_SEC;
    f.track_end = line(new_end).track_frame(new_end).max(0.0) / FRAMES_PER_SEC;
}

/// Run the whole pipeline over a mix's peaks, with the track in slot `itself` (the recording,
/// when the library has it too) left out: it would match the whole mix and swallow every track
/// heard in it.
pub fn find(
    index: &LibraryIndex,
    peaks: &[Peak],
    frames: u32,
    tempo_range: f64,
    itself: Option<u32>,
    ctx: &Ctx<'_>,
) -> Result<Vec<Found>> {
    if !(0.0..=0.25).contains(&tempo_range) {
        return Err(Error::Invalid(format!("tempo range {tempo_range} must be between 0 and 0.25")));
    }
    let variants = fingerprint::variants(tempo_range);
    let mut detections = detect(index, peaks, frames, &variants, ctx)?;
    detections.retain(|d| Some(d.slot) != itself);
    let chains = chain(detections, &variants);
    let mut found = plays(chains, &variants);
    let landmarks = fingerprint::landmarks(peaks, QUERY_FANOUT);
    std::thread::scope(|s| {
        for f in found.iter_mut() {
            let (landmarks, variants) = (&landmarks, &variants);
            s.spawn(move || trace(index, landmarks, variants, f));
        }
    });
    found.retain(|f| f.duration() >= PLAY_SECS);
    Ok(resolve(found))
}

#[cfg(test)]
mod tests {
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
                Detection {
                    slot: 1,
                    slice,
                    variant: variant as u16,
                    votes: 20,
                    stretch_votes: 0,
                    pitch_votes: 20,
                    pairs,
                }
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

    #[test]
    fn a_pitch_ride_carries_on_where_a_cue_jump_jumps() {
        let variants = fingerprint::variants(0.08);
        // 40 s at +0%, then the pitch fader up to +6%: the track carries on from where it was.
        let ride = plays(
            vec![chain_at(&variants, 0.0, 40.0, 10.0, 1.0), chain_at(&variants, 40.0, 80.0, 50.0, 1.06)],
            &variants,
        );
        assert_eq!(ride.iter().map(|p| (p.fits.len(), p.jumps)).collect::<Vec<_>>(), [(2, 0)], "{ride:#?}");
        // At one speed, but 20 s further on in the track after 40 s: a cue jump.
        let skip = plays(
            vec![chain_at(&variants, 0.0, 40.0, 10.0, 1.0), chain_at(&variants, 40.0, 80.0, 70.0, 1.0)],
            &variants,
        );
        assert_eq!(skip.iter().map(|p| (p.fits.len(), p.jumps)).collect::<Vec<_>>(), [(2, 1)], "{skip:#?}");
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
}
