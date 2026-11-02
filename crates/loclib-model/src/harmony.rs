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
    // A step up from the cheaper cost (`if_mislabelled` is never more), so the rounded result
    // can't drop as `sure` rises, which `sure * x + (1 - sure) * y` can.
    if_mislabelled + sure * (as_labelled - if_mislabelled)
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
        .filter(|t| t.file_path != track.file_path)
        .filter_map(|t| {
            let other = Camelot::parse(&t.key)?;
            let relation = key.relation(other)?;
            let gap = tempo_gap(track.bpm, t.bpm);
            // Another copy of the same recording is ruled out last: that normalises its tags.
            if gap > 0.06 || t.fingerprint_key() == me {
                return None;
            }
            Some(Suggestion { track: t.clone(), key: other, relation, tempo_gap: gap })
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
mod tests;
