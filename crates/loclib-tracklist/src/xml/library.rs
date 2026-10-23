//! What a library holds of a rekordbox XML collection's tracks: tags, key, tempo, beat grid
//! (`TEMPO`) and cues (`POSITION_MARK`), each value checked, as the file can say anything.

use loclib_model::analysis::Beat;

use super::Tag;
use crate::Entry;

/// The most beats [`XmlTrack::beats`] expands a grid to: a two-hour mix at 180 BPM has 21,600.
pub const MAX_BEATS: usize = 1 << 18;
/// Elements nested deeper than this are left out, with everything in them: room for some 60
/// levels of playlist folders.
pub(super) const MAX_DEPTH: usize = 64;
/// Times past a day (in ms) are out of range.
const MAX_MS: f64 = 86_400_000.0;
/// The tempos a beat grid holds (hundredths of a BPM, up to 655.35).
const BPM: std::ops::RangeInclusive<f64> = 0.01..=655.35;
/// The most the playlists' folder paths come to, each part costing its name's bytes and a few
/// more: a library of 10,000 playlists three folders deep takes about 1 MiB.
pub const MAX_PATH_BYTES: usize = 16 << 20;

/// A rekordbox XML file read whole ([`crate::read_xml_library`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct XmlLibrary {
    /// The `COLLECTION`'s tracks, in order.
    pub tracks: Vec<XmlTrack>,
    /// The playlists, in the file's order (none from [`crate::read_xml_collection`]).
    pub playlists: Vec<XmlPlaylist>,
    /// What was left out as out of range, a line each, counted.
    pub warnings: Vec<String>,
}

/// A playlist: its folder path (`Sets/2026/Warmup`, without rekordbox's `ROOT`) and its entries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct XmlPlaylist {
    pub path: String,
    pub entries: Vec<XmlEntry>,
}

/// A playlist's entry: a collection track, by its index in [`XmlLibrary::tracks`], or one the
/// collection lacks, as an entry with only what its key says (a location key's file).
#[derive(Debug, Clone, PartialEq)]
pub enum XmlEntry {
    /// `line` is the playlist's line for it, `raw` its key (`Key="5"`).
    Track {
        index: usize,
        line: usize,
        raw: String,
    },
    Missing(Entry),
}

impl XmlLibrary {
    /// `e` as a tracklist's entry: its collection track's, at the playlist's line, its key as the
    /// text it came from.
    pub fn entry(&self, e: &XmlEntry) -> Entry {
        let (index, line, raw) = match e {
            XmlEntry::Track { index, line, raw } => (*index, *line, raw),
            XmlEntry::Missing(missing) => return missing.clone(),
        };
        // Not the track's own `raw`, its whole TRACK tag.
        let t = &self.tracks[index].entry;
        Entry {
            line,
            raw: raw.clone(),
            title: t.title.clone(),
            artist: t.artist.clone(),
            album: t.album.clone(),
            mix: t.mix.clone(),
            duration_secs: t.duration_secs,
            path: t.path.clone(),
            isrc: t.isrc.clone(),
        }
    }

    /// The file `e` names, when it names one.
    pub fn file<'a>(&'a self, e: &'a XmlEntry) -> Option<&'a str> {
        match e {
            XmlEntry::Track { index, .. } => self.tracks[*index].entry.path.as_deref(),
            XmlEntry::Missing(missing) => missing.path.as_deref(),
        }
    }

    /// The entries of the playlist `query` names: by its path, or ignoring case when that's one
    /// playlist's ([`loclib_model::model::pick`]).
    pub fn playlist(&self, query: &str) -> Option<Vec<Entry>> {
        let picked = loclib_model::model::pick(self.playlists.iter().map(|p| (&p.path, p)), query)?;
        Some(picked.entries.iter().map(|e| self.entry(e)).collect())
    }
}

/// A track of the collection: what a tracklist's entry has of it, and what a library keeps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct XmlTrack {
    /// Its title, artist, album, mix name, length (`TotalTime`), file (`Location`) and line.
    pub entry: Entry,
    pub genre: String,
    pub label: String,
    pub remixer: String,
    pub composer: String,
    pub comment: String,
    /// 0 when the file says none.
    pub year: u16,
    /// 0 to 5 stars, from rekordbox's 0-255.
    pub rating: u8,
    /// `Tonality`, as written: a key name (`Am`) or a Camelot code (`8A`).
    pub key: Option<String>,
    /// `AverageBpm`, when it's a tempo (rekordbox writes 0 for a track it hasn't analysed).
    pub bpm: Option<f64>,
    /// Its beat grid's tempo changes (`TEMPO`), in time order.
    pub tempos: Vec<Tempo>,
    /// Its hot cues, memory cues and loops (`POSITION_MARK`), in the file's order.
    pub marks: Vec<Mark>,
}

/// Where a beat grid's tempo starts or changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tempo {
    /// The beat it starts on (`Inizio`), in ms; before 0 when the grid starts before the audio.
    pub start_ms: f64,
    pub bpm: f64,
    /// Beats to the bar (`Metro`, `4/4`), 1-4.
    pub beats_per_bar: u8,
    /// Which beat of its bar the first one is (`Battito`), from 1.
    pub beat: u8,
}

/// A hot cue, a memory cue or a loop.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mark {
    /// 0 for hot cue A to 7 for H; a memory cue without one.
    pub hot_cue: Option<u8>,
    pub start_ms: u32,
    /// Where a loop ends, after it starts.
    pub end_ms: Option<u32>,
    pub name: String,
    /// `Red`, `Green` and `Blue`, when it has all three.
    pub rgb: Option<[u8; 3]>,
}

/// Why a `TEMPO` or a `POSITION_MARK` is left out.
#[derive(Debug, PartialEq)]
pub(super) enum Left {
    OutOfRange,
    /// A value rekordbox writes that isn't taken: a fade or load mark, a meter past 4 beats.
    Unsupported,
}

/// How many values of each kind were left out, and why.
#[derive(Default)]
pub(super) struct Skipped {
    pub bpms: usize,
    pub tempos: usize,
    pub meters: usize,
    pub marks: usize,
    pub mark_kinds: usize,
    pub nested: usize,
    pub paths: usize,
}

impl Skipped {
    /// Counts a TEMPO left out, by why.
    pub fn tempo(&mut self, why: Left) {
        *match why {
            Left::OutOfRange => &mut self.tempos,
            Left::Unsupported => &mut self.meters,
        } += 1;
    }

    /// Counts a POSITION_MARK left out, by why.
    pub fn mark(&mut self, why: Left) {
        *match why {
            Left::OutOfRange => &mut self.marks,
            Left::Unsupported => &mut self.mark_kinds,
        } += 1;
    }

    pub fn warnings(&self) -> Vec<String> {
        let said = [
            (self.bpms, "track tempo(s) (AverageBpm) left out: not 0.01-655.35".to_string()),
            (self.tempos, "beat grid tempo(s) (TEMPO) left out: a time, tempo or beat out of range".into()),
            (
                self.meters,
                "beat grid tempo(s) (TEMPO) left out: only 1-4 beats to the bar (Metro) are supported".into(),
            ),
            (self.marks, "cue(s) (POSITION_MARK) left out: a type, time or hot cue out of range".into()),
            (
                self.mark_kinds,
                "cue(s) (POSITION_MARK) left out: fade-in, fade-out and load marks (Type 1-3) aren't supported".into(),
            ),
            (self.nested, format!("element(s) nested more than {MAX_DEPTH} deep left out, with what's in them")),
            (
                self.paths,
                format!("playlist(s) left out: their folder paths come to more than {} MiB", MAX_PATH_BYTES >> 20),
            ),
        ];
        said.into_iter().filter(|(n, _)| *n > 0).map(|(n, what)| format!("{n} {what}")).collect()
    }
}

/// A `TRACK` of the collection, `entry` read from it already.
pub(super) fn track(tag: &Tag<'_>, entry: Entry, skipped: &mut Skipped) -> XmlTrack {
    let text = |name| tag.attr(name).map(str::trim).unwrap_or_default().to_string();
    let number = |name| tag.attr(name).and_then(|v| v.trim().parse::<f64>().ok());
    let bpm = number("AverageBpm").filter(|&b| b != 0.0);
    if bpm.is_some_and(|b| !BPM.contains(&b)) {
        skipped.bpms += 1;
    }
    let rating = tag.attr("Rating").and_then(|v| v.trim().parse::<u8>().ok()).unwrap_or(0);
    XmlTrack {
        entry,
        genre: text("Genre"),
        label: text("Label"),
        remixer: text("Remixer"),
        composer: text("Composer"),
        comment: text("Comments"),
        year: tag.attr("Year").and_then(|v| v.trim().parse().ok()).unwrap_or(0),
        // rekordbox writes 0, 51, 102, 153, 204 or 255.
        rating: (rating.saturating_add(25) / 51).min(5),
        key: Some(text("Tonality")).filter(|k| !k.is_empty()),
        bpm: bpm.filter(|b| BPM.contains(b)),
        tempos: Vec::new(),
        marks: Vec::new(),
    }
}

/// A `TEMPO`, or why it's left out.
pub(super) fn tempo(tag: &Tag<'_>) -> Result<Tempo, Left> {
    let number = |name| tag.attr(name).and_then(|v| v.trim().parse::<f64>().ok());
    // To the microsecond, so a time written to the millisecond is exactly that.
    let start_ms = number("Inizio").map(|s| (s * 1e6).round() / 1e3).filter(|s| s.abs() <= MAX_MS);
    let start_ms = start_ms.ok_or(Left::OutOfRange)?;
    let bpm = number("Bpm").filter(|b| BPM.contains(b)).ok_or(Left::OutOfRange)?;
    let beats_per_bar = match tag.attr("Metro").map(|m| m.split('/').next().unwrap_or_default().trim().parse::<u32>()) {
        None => 4,
        Some(Ok(n @ 1..=4)) => n as u8,
        Some(Ok(5..)) => return Err(Left::Unsupported),
        Some(_) => return Err(Left::OutOfRange),
    };
    let beat = match tag.attr("Battito") {
        None => 1,
        Some(b) => b.trim().parse::<u8>().ok().filter(|b| (1..=beats_per_bar).contains(b)).ok_or(Left::OutOfRange)?,
    };
    Ok(Tempo { start_ms, bpm, beats_per_bar, beat })
}

/// A `POSITION_MARK`: a cue (`Type` 0) or a loop (4) at `Start` (to `End`) seconds, a hot cue
/// (`Num` 0-7) or a memory cue (-1); or why it's left out.
pub(super) fn mark(tag: &Tag<'_>) -> Result<Mark, Left> {
    let ms = |name| {
        let secs = tag.attr(name)?.trim().parse::<f64>().ok()?;
        let ms = secs * 1000.0;
        (0.0..=MAX_MS).contains(&ms).then(|| ms.round() as u32)
    };
    let is_loop = match tag.attr("Type").map(str::trim) {
        Some("0") => false,
        Some("4") => true,
        Some("1" | "2" | "3") => return Err(Left::Unsupported),
        _ => return Err(Left::OutOfRange),
    };
    let start_ms = ms("Start").ok_or(Left::OutOfRange)?;
    let end_ms = match is_loop {
        false => None,
        true => Some(ms("End").filter(|&end| end > start_ms).ok_or(Left::OutOfRange)?),
    };
    let hot_cue = match tag.attr("Num").and_then(|n| n.trim().parse::<i8>().ok()) {
        Some(-1) => None,
        Some(n @ 0..=7) => Some(n as u8),
        _ => return Err(Left::OutOfRange),
    };
    let channel = |name| tag.attr(name).and_then(|v| v.trim().parse::<u8>().ok());
    let rgb = (|| Some([channel("Red")?, channel("Green")?, channel("Blue")?]))();
    let name = tag.attr("Name").unwrap_or_default().to_string();
    Ok(Mark { hot_cue, start_ms, end_ms, name, rgb })
}

impl XmlTrack {
    /// Its beat grid: the beats of each tempo, from the first at or after 0 to half a beat before
    /// the next tempo, up to the track's end (`TotalTime`), and whether it was cut at
    /// [`MAX_BEATS`]. None without a length to end at.
    pub fn beats(&self) -> (Vec<Beat>, bool) {
        let Some(secs) = self.entry.duration_secs else { return (Vec::new(), false) };
        // Half a millisecond short, so no beat rounds to the end itself.
        let end = (f64::from(secs) * 1000.0).min(MAX_MS) - 0.5;
        let mut beats: Vec<Beat> = Vec::new();
        for (i, t) in self.tempos.iter().enumerate() {
            let interval = 60_000.0 / t.bpm;
            // rekordbox puts a TEMPO on each beat of a grid that moves, at its time rounded to the
            // millisecond: the beat before it projects to within a millisecond of it, and is it.
            // Half a beat before keeps them apart, so the times only go up.
            let until = self.tempos.get(i + 1).map_or(end, |next| (next.start_ms - interval / 2.0).min(end));
            let mut k = if t.start_ms < 0.0 { (-t.start_ms / interval).ceil() } else { 0.0 };
            loop {
                let at = t.start_ms + k * interval;
                if at >= until {
                    break;
                }
                if beats.len() == MAX_BEATS {
                    return (beats, true);
                }
                let time_ms = at.round().max(0.0) as u32;
                let in_bar = (u64::from(t.beat) - 1 + k as u64) % u64::from(t.beats_per_bar);
                beats.push(Beat { beat_number: in_bar as u16 + 1, bpm: t.bpm, time_ms });
                k += 1.0;
            }
        }
        (beats, false)
    }
}

#[cfg(test)]
mod tests;
