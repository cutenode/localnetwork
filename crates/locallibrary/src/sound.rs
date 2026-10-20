//! A second opinion on tracks' tempo and key, and how loud each is, from what the analyzers
//! chosen for those roles worked out (see [`crate::fields::roles`]). Nothing is analysed here:
//! the app runs the analyzers first (as `doctor --analyze` does), and the values are read from the
//! store.
//!
//! Doctor compares the library's stored values with the estimates ([`report`]); the set planner
//! and the check before a set use what's kept already ([`key_confidences`], [`levels`]).

pub mod bench;

use rustc_hash::FxHashMap;
use schemars::JsonSchema;
use serde::Serialize;

pub use loclib_analysis::benchmark::{KeyOutcome, Outcome, Report, TempoOutcome};
pub use loclib_analysis::tempo::{Plausible, TempoPrior, TempoRelation};

use crate::audio_cache::TrackFailure;
use crate::device::Device;
use crate::fields::roles::RoleValues;
use crate::fields::{FieldValue, Role, by_role};
use crate::harmony::Camelot;
use crate::model::{Library, Track, TrackRef};
use crate::store::Store;

/// The roles doctor's analysis reads.
pub const ROLES: [Role; 7] =
    [Role::Bpm, Role::BpmConfidence, Role::Key, Role::KeyConfidence, Role::Loudness, Role::TruePeak, Role::ClippedRuns];

/// The roles a track's [`Level`] is made of.
const LEVEL: [Role; 3] = [Role::Loudness, Role::TruePeak, Role::ClippedRuns];

/// A role's value as a number, if it's one.
fn number(heard: &RoleValues, role: Role) -> Option<f64> {
    match heard.get(&role)? {
        FieldValue::Number(n) => Some(*n),
        FieldValue::Integer(n) => Some(*n as f64),
        _ => None,
    }
}

/// The key a track was heard in, if it was.
fn heard_key(heard: &RoleValues) -> Option<Camelot> {
    match heard.get(&Role::Key)? {
        FieldValue::String(code) => Camelot::parse(code),
        _ => None,
    }
}

/// How sure a track's stored key is, given the key it was heard in; `None` when they agree (or
/// the stored one isn't a key), which counts as sure.
///
/// When two analyzers disagree, at least one is wrong. Key detection's mistakes are mostly the
/// fifth-related, relative or parallel key (see loclib-analysis's README for the figures), so such a
/// disagreement most likely means the truth is one of the two: even odds, 0.5. Keys related in
/// none of those ways suggest a track neither can place (a weak or changing key), so the stored one
/// is trusted least, 0.25.
pub fn key_confidence(stored: &str, heard: Camelot) -> Option<f64> {
    let stored = Camelot::parse(stored)?;
    match KeyOutcome::of(stored, Some(heard)) {
        KeyOutcome::Correct => None,
        KeyOutcome::Fifth | KeyOutcome::Relative | KeyOutcome::Parallel => Some(0.5),
        _ => Some(0.25),
    }
}

/// [`key_confidence`] for each of `tracks` heard in a key already (its `key` role, kept in the
/// store), by track id: nothing is analysed, and tracks not heard are left out (sure), as are
/// those whose keys agree.
pub fn key_confidences<'t>(device: &Device, tracks: impl IntoIterator<Item = &'t Track>) -> FxHashMap<u32, f64> {
    // Without a store nothing was kept.
    let Some(store) = crate::api::registry() else { return FxHashMap::default() };
    let tracks: Vec<&Track> = tracks.into_iter().collect();
    let heard = by_role(&store, &[Role::Key], device, &tracks);
    tracks.iter().filter_map(|t| Some((t.id, key_confidence(&t.key, heard_key(heard.get(&t.id)?)?)?))).collect()
}

/// A track whose stored tempo the estimate doesn't agree with.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TempoFinding {
    pub track: TrackRef,
    /// The library's BPM.
    pub stored: f64,
    /// The analyzer's BPM.
    pub estimate: f32,
    /// 0 to 1 (none: the analyzer doesn't say).
    pub confidence: Option<f32>,
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
    /// 0 to 1 (none: the analyzer doesn't say).
    pub confidence: Option<f32>,
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

/// A track's level, as its loudness, true peak and clipped runs roles have it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Level {
    /// LUFS (`None` for silence).
    pub integrated_lufs: Option<f64>,
    /// Minus infinity for silence, which has no peak.
    pub true_peak_dbtp: f64,
    pub clipped_runs: u64,
}

impl Level {
    /// A track's, from its role values: `None` when none of the three was worked out.
    fn of(heard: &RoleValues) -> Option<Level> {
        LEVEL.iter().any(|r| heard.contains_key(r)).then(|| Level {
            integrated_lufs: number(heard, Role::Loudness),
            true_peak_dbtp: number(heard, Role::TruePeak).unwrap_or(f64::NEG_INFINITY),
            clipped_runs: number(heard, Role::ClippedRuns).map_or(0, |n| n.max(0.0) as u64),
        })
    }
}

/// The levels kept for each of `tracks` (of `device`) in `store`, in their order: those with
/// none are left out. Nothing is analysed.
pub fn levels<'t>(store: &Store, device: &Device, tracks: &[&'t Track]) -> Vec<(&'t Track, Level)> {
    let heard = by_role(store, &LEVEL, device, tracks);
    tracks.iter().filter_map(|t| Some((*t, Level::of(heard.get(&t.id)?)?))).collect()
}

/// Tracks whose stored tempo or key the estimates question, and tracks whose level may be a
/// problem.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct AnalysisReport {
    /// Tracks with an analysis (kept from before, or new).
    pub analyzed: usize,
    /// Analysed just now (the rest were kept from before).
    pub decoded: usize,
    /// Tracks whose audio couldn't be analysed.
    pub failed: Vec<TrackFailure>,
    pub tempo: Vec<TempoFinding>,
    pub key: Vec<KeyFinding>,
    pub loudness: Vec<LoudnessFinding>,
    /// The median integrated loudness of the analysed tracks (LUFS).
    pub median_lufs: Option<f64>,
}

/// What `heard` (each analysed track's [`ROLES`], by id) says about `library`'s stored tempos and
/// keys and its tracks' levels. Which of a disagreeing pair of tempos is likelier comes from
/// every stored tempo in the library. Reports nothing analysed just now, and no failures.
pub fn findings(library: &Library, heard: &FxHashMap<u32, RoleValues>) -> AnalysisReport {
    let prior = TempoPrior::library(library.tracks.iter().map(|t| t.bpm));
    let analysed: Vec<(&Track, &RoleValues)> =
        library.tracks.iter().filter_map(|t| heard.get(&t.id).map(|h| (t, h))).collect();
    let levels: Vec<(&Track, Level)> = analysed.iter().filter_map(|(t, h)| Some((*t, Level::of(h)?))).collect();
    let mut report = AnalysisReport {
        analyzed: analysed.len(),
        loudness: loudness_findings(&levels),
        median_lufs: median_lufs(levels.iter().map(|(_, l)| l)),
        ..Default::default()
    };
    for (t, h) in analysed {
        // As the analyzer worked them out: sound's are single precision.
        let single = |role| number(h, role).map(|n| n as f32);
        if let Some(bpm) = number(h, Role::Bpm)
            && let Some(relation) = TempoRelation::between(t.bpm, bpm).filter(|r| *r != TempoRelation::Agrees)
        {
            report.tempo.push(TempoFinding {
                track: t.into(),
                stored: t.bpm,
                estimate: bpm as f32,
                confidence: single(Role::BpmConfidence),
                relation,
                likelier: prior.more_plausible(t.bpm, bpm),
            });
        }
        if let (Some(stored), Some(key)) = (Camelot::parse(&t.key), heard_key(h)) {
            let relation = KeyOutcome::of(stored, Some(key));
            if relation != KeyOutcome::Correct {
                report.key.push(KeyFinding {
                    track: t.into(),
                    stored: t.key.clone(),
                    estimate: key.code(),
                    confidence: single(Role::KeyConfidence),
                    relation,
                });
            }
        }
    }
    report
}

fn median_lufs<'a>(levels: impl Iterator<Item = &'a Level>) -> Option<f64> {
    let mut lufs: Vec<f64> = levels.filter_map(|l| l.integrated_lufs).collect();
    lufs.sort_by(f64::total_cmp);
    let mid = lufs.len() / 2;
    match lufs.len() {
        0 => None,
        n if n % 2 == 1 => Some(lufs[mid]),
        _ => Some((lufs[mid - 1] + lufs[mid]) / 2.0),
    }
}

/// Tracks over 0 dBTP, clipped, or far from the median loudness of `levels`.
pub fn loudness_findings(levels: &[(&Track, Level)]) -> Vec<LoudnessFinding> {
    let median = median_lufs(levels.iter().map(|(_, l)| l));
    levels
        .iter()
        .filter_map(|(t, l)| {
            let far = matches!((l.integrated_lufs, median), (Some(x), Some(m)) if (x - m).abs() > FAR_FROM_MEDIAN_LU);
            let problems: Vec<LoudnessProblem> = [
                (l.true_peak_dbtp > 0.0, LoudnessProblem::OverZero),
                (l.clipped_runs > 0, LoudnessProblem::Clipped),
                (far, LoudnessProblem::FarFromMedian),
            ]
            .into_iter()
            .filter_map(|(found, p)| found.then_some(p))
            .collect();
            (!problems.is_empty()).then(|| LoudnessFinding {
                track: (*t).into(),
                problems,
                integrated_lufs: l.integrated_lufs,
                true_peak_dbtp: l.true_peak_dbtp,
                clipped_runs: l.clipped_runs,
            })
        })
        .collect()
}

/// What the [`ROLES`] kept in `store` say about `library`'s tracks on `device`, for doctor:
/// nothing is analysed (the app analyses them first, and says what it did in `decoded` and
/// `failed`).
pub fn report(store: &Store, device: &Device, library: &Library) -> AnalysisReport {
    let tracks: Vec<&Track> = library.tracks.iter().collect();
    findings(library, &by_role(store, &ROLES, device, &tracks))
}

#[cfg(test)]
mod tests;
