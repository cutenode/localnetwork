//! A DJ's profile: what their sets, collection and player settings say about how they play.
//!
//! Everything comes from data locallibrary already reads: the sets the DJ declared (see
//! [`crate::sets`]), each track's rekordbox analysis (BPM, key, phrase mood, cues), its tags
//! (genre, label, year) and the drives' My Settings. A set's tracks are found in every library the
//! profile reads (mounted drives, the store's latest snapshots and the history lists `pull`
//! archived; see [`crate::plays`]) by file, for their analysis. [`build`] works these out into a
//! [`ProfileReport`], which stays on this computer.
//! [`document`] turns a report into a [`ProfileDocument`]: the coarser part the DJ approves for a
//! public profile (publishing it to the AT Protocol comes later). The DJ's choices (which sections
//! go out, tracks to name or never name, rounding, genre spellings) are [`ProfilePrefs`], kept in
//! the store.
//!
//! Most numbers describe what was *played* (every appearance in a set counts). Until the DJ
//! declares sets, the sessions players recorded stand in for them ([`Basis::Sessions`]): practice
//! and track checks land in those too, so the report says so and the document never counts them.
//! Without either, the collection is described instead ([`Basis::Collection`]).
//!
//! What a profile can't say: anything that compares a DJ with other DJs (tracks only they play),
//! since that needs other DJs' data.

mod document;

pub use document::*;

use indexmap::IndexMap;

use chrono::{DateTime, Datelike, Local, NaiveDate, Utc};
use rustc_hash::{FxHashMap, FxHashSet};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::anlz::{self, Mood};
use crate::device::Device;
use crate::doctor::Databases;
use crate::harmony::{Camelot, Relation, tempo_gap};
use crate::hash::Hash;
use crate::model::{BpmRange, Library, Track, normalize};
use crate::paths;
use crate::plays::{self, HistoryList};
use crate::progress::{Ctx, Event, Phase};
use crate::sets::{Set, SetBook, SetEntry};
use crate::settings::{self, DriveSettings, SettingValue};
use crate::store::{Snapshot, Store};
use crate::{Error, Result};

pub const DEFAULT_MIN_SET_TRACKS: usize = 5;
pub const DEFAULT_TOP: usize = 10;

/// Settings that say something about how a DJ plays, in display order.
pub const STYLE_SETTINGS: [&str; 7] =
    ["quantize", "sync", "master_tempo", "jog_mode", "vinyl_speed_adjust", "tempo_range", "auto_cue"];

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProfileOptions {
    /// Only sets played on or after this day. Sessions (without declared sets) count from the day
    /// a pull first archived them; those still only on a drive are recent, so they always count.
    #[serde(default)]
    pub since: Option<NaiveDate>,
    /// Only sets played on or before this day (sessions still only on a drive don't count).
    #[serde(default)]
    pub until: Option<NaiveDate>,
    /// Without declared sets: sessions with fewer tracks (sound checks, previews) don't count.
    #[serde(default = "default_min_set_tracks")]
    pub min_set_tracks: usize,
    /// How many entries ranked lists keep.
    #[serde(default = "default_top")]
    pub top: usize,
    /// Read analysis files for phrase mood and cue statistics (one or two small files per track).
    #[serde(default = "yes")]
    pub analysis: bool,
    /// The DJ's genre spellings, e.g. `{"dnb": "Drum & Bass"}` (see [`ProfilePrefs`]).
    #[serde(default)]
    pub genre_aliases: IndexMap<String, String>,
}

fn default_min_set_tracks() -> usize {
    DEFAULT_MIN_SET_TRACKS
}

fn default_top() -> usize {
    DEFAULT_TOP
}

fn yes() -> bool {
    true
}

impl Default for ProfileOptions {
    fn default() -> Self {
        ProfileOptions {
            since: None,
            until: None,
            min_set_tracks: DEFAULT_MIN_SET_TRACKS,
            top: DEFAULT_TOP,
            analysis: true,
            genre_aliases: IndexMap::new(),
        }
    }
}

/// Where a profile's data comes from.
pub struct Sources<'a> {
    pub drives: Vec<Device>,
    /// The store: its archived history lists and its libraries' latest snapshots.
    pub store: Option<&'a Store>,
    /// Store libraries to read (`None`: all of them).
    pub libraries: Option<Vec<Uuid>>,
}

// ---------------------------------------------------------------------------------------------
// The report
// ---------------------------------------------------------------------------------------------

/// What the numbers describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// Tracks as played in the DJ's sets (a track played in three sets counts three times).
    Plays,
    /// No sets declared (or the store left out): tracks as played in the sessions players
    /// recorded with `min_set_tracks` or more tracks, which can be practice as well as gigs.
    Sessions,
    /// Nothing played to count (no sets in the range, or no sets declared and no sessions): the
    /// collection, each track once.
    Collection,
}

impl Basis {
    /// What the plays were counted in: `set`, or `session` without declared sets.
    pub fn unit(self) -> &'static str {
        match self {
            Basis::Sessions => "session",
            Basis::Plays | Basis::Collection => "set",
        }
    }

    /// Whether a document on this basis shows section `s`: sessions are never counted in public.
    pub fn publishes(self, s: Section) -> bool {
        !(self == Basis::Sessions && s == Section::Sets)
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProfileReport {
    pub generated: DateTime<Utc>,
    pub options: ProfileOptions,
    pub basis: Basis,
    pub sources: Vec<SourceInfo>,
    pub summary: Summary,
    pub sets: SetsSection,
    pub tempo: TempoSection,
    pub arc: ArcSection,
    pub mixing: MixingSection,
    pub keys: KeysSection,
    /// `None` without analysis files.
    pub energy: Option<EnergySection>,
    pub genres: RankedSection,
    pub labels: RankedSection,
    pub artists: RankedSection,
    pub era: EraSection,
    pub signature: SignatureSection,
    /// `None` without analysis files.
    pub prep: Option<PrepSection>,
    pub collection: CollectionSection,
    /// `None` when no drive has My Settings files and the store has no settings profile.
    pub style: Option<StyleSection>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SourceInfo {
    /// `drive` or `store`.
    pub kind: &'static str,
    pub name: String,
    /// History lists (sessions) read.
    pub history_lists: usize,
    pub tracks: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[schemars(rename = "ProfileSummary")]
pub struct Summary {
    pub headline: String,
    pub tags: Vec<Tag>,
}

/// A trait the numbers support, with the evidence.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Tag {
    pub id: &'static str,
    pub label: String,
    pub why: String,
}

/// The sets the numbers come from: the DJ's, or the sessions standing in for them (see
/// [`ProfileReport::basis`]).
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct SetsSection {
    pub count: usize,
    /// Track appearances across every set.
    pub plays: usize,
    pub distinct_tracks: usize,
    pub median_tracks: usize,
    pub longest: usize,
    /// The days of the oldest and newest sets ([`SetInfo::date`]).
    pub oldest: Option<NaiveDate>,
    pub newest: Option<NaiveDate>,
    /// Sessions only on a drive, which the store hasn't archived yet.
    pub not_archived: usize,
    /// Sessions left out for having fewer than `min_set_tracks` tracks.
    pub too_short: usize,
    /// Newest first (sessions only on a drive, undated, first).
    pub list: Vec<SetInfo>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SetInfo {
    /// The set's id, or the session's (its history list's fingerprint, see
    /// [`crate::sets::Session::id`]).
    pub id: String,
    /// The set's title, else its venue; the session's history list (folders and name).
    pub name: String,
    /// Where: the set's venue and city; the drive a session was read from, or whose snapshot
    /// archived it.
    pub place: String,
    /// The day the set was played. A session has no such day: this is the day a pull first
    /// archived it (it was played then or before), and none while it's only on a drive.
    pub date: Option<NaiveDate>,
    pub tracks: usize,
    pub opening_bpm: Option<f64>,
    pub closing_bpm: Option<f64>,
    /// Share of transitions between keyed tracks that are harmonic.
    pub harmonic_share: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct TempoSection {
    /// Plays (or tracks) with a BPM.
    pub tracks: usize,
    pub median: Option<f64>,
    /// Where the middle 80% sits: the 10th and 90th percentiles.
    pub core: Option<BpmRange>,
    pub p25: Option<f64>,
    pub p75: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub bin_width: f64,
    /// Covering the 2nd to 98th percentile.
    pub histogram: Vec<TempoBin>,
}

crate::schema::labelled_tuple! {
    /// A bar of the tempo histogram: `[bpm, count]`.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct TempoBin {
        /// The lowest BPM in the bin.
        pub bpm: f64,
        pub count: usize,
    }
}

crate::schema::labelled_tuple! {
    /// A Camelot key and how often it comes: `[key, count]`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct KeyCount {
        pub key: String,
        pub count: usize,
    }
}

crate::schema::labelled_tuple! {
    /// A decade (`1990`) and how many tracks were released in it: `[decade, count]`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DecadeCount {
        pub decade: i32,
        pub count: usize,
    }
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct ArcSection {
    /// Sets long enough to have a shape (6+ tracks with a BPM).
    pub sets: usize,
    /// Median BPM over the first and last quarter of a set, averaged over sets.
    pub opening_bpm: Option<f64>,
    pub closing_bpm: Option<f64>,
    /// Shares of sets whose closing quarter is at least 2 BPM faster, within 2 BPM, or at least
    /// 2 BPM slower than their opening quarter.
    pub builds: f64,
    pub steady: f64,
    pub winds_down: f64,
    /// BPM relative to each set's median, across ten stretches of a set, averaged over sets.
    pub curve: Vec<Option<f64>>,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct MixingSection {
    /// Pairs of tracks played one after the other within sets (not across a track gone from
    /// the library, nor a track played twice in a row).
    pub transitions: usize,
    /// Transitions where both keys are known.
    pub keyed: usize,
    pub same_key: usize,
    pub adjacent: usize,
    pub relative: usize,
    pub energy_boost: usize,
    pub clash: usize,
    /// (same + adjacent + relative + energy boost) / keyed.
    pub harmonic_share: Option<f64>,
    /// Transitions where both BPMs are known.
    pub timed: usize,
    /// Median tempo change between consecutive tracks, in percent (half and double time count
    /// as the same tempo).
    pub median_step_pct: Option<f64>,
    pub within_3pct: Option<f64>,
    pub within_6pct: Option<f64>,
    /// Transitions that only line up at half or double time.
    pub half_double: usize,
    /// Transitions where both tracks have a genre.
    pub genred: usize,
    pub genre_change_share: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct KeysSection {
    pub tracks: usize,
    /// Every Camelot key in wheel order (1A, 1B, 2A ... 12B), with its count.
    pub wheel: Vec<KeyCount>,
    pub minor_share: Option<f64>,
    pub top: Vec<Share>,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct EnergySection {
    /// Plays (or tracks) whose phrase analysis was read.
    pub tracks: usize,
    pub high: f64,
    pub mid: f64,
    pub low: f64,
    /// Average mood (high 1, mid 0.5, low 0) across ten stretches of a set.
    pub curve: Vec<Option<f64>>,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct RankedSection {
    /// Plays (or tracks) with a value.
    pub known: usize,
    pub distinct: usize,
    pub top: Vec<Share>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Share {
    pub name: String,
    pub count: usize,
    /// Of `known`.
    pub share: f64,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct EraSection {
    /// Plays (or tracks) with a release year.
    pub tracks: usize,
    pub median_year: Option<i32>,
    pub oldest: Option<i32>,
    pub newest: Option<i32>,
    /// Released the year it was played, or the year before.
    pub recent_share: Option<f64>,
    /// Oldest first.
    pub decades: Vec<DecadeCount>,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct SignatureSection {
    /// Played in two or more sets, most sets first.
    pub tracks: Vec<PlayedTrack>,
    /// The same move from one track into another, in two or more sets.
    pub transitions: Vec<SignatureTransition>,
    /// Opened (or closed) two or more sets.
    pub openers: Vec<PlayedTrack>,
    pub closers: Vec<PlayedTrack>,
    /// Every track played, most sets first.
    pub played: Vec<PlayedTrack>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlayedTrack {
    pub name: String,
    pub artist: String,
    pub title: String,
    /// Sets it appears in.
    pub sets: usize,
    pub plays: usize,
    pub opened: usize,
    pub closed: usize,
    /// A declared set marks it unnamed (`sets edit --unnamed`): the public document never names
    /// it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unnamed: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SignatureTransition {
    pub from: String,
    pub to: String,
    pub sets: usize,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct PrepSection {
    /// Collection tracks whose cues were read.
    pub tracks: usize,
    pub hot_cue_share: f64,
    /// Hot cues per track, on tracks that have any.
    pub hot_cues_per_track: Option<f64>,
    pub memory_cue_share: f64,
    pub loop_share: f64,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct CollectionSection {
    pub tracks: usize,
    pub playlists: usize,
    pub folders: usize,
    pub rated_share: Option<f64>,
    pub coloured_share: Option<f64>,
    /// Tracks played in at least one set.
    pub played_share: Option<f64>,
    /// Extra files of a recording already counted (the most any one library has): copies such
    /// as `Track-1.mp3` left by repeated exports. `locallibrary doctor` lists them.
    pub duplicate_files: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StyleSection {
    /// `settings profile` (the store's) or the drive the files were read from.
    pub source: String,
    pub values: Vec<StyleValue>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StyleValue {
    pub key: String,
    pub label: String,
    pub value: String,
    pub value_label: String,
}

// ---------------------------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------------------------

/// Where a track's analysis files can be read.
#[derive(Debug, Clone, Copy)]
enum Origin {
    Drive(usize),
    Snapshot(usize),
}

struct Files<'a> {
    drives: &'a [Device],
    store: Option<&'a Store>,
    /// Lower-cased path key to object, per loaded snapshot.
    snapshots: Vec<FxHashMap<String, Hash>>,
    by_id: FxHashMap<Hash, usize>,
}

impl<'a> Files<'a> {
    fn snapshot(&mut self, snap: &Snapshot) -> usize {
        if let Some(&i) = self.by_id.get(&snap.id) {
            return i;
        }
        let map = snap.files.iter().filter_map(|f| Some((f.path.to_lowercase(), f.hash?))).collect();
        self.snapshots.push(map);
        self.by_id.insert(snap.id, self.snapshots.len() - 1);
        self.snapshots.len() - 1
    }

    fn read(&self, origin: Origin, key: &str) -> Option<Vec<u8>> {
        match origin {
            Origin::Drive(i) => self.drives[i].checked_path(key).ok().and_then(|p| std::fs::read(p).ok()),
            Origin::Snapshot(i) => {
                let h = self.snapshots[i].get(&key.to_lowercase())?;
                std::fs::read(self.store?.object_path(h)).ok()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct CueCounts {
    hot: usize,
    memory: usize,
    loops: usize,
}

/// One recording, however many libraries list it.
struct Rec {
    track: Track,
    origin: Option<Origin>,
    in_collection: bool,
    camelot: Option<Camelot>,
    genres: Vec<String>,
    year: Option<i32>,
    mood: Option<Mood>,
    cues: Option<CueCounts>,
    /// A declared set marks it unnamed.
    unnamed: bool,
}

impl Rec {
    fn bpm(&self) -> Option<f64> {
        let b = self.track.bpm;
        (40.0..=300.0).contains(&b).then_some(b)
    }
}

/// A history list read, before the profile knows whether it stands in for a set.
struct ListData {
    list: HistoryList,
    /// The drive it was read from, or the drive whose snapshot archived it.
    source: String,
    first_seen: Option<DateTime<Utc>>,
    tracks: Vec<usize>,
}

/// A set as the profile counts it: one the DJ declared, or a session standing in for one (its
/// fields are [`SetInfo`]'s).
struct SetData {
    id: String,
    name: String,
    place: String,
    date: Option<NaiveDate>,
    tracks: Vec<usize>,
    /// Where entries whose track is gone from the library were: the positions in `tracks` they
    /// came before.
    missing_at: Vec<usize>,
}

impl SetData {
    /// Pairs of plays that were one after the other: not across an entry whose track is gone
    /// from the library, nor a track played twice in a row (reloaded, or on both decks).
    fn transitions(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.tracks
            .windows(2)
            .enumerate()
            .filter(|(i, w)| w[0] != w[1] && !self.missing_at.contains(&(i + 1)))
            .map(|(_, w)| (w[0], w[1]))
    }
}

struct Builder<'a> {
    options: &'a ProfileOptions,
    files: Files<'a>,
    recs: Vec<Rec>,
    by_key: FxHashMap<String, usize>,
    /// Every file of every library read ([`plays::file_key`]), to find a set's entries by.
    by_file: FxHashMap<String, usize>,
    lists: Vec<ListData>,
    sets: Vec<SetData>,
    playlists: FxHashSet<(bool, String)>,
    duplicate_files: usize,
    sources: Vec<SourceInfo>,
    warnings: Vec<String>,
}

/// Identity of a recording across libraries: artist, title and length, or the file when a
/// track has neither artist nor title.
fn rec_key(t: &Track) -> String {
    if t.artist.trim().is_empty() && t.title.trim().is_empty() {
        format!("file\u{1f}{}", plays::file_key(&t.file_path))
    } else {
        t.fingerprint_key()
    }
}

fn release_year(t: &Track) -> Option<i32> {
    let y = if t.year > 0 { i32::from(t.year) } else { t.release_date.get(..4)?.parse().ok()? };
    (1900..=Utc::now().year() + 1).contains(&y).then_some(y)
}

impl<'a> Builder<'a> {
    /// The recording `t` is, added when it's new. Its file finds it too ([`Builder::entry`]).
    fn rec(&mut self, t: &Track, origin: Option<Origin>) -> usize {
        let key = rec_key(t);
        let i = match self.by_key.get(&key) {
            Some(&i) => {
                if self.recs[i].origin.is_none() && !t.analyze_path.is_empty() {
                    self.recs[i].origin = origin;
                    self.recs[i].track.analyze_path = t.analyze_path.clone();
                }
                i
            }
            None => {
                self.recs.push(Rec {
                    track: t.clone(),
                    origin: origin.filter(|_| !t.analyze_path.is_empty()),
                    in_collection: false,
                    camelot: Camelot::parse(&t.key),
                    genres: genres(&t.genre, &self.options.genre_aliases),
                    year: release_year(t),
                    mood: None,
                    cues: None,
                    unnamed: false,
                });
                self.by_key.insert(key, self.recs.len() - 1);
                self.recs.len() - 1
            }
        };
        if !t.file_path.is_empty() {
            self.by_file.entry(plays::file_key(&t.file_path)).or_insert(i);
        }
        i
    }

    fn add_collection(&mut self, lib: &Library, origin: Origin) {
        let mut distinct = FxHashSet::default();
        for t in &lib.tracks {
            let i = self.rec(t, Some(origin));
            self.recs[i].in_collection = true;
            distinct.insert(i);
        }
        self.duplicate_files = self.duplicate_files.max(lib.tracks.len() - distinct.len());
        for p in &lib.playlists {
            self.playlists.insert((p.is_folder, normalize(&lib.playlist_path(p.id))));
        }
    }

    /// Adds a history list unless it's already there (the same list on two drives, or on a drive
    /// and in the store).
    fn add_list(&mut self, list: HistoryList, source: String, first_seen: Option<DateTime<Utc>>, origin: Origin) {
        if self.lists.iter().any(|l| l.list.same_as(&list)) {
            return;
        }
        let tracks = list.tracks.iter().map(|t| self.rec(t, Some(origin))).collect();
        self.lists.push(ListData { list, source, first_seen, tracks });
    }

    /// The recording a set's entry is: the library track with its file, else a track made of what
    /// the entry kept, with no analysis to read.
    fn entry(&mut self, e: &SetEntry) -> usize {
        if let Some(i) = e.file_key().and_then(|k| self.by_file.get(&k).copied()) {
            return i;
        }
        let track = Track {
            title: e.title.clone(),
            artist: e.artist.clone(),
            album: e.album.clone(),
            genre: e.genre.clone(),
            label: e.label.clone(),
            key: e.key.clone(),
            isrc: e.isrc.clone(),
            bpm: e.bpm,
            duration_secs: e.duration_secs,
            year: e.year,
            file_path: e.file.clone().unwrap_or_default(),
            ..Track::default()
        };
        self.rec(&track, None)
    }

    /// Without declared sets, the sessions in range with `min_set_tracks` or more tracks become
    /// the sets, newest first. Returns how many sessions were too short.
    fn sessions_as_sets(&mut self) -> usize {
        let o = self.options;
        // The local day, as sets and sessions' windows are dated.
        let day = |l: &ListData| l.first_seen.map(|t| t.with_timezone(&Local).date_naive());
        let (long, short): (Vec<ListData>, Vec<ListData>) =
            std::mem::take(&mut self.lists).into_iter().partition(|l| l.tracks.len() >= o.min_set_tracks);
        let mut sessions: Vec<ListData> = long
            .into_iter()
            .filter(|l| match day(l) {
                Some(d) => o.since.is_none_or(|x| d >= x) && o.until.is_none_or(|x| d <= x),
                // Still on a drive, not archived: played since the last pull.
                None => o.until.is_none(),
            })
            .collect();
        // Newest first: undated (still on a drive) first, then by date.
        sessions.sort_by(|a, c| match (a.first_seen, c.first_seen) {
            (None, None) => c.list.name.cmp(&a.list.name),
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (Some(x), Some(y)) => y.cmp(&x).then(c.list.name.cmp(&a.list.name)),
        });
        self.sets = sessions
            .into_iter()
            .map(|l| SetData {
                id: l.list.fingerprint(),
                date: day(&l),
                name: l.list.path,
                place: l.source,
                tracks: l.tracks,
                missing_at: l.list.missing_at,
            })
            .collect();
        short.len()
    }

    fn read_analysis(&mut self, played: &FxHashSet<usize>, ctx: &Ctx<'_>) -> Result<()> {
        let todo: Vec<usize> = (0..self.recs.len())
            .filter(|&i| self.recs[i].origin.is_some() && (played.contains(&i) || self.recs[i].in_collection))
            .collect();
        ctx.phase_start(Phase::Scan, todo.len() as u64, 0);
        for i in todo {
            ctx.check()?;
            let rec = &self.recs[i];
            let origin = rec.origin.expect("filtered");
            ctx.emit(Event::Item { phase: Phase::Scan, path: rec.track.display_name(), bytes: 0 });
            let dat = paths::key(&rec.track.analyze_path);
            // Every track's .EXT is read: it holds all of a track's hot cues, where the .DAT
            // holds A to C only (see `anlz::write`). Its phrases only played tracks need.
            let ext = dat.strip_suffix(".DAT").and_then(|s| self.files.read(origin, &format!("{s}.EXT")));
            let mut mood = None;
            let mut cue_list = None;
            if let Some(bytes) = &ext {
                if played.contains(&i) {
                    mood = anlz::song_structure(bytes).ok().flatten().map(|s| s.mood);
                }
                cue_list = anlz::cues(bytes).ok().filter(|c| !c.is_empty());
            }
            if cue_list.is_none() {
                cue_list = self.files.read(origin, &dat).and_then(|b| anlz::cues(&b).ok());
            }
            let rec = &mut self.recs[i];
            rec.mood = mood;
            rec.cues = cue_list.map(|cues| CueCounts {
                hot: cues.iter().filter(|c| c.hot_cue.is_some()).count(),
                memory: cues.iter().filter(|c| c.hot_cue.is_none() && c.end_ms.is_none()).count(),
                loops: cues.iter().filter(|c| c.end_ms.is_some()).count(),
            });
        }
        ctx.phase_end(Phase::Scan);
        Ok(())
    }
}

/// Works out a profile from drives and the store.
pub fn build(sources: &Sources<'_>, options: &ProfileOptions, ctx: &Ctx<'_>) -> Result<ProfileReport> {
    let mut b = Builder {
        options,
        files: Files {
            drives: &sources.drives,
            store: sources.store,
            snapshots: Vec::new(),
            by_id: FxHashMap::default(),
        },
        recs: Vec::new(),
        by_key: FxHashMap::default(),
        by_file: FxHashMap::default(),
        lists: Vec::new(),
        sets: Vec::new(),
        playlists: FxHashSet::default(),
        duplicate_files: 0,
        sources: Vec::new(),
        warnings: Vec::new(),
    };

    // The store first, so archived lists keep the date they were first seen.
    if let Some(store) = sources.store {
        let libraries: Vec<_> = store
            .libraries()?
            .into_iter()
            .filter(|l| sources.libraries.as_ref().is_none_or(|ids| ids.contains(&l.id)))
            .collect();
        let mut lists = 0;
        let mut tracks = 0;
        for lib in &libraries {
            ctx.check()?;
            let Some(head) = lib.head else { continue };
            let snap = store.load_snapshot(&head)?;
            match Databases::from_snapshot(store, &snap) {
                Ok(dbs) => {
                    let o = b.files.snapshot(&snap);
                    tracks += dbs.primary().tracks.len();
                    b.add_collection(dbs.primary(), Origin::Snapshot(o));
                }
                Err(e) => b.warnings.push(format!("library {}: {e}", lib.name)),
            }
        }
        let filters: Vec<Option<Uuid>> = match &sources.libraries {
            None => vec![None],
            Some(_) => libraries.iter().map(|l| Some(l.id)).collect(),
        };
        let mut snapshots: FxHashMap<Hash, Snapshot> = FxHashMap::default();
        for filter in filters {
            let archive = plays::archive(store, filter, ctx)?;
            b.warnings.extend(archive.warnings);
            for a in archive.lists {
                lists += 1;
                let id = a.first_seen.snapshot;
                if let std::collections::hash_map::Entry::Vacant(e) = snapshots.entry(id) {
                    e.insert(store.load_snapshot(&id)?);
                }
                let o = b.files.snapshot(&snapshots[&id]);
                let source = a.first_seen.drive.clone().unwrap_or_else(|| a.first_seen.library.clone());
                b.add_list(a.list, source, Some(a.first_seen.created), Origin::Snapshot(o));
            }
        }
        let n = libraries.len();
        if n > 0 || lists > 0 {
            b.sources.push(SourceInfo {
                kind: "store",
                name: format!("store ({n} librar{})", if n == 1 { "y" } else { "ies" }),
                history_lists: lists,
                tracks,
            });
        }
    }

    for (i, d) in sources.drives.iter().enumerate() {
        ctx.check()?;
        let dbs = match Databases::read(d) {
            Ok(dbs) => dbs,
            Err(e) => {
                b.warnings.push(format!("{}: {e}", d.name()));
                continue;
            }
        };
        if let Some(e) = &dbs.plus_error {
            b.warnings.push(format!("{}: {e}; only export.pdb was read", d.name()));
        }
        b.add_collection(dbs.primary(), Origin::Drive(i));
        let lists = plays::histories(&dbs);
        b.sources.push(SourceInfo {
            kind: "drive",
            name: d.name(),
            history_lists: lists.len(),
            tracks: dbs.primary().tracks.len(),
        });
        for list in lists {
            b.add_list(list, d.name(), None, Origin::Drive(i));
        }
    }

    if b.sources.is_empty() {
        return Err(Error::NotFound("nothing to build a profile from: no drive, and no library in the store".into()));
    }

    // The sets: the DJ's own, played in the range, newest first. Once there are any, sessions
    // never count, whatever the range.
    let book = sources.store.map(SetBook::load).transpose()?.unwrap_or_default();
    let mut in_range: Vec<_> = book
        .sets
        .iter()
        .filter(|s| options.since.is_none_or(|d| s.date >= d) && options.until.is_none_or(|d| s.date <= d))
        .collect();
    in_range.sort_by(|a, c| Set::newest_first(a, c));
    for s in in_range {
        let tracks = s
            .tracks
            .iter()
            .map(|e| {
                let i = b.entry(e);
                b.recs[i].unnamed |= e.unnamed;
                i
            })
            .collect();
        b.sets.push(SetData {
            id: s.id.clone(),
            name: s.name().unwrap_or("untitled").to_string(),
            place: s.place(),
            date: Some(s.date),
            tracks,
            missing_at: Vec::new(),
        });
    }
    // None declared: the sessions long enough to stand in for sets.
    let (counted, too_short) =
        if book.sets.is_empty() { (Basis::Sessions, b.sessions_as_sets()) } else { (Basis::Plays, 0) };

    let played: FxHashSet<usize> = b.sets.iter().flat_map(|s| s.tracks.iter().copied()).collect();
    if options.analysis {
        b.read_analysis(&played, ctx)?;
    }

    let style = style(sources);
    let generated = Utc::now();
    let mut report = compute(&b, counted, too_short, generated);
    report.style = style;
    report.summary = summarise(&report);
    Ok(report)
}

// ---------------------------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------------------------

fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let pos = p.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    Some(sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64))
}

fn median(values: &[f64]) -> Option<f64> {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    percentile(&v, 0.5)
}

fn share(part: usize, whole: usize) -> Option<f64> {
    (whole > 0).then(|| part as f64 / whole as f64)
}

/// Which tenth of a set position `i` of `n` falls in.
fn decile(i: usize, n: usize) -> usize {
    (i * 10 / n.max(1)).min(9)
}

fn ranked(counts: FxHashMap<String, (usize, FxHashMap<String, usize>)>, known: usize, top: usize) -> RankedSection {
    let distinct = counts.len();
    let mut v: Vec<Share> = counts
        .into_values()
        .map(|(count, spellings)| {
            // The spelling used most often.
            let name =
                spellings.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0))).map(|s| s.0).unwrap_or_default();
            Share { name, count, share: share(count, known).unwrap_or(0.0) }
        })
        .collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));
    v.truncate(top);
    RankedSection { known, distinct, top: v }
}

fn tally(counts: &mut FxHashMap<String, (usize, FxHashMap<String, usize>)>, name: &str) {
    let e = counts.entry(normalize(name)).or_default();
    e.0 += 1;
    *e.1.entry(name.trim().to_string()).or_default() += 1;
}

/// `counted`: what the sets are, [`Basis::Plays`] (declared) or [`Basis::Sessions`].
fn compute(b: &Builder<'_>, counted: Basis, too_short: usize, now: DateTime<Utc>) -> ProfileReport {
    let o = b.options;
    let recs = &b.recs;
    let sets = &b.sets;

    // The pool: every play, or the collection when there are no sets. Each play carries the year
    // it was played in (for "recent" releases).
    let mut pool: Vec<(usize, i32)> = Vec::new();
    for s in sets {
        let year = s.date.map_or(now.year(), |d| d.year());
        pool.extend(s.tracks.iter().map(|&i| (i, year)));
    }
    let basis = if pool.is_empty() {
        pool = (0..recs.len()).filter(|&i| recs[i].in_collection).map(|i| (i, now.year())).collect();
        Basis::Collection
    } else {
        counted
    };

    // Sets.
    let mut set_list = Vec::new();
    let mut lengths: Vec<f64> = Vec::new();
    for s in sets {
        let bpms: Vec<f64> = s.tracks.iter().filter_map(|&i| recs[i].bpm()).collect();
        let q = (bpms.len() / 4).max(1);
        let (opening_bpm, closing_bpm) =
            if bpms.len() >= 6 { (median(&bpms[..q]), median(&bpms[bpms.len() - q..])) } else { (None, None) };
        let (keyed, harmonic) =
            s.transitions().fold((0, 0), |(k, h), (x, y)| match (recs[x].camelot, recs[y].camelot) {
                (Some(a), Some(c)) => (k + 1, h + usize::from(a.relation_with_boost(c).is_some())),
                _ => (k, h),
            });
        lengths.push(s.tracks.len() as f64);
        set_list.push(SetInfo {
            id: s.id.clone(),
            name: s.name.clone(),
            place: s.place.clone(),
            date: s.date,
            tracks: s.tracks.len(),
            opening_bpm,
            closing_bpm,
            harmonic_share: share(harmonic, keyed),
        });
    }
    let dated: Vec<NaiveDate> = sets.iter().filter_map(|s| s.date).collect();
    let set_section = SetsSection {
        count: sets.len(),
        plays: if basis == Basis::Collection { 0 } else { pool.len() },
        distinct_tracks: sets.iter().flat_map(|s| s.tracks.iter()).collect::<FxHashSet<_>>().len(),
        median_tracks: median(&lengths).map_or(0, |m| m.round() as usize),
        longest: sets.iter().map(|s| s.tracks.len()).max().unwrap_or(0),
        oldest: dated.iter().min().copied(),
        newest: dated.iter().max().copied(),
        not_archived: sets.iter().filter(|s| s.date.is_none()).count(),
        too_short,
        list: set_list,
    };

    // Tempo.
    let mut bpms: Vec<f64> = pool.iter().filter_map(|&(i, _)| recs[i].bpm()).collect();
    bpms.sort_by(f64::total_cmp);
    let tempo = tempo_section(&bpms);

    // Arc and energy curves.
    let mut arc = ArcSection { curve: vec![None; 10], ..Default::default() };
    let mut curve_sum = [(0.0f64, 0usize); 10];
    let mut energy_sum = [(0.0f64, 0usize); 10];
    let (mut builds, mut steady, mut winds) = (0usize, 0usize, 0usize);
    let (mut openings, mut closings) = (Vec::new(), Vec::new());
    for s in sets {
        let n = s.tracks.len();
        for (pos, &i) in s.tracks.iter().enumerate() {
            if let Some(m) = recs[i].mood {
                let e = &mut energy_sum[decile(pos, n)];
                e.0 += mood_score(m);
                e.1 += 1;
            }
        }
        let timed: Vec<(usize, f64)> =
            s.tracks.iter().enumerate().filter_map(|(pos, &i)| recs[i].bpm().map(|b| (pos, b))).collect();
        if timed.len() < 6 {
            continue;
        }
        let all: Vec<f64> = timed.iter().map(|t| t.1).collect();
        let set_median = median(&all).unwrap_or(0.0);
        let q = (all.len() / 4).max(1);
        let (open, close) = (median(&all[..q]).unwrap_or(0.0), median(&all[all.len() - q..]).unwrap_or(0.0));
        openings.push(open);
        closings.push(close);
        match close - open {
            d if d >= 2.0 => builds += 1,
            d if d <= -2.0 => winds += 1,
            _ => steady += 1,
        }
        let mut by_decile: [Vec<f64>; 10] = Default::default();
        for (pos, bpm) in &timed {
            by_decile[decile(*pos, n)].push(*bpm);
        }
        for (d, v) in by_decile.iter().enumerate() {
            if let Some(m) = median(v) {
                curve_sum[d].0 += m - set_median;
                curve_sum[d].1 += 1;
            }
        }
        arc.sets += 1;
    }
    if arc.sets > 0 {
        arc.opening_bpm = Some(openings.iter().sum::<f64>() / openings.len() as f64);
        arc.closing_bpm = Some(closings.iter().sum::<f64>() / closings.len() as f64);
        arc.builds = builds as f64 / arc.sets as f64;
        arc.steady = steady as f64 / arc.sets as f64;
        arc.winds_down = winds as f64 / arc.sets as f64;
        arc.curve = curve_sum.iter().map(|&(s, n)| (n > 0).then(|| s / n as f64)).collect();
    }

    // Mixing.
    let mut mixing = MixingSection::default();
    let mut steps = Vec::new();
    let mut genre_changes = 0;
    for s in sets {
        for (x, y) in s.transitions() {
            let (a, c) = (&recs[x], &recs[y]);
            mixing.transitions += 1;
            if let (Some(ka), Some(kc)) = (a.camelot, c.camelot) {
                mixing.keyed += 1;
                match ka.relation_with_boost(kc) {
                    Some(Relation::Same) => mixing.same_key += 1,
                    Some(Relation::Adjacent) => mixing.adjacent += 1,
                    Some(Relation::Relative) => mixing.relative += 1,
                    Some(Relation::EnergyBoost) => mixing.energy_boost += 1,
                    None => mixing.clash += 1,
                }
            }
            if let (Some(ba), Some(bc)) = (a.bpm(), c.bpm()) {
                mixing.timed += 1;
                let gap = tempo_gap(ba, bc);
                steps.push(gap * 100.0);
                if (bc / ba - 1.0).abs() > 0.08 && gap <= 0.06 {
                    mixing.half_double += 1;
                }
            }
            if let (Some(ga), Some(gc)) = (a.genres.first(), c.genres.first()) {
                mixing.genred += 1;
                genre_changes += usize::from(normalize(ga) != normalize(gc));
            }
        }
    }
    let harmonic = mixing.same_key + mixing.adjacent + mixing.relative + mixing.energy_boost;
    mixing.harmonic_share = share(harmonic, mixing.keyed);
    mixing.median_step_pct = median(&steps);
    mixing.within_3pct = share(steps.iter().filter(|s| **s <= 3.0).count(), steps.len());
    mixing.within_6pct = share(steps.iter().filter(|s| **s <= 6.0).count(), steps.len());
    mixing.genre_change_share = share(genre_changes, mixing.genred);

    // Keys.
    let mut wheel = [0usize; 24];
    let mut minor = 0;
    let mut keyed = 0;
    for &(i, _) in &pool {
        if let Some(k) = recs[i].camelot {
            wheel[k.order() as usize] += 1;
            minor += usize::from(!k.major);
            keyed += 1;
        }
    }
    let wheel: Vec<KeyCount> = (0..24u8)
        .map(|o| KeyCount { key: Camelot { number: o / 2 + 1, major: o % 2 == 1 }.code(), count: wheel[o as usize] })
        .collect();
    let mut top_keys: Vec<Share> = wheel
        .iter()
        .filter(|k| k.count > 0)
        .map(|k| Share { name: k.key.clone(), count: k.count, share: share(k.count, keyed).unwrap_or(0.0) })
        .collect();
    top_keys.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));
    top_keys.truncate(o.top);
    let keys = KeysSection { tracks: keyed, wheel, minor_share: share(minor, keyed), top: top_keys };

    // Energy.
    let moods: Vec<Mood> = pool.iter().filter_map(|&(i, _)| recs[i].mood).collect();
    let energy = (!moods.is_empty()).then(|| {
        let n = moods.len();
        let count = |m: Mood| moods.iter().filter(|x| **x == m).count() as f64 / n as f64;
        EnergySection {
            tracks: n,
            high: count(Mood::High),
            mid: count(Mood::Mid),
            low: count(Mood::Low),
            curve: energy_sum.iter().map(|&(s, n)| (n > 0).then(|| s / n as f64)).collect(),
        }
    });

    // Genres, labels, artists.
    let (mut g, mut l, mut a) = (FxHashMap::default(), FxHashMap::default(), FxHashMap::default());
    let (mut g_known, mut l_known, mut a_known) = (0, 0, 0);
    for &(i, _) in &pool {
        let r = &recs[i];
        if !r.genres.is_empty() {
            g_known += 1;
            for name in &r.genres {
                tally(&mut g, name);
            }
        }
        if !r.track.label.trim().is_empty() {
            l_known += 1;
            tally(&mut l, &r.track.label);
        }
        let credits = artist_credits(&r.track.artist);
        if !credits.is_empty() {
            a_known += 1;
            for name in &credits {
                tally(&mut a, name);
            }
        }
    }
    let genres = ranked(g, g_known, o.top);
    let labels = ranked(l, l_known, o.top);
    let artists = ranked(a, a_known, o.top);

    // Era.
    let years: Vec<(i32, i32)> = pool.iter().filter_map(|&(i, played)| recs[i].year.map(|y| (y, played))).collect();
    let mut era = EraSection { tracks: years.len(), ..Default::default() };
    if !years.is_empty() {
        let ys: Vec<f64> = years.iter().map(|y| y.0 as f64).collect();
        era.median_year = median(&ys).map(|m| m.round() as i32);
        era.oldest = years.iter().map(|y| y.0).min();
        era.newest = years.iter().map(|y| y.0).max();
        era.recent_share = share(years.iter().filter(|(y, played)| *y >= played - 1).count(), years.len());
        let mut decades: IndexMap<i32, usize> = IndexMap::new();
        for (y, _) in &years {
            *decades.entry(y / 10 * 10).or_default() += 1;
        }
        decades.sort_unstable_keys();
        era.decades = decades.into_iter().map(|(decade, count)| DecadeCount { decade, count }).collect();
    }

    // Signature.
    let signature = signature_section(recs, sets, o.top);

    // Prep (the collection).
    let cued: Vec<CueCounts> = recs.iter().filter(|r| r.in_collection).filter_map(|r| r.cues).collect();
    let prep = (!cued.is_empty()).then(|| {
        let n = cued.len();
        let with_hot: Vec<&CueCounts> = cued.iter().filter(|c| c.hot > 0).collect();
        PrepSection {
            tracks: n,
            hot_cue_share: with_hot.len() as f64 / n as f64,
            hot_cues_per_track: (!with_hot.is_empty())
                .then(|| with_hot.iter().map(|c| c.hot).sum::<usize>() as f64 / with_hot.len() as f64),
            memory_cue_share: cued.iter().filter(|c| c.memory > 0).count() as f64 / n as f64,
            loop_share: cued.iter().filter(|c| c.loops > 0).count() as f64 / n as f64,
        }
    });

    // Collection.
    let in_collection: Vec<&Rec> = recs.iter().filter(|r| r.in_collection).collect();
    let n = in_collection.len();
    let played: FxHashSet<usize> = sets.iter().flat_map(|s| s.tracks.iter().copied()).collect();
    let collection = CollectionSection {
        tracks: n,
        playlists: b.playlists.iter().filter(|(folder, _)| !folder).count(),
        folders: b.playlists.iter().filter(|(folder, _)| *folder).count(),
        rated_share: share(in_collection.iter().filter(|r| r.track.rating > 0).count(), n),
        coloured_share: share(in_collection.iter().filter(|r| r.track.color.is_some()).count(), n),
        played_share: (!sets.is_empty())
            .then(|| share(played.iter().filter(|&&i| recs[i].in_collection).count(), n))
            .flatten(),
        duplicate_files: b.duplicate_files,
    };

    ProfileReport {
        generated: now,
        options: o.clone(),
        basis,
        sources: b.sources.clone(),
        summary: Summary { headline: String::new(), tags: Vec::new() },
        sets: set_section,
        tempo,
        arc,
        mixing,
        keys,
        energy,
        genres,
        labels,
        artists,
        era,
        signature,
        prep,
        collection,
        style: None,
        warnings: b.warnings.clone(),
    }
}

fn mood_score(m: Mood) -> f64 {
    match m {
        Mood::High => 1.0,
        Mood::Mid => 0.5,
        Mood::Low => 0.0,
    }
}

fn tempo_section(sorted: &[f64]) -> TempoSection {
    let mut t = TempoSection { tracks: sorted.len(), bin_width: 2.0, ..Default::default() };
    if sorted.is_empty() {
        return t;
    }
    t.median = percentile(sorted, 0.5);
    t.core = percentile(sorted, 0.1).zip(percentile(sorted, 0.9)).map(|(low, high)| BpmRange { low, high });
    t.p25 = percentile(sorted, 0.25);
    t.p75 = percentile(sorted, 0.75);
    t.min = sorted.first().copied();
    t.max = sorted.last().copied();
    let lo = percentile(sorted, 0.02).unwrap_or(0.0);
    let hi = percentile(sorted, 0.98).unwrap_or(0.0);
    t.bin_width = match hi - lo {
        s if s <= 40.0 => 2.0,
        s if s <= 80.0 => 4.0,
        _ => 10.0,
    };
    let w = t.bin_width;
    let first = (lo / w).floor() * w;
    let bins = (((hi - first) / w).floor() as usize + 1).max(1);
    let mut counts = vec![0usize; bins];
    for b in sorted {
        let i = ((b - first) / w).floor().clamp(0.0, (bins - 1) as f64) as usize;
        counts[i] += 1;
    }
    t.histogram =
        counts.into_iter().enumerate().map(|(i, count)| TempoBin { bpm: first + i as f64 * w, count }).collect();
    t
}

fn signature_section(recs: &[Rec], sets: &[SetData], top: usize) -> SignatureSection {
    #[derive(Default)]
    struct Counts {
        sets: usize,
        plays: usize,
        opened: usize,
        closed: usize,
    }
    let mut per: FxHashMap<usize, Counts> = FxHashMap::default();
    let mut moves: FxHashMap<(usize, usize), usize> = FxHashMap::default();
    for s in sets {
        for &i in s.tracks.iter().collect::<FxHashSet<_>>() {
            per.entry(i).or_default().sets += 1;
        }
        for &i in &s.tracks {
            per.entry(i).or_default().plays += 1;
        }
        if let (Some(&first), Some(&last)) = (s.tracks.first(), s.tracks.last()) {
            per.entry(first).or_default().opened += 1;
            per.entry(last).or_default().closed += 1;
        }
        for m in s.transitions().collect::<FxHashSet<_>>() {
            *moves.entry(m).or_default() += 1;
        }
    }
    let played_track = |i: usize, c: &Counts| PlayedTrack {
        name: recs[i].track.display_name(),
        artist: recs[i].track.artist.clone(),
        title: recs[i].track.title.clone(),
        sets: c.sets,
        plays: c.plays,
        opened: c.opened,
        closed: c.closed,
        unnamed: recs[i].unnamed,
    };
    let mut played: Vec<PlayedTrack> = per.iter().map(|(&i, c)| played_track(i, c)).collect();
    played.sort_by(|a, b| b.sets.cmp(&a.sets).then(b.plays.cmp(&a.plays)).then(a.name.cmp(&b.name)));
    let pick = |f: &dyn Fn(&PlayedTrack) -> usize| {
        let mut v: Vec<PlayedTrack> = played.iter().filter(|p| f(p) >= 2).cloned().collect();
        v.sort_by(|a, b| f(b).cmp(&f(a)).then(a.name.cmp(&b.name)));
        v.truncate(top);
        v
    };
    let tracks = pick(&|p| p.sets);
    let openers = pick(&|p| p.opened);
    let closers = pick(&|p| p.closed);
    let mut transitions: Vec<SignatureTransition> = moves
        .into_iter()
        .filter(|(_, n)| *n >= 2)
        .map(|((a, b), n)| SignatureTransition {
            from: recs[a].track.display_name(),
            to: recs[b].track.display_name(),
            sets: n,
        })
        .collect();
    transitions.sort_by(|a, b| b.sets.cmp(&a.sets).then(a.from.cmp(&b.from)).then(a.to.cmp(&b.to)));
    transitions.truncate(top);
    SignatureSection { tracks, transitions, openers, closers, played }
}

/// My Settings that describe a playing style: the store's settings profile when it sets any of
/// them, otherwise the first drive with settings files.
fn style(sources: &Sources<'_>) -> Option<StyleSection> {
    let describe = |key: &str, value: &str| -> Option<StyleValue> {
        let def = settings::find(key)?;
        let choice = def.choice(value)?;
        Some(StyleValue {
            key: key.to_string(),
            label: def.label.to_string(),
            value: choice.id.to_string(),
            value_label: choice.label.to_string(),
        })
    };
    if let Some(profile) = sources.store.and_then(|s| s.settings_profile().ok()) {
        let values: Vec<StyleValue> =
            STYLE_SETTINGS.iter().filter_map(|k| profile.base.get(*k).and_then(|v| describe(k, v))).collect();
        if !values.is_empty() {
            return Some(StyleSection { source: "settings profile".into(), values });
        }
    }
    for d in &sources.drives {
        let snap = settings::read_all(&DriveSettings(d.root()));
        let values: Vec<StyleValue> = STYLE_SETTINGS
            .iter()
            .filter_map(|k| match snap.values.get(*k) {
                Some(SettingValue::Known(v)) => describe(k, v),
                _ => None,
            })
            .collect();
        if !values.is_empty() {
            return Some(StyleSection { source: d.name(), values });
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------------------------

fn pct(x: f64) -> String {
    format!("{:.0}%", x * 100.0)
}

fn and_list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [a] => a.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// "122–128 BPM" (or "128 BPM").
pub fn bpm_range(lo: f64, hi: f64) -> String {
    let (lo, hi) = (lo.round(), hi.round());
    if lo == hi { format!("{lo:.0} BPM") } else { format!("{lo:.0}–{hi:.0} BPM") }
}

/// The generated headline, from the sections `shows` lets through: the main genres, the tempo
/// range ([`bpm_range`]), and how many sets or sessions (or, without either, tracks in the
/// collection), each count written by `count`.
pub fn headline(r: &ProfileReport, shows: impl Fn(Section) -> bool, count: impl Fn(usize) -> String) -> String {
    let mut parts = Vec::new();
    let genres: Vec<&str> = r.genres.top.iter().filter(|s| s.share >= 0.1).take(2).map(|s| s.name.as_str()).collect();
    if !genres.is_empty() && shows(Section::Genres) {
        parts.push(and_list(&genres));
    }
    if let Some(core) = r.tempo.core
        && shows(Section::Tempo)
    {
        parts.push(bpm_range(core.low, core.high));
    }
    let n = r.sets.count;
    match r.basis {
        Basis::Collection if shows(Section::Collection) => parts.push(format!("{} tracks", count(r.collection.tracks))),
        Basis::Plays | Basis::Sessions if shows(Section::Sets) => {
            parts.push(format!("{} {}{}", count(n), r.basis.unit(), if n == 1 { "" } else { "s" }))
        }
        _ => {}
    }
    parts.join(" · ")
}

fn summarise(r: &ProfileReport) -> Summary {
    let mut tags = Vec::new();
    let mut tag = |id: &'static str, label: String, why: String| tags.push(Tag { id, label, why });
    let m = &r.mixing;
    if let Some(h) = m.harmonic_share
        && m.keyed >= 20
        && h >= 0.6
    {
        tag("harmonic", "Mixes in key".into(), format!("{} of {} transitions are harmonic", pct(h), m.keyed));
    }
    if let Some(w) = m.within_3pct
        && m.timed >= 20
        && w >= 0.8
    {
        tag(
            "smooth_tempo",
            "Smooth tempo changes".into(),
            format!("{} of transitions change tempo by 3% or less", pct(w)),
        );
    }
    if let Some(g) = m.genre_change_share
        && m.genred >= 20
        && g >= 0.5
    {
        tag("genre_hopping", "Genre-hopping".into(), format!("{} of transitions change genre", pct(g)));
    }
    if let Some(top) = r.genres.top.first()
        && r.genres.known >= 20
        && top.share >= 0.6
    {
        tag("focused", format!("Mostly {}", top.name), format!("{} of plays are tagged {}", pct(top.share), top.name));
    }
    let unit = r.basis.unit();
    if r.arc.sets >= 3 && r.arc.builds >= 0.5 {
        tag(
            "builds",
            format!("Builds the tempo through a {unit}"),
            format!("{} of {} {unit}s end at least 2 BPM faster than they start", pct(r.arc.builds), r.arc.sets),
        );
    }
    if let Some(recent) = r.era.recent_share
        && r.era.tracks >= 20
        && recent >= 0.5
    {
        tag(
            "fresh",
            "Plays new releases".into(),
            format!("{} of plays came out that year or the year before", pct(recent)),
        );
    }
    if let Some(y) = r.era.median_year
        && r.era.tracks >= 20
        && y <= r.generated.year() - 10
    {
        tag("digger", "Plays older records".into(), format!("half of the plays were released in {y} or earlier"));
    }
    if r.sets.count >= 5 && r.sets.plays > 0 {
        let fresh = r.sets.distinct_tracks as f64 / r.sets.plays as f64;
        if fresh >= 0.85 {
            tag(
                "rarely_repeats",
                "Rarely repeats tracks".into(),
                format!("{} of plays are of different tracks", pct(fresh)),
            );
        }
    }
    if let Some(t) = r.signature.tracks.first()
        && r.sets.count >= 4
        && t.sets as f64 / r.sets.count as f64 >= 0.5
    {
        tag(
            "signature",
            "Has signature tracks".into(),
            format!("{} is in {} of {} {unit}s", t.name, t.sets, r.sets.count),
        );
    }
    if let Some(p) = &r.prep
        && p.tracks >= 20
        && p.hot_cue_share >= 0.8
    {
        tag(
            "prepared",
            "Cues up every track".into(),
            format!("{} of the collection has hot cues", pct(p.hot_cue_share)),
        );
    }
    if let Some(s) = &r.style {
        let get = |k: &str| s.values.iter().find(|v| v.key == k);
        if get("quantize").is_some_and(|v| v.value == "off") {
            tag("quantize_off", "Plays with quantize off".into(), format!("My Settings ({})", s.source));
        }
        if let Some(v) = get("tempo_range").filter(|v| v.value == "16" || v.value == "wide") {
            tag("wide_tempo", "Wide pitch range".into(), format!("tempo range {} ({})", v.value_label, s.source));
        }
    }
    Summary { headline: headline(r, |_| true, |n| n.to_string()), tags }
}

// ---------------------------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------------------------

/// Words that mean "no genre".
const NOT_GENRES: [&str; 7] = ["", "-", "unknown", "other", "none", "genre", "n/a"];

/// Spellings that mean the same genre.
const GENRE_ALIASES: [(&str, &str); 22] = [
    ("dnb", "Drum & Bass"),
    ("d&b", "Drum & Bass"),
    ("d'n'b", "Drum & Bass"),
    ("drum n bass", "Drum & Bass"),
    ("drum'n'bass", "Drum & Bass"),
    ("drum and bass", "Drum & Bass"),
    ("drum & bass", "Drum & Bass"),
    ("hiphop", "Hip-Hop"),
    ("hip hop", "Hip-Hop"),
    ("hip-hop", "Hip-Hop"),
    ("rnb", "R&B"),
    ("r&b", "R&B"),
    ("r'n'b", "R&B"),
    ("edm", "EDM"),
    ("ukg", "UK Garage"),
    ("uk garage", "UK Garage"),
    ("techhouse", "Tech House"),
    ("tech-house", "Tech House"),
    ("deephouse", "Deep House"),
    ("house music", "House"),
    ("dubstep", "Dubstep"),
    ("electronica", "Electronica"),
];

/// "deep house" → "Deep House"; short all-capital words ("UK", "EDM") stay as they are.
fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|word| {
            word.split('-')
                .map(|w| {
                    if w.len() <= 4 && w.chars().any(char::is_alphabetic) && !w.chars().any(char::is_lowercase) {
                        return w.to_string();
                    }
                    let mut c = w.chars();
                    let Some(f) = c.next() else { return String::new() };
                    // A letter whose capital is two ("ß" → "SS") stays, or the result would read
                    // back differently ("SSa" → "Ssa").
                    let first: String = match f.to_uppercase() {
                        up if up.len() == 1 => up.collect(),
                        _ => f.to_string(),
                    };
                    first + &c.flat_map(char::to_lowercase).collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("-")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A track's genre text as genres: split at `/ , ; |` (not `&`: "Drum & Bass"), spelled one way.
pub fn genres(raw: &str, aliases: &IndexMap<String, String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in raw.split(['/', ',', ';', '|']) {
        let part = part.split_whitespace().collect::<Vec<_>>().join(" ");
        let key = normalize(&part);
        if NOT_GENRES.contains(&key.as_str()) {
            continue;
        }
        let name = aliases
            .iter()
            .find(|(k, _)| normalize(k) == key)
            .map(|(_, v)| v.clone())
            .or_else(|| GENRE_ALIASES.iter().find(|(k, _)| *k == key).map(|(_, v)| v.to_string()))
            .unwrap_or_else(|| title_case(&part));
        if !out.iter().any(|g| normalize(g) == normalize(&name)) {
            out.push(name);
        }
    }
    out
}

/// `s` split wherever one of the (ASCII) separators appears, ignoring case.
fn split_ci<'s>(s: &'s str, seps: &[&str]) -> Vec<&'s str> {
    let mut out = Vec::new();
    let (mut start, mut i) = (0, 0);
    while i < s.len() {
        match seps.iter().find(|sep| s.get(i..i + sep.len()).is_some_and(|w| w.eq_ignore_ascii_case(sep))) {
            Some(sep) => {
                out.push(&s[start..i]);
                i += sep.len();
                start = i;
            }
            None => {
                i += 1;
                while !s.is_char_boundary(i) {
                    i += 1;
                }
            }
        }
    }
    out.push(&s[start..]);
    out
}

/// The artists a credit names, for counting: split at commas, semicolons and featuring credits
/// ("A, B feat. C" is A, B and C). `&` and `x` don't split: "Chase & Status" is one act.
pub fn artist_credits(artist: &str) -> Vec<String> {
    let seps = [",", ";", " feat. ", " feat ", " ft. ", " ft ", " featuring ", "(feat. ", "(ft. "];
    let mut out: Vec<String> = Vec::new();
    for part in split_ci(artist, &seps) {
        let name = part.trim_matches(|c: char| c.is_whitespace() || c == '(' || c == ')');
        if !name.is_empty() && !out.iter().any(|x| normalize(x) == normalize(name)) {
            out.push(name.to_string());
        }
    }
    out
}

/// Every artist credited: split at commas, `&`, `x`, `vs`, `and` and featuring credits. Used to
/// hide an artist wherever they're credited.
pub fn credited_artists(artist: &str) -> Vec<String> {
    let mut s = format!(" {} ", artist.to_lowercase());
    for sep in
        [",", "&", " x ", " vs. ", " vs ", " and ", " feat. ", " feat ", " ft. ", " ft ", " featuring ", "(", ")"]
    {
        s = s.replace(sep, "\u{1f}");
    }
    s.split('\u{1f}').map(normalize).filter(|a| !a.is_empty()).collect()
}

#[cfg(test)]
mod tests;
