//! Sets: the performances a DJ played, as they say they played them.
//!
//! A history list is a log of one media connection (see [`crate::plays`]). Players start one
//! every time a USB or SD goes in and log any track that plays for about a minute, with no
//! on-air check and no clock, so practice, prep and track checks land in the same lists as gigs,
//! and one gig can spread over several lists, both databases and several drives. locallibrary
//! calls those raw logs *sessions* ([`Session`]) and never counts or publishes one as a set.
//!
//! A [`Set`] is what the DJ declares: the day, the place, who they played with, and a tracklist
//! built from sessions and/or a recording of the set (see [`crate::mix`]), then edited. Sets live
//! in the store's `sets.json` ([`SetBook`]), with the sessions the DJ said weren't a set and the
//! logs imported from other tools (Beat Link Trigger), which carry real play times.

pub mod build;
pub mod edit;
pub mod export;
pub mod import;
pub mod query;
pub mod session;
pub mod suggest;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, NaiveDate, Utc};
use lofty::prelude::*;
use lofty::tag::ItemKey;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::fields::FieldValue;
use crate::fields::roles::{Role, RoleValues};
use crate::local_time;
use crate::model::{Track, artist_credits, isrc};
use crate::profile::ProfilePrefs;
use crate::store::Store;
use crate::{Error, Result, fsutil};

/// The store file holding the DJ's sets.
pub const SETS_FILE: &str = "sets.json";

/// A performance: what the DJ played, where and when.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Set {
    /// A TID ([`new_id`]): the record key when it's published, and how commands name it (any
    /// unique prefix of 4 or more characters).
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The day it was played (local).
    pub date: NaiveDate,
    /// `club`, `festival`, `radio`, `livestream`, `mix` (the Lexicon's `knownValues`) or the DJ's
    /// own word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub venue: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    /// ISO 3166 country code, `GB`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    /// The event it was part of: the `at://` URI of a `community.lexicon.calendar.event` record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    /// Who they played with (back to back).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub with: Vec<Partner>,
    /// Where people can listen to it (Mixcloud, SoundCloud, YouTube...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// The recording on this computer the tracklist was found in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording: Option<Recording>,
    /// The DJ's own notes. Never published.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// The sessions it was built from, by id ([`Session::id`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<String>,
    /// In play order.
    pub tracks: Vec<SetEntry>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Set {
    /// The id as listings show it.
    pub fn short_id(&self) -> &str {
        self.id.get(..8).unwrap_or(&self.id)
    }

    /// What it's called: its title, else its venue.
    pub fn name(&self) -> Option<&str> {
        self.title.as_deref().or(self.venue.as_deref())
    }

    /// [`Set::name`], else the day it was played.
    pub fn heading(&self) -> String {
        self.name().map_or_else(|| self.date.to_string(), String::from)
    }

    /// Its venue and city, as far as they're known: `Warehouse, Leeds`.
    pub fn place(&self) -> String {
        [&self.venue, &self.city].into_iter().flatten().cloned().collect::<Vec<_>>().join(", ")
    }

    /// Newest first: by the day it was played, then the one declared last.
    pub fn newest_first(a: &Set, b: &Set) -> std::cmp::Ordering {
        b.date.cmp(&a.date).then(b.created_at.cmp(&a.created_at))
    }

    /// Which tracks go out without their names wherever the set is published: those it marks
    /// unnamed, those at the positions (from 1) in `hide`, and those `prefs` hides (see
    /// [`SetEntry::may_name`]). A position the set doesn't have is refused.
    pub fn unnamed(&self, hide: &[usize], prefs: &ProfilePrefs) -> Result<Unnamed> {
        if let Some(&bad) = hide.iter().find(|&&p| p == 0 || p > self.tracks.len()) {
            return Err(Error::invalid(format!("there's no track {bad} to hide: the set has {}", self.tracks.len())));
        }
        let (mut marked, mut asked, mut profile) = (Vec::new(), Vec::new(), Vec::new());
        for (n, e) in (1..).zip(&self.tracks) {
            if e.unnamed {
                marked.push(n);
            } else if hide.contains(&n) {
                asked.push(n);
            } else if !e.may_name(prefs) {
                profile.push(n);
            }
        }
        let notes = [
            unnamed_note(&marked, "marked unnamed (`sets edit --named`)"),
            unnamed_note(&asked, "hidden as asked"),
            unnamed_note(&profile, "hidden by your profile (`profile unhide`)"),
        ]
        .into_iter()
        .flatten()
        .collect();
        Ok(Unnamed { positions: marked.into_iter().chain(asked).chain(profile).collect(), notes })
    }
}

/// A set's tracks published without their names ([`Set::unnamed`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unnamed {
    /// Their positions, from 1.
    pub positions: BTreeSet<usize>,
    /// Why, for each reason one is: `tracks 3, 7 aren't named: hidden as asked`.
    pub notes: Vec<String>,
}

/// `track 3 isn't named: why`, or `tracks 3, 7 aren't named: why`.
fn unnamed_note(positions: &[usize], why: &str) -> Option<String> {
    let list: Vec<String> = positions.iter().map(usize::to_string).collect();
    match list.as_slice() {
        [] => None,
        [one] => Some(format!("track {one} isn't named: {why}")),
        many => Some(format!("tracks {} aren't named: {why}", many.join(", "))),
    }
}

/// Someone the DJ played with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Partner {
    pub name: String,
    /// Their Atmosphere account, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did: Option<String>,
}

/// A recording of a set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "SetRecording")]
pub struct Recording {
    pub path: PathBuf,
    /// The mix report kept in the store for it (`mix.reports.get`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
}

/// Where a set's track came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EntryOrigin {
    /// A session logged it.
    Session,
    /// It was heard in the recording.
    Recording,
    /// The DJ added it.
    Added,
}

/// One track of a set, kept as it was when it went in, so the set outlives library changes and
/// reads without the drive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetEntry {
    pub title: String,
    pub artist: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub album: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub genre: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub isrc: String,
    /// As the library has it (note name, Camelot or Open Key).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    /// 0 when unknown.
    #[serde(default, serialize_with = "known_bpm")]
    pub bpm: f64,
    #[serde(default)]
    pub year: u16,
    #[serde(default)]
    pub duration_secs: u32,
    /// The audio file's path in the library (`/Contents/...`), to find the track again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Seconds from the start of the set (from a recording or a timed log).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_seconds: Option<u32>,
    /// MusicBrainz IDs from the audio file's tags, else from what the analyzers chosen for their
    /// roles worked out for it, read from a mounted drive when the set is made, edited or published.
    #[serde(default, skip_serializing_if = "MusicBrainzIds::is_empty")]
    pub musicbrainz: MusicBrainzIds,
    /// Published without naming it: only its place in the set.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unnamed: bool,
    pub origin: EntryOrigin,
    /// The session that logged it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

/// MusicBrainz IDs, as MusicBrainz Picard tags them: `MUSICBRAINZ_TRACKID` (ID3's `UFID`) is the
/// recording, `MUSICBRAINZ_ALBUMID` the release, `MUSICBRAINZ_ARTISTID` each credited artist. An
/// analyzer answers them as the roles `recording`, `release` and `artists`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicBrainzIds {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<Uuid>,
    /// In the credit's order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artists: Vec<Uuid>,
}

impl MusicBrainzIds {
    /// The roles of the analyzers' fields these are read from: the IDs, and what they were
    /// worked out from.
    pub const ROLES: [Role; 5] = [Role::Recording, Role::Release, Role::Artists, Role::Isrc, Role::Credits];

    pub fn is_empty(&self) -> bool {
        *self == MusicBrainzIds::default()
    }

    /// These, with what they lack from `other`: the recording and the release each, the artists
    /// all together (they pair with the credit's names only as a whole).
    pub fn or(self, other: MusicBrainzIds) -> MusicBrainzIds {
        MusicBrainzIds {
            recording: self.recording.or(other.recording),
            release: self.release.or(other.release),
            artists: if self.artists.is_empty() { other.artists } else { self.artists },
        }
    }

    /// Whether `values` (an analyzer's for [`Self::ROLES`]) were worked out for `e` as the set has
    /// it, as far as the analyzer says (its `isrc` and `credits`): for `e`'s ISRC, and then for
    /// its artist's credits too. An analyzer's values are kept by audio file, and a library can
    /// retag a track without changing the file.
    fn worked_out_for(values: &RoleValues, e: &SetEntry) -> (bool, bool) {
        let isrc = match values.get(&Role::Isrc) {
            None => true,
            Some(FieldValue::Null) => isrc(&e.isrc).is_none(),
            Some(FieldValue::String(s)) => isrc(s).is_some_and(|s| Some(s) == isrc(&e.isrc)),
            Some(_) => false,
        };
        let credits = match values.get(&Role::Credits) {
            None => true,
            Some(FieldValue::Strings(names)) => *names == artist_credits(&e.artist),
            Some(_) => false,
        };
        (isrc, isrc && credits)
    }

    /// Whether `values` (an analyzer's for [`Self::ROLES`]) say something [`Self::from_roles`]
    /// leaves out as being about another ISRC or credit than `e`'s: worth working out again.
    pub fn stale(values: &RoleValues, e: &SetEntry) -> bool {
        !values.is_empty() && Self::worked_out_for(values, e) != (true, true)
    }

    /// The IDs `values` (an analyzer's for [`Self::ROLES`]) give `e`, once they're for its ISRC:
    /// the recording and the release each when it's a UUID, and the artists only when every one
    /// is, they're for its credit, and there's one for each name [`artist_credits`] splits its
    /// artist into, since they're paired with those names in that order. Anything else is left
    /// out.
    pub fn from_roles(values: &RoleValues, e: &SetEntry) -> MusicBrainzIds {
        let (ids, credited) = Self::worked_out_for(values, e);
        if !ids {
            return MusicBrainzIds::default();
        }
        let id = |role| match values.get(&role) {
            Some(FieldValue::String(s)) => Uuid::parse_str(s.trim()).ok(),
            _ => None,
        };
        let artists: Option<Vec<Uuid>> = match values.get(&Role::Artists) {
            Some(FieldValue::Strings(ids)) if credited => ids.iter().map(|s| Uuid::parse_str(s.trim()).ok()).collect(),
            _ => None,
        };
        let artists = artists.filter(|ids| ids.len() == artist_credits(&e.artist).len()).unwrap_or_default();
        MusicBrainzIds { recording: id(Role::Recording), release: id(Role::Release), artists }
    }

    /// The IDs the tags of the audio file at `path` hold: none when it has none, or can't be read.
    /// Anything that isn't a UUID is left out.
    pub fn read(path: &Path) -> MusicBrainzIds {
        let Ok(tagged) = lofty::read_from_path(path) else { return MusicBrainzIds::default() };
        let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else { return MusicBrainzIds::default() };
        // ID3v2.4 keeps several values in one frame (lofty splits those); Picard writes ID3v2.3's
        // joined with `/`.
        let ids = |key: ItemKey| -> Vec<Uuid> {
            tag.get_strings(key).flat_map(|v| v.split('/')).filter_map(|v| Uuid::parse_str(v.trim()).ok()).collect()
        };
        MusicBrainzIds {
            recording: ids(ItemKey::MusicBrainzRecordingId).into_iter().next(),
            release: ids(ItemKey::MusicBrainzReleaseId).into_iter().next(),
            artists: ids(ItemKey::MusicBrainzArtistId),
        }
    }
}

/// A tempo as JSON holds it: an infinite or NaN one is no tempo, and serde_json would write it as
/// `null`, which doesn't read back as a number (the whole of `sets.json` with it), so it's unknown.
fn known_bpm<S: serde::Serializer>(bpm: &f64, s: S) -> std::result::Result<S::Ok, S::Error> {
    s.serialize_f64(if bpm.is_finite() { *bpm } else { 0.0 })
}

impl SetEntry {
    /// `t` as a set entry.
    pub fn from_track(t: &Track, origin: EntryOrigin, session: Option<&str>) -> SetEntry {
        SetEntry {
            title: t.title.trim().to_string(),
            artist: t.artist.trim().to_string(),
            album: t.album.trim().to_string(),
            label: t.label.trim().to_string(),
            genre: t.genre.trim().to_string(),
            isrc: t.isrc.trim().to_string(),
            key: t.key.trim().to_string(),
            bpm: t.bpm,
            year: t.year,
            duration_secs: t.duration_secs,
            file: Some(t.file_path.clone()).filter(|f| !f.is_empty()),
            start_seconds: None,
            musicbrainz: MusicBrainzIds::default(),
            unnamed: false,
            origin,
            session: session.map(String::from),
        }
    }

    /// A track the DJ names by hand: `Artist - Title`, or just a title.
    pub fn added(text: &str) -> SetEntry {
        let (artist, title) = match text.split_once(" - ") {
            Some((a, t)) => (a.trim(), t.trim()),
            None => ("", text.trim()),
        };
        SetEntry {
            title: title.to_string(),
            artist: artist.to_string(),
            album: String::new(),
            label: String::new(),
            genre: String::new(),
            isrc: String::new(),
            key: String::new(),
            bpm: 0.0,
            year: 0,
            duration_secs: 0,
            file: None,
            start_seconds: None,
            musicbrainz: MusicBrainzIds::default(),
            unnamed: false,
            origin: EntryOrigin::Added,
            session: None,
        }
    }

    /// `Artist - Title`, or the title alone.
    pub fn name(&self) -> String {
        match (self.artist.is_empty(), self.title.is_empty()) {
            (false, false) => format!("{} - {}", self.artist, self.title),
            (true, _) => self.title.clone(),
            (false, true) => self.artist.clone(),
        }
    }

    /// Whether what's shared with others may name it: not when the set marks it unnamed, or
    /// `prefs`, the DJ's profile, hides the track or its artist.
    pub fn may_name(&self, prefs: &ProfilePrefs) -> bool {
        !self.unnamed && prefs.may_name(&self.name(), &self.artist)
    }

    /// The file as [`crate::paths::file_key`] keys it, to match a library's tracks by.
    pub fn file_key(&self) -> Option<String> {
        self.file.as_deref().map(crate::paths::file_key)
    }
}

/// A session: one run of play logging. A history list a player recorded, or a log imported from
/// another tool ([`ImportedSession`]).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Session {
    /// The list's fingerprint ([`crate::plays::HistoryList::fingerprint`]): the same wherever
    /// it's read. Commands take any unique prefix of 4 or more characters.
    pub id: String,
    pub name: String,
    /// Folders and name.
    pub path: String,
    /// `device_library` (`export.pdb`), `onelibrary`, or the tool a log was imported from
    /// (`beat_link_trigger`, or a plugin's importer, `<plugin>.<importer>`).
    pub source: String,
    /// The mounted drives holding it now.
    pub drives: Vec<String>,
    /// The drive whose snapshot first held it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_seen_on: Option<String>,
    pub window: Window,
    /// In play order; `start_seconds` is set for timed logs.
    pub tracks: Vec<SetEntry>,
    /// Entries whose track isn't in the library any more.
    pub missing: usize,
    /// Sets built from it, by id.
    pub sets: Vec<String>,
    /// The DJ said it wasn't a set.
    pub dismissed: bool,
}

impl Session {
    /// The id as listings show it.
    pub fn short_id(&self) -> &str {
        &self.id[..self.id.len().min(8)]
    }
}

/// When a session was recorded, as far as anyone can tell. Players have no clock, so for a
/// history list it's a window: after locallibrary last saw the drive without the list, before it
/// first saw it with it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Window {
    /// Recorded after this, when known.
    pub after: Option<DateTime<Utc>>,
    /// Recorded before this, when known.
    pub before: Option<DateTime<Utc>>,
    /// The times come from the log itself (an imported log): when the first track started and
    /// the last one stopped.
    pub exact: bool,
}

impl Window {
    /// The local day it falls on, when `after` and `before` are on the same one. A timed log is
    /// on the day it started: a night that runs past midnight is still that night.
    pub fn day(&self) -> Option<NaiveDate> {
        let local = |t: DateTime<Utc>| t.with_timezone(&Local).date_naive();
        match (self.after, self.before) {
            (Some(a), _) if self.exact => Some(local(a)),
            (Some(a), Some(b)) if local(a) == local(b) => Some(local(a)),
            _ => None,
        }
    }

    /// `on 3 Oct 2026`, `between 29 Sep and 3 Oct 2026`, `before 3 Oct 2026`, `after ...`, or
    /// `unknown`.
    pub fn label(&self) -> String {
        let day = |t: DateTime<Utc>| local_time(t, "%-d %b %Y").to_string();
        if let Some(d) = self.day() {
            return format!("on {}", d.format("%-d %b %Y"));
        }
        match (self.after, self.before) {
            (Some(a), Some(b)) => format!("between {} and {}", day(a), day(b)),
            (None, Some(b)) => format!("before {}", day(b)),
            (Some(a), None) => format!("after {}", day(a)),
            (None, None) => "unknown".into(),
        }
    }
}

/// A play log imported from another tool, with real play times.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ImportedSession {
    /// Like a history list's fingerprint: the same log imported twice is one session.
    pub id: String,
    pub name: String,
    /// `beat_link_trigger`, or the plugin's importer it was read with (`<plugin>.<importer>`).
    pub tool: String,
    /// The file it was imported from.
    pub file: PathBuf,
    pub imported_at: DateTime<Utc>,
    pub plays: Vec<TimedPlay>,
}

/// One play in an imported log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TimedPlay {
    pub entry: SetEntry,
    pub started: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<DateTime<Utc>>,
    /// The player it was played on (`3`), when the log says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player: Option<String>,
}

/// The store's `sets.json`: the DJ's sets and what they said about sessions.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SetBook {
    /// Newest first.
    #[serde(default)]
    pub sets: Vec<Set>,
    /// Sessions the DJ said weren't a set, by id: `sets suggest` leaves them out.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub dismissed: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imported: Vec<ImportedSession>,
}

impl SetBook {
    pub fn load(store: &Store) -> Result<SetBook> {
        Ok(fsutil::read_json_opt(&store.root().join(SETS_FILE))?.unwrap_or_default())
    }

    /// Write the book, its sets newest first.
    pub fn save(&mut self, store: &Store) -> Result<()> {
        self.sets.sort_by(Set::newest_first);
        fsutil::write_json_atomic(&store.root().join(SETS_FILE), self)
    }

    /// A set by `query`: `last` (the newest), an id prefix of 4 or more characters, a title
    /// (ignoring case), or a day (`2026-10-03`) when only one set was played on it.
    pub fn find(&self, query: &str) -> Result<&Set> {
        let i = self.position(query)?;
        Ok(&self.sets[i])
    }

    pub(crate) fn find_mut(&mut self, query: &str) -> Result<&mut Set> {
        let i = self.position(query)?;
        Ok(&mut self.sets[i])
    }

    fn position(&self, query: &str) -> Result<usize> {
        let q = query.trim();
        if q.is_empty() {
            return Err(Error::invalid("name a set: its id, title or day, or `last`"));
        }
        if q.eq_ignore_ascii_case("last") {
            return newest(self.sets.iter().enumerate())
                .ok_or_else(|| Error::NotFound("a set (there are none yet)".into()));
        }
        let matches = |pred: &dyn Fn(&Set) -> bool| -> Vec<usize> {
            self.sets.iter().enumerate().filter(|(_, s)| pred(s)).map(|(i, _)| i).collect()
        };
        let candidates = [
            matches(&|s| s.id == q),
            if q.len() >= 4 { matches(&|s| s.id.starts_with(q)) } else { Vec::new() },
            matches(&|s| s.title.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(q))),
            match NaiveDate::parse_from_str(q, "%Y-%m-%d") {
                Ok(d) => matches(&|s| s.date == d),
                Err(_) => Vec::new(),
            },
        ];
        for found in candidates {
            match found.as_slice() {
                [] => continue,
                [i] => return Ok(*i),
                many => {
                    let names: Vec<String> = many.iter().map(|&i| describe(&self.sets[i])).collect();
                    return Err(Error::invalid(format!("{q:?} could be any of: {}", names.join("; "))));
                }
            }
        }
        Err(Error::NotFound(format!("set {q:?} (`locallibrary sets` lists them)")))
    }

    /// Keeps `log` (checked as it was read) as a session: the same log imported again replaces
    /// its session.
    pub(crate) fn keep(&mut self, log: ImportedSession) {
        self.imported.retain(|s| s.id != log.id);
        self.imported.push(log);
    }

    /// The sets built from session `id`.
    pub(crate) fn sets_with_session<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a Set> + 'a {
        self.sets.iter().filter(move |s| s.sessions.iter().any(|x| x == id))
    }
}

fn newest<'a>(sets: impl Iterator<Item = (usize, &'a Set)>) -> Option<usize> {
    sets.min_by(|(_, a), (_, b)| Set::newest_first(a, b)).map(|(i, _)| i)
}

/// `2026-10-03 Friday at the Warehouse (3l2k...)`.
pub fn describe(s: &Set) -> String {
    format!("{} {} ({})", s.date, s.name().unwrap_or("untitled"), s.short_id())
}

/// A new set id: a TID whose time is a random moment on `date`, so published sets sort by the
/// day they were played.
pub fn new_id(date: NaiveDate) -> Result<String> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|e| Error::invalid(format!("no randomness available: {e}")))?;
    let n = u64::from_le_bytes(bytes);
    let midnight = date.and_hms_opt(0, 0, 0).map_or(0, |t| t.and_utc().timestamp_micros().max(0) as u64);
    Ok(tid(midnight + n % 86_400_000_000, (n >> 54) as u32))
}

/// An AT Protocol TID: 53 bits of microseconds since 1970 and a 10-bit clock id, in
/// base32-sortable.
fn tid(micros: u64, clock: u32) -> String {
    const ALPHABET: &[u8; 32] = b"234567abcdefghijklmnopqrstuvwxyz";
    let v = ((micros & ((1 << 53) - 1)) << 10) | u64::from(clock & 0x3ff);
    (0..13).rev().map(|i| ALPHABET[((v >> (i * 5)) & 31) as usize] as char).collect()
}

#[cfg(test)]
mod tests;
