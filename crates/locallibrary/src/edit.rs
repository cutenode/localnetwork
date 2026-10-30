//! Editing a drive's playlists and tracks in place: create, rename, move and delete playlists
//! and folders, change what's in them and in what order, change track details, delete tracks,
//! point tracks at moved audio files, replace duplicates and clear player histories.
//!
//! An edit is planned against the drive's main library (`export.pdb`, else OneLibrary) and
//! resolved into operations that name playlists by their path and tracks by their audio file,
//! because row ids differ between the two databases. Applying runs the operations on each
//! database the drive has, keeping a model of what each database should then hold; the edited
//! databases are re-read and must match their models exactly before either is written. A
//! playlist or track one database lacks (they can drift apart) is skipped there with a warning.
//!
//! Every applied edit is recorded on the drive (see [`journal`]): rekordbox's next export to the
//! drive replaces its databases, and `replay` puts the recorded edits back. A plan can mix in My
//! Tag, cue and artwork edits ([`crate::tags`], [`crate::cue_edit`], [`crate::artwork`]), which it
//! makes first, under the same backup and in the same journal entry (see [`plan`]).

use indexmap::IndexMap;

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use rekordbox_pdb::{Database, PdbEditor, TableType};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::artwork::ImageSource;
use crate::backup;
use crate::commit::{Changes, NewDatabases, NewFile, Options};
use crate::cue_edit::CueChange;
use crate::device::{Device, LineageOp};
use crate::error::{Action, Reason};
use crate::fsutil;
use crate::harmony;
use crate::hash::Hash;
use crate::model::{Library, LibraryDiff, MAX_DEPTH, Playlist, PlaylistKey, Track, TrackColor, TrackRef, is_isrc};
use crate::onelibrary::{OneLibrary, Tree};
use crate::paths;
use crate::pdb;
use crate::progress::{Ctx, Phase};
use crate::query::{self, DriveContext, TrackSelector};
use crate::tags::TagChange;
use crate::{Error, Result};

// ---------------------------------------------------------------------------------------------
// Edits
// ---------------------------------------------------------------------------------------------

/// One change to a drive's library. Playlists are given as paths (`Folder/Name`) or unique
/// names; positions in a playlist count from 1.
// Edits are few (a plan's, the journal's): boxing `EditTracks`' fields would only make every
// caller box them.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Edit {
    /// Create a playlist (or folder) at `path`, creating missing folders above it. Nothing
    /// happens if it exists already.
    CreatePlaylist {
        path: String,
        #[serde(default)]
        folder: bool,
    },
    RenamePlaylist {
        playlist: String,
        name: String,
    },
    /// Move a playlist or folder into folder `into` (the top level when absent or empty), at
    /// `position` among its new siblings (the end when absent).
    MovePlaylist {
        playlist: String,
        #[serde(default)]
        into: Option<String>,
        #[serde(default)]
        position: Option<usize>,
    },
    /// Delete a playlist, or a folder with everything in it. Tracks stay in the library unless
    /// `with_tracks`: then tracks no playlist outside it (and no history list) uses go too, with
    /// their files, as when removing a guest's crate after a back-to-back set.
    DeletePlaylist {
        playlist: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        with_tracks: bool,
    },
    /// Add tracks at `position` (the end when absent). Tracks already in the playlist are left
    /// out unless `allow_repeats`.
    AddTracks {
        playlist: String,
        tracks: TrackSelector,
        #[serde(default)]
        position: Option<usize>,
        /// Order the added tracks (see [`query::parse_sort`]).
        #[serde(default)]
        sort: Option<String>,
        #[serde(default)]
        allow_repeats: bool,
    },
    /// Remove every entry of the selected tracks and/or the entries at `positions`.
    RemoveTracks {
        playlist: String,
        #[serde(default)]
        tracks: Option<TrackSelector>,
        #[serde(default)]
        positions: Vec<usize>,
    },
    /// Move the entries at `positions` (kept in their order) so the first lands at `to`.
    MoveTracks {
        playlist: String,
        positions: Vec<usize>,
        to: usize,
    },
    /// Make a playlist hold exactly the selected tracks (a query or filter may select none),
    /// optionally sorted and limited. With `create`, the playlist at exactly that path, made if
    /// missing. Rule-based playlists use this.
    SetTracks {
        playlist: String,
        tracks: TrackSelector,
        #[serde(default)]
        sort: Option<String>,
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        create: bool,
    },
    /// Make a playlist's entries exactly these audio files (device paths) in this order, repeats
    /// and all; none empties it. With `create`, the playlist at exactly that path, made (with
    /// missing folders above it) if missing. Imported tracklists are made this way. The journal
    /// records these edits this way: filling a playlist (`SetTracks`), ordering it for mixing
    /// (`PlanSet`), and removing or moving entries whose files have other entries in it too.
    SetEntries {
        playlist: String,
        paths: Vec<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        create: bool,
    },
    /// Reorder a playlist (see [`query::parse_sort`]).
    SortPlaylist {
        playlist: String,
        by: String,
    },
    /// Drop repeated entries (the first stays); with `recordings`, also other copies of the same
    /// recording (artist, title and duration).
    Dedupe {
        playlist: String,
        #[serde(default)]
        recordings: bool,
    },
    /// Order a playlist for mixing (see [`harmony::order_set`], with how sure each key is from
    /// the keys an analyzer heard already, see [`crate::sound::key_confidences`]): in place, or
    /// into a new playlist at `into`.
    PlanSet {
        playlist: String,
        #[serde(default)]
        start: Option<TrackSelector>,
        #[serde(default)]
        into: Option<String>,
    },
    EditTracks {
        tracks: TrackSelector,
        fields: TrackFields,
    },
    /// Delete tracks with their playlist and history entries, and (unless `keep_files`) their
    /// audio, analysis and artwork files when no other track uses them. A track goes with every
    /// row of its audio file, in each database; a path may name one only the drive's other
    /// database lists (OneLibrary, drifted from `export.pdb`), which is deleted there.
    DeleteTracks {
        tracks: TrackSelector,
        #[serde(default)]
        keep_files: bool,
    },
    /// Replace one copy of a recording with another: playlist and history entries (and My Tags)
    /// move to `keep`, then `duplicate` is deleted unless `keep_duplicate`.
    ReplaceTrack {
        duplicate: TrackSelector,
        keep: TrackSelector,
        #[serde(default)]
        keep_duplicate: bool,
    },
    /// Point one track at another audio file on the drive (a device path), as when its file was
    /// moved or renamed: in both databases and in the analysis files that record it. With `file`,
    /// the file is another encoding of the recording (a lossless copy of an MP3): the rows take
    /// its format, size, sample rate, bit depth and bit rate, and with `shift_ms` its cues and
    /// beat grid move by that much, for audio that starts at another point.
    SetPath {
        track: TrackSelector,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<AudioFile>,
        #[serde(default, skip_serializing_if = "is_zero")]
        shift_ms: i32,
    },
    /// Delete history lists (what players recorded) by name, or all of them.
    DeleteHistory {
        #[serde(default)]
        names: Vec<String>,
        #[serde(default)]
        all: bool,
    },
    /// Delete history lists, each the one with exactly its name and entries. The journal records
    /// deleting history lists this way, so a replay leaves lists players recorded since, under a
    /// reused name too.
    DeleteHistoryLists {
        lists: Vec<HistoryList>,
    },
    /// A My Tag edit (see [`crate::tags`]), its tracks named by file. `tags` edits are recorded
    /// this way too.
    MyTag(crate::tags::TagChange),
    /// A cue edit (see [`crate::cue_edit`]) on the tracks of these audio files (device paths).
    /// `cues` edits are recorded this way too.
    Cues {
        tracks: Vec<String>,
        change: crate::cue_edit::CueChange,
    },
    /// Give tracks this artwork, scaled to the sizes players show (see [`crate::artwork`]). The
    /// journal records the tracks by file and the image as staged on the drive.
    SetArtwork {
        tracks: TrackSelector,
        image: ImageSource,
    },
    /// Write a beat grid, phrases or a waveform into the analysis files of the track of this
    /// audio file (a device path, see [`crate::analysis_edit`]). Planned, a grid brings the
    /// track's BPM with it (an `edit_tracks`); the journal records a waveform as staged on the
    /// drive.
    SetAnalysis {
        track: String,
        #[serde(flatten)]
        analysis: crate::analysis_edit::NewAnalysis,
    },
}

/// A history list as the journal names it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "HistoryListName")]
pub struct HistoryList {
    pub name: String,
    /// Its entries' audio files (device paths), in order.
    pub tracks: Vec<String>,
}

/// New values for track details; fields left out stay as they are, and an empty name clears
/// artist, album, genre, label, key and the like (an empty ISRC or release date clears it too).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrackFields {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    /// The databases keep it with the album, so a track needs one (given here too, or its own).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remixer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mix_name: Option<String>,
    /// 0-5 stars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<u8>,
    /// A colour name (`aqua`), rekordbox's number (1-8), or `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub play_count: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disc_number: Option<u16>,
    /// 1-999.99, kept to hundredths as rekordbox keeps it. The beat grid in the analysis files,
    /// which players count beats by, stays as it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    /// Like `GBDUW0000059`: two letters, three letters or digits, and seven digits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isrc: Option<String>,
    /// `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
}

impl TrackFields {
    /// The colour to set: `None` leaves it, `Some(None)` clears it.
    pub fn color_value(&self) -> Result<Option<Option<TrackColor>>> {
        self.color.as_deref().map(query::parse_color).transpose()
    }

    pub fn is_empty(&self) -> bool {
        *self == TrackFields::default()
    }

    fn validate(&self) -> Result<()> {
        if self.is_empty() {
            return Err(Error::invalid("give at least one field to change"));
        }
        if self.rating.is_some_and(|r| r > 5) {
            return Err(Error::invalid("ratings go from 0 to 5"));
        }
        self.color_value()?;
        if let Some(k) = &self.key
            && !k.trim().is_empty()
            && harmony::Camelot::parse(k).is_none()
        {
            return Err(Error::invalid(format!("{k:?} isn't a key (8A, Am, 1m...)")));
        }
        if self.bpm.is_some_and(|b| !(1.0..=999.99).contains(&b)) {
            return Err(Error::invalid("BPM goes from 1 to 999.99"));
        }
        if let Some(i) = self.isrc.as_deref().filter(|i| !i.is_empty() && !is_isrc(i)) {
            return Err(Error::invalid(format!("{i:?} isn't an ISRC (like GBDUW0000059)")));
        }
        // Zero-padded, as rekordbox writes them (chrono takes `2019-5-1` and `2019-05-1` too):
        // the Traktor export reads the date from the first ten characters.
        let date = |d: &str| {
            let digits = d.len() == 10
                && d.bytes().enumerate().all(|(i, c)| if i == 4 || i == 7 { c == b'-' } else { c.is_ascii_digit() });
            digits && chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok()
        };
        if let Some(d) = self.release_date.as_deref().filter(|d| !d.is_empty() && !date(d)) {
            return Err(Error::invalid(format!("{d:?} isn't a release date (YYYY-MM-DD)")));
        }
        Ok(())
    }

    /// The details as they will be after the edit.
    fn apply_to(&self, t: &mut Track) {
        let set = |v: &Option<String>, f: &mut String| {
            if let Some(v) = v {
                *f = v.clone();
            }
        };
        // The databases store no name for these when one is given blank; names the edit leaves
        // alone are kept as they are.
        let name = |v: &Option<String>, f: &mut String| {
            if let Some(v) = v {
                *f = if v.trim().is_empty() { String::new() } else { v.clone() };
            }
        };
        set(&self.title, &mut t.title);
        name(&self.artist, &mut t.artist);
        name(&self.genre, &mut t.genre);
        name(&self.label, &mut t.label);
        name(&self.key, &mut t.key);
        name(&self.remixer, &mut t.remixer);
        name(&self.original_artist, &mut t.original_artist);
        name(&self.composer, &mut t.composer);
        set(&self.comment, &mut t.comment);
        set(&self.mix_name, &mut t.mix_name);
        name(&self.album_artist, &mut t.album_artist);
        set(&self.isrc, &mut t.isrc);
        set(&self.release_date, &mut t.release_date);
        name(&self.album, &mut t.album);
        // As a cleared album's row goes, its artist goes with it.
        if self.album.is_some() && t.album.is_empty() {
            t.album_artist.clear();
        }
        if let Some(r) = self.rating {
            t.rating = r;
        }
        if let Ok(Some(c)) = self.color_value() {
            t.color = c;
        }
        if let Some(y) = self.year {
            t.year = y;
        }
        if let Some(p) = self.play_count {
            t.play_count = p;
        }
        if let Some(n) = self.track_number {
            t.track_number = n;
        }
        if let Some(n) = self.disc_number {
            t.disc_number = n;
        }
        if let Some(b) = self.bpm {
            t.bpm = (b * 100.0).round() / 100.0;
        }
    }

    /// `rating 5, colour aqua, genre "Deep House"`.
    fn describe(&self) -> String {
        let mut parts = Vec::new();
        for (name, v) in [
            ("title", &self.title),
            ("artist", &self.artist),
            ("album", &self.album),
            ("genre", &self.genre),
            ("label", &self.label),
            ("key", &self.key),
            ("remixer", &self.remixer),
            ("original artist", &self.original_artist),
            ("composer", &self.composer),
            ("comment", &self.comment),
            ("mix", &self.mix_name),
            ("album artist", &self.album_artist),
            ("colour", &self.color),
            ("ISRC", &self.isrc),
            ("release date", &self.release_date),
        ] {
            if let Some(v) = v {
                parts.push(format!("{name} {v:?}"));
            }
        }
        for (name, v) in [
            ("rating", self.rating.map(u32::from)),
            ("year", self.year.map(u32::from)),
            ("play count", self.play_count.map(u32::from)),
            ("track number", self.track_number),
            ("disc", self.disc_number.map(u32::from)),
        ] {
            if let Some(v) = v {
                parts.push(format!("{name} {v}"));
            }
        }
        if let Some(b) = self.bpm {
            parts.push(format!("BPM {}", (b * 100.0).round() / 100.0));
        }
        parts.join(", ")
    }
}

// ---------------------------------------------------------------------------------------------
// Operations: edits resolved to playlist paths and track files
// ---------------------------------------------------------------------------------------------

/// A playlist or folder by the names from the top level down (empty for the top level itself).
type NodePath = Vec<String>;

#[derive(Debug, Clone, PartialEq)]
enum Op {
    Create {
        parent: NodePath,
        name: String,
        folder: bool,
    },
    /// `folder`: whether `node` is a folder, as a playlist and a folder can share a path.
    Rename {
        node: NodePath,
        folder: bool,
        name: String,
    },
    Place {
        node: NodePath,
        folder: bool,
        parent: NodePath,
        position: Option<usize>,
    },
    Delete {
        node: NodePath,
        folder: bool,
    },
    /// `before`: the entries the edit was planned on, for an edit that changes them rather than
    /// saying what they are. A database whose entries differ (a CDJ-3000X writes its playlist
    /// edits to OneLibrary alone) gets the edit made in its own (see [`rebase_entries`]).
    SetEntries {
        node: NodePath,
        tracks: Vec<String>,
        before: Option<Vec<String>>,
    },
    SetFields {
        tracks: Vec<String>,
        fields: Box<TrackFields>,
    },
    /// Every row of each of `tracks`' files, and of `elsewhere`'s: files only the drive's other
    /// database lists, which a database lacking them leaves be.
    DeleteTracks {
        tracks: Vec<String>,
        elsewhere: Vec<String>,
    },
    Repoint {
        from: String,
        to: String,
    },
    /// The track of audio file `track` (a key, lower-cased) points at `path` (a device path), a
    /// file `file` describes when it's another encoding, whose times are `shift_ms` later.
    SetPath {
        track: String,
        path: String,
        file: Option<AudioFile>,
        shift_ms: i32,
    },
    /// History lists by name and which of the lists with that name (0 = the first).
    DeleteHistory {
        lists: Vec<(String, usize)>,
    },
}

/// The first node at `path` in display order, top down, that is a folder (`Some(true)`), a
/// playlist (`Some(false)`) or either: the one [`Library::find_playlist_of_kind`] finds when
/// planning, also when same-named siblings share the path, or a playlist and a folder do.
fn find_node(lib: &Library, path: &[String], folder: Option<bool>) -> Option<u32> {
    fn below(lib: &Library, parent: Option<u32>, path: &[String], folder: Option<bool>) -> Option<u32> {
        let Some((name, rest)) = path.split_first() else { return parent };
        // Only folders lead further down.
        let kind = if rest.is_empty() { folder } else { Some(true) };
        lib.children(parent)
            .into_iter()
            .filter(|p| p.name == *name && kind.is_none_or(|f| p.is_folder == f))
            .find_map(|p| below(lib, Some(p.id), rest, folder))
    }
    below(lib, None, path, folder)
}

/// A playlist's entries (track keys) in a database whose own, `current`, differ from the ones an
/// edit was planned on: `before`, which the edit turned into `after`. The edit's removals and
/// additions are made in `current`; an added entry goes after the entry it follows in `after`, or
/// first when it follows none. Entries only `current` has stay where they are, and the ones it
/// shares with `after` keep its order unless the edit reordered them, which orders them as `after`
/// does.
fn rebase_entries<'a>(current: &'a [String], before: &[String], after: &'a [String]) -> Vec<&'a str> {
    let counts = |keys: &[String]| -> FxHashMap<String, usize> {
        let mut out = FxHashMap::default();
        for k in keys {
            *out.entry(k.clone()).or_default() += 1;
        }
        out
    };
    let (planned, edited) = (counts(before), counts(after));
    // The edit's removals: the occurrences `before` has beyond `after`'s.
    let mut removed: FxHashMap<&str, usize> =
        planned.iter().map(|(k, n)| (k.as_str(), n.saturating_sub(edited.get(k).copied().unwrap_or(0)))).collect();
    let kept: Vec<&str> = current
        .iter()
        .map(String::as_str)
        .filter(|k| match removed.get_mut(k) {
            Some(n) if *n > 0 => {
                *n -= 1;
                false
            }
            _ => true,
        })
        .collect();
    // Each entry of `after` was in `before` (its places, in order, by key) or the edit adds it.
    let (mut seen, mut places, mut added) =
        (FxHashMap::default(), FxHashMap::<&str, Vec<usize>>::default(), Vec::new());
    for (i, k) in after.iter().enumerate() {
        let n: &mut usize = seen.entry(k.as_str()).or_default();
        *n += 1;
        match *n > planned.get(k).copied().unwrap_or(0) {
            true => added.push(i),
            false => places.entry(k.as_str()).or_default().push(i),
        }
    }
    // `current`'s entries, each with its place in `after` (`None`: only `current` has it).
    let mut taken: FxHashMap<&str, usize> = FxHashMap::default();
    let mut out: Vec<(Option<usize>, &str)> = kept
        .into_iter()
        .map(|k| {
            let n = taken.entry(k).or_default();
            let place = places.get(k).and_then(|p| p.get(*n)).copied();
            *n += 1;
            (place, k)
        })
        .collect();
    fn in_other<'k>(keys: &'k [String], other: &FxHashMap<String, usize>) -> Vec<&'k str> {
        keys.iter().filter(|k| other.contains_key(*k)).map(String::as_str).collect()
    }
    if in_other(before, &edited) != in_other(after, &planned) {
        let mut shared: Vec<(Option<usize>, &str)> = out.iter().filter(|(p, _)| p.is_some()).copied().collect();
        shared.sort();
        let mut shared = shared.into_iter();
        for slot in out.iter_mut().filter(|(p, _)| p.is_some()) {
            *slot = shared.next().expect("as many as the slots");
        }
    }
    for i in added {
        let follows =
            out.iter().enumerate().filter(|(_, (p, _))| p.is_some_and(|p| p < i)).max_by_key(|(_, (p, _))| *p);
        let at = follows.map_or(0, |(at, _)| at + 1);
        out.insert(at, (Some(i), after[i].as_str()));
    }
    out.into_iter().map(|(_, k)| k).collect()
}

fn kind(folder: bool) -> &'static str {
    if folder { "folder" } else { "playlist" }
}

fn shown(path: &[String]) -> String {
    if path.is_empty() { "the top level".into() } else { path.join("/") }
}

/// What applying operations needs from a database; the shared logic in [`apply_op`] keeps the
/// database's [`Library`] model in step.
trait EditDb {
    fn lib(&mut self) -> &mut Library;
    fn create(&mut self, name: &str, parent: u32, folder: bool) -> Result<u32>;
    fn rename(&mut self, id: u32, name: &str) -> Result<()>;
    fn place(&mut self, id: u32, parent: u32, sort_order: u32) -> Result<()>;
    fn delete(&mut self, ids: &[u32]) -> Result<()>;
    fn set_entries(&mut self, id: u32, tracks: &[u32]) -> Result<()>;
    fn set_fields(&mut self, ids: &[u32], fields: &TrackFields) -> Result<()>;
    fn delete_tracks(&mut self, ids: &[u32]) -> Result<()>;
    fn repoint(&mut self, from: u32, to: u32) -> Result<()>;
    /// Point a track at `path`; with `file`, a file of another encoding, whose cues are
    /// `shift_ms` later.
    fn set_path(&mut self, id: u32, path: &str, file: Option<&AudioFile>, shift_ms: i32) -> Result<()>;
    fn delete_history(&mut self, ids: &[u32]) -> Result<()>;
    /// A file's size as this database keeps it: OneLibrary whole, export.pdb modulo 2^32.
    fn stored_size(&self, size: u64) -> u64 {
        size
    }
}

/// A library on its own: the expected result of an edit.
struct ModelDb<'a> {
    lib: Library,
    /// export.pdb's database, when `lib` is its library: names and details too long for one of
    /// its rows are refused here, as export.pdb's editor would only refuse them when applying.
    pdb: Option<&'a Database>,
}

/// `what` (a name) is too long for export.pdb.
fn too_long(what: &str, name: &str) -> Error {
    Error::invalid(format!("{what} is too long for export.pdb ({} characters)", name.chars().count()))
}

impl EditDb for ModelDb<'_> {
    fn lib(&mut self) -> &mut Library {
        &mut self.lib
    }
    fn create(&mut self, name: &str, parent: u32, folder: bool) -> Result<u32> {
        if let Some(db) = self.pdb
            && db.check_playlist_name(name).is_err()
        {
            let what = if folder { "folder" } else { "playlist" };
            let parent = shown(&self.lib.playlist_names(parent));
            return Err(too_long(&format!("the name of the new {what} in {parent}"), name));
        }
        Ok(self.lib.playlists.iter().map(|p| p.id).max().unwrap_or(0) + 1)
    }
    fn rename(&mut self, id: u32, name: &str) -> Result<()> {
        if let Some(db) = self.pdb
            && db.check_playlist_name(name).is_err()
        {
            return Err(too_long(&format!("the new name for {}", self.lib.playlist_path(id)), name));
        }
        Ok(())
    }
    fn place(&mut self, _: u32, _: u32, _: u32) -> Result<()> {
        Ok(())
    }
    fn delete(&mut self, _: &[u32]) -> Result<()> {
        Ok(())
    }
    fn set_entries(&mut self, _: u32, _: &[u32]) -> Result<()> {
        Ok(())
    }
    fn set_fields(&mut self, ids: &[u32], f: &TrackFields) -> Result<()> {
        let Some(db) = self.pdb else { return Ok(()) };
        // The names PdbDb::set_fields looks up, adding a row for one it doesn't find.
        for (field, name, table) in [
            ("artist", &f.artist, TableType::Artists),
            ("remixer", &f.remixer, TableType::Artists),
            ("original artist", &f.original_artist, TableType::Artists),
            ("composer", &f.composer, TableType::Artists),
            ("album artist", &f.album_artist, TableType::Artists),
            ("album", &f.album, TableType::Albums),
            ("genre", &f.genre, TableType::Genres),
            ("label", &f.label, TableType::Labels),
            ("key", &f.key, TableType::Keys),
        ] {
            if let Some(name) = name.as_deref().filter(|n| !n.trim().is_empty())
                && db.check_lookup_name(table, name).is_err()
            {
                return Err(too_long(&format!("the {field} name"), name));
            }
        }
        // Titles, comments, mix names, ISRCs and release dates share a row with the track's other
        // strings.
        let set: Vec<&str> = [
            ("title", &f.title),
            ("comment", &f.comment),
            ("mix name", &f.mix_name),
            ("ISRC", &f.isrc),
            ("release date", &f.release_date),
        ]
        .into_iter()
        .filter_map(|(field, v)| v.as_ref().map(|_| field))
        .collect();
        let Some((last, rest)) = set.split_last() else { return Ok(()) };
        let rows: FxHashMap<u32, &rekordbox_pdb::Track> = db.tracks.iter().map(|t| (t.id, t)).collect();
        for t in self.lib.tracks.iter().filter(|t| ids.contains(&t.id)) {
            let mut after = t.clone();
            f.apply_to(&mut after);
            let strings = [
                ("title", after.title.as_str()),
                ("comment", after.comment.as_str()),
                ("mix_name", after.mix_name.as_str()),
                ("isrc", after.isrc.as_str()),
                ("release_date", after.release_date.as_str()),
            ];
            if let Some(row) = rows.get(&t.id)
                && db.check_track_strings(row, &strings).is_err()
            {
                let (fields, verb) = match rest {
                    [] => (last.to_string(), "is"),
                    _ => (format!("{} and {last}", rest.join(", ")), "are"),
                };
                return Err(Error::invalid(format!(
                    "the {fields} of {} {verb} too long for export.pdb",
                    t.display_name()
                )));
            }
        }
        Ok(())
    }
    fn delete_tracks(&mut self, _: &[u32]) -> Result<()> {
        Ok(())
    }
    fn repoint(&mut self, _: u32, _: u32) -> Result<()> {
        Ok(())
    }
    fn set_path(&mut self, id: u32, path: &str, _: Option<&AudioFile>, _: i32) -> Result<()> {
        let row = self.pdb.and_then(|db| db.tracks.iter().find(|t| t.id == id).map(|row| (db, row)));
        if let Some((db, row)) = row
            && db.check_track_strings(row, &[("file_path", path), ("filename", file_name(path))]).is_err()
        {
            return Err(too_long("the audio file's path", path));
        }
        Ok(())
    }
    fn stored_size(&self, size: u64) -> u64 {
        if self.pdb.is_some() { size % (1 << 32) } else { size }
    }
    fn delete_history(&mut self, _: &[u32]) -> Result<()> {
        Ok(())
    }
}

/// The last part of a device path.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

struct PdbDb {
    editor: PdbEditor,
    lib: Library,
    /// Tracks removed and replaced, for the My Tag links in `exportExt.pdb`.
    deleted: Vec<u32>,
    repointed: Vec<(u32, u32)>,
    /// Lookup rows (table, id) edited or deleted tracks used before: removed at the end if
    /// nothing uses them any more.
    candidates: Vec<(TableType, u32)>,
    original: Database,
}

impl PdbDb {
    fn note_lookups(&mut self, ids: &[u32], artwork: bool) {
        for t in self.original.tracks.iter().filter(|t| ids.contains(&t.id)) {
            for a in [t.artist_id, t.remixer_id, t.original_artist_id, t.composer_id] {
                self.candidates.push((TableType::Artists, a));
            }
            self.candidates.push((TableType::Albums, t.album_id));
            self.candidates.push((TableType::Genres, t.genre_id));
            self.candidates.push((TableType::Labels, t.label_id));
            self.candidates.push((TableType::Keys, t.key_id));
            if artwork {
                self.candidates.push((TableType::Artwork, t.artwork_id));
            }
        }
    }

    /// Remove candidate lookup rows no track (or album) references.
    fn prune(&mut self) -> Result<()> {
        let db = self.editor.database()?;
        let mut used: FxHashMap<u32, FxHashSet<u32>> = FxHashMap::default();
        let mut note = |t: TableType, id: u32| {
            used.entry(t as u32).or_default().insert(id);
        };
        for t in &db.tracks {
            for a in [t.artist_id, t.remixer_id, t.original_artist_id, t.composer_id] {
                note(TableType::Artists, a);
            }
            note(TableType::Albums, t.album_id);
            note(TableType::Genres, t.genre_id);
            note(TableType::Labels, t.label_id);
            note(TableType::Keys, t.key_id);
            note(TableType::Artwork, t.artwork_id);
        }
        let albums: FxHashMap<u32, u32> = db.albums.iter().map(|a| (a.id, a.artist_id)).collect();
        let unused = |t: TableType, id: u32, used: &FxHashMap<u32, FxHashSet<u32>>| {
            id != 0 && !used.get(&(t as u32)).is_some_and(|u| u.contains(&id))
        };
        let mut gone: Vec<(TableType, u32)> = Vec::new();
        let mut candidates = std::mem::take(&mut self.candidates);
        candidates.sort_by_key(|(t, id)| (*t as u32, *id));
        candidates.dedup();
        // Albums first: an album that goes no longer keeps its artist.
        for &(t, id) in candidates.iter().filter(|(t, _)| *t == TableType::Albums) {
            if unused(t, id, &used) && albums.contains_key(&id) {
                gone.push((t, id));
            }
        }
        for a in &db.albums {
            if !gone.contains(&(TableType::Albums, a.id)) {
                used.entry(TableType::Artists as u32).or_default().insert(a.artist_id);
            } else {
                candidates.push((TableType::Artists, a.artist_id));
            }
        }
        for &(t, id) in candidates.iter().filter(|(t, _)| *t != TableType::Albums) {
            if unused(t, id, &used) && !gone.contains(&(t, id)) {
                gone.push((t, id));
            }
        }
        for t in [
            TableType::Albums,
            TableType::Artists,
            TableType::Genres,
            TableType::Labels,
            TableType::Keys,
            TableType::Artwork,
        ] {
            let ids: Vec<u32> = gone.iter().filter(|(g, _)| *g == t).map(|(_, id)| *id).collect();
            if !ids.is_empty() {
                self.editor.remove_rows(t, &ids)?;
            }
        }
        Ok(())
    }
}

impl EditDb for PdbDb {
    fn lib(&mut self) -> &mut Library {
        &mut self.lib
    }
    fn create(&mut self, name: &str, parent: u32, folder: bool) -> Result<u32> {
        Ok(self.editor.create_playlist(name, parent, folder)?)
    }
    fn rename(&mut self, id: u32, name: &str) -> Result<()> {
        Ok(self.editor.rename_playlist(id, name)?)
    }
    fn place(&mut self, id: u32, parent: u32, sort_order: u32) -> Result<()> {
        Ok(self.editor.move_playlist(id, parent, sort_order)?)
    }
    fn delete(&mut self, ids: &[u32]) -> Result<()> {
        Ok(self.editor.remove_playlists(ids)?)
    }
    fn set_entries(&mut self, id: u32, tracks: &[u32]) -> Result<()> {
        Ok(self.editor.set_playlist_entries(id, tracks)?)
    }
    fn set_fields(&mut self, ids: &[u32], f: &TrackFields) -> Result<()> {
        self.note_lookups(ids, false);
        let color = f.color_value()?;
        for &id in ids {
            let (album, album_artist) =
                self.lib.track(id).map(|t| (t.album.clone(), t.album_artist.clone())).unwrap_or_default();
            let ed = &mut self.editor;
            let mut strings = Vec::new();
            for (name, v) in [
                ("title", &f.title),
                ("comment", &f.comment),
                ("mix_name", &f.mix_name),
                ("isrc", &f.isrc),
                ("release_date", &f.release_date),
            ] {
                if let Some(v) = v {
                    strings.push((name, v.as_str()));
                }
            }
            if !strings.is_empty() {
                ed.set_track_strings(id, &strings)?;
            }
            let artist = |ed: &mut PdbEditor, name: &str| -> Result<u32> {
                Ok(if name.trim().is_empty() { 0 } else { ed.artist_id_for(name)? })
            };
            for (v, field) in [
                (&f.artist, "artist_id"),
                (&f.remixer, "remixer_id"),
                (&f.original_artist, "original_artist_id"),
                (&f.composer, "composer_id"),
            ] {
                if let Some(name) = v {
                    let a = artist(ed, name)?;
                    ed.set_track_field(id, field, a)?;
                }
            }
            // An album row holds its artist: either changing takes the row of the pair. Only a name
            // given blank clears the album; the track's own is kept whatever it is.
            if f.album.is_some() || f.album_artist.is_some() {
                let name = match &f.album {
                    Some(name) => Some(name).filter(|n| !n.trim().is_empty()),
                    None => Some(&album).filter(|a| !a.is_empty()),
                };
                let album = match name {
                    None => 0,
                    Some(name) => {
                        let a = artist(ed, f.album_artist.as_ref().unwrap_or(&album_artist))?;
                        ed.album_id_for(name, a)?
                    }
                };
                ed.set_track_field(id, "album_id", album)?;
            }
            for (v, field) in [(&f.genre, "genre_id"), (&f.label, "label_id"), (&f.key, "key_id")] {
                if let Some(name) = v {
                    let lookup = if name.trim().is_empty() {
                        0
                    } else {
                        match field {
                            "genre_id" => ed.genre_id_for(name)?,
                            "label_id" => ed.label_id_for(name)?,
                            _ => ed.key_id_for(name)?,
                        }
                    };
                    ed.set_track_field(id, field, lookup)?;
                }
            }
            for (v, field) in [
                (f.rating.map(u32::from), "rating"),
                (color.map(|c| c.map_or(0, |c| u32::from(c.id()))), "color_id"),
                (f.year.map(u32::from), "year"),
                (f.play_count.map(u32::from), "play_count"),
                (f.track_number, "track_number"),
                (f.disc_number.map(u32::from), "disc_number"),
                (f.bpm.map(|b| (b * 100.0).round() as u32), "tempo"),
            ] {
                if let Some(v) = v {
                    ed.set_track_field(id, field, v)?;
                }
            }
        }
        Ok(())
    }
    fn delete_tracks(&mut self, ids: &[u32]) -> Result<()> {
        self.note_lookups(ids, true);
        self.deleted.extend(ids);
        Ok(self.editor.remove_tracks(ids)?)
    }
    fn repoint(&mut self, from: u32, to: u32) -> Result<()> {
        self.editor.repoint_track(from, to)?;
        self.repointed.push((from, to));
        Ok(())
    }
    fn set_path(&mut self, id: u32, path: &str, file: Option<&AudioFile>, _: i32) -> Result<()> {
        self.editor.set_track_strings(id, &[("file_path", path), ("filename", file_name(path))])?;
        self.editor.set_track_field(id, "file_type", rekordbox_pdb::file_type_code(path).into())?;
        if let Some(f) = file {
            self.editor.set_track_field(id, "file_size", self.stored_size(f.size) as u32)?;
            self.editor.set_track_field(id, "sample_rate", f.sample_rate)?;
            self.editor.set_track_field(id, "sample_depth", f.bit_depth.into())?;
            self.editor.set_track_field(id, "bitrate", f.bitrate)?;
        }
        Ok(())
    }
    // export.pdb keeps sizes modulo 2^32, as rekordbox writes them.
    fn stored_size(&self, size: u64) -> u64 {
        size % (1 << 32)
    }
    fn delete_history(&mut self, ids: &[u32]) -> Result<()> {
        Ok(self.editor.remove_history_playlists(ids)?)
    }
}

struct PlusDb {
    one: OneLibrary,
    lib: Library,
}

impl EditDb for PlusDb {
    fn lib(&mut self) -> &mut Library {
        &mut self.lib
    }
    fn create(&mut self, name: &str, parent: u32, folder: bool) -> Result<u32> {
        self.one.create_playlist(name, parent, folder)
    }
    fn rename(&mut self, id: u32, name: &str) -> Result<()> {
        self.one.rename_node(Tree::Playlist, id, name)
    }
    fn place(&mut self, id: u32, parent: u32, sort_order: u32) -> Result<()> {
        self.one.place_node(Tree::Playlist, id, parent, sort_order)
    }
    fn delete(&mut self, ids: &[u32]) -> Result<()> {
        self.one.remove_nodes(Tree::Playlist, ids)
    }
    fn set_entries(&mut self, id: u32, tracks: &[u32]) -> Result<()> {
        self.one.set_playlist_entries(id, tracks)
    }
    fn set_fields(&mut self, ids: &[u32], fields: &TrackFields) -> Result<()> {
        self.one.set_track_fields(ids, fields)
    }
    fn delete_tracks(&mut self, ids: &[u32]) -> Result<()> {
        self.one.remove_tracks(ids)
    }
    fn repoint(&mut self, from: u32, to: u32) -> Result<()> {
        self.one.repoint_track(from, to)?;
        Ok(())
    }
    fn set_path(&mut self, id: u32, path: &str, file: Option<&AudioFile>, shift_ms: i32) -> Result<()> {
        self.one.set_track_path(id, path, file, shift_ms)
    }
    fn delete_history(&mut self, ids: &[u32]) -> Result<()> {
        self.one.remove_nodes(Tree::History, ids)
    }
}

/// Refuse a playlist at `path` whose deepest node would be `depth` names down: deeper than
/// [`MAX_DEPTH`], a file's tree is read back as a loop and its rows moved to the top level
/// ([`crate::model::untangle`]).
fn too_deep(path: &[String], depth: usize) -> Result<()> {
    if depth <= MAX_DEPTH {
        return Ok(());
    }
    Err(Error::invalid(format!(
        "{} would put playlists {depth} levels deep; they nest {MAX_DEPTH} levels at most",
        shown(path)
    )))
}

/// Apply one operation to a database and its model. With `strict`, anything the operation names
/// that the database lacks is an error; otherwise (the drive's second database) it's skipped
/// with a warning.
fn apply_op(db: &mut dyn EditDb, op: &Op, strict: bool, label: &str, warnings: &mut Vec<String>) -> Result<()> {
    let mut skip = |msg: String| -> Result<()> {
        if strict {
            return Err(Error::NotFound(msg));
        }
        warnings.push(format!("{label}: {msg}; skipped there"));
        Ok(())
    };
    let ids_of = |lib: &Library, keys: &[String]| -> (Vec<u32>, Vec<String>) {
        let by_key: FxHashMap<String, u32> = lib.tracks.iter().map(|t| (paths::file_key(&t.file_path), t.id)).collect();
        let mut missing = Vec::new();
        let ids = keys
            .iter()
            .filter_map(|k| {
                let id = by_key.get(k).copied();
                if id.is_none() {
                    missing.push(k.clone());
                }
                id
            })
            .collect();
        (ids, missing)
    };
    let folder_id = |lib: &Library, path: &[String]| -> std::result::Result<Option<u32>, String> {
        if path.is_empty() {
            return Ok(None);
        }
        match (find_node(lib, path, Some(true)), find_node(lib, path, Some(false))) {
            (Some(id), _) => Ok(Some(id)),
            (None, Some(_)) => Err(format!("{} is a playlist, not a folder", shown(path))),
            (None, None) => Err(format!("no folder {}", shown(path))),
        }
    };
    match op {
        Op::Create { parent, name, folder } => {
            // Found by its whole path, as resolving and later ops find it: it can sit in a later
            // one of same-named folders than the first, which is where a new one goes.
            let path = [parent.clone(), vec![name.clone()]].concat();
            too_deep(&path, path.len())?;
            if find_node(db.lib(), &path, Some(*folder)).is_some() {
                return Ok(());
            }
            if find_node(db.lib(), &path, None).is_some() {
                return Err(Error::conflict(
                    Reason::Exists,
                    format!("{} exists already as a {}", shown(&path), kind(!*folder)),
                )
                .with("path", &path));
            }
            let parent_id = match folder_id(db.lib(), parent) {
                Ok(p) => p,
                Err(e) => return skip(e),
            };
            let id = db.create(name, parent_id.unwrap_or(0), *folder)?;
            let lib = db.lib();
            let sort_order = lib.children(parent_id).iter().map(|p| p.sort_order as i64).max().unwrap_or(-1) + 1;
            lib.playlists.push(Playlist {
                id,
                parent_id,
                name: name.clone(),
                is_folder: *folder,
                sort_order: sort_order as u32,
                track_ids: Vec::new(),
            });
        }
        Op::Rename { node, folder, name } => {
            let Some(id) = find_node(db.lib(), node, Some(*folder)) else {
                return skip(format!("no {} {}", kind(*folder), shown(node)));
            };
            let parent = db.lib().playlist(id).and_then(|p| p.parent_id);
            if db.lib().children(parent).iter().any(|p| p.id != id && p.name == *name) {
                return Err(Error::conflict(
                    Reason::Exists,
                    format!("something named {name:?} is already next to {}", shown(node)),
                )
                .with("name", name));
            }
            db.rename(id, name)?;
            if let Some(p) = db.lib().playlists.iter_mut().find(|p| p.id == id) {
                p.name = name.clone();
            }
        }
        Op::Place { node, folder, parent, position } => {
            let Some(id) = find_node(db.lib(), node, Some(*folder)) else {
                return skip(format!("no {} {}", kind(*folder), shown(node)));
            };
            // Of same-named folders, a path finds the first: a node in a later one that's placed
            // in a folder of its own folder's path is reordered where it is, never moved across.
            let parent_id = match db.lib().playlist(id).and_then(|p| p.parent_id) {
                Some(here) if db.lib().playlist_names(here) == *parent => Some(here),
                _ => match folder_id(db.lib(), parent) {
                    Ok(p) => p,
                    Err(e) => return skip(e),
                },
            };
            let mut cur = parent_id;
            while let Some(c) = cur {
                if c == id {
                    return Err(Error::invalid(format!("can't move {} into itself", shown(node))));
                }
                cur = db.lib().playlist(c).and_then(|p| p.parent_id);
            }
            // What's in it moves down with it.
            let (lib, mut below, mut level) = (&*db.lib(), 0, vec![id]);
            while !level.is_empty() {
                level = level.iter().flat_map(|&p| lib.children(Some(p))).map(|p| p.id).collect();
                below += usize::from(!level.is_empty());
            }
            too_deep(node, parent.len() + 1 + below)?;
            // Playlists are named by their path, so a move can't give one a namesake sibling (as
            // rename can't; reordering among namesakes rekordbox made stays possible).
            let name = node.last().cloned().unwrap_or_default();
            let moved = db.lib().playlist(id).is_some_and(|p| p.parent_id != parent_id);
            if moved && db.lib().children(parent_id).iter().any(|p| p.id != id && p.name == name) {
                return Err(Error::conflict(
                    Reason::Exists,
                    format!("something named {name:?} is already in {}", shown(parent)),
                )
                .with("name", &name));
            }
            let mut siblings: Vec<u32> =
                db.lib().children(parent_id).iter().map(|p| p.id).filter(|s| *s != id).collect();
            let at = position.unwrap_or(siblings.len()).min(siblings.len());
            siblings.insert(at, id);
            for (i, sib) in siblings.into_iter().enumerate() {
                let p = db.lib().playlist(sib).cloned().expect("listed above");
                if sib == id || p.sort_order != i as u32 {
                    db.place(sib, parent_id.unwrap_or(0), i as u32)?;
                    let p = db.lib().playlists.iter_mut().find(|p| p.id == sib).expect("listed above");
                    p.parent_id = parent_id;
                    p.sort_order = i as u32;
                }
            }
        }
        Op::Delete { node, folder } => {
            let Some(id) = find_node(db.lib(), node, Some(*folder)) else {
                return skip(format!("no {} {}", kind(*folder), shown(node)));
            };
            let mut doomed = vec![id];
            let mut i = 0;
            while i < doomed.len() {
                let kids: Vec<u32> = db.lib().children(Some(doomed[i])).iter().map(|p| p.id).collect();
                doomed.extend(kids);
                i += 1;
            }
            db.delete(&doomed)?;
            db.lib().playlists.retain(|p| !doomed.contains(&p.id));
        }
        Op::SetEntries { node, tracks, before } => {
            let id = match (find_node(db.lib(), node, Some(false)), find_node(db.lib(), node, Some(true))) {
                (Some(id), _) => id,
                (None, Some(_)) => return Err(Error::invalid(format!("{} is a folder", shown(node)))),
                (None, None) => return skip(format!("no playlist {}", shown(node))),
            };
            let (mut ids, missing) = ids_of(db.lib(), tracks);
            if !missing.is_empty() {
                skip(format!(
                    "{} track(s) for {} aren't in this database (e.g. {})",
                    missing.len(),
                    shown(node),
                    missing[0]
                ))?;
            }
            if let Some(before) = before {
                let lib = db.lib();
                let own: Vec<String> = lib
                    .playlist(id)
                    .map(|p| {
                        lib.tracks_by_id(&p.track_ids).into_iter().map(|t| paths::file_key(&t.file_path)).collect()
                    })
                    .unwrap_or_default();
                if own != *before {
                    warnings.push(format!(
                        "{label}: {}'s entries differ from the ones the edit was planned on; it's made in its own",
                        shown(node)
                    ));
                    let rebased: Vec<String> =
                        rebase_entries(&own, before, tracks).into_iter().map(String::from).collect();
                    ids = ids_of(lib, &rebased).0;
                }
            }
            if db.lib().playlist(id).is_some_and(|p| p.track_ids == ids) {
                return Ok(());
            }
            db.set_entries(id, &ids)?;
            if let Some(p) = db.lib().playlists.iter_mut().find(|p| p.id == id) {
                p.track_ids = ids;
            }
        }
        Op::SetFields { tracks, fields } => {
            let (ids, missing) = ids_of(db.lib(), tracks);
            if !missing.is_empty() {
                skip(format!("{} track(s) aren't in this database (e.g. {})", missing.len(), missing[0]))?;
            }
            if ids.is_empty() {
                return Ok(());
            }
            db.set_fields(&ids, fields)?;
            for t in db.lib().tracks.iter_mut().filter(|t| ids.contains(&t.id)) {
                fields.apply_to(t);
            }
        }
        Op::DeleteTracks { tracks, elsewhere } => {
            let (_, missing) = ids_of(db.lib(), tracks);
            if !missing.is_empty() {
                skip(format!("{} track(s) aren't in this database (e.g. {})", missing.len(), missing[0]))?;
            }
            // Every row of a file: two can share one, and its files go.
            let files: FxHashSet<&String> = tracks.iter().chain(elsewhere).collect();
            let ids: Vec<u32> = (db.lib().tracks.iter())
                .filter(|t| files.contains(&paths::file_key(&t.file_path)))
                .map(|t| t.id)
                .collect();
            if ids.is_empty() {
                return Ok(());
            }
            db.delete_tracks(&ids)?;
            let lib = db.lib();
            lib.tracks.retain(|t| !ids.contains(&t.id));
            for p in lib.playlists.iter_mut().chain(lib.history.iter_mut()) {
                p.track_ids.retain(|t| !ids.contains(t));
            }
        }
        Op::Repoint { from, to } => {
            let (ids, missing) = ids_of(db.lib(), &[from.clone(), to.clone()]);
            if !missing.is_empty() {
                return skip(format!("track {} isn't in this database", missing[0]));
            }
            let (from, to) = (ids[0], ids[1]);
            db.repoint(from, to)?;
            let lib = db.lib();
            for p in lib.playlists.iter_mut().chain(lib.history.iter_mut()) {
                for t in p.track_ids.iter_mut().filter(|t| **t == from) {
                    *t = to;
                }
            }
        }
        Op::SetPath { track, path, file, shift_ms } => {
            let (ids, missing) = ids_of(db.lib(), std::slice::from_ref(track));
            if !missing.is_empty() {
                return skip(format!("track {track} isn't in this database"));
            }
            db.set_path(ids[0], path, file.as_ref(), *shift_ms)?;
            let size = file.map(|f| db.stored_size(f.size));
            if let Some(t) = db.lib().tracks.iter_mut().find(|t| t.id == ids[0]) {
                t.file_path = path.clone();
                if let (Some(f), Some(size)) = (file, size) {
                    (t.file_size, t.sample_rate, t.sample_depth, t.bitrate) =
                        (size, f.sample_rate, f.bit_depth, f.bitrate);
                }
            }
        }
        Op::DeleteHistory { lists } => {
            let mut ids = Vec::new();
            for (name, nth) in lists {
                match db.lib().history.iter().filter(|h| !h.is_folder && h.name == *name).nth(*nth) {
                    Some(h) => ids.push(h.id),
                    None => skip(format!("no history list {name:?}"))?,
                }
            }
            if ids.is_empty() {
                return Ok(());
            }
            db.delete_history(&ids)?;
            db.lib().history.retain(|h| !ids.contains(&h.id));
            db.lib().history_playlists = db.lib().history.iter().filter(|h| !h.is_folder).count();
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Resolving edits
// ---------------------------------------------------------------------------------------------

/// What resolving an edit needs to know.
struct Resolver<'a> {
    device: &'a Device,
    /// What the edits' selectors need from the drive besides its library.
    extra: &'a DriveContext,
    /// Replaying recorded edits: tracks and history lists an edit names that are gone are left
    /// out of it (with a warning) rather than failing it.
    lenient: bool,
    /// Warnings about what was left out, for the edit being resolved.
    notes: std::cell::RefCell<Vec<String>>,
    /// Stops an edit that takes a while (ordering a long playlist for mixing) when cancelled.
    ctx: Ctx<'a>,
    /// Audio files (keys) the edit writes, which a track may be pointed at before they exist.
    incoming: &'a [String],
    /// Audio files (keys) of tracks only the drive's other database lists, which a delete may
    /// name.
    elsewhere: FxHashSet<String>,
}

fn split_path(path: &str) -> Result<NodePath> {
    let names: NodePath = path.split('/').map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).collect();
    if names.is_empty() {
        return Err(Error::invalid("give a playlist name"));
    }
    Ok(names)
}

fn playlist<'l>(lib: &'l Library, spec: &str) -> Result<&'l Playlist> {
    lib.find_playlist(spec)
        .ok_or_else(|| Error::NotFound(format!("playlist {spec:?} (give its path, like Folder/Name)")))
}

/// A playlist, not a folder: one shown after a folder of its name too.
fn list<'l>(lib: &'l Library, spec: &str) -> Result<&'l Playlist> {
    match lib.find_playlist_of_kind(spec, false) {
        Some(p) => Ok(p),
        None if playlist(lib, spec)?.is_folder => Err(Error::invalid(format!("{spec:?} is a folder; give a playlist"))),
        None => Err(Error::NotFound(format!("playlist {spec:?}"))),
    }
}

/// `p`, found for `spec`, unless a replay (`recorded`) finds it elsewhere than at the path `spec`
/// gives (ignoring case, as lookups do): the journal names a playlist by its path, and once the
/// one there is gone, a lookup by name alone finds another of that name in some folder.
fn at_recorded_path<'l>(lib: &Library, p: &'l Playlist, spec: &str, recorded: bool) -> Result<&'l Playlist> {
    let path: Vec<String> =
        lib.playlist_names(p.id).iter().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect();
    if recorded && !path.join("/").eq_ignore_ascii_case(&split_path(spec)?.join("/")) {
        return Err(Error::NotFound(format!("playlist {spec:?}")));
    }
    Ok(p)
}

/// The playlist at exactly the path `spec`, for an edit that makes it when it's missing: never
/// one of that name in another folder, nor one whose path differs in case (a rule for `FoO` makes
/// it next to a `Foo`). When several playlists' paths match ignoring case, which one was meant
/// can't be told, and the error names them.
pub(crate) fn find_exact<'l>(lib: &'l Library, spec: &str) -> Result<Option<&'l Playlist>> {
    let names = split_path(spec)?;
    if let Some(id) = find_node(lib, &names, Some(false)) {
        return Ok(lib.playlist(id));
    }
    let path = names.join("/");
    let mut near: Vec<String> = Vec::new();
    for (p, _) in lib.display_order().into_iter().filter(|(p, n)| !n.is_folder && p.eq_ignore_ascii_case(&path)) {
        if !near.contains(&p) {
            near.push(p);
        }
    }
    if near.len() > 1 {
        return Err(Error::invalid(format!(
            "{path:?} matches {} ignoring case; give the exact path",
            near.iter().map(|p| format!("{p:?}")).collect::<Vec<_>>().join(" and ")
        )));
    }
    Ok(None)
}

/// 1-based positions to 0-based indexes, checked against a playlist of `len` entries.
fn indexes(positions: &[usize], len: usize) -> Result<Vec<usize>> {
    let mut out = Vec::new();
    for &p in positions {
        if p == 0 || p > len {
            return Err(Error::invalid(format!("position {p} is outside the playlist (1-{len})")));
        }
        out.push(p - 1);
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

impl<'a> Resolver<'a> {
    fn new(device: &'a Device, extra: &'a DriveContext, lenient: bool, ctx: Ctx<'a>) -> Resolver<'a> {
        Resolver {
            device,
            extra,
            lenient,
            notes: Default::default(),
            ctx,
            incoming: &[],
            elsewhere: Default::default(),
        }
    }

    fn note(&self, warning: String) {
        self.notes.borrow_mut().push(warning);
    }

    /// Of recorded audio files (device paths), those of tracks `lib` has. Strictly every one
    /// must be there (as selecting by path requires); leniently the others are left out.
    fn present(&self, lib: &Library, files: &[String], what: &str) -> Vec<String> {
        if !self.lenient {
            return files.to_vec();
        }
        let have: FxHashSet<String> = lib.tracks.iter().map(|t| paths::file_key(&t.file_path)).collect();
        let (kept, gone): (Vec<String>, Vec<String>) =
            files.iter().cloned().partition(|f| have.contains(&paths::file_key(f)));
        if let Some(example) = gone.first() {
            self.note(format!(
                "{} track(s) {what} aren't on the drive any more (e.g. {example}); the rest are replayed",
                gone.len()
            ));
        }
        kept
    }

    fn keys(lib: &Library, ids: &[u32]) -> Vec<String> {
        lib.tracks_by_id(ids).into_iter().map(|t| paths::file_key(&t.file_path)).collect()
    }

    fn device_paths(lib: &Library, ids: &[u32]) -> Vec<String> {
        lib.tracks_by_id(ids).into_iter().map(|t| t.file_path.clone()).collect()
    }

    fn select(&self, lib: &Library, sel: &TrackSelector, allow_empty: bool) -> Result<Vec<u32>> {
        let ctx = self.extra.query(lib);
        if self.lenient && !sel.paths.is_empty() {
            let paths = self.present(lib, &sel.paths, "the edit selects");
            return TrackSelector { paths, ..sel.clone() }.select_ordered(&ctx, allow_empty);
        }
        sel.select_ordered(&ctx, allow_empty)
    }

    /// Ops for `edit` against `lib` (the expected library so far), with a description, and the
    /// edit as recorded in the journal: tracks named by file, and what changed rather than what
    /// it left where a later export can add to it, so it replays there (a move, and a playlist
    /// deleted with its tracks, take two edits).
    fn resolve(&self, edit: &Edit, lib: &Library) -> Result<(Vec<Op>, String, Vec<Edit>)> {
        let entries = |p: &Playlist| -> Vec<u32> { p.track_ids.clone() };
        // A playlist or folder as the journal names it: by its path, which a folder's ends in `/`
        // (a playlist and a folder can share one).
        let named = |p: &Playlist| lib.playlist_path(p.id) + if p.is_folder { "/" } else { "" };
        let playlist = |spec: &str| playlist(lib, spec).and_then(|p| at_recorded_path(lib, p, spec, self.lenient));
        let list = |spec: &str| list(lib, spec).and_then(|p| at_recorded_path(lib, p, spec, self.lenient));
        let set = |p: &Playlist, ids: &[u32]| Op::SetEntries {
            node: lib.playlist_names(p.id),
            tracks: Self::keys(lib, ids),
            before: Some(Self::keys(lib, &p.track_ids)),
        };
        // An edit that rewrites entries as the journal keeps it: the entries themselves, by file.
        // With `create`, a replay makes the playlist at its path if it's missing, so only when
        // that path names it (a name with a slash, or spaces around it, doesn't).
        let record = |p: &Playlist, ids: &[u32], create: bool| {
            let playlist = lib.playlist_path(p.id);
            let create = create && split_path(&playlist).is_ok_and(|n| find_node(lib, &n, Some(false)) == Some(p.id));
            Edit::SetEntries { playlist, paths: Self::device_paths(lib, ids), create }
        };
        // Resolving such an edit for the playlist at exactly `path`, made first if missing.
        let fill_new = |path: &str, ids: &[u32]| {
            let playlist = split_path(path)?.join("/");
            self.resolve(&Edit::SetEntries { playlist, paths: Self::device_paths(lib, ids), create: true }, lib)
        };
        let (ops, desc, canonical) = match edit {
            Edit::CreatePlaylist { path, folder } => {
                let names = split_path(path)?;
                let mut ops = Vec::new();
                for i in 1..names.len() {
                    if find_node(lib, &names[..i], Some(true)).is_none() {
                        ops.push(Op::Create {
                            parent: names[..i - 1].to_vec(),
                            name: names[i - 1].clone(),
                            folder: true,
                        });
                    }
                }
                let exists = find_node(lib, &names, Some(*folder)).is_some();
                ops.push(Op::Create {
                    parent: names[..names.len() - 1].to_vec(),
                    name: names[names.len() - 1].clone(),
                    folder: *folder,
                });
                let what = if *folder { "folder" } else { "playlist" };
                let desc = if exists {
                    format!("{what} {} exists already", names.join("/"))
                } else {
                    format!("create {what} {}", names.join("/"))
                };
                (ops, desc, edit.clone())
            }
            Edit::RenamePlaylist { playlist: spec, name } => {
                let p = playlist(spec)?;
                let name = name.trim();
                if name.is_empty() || name.contains('/') {
                    return Err(Error::invalid("names can't be empty or contain /"));
                }
                let node = lib.playlist_names(p.id);
                let canonical = Edit::RenamePlaylist { playlist: named(p), name: name.to_string() };
                // Its name already (names are compared as written, as everywhere here): nothing
                // to write, where a rename op counts as a change whatever it renames to. (With a
                // twin of that name beside it, the op says the name is taken.)
                let twin = lib.children(p.parent_id).iter().any(|q| q.id != p.id && q.name == name);
                if p.name == name && !twin {
                    return Ok((Vec::new(), format!("{} is called that already", node.join("/")), vec![canonical]));
                }
                let desc = format!("rename {} to {name:?}", node.join("/"));
                (vec![Op::Rename { node, folder: p.is_folder, name: name.to_string() }], desc, canonical)
            }
            Edit::MovePlaylist { playlist: spec, into, position } => {
                let p = playlist(spec)?;
                let folder = match into.as_deref().map(str::trim).filter(|s| !s.is_empty() && *s != "/") {
                    None => None,
                    Some(f) => match lib
                        .find_playlist_of_kind(f, true)
                        .filter(|x| at_recorded_path(lib, x, f, self.lenient).is_ok())
                    {
                        Some(folder) => Some(folder),
                        None => {
                            playlist(f)?;
                            return Err(Error::invalid(format!("{f:?} is a playlist, not a folder")));
                        }
                    },
                };
                let parent = folder.map(|f| lib.playlist_names(f.id)).unwrap_or_default();
                if *position == Some(0) {
                    return Err(Error::invalid("positions start at 1"));
                }
                let node = lib.playlist_names(p.id);
                let desc = format!(
                    "move {} into {}{}",
                    node.join("/"),
                    shown(&parent),
                    position.map(|n| format!(" at position {n}")).unwrap_or_default()
                );
                let place = Op::Place { node, folder: p.is_folder, parent, position: position.map(|n| n - 1) };
                let into = folder.map(|f| lib.playlist_path(f.id));
                (vec![place], desc, Edit::MovePlaylist { playlist: named(p), into, position: *position })
            }
            Edit::DeletePlaylist { playlist: spec, with_tracks } => {
                let p = playlist(spec)?;
                let node = lib.playlist_names(p.id);
                let mut desc = if p.is_folder {
                    format!("delete folder {} and everything in it", node.join("/"))
                } else {
                    format!("delete playlist {} ({} entries)", node.join("/"), p.track_ids.len())
                };
                let mut ops = vec![Op::Delete { node, folder: p.is_folder }];
                let mut recorded = vec![Edit::DeletePlaylist { playlist: named(p), with_tracks: false }];
                if *with_tracks {
                    let mut inside = FxHashSet::from_iter([p.id]);
                    let mut queue = vec![p.id];
                    while let Some(id) = queue.pop() {
                        for c in lib.children(Some(id)) {
                            inside.insert(c.id);
                            queue.push(c.id);
                        }
                    }
                    let used_elsewhere: FxHashSet<u32> = lib
                        .playlists
                        .iter()
                        .filter(|q| !inside.contains(&q.id))
                        .chain(lib.history.iter())
                        .flat_map(|q| q.track_ids.iter().copied())
                        .collect();
                    // A track goes with every row of its file: not one whose file a track used
                    // elsewhere shares.
                    let files_elsewhere: FxHashSet<String> = (lib.tracks.iter())
                        .filter(|t| used_elsewhere.contains(&t.id))
                        .map(|t| paths::file_key(&t.file_path))
                        .collect();
                    let mut only_here: Vec<u32> = lib
                        .playlists
                        .iter()
                        .filter(|q| inside.contains(&q.id))
                        .flat_map(|q| q.track_ids.iter().copied())
                        .filter(|t| !used_elsewhere.contains(t))
                        .filter(|t| {
                            lib.track(*t).is_none_or(|t| !files_elsewhere.contains(&paths::file_key(&t.file_path)))
                        })
                        .collect();
                    only_here.sort_unstable();
                    only_here.dedup();
                    if !only_here.is_empty() {
                        desc.push_str(&format!(", with the {} track(s) only it uses", only_here.len()));
                        ops.push(Op::DeleteTracks { tracks: Self::keys(lib, &only_here), elsewhere: Vec::new() });
                        // Recorded as the tracks it deleted: a replay deletes those, not tracks
                        // added to the playlist since.
                        recorded.push(Edit::DeleteTracks {
                            tracks: TrackSelector::paths(Self::device_paths(lib, &only_here)),
                            keep_files: false,
                        });
                    }
                }
                return Ok((ops, desc, recorded));
            }
            Edit::AddTracks { playlist: spec, tracks, position, sort, allow_repeats } => {
                let p = list(spec)?;
                let mut add = self.select(lib, tracks, false)?;
                if let Some(s) = sort {
                    add = sorted(lib, &add, s)?;
                }
                let old = entries(p);
                if !allow_repeats {
                    add.retain(|id| !old.contains(id));
                }
                let at = match position {
                    Some(0) => return Err(Error::invalid("positions start at 1")),
                    Some(n) => (*n - 1).min(old.len()),
                    None => old.len(),
                };
                let mut new = old.clone();
                new.splice(at..at, add.iter().copied());
                let desc = format!("add {} track(s) to {}", add.len(), lib.playlist_path(p.id));
                // Adding none records nothing: a replay would select no tracks.
                if add.is_empty() {
                    return Ok((Vec::new(), desc, Vec::new()));
                }
                let canonical = Edit::AddTracks {
                    playlist: lib.playlist_path(p.id),
                    tracks: TrackSelector::paths(Self::device_paths(lib, &add)),
                    position: *position,
                    sort: None,
                    allow_repeats: *allow_repeats,
                };
                (vec![set(p, &new)], desc, canonical)
            }
            Edit::RemoveTracks { playlist: spec, tracks, positions } => {
                let p = list(spec)?;
                let old = entries(p);
                let doomed: FxHashSet<u32> = match tracks {
                    Some(sel) => self.select(lib, sel, true)?.into_iter().collect(),
                    None => FxHashSet::default(),
                };
                let at: FxHashSet<usize> = indexes(positions, old.len())?.into_iter().collect();
                if tracks.is_none() && positions.is_empty() {
                    return Err(Error::invalid("say which tracks or positions to remove"));
                }
                let new: Vec<u32> = old
                    .iter()
                    .enumerate()
                    .filter(|(i, t)| !at.contains(i) && !doomed.contains(t))
                    .map(|(_, t)| *t)
                    .collect();
                let desc = format!("remove {} entr(ies) from {}", old.len() - new.len(), lib.playlist_path(p.id));
                // Recorded as the files whose entries it removed, so a replay leaves entries a
                // later export added; when it left some entries of those files, as what it left.
                let mut removed: Vec<u32> = old
                    .iter()
                    .enumerate()
                    .filter(|(i, t)| at.contains(i) || doomed.contains(t))
                    .map(|(_, t)| *t)
                    .collect();
                let mut seen = FxHashSet::default();
                removed.retain(|t| seen.insert(*t));
                // Removing none records nothing: a replay would select no tracks.
                if removed.is_empty() {
                    return Ok((Vec::new(), desc, Vec::new()));
                }
                let canonical = match removed.iter().any(|t| new.contains(t)) {
                    false => Edit::RemoveTracks {
                        playlist: lib.playlist_path(p.id),
                        tracks: Some(TrackSelector::paths(Self::device_paths(lib, &removed))),
                        positions: Vec::new(),
                    },
                    true => record(p, &new, false),
                };
                (vec![set(p, &new)], desc, canonical)
            }
            Edit::MoveTracks { playlist: spec, positions, to } => {
                let p = list(spec)?;
                let old = entries(p);
                let picked = indexes(positions, old.len())?;
                if picked.is_empty() {
                    return Err(Error::invalid("say which positions to move"));
                }
                let moving: Vec<u32> = picked.iter().map(|i| old[*i]).collect();
                let mut rest: Vec<u32> =
                    old.iter().enumerate().filter(|(i, _)| !picked.contains(i)).map(|(_, t)| *t).collect();
                let at = to.saturating_sub(1).min(rest.len());
                // Recorded as the files it moved coming out and going back in at `to`, so a replay
                // leaves entries a later export added; when those files have other entries too,
                // as what it left.
                let alone = moving.iter().collect::<FxHashSet<_>>().len() == moving.len()
                    && !rest.iter().any(|t| moving.contains(t));
                let playlist = lib.playlist_path(p.id);
                let files = TrackSelector::paths(Self::device_paths(lib, &moving));
                rest.splice(at..at, moving);
                let desc = format!("move {} entr(ies) in {playlist} to position {to}", picked.len());
                if alone {
                    let recorded = vec![
                        Edit::RemoveTracks {
                            playlist: playlist.clone(),
                            tracks: Some(files.clone()),
                            positions: Vec::new(),
                        },
                        Edit::AddTracks {
                            playlist,
                            tracks: files,
                            position: Some(at + 1),
                            sort: None,
                            allow_repeats: false,
                        },
                    ];
                    return Ok((vec![set(p, &rest)], desc, recorded));
                }
                (vec![set(p, &rest)], desc, record(p, &rest, false))
            }
            Edit::SetTracks { playlist: spec, tracks, sort, limit, create } => {
                let mut ids = self.select(lib, tracks, true)?;
                if let Some(s) = sort {
                    ids = sorted(lib, &ids, s)?;
                }
                if let Some(n) = limit {
                    ids.truncate(*n);
                }
                // Recorded as the tracks picked: selecting again on a later export, where ids and
                // library order differ, can pick others. With `create` too: the journal may lack
                // the edit that made the playlist (cleared, or a merge made it), and a replay after
                // a new export makes it again. A playlist to make if missing is found by its exact
                // path only (see `find_exact`).
                let desc = |path: &str| format!("fill {path} with {} track(s)", ids.len());
                let found = match create {
                    true => find_exact(lib, spec)?,
                    false => lib
                        .find_playlist_of_kind(spec, false)
                        .filter(|p| at_recorded_path(lib, p, spec, self.lenient).is_ok()),
                };
                // The rule says what the playlist holds, so no database keeps entries of its own.
                let fill = |p: &Playlist| Op::SetEntries {
                    node: lib.playlist_names(p.id),
                    tracks: Self::keys(lib, &ids),
                    before: None,
                };
                match found {
                    Some(p) => (vec![fill(p)], desc(&lib.playlist_path(p.id)), record(p, &ids, *create)),
                    None if *create => {
                        let (ops, _, canonical) = fill_new(spec, &ids)?;
                        return Ok((ops, desc(&split_path(spec)?.join("/")), canonical));
                    }
                    None => return Err(list(spec).err().unwrap_or_else(|| Error::NotFound(spec.clone()))),
                }
            }
            Edit::SetEntries { playlist: spec, paths: files_given, create } => {
                // Recorded by the path it was found at, as every edit is: a replay finds a playlist
                // only there, never another of its name.
                let (mut ops, node, playlist) = match create {
                    true => {
                        find_exact(lib, spec)?;
                        let (ops, _, _) =
                            self.resolve(&Edit::CreatePlaylist { path: spec.clone(), folder: false }, lib)?;
                        (ops, split_path(spec)?, spec.clone())
                    }
                    false => {
                        let p = list(spec)?;
                        (Vec::new(), lib.playlist_names(p.id), named(p))
                    }
                };
                // A file with no track fails planning, as selecting it by path does (a replay
                // leaves it out).
                let files = self.present(lib, files_given, &format!("recorded in {}", node.join("/")));
                let tracks = files.iter().map(|f| paths::file_key(f)).collect();
                let desc = format!("fill {} with {} entr(ies)", node.join("/"), files.len());
                ops.push(Op::SetEntries { node, tracks, before: None });
                (ops, desc, Edit::SetEntries { playlist, paths: files_given.clone(), create: *create })
            }
            Edit::SortPlaylist { playlist: spec, by } => {
                let p = list(spec)?;
                let new = sorted(lib, &entries(p), by)?;
                let canonical = Edit::SortPlaylist { playlist: named(p), by: by.clone() };
                (vec![set(p, &new)], format!("sort {} by {by}", lib.playlist_path(p.id)), canonical)
            }
            Edit::Dedupe { playlist: spec, recordings } => {
                let p = list(spec)?;
                let mut seen = FxHashSet::default();
                let new: Vec<u32> = entries(p)
                    .into_iter()
                    .filter(|id| {
                        let k = match (recordings, lib.track(*id)) {
                            (true, Some(t)) => t.fingerprint_key(),
                            _ => id.to_string(),
                        };
                        seen.insert(k)
                    })
                    .collect();
                let desc = format!("drop {} repeat(s) from {}", p.track_ids.len() - new.len(), lib.playlist_path(p.id));
                (vec![set(p, &new)], desc, Edit::Dedupe { playlist: named(p), recordings: *recordings })
            }
            Edit::PlanSet { playlist: spec, start, into } => {
                let p = list(spec)?;
                let mut seen = FxHashSet::default();
                let ids: Vec<u32> = p.track_ids.iter().copied().filter(|id| seen.insert(*id)).collect();
                let tracks = lib.tracks_by_id(&ids);
                let start = match start {
                    Some(sel) => self.select(lib, sel, false)?.first().copied(),
                    None => None,
                };
                let energy = self.energy(&tracks);
                // How sure each key is, from the keys analyzers heard (none is analysed here).
                let sure = crate::sound::key_confidences(self.device, tracks.iter().copied());
                // Recorded as the order it gives: `start` may name a track by id, and the analysis
                // behind the energy and keys can change on a later export.
                let order = harmony::order_set(&tracks, start, &energy, &sure, &self.ctx)?;
                let desc = format!("order {} for mixing", lib.playlist_path(p.id));
                match into {
                    None => (vec![set(p, &order)], desc, record(p, &order, false)),
                    Some(target) => {
                        let (ops, _, canonical) = fill_new(target, &order)?;
                        return Ok((ops, format!("{desc} into {target}"), canonical));
                    }
                }
            }
            Edit::EditTracks { tracks, fields } => {
                fields.validate()?;
                let ids = self.select(lib, tracks, false)?;
                if fields.album_artist.as_deref().is_some_and(|a| !a.trim().is_empty()) {
                    let no_album = lib
                        .tracks_by_id(&ids)
                        .into_iter()
                        .find(|t| fields.album.as_ref().unwrap_or(&t.album).trim().is_empty());
                    if let Some(t) = no_album {
                        return Err(Error::invalid(format!(
                            "{} has no album to keep an album artist with: give one too",
                            t.display_name()
                        )));
                    }
                }
                let desc = format!("set {} on {} track(s)", fields.describe(), ids.len());
                let canonical = Edit::EditTracks {
                    tracks: TrackSelector::paths(Self::device_paths(lib, &ids)),
                    fields: fields.clone(),
                };
                (
                    vec![Op::SetFields { tracks: Self::keys(lib, &ids), fields: Box::new(fields.clone()) }],
                    desc,
                    canonical,
                )
            }
            Edit::DeleteTracks { tracks, keep_files } => {
                let (elsewhere, here): (Vec<String>, Vec<String>) =
                    tracks.paths.iter().cloned().partition(|p| self.elsewhere.contains(&paths::file_key(p)));
                // Replaying, the tracks already gone are gone.
                let here = self.present(lib, &here, "the edit selects");
                let rest = TrackSelector { paths: here, ..tracks.clone() };
                let ids = match rest.is_empty() && !tracks.is_empty() {
                    true => Vec::new(),
                    false => self.select(lib, &rest, false)?,
                };
                let desc = format!(
                    "delete {} track(s){}",
                    ids.len() + elsewhere.len(),
                    if *keep_files { " (their files stay on the drive)" } else { "" }
                );
                let canonical = Edit::DeleteTracks {
                    tracks: TrackSelector::paths(Self::device_paths(lib, &ids).into_iter().chain(elsewhere.clone())),
                    keep_files: *keep_files,
                };
                let elsewhere = elsewhere.iter().map(|p| paths::file_key(p)).collect();
                (vec![Op::DeleteTracks { tracks: Self::keys(lib, &ids), elsewhere }], desc, canonical)
            }
            Edit::ReplaceTrack { duplicate, keep, keep_duplicate } => {
                let one = |sel: &TrackSelector, what: &str| -> Result<&Track> {
                    match self.select(lib, sel, false)?.as_slice() {
                        [id] => Ok(lib.track(*id).expect("selected")),
                        ids => {
                            Err(Error::invalid(format!("select exactly one track to {what} ({} selected)", ids.len())))
                        }
                    }
                };
                let (dup, keeper) = (one(duplicate, "replace")?, one(keep, "keep")?);
                // The duplicate goes with every row of its file.
                if paths::file_key(&dup.file_path) == paths::file_key(&keeper.file_path) {
                    return Err(Error::invalid("a track can't replace itself, or another of its own file"));
                }
                let mut ops =
                    vec![Op::Repoint { from: paths::file_key(&dup.file_path), to: paths::file_key(&keeper.file_path) }];
                if !keep_duplicate {
                    ops.push(Op::DeleteTracks { tracks: vec![paths::file_key(&dup.file_path)], elsewhere: Vec::new() });
                }
                let desc = format!("replace {} with {}", dup.display_name(), keeper.display_name());
                let canonical = Edit::ReplaceTrack {
                    duplicate: TrackSelector::paths([dup.file_path.clone()]),
                    keep: TrackSelector::paths([keeper.file_path.clone()]),
                    keep_duplicate: *keep_duplicate,
                };
                (ops, desc, canonical)
            }
            Edit::SetPath { track, path, file, shift_ms } => {
                let t = match self.select(lib, track, false)?.as_slice() {
                    [id] => lib.track(*id).expect("selected"),
                    ids => {
                        return Err(Error::invalid(format!(
                            "select exactly one track to point ({} selected)",
                            ids.len()
                        )));
                    }
                };
                // An audio file on the drive, as the drive spells its name, never one of
                // locallibrary's or rekordbox's own.
                let on_drive = self.device.file_key(&paths::key(path));
                let key = on_drive.clone().unwrap_or_else(|| paths::key(path));
                let audio = paths::is_safe_key(&key)
                    && !key.split('/').any(paths::is_ignored_name)
                    && matches!(paths::classify(&key), None | Some(paths::FileClass::Audio))
                    && (on_drive.is_some() || self.incoming.contains(&key));
                if !audio {
                    return Err(Error::NotFound(format!("audio file {path:?} on the drive")));
                }
                if let Some(other) =
                    lib.tracks.iter().find(|o| o.id != t.id && paths::file_key(&o.file_path) == key.to_lowercase())
                {
                    return Err(Error::invalid(format!("{path} is the audio file of {}", other.display_name())));
                }
                // Each database's edit finds the track by its file: two rows of it can't be told apart.
                if lib.tracks.iter().filter(|o| paths::file_key(&o.file_path) == paths::file_key(&t.file_path)).count()
                    > 1
                {
                    return Err(Error::invalid(format!(
                        "{} is the file of several tracks, which can't be told apart",
                        t.file_path
                    )));
                }
                let path = paths::device_path(&key);
                let desc = format!("point {} at {path}", t.display_name());
                let (file, shift_ms) = (*file, *shift_ms);
                let canonical = Edit::SetPath {
                    track: TrackSelector::paths([t.file_path.clone()]),
                    path: path.clone(),
                    file,
                    shift_ms,
                };
                (vec![Op::SetPath { track: paths::file_key(&t.file_path), path, file, shift_ms }], desc, canonical)
            }
            Edit::DeleteHistory { names, all } => {
                let lists: Vec<&Playlist> = lib.history.iter().filter(|h| !h.is_folder).collect();
                let mut picked = Vec::new();
                let mut counts: FxHashMap<&str, usize> = FxHashMap::default();
                for h in &lists {
                    let nth = counts.entry(h.name.as_str()).or_default();
                    if *all || names.iter().any(|n| n.trim().eq_ignore_ascii_case(&h.name)) {
                        picked.push((h.name.clone(), *nth));
                    }
                    *nth += 1;
                }
                for n in names {
                    if !lists.iter().any(|h| n.trim().eq_ignore_ascii_case(&h.name)) {
                        return Err(Error::NotFound(format!("history list {n:?}")));
                    }
                }
                if picked.is_empty() && !all {
                    return Err(Error::invalid("name the history lists to delete, or delete all"));
                }
                let desc = format!("delete {} history list(s)", picked.len());
                // Deleting all of none records nothing: a replay would find no list to delete.
                if picked.is_empty() {
                    return Ok((Vec::new(), desc, Vec::new()));
                }
                // Recorded as the lists it deletes, by name and entries: replaying `all`, or the
                // names, would also take lists players recorded since.
                let lists = picked
                    .iter()
                    .map(|(name, nth)| {
                        let h = lists.iter().filter(|h| h.name == *name).nth(*nth).expect("picked");
                        HistoryList { name: name.clone(), tracks: Self::device_paths(lib, &h.track_ids) }
                    })
                    .collect();
                (vec![Op::DeleteHistory { lists: picked }], desc, Edit::DeleteHistoryLists { lists })
            }
            Edit::MyTag(_) | Edit::Cues { .. } | Edit::SetArtwork { .. } | Edit::SetAnalysis { .. } => {
                unreachable!("planned as steps (see `Resolver::step`)")
            }
            Edit::DeleteHistoryLists { lists: wanted } => {
                let lists: Vec<&Playlist> = lib.history.iter().filter(|h| !h.is_folder).collect();
                let files = |h: &Playlist| -> Vec<String> { Self::keys(lib, &h.track_ids) };
                let mut picked: Vec<(String, usize)> = Vec::new();
                for w in wanted {
                    let tracks: Vec<String> = w.tracks.iter().map(|f| paths::file_key(f)).collect();
                    // The first list with its name and entries not picked already.
                    let found = lists
                        .iter()
                        .filter(|h| h.name == w.name)
                        .enumerate()
                        .find(|(nth, h)| !picked.contains(&(w.name.clone(), *nth)) && files(h) == tracks);
                    match found {
                        Some((nth, _)) => picked.push((w.name.clone(), nth)),
                        None if self.lenient => {
                            self.note(format!("history list {:?} isn't on the drive as it was deleted", w.name))
                        }
                        None => {
                            return Err(Error::NotFound(format!(
                                "history list {:?} with those {} entries",
                                w.name,
                                tracks.len()
                            )));
                        }
                    }
                }
                if picked.is_empty() {
                    return Err(Error::NotFound("the history lists to delete".into()));
                }
                let desc = format!("delete {} history list(s)", picked.len());
                let lists = picked
                    .iter()
                    .map(|(name, nth)| {
                        let h = lists.iter().filter(|h| h.name == *name).nth(*nth).expect("picked");
                        HistoryList { name: name.clone(), tracks: Self::device_paths(lib, &h.track_ids) }
                    })
                    .collect();
                (vec![Op::DeleteHistory { lists: picked }], desc, Edit::DeleteHistoryLists { lists })
            }
        };
        Ok((ops, desc, vec![canonical]))
    }

    /// A step of a plan ([`Edit::is_step`], and pointing a track at another file in a replay) as
    /// it's recorded and made, with a description. Each is planned in full only as it's made,
    /// after the ones before it, so here, planning edits as given, only the tracks it names are
    /// checked to be on the drive. An artwork edit's image is staged: its bytes go in `images`
    /// unless it was given staged.
    fn step(&self, edit: &Edit, lib: &Library, images: &mut FxHashMap<Hash, Vec<u8>>) -> Result<(Edit, String)> {
        let on_drive = |files: &[String]| match self.lenient {
            true => Ok(()),
            false => self.select(lib, &TrackSelector::paths(files.iter().cloned()), false).map(drop),
        };
        let desc = match edit {
            Edit::MyTag(change) => {
                if let TagChange::Add { tracks, .. } | TagChange::Remove { tracks, .. } = change {
                    on_drive(tracks)?;
                }
                change.describe()
            }
            Edit::Cues { tracks, change } => {
                on_drive(tracks)?;
                if let CueChange::Copy { from } | CueChange::Carry { from, .. } = change {
                    on_drive(std::slice::from_ref(from))?;
                }
                change.describe(tracks.len())
            }
            Edit::SetArtwork { tracks, image } => {
                // Replaying, the recorded files are found as it's made, after the edits pointing
                // tracks at other files before it, as a cue edit's are.
                let files = match self.lenient && *tracks == TrackSelector::paths(tracks.paths.iter().cloned()) {
                    true => tracks.paths.clone(),
                    false => Self::device_paths(lib, &self.select(lib, tracks, false)?),
                };
                let (hash, bytes) = crate::artwork::stage(self.device, image)?;
                images.extend(bytes.map(|b| (hash, b)));
                let desc = format!("set the artwork of {} track(s)", files.len());
                let canonical =
                    Edit::SetArtwork { tracks: TrackSelector::paths(files), image: ImageSource::Staged(hash) };
                return Ok((canonical, desc));
            }
            Edit::SetAnalysis { track, analysis } => {
                on_drive(std::slice::from_ref(track))?;
                if analysis.is_empty() {
                    return Err(Error::invalid("say what to write: a beat grid, phrases or a waveform"));
                }
                let mut analysis = analysis.clone();
                if let Some(wave) = &analysis.waveform {
                    let (hash, bytes) = crate::analysis_edit::stage(self.device, wave)?;
                    images.extend(bytes.map(|b| (hash, b)));
                    analysis.waveform = Some(crate::analysis_edit::WaveSource::Staged(hash));
                }
                // Replaying, it's checked as it's made, after the edits before it.
                if !self.lenient {
                    for w in crate::analysis_edit::check(self.device, lib, track, &analysis, images)? {
                        self.note(w);
                    }
                }
                let desc = analysis.describe(track);
                return Ok((Edit::SetAnalysis { track: track.clone(), analysis }, desc));
            }
            Edit::SetPath { track, path, .. } => format!("point {} at {path}", track.paths.join(", ")),
            _ => unreachable!("only My Tag, cue, artwork, analysis and pointing edits are steps"),
        };
        Ok((edit.clone(), desc))
    }

    /// Each track's energy from rekordbox's phrase analysis (1 low, 2 mid, 3 high), where known.
    fn energy(&self, tracks: &[&Track]) -> FxHashMap<u32, f64> {
        tracks
            .iter()
            .filter_map(|t| {
                let a = crate::anlz::load(self.device, t);
                let mood = a.structure.as_ref()?.mood;
                Some((
                    t.id,
                    match mood {
                        crate::anlz::Mood::High => 3.0,
                        crate::anlz::Mood::Mid => 2.0,
                        crate::anlz::Mood::Low => 1.0,
                    },
                ))
            })
            .collect()
    }
}

fn sorted(lib: &Library, ids: &[u32], by: &str) -> Result<Vec<u32>> {
    let keys = query::parse_sort(by)?;
    let mut tracks = lib.tracks_by_id(ids);
    query::sort_tracks(&mut tracks, &keys);
    Ok(tracks.iter().map(|t| t.id).collect())
}

// ---------------------------------------------------------------------------------------------
// Plan and apply
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct EditOptions {
    /// Skip edits that no longer apply (with a warning) instead of failing: for replaying
    /// recorded edits after a new export.
    #[serde(default)]
    pub lenient: bool,
    /// Don't record the edits in the drive's journal.
    #[serde(default)]
    pub no_journal: bool,
}

/// What a track's rows say about its audio file, for a track pointed at another encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AudioFile {
    pub size: u64,
    pub sample_rate: u32,
    pub bit_depth: u16,
    /// kbps.
    pub bitrate: u32,
}

fn is_zero(n: &i32) -> bool {
    *n == 0
}

/// A playlist whose contents change.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlaylistChange {
    pub path: String,
    pub before: usize,
    pub after: usize,
    pub added: usize,
    pub removed: usize,
    /// Same tracks, different order.
    pub reordered: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct EditPlan {
    pub drive: String,
    pub root: PathBuf,
    /// One line per edit.
    pub changes: Vec<String>,
    pub playlists_created: Vec<String>,
    pub playlists_deleted: Vec<String>,
    pub playlists_changed: Vec<PlaylistChange>,
    pub tracks_changed: usize,
    pub tracks_removed: Vec<TrackRef>,
    pub history_removed: usize,
    /// Files to delete (removed tracks' audio, analysis and artwork), as drive keys.
    pub files: Vec<String>,
    pub bytes: u64,
    /// `export.pdb` and/or `exportLibrary.db`.
    pub databases: Vec<String>,
    pub needed_bytes: u64,
    pub free_space: Option<u64>,
    pub fits: bool,
    pub warnings: Vec<String>,
    /// The edits as the drive's journal records them (tracks named by file), which applying it
    /// records there, unless it was planned with `no_journal`.
    pub recorded: Vec<Edit>,
    /// OneLibrary, beside `export.pdb`, changes. The counts above are `export.pdb`'s, so they
    /// leave out edits only OneLibrary lacks, as after rekordbox exported over it alone.
    pub onelibrary_changes: bool,
    /// Applying it records `recorded` in the drive's journal (it wasn't planned with
    /// `no_journal`).
    #[serde(skip)]
    journal: bool,
    /// My Tag, cue and artwork edits to make first (see [`plan`]).
    #[serde(skip)]
    replay: Option<Box<Steps>>,
    #[serde(skip)]
    ops: Vec<Op>,
    /// Hash of the database files when planned.
    #[serde(skip)]
    state: Option<Hash>,
}

impl EditPlan {
    pub fn is_noop(&self) -> bool {
        self.playlists_created.is_empty()
            && self.playlists_deleted.is_empty()
            && self.playlists_changed.is_empty()
            && self.tracks_changed == 0
            && self.tracks_removed.is_empty()
            && self.history_removed == 0
            && !self.uncounted_ops()
            && !self.onelibrary_changes
            && self.replay.is_none()
    }

    /// Whether applying it loses something: tracks (and their files), playlists or history
    /// lists removed, a playlist left with fewer entries, cues deleted, or My Tags taken off
    /// tracks or deleted.
    pub fn destructive(&self) -> bool {
        let mut steps = self.replay.iter().flat_map(|s| &s.steps);
        !self.tracks_removed.is_empty()
            || !self.playlists_deleted.is_empty()
            || self.history_removed > 0
            || !self.files.is_empty()
            || self.playlists_changed.iter().any(|c| c.after < c.before)
            || steps.any(|e| {
                matches!(
                    e,
                    Edit::Cues { change: CueChange::Delete(_), .. }
                        | Edit::MyTag(TagChange::Remove { .. } | TagChange::Delete { .. })
                )
            })
    }

    /// Ops whose changes the counts above can miss: renames, moves, repoints (which can change
    /// only history entries or My Tag links) and new audio paths.
    fn uncounted_ops(&self) -> bool {
        self.ops
            .iter()
            .any(|op| matches!(op, Op::Rename { .. } | Op::Place { .. } | Op::Repoint { .. } | Op::SetPath { .. }))
    }
}

/// The drive's databases: `export.pdb` with its library, and OneLibrary with its library.
struct Databases {
    pdb: Option<(Vec<u8>, Database, Library)>,
    plus: Option<(Library, OneLibrary)>,
}

fn open_databases(device: &Device, warnings: &mut Vec<String>) -> Result<Databases> {
    if !device.has_export() {
        return Err(Error::NotAnExport(device.root().to_path_buf()));
    }
    let pdb = match device.has_device_library() {
        true => {
            let bytes = device.pdb_bytes()?;
            let db = pdb::parse_database(bytes.clone(), &device.path(paths::EXPORT_PDB))?;
            let lib = pdb::library_from_database(&db);
            Some((bytes, db, lib))
        }
        false => None,
    };
    let plus = crate::onelibrary::open_plus(device, pdb.is_some(), warnings)?;
    Ok(Databases { pdb, plus })
}

/// Work out what `edits` do to the drive (none: a plan that changes nothing). Its My Tag, cue,
/// artwork and analysis edits are made first, in their order, after the plan's one backup, each
/// planned in full as it's made (a replace carrying cues onto its keeper); then the others. An
/// analysis edit setting a beat grid brings the track's BPM with it (an `EditTracks` after it,
/// recorded too, so a replay doesn't add another). Planning checks the tracks they name are on
/// the drive, and applying fails on one that doesn't apply, which a replay (`lenient`) skips with
/// a warning instead.
pub fn plan(device: &Device, edits: &[Edit], opts: &EditOptions, ctx: &Ctx<'_>) -> Result<EditPlan> {
    plan_writing(device, edits, opts, &[], ctx)
}

/// [`plan`] for edits that point tracks at `incoming` audio files (keys), which the apply writes
/// (see [`apply_with`]).
pub(crate) fn plan_writing(
    device: &Device,
    edits: &[Edit],
    opts: &EditOptions,
    incoming: &[String],
    ctx: &Ctx<'_>,
) -> Result<EditPlan> {
    let mut edits = edits.to_vec();
    if !opts.lenient {
        edits = edits.into_iter().flat_map(with_tempo).collect();
    }
    let extra = DriveContext::read(device, edits.iter().flat_map(Edit::selectors))?;
    plan_with(device, &edits, opts, &extra, incoming, ctx)
}

/// `edit`, and when it sets a beat grid, the track's BPM after it: the grid's tempo.
fn with_tempo(edit: Edit) -> Vec<Edit> {
    let Edit::SetAnalysis { track, analysis } = &edit else { return vec![edit] };
    let Some(bpm) = analysis.tempo() else { return vec![edit] };
    let tracks = TrackSelector::paths([track.clone()]);
    let tempo = Edit::EditTracks { tracks, fields: TrackFields { bpm: Some(bpm), ..TrackFields::default() } };
    vec![edit, tempo]
}

impl Edit {
    /// A My Tag, cue, artwork or analysis edit: made on its own, before a plan's other edits (see
    /// [`plan`]).
    pub(crate) fn is_step(&self) -> bool {
        matches!(self, Edit::MyTag(_) | Edit::Cues { .. } | Edit::SetArtwork { .. } | Edit::SetAnalysis { .. })
    }

    /// The track selections the edit makes.
    fn selectors(&self) -> Vec<&TrackSelector> {
        match self {
            Edit::AddTracks { tracks, .. }
            | Edit::SetTracks { tracks, .. }
            | Edit::EditTracks { tracks, .. }
            | Edit::DeleteTracks { tracks, .. }
            | Edit::SetArtwork { tracks, .. } => vec![tracks],
            Edit::RemoveTracks { tracks, .. } => tracks.iter().collect(),
            Edit::PlanSet { start, .. } => start.iter().collect(),
            Edit::ReplaceTrack { duplicate, keep, .. } => vec![duplicate, keep],
            Edit::SetPath { track, .. } => vec![track],
            _ => Vec::new(),
        }
    }
}

/// Resolve `edit` against the model and make its ops there, with the edit as recorded: nothing
/// for one that changes nothing. One that fails leaves the model as it was.
fn plan_edit(
    resolver: &Resolver,
    model: &mut ModelDb<'_>,
    edit: &Edit,
    warnings: &mut Vec<String>,
) -> Result<(Vec<Op>, String, Vec<Edit>)> {
    let (edit_ops, desc, canonical) = resolver.resolve(edit, &model.lib)?;
    let before = model.lib.clone();
    for op in &edit_ops {
        if let Err(e) = apply_op(model, op, true, "plan", warnings) {
            model.lib = before;
            return Err(e);
        }
    }
    // An edit that changes nothing isn't recorded: a replay could only skip it. (A repoint can
    // move only My Tags, which the model doesn't hold.)
    let changed = model.lib != before || edit_ops.iter().any(|op| matches!(op, Op::Repoint { .. }));
    Ok((edit_ops, desc, if changed { canonical } else { Vec::new() }))
}

/// [`plan`], with what the edits' selectors need from the drive.
fn plan_with(
    device: &Device,
    edits: &[Edit],
    opts: &EditOptions,
    extra: &DriveContext,
    incoming: &[String],
    ctx: &Ctx<'_>,
) -> Result<EditPlan> {
    let mut warnings = Vec::new();
    if !opts.no_journal {
        // The journal must be one this build can add to (not a newer one).
        journal::list(device)?;
    }
    let state = Some(crate::commit::database_state(device));
    let dbs = open_databases(device, &mut warnings)?;
    let primary = match (&dbs.pdb, &dbs.plus) {
        (Some((_, _, l)), _) | (None, Some((l, _))) => l,
        (None, None) => return Err(Error::NotAnExport(device.root().to_path_buf())),
    };
    // An edit that no longer applies, replaying, is skipped with a warning. (Stopped, ordering a
    // long playlist, say, isn't one.)
    let skip = |e: Error, warnings: &mut Vec<String>| match e {
        e if opts.lenient && !matches!(e, Error::Cancelled) => {
            warnings.push(format!("skipped an edit that no longer applies: {e}"));
            Ok(())
        }
        e => Err(e),
    };
    // My Tag, cue and artwork edits are made first: they change no playlists and no track details
    // the others are planned on (artwork edits change only which image a track's row names), and
    // the others can only remove tracks they name (a tag on a duplicate replaced later then moves
    // to the keeper, as it did; a track deleted after its artwork is set takes the new images
    // along). A replay points tracks at other files with them, in the order recorded: a cue edit
    // made after it names the new file, one before it the old.
    let (given, rest): (Vec<Edit>, Vec<Edit>) =
        edits.iter().cloned().partition(|e| e.is_step() || (opts.lenient && matches!(e, Edit::SetPath { .. })));
    let mut resolver = Resolver::new(device, extra, opts.lenient, ctx.clone());
    resolver.incoming = incoming;
    if let (Some((_, _, pdb)), Some((plus, _))) = (&dbs.pdb, &dbs.plus) {
        let here: FxHashSet<String> = pdb.tracks.iter().map(|t| paths::file_key(&t.file_path)).collect();
        resolver.elsewhere =
            plus.tracks.iter().map(|t| paths::file_key(&t.file_path)).filter(|k| !here.contains(k)).collect();
    }
    let (mut steps, mut changes, mut images) = (Vec::new(), Vec::new(), FxHashMap::default());
    for step in &given {
        let made = resolver.step(step, primary, &mut images);
        warnings.extend(std::mem::take(&mut *resolver.notes.borrow_mut()));
        match made {
            Ok((step, desc)) => {
                steps.push(step);
                changes.push(desc);
            }
            Err(e) => skip(e, &mut warnings)?,
        }
    }
    let mut model = ModelDb { lib: primary.clone(), pdb: dbs.pdb.as_ref().map(|(_, db, _)| db) };
    let mut ops = Vec::new();
    let mut recorded = Vec::new();
    // Tracks deleted by an edit that keeps their files (the others' files go).
    let mut kept: FxHashSet<String> = FxHashSet::default();
    let mut index = None;
    for edit in &rest {
        ctx.check()?;
        let resolved = track_index::plan_edit_indexed(&resolver, &mut model, &mut index, edit, &mut warnings);
        let notes = std::mem::take(&mut *resolver.notes.borrow_mut());
        match resolved {
            Ok((edit_ops, desc, canonical)) => {
                warnings.extend(notes);
                if let Edit::DeleteTracks { keep_files: true, .. } = edit {
                    for op in &edit_ops {
                        if let Op::DeleteTracks { tracks, elsewhere } = op {
                            kept.extend(tracks.iter().chain(elsewhere).cloned());
                        }
                    }
                }
                ops.extend(edit_ops);
                changes.push(desc);
                recorded.extend(canonical);
            }
            Err(e) => skip(e, &mut warnings)?,
        }
    }
    // The second database, as far as a model can tell (its own warnings). It can lack edits the
    // first has (rekordbox exported over it alone), which then change only it.
    let mut onelibrary_changes = false;
    if let (Some(_), Some((plus_lib, _))) = (&dbs.pdb, &dbs.plus) {
        let mut second = ModelDb { lib: plus_lib.clone(), pdb: None };
        let mut index = None;
        for op in &ops {
            track_index::apply_op_indexed(&mut second, &mut index, op, false, "exportLibrary.db", &mut warnings)?;
        }
        onelibrary_changes = second.lib != *plus_lib;
    }

    // What changes, on the main database.
    let after = &model.lib;
    let deletes_elsewhere =
        ops.iter().any(|op| matches!(op, Op::DeleteTracks { elsewhere, .. } if !elsewhere.is_empty()));
    if onelibrary_changes && after == primary && !deletes_elsewhere {
        changes.push("make them in exportLibrary.db, which lacks them".into());
    }
    let diff = LibraryDiff::between(primary, after);
    // Playlists matched by key, as the diff matches them: same-named siblings share a path, and
    // each reports its own numbers.
    let before: FxHashMap<PlaylistKey, &Playlist> = primary.keyed_playlists().map(|(k, _, p)| (k, p)).collect();
    let playlists_changed = after
        .keyed_playlists()
        .filter(|(_, _, p)| !p.is_folder)
        .filter_map(|(key, path, p)| {
            let (old, new) = (&before.get(&key)?.track_ids, &p.track_ids);
            if old == new {
                return None;
            }
            let (o, n): (FxHashSet<u32>, FxHashSet<u32>) =
                (old.iter().copied().collect(), new.iter().copied().collect());
            let mut o_sorted = old.clone();
            let mut n_sorted = new.clone();
            o_sorted.sort_unstable();
            n_sorted.sort_unstable();
            let change = PlaylistChange {
                path,
                before: old.len(),
                after: new.len(),
                added: n.difference(&o).count(),
                removed: o.difference(&n).count(),
                reordered: o_sorted == n_sorted,
            };
            Some((key, change))
        })
        .collect::<IndexMap<_, _>>()
        .sorted_unstable_by(|a, _, b, _| a.cmp(b))
        .map(|(_, change)| change)
        .collect();
    let doomed: FxHashSet<String> = ops
        .iter()
        .filter_map(|op| match op {
            Op::DeleteTracks { tracks, elsewhere } => Some(tracks.iter().chain(elsewhere).cloned()),
            _ => None,
        })
        .flatten()
        .collect();
    // The main database's, then those only the other lists.
    let only_elsewhere = (dbs.plus.iter().filter(|_| dbs.pdb.is_some()))
        .flat_map(|(l, _)| &l.tracks)
        .filter(|t| resolver.elsewhere.contains(&paths::file_key(&t.file_path)));
    let tracks_removed: Vec<TrackRef> = (primary.tracks.iter().chain(only_elsewhere))
        .filter(|t| doomed.contains(&paths::file_key(&t.file_path)))
        .map(TrackRef::from)
        .collect();
    let edited: FxHashSet<String> = ops
        .iter()
        .filter_map(|op| match op {
            Op::SetFields { tracks, .. } => Some(tracks.iter().cloned()),
            _ => None,
        })
        .flatten()
        .collect();
    // Only tracks whose fields end up different: replaying an edit that's still in place, or
    // setting what a track already holds, changes nothing.
    let was: FxHashMap<u32, &Track> = primary.tracks.iter().map(|t| (t.id, t)).collect();
    let tracks_changed = after
        .tracks
        .iter()
        .filter(|t| edited.contains(&paths::file_key(&t.file_path)) && was.get(&t.id).is_some_and(|w| *w != *t))
        .count();
    let history_removed =
        primary.history.iter().filter(|h| !h.is_folder).count() - after.history.iter().filter(|h| !h.is_folder).count();

    // Files the removed tracks leave unused.
    let mut files = IndexMap::new();
    if doomed.iter().any(|k| !kept.contains(k)) {
        let libraries: Vec<&Library> =
            dbs.pdb.iter().map(|(_, _, l)| l).chain(dbs.plus.iter().map(|(l, _)| l)).collect();
        let (removable, left) = crate::track_files::removable_files(
            device,
            &libraries,
            dbs.plus.as_ref().map(|(_, o)| o),
            &|t: &Track| {
                let k = paths::file_key(&t.file_path);
                doomed.contains(&k) && !kept.contains(&k)
            },
            ctx,
        )?;
        files = removable;
        if let Some(example) = left.values().next() {
            warnings.push(format!(
                "{} audio file(s) outside Contents/ are left on the drive (e.g. {example}); locallibrary never deletes files there",
                left.len()
            ));
        }
    }
    let playlist_ops =
        ops.iter().any(|op| !matches!(op, Op::SetFields { .. } | Op::SetPath { .. } | Op::DeleteHistory { .. }));
    if playlist_ops {
        warnings
            .extend(device.rekordbox_sync_warning("its next sync may undo these changes to the playlists it syncs"));
    }

    let mut databases = Vec::new();
    if dbs.pdb.is_some() {
        databases.push(paths::EXPORT_PDB.rsplit('/').next().unwrap().to_string());
    }
    if dbs.plus.is_some() {
        databases.push(paths::LIBRARY_PLUS_DB.rsplit('/').next().unwrap().to_string());
    }
    // My Tag edits a replay makes as it's applied write exportExt.pdb too.
    if steps.iter().any(|s| matches!(s, Edit::MyTag(_))) && device.path(paths::EXPORT_EXT_PDB).exists() {
        databases.push(paths::EXPORT_EXT_PDB.rsplit('/').next().unwrap().to_string());
    }
    let cluster = fsutil::cluster_size(device.root());
    let round = |n: u64| n.div_ceil(cluster) * cluster;
    let size = |key| std::fs::metadata(device.path(key)).map(|m| m.len()).unwrap_or(0);
    let db_bytes: u64 = paths::DATABASE_FILES.iter().map(|k| round(size(k))).sum();
    let bytes: u64 = files.values().sum();
    // Backup of the databases and deleted files, plus each new database staged next to the old,
    // and each image artwork edits stage, with the two sizes made from it in each database (no
    // bigger than it; a staged waveform, which no database holds, is counted as one too).
    let staged = images.values().map(|b| round(b.len() as u64)).sum::<u64>() * (1 + 2 * databases.len() as u64);
    let needed_bytes = files.values().map(|&n| round(n)).sum::<u64>() + 2 * db_bytes + staged;
    let free_space = fsutil::available_space(device.root());
    let fits = free_space.is_none_or(|f| needed_bytes <= f);
    if !fits {
        warnings.push(format!(
            "the backup and new databases need about {needed_bytes} bytes but only {} are free on the drive",
            free_space.unwrap_or(0)
        ));
    }
    let created: Vec<String> = diff.added_playlists.clone();
    let deleted: Vec<String> = diff.removed_playlists.clone();
    Ok(EditPlan {
        drive: device.name(),
        root: device.root().to_path_buf(),
        changes,
        playlists_created: created,
        playlists_deleted: deleted,
        playlists_changed,
        tracks_changed,
        tracks_removed,
        history_removed,
        files: files.keys().cloned().collect(),
        bytes,
        databases,
        needed_bytes,
        free_space,
        fits,
        warnings: dedup(warnings),
        // My Tag, cue and artwork edits are recorded first, as they're made.
        recorded: steps.iter().cloned().chain(recorded).collect(),
        onelibrary_changes,
        journal: !opts.no_journal,
        replay: (!steps.is_empty()).then(|| Box::new(Steps { steps, rest, opts: opts.clone(), images })),
        ops,
        state,
    })
}

fn dedup(v: Vec<String>) -> Vec<String> {
    let mut seen = FxHashSet::default();
    v.into_iter().filter(|w| seen.insert(w.clone())).collect()
}

/// The parts of a library an edit can change, comparable across databases (tracks by file,
/// playlists and folders by key, so same-named siblings stay apart).
#[derive(Debug, PartialEq)]
struct Shape<'a> {
    tracks: IndexMap<String, Track>,
    /// Path and entries.
    playlists: IndexMap<PlaylistKey<'a>, (String, Vec<String>)>,
    /// The names and kinds of each folder's children in order (the top level's under an empty
    /// key).
    order: IndexMap<PlaylistKey<'a>, Vec<(&'a str, bool)>>,
    history: Vec<(String, Vec<String>)>,
}

fn shape(lib: &Library) -> Shape<'_> {
    let keys: FxHashMap<u32, String> = lib.tracks.iter().map(|t| (t.id, paths::file_key(&t.file_path))).collect();
    let entries = |ids: &[u32]| ids.iter().map(|id| keys.get(id).cloned().unwrap_or_default()).collect::<Vec<_>>();
    let kids = |parent| lib.children(parent).into_iter().map(|p| (p.name.as_str(), p.is_folder)).collect();
    let mut order = IndexMap::from([(Vec::new(), kids(None))]);
    let mut playlists = IndexMap::new();
    for (key, path, p) in lib.keyed_playlists() {
        if p.is_folder {
            order.insert(key.clone(), kids(Some(p.id)));
        }
        playlists.insert(key, (path, entries(&p.track_ids)));
    }
    let mut history: Vec<&Playlist> = lib.history.iter().filter(|h| !h.is_folder).collect();
    history.sort_by_key(|h| (h.sort_order, h.id));
    Shape {
        tracks: lib.tracks.iter().map(|t| (paths::file_key(&t.file_path), Track { id: 0, ..t.clone() })).collect(),
        playlists,
        order,
        history: history.iter().map(|h| (h.name.clone(), entries(&h.track_ids))).collect(),
    }
}

/// Check an edited database reads back as its model says. Returns a short description of the
/// first difference.
fn verify(expected: &Library, actual: &Library) -> std::result::Result<(), String> {
    let (e, a) = (shape(expected), shape(actual));
    if e == a {
        return Ok(());
    }
    if let Some((k, t)) = e.tracks.iter().find(|(k, t)| a.tracks.get(*k) != Some(t)) {
        return Err(format!("track {k}: expected {t:?}, read back {:?}", a.tracks.get(k)));
    }
    if e.tracks.len() != a.tracks.len() {
        return Err(format!("{} tracks expected, {} read back", e.tracks.len(), a.tracks.len()));
    }
    if let Some((k, (path, p))) = e.playlists.iter().find(|(k, p)| a.playlists.get(*k) != Some(p)) {
        return Err(format!("playlist {path}: expected {p:?}, read back {:?}", a.playlists.get(k).map(|(_, p)| p)));
    }
    if e.playlists.len() != a.playlists.len() {
        return Err(format!("{} playlists expected, {} read back", e.playlists.len(), a.playlists.len()));
    }
    if e.order != a.order {
        return Err("playlist order differs".into());
    }
    match e.history.iter().zip(&a.history).find(|(x, y)| x != y) {
        Some(((name, entries), (_, back))) => {
            Err(format!("history list {name:?}: expected {entries:?}, read back {back:?} (or another list)"))
        }
        None => Err(format!("{} history lists expected, {} read back", e.history.len(), a.history.len())),
    }
}

/// The analysis files (`.DAT`, `.EXT`, `.2EX`) at `analyze_path` (a device path) that record an
/// audio file, recording `path` instead, with their beat grid and cues `shift_ms` later. One that
/// records `path` already is left as it is (its times moved with it), so a replay moves them
/// once; one in a format this can't rewrite is left as it is, with a warning.
pub(crate) fn analysis_pointed_at(
    device: &Device,
    analyze_path: &str,
    path: &str,
    shift_ms: i32,
    warnings: &mut Vec<String>,
) -> Result<Vec<NewFile<'static>>> {
    let dat = paths::key(analyze_path);
    let Some(stem) = dat.strip_suffix(".DAT") else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for key in ["DAT", "EXT", "2EX"].map(|ext| format!("{stem}.{ext}")) {
        if !paths::is_safe_key(&key) || paths::classify(&key) != Some(paths::FileClass::Analysis) {
            continue;
        }
        let file = device.checked_path(&key)?;
        let bytes = match std::fs::read(&file) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(Error::Io { path: file, source: e }),
        };
        if crate::anlz::read_path(&bytes).is_ok_and(|p| p.as_deref() == Some(path)) {
            continue;
        }
        let rewritten = crate::anlz::rewrite_path(&bytes, path).and_then(|b| crate::anlz::shift_times(&b, shift_ms));
        match rewritten {
            Ok(new) if new != bytes => out.push(NewFile::bytes(key, new)),
            Ok(_) => {}
            Err(_) => warnings.push(format!("{key}: unrecognised analysis format, left as it was")),
        }
    }
    Ok(out)
}

/// `exportExt.pdb` once export.pdb tracks were removed or replaced: My Tag links follow them
/// (they name export.pdb track ids, and a removed id gets reused by the next track added).
/// `None` when nothing needs changing or the drive has no such file.
pub(crate) fn ext_after(device: &Device, deleted: &[u32], repointed: &[(u32, u32)]) -> Result<Option<Vec<u8>>> {
    if deleted.is_empty() && repointed.is_empty() {
        return Ok(None);
    }
    let path = device.path(paths::EXPORT_EXT_PDB);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::Io { path, source: e }),
    };
    crate::tags::guard_ext(|| {
        let mut ed = crate::tags::ext_editor(bytes.clone())?;
        let db = rekordbox_pdb::ExtDatabase::from_bytes(bytes)?;
        if !db.has_table(rekordbox_pdb::ExtTableType::TagTracks) {
            return Ok(None);
        }
        let mut changed = false;
        for (from, to) in repointed {
            for link in db.tag_tracks.iter().filter(|l| l.track_id == *from) {
                changed |= ed.link(link.tag_id, *to)?;
            }
        }
        changed |= ed.unlink_tracks(deleted)? > 0;
        if !changed {
            return Ok(None);
        }
        let out = ed.to_bytes()?;
        // Read back with the checks its input passed.
        crate::tags::ext_editor(out.clone())?;
        Ok(Some(out))
    })
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct EditReport {
    pub changes: usize,
    pub databases: Vec<String>,
    pub files_deleted: usize,
    pub freed_bytes: u64,
    pub backup: Option<String>,
    pub fingerprint: Option<Hash>,
    pub warnings: Vec<String>,
}

/// What making one My Tag, cue or artwork edit of a plan came to (see [`crate::tags::replay`],
/// [`crate::cue_edit::replay`], [`crate::artwork::replay`]). Replaying, one that no longer
/// applies only has a warning.
#[derive(Debug, Default)]
pub(crate) struct Replayed {
    /// The files written.
    pub databases: Vec<String>,
    pub fingerprint: Option<Hash>,
    pub warnings: Vec<String>,
}

impl Replayed {
    /// `gone` of the tracks the edit (`described`) names, `example` one of them, aren't on the
    /// drive: an error unless replaying (`strict`: making edits as given), when it's made without
    /// them.
    pub(crate) fn tracks_gone(&mut self, strict: bool, gone: usize, example: &str, described: &str) -> Result<()> {
        if strict {
            return Err(Error::NotFound(format!("no track at {example}")));
        }
        self.warnings.push(format!(
            "{gone} track(s) aren't on the drive any more (e.g. {example}), so \"{described}\" is replayed without them"
        ));
        Ok(())
    }

    /// The edit (`described`) doesn't apply (`e`): an error unless replaying, when it's skipped.
    pub(crate) fn skipped(mut self, strict: bool, described: &str, e: Error) -> Result<Replayed> {
        if strict {
            return Err(e);
        }
        self.warnings.push(format!("skipped an edit that no longer applies ({described}): {e}"));
        Ok(self)
    }
}

pub fn apply(plan: &EditPlan, device: &mut Device, ctx: &Ctx<'_>) -> Result<EditReport> {
    apply_with(plan, device, Vec::new(), ctx)
}

/// [`apply`], writing `files` (audio a relocation copies, say) along with the databases, under
/// the same backup. A plan with My Tag, cue or artwork edits takes none.
pub(crate) fn apply_with(
    plan: &EditPlan,
    device: &mut Device,
    files: Vec<NewFile<'_>>,
    ctx: &Ctx<'_>,
) -> Result<EditReport> {
    if plan.is_noop() && files.is_empty() {
        return Ok(EditReport::default());
    }
    if !plan.fits {
        return Err(Error::NoSpace {
            path: plan.root.clone(),
            needed: plan.needed_bytes,
            available: plan.free_space.unwrap_or(0),
        });
    }
    let _lock = crate::commit::lock(device, plan.state)?;
    match &plan.replay {
        Some(steps) => replay(plan, steps, device, ctx),
        None if plan.journal && !plan.recorded.is_empty() => {
            journal::list(device)?; // one this build can add to
            apply_locked(plan, device, true, files, ctx)
        }
        None => apply_locked(plan, device, true, files, ctx),
    }
}

/// A plan's My Tag, cue and artwork edits (and a replay's pointing edits), and the others.
#[derive(Debug, Clone)]
struct Steps {
    /// Made before the others in their order, each planned as it's made (replaying, one that no
    /// longer applies is skipped then).
    steps: Vec<Edit>,
    /// The other edits, planned again once those are made.
    rest: Vec<Edit>,
    opts: EditOptions,
    /// The images artwork edits and the waveforms analysis edits stage on the drive, by hash
    /// (those given staged are on it).
    images: FxHashMap<Hash, Vec<u8>>,
}

/// A plan with steps (see [`Steps`]): one backup first (the databases, the analysis files cue
/// edits may rewrite, the images artwork edits may replace and the files removed tracks leave),
/// so `restore` undoes all of it; then each step, planned as it's made; then the others, planned
/// again. The backup notes what the plan left, all of it made or not.
fn replay(plan: &EditPlan, steps: &Steps, device: &mut Device, ctx: &Ctx<'_>) -> Result<EditReport> {
    let library = device.library()?;
    let mut extra = plan.files.clone();
    for step in &steps.steps {
        // The analysis files the step rewrites (a cue edit's, and a pointed track's), and the
        // images an artwork edit replaces in place.
        match step {
            Edit::Cues { tracks, .. } => extra.extend(crate::cue_edit::analysis_files(device, &library, tracks)),
            Edit::SetPath { track, .. } => {
                extra.extend(crate::cue_edit::analysis_files(device, &library, &track.paths))
            }
            Edit::SetArtwork { tracks, .. } => extra.extend(crate::artwork::image_files(device, &tracks.paths)?),
            Edit::SetAnalysis { track, .. } => {
                extra.extend(crate::analysis_edit::analysis_files(device, &library, track))
            }
            _ => {}
        }
    }
    extra.sort();
    extra.dedup();
    // A plan of new edits (a replace carrying cues) is an edit like any other to undo.
    let label = if steps.opts.no_journal { "before replaying edits" } else { "before editing" };
    let backup = backup::create(device, label, &extra, None)?;
    // Changes are counted as they're made: a recorded edit that no longer applies writes nothing.
    let mut report = EditReport { backup: Some(backup.id.clone()), ..Default::default() };
    let swaps = device.swaps();
    let mut made = (Vec::new(), Vec::new());
    let replayed = replay_steps(steps, &plan.changes, &extra, device, &mut report, &mut made, ctx);
    // What it made is recorded as one entry under the one backup, also when a later edit failed,
    // so a replay after rekordbox exports makes that again.
    let recorded = match steps.opts.no_journal || made.0.is_empty() {
        true => Ok(()),
        false => journal::append(device, &made.0, &made.1, Some(&backup.id)),
    };
    // Having changed nothing, it leaves no backup (a backup it can't take away stays, said so).
    if replayed.is_err() && device.swaps() == swaps {
        if let Err(e) = backup::discard(device, &backup.id) {
            ctx.warn(format!("backup {} stays on the drive: {e}", backup.id));
        }
        return replayed.map(|_| report);
    }
    let sealed = backup::seal(device, &backup.id).and_then(|_| backup::prune(device));
    replayed?;
    recorded?;
    sealed?;
    report.databases = dedup(report.databases);
    report.warnings = dedup(report.warnings);
    Ok(report)
}

/// [`replay`]'s edits, made in turn (`changes`: the plan's, the steps' first), adding what they
/// made to `made`: the edits as recorded, and what they did. An edit that failed once it changed
/// the drive is made too.
fn replay_steps(
    steps: &Steps,
    changes: &[String],
    extra: &[String],
    device: &mut Device,
    report: &mut EditReport,
    made: &mut (Vec<Edit>, Vec<String>),
    ctx: &Ctx<'_>,
) -> Result<()> {
    let (name, drive) = (device.name(), device.root().display().to_string());
    let (what, undo) = if steps.opts.lenient { ("replay", "the replay") } else { ("plan", "the edits") };
    // Undoing takes back what it wrote; having written nothing, it leaves no backup (see `replay`).
    let stopped = |e: Error, done: usize, written: bool| {
        let stopped = format!("the {what} stopped after {done} of its edits: {e}");
        let undo = Action::call("undo.plan", serde_json::json!({ "drive": drive }), format!("Undo {undo} on {name}"));
        match written {
            false => Error::conflict(Reason::ReplayStopped, format!("{stopped}; it changed nothing")),
            true => Error::conflict(Reason::ReplayStopped, format!("{stopped}. Undoing takes back what it made"))
                .action(undo),
        }
        .with("drive", &drive)
    };
    let (strict, start) = (!steps.opts.lenient, device.swaps());
    // Files the steps wrote, which the other edits may then delete (a new image of a track they
    // delete), though the backup made before them has none of them.
    let mut written = Vec::new();
    for (done, step) in steps.steps.iter().enumerate() {
        ctx.check()?;
        let before = device.swaps();
        let replayed = match step {
            Edit::MyTag(change) => crate::tags::replay(device, change, strict, ctx),
            Edit::Cues { tracks, change } => crate::cue_edit::replay(device, tracks, change, strict, ctx),
            Edit::SetArtwork { tracks, image: ImageSource::Staged(hash) } => {
                crate::artwork::replay(device, &tracks.paths, hash, &steps.images, strict, ctx)
            }
            Edit::SetAnalysis { track, analysis } => {
                crate::analysis_edit::replay(device, track, analysis, &steps.images, strict, ctx)
            }
            Edit::SetPath { .. } => replay_set_path(device, step, ctx),
            _ => unreachable!("only My Tag, cue, artwork (staged), analysis and pointing edits are steps"),
        };
        if replayed.is_ok() || device.swaps() != before {
            made.0.push(step.clone());
            made.1.push(changes[done].clone());
        }
        let replayed = replayed.map_err(|e| stopped(e, done, device.swaps() != start))?;
        report.changes += usize::from(!replayed.databases.is_empty());
        written.extend(replayed.databases.iter().cloned());
        // The databases among the files written (cue and artwork edits write analysis files and
        // images too).
        let database = |f: &String| paths::DATABASE_FILES.iter().any(|k| file_name(k) == f);
        report.databases.extend(replayed.databases.into_iter().filter(database));
        report.fingerprint = replayed.fingerprint.or(report.fingerprint);
        for w in &replayed.warnings {
            ctx.warn(w.clone());
        }
        report.warnings.extend(replayed.warnings);
    }
    if steps.rest.is_empty() {
        return Ok(());
    }
    let done = steps.steps.len();
    let mut again = crate::edit::plan(device, &steps.rest, &steps.opts, ctx)
        .map_err(|e| stopped(e, done, device.swaps() != start))?;
    if again.files.iter().any(|f| !extra.contains(f) && !written.contains(f)) {
        let changed = Error::conflict(Reason::Changed, "the drive changed while replaying");
        return Err(stopped(changed, done, device.swaps() != start));
    }
    if again.is_noop() {
        return Ok(());
    }
    // Recorded with the steps (see `replay`), not on their own.
    let recorded = std::mem::take(&mut again.recorded);
    let before = device.swaps();
    let applied = apply_locked(&again, device, false, Vec::new(), ctx);
    if applied.is_ok() || device.swaps() != before {
        made.0.extend(recorded);
        made.1.extend(again.changes.iter().cloned());
    }
    let r = applied.map_err(|e| stopped(e, done, device.swaps() != start))?;
    report.changes += r.changes;
    report.databases.extend(r.databases);
    report.files_deleted = r.files_deleted;
    report.freed_bytes = r.freed_bytes;
    report.fingerprint = r.fingerprint.or(report.fingerprint);
    report.warnings.extend(r.warnings);
    Ok(())
}

/// One recorded pointing of a track at another file, made in a replay (see [`replay`]): skipped,
/// with a warning, when it no longer applies (rekordbox's export no longer has the old file).
fn replay_set_path(device: &mut Device, step: &Edit, ctx: &Ctx<'_>) -> Result<Replayed> {
    let opts = EditOptions { lenient: false, no_journal: true };
    let plan = match plan(device, std::slice::from_ref(step), &opts, ctx) {
        Ok(plan) => plan,
        Err(e) if matches!(e.inner(), Error::Cancelled) => return Err(e),
        Err(e) => {
            return Ok(Replayed { warnings: vec![format!("a track's new file: {e}; skipped")], ..Default::default() });
        }
    };
    if plan.is_noop() {
        return Ok(Replayed::default());
    }
    let r = apply_locked(&plan, device, false, Vec::new(), ctx)?;
    Ok(Replayed { databases: r.databases, fingerprint: r.fingerprint, warnings: r.warnings })
}

/// [`apply_with`] with the drive locked already, backing it up first only with `backup`.
fn apply_locked(
    plan: &EditPlan,
    device: &mut Device,
    backup: bool,
    mut files: Vec<NewFile<'_>>,
    ctx: &Ctx<'_>,
) -> Result<EditReport> {
    let mut warnings = Vec::new();
    // A plan of no edits only writes `files`.
    let dbs = match plan.ops.is_empty() {
        true => Databases { pdb: None, plus: None },
        false => open_databases(device, &mut warnings)?,
    };
    ctx.check()?;
    // Analysis files record the audio file they belong to: a track pointed at another records it.
    let primary = dbs.pdb.as_ref().map(|(_, _, l)| l).or(dbs.plus.as_ref().map(|(l, _)| l));
    for op in &plan.ops {
        // The row the databases' edit takes: the last of the file's (see `apply_op`).
        if let Op::SetPath { track, path, shift_ms, .. } = op
            && let Some(t) =
                primary.and_then(|l| l.tracks.iter().rev().find(|t| paths::file_key(&t.file_path) == *track))
        {
            let mut unreadable = Vec::new();
            files.extend(analysis_pointed_at(device, &t.analyze_path, path, *shift_ms, &mut unreadable)?);
            // Cues move in the analysis files and OneLibrary together, or not at all.
            if *shift_ms != 0 && !unreadable.is_empty() {
                return Err(Error::invalid(format!(
                    "{}: its cues can't be moved: {}",
                    t.display_name(),
                    unreadable.join("; ")
                )));
            }
            warnings.extend(unreadable);
        }
    }

    // 1. Databases, both edited and verified in memory before anything on the drive changes (an
    // edit a database can't take fails here, leaving no backup and no interrupted operation).
    ctx.phase_start(Phase::Write, plan.databases.len() as u64, 0);
    let has_pdb = dbs.pdb.is_some();
    let mut ext_bytes = None;
    let pdb_bytes = match dbs.pdb {
        None => None,
        Some((bytes, original, lib)) => {
            let mut db = PdbDb {
                editor: PdbEditor::from_bytes(bytes),
                lib,
                deleted: Vec::new(),
                repointed: Vec::new(),
                candidates: Vec::new(),
                original,
            };
            let bytes = pdb::guard_editor(|| {
                for op in &plan.ops {
                    apply_op(&mut db, op, true, "export.pdb", &mut warnings)?;
                }
                db.prune()?;
                Ok(db.editor.to_bytes())
            })?;
            let check = pdb::parse_database(bytes.clone(), &device.path(paths::EXPORT_PDB))?;
            if let Err(e) = verify(&db.lib, &pdb::library_from_database(&check)) {
                return Err(Error::Pdb(format!("the edited database failed verification ({e}); drive left unchanged")));
            }
            ext_bytes = match ext_after(device, &db.deleted, &db.repointed) {
                Ok(ext) => ext,
                Err(e) => {
                    warnings.push(format!("My Tags in exportExt.pdb weren't updated: {e}"));
                    None
                }
            };
            Some(bytes)
        }
    };
    let plus_bytes = match dbs.plus {
        None => None,
        Some((lib, one)) => {
            let mut db = PlusDb { one, lib };
            for op in &plan.ops {
                apply_op(&mut db, op, !has_pdb, "exportLibrary.db", &mut warnings)?;
            }
            let bytes = db.one.to_encrypted()?;
            let check = OneLibrary::from_encrypted(&bytes, None)?.library()?;
            if let Err(e) = verify(&db.lib, &check) {
                return Err(Error::OneLibrary(format!(
                    "the edited database failed verification ({e}); drive left unchanged"
                )));
            }
            Some(bytes)
        }
    };
    // 2. Back up, swap the databases in, then delete the removed tracks' files (nothing references
    // them any more), record the new state and the edits for replaying.
    ctx.phase_end(Phase::Write);
    let changes = Changes {
        databases: NewDatabases { pdb: pdb_bytes, ext: ext_bytes, plus: plus_bytes },
        files,
        deletes: plan.files.clone(),
        remove_first: Vec::new(),
    };
    let description = plan.changes.first().cloned().unwrap_or_else(|| "edit".into());
    let mut opts = Options::new(LineageOp::Edit, description);
    if backup {
        opts = opts.backup("before editing");
    }
    opts.journal = plan.journal.then_some((&plan.recorded, &plan.changes));
    let done = crate::commit::commit(device, changes, opts, ctx)?;
    for w in &warnings {
        ctx.warn(w.clone());
    }
    Ok(EditReport {
        changes: plan.changes.len(),
        databases: done.databases,
        files_deleted: done.deleted,
        freed_bytes: done.freed,
        backup: done.backup,
        fingerprint: done.fingerprint,
        warnings: dedup(warnings),
    })
}

// ---------------------------------------------------------------------------------------------
// Subsets (for packing a drive for a set)
// ---------------------------------------------------------------------------------------------

/// A drive's databases cut down to some of its playlists (see [`crate::pack`]).
pub(crate) struct Subset {
    pub pdb: Option<Vec<u8>>,
    /// `exportExt.pdb` without the My Tag links of tracks left out (`None`: copy it as it is).
    pub ext: Option<Vec<u8>>,
    pub plus: Option<Vec<u8>>,
    /// The main database's library after the cut.
    pub library: Library,
    /// The other database's library after the cut, if the drive has both.
    pub second: Option<Library>,
    pub warnings: Vec<String>,
}

/// The source drive's databases holding only `playlists` (paths of playlists or folders; a
/// folder brings everything in it), the folders above them and the tracks they use, plus the
/// history lists when `history`. Nothing on the drive changes.
/// The playlists and folders `specs` name in `lib`, with everything below them and the folders
/// above them.
pub(crate) fn chosen_nodes(lib: &Library, specs: &[String]) -> Result<FxHashSet<u32>> {
    let mut keep: FxHashSet<u32> = FxHashSet::default();
    for spec in specs {
        let p = playlist(lib, spec)?;
        let mut queue = vec![p.id];
        while let Some(id) = queue.pop() {
            if keep.insert(id) {
                queue.extend(lib.children(Some(id)).iter().map(|c| c.id));
            }
        }
        let mut up = p.parent_id;
        while let Some(id) = up.filter(|id| keep.insert(*id)) {
            up = lib.playlist(id).and_then(|q| q.parent_id);
        }
    }
    Ok(keep)
}

pub(crate) fn subset(device: &Device, playlists: &[String], history: bool, ctx: &Ctx<'_>) -> Result<Subset> {
    let mut warnings = Vec::new();
    let dbs = open_databases(device, &mut warnings)?;
    let primary = match (&dbs.pdb, &dbs.plus) {
        (Some((_, _, l)), _) | (None, Some((l, _))) => l.clone(),
        (None, None) => return Err(Error::NotAnExport(device.root().to_path_buf())),
    };
    if playlists.is_empty() {
        return Err(Error::invalid("choose at least one playlist or folder to pack"));
    }
    let keep = chosen_nodes(&primary, playlists)?;
    let mut ops = Vec::new();
    // Whole subtrees that go: a dropped node whose parent stays (or that is top level).
    for p in primary.playlists.iter().filter(|p| !keep.contains(&p.id)) {
        if p.parent_id.is_none_or(|parent| keep.contains(&parent)) {
            ops.push(Op::Delete { node: primary.playlist_names(p.id), folder: p.is_folder });
        }
    }
    let used: FxHashSet<u32> = primary
        .playlists
        .iter()
        .filter(|p| keep.contains(&p.id))
        .chain(primary.history.iter().filter(|_| history))
        .flat_map(|p| p.track_ids.iter().copied())
        .collect();
    // A track goes with every row of its file: not one whose file a kept track shares.
    let (kept, unused): (Vec<&Track>, Vec<&Track>) = primary.tracks.iter().partition(|t| used.contains(&t.id));
    let kept: FxHashSet<String> = kept.iter().map(|t| paths::file_key(&t.file_path)).collect();
    let dropped: Vec<String> =
        unused.iter().map(|t| paths::file_key(&t.file_path)).filter(|k| !kept.contains(k)).collect();
    if !dropped.is_empty() {
        ops.push(Op::DeleteTracks { tracks: dropped, elsewhere: Vec::new() });
    }
    if !history {
        let mut counts: FxHashMap<&str, usize> = FxHashMap::default();
        let lists: Vec<(String, usize)> = primary
            .history
            .iter()
            .filter(|h| !h.is_folder)
            .map(|h| {
                let n = counts.entry(h.name.as_str()).or_default();
                *n += 1;
                (h.name.clone(), *n - 1)
            })
            .collect();
        if !lists.is_empty() {
            ops.push(Op::DeleteHistory { lists });
        }
    }
    ctx.check()?;
    let mut model = ModelDb { lib: primary, pdb: None };
    for op in &ops {
        apply_op(&mut model, op, true, "plan", &mut warnings)?;
    }
    let (pdb, ext, plus, second) = edit_in_memory(device, dbs, &ops, true, &mut warnings)?;
    Ok(Subset { pdb, ext, plus, library: model.lib, second, warnings: dedup(warnings) })
}

/// A track pointed at a copy of its audio (see [`repoint_in_memory`]).
#[derive(Debug, Clone)]
pub(crate) struct Move {
    /// The track's audio file, a lower-cased key.
    pub track: String,
    /// The copy's device path.
    pub path: String,
    pub file: AudioFile,
    /// How much later the copy's times are.
    pub shift_ms: i32,
}

/// Databases a pack wrote (`pdb` and `plus`, see [`subset`]) with tracks pointed at the copies
/// made for them. A track one database lacks is left out of it. Returns the new `export.pdb` and
/// `exportLibrary.db`.
#[allow(clippy::type_complexity)]
pub(crate) fn repoint_in_memory(
    device: &Device,
    pdb: Option<&[u8]>,
    plus: Option<&[u8]>,
    moves: &[Move],
) -> Result<(Option<Vec<u8>>, Option<Vec<u8>>)> {
    let pdb = match pdb {
        Some(bytes) => {
            let db = pdb::parse_database(bytes.to_vec(), &device.path(paths::EXPORT_PDB))?;
            let lib = pdb::library_from_database(&db);
            Some((bytes.to_vec(), db, lib))
        }
        None => None,
    };
    let plus = match plus {
        Some(bytes) => {
            let one = OneLibrary::from_encrypted(bytes, None)?;
            Some((one.library()?, one))
        }
        None => None,
    };
    let ops: Vec<Op> = moves
        .iter()
        .map(|m| Op::SetPath { track: m.track.clone(), path: m.path.clone(), file: Some(m.file), shift_ms: m.shift_ms })
        .collect();
    let (pdb, _, plus, _) = edit_in_memory(device, Databases { pdb, plus }, &ops, false, &mut Vec::new())?;
    Ok((pdb, plus))
}

/// `dbs` with `ops` made, verified against their models, as bytes: `export.pdb`, the drive's
/// `exportExt.pdb` when its My Tag links changed, `exportLibrary.db`, and OneLibrary's library
/// when the drive has both. With `strict`, anything an op names that the main database lacks is
/// an error (the other skips it). Nothing on the drive changes.
#[allow(clippy::type_complexity)]
fn edit_in_memory(
    device: &Device,
    dbs: Databases,
    ops: &[Op],
    strict: bool,
    warnings: &mut Vec<String>,
) -> Result<(Option<Vec<u8>>, Option<Vec<u8>>, Option<Vec<u8>>, Option<Library>)> {
    let has_pdb = dbs.pdb.is_some();
    let mut ext = None;
    let pdb = match dbs.pdb {
        None => None,
        Some((bytes, original, lib)) => {
            let mut db = PdbDb {
                editor: PdbEditor::from_bytes(bytes),
                lib,
                deleted: Vec::new(),
                repointed: Vec::new(),
                candidates: Vec::new(),
                original,
            };
            let bytes = pdb::guard_editor(|| {
                for op in ops {
                    apply_op(&mut db, op, strict, "export.pdb", warnings)?;
                }
                db.prune()?;
                Ok(db.editor.to_bytes())
            })?;
            let check = pdb::parse_database(bytes.clone(), &device.path(paths::EXPORT_PDB))?;
            if let Err(e) = verify(&db.lib, &pdb::library_from_database(&check)) {
                return Err(Error::Pdb(format!("the packed database failed verification ({e})")));
            }
            ext = ext_after(device, &db.deleted, &db.repointed)?;
            Some(bytes)
        }
    };
    let (plus, second) = match dbs.plus {
        None => (None, None),
        Some((lib, one)) => {
            let mut db = PlusDb { one, lib };
            for op in ops {
                apply_op(&mut db, op, strict && !has_pdb, "exportLibrary.db", warnings)?;
            }
            let bytes = db.one.to_encrypted()?;
            let check = OneLibrary::from_encrypted(&bytes, None)?.library()?;
            if let Err(e) = verify(&db.lib, &check) {
                return Err(Error::OneLibrary(format!("the packed database failed verification ({e})")));
            }
            (Some(bytes), has_pdb.then_some(db.lib))
        }
    };
    Ok((pdb, ext, plus, second))
}

// ---------------------------------------------------------------------------------------------
// Journal
// ---------------------------------------------------------------------------------------------

mod track_index;

/// The edits made to a drive, kept in `.locallibrary/edits.json` so they can be made again after
/// rekordbox replaces the drive's databases with a new export.
pub mod journal {
    use super::*;

    const FILE: &str = "edits.json";

    /// The journal as a key below the drive's root, for plans made from it.
    pub(crate) fn key() -> String {
        format!("{}/{FILE}", paths::STATE_DIR)
    }

    /// The journal's format. 1 was a bare list of entries; 2 is `{"version": 2, "entries": [...]}`
    /// and its entries can hold edits older builds don't know (`set_entries`,
    /// `delete_history_lists`, `my_tag`, `cues`); 3 adds `set_artwork`, and track details 2 would
    /// drop (`album_artist`, `bpm`, `isrc`, `release_date`); 4 adds `set_analysis` and cue edits
    /// setting several (`set_many`). Each is written only when an entry uses what it adds. A
    /// journal of a later version is refused rather than misread or overwritten.
    pub const VERSION: u32 = 4;

    #[derive(Serialize)]
    struct Versioned<'a> {
        version: u32,
        entries: &'a [JournalEntry],
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    pub struct JournalEntry {
        pub at: DateTime<Utc>,
        pub edits: Vec<Edit>,
        /// What the edits did, as described when they were applied.
        pub changes: Vec<String>,
        /// The backup made before them, which `restore` undoes them with.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub backup: Option<String>,
    }

    pub fn list(device: &Device) -> Result<Vec<JournalEntry>> {
        let path = device.state_dir().join(FILE);
        let Some(stored) = fsutil::read_json_opt::<serde_json::Value>(&path)? else { return Ok(Vec::new()) };
        let entries = match stored {
            list @ serde_json::Value::Array(_) => list,
            serde_json::Value::Object(mut file) => {
                let version = file.get("version").and_then(serde_json::Value::as_u64).unwrap_or(0);
                if version > u64::from(VERSION) {
                    return Err(Error::invalid(format!(
                        "this journal was written by a newer locallibrary ({}, version {version}; this one reads up to \
                         {VERSION}): update locallibrary to list or replay its edits",
                        path.display()
                    )));
                }
                file.remove("entries").unwrap_or_default()
            }
            _ => return Err(Error::parse(&path, "not a journal of edits")),
        };
        serde_json::from_value(entries).map_err(|e| Error::parse(&path, e))
    }

    /// Write the journal, keeping only the staged files its edits name.
    fn write(device: &Device, entries: &[JournalEntry]) -> Result<()> {
        let version = version(entries);
        fsutil::write_json_atomic(&device.state_dir().join(FILE), &Versioned { version, entries })?;
        staged(entries).into_iter().try_for_each(|(kind, keep)| kind.prune(device, &keep))
    }

    /// The version `entries` need: 4 once one holds an analysis edit or a cue edit setting
    /// several, 3 once one holds an artwork edit or a track detail 3 added, else 2, so a build
    /// that reads an older one keeps editing a drive whose edits use nothing newer.
    fn version(entries: &[JournalEntry]) -> u32 {
        let needs = |e: &Edit| match e {
            Edit::SetAnalysis { .. } | Edit::Cues { change: crate::cue_edit::CueChange::SetMany(_), .. } => 4,
            Edit::SetArtwork { .. } => 3,
            Edit::EditTracks { fields, .. }
                if fields.album_artist.is_some()
                    || fields.bpm.is_some()
                    || fields.isrc.is_some()
                    || fields.release_date.is_some() =>
            {
                3
            }
            _ => 2,
        };
        entries.iter().flat_map(|e| &e.edits).map(needs).max().unwrap_or(2)
    }

    /// The staged files the entries' edits name: artwork edits' images, analysis edits'
    /// waveforms.
    fn staged(entries: &[JournalEntry]) -> [(crate::staged::Staged, FxHashSet<Hash>); 2] {
        let edits = || entries.iter().flat_map(|e| &e.edits);
        let images = edits().filter_map(|e| match e {
            Edit::SetArtwork { image: ImageSource::Staged(hash), .. } => Some(*hash),
            _ => None,
        });
        let waveforms = edits().filter_map(|e| match e {
            Edit::SetAnalysis { analysis, .. } => match analysis.waveform {
                Some(crate::analysis_edit::WaveSource::Staged(hash)) => Some(hash),
                _ => None,
            },
            _ => None,
        });
        [(crate::staged::IMAGES, images.collect()), (crate::staged::WAVEFORMS, waveforms.collect())]
    }

    /// Record edits just made, after `backup`.
    pub(crate) fn append(device: &Device, edits: &[Edit], changes: &[String], backup: Option<&str>) -> Result<()> {
        let mut entries = list(device)?;
        let backup = backup.map(str::to_string);
        entries.push(JournalEntry { at: Utc::now(), edits: edits.to_vec(), changes: changes.to_vec(), backup });
        write(device, &entries)
    }

    /// Forget the edits restoring `backup` undid: the ones it was made for and every one recorded
    /// since it was made, so a replay doesn't make them again. Returns how many entries went.
    pub(crate) fn undone(device: &Device, backup: &crate::backup::BackupInfo) -> Result<usize> {
        let order = order(device)?;
        let entries = list(device)?;
        let (gone, kept): (Vec<_>, Vec<_>) = entries.into_iter().partition(|e| undoes(&order, backup, e));
        if !gone.is_empty() {
            write(device, &kept)?;
        }
        Ok(gone.len())
    }

    /// The entries restoring `backup` would take off (see `undone`), oldest first.
    pub fn taken_off(device: &Device, backup: &crate::backup::BackupInfo) -> Result<Vec<JournalEntry>> {
        let order = order(device)?;
        Ok(list(device)?.into_iter().filter(|e| undoes(&order, backup, e)).collect())
    }

    /// Each of the drive's backups by id, and its place among them (0: the newest).
    fn order(device: &Device) -> Result<FxHashMap<String, usize>> {
        Ok(crate::backup::list(device)?.into_iter().enumerate().map(|(i, b)| (b.id, i)).collect())
    }

    /// Whether restoring `backup` takes `e` back: `e`'s own backup is that one or a later one (in
    /// the order the drive's backups were made, as computers' clocks differ), or, when it has none
    /// left, `e` was recorded after `backup` was made.
    fn undoes(order: &FxHashMap<String, usize>, backup: &crate::backup::BackupInfo, e: &JournalEntry) -> bool {
        match (e.backup.as_ref().and_then(|id| order.get(id)), order.get(&backup.id)) {
            (Some(own), Some(restored)) => own <= restored,
            _ => e.backup.as_deref() == Some(backup.id.as_str()) || e.at >= backup.created,
        }
    }

    /// Forget every recorded edit, with the files they staged: the drive got a whole library
    /// from a snapshot (a push) or a pack, and they were made to the one it replaced. A journal
    /// this build can't read goes too.
    pub(crate) fn forget(device: &Device) -> Result<()> {
        fsutil::remove_if_exists(&device.state_dir().join(FILE))?;
        staged(&[]).into_iter().try_for_each(|(kind, none)| kind.prune(device, &none))
    }

    /// Give `target` the recorded edits of `source`, whose library a sync just copied onto it: the
    /// journal travels with the library, so `refresh` makes edits made on any clone again after
    /// rekordbox exports to this one. Without a journal this build can read on `source`,
    /// `target`'s is forgotten (see [`forget`]).
    pub(crate) fn copy(source: &Device, target: &Device) -> Result<()> {
        match list(source) {
            Ok(entries) if !entries.is_empty() => {
                for (kind, hashes) in staged(&entries) {
                    kind.copy(source, target, &hashes)?;
                }
                write(target, &entries)
            }
            _ => forget(target),
        }
    }

    /// A plan's warning that the drive's recorded edits will be forgotten (see [`forget`]), or
    /// replaced by those of `source`, the drive whose library replaces its own (see [`copy`]):
    /// when it has any `source` lacks. `by` (`this`, `packing`) replaces the library they were
    /// made to.
    pub(crate) fn forget_warning(device: &Device, source: Option<&Device>, by: &str) -> Option<String> {
        let kept: Vec<DateTime<Utc>> =
            source.and_then(|s| list(s).ok()).unwrap_or_default().iter().map(|e| e.at).collect();
        let edits = list(device).map_or(0, |j| j.iter().filter(|e| !kept.contains(&e.at)).count());
        let lacks = source.map(|s| format!(" {} lacks", s.name())).unwrap_or_default();
        (edits > 0).then(|| {
            format!(
                "{} has {edits} recorded edit(s){lacks}, made to the library {by} replaces: they're forgotten",
                device.name()
            )
        })
    }

    /// Forget the recorded edits (all, or those before `before`). Returns how many entries went.
    pub fn clear(device: &Device, before: Option<DateTime<Utc>>) -> Result<usize> {
        let entries = list(device)?;
        let (gone, kept): (Vec<_>, Vec<_>) = entries.into_iter().partition(|e| before.is_none_or(|b| e.at < b));
        write(device, &kept)?;
        Ok(gone.len())
    }

    /// Every recorded edit, oldest first, for [`super::plan`] with `lenient` set.
    pub fn edits(device: &Device) -> Result<Vec<Edit>> {
        Ok(list(device)?.into_iter().flat_map(|e| e.edits).collect())
    }
}

#[cfg(test)]
mod tests;
