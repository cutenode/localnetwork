//! Reading `collection.nml` back: the tracks, their markers and the playlists, as the module docs
//! describe them. Traktor Pro 3 and 4 write more than this crate does; what's read of it follows
//! Mixxx's reader (`src/library/traktor/traktorfeature.cpp`), the attributes traktor-nml-utils
//! lists from real collections (`traktor_nml_utils/models/collection.py`) and Traktor's manual
//! ("Working with Cue Points"):
//!
//! * `CUE_V2`'s `TYPE` 1 is a fade-in marker, 2 a fade-out marker and 3 a load marker. Traktor
//!   colours markers by type (cues blue, fade markers orange, load markers yellow, loops green,
//!   grid markers white); a `COLOR` attribute is read too, when it's `#rrggbb`. A grid marker may
//!   hold a `GRID` with its own `BPM`. An unnamed marker is called `n.n.`.
//! * the outermost playlist node is `$ROOT`; smart playlists are `NODE TYPE="SMARTLIST"`, which
//!   hold rules rather than tracks.

use loclib_model::analysis::Beat;
use loclib_model::harmony::Camelot;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

use super::{Location, key_of};
use crate::{Error, Result};

/// The fastest tempo a beat grid is laid down at: rekordbox's grids hold hundredths of a BPM in
/// 16 bits.
const MAX_BPM: f64 = 655.35;

/// What [`read`] found in a collection.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Collection {
    /// Its tracks, in the file's order.
    pub entries: Vec<Entry>,
    /// Its playlists, in the file's order, depth first. Folders are the paths above them.
    pub lists: Vec<List>,
    /// What it left out, a line each.
    pub warnings: Vec<String>,
}

/// A track (`COLLECTION`'s `ENTRY`): each field empty when the file doesn't say.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entry {
    pub location: Location,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub label: String,
    pub comment: String,
    pub remixer: String,
    /// `INFO KEY`: the key as Traktor shows it, in the notation it was set to show.
    pub key_text: String,
    /// `MUSICAL_KEY VALUE`.
    pub key: Option<Camelot>,
    /// `TEMPO BPM`.
    pub bpm: Option<f64>,
    /// `PLAYTIME_FLOAT`, else `PLAYTIME`.
    pub duration_secs: Option<f64>,
    /// 0 to 5 stars, from `RANKING` (51 a star).
    pub rating: u8,
    /// `RELEASE_DATE`'s year.
    pub year: u16,
    /// Its `CUE_V2`s, in the file's order.
    pub markers: Vec<Marker>,
}

/// A `CUE_V2`: a cue, a loop or another of Traktor's markers.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub kind: MarkerKind,
    /// Empty for Traktor's unnamed `n.n.`.
    pub name: String,
    pub start_ms: f64,
    /// A loop's length; 0 for the others.
    pub len_ms: f64,
    /// `HOTCUE`: its pad, 0 for the first; `None` for -1, none.
    pub hot_cue: Option<u8>,
    /// `COLOR`, when it's a colour (`#rrggbb`, in lowercase).
    pub color: Option<String>,
    /// A grid marker's own tempo (its `GRID BPM`).
    pub bpm: Option<f64>,
}

/// A marker's `TYPE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerKind {
    Cue,
    FadeIn,
    FadeOut,
    Load,
    /// A grid marker: a downbeat, from which the grid runs at its tempo.
    Grid,
    Loop,
}

impl MarkerKind {
    /// The kind `TYPE` numbers.
    fn of(value: u8) -> Option<MarkerKind> {
        let kinds = [
            MarkerKind::Cue,
            MarkerKind::FadeIn,
            MarkerKind::FadeOut,
            MarkerKind::Load,
            MarkerKind::Grid,
            MarkerKind::Loop,
        ];
        kinds.get(usize::from(value)).copied()
    }

    /// The colour Traktor shows the kind in, by a name cue edits take; `None` for a grid
    /// marker's white, which rekordbox's cues don't have.
    pub fn colour(self) -> Option<&'static str> {
        match self {
            MarkerKind::Cue => Some("blue"),
            MarkerKind::FadeIn | MarkerKind::FadeOut => Some("orange"),
            MarkerKind::Load => Some("yellow"),
            MarkerKind::Grid => None,
            MarkerKind::Loop => Some("green"),
        }
    }
}

/// A playlist (`NODE TYPE="PLAYLIST"`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct List {
    /// The folders above it, then its name.
    pub path: Vec<String>,
    /// Its tracks in order, by [`Location::key`].
    pub keys: Vec<String>,
}

impl Entry {
    /// The beat grid its grid markers lay down: from each (a downbeat), at its tempo or the
    /// track's, to the next or the end of the track, and from the first back to the start. Empty
    /// without a grid marker; why there's none when the track's length isn't known or a tempo
    /// isn't one a grid holds.
    pub fn beats(&self) -> Result<Vec<Beat>, &'static str> {
        let mut grid: Vec<&Marker> = self.markers.iter().filter(|m| m.kind == MarkerKind::Grid).collect();
        if grid.is_empty() {
            return Ok(Vec::new());
        }
        grid.sort_by(|a, b| a.start_ms.total_cmp(&b.start_ms));
        let end = self.duration_secs.map(|s| s * 1000.0).filter(|&ms| ms > 0.0 && ms <= f64::from(u32::MAX));
        let end = end.ok_or("the track's length isn't known")?;
        let mut beats: Vec<Beat> = Vec::new();
        for (i, m) in grid.iter().enumerate() {
            let bpm = m.bpm.or(self.bpm).filter(|b| *b > 0.0 && *b <= MAX_BPM);
            let bpm = bpm.ok_or("its tempo isn't more than 0 and at most 655.35 BPM")?;
            let step = 60_000.0 / bpm;
            let until = grid.get(i + 1).map_or(end, |next| next.start_ms.min(end));
            // The first beat at or after the start of the track; past the first marker, from it.
            let from = (-m.start_ms / step).ceil() as i64;
            for k in if i == 0 { from } else { from.max(0) }.. {
                let at = m.start_ms + k as f64 * step;
                if at >= until {
                    break;
                }
                let time_ms = at.round() as u32;
                // Two markers closer than a millisecond: the later one's beat goes.
                if beats.last().is_none_or(|b| b.time_ms < time_ms) {
                    beats.push(Beat { beat_number: k.rem_euclid(4) as u16 + 1, bpm, time_ms });
                }
            }
        }
        Ok(beats)
    }
}

/// A collection Traktor wrote (or [`build`](super::build) did), read: its tracks, their markers
/// and its playlists. It's refused when it isn't well-formed XML in UTF-8 with an `NML` root.
pub fn read(nml: &[u8]) -> Result<Collection> {
    let invalid = |why: String| Error::Invalid(format!("not a Traktor collection: {why}"));
    let text = std::str::from_utf8(nml).map_err(|e| invalid(e.to_string()))?;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut state = State::default();
    loop {
        let event = reader.read_event();
        let event = event.map_err(|e| invalid(format!("{e} at byte {}", reader.error_position())))?;
        match event {
            Event::Start(e) => state.open(&e).map_err(invalid)?,
            Event::Empty(e) => {
                state.open(&e).map_err(invalid)?;
                state.close();
            }
            Event::End(_) => state.close(),
            Event::Eof => break,
            _ => {}
        }
    }
    if !state.rooted {
        return Err(invalid("there's no NML element".into()));
    }
    if !state.open.is_empty() {
        return Err(invalid("it ends before its NML element does".into()));
    }
    Ok(state.finish())
}

/// Where [`read`] is in the file, and what it's read.
#[derive(Default)]
struct State {
    /// The elements open, outermost first.
    open: Vec<String>,
    rooted: bool,
    /// The collection's entry being read, while `in_entry`.
    entry: Entry,
    in_entry: bool,
    /// The entry's marker being read.
    marker: Option<Marker>,
    /// The names of the playlist nodes open.
    nodes: Vec<String>,
    /// The playlist being read.
    list: Option<List>,
    out: Collection,
    without_file: usize,
    unknown_markers: usize,
    smart: usize,
}

impl State {
    fn open(&mut self, e: &BytesStart<'_>) -> Result<(), String> {
        let name = e.name().as_ref().to_string();
        let a = Attrs::of(e)?;
        let parent = self.open.last().map(String::as_str);
        match (parent, name.as_str()) {
            (None, "NML") => self.rooted = true,
            (None, other) => return Err(format!("its root element is {other}, not NML")),
            (Some("COLLECTION"), "ENTRY") => {
                self.entry = Entry { title: a.text("TITLE"), artist: a.text("ARTIST"), ..Entry::default() };
                self.in_entry = true;
            }
            (Some("ENTRY"), _) if self.in_entry => self.read_entry(&name, &a),
            (Some("CUE_V2"), "GRID") => {
                if let Some(m) = self.marker.as_mut() {
                    m.bpm = a.number("BPM");
                }
            }
            (_, "NODE") => {
                self.nodes.push(a.text("NAME"));
                match a.text("TYPE").as_str() {
                    "PLAYLIST" => {
                        let root = usize::from(self.nodes.first().is_some_and(|n| n == "$ROOT"));
                        self.list = Some(List { path: self.nodes[root..].to_vec(), keys: Vec::new() });
                    }
                    "SMARTLIST" => self.smart += 1,
                    _ => {}
                }
            }
            (Some("ENTRY"), "PRIMARYKEY") if a.text("TYPE") == "TRACK" => {
                if let Some(list) = self.list.as_mut() {
                    list.keys.push(a.text("KEY"));
                }
            }
            _ => {}
        }
        self.open.push(name);
        Ok(())
    }

    /// An element of the entry being read.
    fn read_entry(&mut self, name: &str, a: &Attrs) {
        let e = &mut self.entry;
        match name {
            "LOCATION" => {
                e.location = Location { volume: a.text("VOLUME"), dir: a.text("DIR"), file: a.text("FILE") };
            }
            "ALBUM" => e.album = a.text("TITLE"),
            "INFO" => {
                e.genre = a.text("GENRE");
                e.label = a.text("LABEL");
                e.comment = a.text("COMMENT");
                e.remixer = a.text("REMIXER");
                e.key_text = a.text("KEY");
                e.duration_secs = a.number("PLAYTIME_FLOAT").or(a.number("PLAYTIME")).filter(|s| *s > 0.0);
                e.rating = a.number("RANKING").map_or(0, |r| (r / 51.0).clamp(0.0, 5.0) as u8);
                let year = a.text("RELEASE_DATE").split('/').next().and_then(|y| y.parse::<u16>().ok());
                e.year = year.unwrap_or(0);
            }
            "TEMPO" => e.bpm = a.number("BPM").filter(|b| *b > 0.0),
            "MUSICAL_KEY" => e.key = a.parsed("VALUE").and_then(key_of),
            "CUE_V2" => {
                let Some(kind) = a.parsed("TYPE").and_then(MarkerKind::of) else {
                    self.unknown_markers += 1;
                    return;
                };
                let name = a.text("NAME");
                let color = a.text("COLOR").to_ascii_lowercase();
                let hex =
                    color.strip_prefix('#').is_some_and(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()));
                self.marker = Some(Marker {
                    kind,
                    name: if name == "n.n." { String::new() } else { name },
                    start_ms: a.number("START").unwrap_or(0.0),
                    len_ms: a.number("LEN").filter(|l| *l > 0.0).unwrap_or(0.0),
                    // -1 is no pad, which isn't a u8.
                    hot_cue: a.parsed("HOTCUE"),
                    color: hex.then_some(color),
                    bpm: None,
                });
            }
            _ => {}
        }
    }

    /// Closes the element opened last (quick-xml has checked there's one, and that it's the one
    /// the end tag names).
    fn close(&mut self) {
        let name = self.open.pop().unwrap_or_default();
        match (self.open.last().map(String::as_str), name.as_str()) {
            (Some("COLLECTION"), "ENTRY") => {
                self.in_entry = false;
                let e = std::mem::take(&mut self.entry);
                match e.location.file.is_empty() {
                    true => self.without_file += 1,
                    false => self.out.entries.push(e),
                }
            }
            (Some("ENTRY"), "CUE_V2") => self.entry.markers.extend(self.marker.take()),
            (_, "NODE") => {
                self.nodes.pop();
                self.out.lists.extend(self.list.take());
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Collection {
        for (n, what) in [
            (self.without_file, "entr(ies) without a file"),
            (self.unknown_markers, "marker(s) of a type Traktor doesn't document"),
            (self.smart, "smart playlist(s), whose rules aren't read,"),
        ] {
            if n > 0 {
                self.out.warnings.push(format!("{n} {what} left out"));
            }
        }
        self.out
    }
}

/// An element's attributes, their values as XML reads them.
struct Attrs(Vec<(String, String)>);

impl Attrs {
    fn of(e: &BytesStart<'_>) -> Result<Attrs, String> {
        let mut out = Vec::new();
        for a in e.attributes() {
            let a = a.map_err(|e| e.to_string())?;
            let value = a.normalized_value(XmlVersion::Implicit1_0).map_err(|e| e.to_string())?;
            out.push((a.key.as_ref().to_string(), value.into_owned()));
        }
        Ok(Attrs(out))
    }

    /// The attribute `name`, or nothing.
    fn text(&self, name: &str) -> String {
        self.0.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()).unwrap_or_default()
    }

    /// The attribute `name`, when it reads as a `T`.
    fn parsed<T: std::str::FromStr>(&self, name: &str) -> Option<T> {
        self.text(name).trim().parse().ok()
    }

    /// The attribute `name` when it's a finite number.
    fn number(&self, name: &str) -> Option<f64> {
        self.parsed::<f64>(name).filter(|n| n.is_finite())
    }
}

#[cfg(test)]
mod tests;
