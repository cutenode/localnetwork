//! Whether a stored tempo and an estimate agree, or are off by the factors tempo estimators get
//! wrong most: half and double time (octave errors), and sometimes 3/2 or 2/3.

use serde::{Deserialize, Serialize};

/// How far apart two tempos can be and still count as the same, 4%: the window GiantSteps'
/// Accuracy1 uses (Gouyon et al. 2006, Knees et al. 2015). The 4% windows round 1, 3/2 and 2
/// don't overlap, so a pair of tempos is in one class at most.
pub const TOLERANCE: f64 = 0.04;

/// A stored tempo against an estimate of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TempoRelation {
    Agrees,
    /// The stored tempo is half the estimate.
    Half,
    /// The stored tempo is double the estimate.
    Double,
    /// The stored tempo is two thirds of the estimate.
    TwoThirds,
    /// The stored tempo is three halves of the estimate.
    ThreeHalves,
    Disagrees,
}

impl TempoRelation {
    /// How `stored` relates to `estimate`, both in BPM; `None` unless both are positive.
    ///
    /// Neither is ground truth, so the tolerance is symmetric (a ratio within 4% either way, in
    /// log terms) rather than relative to one of them as in a benchmark: swapping the two gives
    /// the opposite relation exactly (half for double, 2/3 for 3/2).
    pub fn between(stored: f64, estimate: f64) -> Option<TempoRelation> {
        let positive = |bpm: f64| bpm > 0.0 && bpm.is_finite();
        if !positive(stored) || !positive(estimate) {
            return None;
        }
        let apart = stored.ln() - estimate.ln();
        let (octave, fifth, tolerance) = (2f64.ln(), 1.5f64.ln(), (1.0 + TOLERANCE).ln());
        let relations = [
            (TempoRelation::Agrees, 0.0),
            (TempoRelation::Double, octave),
            (TempoRelation::Half, -octave),
            (TempoRelation::ThreeHalves, fifth),
            (TempoRelation::TwoThirds, -fifth),
        ];
        Some(
            relations
                .into_iter()
                .find(|(_, ln)| (apart - ln).abs() <= tolerance)
                .map_or(TempoRelation::Disagrees, |r| r.0),
        )
    }
}

/// What tempos are likely for a track, from the tempos already in a library.
#[derive(Debug, Clone)]
pub struct TempoPrior {
    /// Sorted, positive.
    sorted: Vec<f64>,
}

/// Which of two tempos a [`TempoPrior`] finds more likely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Plausible {
    Stored,
    Estimate,
}

impl TempoPrior {
    /// The tempos of a library's tracks (zero and other unknown tempos left out).
    pub fn library(bpms: impl IntoIterator<Item = f64>) -> TempoPrior {
        let mut sorted: Vec<f64> = bpms.into_iter().filter(|b| *b > 0.0 && b.is_finite()).collect();
        sorted.sort_by(f64::total_cmp);
        TempoPrior { sorted }
    }

    /// How likely `bpm` is, from 0 to 1: the share of the library's tracks within [`TOLERANCE`].
    pub fn plausibility(&self, bpm: f64) -> f64 {
        if !(bpm > 0.0 && bpm.is_finite()) || self.sorted.is_empty() {
            return 0.0;
        }
        let (low, high) = (bpm / (1.0 + TOLERANCE), bpm * (1.0 + TOLERANCE));
        let near = self.sorted.partition_point(|b| *b <= high) - self.sorted.partition_point(|b| *b < low);
        near as f64 / self.sorted.len() as f64
    }

    /// Which of `stored` and `estimate` is more plausible; `None` when the prior can't tell them
    /// apart.
    pub fn more_plausible(&self, stored: f64, estimate: f64) -> Option<Plausible> {
        let (s, e) = (self.plausibility(stored), self.plausibility(estimate));
        match s.total_cmp(&e) {
            std::cmp::Ordering::Greater => Some(Plausible::Stored),
            std::cmp::Ordering::Less => Some(Plausible::Estimate),
            std::cmp::Ordering::Equal => None,
        }
    }
}

#[cfg(test)]
mod tests;
