//! Editing cue points on a drive: hot cues A-H, memory cues and loops.
//!
//! A track's cues live in its analysis files (see [`anlz::write_cue_points`]), which every
//! player reads, and in OneLibrary's `cue` table (see [`OneLibrary::replace_cues`]) for the
//! players that read only `exportLibrary.db`; `export.pdb` holds none. An edit rewrites both.
//! Positions are times or beats of the track's beat grid, and a loop can be a number of beats
//! (its end lands on the grid). The analysis files and databases are backed up first, so
//! `locallibrary undo` takes an edit back.
//!
//! rekordbox doesn't know about these edits: exporting the track again from rekordbox brings back
//! the cues its collection has. Every applied edit is recorded in the drive's journal of edits (as
//! a [`CueChange`] on tracks named by file), so `edits replay` makes it again.

use rustc_hash::FxHashMap;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::anlz::{self, Beat, CueFile, CuePoint, DEFAULT_HOT_CUE_RGB, HOT_CUE_PALETTE};
use crate::commit::{Changes, NewDatabases, NewFile, Options};
use crate::device::{Device, LineageOp};
use crate::edit::Replayed;
use crate::fsutil;
use crate::hash::Hash;
use crate::model::{Library, Track, TrackColor, TrackRef};
use crate::onelibrary::{CueRow, CueRowChanges, OneLibrary};
use crate::paths::{self, FileClass};
use crate::progress::Ctx;
use crate::{Error, Result};

/// How far (ms) a memory cue may be from the position given to find it.
const FIND_TOLERANCE_MS: u32 = 100;

/// A hot cue slot, A-H (written as its letter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "String", into = "String")]
pub struct HotCue(pub u8);

impl HotCue {
    pub const COUNT: u8 = 8;

    /// `A` for slot 0. Slots past H only come from a corrupt file; past Z they show as `?` rather
    /// than overflow (or turn into another slot's lower-case letter).
    pub fn letter(self) -> char {
        if self.0 < 26 { char::from(b'A' + self.0) } else { '?' }
    }
}

impl std::fmt::Display for HotCue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.letter())
    }
}

impl std::str::FromStr for HotCue {
    type Err = Error;

    fn from_str(s: &str) -> Result<HotCue> {
        match s.trim().as_bytes() {
            [c @ (b'A'..=b'H' | b'a'..=b'h')] => Ok(HotCue(c.to_ascii_uppercase() - b'A')),
            _ => Err(Error::invalid(format!("hot cues are A-H, not {s:?}"))),
        }
    }
}

impl TryFrom<String> for HotCue {
    type Error = Error;

    fn try_from(s: String) -> Result<HotCue> {
        s.parse()
    }
}

impl From<HotCue> for String {
    fn from(h: HotCue) -> String {
        h.to_string()
    }
}

/// A position in a track.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CuePosition {
    /// Milliseconds from the start.
    Ms(u32),
    /// A beat of the track's beat grid, counting from 1.
    Beat(u32),
}

/// Add a cue, or replace the hot cue in its slot (or the memory cue at its position). A replaced
/// cue keeps its name and colour unless new ones are given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetCue {
    /// The hot cue slot; a memory cue when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hot_cue: Option<HotCue>,
    pub at: CuePosition,
    /// Move a time to the nearest beat of the grid.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub snap: bool,
    /// Make it a loop this many beats long (a whole number, or 0.5, 0.25...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_beats: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// See [`set_color`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// Which existing cues an edit is about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CueRef {
    /// `{"hot": "B"}`
    Hot(HotCue),
    /// `{"memory": {"ms": 62500}}`: the memory cue nearest the position (within 0.1 s).
    Memory(CuePosition),
    /// Every cue.
    All,
}

/// Rename and/or recolour cues (`""` clears a name, `"none"` a colour).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UpdateCue {
    pub cue: CueRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone)]
pub enum CueEdit {
    Set(SetCue),
    Update(UpdateCue),
    Delete(CueRef),
    /// Replace the tracks' cues with a copy of this track's (an id from [`Device::library`]).
    Copy {
        from: u32,
    },
    /// Copy this track's cues onto the tracks that have none, each `shift_ms` later (earlier
    /// when negative): for another file of the same recording whose audio starts at another
    /// point (an MP3's encoder delay). Tracks with cues keep theirs.
    Carry {
        from: u32,
        shift_ms: i32,
    },
}

/// A cue edit as the drive's journal records it (see [`crate::edit::journal`]): a copy's source
/// by its audio file (a device path), like the tracks edited, so a replay on a later export, where
/// ids differ, finds them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CueChange {
    Set(SetCue),
    Update(UpdateCue),
    Delete(CueRef),
    Copy { from: String },
    Carry { from: String, shift_ms: i32 },
}

impl CueChange {
    fn of(edit: &CueEdit, library: &Library) -> CueChange {
        let file = |id: u32| library.track(id).map(|t| t.file_path.clone()).unwrap_or_default();
        match edit {
            CueEdit::Set(s) => CueChange::Set(s.clone()),
            CueEdit::Update(u) => CueChange::Update(u.clone()),
            CueEdit::Delete(r) => CueChange::Delete(r.clone()),
            CueEdit::Copy { from } => CueChange::Copy { from: file(*from) },
            CueEdit::Carry { from, shift_ms } => CueChange::Carry { from: file(*from), shift_ms: *shift_ms },
        }
    }

    /// `set hot cue A on 1 track(s)`.
    pub fn describe(&self, tracks: usize) -> String {
        match self {
            CueChange::Set(SetCue { hot_cue: Some(h), .. }) => format!("set hot cue {h} on {tracks} track(s)"),
            CueChange::Set(SetCue { at, .. }) => {
                format!("set a {} on {tracks} track(s)", describe(&CueRef::Memory(*at)))
            }
            CueChange::Update(u) => format!("rename or recolour {} on {tracks} track(s)", describe(&u.cue)),
            CueChange::Delete(r) => format!("delete {} on {tracks} track(s)", describe(r)),
            CueChange::Copy { from } => format!("copy the cues of {from} to {tracks} track(s)"),
            CueChange::Carry { from, shift_ms } => {
                format!("carry the cues of {from} onto {tracks} track(s) without any, {shift_ms:+} ms")
            }
        }
    }
}

/// The analysis files (keys) that hold the cues of the tracks of `library` at `files` (device
/// paths): what editing them may rewrite, for a backup.
pub(crate) fn analysis_files(device: &Device, library: &Library, files: &[String]) -> Vec<String> {
    let wanted: rustc_hash::FxHashSet<String> = files.iter().map(|f| paths::key(f).to_lowercase()).collect();
    let mut out = Vec::new();
    for t in library.tracks.iter().filter(|t| wanted.contains(&paths::key(&t.file_path).to_lowercase())) {
        let dat = paths::key(&t.analyze_path);
        if t.analyze_path.is_empty() || !paths::is_safe_key(&dat) || paths::classify(&dat) != Some(FileClass::Analysis)
        {
            continue;
        }
        for key in std::iter::once(dat.clone()).chain(ext_key(&dat)) {
            if device.checked_path(&key).is_ok_and(|p| p.is_file()) {
                out.push(key);
            }
        }
    }
    out
}

/// A cue as shown to people.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct CueInfo {
    /// `A`-`H`; `None` for memory cues.
    pub hot_cue: Option<String>,
    pub start_ms: u32,
    /// Where the loop ends, for loops.
    pub end_ms: Option<u32>,
    /// The beat of the grid the cue sits on (counting from 1), if it's on one.
    pub beat: Option<u32>,
    /// A quantised loop's length in beats: `"8"`, `"1/2"`.
    pub loop_beats: Option<String>,
    pub name: String,
    /// `#rrggbb` for a hot cue with a palette colour, a colour name (`red`) for a memory cue.
    pub color: Option<String>,
    pub active_loop: bool,
}

/// A drive track's cues.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TrackCues {
    pub track: TrackRef,
    pub bpm: f64,
    pub cues: Vec<CueInfo>,
    /// How many cue rows OneLibrary (`exportLibrary.db`) has for the track, if it lists it.
    pub onelibrary_cues: Option<usize>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CuePlan {
    pub drive: String,
    pub root: PathBuf,
    /// Tracks whose cues change.
    pub tracks: Vec<TrackCueChange>,
    /// Space needed for the backup and the rewritten files.
    pub needed_bytes: u64,
    pub free_space: Option<u64>,
    pub fits: bool,
    pub warnings: Vec<String>,
    /// The edit as the journal records it.
    #[serde(skip)]
    recorded: Option<crate::edit::Edit>,
}

impl CuePlan {
    pub fn is_noop(&self) -> bool {
        self.tracks.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TrackCueChange {
    pub track: TrackRef,
    pub before: Vec<CueInfo>,
    pub after: Vec<CueInfo>,
    /// Analysis files that change, as drive keys.
    pub files: Vec<String>,
    /// Whether the track's rows in `exportLibrary.db` change too.
    pub onelibrary: bool,
    #[serde(skip)]
    state: TrackState,
}

#[derive(Debug, Clone)]
struct TrackState {
    dat: String,
    ext: Option<String>,
    before: Vec<CuePoint>,
    after: Vec<CuePoint>,
    plus: Option<PlusState>,
}

#[derive(Debug, Clone)]
struct PlusState {
    content_id: u32,
    before: Vec<CueRow>,
    after: Vec<CueRow>,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct CueReport {
    pub tracks_changed: usize,
    pub files_written: Vec<String>,
    pub onelibrary: Option<CueRowChanges>,
    pub backup: Option<String>,
    pub fingerprint: Option<Hash>,
}

// ---------------------------------------------------------------------------------------------
// Colours
// ---------------------------------------------------------------------------------------------

/// Names for entries of rekordbox's hot cue palette ([`HOT_CUE_PALETTE`] codes).
const HOT_CUE_COLORS: [(&str, u8); 15] = [
    ("blue", 0x01),
    ("sky", 0x05),
    ("cyan", 0x09),
    ("aqua", 0x09),
    ("teal", 0x0e),
    ("green", 0x16),
    ("lime", 0x1a),
    ("yellow", 0x20),
    ("amber", 0x22),
    ("orange", 0x24),
    ("red", 0x2a),
    ("pink", 0x2d),
    ("magenta", 0x35),
    ("purple", 0x38),
    ("violet", 0x3c),
];

/// The memory cue colours (rekordbox's colour table) by name.
const MEMORY_COLORS: [(TrackColor, &str); 8] = [
    (TrackColor::Pink, "pink"),
    (TrackColor::Red, "red"),
    (TrackColor::Orange, "orange"),
    (TrackColor::Yellow, "yellow"),
    (TrackColor::Green, "green"),
    (TrackColor::Aqua, "aqua"),
    (TrackColor::Blue, "blue"),
    (TrackColor::Purple, "purple"),
];

fn parse_rgb(s: &str) -> Option<[u8; 3]> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([c(0)?, c(2)?, c(4)?])
}

fn nearest(rgb: [u8; 3], candidates: impl Iterator<Item = [u8; 3]>) -> usize {
    let d = |c: [u8; 3]| (0..3).map(|i| (i32::from(c[i]) - i32::from(rgb[i])).pow(2)).sum::<i32>();
    candidates.enumerate().min_by_key(|(_, c)| d(*c)).map_or(0, |(i, _)| i)
}

/// Give a cue a colour: a name (`red`, `aqua`, `violet`...), `#rrggbb` (the nearest colour players
/// offer), a number (a palette code 1-62 for a hot cue, a colour table row 1-8 for a memory cue),
/// or `none`. Hot cues take rekordbox's 62-colour palette, memory cues its 8 track colours.
pub fn set_color(c: &mut CuePoint, spec: &str) -> Result<()> {
    let s = spec.trim().to_ascii_lowercase();
    let bad = || Error::invalid(format!("unknown colour {spec:?} (a name like red or aqua, #rrggbb, or none)"));
    if c.hot_cue.is_some() {
        let code = match s.as_str() {
            "none" | "default" | "" => 0,
            _ => match (HOT_CUE_COLORS.iter().find(|(n, _)| *n == s), parse_rgb(&s), s.parse::<u8>()) {
                (Some((_, code)), _, _) => *code,
                (_, Some(rgb), _) => nearest(rgb, HOT_CUE_PALETTE.iter().copied()) as u8 + 1,
                (_, _, Ok(n @ 1..=62)) => n,
                _ => return Err(bad()),
            },
        };
        c.color_code = code;
        c.color_rgb = anlz::hot_cue_palette(code).unwrap_or(DEFAULT_HOT_CUE_RGB);
    } else {
        let alias = match s.as_str() {
            "cyan" | "teal" => "aqua",
            "violet" | "magenta" => "purple",
            other => other,
        };
        c.color_id = match (MEMORY_COLORS.iter().find(|(_, n)| *n == alias), parse_rgb(&s), s.parse::<u8>()) {
            _ if matches!(alias, "none" | "default" | "") => 0,
            (Some((color, _)), _, _) => color.id(),
            (_, Some(rgb), _) => MEMORY_COLORS[nearest(rgb, MEMORY_COLORS.iter().map(|(c, _)| c.rgb()))].0.id(),
            (_, _, Ok(n @ 1..=8)) => n,
            _ => return Err(bad()),
        };
    }
    Ok(())
}

fn color_label(c: &CuePoint) -> Option<String> {
    let hex = |[r, g, b]: [u8; 3]| format!("#{r:02x}{g:02x}{b:02x}");
    if c.hot_cue.is_some() {
        match anlz::hot_cue_palette(c.color_code) {
            Some(rgb) => Some(hex(rgb)),
            None if c.color_rgb != [0; 3] && c.color_rgb != DEFAULT_HOT_CUE_RGB => Some(hex(c.color_rgb)),
            None => None,
        }
    } else {
        let color = TrackColor::from_id(c.color_id)?;
        MEMORY_COLORS.iter().find(|(m, _)| *m == color).map(|(_, name)| name.to_string())
    }
}

fn info(c: &CuePoint, beats: &[Beat]) -> CueInfo {
    CueInfo {
        hot_cue: c.hot_cue.map(|h| HotCue(h).to_string()),
        start_ms: c.start_ms,
        end_ms: c.end_ms,
        beat: beats.iter().position(|b| b.time_ms.abs_diff(c.start_ms) <= 1).map(|i| i as u32 + 1),
        loop_beats: c.loop_beats.map(|(n, d)| if d == 1 { n.to_string() } else { format!("{n}/{d}") }),
        name: c.comment.clone(),
        color: color_label(c),
        active_loop: c.active_loop && c.is_loop(),
    }
}

// ---------------------------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------------------------

/// A track's cues as its analysis files hold them.
struct Loaded {
    dat: String,
    ext: Option<String>,
    dat_bytes: Vec<u8>,
    ext_bytes: Option<Vec<u8>>,
    cues: Vec<CuePoint>,
    beats: Vec<Beat>,
}

/// `.../ANLZ0000.DAT` -> `.../ANLZ0000.EXT`.
fn ext_key(dat: &str) -> Option<String> {
    let stem =
        dat.len().checked_sub(4).filter(|&i| dat.is_char_boundary(i) && dat[i..].eq_ignore_ascii_case(".DAT"))?;
    Some(format!("{}.EXT", &dat[..stem]))
}

fn read_optional(device: &Device, key: &str) -> Result<Option<Vec<u8>>> {
    let p = device.checked_path(key)?;
    match std::fs::read(&p) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::Io { path: p, source: e }),
    }
}

fn load(device: &Device, t: &Track) -> Result<Loaded> {
    let no_analysis = || {
        Error::invalid(format!(
            "{} has no analysis files (rekordbox hasn't analysed it), so it can't hold cues",
            t.display_name()
        ))
    };
    if t.analyze_path.is_empty() {
        return Err(no_analysis());
    }
    let dat = paths::key(&t.analyze_path);
    if !paths::is_safe_key(&dat) || paths::classify(&dat) != Some(FileClass::Analysis) {
        return Err(Error::invalid(format!("refusing analysis path {:?} of {}", t.analyze_path, t.display_name())));
    }
    let dat_bytes = read_optional(device, &dat)?.ok_or_else(no_analysis)?;
    let ext = ext_key(&dat);
    let ext_bytes = match &ext {
        Some(k) => read_optional(device, k)?,
        None => None,
    };
    let cues = anlz::read_cue_points(&dat_bytes, ext_bytes.as_deref())?;
    Ok(Loaded {
        dat,
        ext: ext.filter(|_| ext_bytes.is_some()),
        beats: anlz::beat_grid(&dat_bytes).unwrap_or_default(),
        dat_bytes,
        ext_bytes,
        cues,
    })
}

/// OneLibrary's track ids by audio file.
fn plus_ids(lib: &crate::model::Library) -> FxHashMap<String, u32> {
    lib.tracks.iter().map(|t| (paths::key(&t.file_path).to_lowercase(), t.id)).collect()
}

/// List tracks' cues (ids from [`Device::library`]).
pub fn list(device: &Device, track_ids: &[u32]) -> Result<Vec<TrackCues>> {
    list_in(device, &device.library()?, track_ids)
}

/// [`list`], with the drive's library read already.
pub fn list_in(device: &Device, library: &Library, track_ids: &[u32]) -> Result<Vec<TrackCues>> {
    let mut warnings = Vec::new();
    let plus = crate::clean::open_plus(device, device.has_device_library(), &mut warnings)?;
    let by_path = plus.as_ref().map(|(lib, _)| plus_ids(lib));
    let mut out = Vec::new();
    for id in track_ids {
        let t = library.track(*id).ok_or_else(|| Error::NotFound(format!("track #{id}")))?;
        let mut track_warnings = warnings.clone();
        let (cues, beats) = match load(device, t) {
            Ok(l) => (l.cues, l.beats),
            Err(e) => {
                track_warnings.push(e.to_string());
                (Vec::new(), Vec::new())
            }
        };
        let onelibrary_cues = match (&plus, &by_path) {
            (Some((_, one)), Some(ids)) => match ids.get(&paths::key(&t.file_path).to_lowercase()) {
                Some(cid) => Some(one.cue_rows(*cid)?.len()),
                None => None,
            },
            _ => None,
        };
        out.push(TrackCues {
            track: t.into(),
            bpm: t.bpm,
            cues: cues.iter().map(|c| info(c, &beats)).collect(),
            onelibrary_cues,
            warnings: track_warnings,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Editing
// ---------------------------------------------------------------------------------------------

/// A cue after the edit, and where it came from: one of the track's own cues (`origin`) or one
/// of the copied track's (`source`).
#[derive(Debug, Clone)]
struct Draft {
    cue: CuePoint,
    origin: Option<usize>,
    source: Option<usize>,
}

/// A position as milliseconds, and the beat it's on.
fn resolve(at: CuePosition, snap: bool, beats: &[Beat]) -> Result<(u32, Option<usize>)> {
    let no_grid = || Error::invalid("the track has no beat grid; give a time instead");
    match at {
        CuePosition::Beat(n) => {
            if beats.is_empty() {
                return Err(no_grid());
            }
            let i = (n as usize)
                .checked_sub(1)
                .filter(|i| *i < beats.len())
                .ok_or_else(|| Error::invalid(format!("beat {n} isn't on the beat grid (beats 1-{})", beats.len())))?;
            Ok((beats[i].time_ms, Some(i)))
        }
        CuePosition::Ms(ms) if snap => {
            if beats.is_empty() {
                return Err(no_grid());
            }
            let i = (0..beats.len()).min_by_key(|i| beats[*i].time_ms.abs_diff(ms)).expect("non-empty");
            Ok((beats[i].time_ms, Some(i)))
        }
        CuePosition::Ms(ms) => Ok((ms, beats.iter().position(|b| b.time_ms == ms))),
    }
}

/// A loop length in beats as rekordbox stores it: n/1, or 1/2, 1/4 ... 1/32.
fn beat_fraction(beats: f64) -> Result<(u16, u16)> {
    let bad = || Error::invalid(format!("a loop is 1-512 beats, or 1/2, 1/4 ... 1/32 of one, not {beats}"));
    if (1.0..=512.0).contains(&beats) && beats.fract() == 0.0 {
        return Ok((beats as u16, 1));
    }
    let d = 1.0 / beats;
    if beats > 0.0 && d.fract() == 0.0 && (2.0..=32.0).contains(&d) && (d as u16).is_power_of_two() {
        return Ok((1, d as u16));
    }
    Err(bad())
}

/// Where a loop of `(n, d)` beats from `start` ends: on the grid for whole beats when it reaches
/// that far, otherwise at the tempo where it starts.
fn loop_end(start: u32, beat: Option<usize>, (n, d): (u16, u16), beats: &[Beat], track_bpm: f64) -> Result<u32> {
    if let Some(b) = beat.filter(|_| d == 1).and_then(|i| beats.get(i + usize::from(n))) {
        return Ok(b.time_ms);
    }
    let bpm = beats.iter().rev().find(|b| b.time_ms <= start).or(beats.first()).map_or(track_bpm, |b| b.bpm);
    if bpm <= 0.0 {
        return Err(Error::invalid("the track's tempo isn't known, so a loop can't be sized in beats"));
    }
    Ok(start + (f64::from(n) / f64::from(d) * 60_000.0 / bpm).round() as u32)
}

fn check_in_track(t: &Track, ms: u32) -> Result<()> {
    // Durations are whole seconds.
    if t.duration_secs > 0 && u64::from(ms) >= (u64::from(t.duration_secs) + 1) * 1000 {
        return Err(Error::invalid(format!(
            "{} is past the end of {} ({})",
            clock(ms),
            t.display_name(),
            clock(t.duration_secs * 1000)
        )));
    }
    Ok(())
}

/// `m:ss.mmm`
pub fn clock(ms: u32) -> String {
    format!("{}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

fn describe(r: &CueRef) -> String {
    match r {
        CueRef::Hot(h) => format!("hot cue {h}"),
        CueRef::Memory(CuePosition::Ms(ms)) => format!("memory cue at {}", clock(*ms)),
        CueRef::Memory(CuePosition::Beat(b)) => format!("memory cue at beat {b}"),
        CueRef::All => "cues".into(),
    }
}

/// The drafts `r` refers to.
fn find(drafts: &[Draft], r: &CueRef, beats: &[Beat], t: &Track) -> Result<Vec<usize>> {
    let found: Vec<usize> = match r {
        CueRef::All => return Ok((0..drafts.len()).collect()),
        CueRef::Hot(h) => drafts.iter().position(|d| d.cue.hot_cue == Some(h.0)).into_iter().collect(),
        CueRef::Memory(at) => {
            let (ms, _) = resolve(*at, false, beats)?;
            (0..drafts.len())
                .filter(|i| drafts[*i].cue.hot_cue.is_none())
                .filter(|i| drafts[*i].cue.start_ms.abs_diff(ms) <= FIND_TOLERANCE_MS)
                .min_by_key(|i| drafts[*i].cue.start_ms.abs_diff(ms))
                .into_iter()
                .collect()
        }
    };
    if found.is_empty() {
        return Err(Error::NotFound(format!("{} on {}", describe(r), t.display_name())));
    }
    Ok(found)
}

fn set(drafts: &mut Vec<Draft>, t: &Track, beats: &[Beat], s: &SetCue) -> Result<()> {
    if s.hot_cue.is_some_and(|h| h.0 >= HotCue::COUNT) {
        return Err(Error::invalid("hot cues are A-H"));
    }
    let (start, beat) = resolve(s.at, s.snap, beats)?;
    check_in_track(t, start)?;
    let (end, loop_beats) = match s.loop_beats {
        Some(n) => {
            let fraction = beat_fraction(n)?;
            let end = loop_end(start, beat, fraction, beats, t.bpm)?;
            check_in_track(t, end)?;
            (Some(end), Some(fraction))
        }
        None => (None, None),
    };
    let slot = s.hot_cue.map(|h| h.0);
    let existing = match slot {
        Some(_) => drafts.iter().position(|d| d.cue.hot_cue == slot),
        None => drafts.iter().position(|d| d.cue.hot_cue.is_none() && d.cue.start_ms.abs_diff(start) <= 1),
    };
    let i = existing.unwrap_or_else(|| {
        let rgb = if slot.is_some() { DEFAULT_HOT_CUE_RGB } else { [0; 3] };
        let cue = CuePoint { hot_cue: slot, color_rgb: rgb, ..Default::default() };
        drafts.push(Draft { cue, origin: None, source: None });
        drafts.len() - 1
    });
    let c = &mut drafts[i].cue;
    c.start_ms = start;
    c.end_ms = end;
    c.loop_beats = loop_beats;
    c.active_loop &= end.is_some();
    if let Some(n) = &s.name {
        c.comment = n.clone();
    }
    if let Some(color) = &s.color {
        set_color(c, color)?;
    }
    Ok(())
}

/// The cues a track has after `edit`.
fn edited(t: &Track, loaded: &Loaded, edit: &CueEdit, source: Option<&Loaded>) -> Result<Vec<Draft>> {
    let mut drafts: Vec<Draft> =
        loaded.cues.iter().enumerate().map(|(i, c)| Draft { cue: c.clone(), origin: Some(i), source: None }).collect();
    match edit {
        CueEdit::Set(s) => set(&mut drafts, t, &loaded.beats, s)?,
        CueEdit::Update(u) => {
            for i in find(&drafts, &u.cue, &loaded.beats, t)? {
                if let Some(n) = &u.name {
                    drafts[i].cue.comment = n.clone();
                }
                if let Some(color) = &u.color {
                    set_color(&mut drafts[i].cue, color)?;
                }
            }
        }
        CueEdit::Delete(r) => {
            let gone = find(&drafts, r, &loaded.beats, t)?;
            let mut i = 0;
            drafts.retain(|_| {
                i += 1;
                !gone.contains(&(i - 1))
            });
        }
        CueEdit::Carry { .. } if !loaded.cues.is_empty() => {}
        CueEdit::Copy { .. } | CueEdit::Carry { .. } => {
            let source = source.expect("copy has a source");
            let shift = match edit {
                CueEdit::Carry { shift_ms, .. } => i64::from(*shift_ms),
                _ => 0,
            };
            let moved = |ms: u32| {
                u32::try_from(i64::from(ms) + shift)
                    .map_err(|_| Error::invalid(format!("a cue would move before the start of {}", t.display_name())))
            };
            let mut used = vec![false; loaded.cues.len()];
            drafts = source
                .cues
                .iter()
                .enumerate()
                .map(|(s, c)| {
                    let c = &CuePoint {
                        start_ms: moved(c.start_ms)?,
                        end_ms: c.end_ms.map(moved).transpose()?,
                        ..c.clone()
                    };
                    // A cue the track already has (same slot and position) keeps its identity.
                    let origin = (0..loaded.cues.len()).find(|o| {
                        !used[*o]
                            && loaded.cues[*o].hot_cue == c.hot_cue
                            && loaded.cues[*o].start_ms.abs_diff(c.start_ms) <= 1
                    });
                    if let Some(o) = origin {
                        used[o] = true;
                    }
                    for x in [Some(c.start_ms), c.end_ms].into_iter().flatten() {
                        check_in_track(t, x)?;
                    }
                    Ok(Draft { cue: c.clone(), origin, source: Some(s) })
                })
                .collect::<Result<_>>()?;
        }
    }
    drafts.sort_by_key(|d| (d.cue.hot_cue.map_or(0, |h| u32::from(h) + 1), d.cue.start_ms));
    Ok(drafts)
}

/// Everything a cue's entries store, to compare cue lists regardless of order and storage.
type CueKey = (Option<u8>, u32, Option<u32>, Option<(u16, u16)>, String, u8, u8, [u8; 3], bool);

fn key(c: &CuePoint, named: bool) -> CueKey {
    let active = c.active_loop && c.is_loop();
    match named {
        true => (
            c.hot_cue,
            c.start_ms,
            c.end_ms,
            c.loop_beats,
            c.comment.clone(),
            c.color_id,
            c.color_code,
            c.color_rgb,
            active,
        ),
        // Without an .EXT only `PCPT` entries exist: no names, colours or loop lengths.
        false => (c.hot_cue, c.start_ms, c.end_ms, None, String::new(), 0, 0, [0; 3], active),
    }
}

fn same_cues(a: &[CuePoint], b: &[CuePoint], named: bool) -> bool {
    let keys = |l: &[CuePoint]| {
        let mut k: Vec<CueKey> = l.iter().map(|c| key(c, named)).collect();
        k.sort();
        k
    };
    keys(a) == keys(b)
}

fn close(usec: i64, ms: u32) -> bool {
    (usec - i64::from(ms) * 1000).abs() <= 1000
}

/// Give a OneLibrary row with no cue rows (`id`, made for `t` from `export.pdb`'s row by convert
/// or merge) the cues in `t`'s analysis files on `device`: how many it got, or `None` when the
/// analysis files can't be read. A track without analysis files (or with them somewhere [`load`]
/// refuses to read them from) gets none.
pub(crate) fn add_rows_from_analysis(
    one: &mut OneLibrary,
    id: u32,
    device: &Device,
    t: &Track,
) -> Result<Option<usize>> {
    let dat = paths::key(&t.analyze_path);
    let readable = paths::is_safe_key(&dat) && paths::classify(&dat) == Some(FileClass::Analysis);
    if t.analyze_path.is_empty() || !readable || !device.checked_path(&dat).is_ok_and(|p| p.is_file()) {
        return Ok(Some(0));
    }
    let Ok(loaded) = load(device, t) else { return Ok(None) };
    let rows: Vec<CueRow> = loaded.cues.iter().map(|c| row_for(c, CueRow::default())).collect();
    if rows.is_empty() {
        return Ok(Some(0));
    }
    Ok(Some(one.replace_cues(id, &rows)?.added))
}

/// `c` as a row of the `cue` table, starting from `row` (its current row, a copied one, or none).
fn row_for(c: &CuePoint, mut row: CueRow) -> CueRow {
    row.hot_cue = c.hot_cue;
    if !close(row.in_usec, c.start_ms) || row.stored.is_empty() {
        row.in_usec = i64::from(c.start_ms) * 1000;
    }
    row.out_usec = match (row.out_usec, c.end_ms) {
        (Some(o), Some(e)) if close(o, e) => Some(o),
        (_, e) => e.map(|e| i64::from(e) * 1000),
    };
    row.comment = c.comment.clone();
    row.color = if c.hot_cue.is_some() { c.color_code } else { c.color_id };
    row.active_loop = c.active_loop && c.is_loop();
    row.loop_beats = c.loop_beats;
    row
}

/// Match the analysis files' cues to the track's rows: the row index for each cue.
fn match_rows(cues: &[CuePoint], rows: &[CueRow]) -> Vec<Option<usize>> {
    let mut used = vec![false; rows.len()];
    cues.iter()
        .map(|c| {
            let r = (0..rows.len())
                .find(|r| !used[*r] && rows[*r].hot_cue == c.hot_cue && close(rows[*r].in_usec, c.start_ms));
            if let Some(r) = r {
                used[r] = true;
            }
            r
        })
        .collect()
}

/// What the copied track's rows can lend: its row for each of its cues, if they share the
/// audio file (the seek columns are only right for the same file).
type SourceRows = Vec<Option<CueRow>>;

/// Plan an edit of tracks' cues (ids from [`Device::library`]). Set, update and delete edit one
/// track (delete `All` any number); copy replaces each track's cues with the source's.
pub fn plan(device: &Device, track_ids: &[u32], edit: &CueEdit, ctx: &Ctx<'_>) -> Result<CuePlan> {
    if !device.has_export() {
        return Err(Error::NotAnExport(device.root().to_path_buf()));
    }
    if track_ids.is_empty() {
        return Err(Error::invalid("no tracks selected"));
    }
    let one_track = !matches!(edit, CueEdit::Copy { .. } | CueEdit::Carry { .. } | CueEdit::Delete(CueRef::All));
    if one_track && track_ids.len() > 1 {
        return Err(Error::invalid(format!("{} tracks selected; pick one", track_ids.len())));
    }
    let library = device.library()?;
    let track = |id: u32| library.track(id).ok_or_else(|| Error::NotFound(format!("track #{id}")));
    let mut warnings = Vec::new();
    let plus = crate::clean::open_plus(device, device.has_device_library(), &mut warnings)?;
    let by_path = plus.as_ref().map(|(lib, _)| plus_ids(lib));
    let plus_id = |t: &Track| by_path.as_ref().and_then(|m| m.get(&paths::key(&t.file_path).to_lowercase()).copied());

    let source = match edit {
        CueEdit::Copy { from } | CueEdit::Carry { from, .. } => {
            let s = track(*from)?;
            let loaded = load(device, s)?;
            let rows: SourceRows = match (plus_id(s), &plus) {
                (Some(cid), Some((_, one))) => {
                    let rows = one.cue_rows(cid)?;
                    match_rows(&loaded.cues, &rows).into_iter().map(|r| r.map(|r| rows[r].clone())).collect()
                }
                _ => vec![None; loaded.cues.len()],
            };
            Some((s, loaded, rows))
        }
        _ => None,
    };

    let mut tracks = Vec::new();
    for id in track_ids {
        ctx.check()?;
        let t = track(*id)?;
        if source.as_ref().is_some_and(|(s, _, _)| s.id == t.id) {
            continue;
        }
        let loaded = load(device, t)?;
        let drafts = edited(t, &loaded, edit, source.as_ref().map(|(_, l, _)| l))?;
        let after: Vec<CuePoint> = drafts.iter().map(|d| d.cue.clone()).collect();
        if same_cues(&loaded.cues, &after, true) {
            continue;
        }
        let mut files = Vec::new();
        let dat_file = if loaded.ext.is_some() { CueFile::Dat } else { CueFile::DatAlone };
        if anlz::write_cue_points(&loaded.dat_bytes, dat_file, &after)? != loaded.dat_bytes {
            files.push(loaded.dat.clone());
        }
        if let (Some(k), Some(b)) = (&loaded.ext, &loaded.ext_bytes)
            && anlz::write_cue_points(b, CueFile::Ext, &after)? != *b
        {
            files.push(k.clone());
        }

        let plus_state = match (&plus, plus_id(t)) {
            (Some((_, one)), Some(content_id)) => {
                let rows = one.cue_rows(content_id)?;
                let matched = match_rows(&loaded.cues, &rows);
                let unmatched = rows.len() - matched.iter().flatten().count();
                if unmatched > 0 {
                    warnings.push(format!(
                        "exportLibrary.db has {unmatched} cue(s) for {} that its analysis files don't; they'll be replaced by the analysis files' cues",
                        t.display_name()
                    ));
                }
                let same_audio = source
                    .as_ref()
                    .is_some_and(|(s, _, _)| paths::key(&s.file_path).eq_ignore_ascii_case(&paths::key(&t.file_path)));
                let after_rows = drafts
                    .iter()
                    .map(|d| {
                        let own = d.origin.and_then(|o| matched[o]).map(|r| rows[r].clone());
                        let lent = d
                            .source
                            .filter(|_| same_audio)
                            .and_then(|s| source.as_ref()?.2.get(s)?.clone())
                            .map(|r| CueRow { cue_id: None, ..r });
                        row_for(&d.cue, own.or(lent).unwrap_or_default())
                    })
                    .collect();
                Some(PlusState { content_id, before: rows, after: after_rows })
            }
            (Some(_), None) => {
                warnings.push(format!(
                    "{} isn't in exportLibrary.db; players that read OneLibrary won't see its new cues",
                    t.display_name()
                ));
                None
            }
            _ => None,
        };
        if loaded.ext.is_none() && after.iter().any(|c| !c.comment.is_empty() || c.color_id != 0 || c.color_code != 0) {
            warnings.push(match plus_state {
                Some(_) => format!(
                    "{} has no .EXT analysis file, so its cues' names and colours reach only players that read OneLibrary",
                    t.display_name()
                ),
                None => format!("{} has no .EXT analysis file, so its cues' names and colours can't be stored", t.display_name()),
            });
        }
        if files.is_empty() && plus_state.is_none() {
            continue;
        }
        tracks.push(TrackCueChange {
            track: t.into(),
            before: loaded.cues.iter().map(|c| info(c, &loaded.beats)).collect(),
            after: after.iter().map(|c| info(c, &loaded.beats)).collect(),
            files,
            onelibrary: plus_state.is_some(),
            state: TrackState { dat: loaded.dat, ext: loaded.ext, before: loaded.cues, after, plus: plus_state },
        });
    }

    // The backup copies the databases and the analysis files; each file is staged before it
    // replaces the old one.
    let cluster = fsutil::cluster_size(device.root());
    let round = |n: u64| n.div_ceil(cluster) * cluster;
    let size = |key: &str| std::fs::metadata(device.path(key)).map(|m| m.len()).unwrap_or(0);
    let needed_bytes = match tracks.is_empty() {
        true => 0,
        false => {
            let databases: u64 = paths::DATABASE_FILES.iter().map(|k| round(size(k))).sum();
            let analysis: u64 = tracks.iter().flat_map(|t| &t.files).map(|k| 2 * round(size(k))).sum();
            let staged_plus = match tracks.iter().any(|t| t.onelibrary) {
                true => round(size(paths::LIBRARY_PLUS_DB) + size(paths::LIBRARY_PLUS_WAL)),
                false => 0,
            };
            databases + analysis + staged_plus
        }
    };
    let free_space = fsutil::available_space(device.root());
    let fits = free_space.is_none_or(|f| needed_bytes <= f);
    if !fits {
        warnings.push(format!(
            "the edit needs about {needed_bytes} bytes but only {} are free on the drive",
            free_space.unwrap_or(0)
        ));
    }
    warnings.dedup();
    // Recorded with the tracks whose cues change.
    let files: Vec<String> = tracks.iter().map(|c| c.track.file_path.clone()).collect();
    let recorded = crate::edit::Edit::Cues { tracks: files, change: CueChange::of(edit, &library) };
    Ok(CuePlan {
        drive: device.name(),
        root: device.root().to_path_buf(),
        tracks,
        needed_bytes,
        free_space,
        fits,
        warnings,
        recorded: Some(recorded),
    })
}

/// Make a planned edit, and record it in the drive's journal.
pub fn apply(plan: &CuePlan, device: &mut Device, ctx: &Ctx<'_>) -> Result<CueReport> {
    if plan.is_noop() {
        return Ok(CueReport::default());
    }
    if !plan.fits {
        return Err(Error::NoSpace {
            path: plan.root.clone(),
            needed: plan.needed_bytes,
            available: plan.free_space.unwrap_or(0),
        });
    }
    let _lock = crate::commit::lock(device, None)?;
    crate::edit::journal::list(device)?; // one this build can add to
    let changes: Vec<String> = match &plan.recorded {
        Some(crate::edit::Edit::Cues { tracks, change }) => vec![change.describe(tracks.len())],
        _ => Vec::new(),
    };
    let recorded: Vec<crate::edit::Edit> = plan.recorded.iter().cloned().collect();
    apply_locked(plan, device, Some((&recorded, &changes)), ctx)
}

/// Replay a recorded edit, under the backup the replay made (see [`crate::edit::apply`]). One
/// that no longer applies is skipped with a warning; an error is a failure while writing.
pub(crate) fn replay(device: &mut Device, files: &[String], change: &CueChange, ctx: &Ctx<'_>) -> Result<Replayed> {
    let mut out = Replayed::default();
    let skip = |e: &dyn std::fmt::Display| {
        format!("skipped an edit that no longer applies ({}): {e}", change.describe(files.len()))
    };
    let library = device.library()?;
    let by_key: FxHashMap<String, u32> =
        library.tracks.iter().map(|t| (paths::key(&t.file_path).to_lowercase(), t.id)).collect();
    let id = |f: &str| by_key.get(&paths::key(f).to_lowercase()).copied();
    let ids: Vec<u32> = files.iter().filter_map(|f| id(f)).collect();
    if let Some(example) = files.iter().find(|f| id(f).is_none()) {
        out.tracks_gone(files.len() - ids.len(), example, &change.describe(files.len()));
    }
    let edit = match change {
        CueChange::Set(s) => CueEdit::Set(s.clone()),
        CueChange::Update(u) => CueEdit::Update(u.clone()),
        CueChange::Delete(r) => CueEdit::Delete(r.clone()),
        CueChange::Copy { from } | CueChange::Carry { from, .. } => match (id(from), change) {
            (Some(from), CueChange::Carry { shift_ms, .. }) => CueEdit::Carry { from, shift_ms: *shift_ms },
            (Some(from), _) => CueEdit::Copy { from },
            (None, _) => {
                out.warnings.push(skip(&format!("{from} isn't on the drive any more")));
                return Ok(out);
            }
        },
    };
    let plan = match plan(device, &ids, &edit, ctx) {
        Ok(plan) => plan,
        Err(e) => {
            out.warnings.push(skip(&e));
            return Ok(out);
        }
    };
    out.warnings.extend(plan.warnings.iter().cloned());
    if !plan.is_noop() {
        if !plan.fits {
            return Err(Error::NoSpace {
                path: plan.root.clone(),
                needed: plan.needed_bytes,
                available: plan.free_space.unwrap_or(0),
            });
        }
        let report = apply_locked(&plan, device, None, ctx)?;
        out.databases = report.files_written.clone();
        if report.onelibrary.is_some() {
            out.databases.push(paths::LIBRARY_PLUS_DB.rsplit('/').next().unwrap_or_default().to_string());
        }
        out.fingerprint = report.fingerprint;
    }
    Ok(out)
}

/// [`apply`] with the drive locked already. With `journal` (the edit as recorded, and what it
/// did), the drive is backed up first and the edit recorded; a replay has backed it up already.
fn apply_locked(
    plan: &CuePlan,
    device: &mut Device,
    journal: Option<(&[crate::edit::Edit], &[String])>,
    ctx: &Ctx<'_>,
) -> Result<CueReport> {
    let changed = || Error::stale(device.name(), "changed");

    // 1. The cues must still be as planned (rekordbox may have exported again). The new files are
    //    made and checked in memory before anything is written.
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for t in &plan.tracks {
        let s = &t.state;
        let dat = read_optional(device, &s.dat)?.ok_or_else(changed)?;
        let ext = match ext_key(&s.dat) {
            Some(k) => read_optional(device, &k)?.map(|b| (k, b)),
            None => None,
        };
        if ext.as_ref().map(|(k, _)| k) != s.ext.as_ref()
            || anlz::read_cue_points(&dat, ext.as_ref().map(|(_, b)| b.as_slice()))? != s.before
        {
            return Err(changed());
        }
        let dat_file = if ext.is_some() { CueFile::Dat } else { CueFile::DatAlone };
        let new_dat = anlz::write_cue_points(&dat, dat_file, &s.after)?;
        let new_ext = ext.as_ref().map(|(_, b)| anlz::write_cue_points(b, CueFile::Ext, &s.after)).transpose()?;
        // Never write analysis files we can't read back.
        let back = anlz::read_cue_points(&new_dat, new_ext.as_deref())?;
        if !same_cues(&back, &s.after, ext.is_some()) || anlz::beat_grid(&new_dat)? != anlz::beat_grid(&dat)? {
            return Err(Error::parse(device.path(&s.dat), "rewritten cues failed verification; drive left unchanged"));
        }
        if new_dat != dat {
            files.push((s.dat.clone(), new_dat));
        }
        if let (Some((k, old)), Some(new)) = (ext, new_ext)
            && new != old
        {
            files.push((k, new));
        }
    }
    let plus_tracks: Vec<&PlusState> = plan.tracks.iter().filter_map(|t| t.state.plus.as_ref()).collect();
    let mut report = CueReport { tracks_changed: plan.tracks.len(), ..Default::default() };
    let plus_bytes = match plus_tracks.is_empty() {
        true => None,
        false => {
            let mut one = device.library_plus()?;
            for p in &plus_tracks {
                if one.cue_rows(p.content_id)? != p.before {
                    return Err(changed());
                }
            }
            let dangling = one.dangling_bank_list_entries()?;
            let mut changes = CueRowChanges::default();
            for p in &plus_tracks {
                changes += one.replace_cues(p.content_id, &p.after)?;
            }
            let bytes = one.to_encrypted()?;
            let check = OneLibrary::from_encrypted(&bytes, None)?;
            for p in &plus_tracks {
                let rows = check.cue_rows(p.content_id)?;
                let kept = |r: &CueRow| r.cue_id.is_none_or(|id| rows.iter().any(|x| x.cue_id == Some(id)));
                if rows.len() != p.after.len()
                    || !p.after.iter().all(|a| kept(a) && rows.iter().any(|r| r.same_as(a)))
                    || check.dangling_bank_list_entries()? > dangling
                {
                    return Err(Error::OneLibrary("edited cues failed verification; drive left unchanged".into()));
                }
            }
            report.onelibrary = Some(changes);
            Some(bytes)
        }
    };
    ctx.check()?;

    // 2. Back up (the databases and the analysis files replaced), swap the new files in, then
    // the database, and record the new state (and the edit).
    let changes = Changes {
        databases: NewDatabases { plus: plus_bytes, ..Default::default() },
        files: files.into_iter().map(|(key, bytes)| NewFile::bytes(key, bytes)).collect(),
        ..Default::default()
    };
    let mut opts = Options::new(LineageOp::Edit, "edit cues");
    if journal.is_some() {
        opts = opts.backup("before editing cues");
    }
    opts.journal = journal;
    let done = crate::commit::commit(device, changes, opts, ctx)?;
    report.backup = done.backup;
    report.files_written = done.written;
    report.fingerprint = done.fingerprint;
    Ok(report)
}

#[cfg(test)]
mod tests;
