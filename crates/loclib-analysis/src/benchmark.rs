//! Scoring key and tempo analyzers against annotated datasets, the way published results are.
//!
//! The annotation formats are the GiantSteps datasets' (Beatport previews of electronic dance
//! music, the usual benchmark for DJ software):
//!
//! - [GiantSteps Key](https://github.com/GiantSteps/giantsteps-key-dataset), 604 tracks:
//!   `annotations/key/<id>.LOFI.key`, each one key such as `C minor` ([`parse_key`]).
//! - [GiantSteps MTG Key](https://github.com/GiantSteps/giantsteps-mtg-key-dataset), 1,486
//!   tracks: one tab-separated `annotations/annotations.txt` ([`parse_mtg_keys`]).
//! - [GiantSteps Tempo](https://github.com/GiantSteps/giantsteps-tempo-dataset), 664 tracks:
//!   `annotations/tempo/<id>.LOFI.bpm` (Knees et al., ISMIR 2015) and the revised
//!   `annotations_v2/tempo/<id>.LOFI.bpm` (Schreiber and Müller, ISMIR 2018), each one tempo
//!   ([`parse_tempo`]).
//!
//! Keys are scored as MIREX does ([`KeyOutcome`]), tempos with Accuracy1 and Accuracy2
//! ([`TempoOutcome`]).

use std::hash::Hash;

use indexmap::IndexMap;
use loclib_model::harmony::Camelot;
use serde::Serialize;

use crate::tempo::TOLERANCE;
use crate::{Error, Result};

/// One GiantSteps Key annotation: a key such as `C minor` or `D# major`.
pub fn parse_key(text: &str) -> Result<Camelot> {
    Camelot::parse(text).ok_or_else(|| Error::annotation(1, format!("{:?} isn't a key", text.trim())))
}

/// One GiantSteps Tempo annotation, v1 or v2: a tempo in BPM (`126` or `127.0`). v2 has no tempo
/// for three tracks and writes 0.0 for them: `None`.
pub fn parse_tempo(text: &str) -> Result<Option<f64>> {
    let bpm: f64 = text.trim().parse().map_err(|_| Error::annotation(1, format!("{:?} isn't a tempo", text.trim())))?;
    if !(bpm >= 0.0 && bpm.is_finite()) {
        return Err(Error::annotation(1, format!("{bpm} isn't a tempo")));
    }
    Ok((bpm > 0.0).then_some(bpm))
}

/// A track in the GiantSteps MTG Key dataset.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MtgKey {
    pub id: String,
    /// Empty when the annotators heard no key (`-`), more than one when it changes or they
    /// disagreed (`F minor / G# major`).
    pub keys: Vec<Camelot>,
    /// The annotators' confidence: 0, 1 or 2. Results are usually reported on the tracks with
    /// one key and confidence 2.
    pub confidence: u8,
}

/// The GiantSteps MTG Key dataset's `annotations.txt`: a header (`ID`, `MANUAL KEY`, `C`), then
/// one tab-separated row a track.
pub fn parse_mtg_keys(text: &str) -> Result<Vec<MtgKey>> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty() && !line.starts_with("ID\t"))
        .map(|(i, line)| {
            let fields: Vec<&str> = line.split('\t').map(str::trim).collect();
            let [id, keys, confidence] = fields[..] else {
                return Err(Error::annotation(i + 1, "expected an id, a key and a confidence"));
            };
            let confidence =
                confidence.parse().map_err(|_| Error::annotation(i + 1, "the confidence isn't 0, 1 or 2"))?;
            Ok(MtgKey {
                id: id.to_owned(),
                keys: mtg_keys(keys).ok_or_else(|| Error::annotation(i + 1, "unreadable key"))?,
                confidence,
            })
        })
        .collect()
}

/// `-`, `C minor`, `F minor / G# major`, or `G minor/major` (the second key on the first's tonic).
fn mtg_keys(field: &str) -> Option<Vec<Camelot>> {
    if field == "-" {
        return Some(Vec::new());
    }
    let tonic = field.split_whitespace().next()?;
    field
        .split('/')
        .map(|key| Camelot::parse(key).or_else(|| Camelot::parse(&format!("{tonic} {}", key.trim()))))
        .collect()
}

/// How an analyzer's answer for one track compares with the annotation.
pub trait Outcome: Copy + Eq + Hash + Serialize + 'static {
    /// What's annotated and estimated: a key, a tempo.
    type Value;
    /// Every outcome, in the order reports list them.
    const ALL: &'static [Self];
    fn of(reference: Self::Value, estimate: Option<Self::Value>) -> Self;
    /// What the outcome scores, 0 to 1: a report's [`Report::score`] is the mean.
    fn credit(self) -> f64;
}

/// MIREX's key categories, from the [Audio Key Detection task](https://music-ir.org/mirex/wiki/2021:Audio_Key_Detection).
/// A fifth counts either way round, as MIREX has scored it since 2017 (mir_eval's
/// `allow_descending_fifths`, [key.py](https://github.com/mir-evaluation/mir_eval/blob/main/mir_eval/key.py)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum KeyOutcome {
    Correct,
    /// A fifth above or below in the same mode: a neighbour on the Camelot wheel.
    Fifth,
    /// Major for its relative minor or back: same Camelot number.
    Relative,
    /// Major for minor on the same tonic.
    Parallel,
    Other,
    /// The analyzer gave no key.
    Missing,
}

impl Outcome for KeyOutcome {
    type Value = Camelot;
    const ALL: &'static [KeyOutcome] = &[
        KeyOutcome::Correct,
        KeyOutcome::Fifth,
        KeyOutcome::Relative,
        KeyOutcome::Parallel,
        KeyOutcome::Other,
        KeyOutcome::Missing,
    ];

    fn of(reference: Camelot, estimate: Option<Camelot>) -> KeyOutcome {
        let Some(estimate) = estimate else { return KeyOutcome::Missing };
        let steps = (i32::from(estimate.number) - i32::from(reference.number)).rem_euclid(12);
        match () {
            _ if estimate == reference => KeyOutcome::Correct,
            _ if estimate.major == reference.major && (steps == 1 || steps == 11) => KeyOutcome::Fifth,
            _ if estimate == reference.relative() => KeyOutcome::Relative,
            _ if estimate == reference.parallel() => KeyOutcome::Parallel,
            _ => KeyOutcome::Other,
        }
    }

    /// MIREX's weights: correct 1, fifth 0.5, relative 0.3, parallel 0.2, anything else 0.
    fn credit(self) -> f64 {
        match self {
            KeyOutcome::Correct => 1.0,
            KeyOutcome::Fifth => 0.5,
            KeyOutcome::Relative => 0.3,
            KeyOutcome::Parallel => 0.2,
            KeyOutcome::Other | KeyOutcome::Missing => 0.0,
        }
    }
}

/// A tempo estimate against the annotation, within [`TOLERANCE`] (4%) of the annotation times the
/// factor (Gouyon et al. 2006). Accuracy1 counts [`TempoOutcome::Correct`]; Accuracy2 also counts
/// the double, half, triple and third tempos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TempoOutcome {
    Correct,
    /// The estimate is double the annotated tempo.
    Double,
    /// The estimate is half the annotated tempo.
    Half,
    /// The estimate is three times the annotated tempo.
    Triple,
    /// The estimate is a third of the annotated tempo.
    Third,
    Other,
    /// The analyzer gave no tempo.
    Missing,
}

impl TempoOutcome {
    /// Whether Accuracy2 counts it.
    pub fn accuracy2(self) -> bool {
        !matches!(self, TempoOutcome::Other | TempoOutcome::Missing)
    }
}

impl Outcome for TempoOutcome {
    type Value = f64;
    const ALL: &'static [TempoOutcome] = &[
        TempoOutcome::Correct,
        TempoOutcome::Double,
        TempoOutcome::Half,
        TempoOutcome::Triple,
        TempoOutcome::Third,
        TempoOutcome::Other,
        TempoOutcome::Missing,
    ];

    fn of(reference: f64, estimate: Option<f64>) -> TempoOutcome {
        let Some(estimate) = estimate.filter(|e| *e > 0.0 && e.is_finite()) else { return TempoOutcome::Missing };
        let factors = [
            (TempoOutcome::Correct, 1.0),
            (TempoOutcome::Double, 2.0),
            (TempoOutcome::Half, 0.5),
            (TempoOutcome::Triple, 3.0),
            (TempoOutcome::Third, 1.0 / 3.0),
        ];
        factors
            .into_iter()
            .find(|(_, f)| (estimate - f * reference).abs() <= TOLERANCE * f * reference)
            .map_or(TempoOutcome::Other, |(o, _)| o)
    }

    /// Accuracy1: 1 for the right tempo.
    fn credit(self) -> f64 {
        f64::from(u8::from(self == TempoOutcome::Correct))
    }
}

/// One analyzer's answer for one annotated track.
#[derive(Debug, Clone, PartialEq)]
pub struct Row<T> {
    /// Which analyzer answered: `rekordbox`, `stratum-dsp`.
    pub analyzer: String,
    pub track: String,
    pub reference: T,
    pub estimate: Option<T>,
}

/// How one analyzer did.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema), schemars(rename = "{O}Report"))]
pub struct Report<O: Outcome> {
    pub tracks: usize,
    /// Tracks with each outcome, every outcome listed.
    pub counts: IndexMap<O, usize>,
    /// The tracks it didn't get fully right, in the order they came, with what it did instead.
    pub misses: Vec<(String, O)>,
    /// The mean credit, 0 to 1: MIREX's weighted key score, or tempo Accuracy1.
    pub score: f64,
}

impl<O: Outcome> Report<O> {
    fn new() -> Report<O> {
        Report { tracks: 0, counts: O::ALL.iter().map(|o| (*o, 0)).collect(), misses: Vec::new(), score: 0.0 }
    }

    pub fn count(&self, outcome: O) -> usize {
        self.counts[&outcome]
    }

    /// The share of tracks with `outcome`, in percent (0 with no tracks).
    pub fn percent(&self, outcome: O) -> f64 {
        percent(self.count(outcome), self.tracks)
    }
}

impl Report<TempoOutcome> {
    /// Accuracy2, in percent.
    pub fn accuracy2(&self) -> f64 {
        let hits = self.counts.iter().filter(|(o, _)| o.accuracy2()).map(|(_, n)| n).sum();
        percent(hits, self.tracks)
    }
}

fn percent(n: usize, of: usize) -> f64 {
    if of == 0 { 0.0 } else { 100.0 * n as f64 / of as f64 }
}

/// Score `rows`: a report for each analyzer, in the order they first appear.
///
/// ```
/// use loclib_analysis::benchmark::{KeyOutcome, Row, score};
/// use loclib_model::harmony::Camelot;
///
/// let key = |k| Camelot::parse(k);
/// let row = |analyzer: &str, track: &str, reference, estimate| Row {
///     analyzer: analyzer.into(),
///     track: track.into(),
///     reference: key(reference).unwrap(),
///     estimate: key(estimate),
/// };
/// let reports = score::<KeyOutcome>([
///     row("rekordbox", "1", "Am", "Am"),
///     row("rekordbox", "2", "Am", "C"),
///     row("stratum-dsp", "1", "Am", "Em"),
///     row("stratum-dsp", "2", "Am", "?"),
/// ]);
/// assert_eq!(reports["rekordbox"].count(KeyOutcome::Relative), 1);
/// assert_eq!(reports["rekordbox"].score, (1.0 + 0.3) / 2.0);
/// assert_eq!(reports["stratum-dsp"].misses[1], ("2".to_string(), KeyOutcome::Missing));
/// ```
pub fn score<O: Outcome>(rows: impl IntoIterator<Item = Row<O::Value>>) -> IndexMap<String, Report<O>> {
    let mut reports: IndexMap<String, Report<O>> = IndexMap::new();
    for row in rows {
        let outcome = O::of(row.reference, row.estimate);
        let report = reports.entry(row.analyzer).or_insert_with(Report::new);
        report.tracks += 1;
        // A running mean, so the report is whole after every row.
        report.score += (outcome.credit() - report.score) / report.tracks as f64;
        *report.counts.get_mut(&outcome).expect("every outcome is counted") += 1;
        if outcome.credit() < 1.0 {
            report.misses.push((row.track, outcome));
        }
    }
    reports
}

#[cfg(test)]
mod tests {
    use super::*;
    use hegel::generators as gs;

    fn key(s: &str) -> Camelot {
        Camelot::parse(s).unwrap()
    }

    #[test]
    fn reads_giantsteps_annotations() {
        assert_eq!(parse_key("C minor").unwrap(), key("5A"));
        assert_eq!(parse_key("Eb major\n").unwrap(), key("5B"));
        assert!(matches!(parse_key("unknown"), Err(Error::Annotation { line: 1, .. })));
        assert_eq!(parse_tempo("126").unwrap(), Some(126.0));
        assert_eq!(parse_tempo("127.0\n").unwrap(), Some(127.0));
        assert_eq!(parse_tempo("0.0").unwrap(), None);
        assert!(parse_tempo("fast").is_err());
        assert!(parse_tempo("-5").is_err());
        assert!(parse_tempo("inf").is_err());
    }

    #[test]
    fn reads_the_mtg_key_table() {
        let text =
            "ID\tMANUAL KEY\tC\n5061\tD# minor\t2\n10981\t-\t0\n\n7\tF minor / G# major\t1\n8\tG minor/major\t1\n";
        let rows = parse_mtg_keys(text).unwrap();
        assert_eq!(rows[0], MtgKey { id: "5061".into(), keys: vec![key("D#m")], confidence: 2 });
        assert_eq!(rows[1].keys, []);
        assert_eq!(rows[2].keys, [key("Fm"), key("G#")]);
        assert_eq!(rows[3].keys, [key("Gm"), key("G")]);
        let err = |text| match parse_mtg_keys(text) {
            Err(Error::Annotation { line, message }) => (line, message),
            other => panic!("{other:?}"),
        };
        assert_eq!(err("ID\tMANUAL KEY\tC\n1\tC minor\n").0, 2);
        assert_eq!(err("1\tC minor\thigh\n").1, "the confidence isn't 0, 1 or 2");
        assert_eq!(err("1\tH minor\t2\n").1, "unreadable key");
    }

    #[test]
    fn keys_score_as_mirex_does() {
        let am = key("Am");
        let of = |e: &str| KeyOutcome::of(am, Camelot::parse(e));
        assert_eq!(of("Am"), KeyOutcome::Correct);
        assert_eq!(of("Em"), KeyOutcome::Fifth);
        assert_eq!(of("Dm"), KeyOutcome::Fifth);
        assert_eq!(of("C"), KeyOutcome::Relative);
        assert_eq!(of("A"), KeyOutcome::Parallel);
        assert_eq!(of("E"), KeyOutcome::Other);
        assert_eq!(of("Bbm"), KeyOutcome::Other);
        assert_eq!(of(""), KeyOutcome::Missing);
        let credits: Vec<f64> = KeyOutcome::ALL.iter().map(|o| o.credit()).collect();
        assert_eq!(credits, [1.0, 0.5, 0.3, 0.2, 0.0, 0.0]);
    }

    fn any_key(tc: &hegel::TestCase) -> Camelot {
        let (number, major) = tc.draw(gs::tuples!(gs::integers::<u8>().min_value(1).max_value(12), gs::booleans()));
        Camelot { number, major }
    }

    /// A weighted key score is between 0 and 1, 1 only for the right key, and the same whichever
    /// of the two keys is the reference.
    #[hegel::test(test_cases = 300)]
    fn key_scores_are_bounded_and_symmetric(tc: hegel::TestCase) {
        let (a, b) = (any_key(&tc), any_key(&tc));
        let credit = KeyOutcome::of(a, Some(b)).credit();
        assert!((0.0..=1.0).contains(&credit));
        assert_eq!(credit == 1.0, a == b);
        assert_eq!(KeyOutcome::of(b, Some(a)), KeyOutcome::of(a, Some(b)));
    }

    #[test]
    fn tempos_score_with_both_accuracies() {
        let of = |e| TempoOutcome::of(120.0, Some(e));
        assert_eq!(of(124.7), TempoOutcome::Correct);
        assert_eq!(of(125.0), TempoOutcome::Other);
        assert_eq!(of(240.0), TempoOutcome::Double);
        assert_eq!(of(61.0), TempoOutcome::Half);
        assert_eq!(of(360.0), TempoOutcome::Triple);
        assert_eq!(of(40.0), TempoOutcome::Third);
        assert_eq!(of(180.0), TempoOutcome::Other);
        assert_eq!(TempoOutcome::of(120.0, None), TempoOutcome::Missing);
        assert_eq!(TempoOutcome::of(120.0, Some(0.0)), TempoOutcome::Missing);
    }

    /// Accuracy2 is never below Accuracy1, both are percentages, and every track is counted once.
    #[hegel::test(test_cases = 200)]
    fn tempo_reports_add_up(tc: hegel::TestCase) {
        let pairs = tc.draw(gs::vecs(gs::tuples!(
            gs::floats::<f64>().min_value(60.0).max_value(200.0),
            gs::optional(gs::floats::<f64>().min_value(20.0).max_value(600.0))
        )));
        let rows = pairs.iter().enumerate().map(|(i, &(reference, estimate))| Row {
            analyzer: "a".into(),
            track: i.to_string(),
            reference,
            estimate,
        });
        let reports = score::<TempoOutcome>(rows);
        let Some(report) = reports.get("a") else { return assert!(pairs.is_empty()) };
        assert_eq!(report.counts.values().sum::<usize>(), pairs.len());
        let accuracy1 = report.percent(TempoOutcome::Correct);
        assert!((0.0..=100.0).contains(&accuracy1));
        assert!(report.accuracy2() >= accuracy1 && report.accuracy2() <= 100.0);
        assert!((report.score * 100.0 - accuracy1).abs() < 1e-9);
        assert_eq!(report.misses.len(), pairs.len() - report.count(TempoOutcome::Correct));
    }

    #[test]
    fn an_empty_report_scores_zero() {
        let report = Report::<KeyOutcome>::new();
        assert_eq!((report.score, report.percent(KeyOutcome::Correct)), (0.0, 0.0));
        assert_eq!(Report::<TempoOutcome>::new().accuracy2(), 0.0);
    }
}
