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
    paths::key(p)
        .to_lowercase()
        .split('/')
        .filter(|c| !c.is_empty() && *c != "." && *c != "..")
        .map(String::from)
        .collect()
}

fn isrc_key(s: &str) -> Option<String> {
    let k: String = s.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_uppercase();
    (!k.is_empty()).then_some(k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use loclib_model::Track;
    use loclib_model::test_util::track;

    fn lib(tracks: Vec<Track>) -> Library {
        Library { tracks, ..Default::default() }
    }

    fn with(mut t: Track, f: impl FnOnce(&mut Track)) -> Track {
        f(&mut t);
        t
    }

    fn entry(artist: &str, title: &str) -> Entry {
        Entry { title: title.into(), artist: artist.into(), ..Default::default() }
    }

    fn status(lib: &Library, e: Entry) -> Status {
        Matcher::new(lib).find(&e)
    }

    fn found(id: u32, how: MatchKind) -> Status {
        Status::Found { track_id: id, how }
    }

    fn sample() -> Library {
        lib(vec![
            track(1, "Daft Punk", "One More Time", "/Contents/Daft Punk/Discovery/01 One More Time.mp3"),
            with(track(2, "deadmau5", "Strobe", "/Contents/deadmau5/For Lack/Strobe.mp3"), |t| {
                t.mix_name = "Original Mix".into();
                t.duration_secs = 637;
            }),
            with(track(3, "deadmau5", "Strobe (Radio Edit)", "/Contents/deadmau5/Radio/Strobe.mp3"), |t| {
                t.duration_secs = 214;
            }),
            with(track(4, "Avicii", "Levels (Skrillex Remix)", "/Contents/Avicii/Levels/Levels Skrillex.mp3"), |t| {
                t.isrc = "SE-AT1-11-00123".into();
            }),
            track(5, "Kölsch", "Grey", "/Contents/Kolsch/1983/Grey.mp3"),
            track(6, "Daft Punk feat. Pharrell Williams", "Get Lucky", "/Contents/Daft Punk/RAM/Get Lucky.mp3"),
            track(7, "Above & Beyond", "Sun & Moon", "/Contents/Above & Beyond/Group Therapy/Sun & Moon.mp3"),
            track(8, "Eric Prydz", "Opus", "/Contents/Eric Prydz/Opus/01 Intro.mp3"),
            track(9, "Bicep", "Glue", "/Contents/Bicep/Bicep/01 Intro.mp3"),
        ])
    }

    #[test]
    fn matches_by_path_suffix() {
        let l = sample();
        let mut e = entry("", "");
        e.path = Some("/Users/me/Music/deadmau5/For Lack/strobe.MP3".into());
        assert_eq!(status(&l, e.clone()), found(2, MatchKind::Path));
        // Windows path, case and slashes differ, NFD vs NFC.
        e.path = Some("D:\\Music\\DAFT PUNK\\Discovery\\01 One More Time.mp3".into());
        assert_eq!(status(&l, e.clone()), found(1, MatchKind::Path));
        e.path = Some("/Contents/Kolsch/1983/Grey.mp3".into());
        assert_eq!(status(&l, e.clone()), found(5, MatchKind::Path));
        // Two "01 Intro.mp3": the folder decides...
        e.path = Some("/Music/Bicep/01 Intro.mp3".into());
        assert_eq!(status(&l, e.clone()), found(9, MatchKind::Path));
        // ...and without one it's ambiguous.
        e.path = Some("01 Intro.mp3".into());
        assert_eq!(status(&l, e.clone()), Status::Ambiguous { candidates: vec![8, 9] });
        // A name-only match must share a word with the entry: this one goes on to metadata.
        e.path = Some("01 Intro.mp3".into());
        e.artist = "Eric Prydz".into();
        e.title = "Opus".into();
        assert_eq!(status(&l, e), found(8, MatchKind::Path));
        let mut e = entry("Daft Punk", "Get Lucky");
        e.path = Some("/elsewhere/Strobe.mp3".into());
        assert_eq!(status(&l, e), found(6, MatchKind::ArtistTitle));
    }

    #[test]
    fn matches_by_isrc_then_metadata() {
        let l = sample();
        let mut e = entry("Someone", "Something Else");
        e.isrc = Some("seat11100123".into());
        assert_eq!(status(&l, e), found(4, MatchKind::Isrc));
        // Path beats ISRC.
        let mut e = entry("", "");
        e.isrc = Some("SEAT11100123".into());
        e.path = Some("/x/Bicep/01 Intro.mp3".into());
        assert_eq!(status(&l, e), found(9, MatchKind::Path));
    }

    #[test]
    fn matches_by_artist_title_and_duration() {
        let l = sample();
        // Both Strobes are "Strobe" by deadmau5; the duration picks one.
        let mut e = entry("deadmau5", "Strobe");
        e.duration_secs = Some(215);
        assert_eq!(status(&l, e.clone()), found(3, MatchKind::ArtistTitleDuration));
        e.duration_secs = Some(636);
        assert_eq!(status(&l, e.clone()), found(2, MatchKind::ArtistTitleDuration));
        // No duration: "Strobe" with no version is the original mix, not the radio edit.
        e.duration_secs = None;
        assert_eq!(status(&l, e.clone()), found(2, MatchKind::ArtistTitle));
        // Spotify's naming.
        let e = entry("deadmau5", "Strobe - Radio Edit");
        assert_eq!(status(&l, e), found(3, MatchKind::ArtistTitle));
        // A duration matching neither: still ambiguous between versions it can't tell apart.
        let mut e = entry("deadmau5", "Strobe (Club Mix)");
        e.duration_secs = Some(400);
        assert_eq!(status(&l, e), Status::Ambiguous { candidates: vec![2, 3] });
    }

    #[test]
    fn normalised_matches() {
        let l = sample();
        // Accents, case, featured artists, "&" vs "and", Spotify credit lists.
        assert_eq!(status(&l, entry("KOLSCH", "grey")), found(5, MatchKind::ArtistTitle));
        assert_eq!(
            status(&l, entry("Daft Punk, Pharrell Williams, Nile Rodgers", "Get Lucky - Radio Edit")),
            found(6, MatchKind::ArtistTitle)
        );
        assert_eq!(
            status(&l, entry("Daft Punk", "Get Lucky (feat. Pharrell Williams)")),
            found(6, MatchKind::ArtistTitle)
        );
        assert_eq!(
            status(&l, entry("Above and Beyond", "Sun and Moon (Original Mix)")),
            found(7, MatchKind::ArtistTitle)
        );
        assert_eq!(status(&l, entry("Avicii", "Levels (Skrillex RMX)")), found(4, MatchKind::ArtistTitle));
        // Title only.
        assert_eq!(status(&l, entry("", "Glue")), found(9, MatchKind::Title));
        // The Beatport-style mix column counts as part of the title.
        let mut e = entry("Eric Prydz", "Opus");
        e.mix = Some("Original Mix".into());
        assert_eq!(status(&l, e), found(8, MatchKind::ArtistTitle));
    }

    #[test]
    fn versions_are_never_confused() {
        let l = sample();
        // The original of Levels isn't in the library; the Skrillex remix is offered, not picked.
        assert_eq!(status(&l, entry("Avicii", "Levels")), Status::Ambiguous { candidates: vec![4] });
        // A remix that isn't there doesn't become the original.
        assert_eq!(status(&l, entry("Bicep", "Glue (Ewan McVicar Remix)")), Status::Ambiguous { candidates: vec![9] });
        assert_eq!(status(&l, entry("Bicep", "Glue - Live")), Status::Ambiguous { candidates: vec![9] });
    }

    #[test]
    fn fuzzy_matches() {
        let l = sample();
        // A typo.
        assert_eq!(status(&l, entry("Daft Punk", "One More Tme")), found(1, MatchKind::Fuzzy));
        assert_eq!(status(&l, entry("Daft Punkk", "One More Time")), found(1, MatchKind::Fuzzy));
        // Swapped.
        assert_eq!(status(&l, entry("One More Time", "Daft Punk")), found(1, MatchKind::Fuzzy));
        // Everything in the title.
        assert_eq!(status(&l, entry("", "Eric Prydz Opus")), found(8, MatchKind::Fuzzy));
        // A track tagged with the artist inside its title.
        let l2 =
            lib(vec![track(1, "", "Moby - Porcelain", "/Contents/x.mp3"), track(2, "Moby", "Go", "/Contents/y.mp3")]);
        assert_eq!(status(&l2, entry("Moby", "Porcelain")), found(1, MatchKind::Fuzzy));
        // Nothing like it.
        assert_eq!(status(&l, entry("Burial", "Archangel")), Status::Missing);
        assert_eq!(status(&l, entry("", "")), Status::Missing);
        // Same title, different artist: offered at most, never found.
        assert!(!matches!(status(&l, entry("Pink Floyd", "Opus")), Status::Found { .. }));
    }

    /// Regression: an entry with no artist took the only track of its title even when its
    /// duration was far off, so a title-only list filled a playlist with unrelated tracks.
    #[test]
    fn weak_matches_need_the_duration_to_agree() {
        let l = sample();
        let timed = |artist: &str, title: &str, secs| Entry { duration_secs: Some(secs), ..entry(artist, title) };
        assert_eq!(status(&l, timed("", "Glue", 301)), found(9, MatchKind::Title));
        assert_eq!(status(&l, timed("", "Glue", 240)), Status::Ambiguous { candidates: vec![9] });
        assert_eq!(status(&l, timed("", "Eric Prydz Opus", 299)), found(8, MatchKind::Fuzzy));
        assert_eq!(status(&l, timed("", "Eric Prydz Opus", 1500)), Status::Ambiguous { candidates: vec![8] });
        // The artist and title together are enough.
        assert_eq!(status(&l, timed("Bicep", "Glue", 240)), found(9, MatchKind::ArtistTitle));
    }

    #[test]
    fn duplicates_are_ambiguous() {
        let l = lib(vec![
            track(1, "Moby", "Porcelain", "/Contents/Moby/Play/Porcelain.mp3"),
            track(2, "Moby", "Porcelain", "/Contents/Moby/Best Of/Porcelain.mp3"),
            track(3, "Moby", "Go", "/Contents/Moby/Go.mp3"),
            track(4, "Moby", "Go (Woodtick Mix)", "/Contents/Moby/Go2.mp3"),
            with(track(5, "Bicep", "Glu", "/Contents/Bicep/a.mp3"), |t| t.isrc = "X1".into()),
            with(track(6, "Bicep", "Glu", "/Contents/Bicep/b.mp3"), |t| t.isrc = "X1".into()),
        ]);
        assert_eq!(status(&l, entry("Moby", "Porcelain")), Status::Ambiguous { candidates: vec![1, 2] });
        let mut e = entry("", "");
        e.path = Some("/Music/Porcelain.mp3".into());
        assert_eq!(status(&l, e), Status::Ambiguous { candidates: vec![1, 2] });
        let mut e = entry("Bicep", "Glu");
        e.isrc = Some("X1".into());
        assert_eq!(status(&l, e), Status::Ambiguous { candidates: vec![5, 6] });
        // Near-equal fuzzy candidates are never picked between.
        assert_eq!(status(&l, entry("Moby", "Porcelian")), Status::Ambiguous { candidates: vec![1, 2] });
        assert_eq!(status(&l, entry("Moby", "Go")), found(3, MatchKind::ArtistTitle));
        assert_eq!(status(&l, entry("Moby", "Go - Woodtick Mix")), found(4, MatchKind::ArtistTitle));
    }

    #[test]
    fn asides_and_junk_mix_names() {
        let l = lib(vec![
            with(track(1, "Heaven & Earth", "Prescription", "/Contents/a.mp3"), |t| {
                t.mix_name = "exclusivesource4dj.com".into();
            }),
            track(2, "D'marc Cantu", "call home first", "/Contents/b.aif"),
            track(3, "D'marc Cantu", "call home first (DVSMMM)", "/Contents/c.aiff"),
        ]);
        // A mix name that names no version doesn't make a different recording.
        assert_eq!(status(&l, entry("Heaven and Earth", "Prescription")), found(1, MatchKind::ArtistTitle));
        // Exact titles pick their own version...
        assert_eq!(status(&l, entry("D'marc Cantu", "call home first")), found(2, MatchKind::ArtistTitle));
        assert_eq!(status(&l, entry("D'Marc Cantu", "Call Home First (DVSMMM)")), found(3, MatchKind::ArtistTitle));
        // ...and a mangled one still weighs the aside rather than preferring the bare title.
        assert_eq!(status(&l, entry("D'marc Cantu", "call home first (DVSMMM)x")), found(3, MatchKind::Fuzzy));
    }

    #[test]
    fn whole_file_round_trip() {
        let l = sample();
        let m3u = "#EXTM3U\n#EXTINF:320,Daft Punk - One More Time\n/Users/me/Music/Daft Punk/Discovery/01 One More Time.mp3\n\
                   #EXTINF:214,deadmau5 - Strobe (Radio Edit)\n/Users/me/Music/strobe_radio.mp3\n\
                   #EXTINF:300,Burial - Archangel\n/Users/me/Music/Archangel.mp3\n";
        let entries = parse(m3u.as_bytes(), Some("x.m3u")).unwrap();
        let report = Report::new(Format::M3u, match_entries(&l, &entries));
        assert_eq!(report.summary, Summary { entries: 3, found: 2, ambiguous: 0, missing: 1 });
        assert_eq!(report.track_ids(), [1, 3]);
        assert_eq!(report.entries[1].status, found(3, MatchKind::ArtistTitleDuration));

        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["format"], "m3u");
        assert_eq!(json["entries"][0]["status"], "found");
        assert_eq!(json["entries"][0]["how"], "path");
        assert_eq!(json["entries"][0]["track_id"], 1);
        assert_eq!(json["entries"][0]["entry"]["line"], 3);
        assert_eq!(json["entries"][2]["status"], "missing");
        let back: Report = serde_json::from_value(json).unwrap();
        assert_eq!(back, report);
        let amb = serde_json::to_value(Status::Ambiguous { candidates: vec![1, 2] }).unwrap();
        assert_eq!(amb, serde_json::json!({ "status": "ambiguous", "candidates": [1, 2] }));
    }

    #[test]
    fn large_libraries_stay_fast() {
        // 10,000 tracks x 1,000 entries, a mix of every strategy and of misses.
        let words = ["night", "light", "dream", "fire", "love", "deep", "sun", "moon", "city", "drive", "echo", "wave"];
        let tracks: Vec<Track> = (0..10_000u32)
            .map(|i| {
                let w = |k: u32| words[((i / k) % words.len() as u32) as usize];
                let artist = format!("Artist {} {}", w(7), i % 997);
                let title = format!("{} {} {}", w(1), w(3), i);
                with(track(i + 1, &artist, &title, &format!("/Contents/{artist}/{title}.mp3")), |t| {
                    t.duration_secs = 180 + i % 300;
                })
            })
            .collect();
        let l = lib(tracks);
        let entries: Vec<Entry> = (0..1_000u32)
            .map(|k| {
                let t = &l.tracks[(k * 37 % 10_000) as usize];
                match k % 4 {
                    0 => Entry { path: Some(format!("/Users/me/Music{}", &t.file_path[9..])), ..Default::default() },
                    1 => entry(&t.artist, &t.title),
                    2 => entry(&t.artist, &format!("{}x", t.title)),
                    _ => entry("Nobody", &format!("Nothing {k}")),
                }
            })
            .collect();
        let start = std::time::Instant::now();
        let matched = match_entries(&l, &entries);
        let elapsed = start.elapsed();
        let s = Summary::of(&matched);
        assert_eq!(s.entries, 1_000);
        assert!(s.found >= 500, "{s:?}");
        // Generous for unoptimised builds on slow CI machines; release takes a fraction of this.
        assert!(elapsed.as_secs() < 20, "{elapsed:?}");
    }
}
