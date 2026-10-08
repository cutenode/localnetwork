//! The check before a set: one report over the drives going to it, each item ok, a warning or a
//! problem:
//!
//! * every drive holds the same library state as the first one;
//! * on each drive, `export.pdb` and OneLibrary agree, so every player shows the same library;
//! * no track's audio, analysis or artwork is missing, and no audio file is cut short (files
//!   smaller than the database recorded have their end decoded);
//! * no file is too large for FAT32 when a FAT32 drive is in the set;
//! * no locallibrary operation was interrupted;
//! * the drive's history lists are in the store (rekordbox deletes them from the drive when it
//!   imports them), or it has none;
//! * the drive's file system and partition table suit players;
//! * given player models (or remembered ones, see [`crate::equipment::Remembered`]), each model
//!   finds the database it reads, reads the file system, and plays every track's format at its
//!   sample rate and bit depth (see [`crate::equipment`]). Without any, a drive with only one of
//!   the two databases is a warning: some players can't read it.
//!
//! Track formats come from the database rows; a file is opened (headers only) when its row
//! doesn't say, or for `.m4a` files whose bit rate doesn't tell AAC from Apple Lossless.
//!
//! With [`CheckOptions::audio`], every audio file is opened and its last seconds decoded (see
//! [`probe::check_tracks`]), not only the smaller ones, which also finds files that don't decode.
//!
//! Nothing is written to the drives (beyond the hash cache every scan of an adopted drive keeps).

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::compare::{self, Relation, Side};
use crate::device::Device;
use crate::doctor::{self, Databases, Problem};
use crate::equipment::{self, Codec, Exfat, FromPath, MODELS, Model, Unplayable};
use crate::format::players::{self, Verdict};
use crate::format::{FsKind, Scheme};
use crate::model::{Library, Track, TrackRef};
use crate::paths;
use crate::plays;
use crate::probe;
use crate::progress::{Ctx, Phase};
use crate::store::Store;
use crate::tree::FileTree;
use crate::{Error, Result};

/// Every check, in report order.
const CHECKS: &[&str] = &[
    "export",
    "same_library",
    "databases",
    "files",
    "audio",
    "large_files",
    "interrupted",
    "history",
    "file_system",
    "players",
    "formats",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Ok,
    Warning,
    Problem,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CheckItem {
    /// The drive it's about.
    pub drive: String,
    /// What was checked: `export`, `same_library`, `databases`, `files`, `audio`, `large_files`,
    /// `interrupted`, `history`, `file_system`, `players` or `formats`.
    pub check: &'static str,
    pub level: Level,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CheckReport {
    pub drives: Vec<String>,
    pub items: Vec<CheckItem>,
    pub problems: usize,
    pub warnings: usize,
    /// The player models each drive was checked against, and where they came from.
    pub players: Vec<DrivePlayers>,
    /// Tracks a player won't load, grouped as the `formats` items describe them.
    pub unloadable: Vec<Unloadable>,
    /// With [`CheckOptions::audio`]: audio files with a problem, grouped as the `audio` items
    /// describe them.
    pub audio: Vec<AudioIssues>,
}

/// Audio files with one kind of problem.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AudioIssues {
    pub drive: String,
    pub level: Level,
    /// `truncated`, `unreadable`, `undecodable` or `length_differs` (see [`probe::AudioProblem`]).
    pub kind: &'static str,
    /// The `audio` item's message.
    pub message: String,
    pub files: Vec<probe::AudioFinding>,
}

/// Where a drive's player models came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayersFrom {
    /// `--players`.
    Given,
    /// Remembered for the drive's library.
    Library,
    /// Remembered for every library.
    Default,
    /// None given or remembered.
    None,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DrivePlayers {
    pub drive: String,
    pub models: Vec<String>,
    pub from: PlayersFrom,
    /// The library they're remembered for (with `from: library`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
}

/// Tracks one player won't load, for one reason.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Unloadable {
    pub drive: String,
    pub player: String,
    /// `problem`, or `warning` when it rests on a value the model's specs only infer.
    pub level: Level,
    /// The format (`FLAC`), or the file extension when it's no format players list.
    pub format: String,
    pub reason: Unplayable,
    /// The sample rate (Hz) the player doesn't take, when that's the reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
    /// The bit depth the player doesn't take, when that's the reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bit_depth: Option<u16>,
    /// The `formats` item's message.
    pub message: String,
    /// One per audio file.
    pub tracks: Vec<TrackRef>,
}

impl CheckReport {
    /// No problems (warnings allowed).
    pub fn ok(&self) -> bool {
        self.problems == 0
    }

    /// Count the warnings of `check` (`history`...) as problems.
    pub(crate) fn escalate(&mut self, check: &str) {
        for i in self.items.iter_mut().filter(|i| i.check == check && i.level == Level::Warning) {
            i.level = Level::Problem;
            self.warnings -= 1;
            self.problems += 1;
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct CheckOptions {
    /// Player models to check the drives against (`CDJ-2000NXS2`, `CDJ-3000X`...). Without any,
    /// each drive is checked against the models remembered for its library, if any.
    #[serde(default)]
    pub players: Vec<String>,
    /// Also open every audio file and decode its last seconds, to find files that are cut short
    /// or don't decode. It reads a little of every file, so it takes a while on a big drive.
    #[serde(default)]
    pub audio: bool,
}

/// What was read from one drive.
struct Drive<'a> {
    device: &'a Device,
    name: String,
    side: Option<Side>,
    dbs: Option<Databases>,
    fs: Option<FsKind>,
    large: Vec<doctor::LargeFile>,
}

fn names<'a>(items: impl IntoIterator<Item = &'a String>, max: usize) -> String {
    let items: Vec<&String> = items.into_iter().collect();
    let shown: Vec<&str> = items.iter().take(max).map(|s| s.as_str()).collect();
    let more = if items.len() > max { format!(" and {} more", items.len() - max) } else { String::new() };
    format!("{}{more}", shown.join(", "))
}

pub fn check(devices: &[Device], store: Option<&Store>, opts: &CheckOptions, ctx: &Ctx<'_>) -> Result<CheckReport> {
    // Each drive's items, in the order the drives were given.
    let mut items: Vec<Vec<CheckItem>> = Vec::new();
    let mut drives: Vec<Drive> = Vec::new();
    let remembered = match (opts.players.is_empty(), store) {
        (true, Some(store)) => store.player_models()?,
        _ => equipment::Remembered::default(),
    };
    let mut chosen: Vec<DrivePlayers> = Vec::new();
    // Each library's history archive, read once for every drive of it.
    let mut archives: HashMap<Option<Uuid>, plays::HistoryArchive> = HashMap::new();
    let mut unloadable: Vec<Unloadable> = Vec::new();
    let mut audio_issues: Vec<AudioIssues> = Vec::new();
    for device in devices {
        let name = device.name();
        let players = choose_players(&name, device, opts, &remembered, store)?;
        let models: Vec<(String, Option<&'static Model>)> =
            players.models.iter().map(|m| (m.clone(), equipment::find(m))).collect();
        chosen.push(players);
        let mut audio = DriveAudio::default();
        let mut mine = Vec::new();
        let mut item =
            |check, level, message: String| mine.push(CheckItem { drive: name.clone(), check, level, message });
        let mut d = Drive { device, name: name.clone(), side: None, dbs: None, fs: None, large: Vec::new() };
        if !device.has_export() {
            item("export", Level::Problem, "no rekordbox export on this drive".into());
            items.push(mine);
            drives.push(d);
            continue;
        }
        let read = Side::from_device(device, ctx).and_then(|side| {
            let dbs = Databases::read(device)?;
            let tree = device.scan(Some(&dbs.everything()), ctx)?;
            Ok((side, dbs, tree))
        });
        match read {
            Ok((side, dbs, tree)) => {
                d.large = doctor::large_files(dbs.primary(), &tree);
                files_item(&name, &doctor::missing(device, &dbs, &tree, ctx)?, &mut item);
                audio = DriveAudio::read(device, &dbs, &tree, &models, ctx)?;
                if opts.audio {
                    let (checked, findings) = probe::check_tracks(device, &dbs.everything(), &tree, ctx)?;
                    let issues = audio_items(checked, findings, &mut item);
                    audio_issues.extend(issues.into_iter().map(|g| AudioIssues { drive: name.clone(), ..g }));
                }
                d.side = Some(side);
                d.dbs = Some(dbs);
            }
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => item("export", Level::Problem, format!("the library can't be read: {e}")),
        }
        let volume = doctor::volume_of(device.root());
        match &volume {
            Some((disk, _)) => disk_items(disk, &mut item),
            None => item(
                "file_system",
                Level::Warning,
                "not the root of a mounted volume, so its file system wasn't checked".into(),
            ),
        }
        let layout = Layout {
            has_pdb: device.has_device_library(),
            has_plus: device.has_library_plus(),
            volume: volume.as_ref().map(|(disk, v)| (disk.scheme, v.filesystem)),
        };
        let groups = player_items(&models, &layout, &audio, &mut item);
        unloadable.extend(groups.into_iter().map(|g| Unloadable { drive: name.clone(), ..g }));
        d.fs = volume.and_then(|(_, v)| v.filesystem);
        items.push(mine);
        drives.push(d);
    }

    let fat32: Vec<&String> =
        drives.iter().filter(|d| matches!(d.fs, Some(FsKind::Fat32 | FsKind::Fat16))).map(|d| &d.name).collect();
    let first = drives.first().and_then(|d| d.side.as_ref().map(|s| (d.name.clone(), s)));
    for (n, d) in drives.iter().enumerate() {
        let mine = &mut items[n];
        let mut item =
            |check, level, message: String| mine.push(CheckItem { drive: d.name.clone(), check, level, message });
        if let (Some((first_name, first)), Some(side), true) = (&first, &d.side, n > 0) {
            match packed_item(&drives[0], d) {
                Some((level, message)) => item("same_library", level, message),
                None => same_library_item(first_name, first, &d.name, side, &mut item),
            }
        }
        if let Some(dbs) = &d.dbs {
            databases_item(dbs, &mut item);
        }
        large_files_item(d, &fat32, &mut item);
        match d.device.state().and_then(|s| s.pending.as_ref()) {
            Some(p) => item("interrupted", Level::Problem, format!("{}: {}", p.describe(), p.remedy())),
            None => item("interrupted", Level::Ok, "no interrupted operation".into()),
        }
        if let (Some(dbs), Some(side)) = (&d.dbs, &d.side) {
            history_item(d, dbs, side, store, &mut archives, ctx, &mut item)?;
        }
    }
    // By drive, then by check.
    for mine in &mut items {
        mine.sort_by_key(|i| CHECKS.iter().position(|c| *c == i.check));
    }
    let items: Vec<CheckItem> = items.into_iter().flatten().collect();
    Ok(CheckReport {
        problems: items.iter().filter(|i| i.level == Level::Problem).count(),
        warnings: items.iter().filter(|i| i.level == Level::Warning).count(),
        drives: drives.iter().map(|d| d.name.clone()).collect(),
        items,
        players: chosen,
        unloadable,
        audio: audio_issues,
    })
}

fn same_library_item(
    first_name: &str,
    first: &Side,
    name: &str,
    side: &Side,
    item: &mut impl FnMut(&'static str, Level, String),
) {
    let c = compare::compare(first, side);
    let d = &c.diff;
    let changes = format!(
        "{} track(s) and {} playlist(s) differ",
        d.added_tracks.len() + d.removed_tracks.len() + d.changed_tracks.len(),
        d.added_playlists.len() + d.removed_playlists.len() + d.changed_playlists.len()
    );
    let (level, message) = match c.relation {
        Relation::Identical => (Level::Ok, format!("same library as {first_name}")),
        Relation::LeftNewer => {
            (Level::Problem, format!("behind {first_name} ({changes}); `locallibrary sync` brings it up to date"))
        }
        Relation::RightNewer => (Level::Problem, format!("newer than {first_name} ({changes}); sync the drives")),
        Relation::Diverged => (Level::Problem, format!("{name} and {first_name} have both changed ({changes})")),
        Relation::Unrelated => (Level::Problem, format!("holds a different library from {first_name}")),
        Relation::Undetermined => (Level::Problem, format!("differs from {first_name} ({changes})")),
    };
    item("same_library", level, message);
}

/// When one of the two drives was packed from the other (see [`crate::pack`]), it holds some of
/// the other's playlists rather than its library: it's up to date when the drive it was packed
/// from hasn't changed since.
fn packed_item(first: &Drive, d: &Drive) -> Option<(Level, String)> {
    let (first_side, side) = (first.side.as_ref()?, d.side.as_ref()?);
    let packed_from = |packed: &Drive, source: &Drive| {
        crate::pack::record(packed.device)
            .ok()
            .flatten()
            .filter(|r| r.source.is_some() && r.source == source.device.id())
    };
    let (record, source, packed, packed_name) = match (packed_from(d, first), packed_from(first, d)) {
        (Some(r), _) => (r, first_side, &d.name, "this drive"),
        (None, Some(r)) => (r, side, &first.name, first.name.as_str()),
        (None, None) => return None,
    };
    let source_name = &record.source_name;
    let fresh =
        record.source_fingerprint.is_some_and(|fp| source.fingerprint == Some(fp) || source.aliases.contains(&fp));
    let playlists = record.playlists.join(", ");
    Some(if fresh {
        (Level::Ok, format!("{packed_name} is packed from {source_name} ({playlists}) and up to date"))
    } else {
        (
            Level::Warning,
            format!(
                "{packed_name} is packed from {source_name} ({playlists}), which changed since; \
                 `locallibrary pack {source_name} {packed}` brings it up to date"
            ),
        )
    })
}

fn databases_item(dbs: &Databases, item: &mut impl FnMut(&'static str, Level, String)) {
    let (level, message) = match (dbs.drift(), &dbs.pdb, &dbs.plus) {
        (Some(drift), _, _) if drift.error.is_some() => {
            (Level::Problem, format!("OneLibrary can't be read: {}", drift.error.unwrap_or_default()))
        }
        (Some(drift), _, _) if !drift.is_empty() => (
            Level::Problem,
            format!(
                "export.pdb and OneLibrary differ ({} track(s) only in export.pdb, {} only in OneLibrary, {} changed, \
                 {} playlist(s)), so players don't all show the same library; `locallibrary convert --replace` \
                 rebuilds one from the other",
                drift.only_in_device_library.len(),
                drift.only_in_onelibrary.len(),
                drift.changed_tracks.len(),
                drift.playlists.len()
            ),
        ),
        (Some(_), _, _) => (Level::Ok, "export.pdb and OneLibrary agree".into()),
        (None, Some(_), None) => {
            (Level::Ok, "export.pdb only (CDJ-3000X, OPUS-QUAD, XDJ-AZ... need OneLibrary)".into())
        }
        (None, _, _) => (Level::Ok, "OneLibrary only (older players need export.pdb)".into()),
    };
    item("databases", level, message);
}

fn files_item(drive: &str, missing: &[doctor::BrokenTrack], item: &mut impl FnMut(&'static str, Level, String)) {
    let of = |problems: &[Problem]| -> Vec<String> {
        let mut seen = HashSet::new();
        missing
            .iter()
            .filter(|m| problems.contains(&m.problem))
            .filter(|m| seen.insert(m.track.file_path.clone()))
            .map(|m| m.track.name.clone())
            .collect()
    };
    let audio = of(&[Problem::MissingAudio, Problem::UnsafePath]);
    let cut = of(&[Problem::Truncated]);
    let analysis = of(&[Problem::MissingAnalysis]);
    let artwork = of(&[Problem::MissingArtwork]);
    if !audio.is_empty() {
        item("files", Level::Problem, format!("{} track(s) have no audio file: {}", audio.len(), names(&audio, 3)));
    }
    if !cut.is_empty() {
        let message =
            format!("{} track(s) stop short of their length (an interrupted copy?): {}", cut.len(), names(&cut, 3));
        item("files", Level::Problem, message);
    }
    if !analysis.is_empty() {
        item(
            "files",
            Level::Warning,
            format!(
                "{} track(s) have no analysis file (no waveform, beat grid or cues): {}",
                analysis.len(),
                names(&analysis, 3)
            ),
        );
    }
    if !artwork.is_empty() {
        item(
            "files",
            Level::Warning,
            format!(
                "{} track(s) have no artwork image: {}; `locallibrary art auto \"{drive}\"` fills in art their files embed",
                artwork.len(),
                names(&artwork, 3)
            ),
        );
    }
    if missing.is_empty() {
        item("files", Level::Ok, "every track's audio, analysis and artwork is there".into());
    }
}

fn large_files_item(d: &Drive<'_>, fat32: &[&String], item: &mut impl FnMut(&'static str, Level, String)) {
    if d.large.is_empty() {
        item("large_files", Level::Ok, "no files of 4 GB or more".into());
        return;
    }
    let files: Vec<String> = d.large.iter().map(|f| f.path.clone()).collect();
    let others: Vec<&String> = fat32.iter().copied().filter(|n| **n != d.name).collect();
    let (level, message) = match others.is_empty() {
        true => (
            Level::Warning,
            format!("{} file(s) of 4 GB or more won't fit on a FAT32 drive: {}", files.len(), names(&files, 3)),
        ),
        false => (
            Level::Problem,
            format!(
                "{} file(s) of 4 GB or more can't be copied to {} (FAT32): {}",
                files.len(),
                names(others, 3),
                names(&files, 3)
            ),
        ),
    };
    item("large_files", level, message);
}

fn history_item(
    d: &Drive<'_>,
    dbs: &Databases,
    side: &Side,
    store: Option<&Store>,
    archives: &mut HashMap<Option<Uuid>, plays::HistoryArchive>,
    ctx: &Ctx<'_>,
    item: &mut impl FnMut(&'static str, Level, String),
) -> Result<()> {
    let lists = plays::histories(dbs);
    if lists.is_empty() {
        item("history", Level::Ok, "no history lists on the drive".into());
        return Ok(());
    }
    let not_archived = |n: usize| {
        format!(
            "{n} of {} history list(s) aren't in the store; rekordbox deletes them from the drive when it imports \
             them (`locallibrary pull \"{}\"` keeps them)",
            lists.len(),
            d.name
        )
    };
    let Some(store) = store else {
        item("history", Level::Warning, not_archived(lists.len()));
        return Ok(());
    };
    let library = d.device.state().and_then(|s| s.library_id);
    let libraries = match library {
        Some(id) => store.library_by_id(id)?.into_iter().collect(),
        None => store.libraries()?,
    };
    // The drive's current state is a snapshot...
    if let Some(fp) = side.fingerprint {
        for lib in &libraries {
            if let Some(s) = store.find_by_fingerprint(lib, &fp)? {
                let message =
                    format!("{} history list(s) saved in snapshot {} of {}", lists.len(), s.short_id, lib.name);
                item("history", Level::Ok, message);
                return Ok(());
            }
        }
    }
    // ...or (the drive changed since) every list is in some snapshot.
    let archive = match archives.entry(library) {
        Entry::Occupied(e) => e.into_mut(),
        Entry::Vacant(e) => e.insert(plays::archive(store, library, ctx)?),
    };
    match lists.iter().filter(|l| !plays::is_archived(l, archive)).count() {
        0 => item("history", Level::Ok, format!("{} history list(s), all in the store", lists.len())),
        n => item("history", Level::Warning, not_archived(n)),
    }
    Ok(())
}

fn disk_items(disk: &crate::format::Disk, item: &mut impl FnMut(&'static str, Level, String)) {
    let c = players::assess_disk(disk);
    let level = match c.verdict {
        Verdict::AllPlayers => Level::Ok,
        Verdict::SomePlayers | Verdict::Unknown => Level::Warning,
        Verdict::NoPlayers => Level::Problem,
    };
    let notes = if c.notes.is_empty() { String::new() } else { format!(" ({})", c.notes.join("; ")) };
    item("file_system", level, format!("{}{notes}", c.summary));
}

/// The models `--players` names, or else those remembered for the drive's library (or for every
/// library).
fn choose_players(
    drive: &str,
    device: &Device,
    opts: &CheckOptions,
    remembered: &equipment::Remembered,
    store: Option<&Store>,
) -> Result<DrivePlayers> {
    let mut players =
        DrivePlayers { drive: drive.to_string(), models: Vec::new(), from: PlayersFrom::None, library: None };
    if !opts.players.is_empty() {
        players.models = opts.players.clone();
        players.from = PlayersFrom::Given;
        return Ok(players);
    }
    let library = device.state().and_then(|s| s.library_id);
    let (models, own) = remembered.for_library(library);
    players.models = models.to_vec();
    players.from = match (models.is_empty(), own) {
        (true, _) => PlayersFrom::None,
        (false, true) => PlayersFrom::Library,
        (false, false) => PlayersFrom::Default,
    };
    if own && let (Some(store), Some(id)) = (store, library) {
        players.library = store.library_by_id(id)?.map(|l| l.name);
    }
    Ok(players)
}

/// The `audio` items: one per kind of problem the files have, or one saying they're all fine.
/// Returns the groups.
fn audio_items(
    checked: usize,
    findings: Vec<probe::AudioFinding>,
    item: &mut impl FnMut(&'static str, Level, String),
) -> Vec<AudioIssues> {
    let kinds = [
        ("truncated", Level::Problem, "stop short of their length (an interrupted copy?)"),
        ("unreadable", Level::Problem, "can't be opened as audio"),
        ("undecodable", Level::Problem, "don't decode at the end"),
        ("length_differs", Level::Warning, "aren't the length the database says (replaced since the export?)"),
    ];
    let mut groups = Vec::new();
    for (kind, level, what) in kinds {
        let files: Vec<probe::AudioFinding> = findings.iter().filter(|f| f.problem.kind() == kind).cloned().collect();
        if files.is_empty() {
            continue;
        }
        let list: Vec<String> = files.iter().map(|f| f.track.name.clone()).collect();
        let message = format!("{} audio file(s) {what}: {}", files.len(), names(&list, 3));
        item("audio", level, message.clone());
        groups.push(AudioIssues { drive: String::new(), level, kind, message, files });
    }
    if groups.is_empty() {
        item("audio", Level::Ok, format!("all {checked} audio file(s) open and decode to the end"));
    }
    groups
}

/// What decides whether a player can use a drive.
struct Layout {
    has_pdb: bool,
    has_plus: bool,
    /// Partition table and file system, when the drive is a mounted volume.
    volume: Option<(Option<Scheme>, Option<FsKind>)>,
}

/// A track's audio as players see it.
#[derive(Debug, Clone)]
struct TrackAudio {
    track: TrackRef,
    /// The format, or the extension of a file in no format players list; `None` when it can't
    /// be told (an `.m4a` file the database and the file both leave open).
    codec: Option<std::result::Result<Codec, String>>,
    /// Hz, 0 when unknown.
    rate: u32,
    /// Bits, 0 when unknown.
    depth: u16,
}

/// The audio of each database's tracks (one row per file), for the databases a chosen player
/// reads.
#[derive(Debug, Default)]
struct DriveAudio {
    pdb: Option<Vec<TrackAudio>>,
    plus: Option<Vec<TrackAudio>>,
}

/// rekordbox records an MPEG-4 file's average bit rate. Players take AAC up to 320 kbps, and
/// Apple Lossless runs at several hundred; anything in between (or nothing) is left to the file.
fn m4a_codec(bitrate: u32) -> Option<Codec> {
    match bitrate {
        1..=320 => Some(Codec::Aac),
        401.. => Some(Codec::Alac),
        _ => None,
    }
}

/// Whether a track's row leaves something about its audio to the file.
fn needs_probe(t: &Track) -> bool {
    match Codec::from_path(&t.file_path) {
        FromPath::Unknown => false,
        FromPath::AacOrAlac => m4a_codec(t.bitrate).is_none() || t.sample_rate == 0,
        FromPath::Codec(c) => t.sample_rate == 0 || (c.lossless() && t.sample_depth == 0),
    }
}

fn track_audio(t: &Track, probed: Option<&probe::Format>) -> TrackAudio {
    let codec = match Codec::from_path(&t.file_path) {
        FromPath::Codec(c) => Some(Ok(c)),
        FromPath::AacOrAlac => match probed.and_then(|f| f.codec) {
            Some(c @ (Codec::Aac | Codec::Alac)) => Some(Ok(c)),
            _ => m4a_codec(t.bitrate).map(Ok),
        },
        FromPath::Unknown => {
            let ext = t.file_path.rsplit_once('.').map(|(_, e)| e).filter(|e| !e.contains('/'));
            Some(Err(ext.map_or_else(|| "extensionless".to_string(), |e| format!(".{}", e.to_lowercase()))))
        }
    };
    let rate = if t.sample_rate > 0 { t.sample_rate } else { probed.map_or(0, |f| f.sample_rate) };
    let depth = if t.sample_depth > 0 { t.sample_depth } else { probed.map_or(0, |f| f.bit_depth) };
    TrackAudio { track: t.into(), codec, rate, depth }
}

impl DriveAudio {
    fn read(
        device: &Device,
        dbs: &Databases,
        tree: &FileTree,
        models: &[(String, Option<&'static Model>)],
        ctx: &Ctx<'_>,
    ) -> Result<DriveAudio> {
        let players: Vec<&Model> = models.iter().filter_map(|(_, m)| *m).filter(|m| !m.is_mixer()).collect();
        let reads_pdb = |m: &Model| m.export_pdb.value && dbs.pdb.is_some();
        let pdb = dbs.pdb.as_ref().filter(|_| players.iter().any(|m| reads_pdb(m)));
        let plus = dbs.plus.as_ref().filter(|_| players.iter().any(|m| m.onelibrary.value && !reads_pdb(m)));
        let libraries: Vec<&Library> = pdb.into_iter().chain(plus).collect();
        // Files whose rows don't say enough, each opened once.
        let mut todo: Vec<(String, PathBuf, u64)> = Vec::new();
        let mut seen = HashSet::new();
        for t in libraries.iter().flat_map(|l| &l.tracks).filter(|t| needs_probe(t)) {
            let key = paths::key(&t.file_path);
            if !seen.insert(key.to_lowercase()) {
                continue;
            }
            let file = match tree.resolve(&key) {
                Some(e) => Some((device.path(&e.path), e.size)),
                None => device.checked_path(&key).ok().filter(|p| p.is_file()).map(|p| (p, t.file_size)),
            };
            if let Some((path, size)) = file {
                todo.push((key, path, size));
            }
        }
        let formats =
            probe::parallel(&todo, Phase::Scan, |(k, _, n)| (k.clone(), *n), |(_, p, _)| probe::format(p).ok(), ctx)?;
        let probed: HashMap<String, probe::Format> =
            todo.iter().zip(formats).filter_map(|((key, ..), f)| f.map(|f| (key.to_lowercase(), f))).collect();
        let audio = |library: &Library| -> Vec<TrackAudio> {
            let mut files = HashSet::new();
            library
                .tracks
                .iter()
                .filter(|t| files.insert(paths::key(&t.file_path).to_lowercase()))
                .map(|t| track_audio(t, probed.get(&paths::key(&t.file_path).to_lowercase())))
                .collect()
        };
        Ok(DriveAudio { pdb: pdb.map(audio), plus: plus.map(audio) })
    }
}

/// `44.1`, `96`: a sample rate in kHz.
fn khz_number(rate: u32) -> String {
    (rate as f64 / 1000.0).to_string()
}

/// `a, b and c`.
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

fn fs_name(fs: FsKind) -> &'static str {
    match fs {
        FsKind::Fat12 => "FAT12",
        FsKind::Fat16 => "FAT16",
        FsKind::Fat32 => "FAT32",
        FsKind::Exfat => "exFAT",
        FsKind::Hfsplus => "HFS+",
        FsKind::Apfs => "APFS",
        FsKind::Ntfs => "NTFS",
        FsKind::Other => "this file system",
    }
}

/// Without player models, a drive with only one database: some players can't read it.
fn unknown_players_item(layout: &Layout, item: &mut impl FnMut(&'static str, Level, String)) {
    let names = |pick: fn(&Model) -> bool| {
        let v: Vec<&str> = MODELS.iter().filter(|m| !m.is_mixer() && pick(m)).map(|m| m.name).collect();
        v.join(", ")
    };
    let hint = "`--players` (or `locallibrary players set`) checks the models you play on";
    let only = |db: &str, readers: String, other: &str| {
        format!(
            "no player models given, and the drive has only {db}: {readers} read only {other} and can't use it; {hint}"
        )
    };
    match (layout.has_pdb, layout.has_plus) {
        (true, false) => {
            item("players", Level::Warning, only("export.pdb", names(Model::onelibrary_only), "OneLibrary"))
        }
        (false, true) => {
            item("players", Level::Warning, only("OneLibrary", names(|m| !m.onelibrary.value), "export.pdb"))
        }
        _ => {}
    }
}

/// One item per model on whether it can read the drive (database, partition table, file
/// system), and its `formats` items: one per group of tracks it won't load, or one saying it
/// plays them all. Returns the groups.
fn player_items(
    models: &[(String, Option<&'static Model>)],
    layout: &Layout,
    audio: &DriveAudio,
    item: &mut impl FnMut(&'static str, Level, String),
) -> Vec<Unloadable> {
    if models.is_empty() {
        unknown_players_item(layout, item);
    }
    let mut groups = Vec::new();
    for (given, model) in models {
        let Some(m) = model else {
            item(
                "players",
                Level::Warning,
                format!(
                    "{given}: not a model locallibrary knows about; not checked (`locallibrary players models` lists them)"
                ),
            );
            continue;
        };
        let mut issues: Vec<(Level, String)> = Vec::new();
        let has_its_database = (m.export_pdb.value && layout.has_pdb) || (m.onelibrary.value && layout.has_plus);
        if !m.is_mixer() && !has_its_database {
            let (spec, message) = if m.onelibrary_only() {
                (
                    m.onelibrary,
                    "reads only OneLibrary (exportLibrary.db), which this drive doesn't have; \
                     `locallibrary convert --to onelibrary` adds it",
                )
            } else {
                (
                    m.export_pdb,
                    "needs export.pdb, which this drive doesn't have; `locallibrary convert --to device-library` adds it",
                )
            };
            issues.push(match spec.inferred {
                false => (Level::Problem, message.to_string()),
                true => (Level::Warning, format!("probably {message}")),
            });
        }
        match layout.volume {
            None => match issues.first() {
                None => item(
                    "players",
                    Level::Ok,
                    format!("{}: has the database it reads (file system not checked)", m.name),
                ),
                Some((level, message)) => item("players", *level, format!("{}: {message}", m.name)),
            },
            Some((scheme, fs)) => {
                match scheme {
                    Some(Scheme::Mbr) => {}
                    Some(Scheme::Gpt) => {
                        issues.push((Level::Problem, "the drive has a GUID partition table; reformat with MBR".into()))
                    }
                    Some(_) => issues.push((Level::Problem, "players need an MBR partition table".into())),
                    None => issues.push((
                        Level::Warning,
                        "the drive has no partition table, which players aren't documented to read".into(),
                    )),
                }
                match fs.map(|fs| (fs, m.reads(fs))) {
                    None => issues.push((Level::Warning, "the drive's file system isn't known".into())),
                    Some((_, Some(true))) => {}
                    Some((_, None)) => {
                        let version = match m.exfat.value {
                            Exfat::Firmware(v) => v,
                            Exfat::Yes | Exfat::No => "",
                        };
                        issues.push((Level::Warning, format!("reads exFAT only with firmware {version}+")));
                    }
                    Some((FsKind::Fat16 | FsKind::Hfsplus | FsKind::Exfat, Some(false))) if m.is_mixer() => {
                        issues.push((Level::Problem, "settings and firmware updates need FAT32".into()))
                    }
                    Some((fs, Some(false))) => issues.push((Level::Problem, format!("can't read {}", fs_name(fs)))),
                }
                match issues.iter().map(|(l, _)| *l).max() {
                    None => item("players", Level::Ok, format!("{}: can read this drive", m.name)),
                    Some(level) => {
                        let text: Vec<&str> = issues.iter().map(|(_, s)| s.as_str()).collect();
                        item("players", level, format!("{}: {}", m.name, text.join("; ")));
                    }
                }
            }
        }
        let tracks = match m.export_pdb.value && layout.has_pdb {
            true => audio.pdb.as_deref(),
            false => audio.plus.as_deref().filter(|_| m.onelibrary.value),
        };
        if let Some(tracks) = tracks.filter(|_| !m.is_mixer()) {
            groups.extend(format_items(m, tracks, item));
        }
    }
    groups
}

/// What a model takes of `format`: `it plays FLAC at 44.1 and 48 kHz`.
fn takes(m: &Model, format: &str, rates: bool, depths: bool, probably: &str) -> String {
    let Some(a) = Codec::ALL.iter().find(|c| c.name() == format).and_then(|c| m.audio(*c)) else {
        return String::new();
    };
    let mut what = Vec::new();
    if rates {
        what.push(format!("{} kHz", and_list(&a.rates.iter().map(|r| khz_number(*r)).collect::<Vec<_>>())));
    }
    if depths {
        what.push(format!("{}-bit", and_list(&a.depths.iter().map(|d| d.to_string()).collect::<Vec<_>>())));
    }
    format!("it {probably}plays {format} at {}", what.join(", "))
}

/// The `formats` items for one model over a database's tracks.
fn format_items(
    m: &Model,
    tracks: &[TrackAudio],
    item: &mut impl FnMut(&'static str, Level, String),
) -> Vec<Unloadable> {
    // (format, reason, rate, depth, inferred) -> tracks
    type Key = (String, Unplayable, Option<u32>, Option<u16>, bool);
    let mut failing: BTreeMap<Key, Vec<TrackRef>> = BTreeMap::new();
    let mut checked = 0;
    let mut inferred = false;
    for t in tracks {
        let key = match &t.codec {
            None => continue,
            Some(Err(ext)) => (ext.clone(), Unplayable::Format, None, None, false),
            Some(Ok(codec)) => {
                let p = m.plays(*codec, t.rate, t.depth);
                match p.result {
                    Ok(()) => {
                        checked += 1;
                        inferred |= p.inferred;
                        continue;
                    }
                    Err(why) => {
                        let rate = matches!(why, Unplayable::SampleRate | Unplayable::SampleRateAndBitDepth);
                        let depth = matches!(why, Unplayable::BitDepth | Unplayable::SampleRateAndBitDepth);
                        (codec.name().to_string(), why, rate.then_some(t.rate), depth.then_some(t.depth), p.inferred)
                    }
                }
            }
        };
        checked += 1;
        failing.entry(key).or_default().push(t.track.clone());
    }
    let mut groups: Vec<Unloadable> = failing
        .into_iter()
        .map(|((format, reason, sample_rate, bit_depth, inferred), tracks)| {
            let at = match (sample_rate, bit_depth) {
                (Some(r), Some(d)) => format!(" at {} kHz/{d}-bit", khz_number(r)),
                (Some(r), None) => format!(" at {} kHz", khz_number(r)),
                (None, Some(d)) => format!(" at {d}-bit"),
                (None, None) => String::new(),
            };
            let probably = if inferred { "probably " } else { "" };
            let why = match reason {
                Unplayable::Format => format!("it {probably}doesn't play {format}"),
                Unplayable::SampleRate => takes(m, &format, true, false, probably),
                Unplayable::BitDepth => takes(m, &format, false, true, probably),
                Unplayable::SampleRateAndBitDepth => takes(m, &format, true, true, probably),
            };
            let files = if tracks.len() == 1 { "file" } else { "files" };
            let message =
                format!("{} {format} {files}{at} {probably}won't load on the {}: {why}", tracks.len(), m.name);
            Unloadable {
                drive: String::new(),
                player: m.name.to_string(),
                level: if inferred { Level::Warning } else { Level::Problem },
                format,
                reason,
                sample_rate,
                bit_depth,
                message,
                tracks,
            }
        })
        .collect();
    groups.sort_by(|a, b| b.level.cmp(&a.level).then(b.tracks.len().cmp(&a.tracks.len())));
    for g in &groups {
        item("formats", g.level, g.message.clone());
    }
    if groups.is_empty() {
        let message = match inferred {
            false => format!("{}: plays all {checked} track(s) (format, sample rate and bit depth)", m.name),
            true => format!(
                "{}: probably plays all {checked} track(s) (format, sample rate and bit depth; some of its specs are \
                 inferred)",
                m.name
            ),
        };
        item("formats", Level::Ok, message);
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn models(players: &[&str]) -> Vec<(String, Option<&'static Model>)> {
        players.iter().map(|p| (p.to_string(), equipment::find(p))).collect()
    }

    type Items = Vec<(Level, String)>;

    /// The `players` items, and the `formats` items.
    fn verdicts(players: &[&str], layout: &Layout, audio: &DriveAudio) -> (Items, Items) {
        let (mut players_items, mut formats) = (Vec::new(), Vec::new());
        player_items(&models(players), layout, audio, &mut |check, level, message| match check {
            "players" => players_items.push((level, message)),
            _ => formats.push((level, message)),
        });
        (players_items, formats)
    }

    fn levels(v: &[(Level, String)]) -> Vec<Level> {
        v.iter().map(|(l, _)| *l).collect()
    }

    #[test]
    fn players_against_file_systems_and_databases() {
        let none = DriveAudio::default();
        let drive = |fs, has_plus| Layout { has_pdb: true, has_plus, volume: Some((Some(Scheme::Mbr), Some(fs))) };
        let players = |p: &[&str], layout: &Layout| verdicts(p, layout, &none).0;
        let all = ["CDJ-2000NXS2", "cdj-3000", "CDJ-3000X", "DJM-V10"];
        assert_eq!(levels(&players(&all, &drive(FsKind::Fat32, true))), [Level::Ok; 4]);
        let exfat = players(&all, &drive(FsKind::Exfat, true));
        assert_eq!(levels(&exfat), [Level::Problem, Level::Warning, Level::Ok, Level::Problem]);
        assert!(exfat[1].1.contains("firmware 1.20+"), "{exfat:?}");
        assert_eq!(
            levels(&players(&all, &drive(FsKind::Hfsplus, false))),
            [Level::Ok, Level::Ok, Level::Problem, Level::Problem]
        );
        let gpt = Layout { has_pdb: true, has_plus: true, volume: Some((Some(Scheme::Gpt), Some(FsKind::Fat32))) };
        assert_eq!(levels(&players(&["CDJ-3000"], &gpt)), [Level::Problem]);
        let unknown = Layout { has_pdb: true, has_plus: false, volume: None };
        assert_eq!(
            levels(&players(&["CDJ-2000NXS2", "OPUS-QUAD", "XYZ-9"], &unknown)),
            [Level::Ok, Level::Problem, Level::Warning]
        );
        // A drive the CDJ-3000 can't read but players reading only OneLibrary can.
        let only_plus = Layout { has_pdb: false, has_plus: true, volume: None };
        assert_eq!(levels(&players(&["CDJ-3000", "OMNIS-DUO"], &only_plus)), [Level::Problem, Level::Ok]);
    }

    /// Without models, a drive with one database warns (the audit's D8: a drive holding only
    /// export.pdb used to pass without a word about the CDJ-3000X).
    #[test]
    fn no_models_and_one_database() {
        let none = DriveAudio::default();
        let pdb_only = Layout { has_pdb: true, has_plus: false, volume: None };
        let v = verdicts(&[], &pdb_only, &none).0;
        assert_eq!(levels(&v), [Level::Warning]);
        assert!(v[0].1.contains("CDJ-3000X") && v[0].1.contains("XDJ-AN"), "{v:?}");
        let plus_only = Layout { has_pdb: false, has_plus: true, volume: None };
        let v = verdicts(&[], &plus_only, &none).0;
        assert!(v[0].1.contains("CDJ-2000NXS2") && !v[0].1.contains("CDJ-3000X"), "{v:?}");
        let both = Layout { has_pdb: true, has_plus: true, volume: None };
        assert!(verdicts(&[], &both, &none).0.is_empty());
    }

    fn row(path: &str, bitrate: u32, rate: u32, depth: u16) -> Track {
        Track {
            id: 1,
            title: path.into(),
            artist: String::new(),
            album: String::new(),
            album_artist: String::new(),
            genre: String::new(),
            label: String::new(),
            key: String::new(),
            remixer: String::new(),
            original_artist: String::new(),
            composer: String::new(),
            comment: String::new(),
            mix_name: String::new(),
            isrc: String::new(),
            bpm: 0.0,
            duration_secs: 0,
            year: 0,
            track_number: 0,
            disc_number: 0,
            rating: 0,
            color: None,
            play_count: 0,
            bitrate,
            sample_rate: rate,
            sample_depth: depth,
            file_size: 0,
            file_path: path.into(),
            analyze_path: String::new(),
            date_added: String::new(),
            release_date: String::new(),
            artwork_id: None,
            artwork_path: None,
        }
    }

    #[test]
    fn audio_from_rows() {
        let a = track_audio(&row("/Contents/a.m4a", 256, 44_100, 16), None);
        assert_eq!(a.codec, Some(Ok(Codec::Aac)));
        let a = track_audio(&row("/Contents/a.m4a", 900, 96_000, 24), None);
        assert_eq!((a.codec, a.rate, a.depth), (Some(Ok(Codec::Alac)), 96_000, 24));
        // The file has the last word on an .m4a, and fills in what the row leaves out.
        let probed =
            probe::Format { codec: Some(Codec::Alac), sample_rate: 48_000, bit_depth: 24, duration_secs: None };
        let a = track_audio(&row("/Contents/a.m4a", 0, 0, 0), Some(&probed));
        assert_eq!((a.codec, a.rate, a.depth), (Some(Ok(Codec::Alac)), 48_000, 24));
        assert_eq!(track_audio(&row("/Contents/a.m4a", 360, 44_100, 16), None).codec, None);
        assert_eq!(track_audio(&row("/Contents/a.ogg", 0, 0, 0), None).codec, Some(Err(".ogg".into())));
        assert!(needs_probe(&row("/Contents/a.flac", 0, 0, 0)));
        assert!(needs_probe(&row("/Contents/a.flac", 0, 44_100, 0)));
        assert!(!needs_probe(&row("/Contents/a.mp3", 320, 44_100, 0)));
        assert!(needs_probe(&row("/Contents/a.m4a", 360, 44_100, 16)));
    }

    #[test]
    fn formats_players_wont_load() {
        let mut tracks: Vec<TrackAudio> = Vec::new();
        for i in 0..12 {
            tracks.push(track_audio(&row(&format!("/Contents/hi{i}.flac"), 0, 96_000, 24), None));
        }
        tracks.push(track_audio(&row("/Contents/a.mp3", 320, 44_100, 16), None));
        tracks.push(track_audio(&row("/Contents/b.wav", 0, 44_100, 32), None));
        tracks.push(track_audio(&row("/Contents/c.ogg", 0, 44_100, 16), None));
        let audio = DriveAudio { pdb: Some(tracks.clone()), plus: None };
        let layout = Layout { has_pdb: true, has_plus: false, volume: None };

        let (_, formats) = verdicts(&["CDJ-2000NXS"], &layout, &audio);
        assert_eq!(levels(&formats), [Level::Problem; 3]);
        assert_eq!(formats[0].1, "12 FLAC files won't load on the CDJ-2000NXS: it doesn't play FLAC");
        assert!(formats.iter().any(|(_, m)| m == "1 .ogg file won't load on the CDJ-2000NXS: it doesn't play .ogg"));
        let wav = "1 WAV file at 32-bit won't load on the CDJ-2000NXS: it plays WAV at 16 and 24-bit";
        assert!(formats.iter().any(|(_, m)| m == wav), "{formats:?}");

        let (_, formats) = verdicts(&["CDJ-2000NXS2"], &layout, &audio);
        assert_eq!(formats.len(), 2, "FLAC at 96 kHz plays: {formats:?}");

        let audio_plus = DriveAudio { pdb: None, plus: Some(tracks) };
        let layout_plus = Layout { has_pdb: false, has_plus: true, volume: None };
        let (_, formats) = verdicts(&["OMNIS-DUO"], &layout_plus, &audio_plus);
        assert_eq!(
            formats[0].1,
            "12 FLAC files at 96 kHz won't load on the OMNIS-DUO: it plays FLAC at 44.1 and 48 kHz"
        );

        // Inferred specs say "probably", as warnings.
        let (_, formats) = verdicts(&["XDJ-RX2"], &layout, &audio);
        let flac = "12 FLAC files probably won't load on the XDJ-RX2: it probably doesn't play FLAC";
        assert!(formats.contains(&(Level::Warning, flac.to_string())), "{formats:?}");

        let fine =
            DriveAudio { pdb: Some(vec![track_audio(&row("/Contents/a.mp3", 320, 44_100, 16), None)]), plus: None };
        assert_eq!(
            verdicts(&["CDJ-3000"], &layout, &fine).1,
            [(Level::Ok, "CDJ-3000: plays all 1 track(s) (format, sample rate and bit depth)".to_string())]
        );
        // Mixers have no tracks to play.
        assert!(verdicts(&["DJM-A9"], &layout, &fine).1.is_empty());
    }

    #[test]
    fn players_refuse_other_partition_tables_and_file_systems() {
        let none = DriveAudio::default();
        let on = |scheme, fs| Layout { has_pdb: true, has_plus: true, volume: Some((scheme, fs)) };
        let said = |player: &str, layout: &Layout| verdicts(&[player], layout, &none).0.remove(0);
        let mbr = Some(Scheme::Mbr);
        for (fs, name) in [
            (FsKind::Fat12, "FAT12"),
            (FsKind::Ntfs, "NTFS"),
            (FsKind::Apfs, "APFS"),
            (FsKind::Other, "this file system"),
        ] {
            assert_eq!(said("CDJ-3000", &on(mbr, Some(fs))), (Level::Problem, format!("CDJ-3000: can't read {name}")));
        }
        assert_eq!(
            said("DJM-A9", &on(mbr, Some(FsKind::Hfsplus))),
            (Level::Problem, "DJM-A9: settings and firmware updates need FAT32".into())
        );
        assert_eq!(
            said("CDJ-3000", &on(Some(Scheme::Apm), Some(FsKind::Fat32))),
            (Level::Problem, "CDJ-3000: players need an MBR partition table".into())
        );
        // Neither a partition table nor a file system that can be told: warnings, both said.
        assert_eq!(
            said("CDJ-3000", &on(None, None)),
            (
                Level::Warning,
                "CDJ-3000: the drive has no partition table, which players aren't documented to read; the drive's \
                 file system isn't known"
                    .into()
            )
        );
    }

    #[test]
    fn formats_out_of_range_are_problems_and_unknown_ones_are_left_open() {
        let tracks = vec![
            track_audio(&row("/Contents/hi.wav", 0, 192_000, 32), None),
            // An .m4a neither its row nor its file tells apart: not held against any player.
            track_audio(&row("/Contents/a.m4a", 360, 44_100, 16), None),
        ];
        let audio = DriveAudio { pdb: Some(tracks), plus: None };
        let layout = Layout { has_pdb: true, has_plus: false, volume: None };
        let (_, formats) = verdicts(&["CDJ-2000NXS2"], &layout, &audio);
        assert_eq!(formats.len(), 1, "{formats:?}");
        let (level, message) = &formats[0];
        assert_eq!(*level, Level::Problem);
        let start = "1 WAV file at 192 kHz/32-bit won't load on the CDJ-2000NXS2: it plays WAV at 44.1, 48";
        assert!(message.starts_with(start) && message.ends_with(" kHz, 16 and 24-bit"), "{message}");

        // The CDJ-2000's specs are inferred from the CDJ-900's.
        let mp3 =
            DriveAudio { pdb: Some(vec![track_audio(&row("/Contents/a.mp3", 320, 44_100, 16), None)]), plus: None };
        let message = "CDJ-2000: probably plays all 1 track(s) (format, sample rate and bit depth; some of its specs \
                       are inferred)";
        assert_eq!(verdicts(&["CDJ-2000"], &layout, &mp3).1, [(Level::Ok, message.to_string())]);
    }
}
