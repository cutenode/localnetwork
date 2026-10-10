//! The public part of a profile: what the DJ chose to show, rounded so it describes a style
//! without being an inventory.
//!
//! A [`ProfileDocument`] never holds file paths, drive or history-list names, ratings, comments,
//! bitrates or exact dates, whatever the preferences say, and never counts sessions (they stand in
//! for sets until the DJ declares some, [`super::Basis::Sessions`]). Tracks are only named in the
//! `signature` section, and never when the DJ hid the track or its artist, or a set marks it
//! unnamed.

use indexmap::IndexMap;
use std::collections::BTreeSet;

use chrono::{DateTime, Datelike, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Basis, ProfileReport, artist_credits, credited_artists};
use crate::model::normalize;
use crate::store::Store;
use crate::{Error, Result, fsutil};

/// The store file holding [`ProfilePrefs`].
pub const PREFS_FILE: &str = "profile.json";

/// Document format version.
pub const DOCUMENT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "ProfileSection")]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Summary,
    Sets,
    Tempo,
    Arc,
    Mixing,
    Keys,
    Energy,
    Genres,
    Labels,
    Artists,
    Era,
    Signature,
    Prep,
    Collection,
    Style,
}

impl Section {
    pub const ALL: [Section; 15] = [
        Section::Summary,
        Section::Sets,
        Section::Tempo,
        Section::Arc,
        Section::Mixing,
        Section::Keys,
        Section::Energy,
        Section::Genres,
        Section::Labels,
        Section::Artists,
        Section::Era,
        Section::Signature,
        Section::Prep,
        Section::Collection,
        Section::Style,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Section::Summary => "summary",
            Section::Sets => "sets",
            Section::Tempo => "tempo",
            Section::Arc => "arc",
            Section::Mixing => "mixing",
            Section::Keys => "keys",
            Section::Energy => "energy",
            Section::Genres => "genres",
            Section::Labels => "labels",
            Section::Artists => "artists",
            Section::Era => "era",
            Section::Signature => "signature",
            Section::Prep => "prep",
            Section::Collection => "collection",
            Section::Style => "style",
        }
    }

    /// The title in a report on `basis`: without declared sets, the sets are sessions.
    pub fn title_in(self, basis: Basis) -> &'static str {
        match (self, basis) {
            (Section::Sets, Basis::Sessions) => "Sessions",
            (Section::Arc, Basis::Sessions) => "Session shape",
            _ => self.title(),
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Section::Summary => "Summary",
            Section::Sets => "Sets",
            Section::Tempo => "Tempo",
            Section::Arc => "Set shape",
            Section::Mixing => "Mixing",
            Section::Keys => "Keys",
            Section::Energy => "Energy",
            Section::Genres => "Genres",
            Section::Labels => "Labels",
            Section::Artists => "Artists",
            Section::Era => "Era",
            Section::Signature => "Signature tracks",
            Section::Prep => "Prep",
            Section::Collection => "Collection",
            Section::Style => "Player style",
        }
    }

    /// What publishing the section tells people.
    pub fn reveals(self) -> &'static str {
        match self {
            Section::Summary => "a one-line description and traits like \"Mixes in key\"",
            Section::Sets => "roughly how many sets you've played, and since when (year only)",
            Section::Tempo => "the BPM range you play in",
            Section::Arc => "whether your sets build, hold or wind down in tempo",
            Section::Mixing => "how often you mix in key and how smoothly you change tempo",
            Section::Keys => "the keys you play most",
            Section::Energy => "the mix of high, mid and low mood tracks you play",
            Section::Genres => "the genres you play",
            Section::Labels => "the labels you play most (can reveal promo pools)",
            Section::Artists => "the artists you play most",
            Section::Era => "how new or old the music you play is",
            Section::Signature => "tracks you play again and again, by name",
            Section::Prep => "how many of your tracks have cues and loops set",
            Section::Collection => "roughly how big your collection is",
            Section::Style => "player settings such as quantize, jog mode and tempo range",
        }
    }

    /// Published unless the DJ turns them off. Sections that name tracks, artists or labels, or
    /// describe the collection itself, are opt-in.
    pub fn published_by_default(self) -> bool {
        !matches!(self, Section::Labels | Section::Artists | Section::Signature | Section::Prep | Section::Collection)
    }

    pub fn parse(s: &str) -> Option<Section> {
        let s = s.trim().to_lowercase().replace('-', "_");
        let s = match s.as_str() {
            "shape" | "set_shape" => "arc",
            "signature_tracks" | "tracks" => "signature",
            "player" | "player_style" | "settings" => "style",
            "headline" | "tags" => "summary",
            "harmony" => "mixing",
            "bpm" => "tempo",
            "mood" => "energy",
            other => other,
        };
        Section::ALL.into_iter().find(|x| x.name() == s)
    }
}

impl std::fmt::Display for Section {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Precision {
    /// Counts as "at least" round numbers, shares to 5%, BPM to whole numbers.
    #[default]
    Rounded,
    Exact,
}

/// The DJ's choices about their profile, kept in the store.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProfilePrefs {
    /// Sections the document includes (`None`: the defaults, see
    /// [`Section::published_by_default`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sections: Option<BTreeSet<Section>>,
    #[serde(default)]
    pub precision: Precision,
    /// Tracks ("Artist - Title") to list first among signature tracks, in this order.
    #[serde(default)]
    pub pinned: Vec<String>,
    /// Tracks never named (unreleased promos, edits, dubplates). They still count in the numbers.
    #[serde(default)]
    pub hidden: Vec<String>,
    /// Artists never named, wherever they're credited.
    #[serde(default)]
    pub hidden_artists: Vec<String>,
    /// The DJ's genre spellings: genre text as tagged → genre as shown.
    #[serde(default)]
    pub genre_aliases: IndexMap<String, String>,
    /// The DJ's own one-line description, instead of the generated one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
}

impl ProfilePrefs {
    pub fn load(store: &Store) -> Result<ProfilePrefs> {
        Ok(fsutil::read_json_opt(&store.root().join(PREFS_FILE))?.unwrap_or_default())
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        fsutil::write_json_atomic(&store.root().join(PREFS_FILE), self)
    }

    /// The sections the document includes.
    pub fn published(&self) -> BTreeSet<Section> {
        self.sections.clone().unwrap_or_else(|| Section::ALL.into_iter().filter(|s| s.published_by_default()).collect())
    }

    pub fn is_pinned(&self, name: &str) -> bool {
        let n = normalize(name);
        self.pinned.iter().any(|p| normalize(p) == n)
    }

    pub fn is_hidden_track(&self, name: &str) -> bool {
        let n = normalize(name);
        self.hidden.iter().any(|h| normalize(h) == n)
    }

    pub fn is_hidden_artist(&self, artist: &str) -> bool {
        // Every name the profile counts (`artist_credits`: "A; B", "A (feat. B)") as well as the
        // looser split, so hiding any artist the profile could list hides the credit too.
        let mut credited = credited_artists(artist);
        credited.extend(artist_credits(artist).iter().map(|c| normalize(c)));
        self.hidden_artists.iter().any(|h| {
            let h = normalize(h);
            normalize(artist) == h || credited.contains(&h)
        })
    }

    /// Whether a track may be named in the document.
    pub fn may_name(&self, name: &str, artist: &str) -> bool {
        !self.is_hidden_track(name) && !self.is_hidden_artist(artist)
    }

    pub fn apply(&mut self, u: &PrefsUpdate) -> Result<()> {
        if u.reset {
            *self = ProfilePrefs::default();
        }
        if let Some(sections) = &u.sections {
            self.sections = Some(sections.iter().copied().collect());
        }
        if !u.include.is_empty() || !u.exclude.is_empty() {
            let mut s = self.published();
            s.extend(u.include.iter().copied());
            for x in &u.exclude {
                s.remove(x);
            }
            self.sections = Some(s);
        }
        if u.default_sections {
            self.sections = None;
        }
        if let Some(p) = u.precision {
            self.precision = p;
        }
        for name in &u.pin {
            let name = name.trim();
            if name.is_empty() {
                return Err(Error::invalid("can't pin an empty track name"));
            }
            // A hidden track can't be a pinned one (hiding unpins, see below).
            if self.is_hidden_track(name) {
                return Err(Error::invalid(format!("{name:?} is hidden; unhide it before pinning it")));
            }
            if !self.is_pinned(name) {
                self.pinned.push(name.to_string());
            }
        }
        for name in &u.unpin {
            let before = self.pinned.len();
            self.pinned.retain(|p| normalize(p) != normalize(name));
            if self.pinned.len() == before {
                return Err(Error::NotFound(format!("{name:?} isn't pinned")));
            }
        }
        for name in &u.hide {
            let name = name.trim();
            if name.is_empty() {
                return Err(Error::invalid("can't hide an empty track name"));
            }
            if !self.is_hidden_track(name) {
                self.hidden.push(name.to_string());
            }
            // A hidden track can't be a pinned one.
            self.pinned.retain(|p| normalize(p) != normalize(name));
        }
        for name in &u.unhide {
            let before = self.hidden.len();
            self.hidden.retain(|h| normalize(h) != normalize(name));
            if self.hidden.len() == before {
                return Err(Error::NotFound(format!("{name:?} isn't hidden")));
            }
        }
        for artist in &u.hide_artist {
            let artist = artist.trim();
            if artist.is_empty() {
                return Err(Error::invalid("can't hide an empty artist name"));
            }
            if !self.hidden_artists.iter().any(|a| normalize(a) == normalize(artist)) {
                self.hidden_artists.push(artist.to_string());
            }
        }
        for artist in &u.unhide_artist {
            let before = self.hidden_artists.len();
            self.hidden_artists.retain(|a| normalize(a) != normalize(artist));
            if self.hidden_artists.len() == before {
                return Err(Error::NotFound(format!("{artist:?} isn't hidden")));
            }
        }
        for (from, to) in &u.alias {
            if from.trim().is_empty() || to.trim().is_empty() {
                return Err(Error::invalid("a genre alias needs both the tagged text and the genre"));
            }
            self.genre_aliases.retain(|k, _| normalize(k) != normalize(from));
            self.genre_aliases.insert_sorted(from.trim().to_string(), to.trim().to_string());
        }
        for from in &u.unalias {
            let before = self.genre_aliases.len();
            self.genre_aliases.retain(|k, _| normalize(k) != normalize(from));
            if self.genre_aliases.len() == before {
                return Err(Error::NotFound(format!("no genre alias for {from:?}")));
            }
        }
        if let Some(h) = &u.headline {
            let h = h.trim();
            self.headline = (!h.is_empty()).then(|| h.to_string());
        }
        Ok(())
    }
}

/// Changes to [`ProfilePrefs`], applied in field order after `reset`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct PrefsUpdate {
    /// Start again from the defaults.
    #[serde(default)]
    pub reset: bool,
    /// Publish exactly these sections.
    #[serde(default)]
    pub sections: Option<Vec<Section>>,
    #[serde(default)]
    pub include: Vec<Section>,
    #[serde(default)]
    pub exclude: Vec<Section>,
    /// Go back to the default sections.
    #[serde(default)]
    pub default_sections: bool,
    #[serde(default)]
    pub precision: Option<Precision>,
    #[serde(default)]
    pub pin: Vec<String>,
    #[serde(default)]
    pub unpin: Vec<String>,
    #[serde(default)]
    pub hide: Vec<String>,
    #[serde(default)]
    pub unhide: Vec<String>,
    #[serde(default)]
    pub hide_artist: Vec<String>,
    #[serde(default)]
    pub unhide_artist: Vec<String>,
    #[serde(default)]
    pub alias: IndexMap<String, String>,
    #[serde(default)]
    pub unalias: Vec<String>,
    /// The DJ's own headline (empty: back to the generated one).
    #[serde(default)]
    pub headline: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// The document
// ---------------------------------------------------------------------------------------------

/// A count: exact, or (rounded) a round number it's at least.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Count {
    Exact {
        count: u64,
    },
    AtLeast {
        #[serde(rename = "atLeast")]
        at_least: u64,
    },
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDocument {
    pub version: u32,
    pub generated_at: DateTime<Utc>,
    pub precision: Precision,
    /// `plays`, `sessions` or `collection` (see [`Basis`]).
    pub basis: Basis,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sets: Option<DocSets>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tempo: Option<DocTempo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arc: Option<DocArc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mixing: Option<DocMixing>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keys: Option<DocKeys>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub energy: Option<DocEnergy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genres: Option<Vec<DocShare>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<DocShare>>,
    /// A track credited to several artists counts for each, so shares can add up to more than 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artists: Option<Vec<DocShare>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub era: Option<DocEra>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<Vec<DocTrack>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prep: Option<DocPrep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection: Option<DocCollection>,
    /// Setting key → value, e.g. `{"quantize": "off", "jogMode": "vinyl"}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<IndexMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocSets {
    #[serde(flatten)]
    pub count: Count,
    /// Year of the oldest set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<i32>,
    pub median_tracks: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocTempo {
    pub low: f64,
    pub high: f64,
    pub median: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ArcShape {
    Builds,
    Steady,
    WindsDown,
    Varies,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocArc {
    pub shape: ArcShape,
    pub builds: f64,
    pub steady: f64,
    pub winds_down: f64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocMixing {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harmonic: Option<f64>,
    /// Transitions that change tempo by 3% or less.
    #[serde(rename = "smoothTempo", skip_serializing_if = "Option::is_none")]
    pub smooth_tempo: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre_changes: Option<f64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocKeys {
    pub top: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minor: Option<f64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocEnergy {
    pub high: f64,
    pub mid: f64,
    pub low: f64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocShare {
    pub name: String,
    pub share: f64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocEra {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_year: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recent: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocTrack {
    pub artist: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocPrep {
    pub hot_cues: f64,
    pub memory_cues: f64,
    pub loops: f64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocCollection {
    pub tracks: Count,
    pub playlists: Count,
}

struct Round(Precision);

impl Round {
    /// What a share is rounded to: 5%, or 0.1% exactly.
    fn units(&self) -> f64 {
        match self.0 {
            Precision::Rounded => 20.0,
            Precision::Exact => 1000.0,
        }
    }

    fn share(&self, x: f64) -> f64 {
        (x * self.units()).round() / self.units()
    }

    /// Shares of one whole (`parts`, and the rest they leave out), rounded as [`Round::share`]
    /// rounds but so that they still add up: rounding each on its own makes 84/8/8% into
    /// 85/10/10%. Each is rounded down, and what that leaves goes to the largest remainders.
    fn shares(&self, parts: &[f64]) -> Vec<f64> {
        let units = self.units();
        let rest = (1.0 - parts.iter().sum::<f64>()).max(0.0);
        let scaled: Vec<f64> = parts.iter().chain([&rest]).map(|x| x.max(0.0) * units).collect();
        let mut out: Vec<f64> = scaled.iter().map(|x| x.floor()).collect();
        let left = (scaled.iter().sum::<f64>().round() - out.iter().sum::<f64>()).max(0.0) as usize;
        let mut by_remainder: Vec<usize> = (0..scaled.len()).collect();
        by_remainder.sort_by(|&a, &b| (scaled[b] - out[b]).total_cmp(&(scaled[a] - out[a])));
        for &i in by_remainder.iter().take(left) {
            out[i] += 1.0;
        }
        out.truncate(parts.len());
        out.iter().map(|u| u / units).collect()
    }

    fn bpm(&self, x: f64) -> f64 {
        match self.0 {
            Precision::Rounded => x.round(),
            Precision::Exact => (x * 10.0).round() / 10.0,
        }
    }

    fn count(&self, n: usize) -> Count {
        let n = n as u64;
        match self.0 {
            Precision::Exact => Count::Exact { count: n },
            Precision::Rounded if n < 10 => Count::Exact { count: n },
            Precision::Rounded => {
                let step = match n {
                    0..100 => 5,
                    100..1000 => 50,
                    1000..10_000 => 500,
                    _ => 5000,
                };
                Count::AtLeast { at_least: n / step * step }
            }
        }
    }
}

/// The section whose numbers back a summary trait (a [`super::Tag`]'s id): the trait tells as
/// much as the section does, so it's only shown with it. `None` for a trait it doesn't know.
pub fn tag_section(id: &str) -> Option<Section> {
    Some(match id {
        "harmonic" | "smooth_tempo" | "genre_hopping" => Section::Mixing,
        "focused" => Section::Genres,
        "builds" => Section::Arc,
        "fresh" | "digger" => Section::Era,
        "rarely_repeats" => Section::Sets,
        "signature" => Section::Signature,
        "prepared" => Section::Prep,
        "quantize_off" | "wide_tempo" => Section::Style,
        _ => return None,
    })
}

/// "Artist - Title" as artist and title, for a pinned name that isn't among the played tracks.
fn split_name(name: &str) -> DocTrack {
    match name.split_once(" - ") {
        Some((a, t)) => DocTrack { artist: a.trim().to_string(), title: t.trim().to_string() },
        None => DocTrack { artist: String::new(), title: name.trim().to_string() },
    }
}

/// The public part of `report`, as `prefs` allow.
pub fn document(report: &ProfileReport, prefs: &ProfilePrefs) -> ProfileDocument {
    let on = prefs.published();
    let shows = |s: Section| on.contains(&s) && report.basis.publishes(s);
    let r = Round(prefs.precision);
    let mut d = ProfileDocument {
        version: DOCUMENT_VERSION,
        generated_at: report.generated,
        precision: prefs.precision,
        basis: report.basis,
        headline: None,
        tags: Vec::new(),
        sets: None,
        tempo: None,
        arc: None,
        mixing: None,
        keys: None,
        energy: None,
        genres: None,
        labels: None,
        artists: None,
        era: None,
        signature: None,
        prep: None,
        collection: None,
        style: None,
    };

    if on.contains(&Section::Summary) {
        // The generated headline says only what the sections in the document say, counted as
        // they count.
        let count = |n| match r.count(n) {
            Count::Exact { count } => count.to_string(),
            Count::AtLeast { at_least } => format!("{at_least}+"),
        };
        let headline = prefs.headline.clone().unwrap_or_else(|| super::headline(report, shows, count));
        d.headline = (!headline.is_empty()).then_some(headline);
        // A trait is shown only with the section whose numbers back it.
        d.tags = report
            .summary
            .tags
            .iter()
            .filter(|t| tag_section(t.id).is_some_and(shows))
            .map(|t| t.label.clone())
            .collect();
    }
    let s = &report.sets;
    if shows(Section::Sets) && s.count > 0 {
        d.sets = Some(DocSets {
            count: r.count(s.count),
            since: s.oldest.map(|d| d.year()),
            median_tracks: s.median_tracks as u64,
        });
    }
    if on.contains(&Section::Tempo)
        && let (Some(core), Some(median)) = (report.tempo.core, report.tempo.median)
    {
        d.tempo = Some(DocTempo { low: r.bpm(core.low), high: r.bpm(core.high), median: r.bpm(median) });
    }
    let a = &report.arc;
    if on.contains(&Section::Arc) && a.sets > 0 {
        // The shape from the shares as published, so the two never disagree.
        let s = r.shares(&[a.builds, a.steady, a.winds_down]);
        let (builds, steady, winds_down) = (s[0], s[1], s[2]);
        let shape = if builds >= 0.5 {
            ArcShape::Builds
        } else if steady >= 0.5 {
            ArcShape::Steady
        } else if winds_down >= 0.5 {
            ArcShape::WindsDown
        } else {
            ArcShape::Varies
        };
        d.arc = Some(DocArc { shape, builds, steady, winds_down });
    }
    let m = &report.mixing;
    if on.contains(&Section::Mixing) && m.transitions > 0 {
        d.mixing = Some(DocMixing {
            harmonic: m.harmonic_share.map(|x| r.share(x)),
            smooth_tempo: m.within_3pct.map(|x| r.share(x)),
            genre_changes: m.genre_change_share.map(|x| r.share(x)),
        });
    }
    if on.contains(&Section::Keys) && report.keys.tracks > 0 {
        d.keys = Some(DocKeys {
            top: report.keys.top.iter().take(3).map(|k| k.name.clone()).collect(),
            minor: report.keys.minor_share.map(|x| r.share(x)),
        });
    }
    if on.contains(&Section::Energy)
        && let Some(e) = &report.energy
    {
        let s = r.shares(&[e.high, e.mid, e.low]);
        d.energy = Some(DocEnergy { high: s[0], mid: s[1], low: s[2] });
    }
    if on.contains(&Section::Genres) && !report.genres.top.is_empty() {
        let top = &report.genres.top[..report.genres.top.len().min(5)];
        let shares = r.shares(&top.iter().map(|g| g.share).collect::<Vec<_>>());
        d.genres = Some(
            top.iter()
                .zip(shares)
                .map(|(g, share)| DocShare { name: g.name.clone(), share })
                .filter(|g| g.share > 0.0)
                .collect(),
        );
    }
    if on.contains(&Section::Labels) && !report.labels.top.is_empty() {
        // A track has one label: the shares are parts of one whole, like genres'.
        let top = &report.labels.top[..report.labels.top.len().min(5)];
        let shares = r.shares(&top.iter().map(|l| l.share).collect::<Vec<_>>());
        d.labels = Some(top.iter().zip(shares).map(|(l, share)| DocShare { name: l.name.clone(), share }).collect());
    }
    if on.contains(&Section::Artists) {
        let artists: Vec<DocShare> = report
            .artists
            .top
            .iter()
            .filter(|a| !prefs.is_hidden_artist(&a.name))
            .take(5)
            .map(|a| DocShare { name: a.name.clone(), share: r.share(a.share) })
            .collect();
        d.artists = (!artists.is_empty()).then_some(artists);
    }
    if on.contains(&Section::Era) && report.era.tracks > 0 {
        d.era =
            Some(DocEra { median_year: report.era.median_year, recent: report.era.recent_share.map(|x| r.share(x)) });
    }
    if on.contains(&Section::Signature) {
        let mut tracks: Vec<DocTrack> = Vec::new();
        let played = &report.signature.played;
        for pin in &prefs.pinned {
            let p = played.iter().find(|p| normalize(&p.name) == normalize(pin));
            let t = p
                .map(|p| DocTrack { artist: p.artist.clone(), title: p.title.clone() })
                .unwrap_or_else(|| split_name(pin));
            if prefs.may_name(pin, &t.artist) && !p.is_some_and(|p| p.unnamed) && !tracks.contains(&t) {
                tracks.push(t);
            }
        }
        for p in &report.signature.tracks {
            if tracks.len() >= report.options.top.max(prefs.pinned.len()) {
                break;
            }
            let t = DocTrack { artist: p.artist.clone(), title: p.title.clone() };
            if prefs.may_name(&p.name, &p.artist) && !p.unnamed && !tracks.contains(&t) {
                tracks.push(t);
            }
        }
        d.signature = (!tracks.is_empty()).then_some(tracks);
    }
    if on.contains(&Section::Prep)
        && let Some(p) = &report.prep
    {
        d.prep = Some(DocPrep {
            hot_cues: r.share(p.hot_cue_share),
            memory_cues: r.share(p.memory_cue_share),
            loops: r.share(p.loop_share),
        });
    }
    if on.contains(&Section::Collection) && report.collection.tracks > 0 {
        d.collection = Some(DocCollection {
            tracks: r.count(report.collection.tracks),
            playlists: r.count(report.collection.playlists),
        });
    }
    if on.contains(&Section::Style)
        && let Some(s) = &report.style
    {
        d.style = Some(s.values.iter().map(|v| (camel(&v.key), v.value.clone())).collect());
    }
    d
}

/// `jog_mode` → `jogMode`.
fn camel(key: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for c in key.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_round_down_to_round_numbers() {
        let r = Round(Precision::Rounded);
        assert_eq!(r.count(7), Count::Exact { count: 7 });
        assert_eq!(r.count(47), Count::AtLeast { at_least: 45 });
        assert_eq!(r.count(3886), Count::AtLeast { at_least: 3500 });
        assert_eq!(Round(Precision::Exact).count(47), Count::Exact { count: 47 });
        assert_eq!(serde_json::to_value(r.count(47)).unwrap(), serde_json::json!({ "atLeast": 45 }));
        assert_eq!(r.share(0.68), 0.7);
        assert_eq!(r.share(0.66), 0.65);
    }

    #[test]
    fn shares_of_a_whole_still_add_up_rounded() {
        let r = Round(Precision::Rounded);
        assert_eq!(r.shares(&[0.84, 0.08, 0.08]), [0.85, 0.1, 0.05]);
        assert_eq!(r.shares(&[0.86, 0.07, 0.07]), [0.85, 0.1, 0.05]);
        // Part of a whole (the rest is 35%): together they're still 65%.
        assert_eq!(r.shares(&[0.31, 0.22, 0.12]), [0.3, 0.25, 0.1]);
        assert_eq!(Round(Precision::Exact).shares(&[1.0 / 3.0; 3]), [0.334, 0.333, 0.333]);
    }

    #[test]
    fn sections_parse_by_name_and_alias() {
        assert_eq!(Section::parse("signature-tracks"), Some(Section::Signature));
        assert_eq!(Section::parse("Set-Shape"), Some(Section::Arc));
        assert_eq!(Section::parse("nope"), None);
        for s in Section::ALL {
            assert_eq!(Section::parse(s.name()), Some(s));
        }
    }

    #[test]
    fn prefs_updates_validate_and_normalise() {
        let mut p = ProfilePrefs::default();
        assert!(p.published().contains(&Section::Tempo));
        assert!(!p.published().contains(&Section::Signature));
        p.apply(&PrefsUpdate {
            include: vec![Section::Signature],
            exclude: vec![Section::Style],
            ..Default::default()
        })
        .unwrap();
        assert!(p.published().contains(&Section::Signature));
        assert!(!p.published().contains(&Section::Style));

        p.apply(&PrefsUpdate { pin: vec!["Bicep - Glue".into(), "bicep -  glue".into()], ..Default::default() })
            .unwrap();
        assert_eq!(p.pinned, ["Bicep - Glue"]);
        // Hiding a pinned track unpins it.
        p.apply(&PrefsUpdate { hide: vec!["BICEP - GLUE".into()], ..Default::default() }).unwrap();
        assert!(p.pinned.is_empty());
        assert!(!p.may_name("Bicep - Glue", "Bicep"));
        assert!(p.apply(&PrefsUpdate { unpin: vec!["Bicep - Glue".into()], ..Default::default() }).is_err());

        p.apply(&PrefsUpdate { hide_artist: vec!["Clara La San".into()], ..Default::default() }).unwrap();
        assert!(!p.may_name("Bicep - Saku", "Bicep feat. Clara La San"));
        assert!(p.may_name("Bicep - Apricots", "Bicep"));

        p.apply(&PrefsUpdate { headline: Some("Warm-up specialist".into()), ..Default::default() }).unwrap();
        assert_eq!(p.headline.as_deref(), Some("Warm-up specialist"));
        p.apply(&PrefsUpdate { headline: Some(" ".into()), ..Default::default() }).unwrap();
        assert_eq!(p.headline, None);

        p.apply(&PrefsUpdate { reset: true, ..Default::default() }).unwrap();
        assert_eq!(p, ProfilePrefs::default());
    }

    #[test]
    fn every_trait_is_backed_by_a_section() {
        // Each `tag("id", …)` the summary can add: a trait without a section is never shown.
        let src = include_str!("mod.rs");
        let ids: Vec<&str> = src
            .match_indices("tag(")
            .filter_map(|(i, _)| src[i + 4..].trim_start().strip_prefix('"')?.split('"').next())
            .collect();
        assert!(ids.len() >= 12, "{ids:?}");
        for id in ids {
            assert!(tag_section(id).is_some(), "{id}");
        }
    }

    #[test]
    fn setting_keys_become_camel_case() {
        assert_eq!(camel("vinyl_speed_adjust"), "vinylSpeedAdjust");
        assert_eq!(camel("quantize"), "quantize");
    }
}
