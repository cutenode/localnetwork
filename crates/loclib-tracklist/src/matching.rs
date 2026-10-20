//! Finding a tracklist's entries in a library.

use loclib_model::model::Library;
use loclib_model::paths;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use crate::m3u::strip_extension;
use crate::normalise::{Key, artists_compatible, score, tokens};
use crate::{Entry, Format};

/// How an entry was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    /// Same file (by its trailing path components).
    Path,
    Isrc,
    /// Same normalised artist and title, durations within 2 seconds.
    ArtistTitleDuration,
    /// Same normalised artist and title.
    ArtistTitle,
    /// The entry names no artist; same normalised title.
    Title,
    /// Close enough on fuzzy token similarity, with no close runner-up.
    Fuzzy,
    /// Picked from the candidates by hand.
    Chosen,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Status {
    Found {
        track_id: u32,
        how: MatchKind,
    },
    /// Needs a decision: several tracks fit equally well, or the only candidates are weak.
    /// Best first.
    Ambiguous {
        candidates: Vec<u32>,
    },
    Missing,
}

impl Status {
    pub fn track_id(&self) -> Option<u32> {
        match self {
            Status::Found { track_id, .. } => Some(*track_id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Matched {
    pub entry: Entry,
    #[serde(flatten)]
    pub status: Status,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(rename = "ImportSummary"))]
pub struct Summary {
    pub entries: usize,
    pub found: usize,
    pub ambiguous: usize,
    pub missing: usize,
}

impl Summary {
    pub fn of(matched: &[Matched]) -> Summary {
        let mut s = Summary { entries: matched.len(), ..Default::default() };
        for m in matched {
            match m.status {
                Status::Found { .. } => s.found += 1,
                Status::Ambiguous { .. } => s.ambiguous += 1,
                Status::Missing => s.missing += 1,
            }
        }
        s
    }
}

/// Everything an import preview shows: the format read, the counts, and every entry's result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Report {
    pub format: Format,
    pub summary: Summary,
    pub entries: Vec<Matched>,
}

impl Report {
    pub fn new(format: Format, entries: Vec<Matched>) -> Report {
        Report { format, summary: Summary::of(&entries), entries }
    }

    /// Track ids of the found entries, in list order (repeats kept).
    pub fn track_ids(&self) -> Vec<u32> {
        self.entries.iter().filter_map(|m| m.status.track_id()).collect()
    }
}

/// Find every entry in `lib`.
pub fn match_entries(lib: &Library, entries: &[Entry]) -> Vec<Matched> {
    let matcher = Matcher::new(lib);
    entries.iter().map(|e| Matched { entry: e.clone(), status: matcher.find(e) }).collect()
}

/// Durations this close (seconds) count as the same.
pub(crate) const DURATION_SLACK: u32 = 2;
/// Fuzzy scores (0-1): at least `FOUND` with no runner-up within `CLOSE` is a match; down to
/// `WEAK` the candidates are offered as ambiguous.
const FOUND: f32 = 0.8;
const WEAK: f32 = 0.65;
const CLOSE: f32 = 0.05;
/// Fuzzy candidates scored per entry, and offered per ambiguous entry.
const MAX_SCORED: usize = 200;
const MAX_OFFERED: usize = 10;

/// A library indexed for matching. Build once, then [`Matcher::find`] each entry; building
/// indexes every track (paths, ISRCs, normalised titles and tokens), so a 10,000-track library
/// takes milliseconds to index and lookups don't scan it.
pub struct Matcher<'a> {
    lib: &'a Library,
    keys: Vec<Key>,
    /// Lower-cased path components per track.
    parts: Vec<Vec<String>>,
    /// Folded tokens of each track's file name.
    stems: Vec<Vec<String>>,
    by_name: FxHashMap<String, Vec<usize>>,
    by_isrc: FxHashMap<String, Vec<usize>>,
    by_title: FxHashMap<String, Vec<usize>>,
    by_token: FxHashMap<String, Vec<usize>>,
}

impl<'a> Matcher<'a> {
    pub fn new(lib: &'a Library) -> Matcher<'a> {
        let n = lib.tracks.len();
        let mut m = Matcher {
            lib,
            keys: Vec::with_capacity(n),
            parts: Vec::with_capacity(n),
            stems: Vec::with_capacity(n),
            by_name: FxHashMap::default(),
            by_isrc: FxHashMap::default(),
            by_title: FxHashMap::default(),
            by_token: FxHashMap::default(),
        };
        for (i, t) in lib.tracks.iter().enumerate() {
            let key = Key::new(&t.title, &t.artist, &t.mix_name, Some(t.duration_secs).filter(|&d| d > 0));
            let parts = path_parts(&t.file_path);
            let name = parts.last().cloned().unwrap_or_default();
            if !name.is_empty() {
                m.by_name.entry(name.clone()).or_default().push(i);
            }
            if let Some(isrc) = isrc_key(&t.isrc) {
                m.by_isrc.entry(isrc).or_default().push(i);
            }
            if !key.title.is_empty() {
                m.by_title.entry(key.title.clone()).or_default().push(i);
            }
            for w in &key.bag {
                m.by_token.entry(w.clone()).or_default().push(i);
            }
            m.stems.push(tokens(&strip_extension(&name).replace('_', " ")));
            m.parts.push(parts);
            m.keys.push(key);
        }
        m
    }

    /// Where `entry` is in the library: by path, ISRC, artist + title (+ duration), then fuzzy.
    pub fn find(&self, entry: &Entry) -> Status {
        let key = Key::new(
            &entry.title,
            &entry.artist,
            entry.mix.as_deref().unwrap_or(""),
            entry.duration_secs.filter(|&d| d > 0),
        );
        if let Some(s) = entry.path.as_deref().and_then(|p| self.by_path(p, &key)) {
            return s;
        }
        if let Some(c) = entry.isrc.as_deref().and_then(isrc_key).and_then(|k| self.by_isrc.get(&k)) {
            return self.decide(c, &key, MatchKind::Isrc);
        }
        if let Some(s) = self.by_metadata(&key) {
            return s;
        }
        self.fuzzy(&key)
    }

    /// Tracks with the same file name, ranked by how many trailing path components agree. A
    /// match on the file name alone must also share a word with the entry's artist or title
    /// (when it has any), so "01 Track 1.mp3" doesn't pick an unrelated file.
    fn by_path(&self, path: &str, key: &Key) -> Option<Status> {
        let parts = path_parts(path);
        let cands = self.by_name.get(parts.last()?)?;
        let depth = |i: usize| parts.iter().rev().zip(self.parts[i].iter().rev()).take_while(|(a, b)| a == b).count();
        let best = cands.iter().map(|&i| depth(i)).max()?;
        let mut top: Vec<usize> = cands.iter().copied().filter(|&i| depth(i) == best).collect();
        if best == 1 {
            top.retain(|&i| {
                key.bag.is_empty() || key.bag.iter().any(|w| self.keys[i].bag.contains(w) || self.stems[i].contains(w))
            });
        }
        (!top.is_empty()).then(|| self.decide(&top, key, MatchKind::Path))
    }

    /// Same normalised title and version, and compatible artists (one list contains the other,
    /// so "Daft Punk" matches "Daft Punk, Pharrell Williams"). Durations decide between the two
    /// strategies' [`MatchKind`]s.
    fn by_metadata(&self, key: &Key) -> Option<Status> {
        let cands = self.by_title.get(&key.title)?;
        let same: Vec<usize> = cands
            .iter()
            .copied()
            .filter(|&i| {
                let k = &self.keys[i];
                k.mix == key.mix && (key.artists.is_empty() || artists_compatible(&key.artists, &k.artists))
            })
            .collect();
        if same.is_empty() {
            return None;
        }
        if key.artists.is_empty() {
            return Some(self.decide(&same, key, MatchKind::Title));
        }
        if let Some(d) = key.duration {
            let near: Vec<usize> = same
                .iter()
                .copied()
                .filter(|&i| self.keys[i].duration.is_some_and(|t| t.abs_diff(d) <= DURATION_SLACK))
                .collect();
            if !near.is_empty() {
                return Some(self.decide(&near, key, MatchKind::ArtistTitleDuration));
            }
        }
        Some(self.decide(&same, key, MatchKind::ArtistTitle))
    }

    fn fuzzy(&self, key: &Key) -> Status {
        if key.bag.is_empty() {
            return Status::Missing;
        }
        // Candidates share a word with the entry. Words in more than a tenth of the library
        // ("love", "remix") only nominate candidates when the entry has nothing rarer.
        let cap = (self.keys.len() / 10).max(50);
        let postings: Vec<&Vec<usize>> = key.bag.iter().filter_map(|w| self.by_token.get(w)).collect();
        let selective = postings.iter().any(|p| p.len() <= cap);
        let mut hits: FxHashMap<usize, usize> = FxHashMap::default();
        for p in postings.into_iter().filter(|p| !selective || p.len() <= cap) {
            for &i in p {
                *hits.entry(i).or_default() += 1;
            }
        }
        let mut cands: Vec<(usize, usize)> = hits.into_iter().collect();
        cands.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        cands.truncate(MAX_SCORED);
        let mut scored: Vec<(usize, f32)> = cands.into_iter().map(|(i, _)| (i, score(key, &self.keys[i]))).collect();
        scored.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let Some(&(_, best)) = scored.first() else { return Status::Missing };
        if best >= FOUND {
            let close: Vec<usize> = scored.iter().take_while(|(_, s)| *s >= best - CLOSE).map(|&(i, _)| i).collect();
            // The entry matches none of them exactly, so its version words prove nothing here.
            return self.narrow(&close, key, MatchKind::Fuzzy, false);
        }
        if best >= WEAK {
            let floor = WEAK.max(best - 0.1);
            let candidates = scored
                .iter()
                .take_while(|(_, s)| *s >= floor)
                .take(MAX_OFFERED)
                .map(|&(i, _)| self.lib.tracks[i].id)
                .collect();
            return Status::Ambiguous { candidates };
        }
        Status::Missing
    }

    /// One candidate is a match. Several are narrowed to the ones with exactly the entry's
    /// version words ("Extended Mix" over "Radio Edit"), then to the ones within 2 seconds of
    /// its duration; if that leaves one, it's the match, otherwise the entry is ambiguous. A
    /// match on the title alone, or a fuzzy one, is weak: when both sides have a duration that
    /// doesn't agree, the track is only offered ("Intro" in a list is rarely the library's only
    /// "Intro").
    fn decide(&self, cands: &[usize], key: &Key, how: MatchKind) -> Status {
        self.narrow(cands, key, how, true)
    }

    fn narrow(&self, cands: &[usize], key: &Key, how: MatchKind, by_variant: bool) -> Status {
        let same_duration =
            |k: &Key| key.duration.zip(k.duration).is_some_and(|(a, b)| a.abs_diff(b) <= DURATION_SLACK);
        let weak = matches!(how, MatchKind::Title | MatchKind::Fuzzy);
        let found = |i: usize| {
            let track_id = self.lib.tracks[i].id;
            match weak && key.duration.is_some() && self.keys[i].duration.is_some() && !same_duration(&self.keys[i]) {
                true => Status::Ambiguous { candidates: vec![track_id] },
                false => Status::Found { track_id, how },
            }
        };
        if let [i] = cands {
            return found(*i);
        }
        let mut rest = cands.to_vec();
        let same_variant = |k: &Key| k.variant == key.variant;
        let narrowers: &[&dyn Fn(&Key) -> bool] =
            if by_variant { &[&same_variant, &same_duration] } else { &[&same_duration] };
        for keep in narrowers {
            let narrowed: Vec<usize> = rest.iter().copied().filter(|&i| keep(&self.keys[i])).collect();
            match narrowed.len() {
                0 => {}
                1 => return found(narrowed[0]),
                _ => rest = narrowed,
            }
        }
        Status::Ambiguous { candidates: rest.iter().map(|&i| self.lib.tracks[i].id).collect() }
    }
}

fn path_parts(p: &str) -> Vec<String> {
    paths::file_key(p).split('/').filter(|c| !c.is_empty() && *c != "." && *c != "..").map(String::from).collect()
}

fn isrc_key(s: &str) -> Option<String> {
    let k: String = s.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_uppercase();
    (!k.is_empty()).then_some(k)
}

#[cfg(test)]
mod tests;
