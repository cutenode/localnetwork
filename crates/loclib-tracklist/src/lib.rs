#![doc = include_str!("../README.md")]

mod detection;
mod m3u;
mod matching;
mod normalise;
mod plain;
mod table;
#[cfg(test)]
mod testing;
mod text;
mod xml;

pub use matching::{MatchKind, Matched, Matcher, Report, Status, Summary, match_entries};
pub use table::{Record, read_table};
pub use text::text_of;
pub use xml::{MAX_BEATS, Mark, Tempo, XmlLibrary, XmlTrack};

use serde::{Deserialize, Serialize};

use detection::detect_text;
use m3u::parse_m3u;
use plain::parse_plain;
use table::parse_table;
use xml::parse_xml;

/// Why a tracklist couldn't be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A table names no title or file column in its first few rows. `found` is its first row,
    /// joined with ", ".
    #[error("no title or file column in the table's header (found: {found})")]
    NoTitleColumn { found: String },
    /// An XML tag (or an attribute's value) runs to the end of the file.
    #[error("XML: unterminated tag on line {line}")]
    UnterminatedTag { line: usize },
    #[error("XML: unquoted attribute {attr} on line {line}")]
    UnquotedAttribute { attr: String, line: usize },
    #[error("not a rekordbox XML file (no DJ_PLAYLISTS element)")]
    NotRekordboxXml,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// What a tracklist file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(rename = "ImportFormat"))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Format {
    /// M3U / M3U8 playlists, and plain lists of file paths.
    M3u,
    /// A table with a header row (Exportify, TuneMyMusic, Soundiiz, Beatport-style...).
    Csv,
    /// rekordbox's "Export a playlist to a file" text.
    RekordboxText,
    /// rekordbox XML (`DJ_PLAYLISTS`).
    RekordboxXml,
    /// One track per line.
    Text,
}

/// One track as an outside list describes it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(rename = "ImportEntry"))]
pub struct Entry {
    /// 1-based line of the file where the entry starts.
    pub line: usize,
    /// The text it came from (the line, CSV record or XML tag), for messages.
    pub raw: String,
    pub title: String,
    /// Empty when the list names none.
    pub artist: String,
    pub album: Option<String>,
    /// Mix / version name, when the list has a column for it ("Original Mix").
    pub mix: Option<String>,
    pub duration_secs: Option<u32>,
    /// The file as the list names it, with `file://` URLs decoded (`/Users/me/Music/x.mp3`,
    /// `C:\Music\x.mp3`, `../x.mp3`).
    pub path: Option<String>,
    pub isrc: Option<String>,
}

/// Read a tracklist, detecting its format from `hint` (a file name or extension) and the content.
pub fn parse(bytes: &[u8], hint: Option<&str>) -> Result<Vec<Entry>> {
    let text = text_of(bytes);
    parse_text(&text, detect_text(&text, hint))
}

/// Read a tracklist in a known format.
pub fn parse_as(bytes: &[u8], format: Format) -> Result<Vec<Entry>> {
    parse_text(&text_of(bytes), format)
}

/// The format [`parse`] would read `bytes` as.
pub fn detect(bytes: &[u8], hint: Option<&str>) -> Format {
    detect_text(&text_of(bytes), hint)
}

/// The playlists of a rekordbox XML file as (folder path like `Sets/2026/Warmup`, entries),
/// in document order. Folders themselves aren't listed.
pub fn xml_playlists(bytes: &[u8]) -> Result<Vec<(String, Vec<Entry>)>> {
    Ok(parse_xml(&text_of(bytes))?.playlists)
}

/// A rekordbox XML file as a library: its collection's tracks with their tags, key, tempo, beat
/// grid and cues, its playlists as [`xml_playlists`] reads them, and what was left out as out of
/// range.
pub fn read_xml_library(bytes: &[u8]) -> Result<XmlLibrary> {
    parse_xml(&text_of(bytes))
}

fn parse_text(text: &str, format: Format) -> Result<Vec<Entry>> {
    match format {
        Format::M3u => Ok(parse_m3u(text)),
        Format::Csv | Format::RekordboxText => parse_table(text, format),
        Format::RekordboxXml => Ok(parse_xml(text)?.tracks.into_iter().map(|t| t.entry).collect()),
        Format::Text => Ok(parse_plain(text)),
    }
}
