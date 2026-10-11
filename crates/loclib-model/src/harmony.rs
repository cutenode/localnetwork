//! Musical keys on the Camelot wheel, and which tracks mix well with which: harmonic suggestions
//! for the player, key ordering for sorted playlists, and the costs the set planner uses.

use loclib_progress::{Cancelled, Ctx};

use crate::model::{Library, Track};

/// A key on the Camelot wheel: 1-12, minor (A) or major (B). Neighbours on the wheel mix well.
/// Serialised as its code (`"8A"`), and read back from any notation [`Camelot::parse`] takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct Camelot {
    pub number: u8,
    pub major: bool,
}

const NOTES: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];

/// Offsets that put A minor at 8A and C major at 8B: each fifth up is one step round the wheel.
const MINOR_OFFSET: u32 = 4;
const MAJOR_OFFSET: u32 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Relation {
    Same,
    /// One step round the wheel.
    Adjacent,
    /// Same number, minor to major or back.
    Relative,
    /// Two steps up the wheel, or a semitone up (seven steps): raises the energy.
    EnergyBoost,
}

impl Relation {
    pub fn describe(self) -> &'static str {
        match self {
            Relation::Same => "same key",
            Relation::Adjacent => "±1",
            Relation::Relative => "relative",
            Relation::EnergyBoost => "energy boost",
        }
    }
}

impl Camelot {
    /// rekordbox's key text in any of the notations it shows: Camelot (`8A`), Open Key (`1m`),
    /// or a note name (`Am`, `F#`, `Ebm`, `C# minor`).
    pub fn parse(s: &str) -> Option<Camelot> {
        let s = s.trim();
        let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(n) = digits.parse::<u8>()
            && (1..=12).contains(&n)
        {
            let rest = s[digits.len()..].to_ascii_lowercase();
            return match rest.as_str() {
                "a" => Some(Camelot { number: n, major: false }),
                "b" => Some(Camelot { number: n, major: true }),
                // Open Key: 1m is A minor (8A), 1d C major (8B).
                "m" => Some(Camelot { number: (n + 6) % 12 + 1, major: false }),
                "d" => Some(Camelot { number: (n + 6) % 12 + 1, major: true }),
                _ => None,
            };
        }
        let mut chars = s.chars();
        let root = match chars.next()?.to_ascii_uppercase() {
            'C' => 0,
            'D' => 2,
            'E' => 4,
            'F' => 5,
            'G' => 7,
            'A' => 9,
            'B' => 11,
            _ => return None,
        };
        let rest: String = chars.collect();
        // After the note letter, `b` can only be a flat.
        let (shift, quality) = match rest.chars().next() {
            Some(c @ ('#' | '♯')) => (1, &rest[c.len_utf8()..]),
            Some(c @ ('b' | '♭')) => (11, &rest[c.len_utf8()..]),
            _ => (0, rest.as_str()),
        };
        let major = match quality.trim().to_ascii_lowercase().as_str() {
            "" | "maj" | "major" => true,
            "m" | "min" | "minor" => false,
            _ => return None,
        };
        Some(Camelot::from_pitch(root + shift, major))
    }

    /// The key on `pitch` (semitones above C, taken mod 12): `from_pitch(9, false)` is A minor, 8A.
    pub fn from_pitch(pitch: u32, major: bool) -> Camelot {
        let offset = if major { MAJOR_OFFSET } else { MINOR_OFFSET };
        Camelot { number: ((pitch % 12 * 7 + offset) % 12 + 1) as u8, major }
    }

    /// Semitones above C of the key's tonic: 9 for 8A (A minor).
    pub fn pitch(self) -> u32 {
        let offset = if self.major { MAJOR_OFFSET } else { MINOR_OFFSET };
        // 7 is its own inverse mod 12, so stepping back round the wheel is another multiply by 7.
        ((self.number as u32 - 1 + 12 - offset) * 7) % 12
    }

    /// `8A`.
    pub fn code(self) -> String {
        format!("{}{}", self.number, if self.major { 'B' } else { 'A' })
    }

    /// `Am`.
    pub fn note(self) -> String {
        format!("{}{}", NOTES[self.pitch() as usize], if self.major { "" } else { "m" })
    }

    /// The relative key, sharing this one's notes: 8A (A minor) and 8B (C major).
    pub fn relative(self) -> Camelot {
        Camelot { major: !self.major, ..self }
    }

    /// The parallel key, on the same tonic: 8A (A minor) and 11B (A major).
    pub fn parallel(self) -> Camelot {
        Camelot::from_pitch(self.pitch(), !self.major)
    }

    /// How (and whether) a track in `other` mixes harmonically into this key.
    pub fn relation(self, other: Camelot) -> Option<Relation> {
        let steps = (self.number as i32 - other.number as i32).rem_euclid(12);
        match (self.major == other.major, steps) {
            (true, 0) => Some(Relation::Same),
            (true, 1 | 11) => Some(Relation::Adjacent),
            (false, 0) => Some(Relation::Relative),
            _ => None,
        }
    }

    /// [`Camelot::relation`], also counting energy-boost moves from this key into `other`.
    pub fn relation_with_boost(self, other: Camelot) -> Option<Relation> {
        self.relation(other).or_else(|| {
            let up = (other.number as i32 - self.number as i32).rem_euclid(12);
            (self.major == other.major && (up == 2 || up == 7)).then_some(Relation::EnergyBoost)
        })
    }

    /// Position round the wheel for sorting: 1A, 1B, 2A, 2B... 12B.
    pub fn order(self) -> u8 {
        (self.number - 1) * 2 + u8::from(self.major)
    }
}

impl From<Camelot> for String {
    fn from(key: Camelot) -> String {
        key.code()
    }
}

impl TryFrom<String> for Camelot {
    type Error = String;

    fn try_from(text: String) -> Result<Camelot, String> {
        Camelot::parse(&text).ok_or_else(|| format!("{text:?} isn't a key"))
    }
}

/// Sort position of a key text (unparseable keys last).
pub fn key_order(key: &str) -> u8 {
    Camelot::parse(key).map_or(u8::MAX, Camelot::order)
}

/// What a mix between two keys costs when either is unknown.
const UNKNOWN_KEY_COST: f64 = 3.0;

/// How rough a mix from a track in key `a` at `a_bpm` into one in key `b` at `b_bpm` is, for
/// ordering a set: 0 is ideal. Key: same 0, ±1 or relative 1, energy boost 2, anything else 6
/// (unknown keys 3). Tempo: one point per 2% apart, allowing for half and double time.
///
/// `sure` is how likely both keys are right, from 0 to 1: their analyses' confidences multiplied,
/// or 1 for keys taken as given. Below 1 the key part leans toward what the mix would cost if a key
/// were its relative or parallel instead (the mistakes key detection makes most, besides fifths,
/// which already mix as neighbours), and never costs more than an unknown key, so a clash that
/// may only be a wrong label weighs less.
pub fn mix_cost(a: Option<Camelot>, a_bpm: f64, b: Option<Camelot>, b_bpm: f64, sure: f64) -> f64 {
    let gap = tempo_gap(a_bpm, b_bpm);
    key_cost(a, b, sure) + if gap.is_finite() { gap / 0.02 } else { 3.0 }
}

fn key_cost(a: Option<Camelot>, b: Option<Camelot>, sure: f64) -> f64 {
    let labelled = |x: Camelot, y: Camelot| match x.relation_with_boost(y) {
        Some(Relation::Same) => 0.0,
        Some(Relation::Adjacent | Relation::Relative) => 1.0,
        Some(Relation::EnergyBoost) => 2.0,
        None => 6.0,
    };
    let (Some(a), Some(b)) = (a, b) else { return UNKNOWN_KEY_COST };
    let as_labelled = labelled(a, b);
    // A NaN confidence is taken as certain too.
    if sure >= 1.0 || sure.is_nan() {
        return as_labelled;
    }
    let alternates = |k: Camelot| [k, k.relative(), k.parallel()];
    let if_mislabelled =
        alternates(a).into_iter().flat_map(|x| alternates(b).map(|y| labelled(x, y))).fold(UNKNOWN_KEY_COST, f64::min);
    let sure = sure.max(0.0);
    sure * as_labelled + (1.0 - sure) * if_mislabelled
}

/// How far apart two tempos are, allowing for half and double time: 0.02 is 2%.
pub fn tempo_gap(a: f64, b: f64) -> f64 {
    if a <= 0.0 || b <= 0.0 {
        return f64::INFINITY;
    }
    [b / a, 2.0 * b / a, b / (2.0 * a)].into_iter().map(|r| (r - 1.0).abs()).fold(f64::INFINITY, f64::min)
}

pub struct Suggestion {
    pub track: Track,
    pub key: Camelot,
    pub relation: Relation,
    pub tempo_gap: f64,
}

/// Tracks in `library` that mix into `track`: a compatible key and a tempo within 6% (or half or
/// double time). Best first: same key before neighbours, then closest tempo.
pub fn suggestions(library: &Library, track: &Track, limit: usize) -> Vec<Suggestion> {
    let Some(key) = Camelot::parse(&track.key) else { return Vec::new() };
    let me = track.fingerprint_key();
    let mut out: Vec<Suggestion> = library
        .tracks
        .iter()
        .filter(|t| t.file_path != track.file_path && t.fingerprint_key() != me)
        .filter_map(|t| {
            let other = Camelot::parse(&t.key)?;
            let relation = key.relation(other)?;
            let gap = tempo_gap(track.bpm, t.bpm);
            (gap <= 0.06).then(|| Suggestion { track: t.clone(), key: other, relation, tempo_gap: gap })
        })
        .collect();
    out.sort_by(|a, b| a.relation.cmp(&b.relation).then(a.tempo_gap.total_cmp(&b.tempo_gap)));
    out.truncate(limit);
    out
}

/// Sets longer than this keep the greedy order: tidying keeps every pair's cost in a table (32 MB
/// at 2,000 tracks) and each round of it tries every stretch.
const TIDY_LIMIT: usize = 2000;

/// An order for a set: start from `start` (or the calmest, slowest track), then keep picking
/// the track that mixes in most smoothly ([`mix_cost`]), preferring energy that holds or rises,
/// and finally (up to 2,000 tracks) tidy the order by reversing stretches that make the whole set
/// smoother (2-opt). `energy` (e.g. from rekordbox's phrase analysis: 1 low, 2 mid, 3 high) is
/// optional per track. `key_confidence` says how sure each track's key is, 0 to 1 (from a key
/// analyzer); tracks it leaves out have keys taken as given, and a pairing that clashes only on a
/// key that may be wrong costs less (see [`mix_cost`]).
pub fn order_set(
    tracks: &[&Track],
    start: Option<u32>,
    energy: &rustc_hash::FxHashMap<u32, f64>,
    key_confidence: &rustc_hash::FxHashMap<u32, f64>,
    ctx: &Ctx<'_>,
) -> Result<Vec<u32>, Cancelled> {
    if tracks.is_empty() {
        return Ok(Vec::new());
    }
    // Each key parsed once: a set of n tracks weighs n² steps.
    let keys: Vec<Option<Camelot>> = tracks.iter().map(|t| Camelot::parse(&t.key)).collect();
    let e: Vec<f64> = tracks.iter().map(|t| energy.get(&t.id).copied().unwrap_or(2.0)).collect();
    let sure: Vec<f64> = tracks.iter().map(|t| key_confidence.get(&t.id).copied().unwrap_or(1.0)).collect();
    // Cost of playing b straight after a.
    let step = |a: usize, b: usize| {
        mix_cost(keys[a], tracks[a].bpm, keys[b], tracks[b].bpm, sure[a] * sure[b]) + 1.5 * (e[a] - e[b]).max(0.0)
    };
    let n = tracks.len();
    let first = start.and_then(|id| tracks.iter().position(|t| t.id == id)).unwrap_or_else(|| {
        // total_cmp: a NaN tempo or energy (corrupt analysis) mustn't panic.
        (0..n).min_by(|&i, &j| e[i].total_cmp(&e[j]).then(tracks[i].bpm.total_cmp(&tracks[j].bpm))).unwrap()
    });
    let mut order = vec![first];
    let mut used = vec![false; n];
    used[first] = true;
    while order.len() < n {
        ctx.check()?;
        let cur = *order.last().unwrap();
        let next = (0..n).filter(|i| !used[*i]).min_by(|&i, &j| step(cur, i).total_cmp(&step(cur, j))).unwrap();
        used[next] = true;
        order.push(next);
    }
    if n <= TIDY_LIMIT {
        tidy(&mut order, step, ctx)?;
    }
    Ok(order.into_iter().map(|i| tracks[i].id).collect())
}

/// 2-opt on a set's `order` (indexes into its tracks), keeping the first track where it is: each
/// round reverses, in turn, every stretch that makes the whole set smoother, until a round finds
/// none (or after 20). `step(a, b)` is the cost of playing b straight after a.
fn tidy(order: &mut [usize], step: impl Fn(usize, usize) -> f64, ctx: &Ctx<'_>) -> Result<(), Cancelled> {
    let n = order.len();
    // Every step worked out once: a round looks each one up many times.
    let cost: Vec<f64> = (0..n * n).map(|k| step(k / n, k % n)).collect();
    let c = |a: usize, b: usize| cost[a * n + b];
    let mut improved = true;
    let mut rounds = 0;
    while improved && rounds < 20 {
        ctx.check()?;
        improved = false;
        rounds += 1;
        for i in 1..n - 1 {
            // The stretch order[i..=j] played forwards and backwards, kept up to date as it grows,
            // so trying a reversal costs the same however long the stretch is. Reversing it only
            // changes its own steps (they turn round) and the steps into and out of it.
            let (mut forwards, mut backwards) = (0.0, 0.0);
            for j in i + 1..n {
                forwards += c(order[j - 1], order[j]);
                backwards += c(order[j], order[j - 1]);
                let out = |last: usize| order.get(j + 1).map_or(0.0, |&next| c(last, next));
                let now = c(order[i - 1], order[i]) + forwards + out(order[j]);
                let reversed = c(order[i - 1], order[j]) + backwards + out(order[i]);
                if reversed + 1e-9 < now {
                    order[i..=j].reverse();
                    std::mem::swap(&mut forwards, &mut backwards);
                    improved = true;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(s: &str) -> String {
        Camelot::parse(s).map(|c| c.code()).unwrap_or_default()
    }

    #[test]
    fn parses_every_notation() {
        assert_eq!(k("8A"), "8A");
        assert_eq!(k("12b"), "12B");
        assert_eq!(k("Am"), "8A");
        assert_eq!(k("C"), "8B");
        assert_eq!(k("Fm"), "4A");
        assert_eq!(k("Ebm"), "2A");
        assert_eq!(k("D#m"), "2A");
        assert_eq!(k("F#m"), "11A");
        assert_eq!(k("Dbm"), "12A");
        assert_eq!(k("Bbm"), "3A");
        assert_eq!(k("Bm"), "10A");
        assert_eq!(k("B"), "1B");
        assert_eq!(k("Db"), "3B");
        assert_eq!(k("E"), "12B");
        assert_eq!(k("C# minor"), "12A");
        assert_eq!(k("1m"), "8A");
        assert_eq!(k("1d"), "8B");
        assert_eq!(k("6m"), "1A");
        for bad in ["", "13A", "H", "Cx", "0A"] {
            assert_eq!(k(bad), "", "{bad}");
        }
    }

    #[test]
    fn names_round_trip() {
        for n in 1..=12 {
            for major in [false, true] {
                let c = Camelot { number: n, major };
                assert_eq!(Camelot::parse(&c.note()), Some(c), "{}", c.note());
            }
        }
        assert_eq!(Camelot::parse("8A").unwrap().note(), "Am");
        assert_eq!(Camelot::parse("4A").unwrap().note(), "Fm");
    }

    #[test]
    fn harmonic_neighbours() {
        let a = Camelot::parse("12A").unwrap();
        assert_eq!(a.relation(Camelot::parse("12A").unwrap()), Some(Relation::Same));
        assert_eq!(a.relation(Camelot::parse("1A").unwrap()), Some(Relation::Adjacent));
        assert_eq!(a.relation(Camelot::parse("11A").unwrap()), Some(Relation::Adjacent));
        assert_eq!(a.relation(Camelot::parse("12B").unwrap()), Some(Relation::Relative));
        assert_eq!(a.relation(Camelot::parse("2A").unwrap()), None);
        assert_eq!(a.relation(Camelot::parse("1B").unwrap()), None);
        assert_eq!(a.relation_with_boost(Camelot::parse("2A").unwrap()), Some(Relation::EnergyBoost));
        assert_eq!(a.relation_with_boost(Camelot::parse("7A").unwrap()), Some(Relation::EnergyBoost));
        assert_eq!(a.relation_with_boost(Camelot::parse("10A").unwrap()), None);
    }

    fn any_key(tc: &hegel::TestCase) -> Camelot {
        use hegel::generators as gs;
        let (number, major) = tc.draw(gs::tuples!(gs::integers::<u8>().min_value(1).max_value(12), gs::booleans()));
        Camelot { number, major }
    }

    #[test]
    fn relative_and_parallel_keys() {
        let am = Camelot::parse("Am").unwrap();
        assert_eq!(am.relative().note(), "C");
        assert_eq!(am.parallel().note(), "A");
        assert_eq!(am.parallel().code(), "11B");
        assert_eq!(Camelot::parse("C").unwrap().parallel().note(), "Cm");
        assert_eq!(Camelot::from_pitch(9 + 12, false), am);
        assert_eq!(am.pitch(), 9);
    }

    /// Relative and parallel are each their own inverse; the relative key shares the number, the
    /// parallel one the tonic, and neither keeps the mode.
    #[hegel::test(test_cases = 100)]
    fn relative_and_parallel_are_involutions(tc: hegel::TestCase) {
        let k = any_key(&tc);
        assert_eq!(k.relative().relative(), k);
        assert_eq!(k.parallel().parallel(), k);
        assert_eq!(k.relative().number, k.number);
        assert_eq!(k.parallel().pitch(), k.pitch());
        assert_ne!(k.relative().major, k.major);
        assert_ne!(k.parallel().major, k.major);
        assert_eq!(Camelot::from_pitch(k.pitch(), k.major), k);
    }

    /// Keys serialise as their code and read back; text that isn't a key is refused.
    #[test]
    fn keys_serialise_as_codes() {
        let am = Camelot::parse("Am").unwrap();
        assert_eq!(serde_json::to_string(&am).unwrap(), "\"8A\"");
        assert_eq!(serde_json::from_str::<Camelot>("\"Am\"").unwrap(), am);
        assert!(serde_json::from_str::<Camelot>("\"13A\"").is_err());
    }

    /// A certain key costs exactly what it always did, an unsure one never more, and the less sure
    /// it is the less a clash weighs, down to an unknown key's cost or what its relative or
    /// parallel pairing would cost.
    #[hegel::test(test_cases = 300)]
    fn unsure_keys_cost_less(tc: hegel::TestCase) {
        use hegel::generators as gs;
        let (a, b) = (any_key(&tc), any_key(&tc));
        let sure = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(1.0));
        let less_sure = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(sure));
        let certain = mix_cost(Some(a), 124.0, Some(b), 124.0, 1.0);
        let unsure = mix_cost(Some(a), 124.0, Some(b), 124.0, sure);
        assert!(unsure <= certain && unsure >= 0.0, "{certain} {unsure}");
        assert!(mix_cost(Some(a), 124.0, Some(b), 124.0, less_sure) <= unsure);
        assert!(mix_cost(Some(a), 124.0, Some(b), 124.0, 0.0) <= UNKNOWN_KEY_COST);
        assert_eq!(mix_cost(Some(a), 124.0, Some(b), 124.0, f64::NAN), certain);
        assert_eq!(mix_cost(None, 124.0, Some(b), 124.0, sure), UNKNOWN_KEY_COST);
    }

    #[test]
    fn a_clash_that_may_be_a_mislabel_costs_less() {
        let (am, a_major, e_flat_minor) =
            (Some(Camelot::parse("8A").unwrap()), Camelot::parse("11B"), Camelot::parse("2A"));
        // A minor into A major clashes as labelled; if either were its parallel, it'd be the same key.
        assert_eq!(mix_cost(am, 124.0, a_major, 124.0, 1.0), 6.0);
        assert_eq!(mix_cost(am, 124.0, a_major, 124.0, 0.5), 3.0);
        assert_eq!(mix_cost(am, 124.0, a_major, 124.0, 0.0), 0.0);
        // A clash no alternate fixes eases only to an unknown key's cost.
        assert_eq!(mix_cost(am, 124.0, e_flat_minor, 124.0, 0.0), UNKNOWN_KEY_COST);
    }

    /// With no confidences the planner orders as it always did; an unsure key that may be the
    /// parallel of the one before it moves up to follow it.
    #[test]
    fn unsure_keys_change_the_set_order() {
        let t = |id, key: &str| {
            let mut t = crate::test_util::track(id, "A", &id.to_string(), &format!("/Contents/{id}.mp3"));
            t.key = key.into();
            t.bpm = 124.0;
            t
        };
        let tracks = [t(1, "8A"), t(2, "3A"), t(3, "11B")];
        let refs: Vec<&Track> = tracks.iter().collect();
        let ctx = Ctx::silent();
        let energy = Default::default();
        assert_eq!(order_set(&refs, Some(1), &energy, &Default::default(), &ctx).unwrap(), [1, 2, 3]);
        let unsure = [(3, 0.2)].into_iter().collect();
        assert_eq!(order_set(&refs, Some(1), &energy, &unsure, &ctx).unwrap(), [1, 3, 2]);
        assert!(order_set(&[], None, &energy, &unsure, &ctx).unwrap().is_empty());
    }

    #[test]
    fn set_order_walks_the_wheel_and_tempo() {
        let t = |id, key: &str, bpm| {
            let mut t = crate::test_util::track(id, "A", &id.to_string(), &format!("/Contents/{id}.mp3"));
            t.key = key.into();
            t.bpm = bpm;
            t
        };
        let tracks = [t(1, "10A", 126.0), t(2, "8A", 122.0), t(3, "9A", 124.0), t(4, "11A", 128.0), t(5, "8A", 121.0)];
        let refs: Vec<&Track> = tracks.iter().collect();
        let ctx = Ctx::silent();
        let order = order_set(&refs, None, &Default::default(), &Default::default(), &ctx).unwrap();
        assert_eq!(order, [5, 2, 3, 1, 4]);
        assert_eq!(order_set(&refs, Some(4), &Default::default(), &Default::default(), &ctx).unwrap()[0], 4);
    }

    /// `n` made-up tracks: keys round the whole wheel, tempos from 118 to 134 BPM, every energy.
    fn made_up_set(n: u32) -> (Vec<Track>, rustc_hash::FxHashMap<u32, f64>) {
        let tracks = (0..n)
            .map(|id| {
                let mut t = crate::test_util::track(id, "A", &id.to_string(), &format!("/Contents/{id}.mp3"));
                t.key = Camelot { number: (id * 5 % 12 + 1) as u8, major: id % 7 < 3 }.code();
                t.bpm = 118.0 + f64::from(id * 37 % 160) / 10.0;
                t
            })
            .collect();
        (tracks, (0..n).map(|id| (id, f64::from(id % 3 + 1))).collect())
    }

    /// Regression: tidying re-totalled the whole set for every reversal it tried (n³ steps a
    /// round, each parsing two keys), so 400 tracks took about a minute to order.
    #[test]
    fn ordering_a_long_set_stays_quick() {
        let (tracks, energy) = made_up_set(600);
        let refs: Vec<&Track> = tracks.iter().collect();
        let started = std::time::Instant::now();
        assert_eq!(order_set(&refs, None, &energy, &Default::default(), &Ctx::silent()).unwrap().len(), 600);
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
    }

    /// Regression: nothing could stop ordering a long set.
    #[test]
    fn cancelling_stops_ordering() {
        let (tracks, energy) = made_up_set(3);
        let refs: Vec<&Track> = tracks.iter().collect();
        let ctx = Ctx::silent();
        ctx.cancel.cancel();
        assert!(matches!(order_set(&refs, None, &energy, &Default::default(), &ctx), Err(Cancelled)));
        assert!(matches!(tidy(&mut [0, 1, 2], |_, _| 0.0, &ctx), Err(Cancelled)));
    }

    /// 2-opt as first written: every reversal it tries re-totals the whole order.
    fn tidy_by_retotalling(order: &mut [usize], step: impl Fn(usize, usize) -> f64) {
        let n = order.len();
        let total = |o: &[usize]| o.windows(2).map(|w| step(w[0], w[1])).sum::<f64>();
        let mut best = total(order);
        let mut improved = true;
        let mut rounds = 0;
        while improved && rounds < 20 {
            improved = false;
            rounds += 1;
            for i in 1..n - 1 {
                for j in i + 1..n {
                    order[i..=j].reverse();
                    let c = total(order);
                    if c + 1e-9 < best {
                        best = c;
                        improved = true;
                    } else {
                        order[i..=j].reverse();
                    }
                }
            }
        }
    }

    /// Tidying from a table with running totals makes the same reversals as re-totalling the
    /// whole order for each, whatever the costs (a step and its reverse can differ).
    #[hegel::test(test_cases = 300)]
    fn tidying_matches_retotalling(tc: hegel::TestCase) {
        use hegel::generators as gs;
        let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(12));
        let cost: Vec<f64> =
            tc.draw(gs::vecs(gs::floats::<f64>().min_value(0.0).max_value(20.0)).min_size(n * n).max_size(n * n));
        let step = |a: usize, b: usize| cost[a * n + b];
        let mut tidied: Vec<usize> = (0..n).collect();
        let mut expected = tidied.clone();
        tidy(&mut tidied, step, &Ctx::silent()).unwrap();
        tidy_by_retotalling(&mut expected, step);
        assert_eq!(tidied, expected);
    }

    #[test]
    fn orders_round_the_wheel() {
        let mut keys = vec!["2A", "Am", "1B", "", "1A", "12B"];
        keys.sort_by_key(|k| key_order(k));
        assert_eq!(keys, ["1A", "1B", "2A", "Am", "12B", ""]);
    }

    /// Suggestions mix in by key and tempo, best first, and never offer the track itself (by its
    /// file, or a copy of it elsewhere).
    #[test]
    fn suggestions_mix_in_best_first() {
        let t = |id: u32, title: &str, key: &str, bpm| {
            let mut t = crate::test_util::track(id, "A", title, &format!("/Contents/{id}.mp3"));
            t.key = key.into();
            t.bpm = bpm;
            t
        };
        let playing = t(1, "Playing", "8A", 124.0);
        let mut copy = playing.clone();
        copy.id = 2;
        copy.file_path = "/Contents/copy.mp3".into();
        let tracks = vec![
            playing.clone(),
            copy,
            t(3, "Relative, half time", "8B", 62.0),
            t(4, "Neighbour", "9A", 124.0),
            t(5, "Same key", "Am", 125.0),
            t(6, "Too fast", "8A", 140.0),
            t(7, "Clash", "3A", 124.0),
            t(8, "No key", "", 124.0),
        ];
        let library = Library { tracks, ..Default::default() };
        let titles = |limit| suggestions(&library, &playing, limit).into_iter().map(|s| s.track.id).collect::<Vec<_>>();
        assert_eq!(titles(10), [5, 4, 3]);
        assert_eq!(titles(2), [5, 4]);
        assert!(suggestions(&library, &t(9, "Unknown key", "?", 124.0), 10).is_empty());
        let described: Vec<&str> = [Relation::Same, Relation::Adjacent, Relation::Relative, Relation::EnergyBoost]
            .map(Relation::describe)
            .into();
        assert_eq!(described, ["same key", "±1", "relative", "energy boost"]);
    }

    #[test]
    fn tempo_allows_half_and_double_time() {
        assert!(tempo_gap(126.0, 128.0) < 0.02);
        assert!(tempo_gap(140.0, 70.0) < 1e-9);
        assert!(tempo_gap(87.0, 174.0) < 1e-9);
        assert!(tempo_gap(120.0, 140.0) > 0.06);
        assert!(tempo_gap(0.0, 120.0).is_infinite());
    }
}
