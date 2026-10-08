//! Musical keys on the Camelot wheel, and which tracks mix well with which: harmonic suggestions
//! for the player, key ordering for sorted playlists, and the costs the set planner uses.

use crate::Result;
use crate::model::{Library, Track};
use crate::progress::Ctx;

/// A key on the Camelot wheel: 1-12, minor (A) or major (B). Neighbours on the wheel mix well.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        let semitone = (root + shift) % 12;
        let offset = if major { MAJOR_OFFSET } else { MINOR_OFFSET };
        Some(Camelot { number: ((semitone * 7 + offset) % 12 + 1) as u8, major })
    }

    /// `8A`.
    pub fn code(self) -> String {
        format!("{}{}", self.number, if self.major { 'B' } else { 'A' })
    }

    /// `Am`.
    pub fn note(self) -> String {
        let offset = if self.major { MAJOR_OFFSET } else { MINOR_OFFSET };
        // 7 is its own inverse mod 12, so stepping back round the wheel is another multiply by 7.
        let semitone = ((self.number as u32 - 1 + 12 - offset) * 7) % 12;
        format!("{}{}", NOTES[semitone as usize], if self.major { "" } else { "m" })
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

/// Sort position of a key text (unparseable keys last).
pub fn key_order(key: &str) -> u8 {
    Camelot::parse(key).map_or(u8::MAX, Camelot::order)
}

/// How rough a mix from a track in key `a` at `a_bpm` into one in key `b` at `b_bpm` is, for
/// ordering a set: 0 is ideal. Key: same 0, ±1 or relative 1, energy boost 2, anything else 6
/// (unknown keys 3). Tempo: one point per 2% apart, allowing for half and double time.
pub fn mix_cost(a: Option<Camelot>, a_bpm: f64, b: Option<Camelot>, b_bpm: f64) -> f64 {
    let key = match (a, b) {
        (Some(x), Some(y)) => match x.relation_with_boost(y) {
            Some(Relation::Same) => 0.0,
            Some(Relation::Adjacent | Relation::Relative) => 1.0,
            Some(Relation::EnergyBoost) => 2.0,
            None => 6.0,
        },
        _ => 3.0,
    };
    let gap = tempo_gap(a_bpm, b_bpm);
    key + if gap.is_finite() { gap / 0.02 } else { 3.0 }
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
/// optional per track.
pub fn order_set(
    tracks: &[&Track],
    start: Option<u32>,
    energy: &std::collections::HashMap<u32, f64>,
    ctx: &Ctx<'_>,
) -> Result<Vec<u32>> {
    if tracks.is_empty() {
        return Ok(Vec::new());
    }
    // Each key parsed once: a set of n tracks weighs n² steps.
    let keys: Vec<Option<Camelot>> = tracks.iter().map(|t| Camelot::parse(&t.key)).collect();
    let e: Vec<f64> = tracks.iter().map(|t| energy.get(&t.id).copied().unwrap_or(2.0)).collect();
    // Cost of playing b straight after a.
    let step =
        |a: usize, b: usize| mix_cost(keys[a], tracks[a].bpm, keys[b], tracks[b].bpm) + 1.5 * (e[a] - e[b]).max(0.0);
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
fn tidy(order: &mut [usize], step: impl Fn(usize, usize) -> f64, ctx: &Ctx<'_>) -> Result<()> {
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

    #[test]
    fn set_order_walks_the_wheel_and_tempo() {
        let t = |id, key: &str, bpm| {
            let mut t = crate::model::tests::track(id, "A", &id.to_string(), &format!("/Contents/{id}.mp3"));
            t.key = key.into();
            t.bpm = bpm;
            t
        };
        let tracks = [t(1, "10A", 126.0), t(2, "8A", 122.0), t(3, "9A", 124.0), t(4, "11A", 128.0), t(5, "8A", 121.0)];
        let refs: Vec<&Track> = tracks.iter().collect();
        let ctx = Ctx::silent();
        let order = order_set(&refs, None, &Default::default(), &ctx).unwrap();
        assert_eq!(order, [5, 2, 3, 1, 4]);
        assert_eq!(order_set(&refs, Some(4), &Default::default(), &ctx).unwrap()[0], 4);
    }

    /// `n` made-up tracks: keys round the whole wheel, tempos from 118 to 134 BPM, every energy.
    fn made_up_set(n: u32) -> (Vec<Track>, std::collections::HashMap<u32, f64>) {
        let tracks = (0..n)
            .map(|id| {
                let mut t = crate::model::tests::track(id, "A", &id.to_string(), &format!("/Contents/{id}.mp3"));
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
        assert_eq!(order_set(&refs, None, &energy, &Ctx::silent()).unwrap().len(), 600);
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
    }

    /// Regression: nothing could stop ordering a long set.
    #[test]
    fn cancelling_stops_ordering() {
        let (tracks, energy) = made_up_set(3);
        let refs: Vec<&Track> = tracks.iter().collect();
        let ctx = Ctx::silent();
        ctx.cancel.cancel();
        assert!(matches!(order_set(&refs, None, &energy, &ctx), Err(crate::Error::Cancelled)));
        assert!(matches!(tidy(&mut [0, 1, 2], |_, _| 0.0, &ctx), Err(crate::Error::Cancelled)));
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
            let mut t = crate::model::tests::track(id, "A", title, &format!("/Contents/{id}.mp3"));
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
