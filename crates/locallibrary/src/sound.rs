//! A second opinion on tracks' tempo and key, and how loud each is, from their audio (through
//! loclib-analysis), cached on the computer like fingerprints (see [`crate::audio_cache`]).
//!
//! Doctor compares the library's stored values with the estimates ([`findings`]); the set planner
//! and the check before a set use what's already cached ([`key_confidences`],
//! [`Cache::cached`]) and never decode anything themselves.

pub mod bench;

use rustc_hash::FxHashMap;
use schemars::JsonSchema;
use serde::Serialize;

pub use loclib_analysis::benchmark::{KeyOutcome, Outcome, Report, TempoOutcome};
pub use loclib_analysis::tempo::{Plausible, TempoPrior, TempoRelation};
pub use loclib_analysis::{Analysis, Estimate, Loudness, VERSION};

use crate::audio_cache::{self, Kind, TrackFailure};
use crate::device::Device;
use crate::harmony::Camelot;
use crate::model::{Library, Track, TrackRef};
use crate::progress::{Ctx, Phase};
use crate::{Error, Result};

/// Analyses, in `<store>/cache/analysis`.
#[derive(Debug, Clone, Copy)]
pub struct Analyses;

impl Kind for Analyses {
    const DIR: &'static str = "analysis";
    const EXT: &'static str = "json";
    const WHAT: &'static str = "an analysis";
    const VERSION: u32 = loclib_analysis::VERSION;
    type Value = Analysis;

    fn encode(analysis: &Analysis) -> Vec<u8> {
        serde_json::to_vec(analysis).expect("an analysis serialises")
    }

    fn decode(bytes: &[u8]) -> Option<Analysis> {
        serde_json::from_slice(bytes).ok()
    }
}

/// Analyses on disk, one small file per audio file.
pub type Cache = audio_cache::Cache<Analyses>;

/// Everything loclib-analysis measures, so one cached analysis serves every use.
fn analyze_file(file: &std::path::Path, ctx: &Ctx<'_>) -> Result<Analysis> {
    loclib_analysis::analyze(file, loclib_analysis::Want::ALL, ctx).map_err(Error::from)
}

/// How sure a track's stored key is, given the cached estimate of it; `None` when they agree (or
/// either is unknown), which counts as sure.
///
/// When two analyzers disagree, at least one is wrong. Key detection's mistakes are mostly the
/// fifth-related, relative or parallel key (see loclib-analysis's README for the figures), so such a
/// disagreement most likely means the truth is one of the two: even odds, 0.5. Keys related in
/// none of those ways suggest a track neither can place (a weak or changing key), so the stored one
/// is trusted least, 0.25.
pub fn key_confidence(stored: &str, estimate: &Estimate) -> Option<f64> {
    let stored = Camelot::parse(stored)?;
    match KeyOutcome::of(stored, Some(estimate.key)) {
        KeyOutcome::Correct => None,
        KeyOutcome::Fifth | KeyOutcome::Relative | KeyOutcome::Parallel => Some(0.5),
        _ => Some(0.25),
    }
}

/// [`key_confidence`] for each of `tracks` with a cached analysis, by track id: nothing is decoded,
/// and tracks without one are left out (sure), as are those whose keys agree.
pub fn key_confidences<'t>(device: &Device, tracks: impl IntoIterator<Item = &'t Track>) -> FxHashMap<u32, f64> {
    // Without a store there's nothing cached (and no temporary cache worth making).
    let Some(store) = crate::api::registry() else { return FxHashMap::default() };
    let cache = Cache::in_store(&store);
    let tracks: Vec<&Track> = tracks.into_iter().collect();
    let cached = cache.cached(device, tracks.iter().copied());
    tracks
        .iter()
        .filter_map(|t| {
            let estimate = cached.get(&t.id)?.estimate.as_ref()?;
            Some((t.id, key_confidence(&t.key, estimate)?))
        })
        .collect()
}

/// A track whose stored tempo the estimate doesn't agree with.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TempoFinding {
    pub track: TrackRef,
    /// The library's BPM.
    pub stored: f64,
    /// The analyzer's BPM.
    pub estimate: f32,
    /// 0 to 1.
    pub confidence: f32,
    /// The stored tempo against the estimate: `half`, `double`, `two_thirds`, `three_halves` or
    /// `disagrees`.
    pub relation: TempoRelation,
    /// Which of the two the library's other tempos make more likely, if either.
    pub likelier: Option<Plausible>,
}

/// A track whose stored key the estimate doesn't agree with.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct KeyFinding {
    pub track: TrackRef,
    /// The library's key, as it stores it.
    pub stored: String,
    /// The analyzer's key, as a Camelot code (`8A`).
    pub estimate: String,
    /// 0 to 1.
    pub confidence: f32,
    /// The estimate against the stored key: `fifth`, `relative`, `parallel` or `other`.
    pub relation: KeyOutcome,
}

/// What may be wrong with a track's level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoudnessProblem {
    /// Its true peak is above 0 dBTP: a player's converter or a lossy encode can clip it.
    OverZero,
    /// Runs of samples at full scale: clipped when it was mastered or converted.
    Clipped,
    /// Its integrated loudness is more than [`FAR_FROM_MEDIAN_LU`] from the library's median.
    FarFromMedian,
}

/// How far (LU) a track's loudness can be from the library's median before it's reported. Masters
/// for the dance floor differ by a few LU (routine to match with a nudge of the trim); 6 LU is
/// about where the jump between tracks needs a correction that's hard to make mid-blend, and
/// what's left is mostly unmastered or badly converted files.
pub const FAR_FROM_MEDIAN_LU: f64 = 6.0;

/// A track whose level may be a problem.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LoudnessFinding {
    pub track: TrackRef,
    pub problems: Vec<LoudnessProblem>,
    /// LUFS (`None` for silence).
    pub integrated_lufs: Option<f64>,
    pub true_peak_dbtp: f64,
    pub clipped_runs: u64,
}

/// Tracks whose stored tempo or key the estimates question, and tracks whose level may be a
/// problem.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct AnalysisReport {
    /// Tracks with an analysis (cached or new).
    pub analyzed: usize,
    /// Analysed just now (the rest came from the cache).
    pub decoded: usize,
    /// Tracks whose audio couldn't be analysed.
    pub failed: Vec<TrackFailure>,
    pub tempo: Vec<TempoFinding>,
    pub key: Vec<KeyFinding>,
    pub loudness: Vec<LoudnessFinding>,
    /// The median integrated loudness of the analysed tracks (LUFS).
    pub median_lufs: Option<f64>,
}

/// What the analyses of `library`'s tracks (by id) say about its stored tempos and keys and its
/// tracks' levels. Which of a disagreeing pair of tempos is likelier comes from every stored tempo
/// in the library.
pub fn findings(library: &Library, analyses: &FxHashMap<u32, Analysis>) -> AnalysisReport {
    let prior = TempoPrior::library(library.tracks.iter().map(|t| t.bpm));
    let analysed: Vec<(&Track, &Analysis)> =
        library.tracks.iter().filter_map(|t| analyses.get(&t.id).map(|a| (t, a))).collect();
    let mut report = AnalysisReport {
        analyzed: analysed.len(),
        loudness: loudness_findings(analysed.iter().map(|(t, a)| (*t, a.loudness.as_ref()))),
        ..Default::default()
    };
    report.median_lufs = median_lufs(analysed.iter().filter_map(|(_, a)| a.loudness.as_ref()));
    for (t, a) in analysed {
        let Some(e) = &a.estimate else { continue };
        if let Some(relation) = TempoRelation::between(t.bpm, e.bpm.into()).filter(|r| *r != TempoRelation::Agrees) {
            report.tempo.push(TempoFinding {
                track: t.into(),
                stored: t.bpm,
                estimate: e.bpm,
                confidence: e.bpm_confidence,
                relation,
                likelier: prior.more_plausible(t.bpm, e.bpm.into()),
            });
        }
        if let Some(stored) = Camelot::parse(&t.key) {
            let relation = KeyOutcome::of(stored, Some(e.key));
            if relation != KeyOutcome::Correct {
                report.key.push(KeyFinding {
                    track: t.into(),
                    stored: t.key.clone(),
                    estimate: e.key.code(),
                    confidence: e.key_confidence,
                    relation,
                });
            }
        }
    }
    report
}

fn median_lufs<'a>(levels: impl Iterator<Item = &'a Loudness>) -> Option<f64> {
    let mut lufs: Vec<f64> = levels.filter_map(|l| l.integrated_lufs).collect();
    lufs.sort_by(f64::total_cmp);
    let mid = lufs.len() / 2;
    match lufs.len() {
        0 => None,
        n if n % 2 == 1 => Some(lufs[mid]),
        _ => Some((lufs[mid - 1] + lufs[mid]) / 2.0),
    }
}

/// Tracks over 0 dBTP, clipped, or far from the median loudness of `tracks`.
pub fn loudness_findings<'a>(
    tracks: impl Iterator<Item = (&'a Track, Option<&'a Loudness>)> + Clone,
) -> Vec<LoudnessFinding> {
    let median = median_lufs(tracks.clone().filter_map(|(_, l)| l));
    tracks
        .filter_map(|(t, l)| {
            let l = l?;
            let far = matches!((l.integrated_lufs, median), (Some(x), Some(m)) if (x - m).abs() > FAR_FROM_MEDIAN_LU);
            let problems: Vec<LoudnessProblem> = [
                (l.true_peak_dbtp() > 0.0, LoudnessProblem::OverZero),
                (l.clipped_runs > 0, LoudnessProblem::Clipped),
                (far, LoudnessProblem::FarFromMedian),
            ]
            .into_iter()
            .filter_map(|(found, p)| found.then_some(p))
            .collect();
            (!problems.is_empty()).then(|| LoudnessFinding {
                track: t.into(),
                problems,
                integrated_lufs: l.integrated_lufs,
                true_peak_dbtp: l.true_peak_dbtp(),
                clipped_runs: l.clipped_runs,
            })
        })
        .collect()
}

/// Analyse `library`'s tracks (decoding those not cached yet) and report what the analyses say,
/// for doctor.
pub(crate) fn analyze_library(device: &Device, library: &Library, ctx: &Ctx<'_>) -> Result<AnalysisReport> {
    let (cache, _temp) = Cache::of_default_store()?;
    let tracks: Vec<&Track> = library.tracks.iter().collect();
    let run = audio_cache::for_tracks(device, &tracks, &cache, Phase::Analyze, analyze_file, ctx)?;
    let analyses: FxHashMap<u32, Analysis> = run.done.into_iter().map(|d| (d.id, d.value)).collect();
    Ok(AnalysisReport { decoded: run.computed, failed: run.failed, ..findings(library, &analyses) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard(key: &str) -> Estimate {
        Estimate { bpm: 128.0, bpm_confidence: 1.0, key: Camelot::parse(key).unwrap(), key_confidence: 1.0 }
    }

    #[test]
    fn keys_the_analysis_questions_are_less_sure() {
        assert_eq!(key_confidence("Am", &heard("8A")), None);
        assert_eq!(key_confidence("8A", &heard("8B")), Some(0.5));
        assert_eq!(key_confidence("8A", &heard("11B")), Some(0.5));
        assert_eq!(key_confidence("8A", &heard("9A")), Some(0.5));
        assert_eq!(key_confidence("8A", &heard("2B")), Some(0.25));
        assert_eq!(key_confidence("", &heard("8A")), None);
    }

    #[test]
    fn the_median_of_an_even_count_is_between_the_middle_two() {
        let level = |lufs| Loudness {
            integrated_lufs: lufs,
            range_lu: 0.0,
            sample_peak: 0.5,
            true_peak: 0.5,
            clipped_runs: 0,
            clipped_samples: 0,
        };
        let levels = [level(Some(-8.0)), level(Some(-10.0)), level(None), level(Some(-6.0)), level(Some(-12.0))];
        assert_eq!(median_lufs(levels.iter()), Some(-9.0));
        assert_eq!(median_lufs(levels[..2].iter().chain(&levels[3..4])), Some(-8.0));
        assert_eq!(median_lufs(levels[2..3].iter()), None);
    }
}
