//! Turning outside tracklists into tracks of a drive's library: the first half of importing a
//! playlist from somewhere else (a computer's M3U, a Spotify export, rekordbox's own playlist
//! file or XML, a tracklist pasted from a web page). Nothing here writes anything.
//!
//! [`parse`] reads a file into [`Entry`]s, detecting the format from the file name and contents
//! (see [`Format`]):
//!
//! * M3U / M3U8: `#EXTINF` info, and absolute, relative, `file://` and Windows paths.
//! * Tables with a header row: Exportify (Spotify), TuneMyMusic, Soundiiz, Beatport-style
//!   exports, Apple Music's text export, or any CSV naming a title column; `,`, `;` or tab
//!   separated, quoted or not.
//! * rekordbox's "Export a playlist to a file" text: UTF-16 with a BOM, tab-separated, the
//!   browser's visible columns as the header (`#`, `Track Title`, `Artist`, `Time`...). Columns
//!   are matched by name, since which ones appear depends on the user's browser layout.
//! * rekordbox XML (`DJ_PLAYLISTS`): [`parse`] returns the collection, [`xml_playlists`] each
//!   playlist by its folder path.
//! * Plain text: one "Artist - Title" per line (or "Title by Artist"), ignoring numbering,
//!   bullets, cue timestamps, blank lines and comments.
//!
//! [`match_entries`] (or a reusable [`Matcher`]) then finds each entry in a [`Library`], trying
//! in order:
//!
//! 1. the file path, by its trailing components (a computer's `/Users/me/Music/A/x.mp3` is the
//!    drive's `/Contents/A/x.mp3`), case-insensitively;
//! 2. the ISRC;
//! 3. artist + title + duration (within 2 s);
//! 4. normalised artist + title: case, accents, punctuation, "&"/"and", featured artists and
//!    generic version names ("Original Mix", "Radio Edit", "2011 Remaster") don't count, but a
//!    remix, dub or live version never matches the original;
//! 5. fuzzy token similarity (typos, swapped artist and title, artist folded into the title).
//!
//! Whenever several tracks fit equally well, or the best fuzzy candidate is weak, the entry is
//! [`Status::Ambiguous`] with the candidates rather than a guess.

use std::collections::HashMap;

use percent_encoding::percent_decode_str;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::model::{Library, normalize};
use crate::paths;
use crate::{Error, Result};

// ---------------------------------------------------------------------------------------------
// Entries and formats
// ---------------------------------------------------------------------------------------------

/// What a tracklist file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "ImportFormat")]
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "ImportEntry")]
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

fn parse_text(text: &str, format: Format) -> Result<Vec<Entry>> {
    match format {
        Format::M3u => Ok(parse_m3u(text)),
        Format::Csv | Format::RekordboxText => parse_table(text, format),
        Format::RekordboxXml => Ok(parse_xml(text)?.collection),
        Format::Text => Ok(parse_plain(text)),
    }
}

// ---------------------------------------------------------------------------------------------
// Decoding and detection
// ---------------------------------------------------------------------------------------------

/// Bytes to text with `\n` line ends: UTF-8 or UTF-16 (with or without a BOM), falling back to
/// Windows-1252 for the legacy `.m3u` files some software still writes.
pub(crate) fn text_of(bytes: &[u8]) -> String {
    let text = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(rest).into_owned()
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        utf16(rest, true)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        utf16(rest, false)
    } else if let Some(le) = sniff_utf16(bytes) {
        utf16(bytes, le)
    } else {
        match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => bytes.iter().map(|&b| cp1252(b)).collect(),
        }
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    if text.contains('\r') { text.replace("\r\n", "\n").replace('\r', "\n") } else { text.to_string() }
}

fn utf16(bytes: &[u8], le: bool) -> String {
    let units = bytes
        .chunks_exact(2)
        .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) });
    char::decode_utf16(units).map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER)).collect()
}

/// UTF-16 without a BOM: mostly-ASCII text has a zero in every other byte. `Some(true)` for
/// little-endian.
fn sniff_utf16(bytes: &[u8]) -> Option<bool> {
    let pairs: Vec<&[u8]> = bytes[..bytes.len().min(1024)].chunks_exact(2).collect();
    if pairs.len() < 2 {
        return None;
    }
    let le = pairs.iter().filter(|p| p[0] != 0 && p[1] == 0).count();
    let be = pairs.iter().filter(|p| p[0] == 0 && p[1] != 0).count();
    if le * 10 >= pairs.len() * 4 && be * 10 < pairs.len() {
        Some(true)
    } else if be * 10 >= pairs.len() * 4 && le * 10 < pairs.len() {
        Some(false)
    } else {
        None
    }
}

fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘',
        '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9F => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

/// `hint`'s extension, lower-cased ("list.M3U8" and "m3u8" both give "m3u8").
fn extension(hint: &str) -> String {
    let name = hint.rsplit(['/', '\\']).next().unwrap_or(hint);
    name.rsplit_once('.').map_or(name, |(_, e)| e).to_ascii_lowercase()
}

/// Unmistakable content first (XML, `#EXTM3U`, rekordbox's header), then the file extension,
/// then softer evidence (a recognisable table header, a list of paths).
fn detect_text(text: &str, hint: Option<&str>) -> Format {
    let start = text.trim_start();
    if start.starts_with("<?xml") || start.starts_with("<DJ_PLAYLISTS") {
        return Format::RekordboxXml;
    }
    let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if strip_prefix_ci(first, "#EXTM3U").is_some() || text.contains("#EXTINF") {
        return Format::M3u;
    }
    if first.contains('\t') && is_rekordbox_header(first) {
        return Format::RekordboxText;
    }
    match hint.map(extension).as_deref() {
        Some("m3u" | "m3u8") => return Format::M3u,
        Some("xml") => return Format::RekordboxXml,
        Some("csv" | "tsv") => return Format::Csv,
        _ => {}
    }
    if strip_prefix_ci(first, "sep=").is_some() || sniffed_header(first).is_some() {
        return Format::Csv;
    }
    let content: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    if !content.is_empty() && content.iter().filter(|l| looks_like_path(l)).count() * 5 >= content.len() * 3 {
        return Format::M3u;
    }
    Format::Text
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    s.get(..prefix.len()).filter(|p| p.eq_ignore_ascii_case(prefix)).map(|_| &s[prefix.len()..])
}

const AUDIO_EXTENSIONS: &[&str] =
    &["mp3", "wav", "wave", "aif", "aiff", "flac", "m4a", "mp4", "aac", "alac", "ogg", "opus", "wma"];

/// Whether a line of a plain list is a file rather than "Artist - Title".
fn looks_like_path(s: &str) -> bool {
    let lower = s.trim().to_ascii_lowercase();
    let b = lower.as_bytes();
    let drive = b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/');
    lower.starts_with("file:")
        || drive
        || lower.starts_with("\\\\")
        || ((lower.contains('/') || lower.contains('\\'))
            && lower.rsplit_once('.').is_some_and(|(_, e)| AUDIO_EXTENSIONS.contains(&e)))
}

/// The local path a playlist line or `Location` names: `file://` URLs decoded (including
/// `file://localhost/C:/...` and UNC hosts), classic Mac OS `Disk:Folder:file` paths turned into
/// slashes, anything else as written. `None` for other URLs (`https://`, `spotify:`).
fn file_path(loc: &str) -> Option<String> {
    let loc = loc.trim();
    if let Some(rest) = strip_prefix_ci(loc, "file:") {
        let decoded = percent_decode_str(rest).decode_utf8_lossy().into_owned();
        let p = match decoded.strip_prefix("//") {
            Some(r) => match r.find('/') {
                Some(0) | None => r.to_string(),
                Some(i) if r[..i].eq_ignore_ascii_case("localhost") => r[i..].to_string(),
                Some(_) => format!("//{r}"),
            },
            None => decoded,
        };
        let b = p.as_bytes();
        let windows = b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':';
        return Some(if windows { p[1..].to_string() } else { p });
    }
    let scheme = loc.split_once(':').map(|(s, _)| s);
    if scheme.is_some_and(|s| {
        s.len() >= 2
            && s.starts_with(|c: char| c.is_ascii_alphabetic())
            && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    }) {
        return None;
    }
    if loc.matches(':').count() >= 2 && !loc.contains(['/', '\\']) && loc.as_bytes().get(1) != Some(&b':') {
        return Some(format!("/{}", loc.replace(':', "/")));
    }
    (!loc.is_empty()).then(|| loc.to_string())
}

// ---------------------------------------------------------------------------------------------
// M3U and plain text
// ---------------------------------------------------------------------------------------------

fn parse_m3u(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut info: Option<(Option<u32>, String)> = None;
    // Extended M3U's #EXTALB / #EXTART name the album (and its artist) for the tracks below.
    let mut album: Option<String> = None;
    let mut album_artist: Option<String> = None;
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with('#') {
            if let Some(v) = strip_prefix_ci(t, "#EXTINF:") {
                info = Some(parse_extinf(v));
            } else if let Some(v) = strip_prefix_ci(t, "#EXTALB:") {
                album = Some(v.trim().to_string()).filter(|s| !s.is_empty());
            } else if let Some(v) = strip_prefix_ci(t, "#EXTART:") {
                album_artist = Some(v.trim().to_string()).filter(|s| !s.is_empty());
            }
            continue;
        }
        let mut e = location_entry(t);
        e.line = i + 1;
        e.raw = t.to_string();
        if let Some((duration, display)) = info.take() {
            e.duration_secs = duration;
            if !display.is_empty() {
                (e.artist, e.title) = split_artist_title(&display, false);
            }
        }
        if e.artist.is_empty()
            && let Some(a) = &album_artist
        {
            e.artist = a.clone();
        }
        e.album = album.clone();
        out.push(e);
    }
    out
}

/// `#EXTINF:` payload: `323,Artist - Title`, `-1 tvg-id="x",Title`, `323.4,...`.
fn parse_extinf(v: &str) -> (Option<u32>, String) {
    let mut quoted = false;
    let comma = v.char_indices().find(|&(_, c)| {
        if c == '"' {
            quoted = !quoted;
        }
        c == ',' && !quoted
    });
    let (head, display) = match comma {
        Some((i, _)) => (&v[..i], v[i + 1..].trim()),
        None => (v, ""),
    };
    let secs = head.split_whitespace().next().and_then(|n| n.parse::<f64>().ok());
    (secs.filter(|s| *s >= 1.0).map(|s| s.round() as u32), display.to_string())
}

/// An entry for a file or URL, with artist and title guessed from the file name.
fn location_entry(loc: &str) -> Entry {
    let path = file_path(loc);
    let src = path.as_deref().unwrap_or(loc);
    let mut name = src.rsplit(['/', '\\']).next().unwrap_or(src).to_string();
    if path.is_none() {
        name = percent_decode_str(name.split(['?', '#']).next().unwrap_or("")).decode_utf8_lossy().into_owned();
    }
    let mut stem = strip_extension(&name).to_string();
    if !stem.contains(' ') {
        stem = stem.replace('_', " ");
    }
    let (artist, title) = split_artist_title(strip_decoration(&stem, false).0, false);
    Entry { title, artist, path, ..Default::default() }
}

fn strip_extension(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty() && (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            stem
        }
        _ => name,
    }
}

fn parse_plain(text: &str) -> Vec<Entry> {
    let lines: Vec<(usize, &str)> = text
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim()))
        .filter(|(_, l)| !l.is_empty() && !is_comment(l))
        .collect();
    // In a list where most lines start with a bare number ("1 Artist - Title"), that number is
    // the numbering; elsewhere it may belong to the artist ("808 State").
    let bare = lines.iter().filter(|(_, l)| bare_number_len(l).is_some()).count();
    let numbered = bare >= 2 && bare * 5 >= lines.len() * 3;
    let mut out = Vec::new();
    for (line, t) in lines {
        if looks_like_path(t) {
            out.push(Entry { line, raw: t.to_string(), ..location_entry(t) });
            continue;
        }
        let (rest, _) = strip_decoration(t, numbered);
        let (rest, duration_secs) = strip_trailing_duration(rest);
        // A heading ("Tracklist:"), or nothing but decoration.
        if !rest.chars().any(char::is_alphanumeric) || (rest.ends_with(':') && split_at_dash(rest).is_none()) {
            continue;
        }
        let (artist, title) = split_artist_title(rest, true);
        out.push(Entry { line, raw: t.to_string(), title, artist, duration_secs, ..Default::default() });
    }
    out
}

fn is_comment(l: &str) -> bool {
    l.starts_with("//")
        || l.starts_with(';')
        || (l.starts_with('#') && !l[1..].starts_with(|c: char| c.is_ascii_digit()))
}

fn digits_len(s: &str) -> usize {
    s.bytes().take_while(u8::is_ascii_digit).count()
}

/// Length of a leading "12 " (a number of up to 4 digits, then whitespace).
fn bare_number_len(s: &str) -> Option<usize> {
    let d = digits_len(s);
    ((1..=4).contains(&d) && s[d..].starts_with(char::is_whitespace)).then_some(d)
}

/// Strip list decoration from the start of a line: bullets, one cue timestamp ("[00:12:34]",
/// "1:02:03") and one number ("01.", "1)", "#1", "[01]", "01 -"). A bare number ("1 Artist")
/// only counts in a `numbered` list or with a leading zero, so "808 State" survives. Returns the
/// rest and the timestamp in seconds, if any.
fn strip_decoration(s: &str, numbered: bool) -> (&str, Option<u32>) {
    let mut s = strip_bullet(s.trim());
    let mut stamp = strip_timestamp(s);
    if let Some((rest, _)) = stamp {
        s = strip_bullet(rest);
    }
    s = strip_bullet(strip_number(s, numbered));
    if stamp.is_none() {
        stamp = strip_timestamp(s);
        if let Some((rest, _)) = stamp {
            s = strip_bullet(rest);
        }
    }
    (s.trim(), stamp.map(|(_, secs)| secs))
}

fn strip_bullet(s: &str) -> &str {
    const BULLETS: &[char] =
        &['-', '*', '+', '>', '~', '•', '·', '–', '—', '►', '▶', '▸', '○', '◦', '▪', '■', '□', '→'];
    let s = s.trim_start();
    let mut chars = s.chars();
    match chars.next() {
        // An ASCII bullet needs a space after it ("-ziq" is an artist).
        Some(c) if BULLETS.contains(&c) && (!c.is_ascii() || chars.as_str().starts_with(char::is_whitespace)) => {
            chars.as_str().trim_start()
        }
        _ => s,
    }
}

fn strip_timestamp(s: &str) -> Option<(&str, u32)> {
    let close = match s.chars().next() {
        Some('[') => Some(']'),
        Some('(') => Some(')'),
        _ => None,
    };
    let inner = if close.is_some() { &s[1..] } else { s };
    let (len, secs) = clock_prefix(inner)?;
    let mut rest = &inner[len..];
    if let Some(c) = close {
        rest = rest.strip_prefix(c)?;
    }
    (rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace() || matches!(c, '-' | '|' | '–' | '—')))
        .then(|| (rest.trim_start().trim_start_matches('|'), secs))
}

/// "m:ss", "mm:ss" or "h:mm:ss" at the start of `s`: (byte length, seconds).
fn clock_prefix(s: &str) -> Option<(usize, u32)> {
    let mut pos = 0;
    let mut fields = Vec::new();
    loop {
        let d = digits_len(&s[pos..]);
        let ok = if fields.is_empty() { (1..=3).contains(&d) } else { d == 2 };
        if !ok {
            return None;
        }
        let v: u32 = s[pos..pos + d].parse().ok()?;
        if !fields.is_empty() && v >= 60 {
            return None;
        }
        fields.push(v);
        pos += d;
        if fields.len() < 3 && s[pos..].starts_with(':') && s[pos + 1..].starts_with(|c: char| c.is_ascii_digit()) {
            pos += 1;
        } else {
            break;
        }
    }
    // "12:34" must not run on into more digits or a decimal ("12:345", "1:02.5" is still a clock).
    if fields.len() < 2 || s[pos..].starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let secs = fields.iter().fold(0u32, |acc, v| acc * 60 + v);
    Some((pos, secs))
}

fn strip_number(s: &str, numbered: bool) -> &str {
    if let Some(rest) = s.strip_prefix('#') {
        let d = digits_len(rest);
        return if d > 0 { rest[d..].trim_start_matches(['.', ')', ':']) } else { s };
    }
    if let Some(close) = s.strip_prefix('(').map(|_| ')').or_else(|| s.strip_prefix('[').map(|_| ']')) {
        let d = digits_len(&s[1..]);
        if d > 0 && s[1 + d..].starts_with(close) {
            return &s[2 + d..];
        }
        return s;
    }
    let d = digits_len(s);
    if d == 0 || d > 4 {
        return s;
    }
    let rest = &s[d..];
    if let Some(r) = rest.strip_prefix(['.', ')', ':'])
        && (r.is_empty() || r.starts_with(char::is_whitespace))
    {
        return r;
    }
    if !rest.starts_with(char::is_whitespace) {
        return s; // "10cc", "1.5 Step"
    }
    let r = rest.trim_start();
    // "1 - Artist - Title": the number, a dash, then the usual separator.
    if let Some(r2) = r.strip_prefix(['-', '–', '—'])
        && split_at_dash(r2).is_some()
    {
        return r2;
    }
    if numbered || s.starts_with('0') { r } else { s }
}

/// A duration at the end of a line: "(5:23)", "[5:23]" or " 5:23".
fn strip_trailing_duration(s: &str) -> (&str, Option<u32>) {
    let t = s.trim_end();
    let found = if let Some(x) = t.strip_suffix(')') {
        x.rfind('(').map(|i| (&x[i + 1..], i))
    } else if let Some(x) = t.strip_suffix(']') {
        x.rfind('[').map(|i| (&x[i + 1..], i))
    } else {
        // The space may be wider than a byte (a no-break space, an ideographic space).
        t.char_indices().rfind(|(_, c)| c.is_whitespace()).map(|(i, c)| (&t[i + c.len_utf8()..], i))
    };
    match found {
        Some((inner, cut)) if cut > 0 && clock_prefix(inner).is_some_and(|(len, _)| len == inner.len()) => {
            let rest = t[..cut].trim_end().trim_end_matches(['-', '|', '–', '—']).trim_end();
            (rest, clock_prefix(inner).map(|(_, secs)| secs))
        }
        _ => (s, None),
    }
}

/// The first spaced dash ("Artist - Title", "–", "—", "--"), or an unspaced en/em dash
/// ("Artist–Title"; a plain hyphen is too common inside names): (start, separator length).
fn split_at_dash(s: &str) -> Option<(usize, usize)> {
    const SEPARATORS: &[&str] = &[" - ", " – ", " — ", " -- ", " ‒ ", " ― "];
    SEPARATORS
        .iter()
        .filter_map(|sep| s.find(sep).map(|i| (i, sep.len())))
        .min_by_key(|&(i, _)| i)
        .or_else(|| s.find(['–', '—']).filter(|&i| i > 0).map(|i| (i, 3)))
}

/// "Artist - Title" (or, with `allow_by`, "Title by Artist") into (artist, title). Without a
/// separator the whole text is the title.
fn split_artist_title(s: &str, allow_by: bool) -> (String, String) {
    let s = s.trim();
    if let Some((i, n)) = split_at_dash(s) {
        let (artist, title) = (s[..i].trim(), s[i + n..].trim());
        if !title.is_empty() {
            return (artist.to_string(), title.to_string());
        }
    }
    if allow_by && let Some(i) = s.to_ascii_lowercase().rfind(" by ").filter(|&i| i > 0) {
        let (title, artist) = (s[..i].trim(), s[i + 4..].trim());
        if !artist.is_empty() {
            return (artist.to_string(), title.to_string());
        }
    }
    (String::new(), s.to_string())
}

// ---------------------------------------------------------------------------------------------
// Tables: CSV and rekordbox's text export
// ---------------------------------------------------------------------------------------------

pub(crate) struct Record<'a> {
    pub line: usize,
    raw: &'a str,
    pub fields: Vec<String>,
}

/// RFC 4180 with leniency: quotes only open at the start of a field (after optional spaces),
/// `""` inside quotes is a quote, text after a closing quote is kept, and quoted fields may span
/// lines. Without `quoting` (rekordbox's text), quotes are ordinary characters. Blank records
/// are dropped.
pub(crate) fn read_table(text: &str, delim: char, quoting: bool) -> Vec<Record<'_>> {
    let mut records = Vec::new();
    let mut fields = Vec::new();
    let mut field = String::new();
    let (mut in_quotes, mut was_quoted) = (false, false);
    let (mut line, mut record_line, mut record_start) = (1, 1, 0);
    fn push<'a>(records: &mut Vec<Record<'a>>, fields: Vec<String>, line: usize, raw: &'a str) {
        if fields.iter().any(|f| !f.trim().is_empty()) {
            records.push(Record { line, raw, fields });
        }
    }
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek().is_some_and(|&(_, n)| n == '"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => (in_quotes, was_quoted) = (false, true),
                _ => {
                    line += usize::from(c == '\n');
                    field.push(c);
                }
            }
            continue;
        }
        match c {
            '"' if quoting && !was_quoted && field.trim().is_empty() => {
                field.clear();
                in_quotes = true;
            }
            '\n' => {
                fields.push(std::mem::take(&mut field));
                push(&mut records, std::mem::take(&mut fields), record_line, &text[record_start..i]);
                was_quoted = false;
                line += 1;
                (record_line, record_start) = (line, i + 1);
            }
            c if c == delim => {
                fields.push(std::mem::take(&mut field));
                was_quoted = false;
            }
            c => field.push(c),
        }
    }
    if !field.is_empty() || !fields.is_empty() {
        fields.push(field);
        push(&mut records, fields, record_line, &text[record_start..]);
    }
    records
}

/// The most frequent of `,` `;` and tab outside quotes in a header line (ties go to tab, then
/// `;`: commas are likelier inside values).
fn sniff_delimiter(line: &str) -> char {
    const DELIMS: [char; 3] = [',', ';', '\t'];
    let mut counts = [0usize; 3];
    let mut quoted = false;
    for c in line.chars() {
        if c == '"' {
            quoted = !quoted;
        } else if !quoted && let Some(k) = DELIMS.iter().position(|&d| d == c) {
            counts[k] += 1;
        }
    }
    if counts == [0; 3] {
        return ',';
    }
    (0..3).max_by_key(|&k| counts[k]).map_or(',', |k| DELIMS[k])
}

// Header names, lower-cased with everything but letters and digits removed, best first.
const TITLE: &[&str] = &[
    "tracktitle",
    "trackname",
    "title",
    "songtitle",
    "songname",
    "name",
    "song",
    "track",
    "トラックタイトル",
    "タイトル",
    "曲名",
];
const ARTIST: &[&str] = &[
    "artistnames",
    "artistname",
    "artist",
    "artists",
    "trackartist",
    "trackartists",
    "artistsnames",
    "performer",
    "performers",
    "アーティスト",
];
const ALBUM: &[&str] = &["albumname", "album", "albumtitle", "release", "releasename", "releasetitle", "アルバム"];
const MIX: &[&str] = &["mixname", "mix", "version"];
const DURATION_MS: &[&str] = &["durationms", "trackdurationms", "lengthms", "timems"];
const DURATION: &[&str] = &["duration", "length", "time", "tracklength", "totaltime", "durationsecs", "時間"];
const ISRC: &[&str] = &["isrc", "isrccode"];
const PATH: &[&str] = &["location", "filepath", "path", "file", "filename", "filelocation", "fullpath"];

#[derive(Debug, Default)]
struct Columns {
    title: Option<usize>,
    artist: Option<usize>,
    album: Option<usize>,
    mix: Option<usize>,
    /// Column and whether it holds milliseconds.
    duration: Option<(usize, bool)>,
    isrc: Option<usize>,
    path: Option<usize>,
}

impl Columns {
    fn from_header(fields: &[String]) -> Columns {
        let names: Vec<String> = fields.iter().map(|f| header_name(f)).collect();
        let find = |synonyms: &[&str]| synonyms.iter().find_map(|s| names.iter().position(|n| n == s));
        Columns {
            title: find(TITLE),
            artist: find(ARTIST),
            album: find(ALBUM),
            mix: find(MIX),
            duration: find(DURATION_MS).map(|i| (i, true)).or_else(|| find(DURATION).map(|i| (i, false))),
            isrc: find(ISRC),
            path: find(PATH),
        }
    }

    fn recognised(&self) -> usize {
        [self.title, self.artist, self.album, self.mix, self.duration.map(|d| d.0), self.isrc, self.path]
            .iter()
            .filter(|c| c.is_some())
            .count()
    }
}

fn header_name(s: &str) -> String {
    s.trim().chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// rekordbox's export starts with its `#` column, then the browser's ("Track Title"...).
fn is_rekordbox_header(line: &str) -> bool {
    let names: Vec<String> = line.split('\t').map(header_name).collect();
    names.iter().any(|n| n == "tracktitle" || n == "トラックタイトル")
        || (line.split('\t').next().is_some_and(|f| f.trim() == "#")
            && names.iter().any(|n| TITLE.contains(&n.as_str())))
}

/// A first line that is clearly a table header (a title column and at least one more), for
/// files without a telling extension.
fn sniffed_header(first: &str) -> Option<Columns> {
    let record = read_table(first, sniff_delimiter(first), true).into_iter().next()?;
    let cols = Columns::from_header(&record.fields);
    (cols.title.is_some() && cols.recognised() >= 2).then_some(cols)
}

fn parse_table(text: &str, format: Format) -> Result<Vec<Entry>> {
    // Excel's "sep=;" first line names the delimiter.
    let (body, skipped, sep) = match text.split_once('\n') {
        Some((first, rest)) => match strip_prefix_ci(first.trim(), "sep=") {
            Some(d) if d.chars().count() == 1 => (rest, 1, d.chars().next()),
            _ => (text, 0, None),
        },
        None => (text, 0, None),
    };
    let first = body.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let rekordbox = format == Format::RekordboxText;
    let delim = sep.unwrap_or_else(|| if rekordbox { '\t' } else { sniff_delimiter(first) });
    // rekordbox doesn't quote, so a title like `"Heroes"` is literal.
    let records = read_table(body, delim, !rekordbox);
    // The header is the first record naming a title or file column (some exports put a
    // playlist name above it).
    let Some((h, cols)) = records
        .iter()
        .take(5)
        .enumerate()
        .map(|(i, r)| (i, Columns::from_header(&r.fields)))
        .find(|(_, c)| c.title.is_some() || c.path.is_some())
    else {
        let found = records.first().map(|r| r.fields.join(", ")).unwrap_or_default();
        return Err(Error::invalid(format!("no title or file column in the table's header (found: {found})")));
    };
    let mut out = Vec::new();
    for r in &records[h + 1..] {
        let get = |i: Option<usize>| i.and_then(|i| r.fields.get(i)).map(|s| s.trim()).filter(|s| !s.is_empty());
        let mut e = Entry {
            line: r.line + skipped,
            raw: r.raw.trim_end().to_string(),
            title: get(cols.title).unwrap_or_default().to_string(),
            artist: get(cols.artist).unwrap_or_default().to_string(),
            album: get(cols.album).map(String::from),
            mix: get(cols.mix).map(String::from),
            duration_secs: cols.duration.and_then(|(i, ms)| get(Some(i)).and_then(|v| parse_duration(v, ms))),
            path: get(cols.path).and_then(file_path),
            isrc: get(cols.isrc).map(String::from),
        };
        if e.title.is_empty() {
            if let Some(p) = &e.path {
                let from_name = location_entry(p);
                e.title = from_name.title;
                if e.artist.is_empty() {
                    e.artist = from_name.artist;
                }
            }
        } else if cols.artist.is_none() {
            (e.artist, e.title) = split_artist_title(&e.title, false);
        }
        if !e.title.is_empty() || e.path.is_some() {
            out.push(e);
        }
    }
    Ok(out)
}

/// "5:23", "1:02:03", "323" (seconds; milliseconds with `ms` or when implausibly long), "323.5".
fn parse_duration(s: &str, ms: bool) -> Option<u32> {
    let s = s.trim();
    if s.contains(':') {
        let (len, secs) = clock_prefix(s)?;
        let rest = &s[len..];
        return (rest.is_empty() || (rest.starts_with('.') && rest[1..].bytes().all(|b| b.is_ascii_digit())))
            .then_some(secs)
            .filter(|&v| v > 0);
    }
    let v: f64 = s.replace(',', ".").parse().ok()?;
    if !v.is_finite() || v <= 0.0 {
        return None;
    }
    let secs = if ms || v > 36_000.0 { v / 1000.0 } else { v };
    Some(secs.round() as u32).filter(|&v| v > 0)
}

// ---------------------------------------------------------------------------------------------
// rekordbox XML
// ---------------------------------------------------------------------------------------------

struct XmlDoc {
    collection: Vec<Entry>,
    playlists: Vec<(String, Vec<Entry>)>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum TagKind {
    Start,
    End,
    Empty,
}

struct Tag<'a> {
    name: &'a str,
    kind: TagKind,
    attrs: Vec<(&'a str, String)>,
    line: usize,
    raw: &'a str,
}

impl Tag<'_> {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| *n == name).map(|(_, v)| v.as_str())
    }
}

/// A minimal XML tag scanner: elements and their attributes (entities decoded), skipping text,
/// comments, CDATA, processing instructions and doctypes. Enough for rekordbox's regular format.
struct Scanner<'a> {
    text: &'a str,
    pos: usize,
    line: usize,
    counted: usize,
}

impl<'a> Scanner<'a> {
    fn new(text: &'a str) -> Self {
        Scanner { text, pos: 0, line: 1, counted: 0 }
    }

    fn next_tag(&mut self) -> Result<Option<Tag<'a>>> {
        let text = self.text;
        loop {
            let Some(off) = text[self.pos..].find('<') else { return Ok(None) };
            let start = self.pos + off;
            let rest = &text[start..];
            let skip = [("<!--", "-->"), ("<![CDATA[", "]]>"), ("<?", "?>"), ("<!", ">")]
                .iter()
                .find(|(open, _)| rest.starts_with(open))
                .map(|(_, close)| rest.find(close).map_or(text.len(), |i| start + i + close.len()));
            if let Some(next) = skip {
                self.pos = next;
                continue;
            }
            self.line += text[self.counted..start].matches('\n').count();
            self.counted = start;
            let line = self.line;
            let unterminated = || Error::invalid(format!("XML: unterminated tag on line {line}"));
            let b = rest.as_bytes();
            let end_tag = b.get(1) == Some(&b'/');
            let mut i = if end_tag { 2 } else { 1 };
            let name_start = i;
            while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' && b[i] != b'/' {
                i += 1;
            }
            let name = &rest[name_start..i];
            let mut attrs = Vec::new();
            let kind = loop {
                while i < b.len() && b[i].is_ascii_whitespace() {
                    i += 1;
                }
                match b.get(i) {
                    None => return Err(unterminated()),
                    Some(b'>') => {
                        i += 1;
                        break if end_tag { TagKind::End } else { TagKind::Start };
                    }
                    Some(b'/') if b.get(i + 1) == Some(&b'>') => {
                        i += 2;
                        break TagKind::Empty;
                    }
                    Some(_) => {
                        let attr_start = i;
                        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
                            i += 1;
                        }
                        let attr = &rest[attr_start..i];
                        while i < b.len() && b[i].is_ascii_whitespace() {
                            i += 1;
                        }
                        if b.get(i) != Some(&b'=') {
                            // Valueless attribute (not XML, but don't choke on it).
                            if attr.is_empty() {
                                i += 1;
                            }
                            attrs.push((attr, String::new()));
                            continue;
                        }
                        i += 1;
                        while i < b.len() && b[i].is_ascii_whitespace() {
                            i += 1;
                        }
                        let quote = *b.get(i).ok_or_else(unterminated)?;
                        if quote != b'"' && quote != b'\'' {
                            return Err(Error::invalid(format!("XML: unquoted attribute {attr} on line {line}")));
                        }
                        let len = rest[i + 1..].find(quote as char).ok_or_else(unterminated)?;
                        attrs.push((attr, unescape(&rest[i + 1..i + 1 + len])));
                        i += len + 2;
                    }
                }
            };
            self.pos = start + i;
            return Ok(Some(Tag { name, kind, attrs, line, raw: &rest[..i] }));
        }
    }
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let entity = rest.find(';').filter(|&j| j <= 10).and_then(|j| {
            let name = &rest[1..j];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => match name.strip_prefix('#') {
                    Some(n) => match n.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => n.parse().ok(),
                    }
                    .and_then(char::from_u32),
                    None => None,
                },
            };
            c.map(|c| (c, j))
        });
        match entity {
            Some((c, j)) => {
                out.push(c);
                rest = &rest[j + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

struct XmlNode {
    name: String,
    playlist: bool,
    /// `KeyType="1"`: the playlist's TRACK keys are Locations rather than TrackIDs.
    by_location: bool,
    /// (key, line) for each TRACK.
    keys: Vec<(String, usize)>,
}

fn parse_xml(text: &str) -> Result<XmlDoc> {
    let mut scanner = Scanner::new(text);
    let mut collection: Vec<Entry> = Vec::new();
    let mut by_id: HashMap<String, usize> = HashMap::new();
    let mut by_location: HashMap<String, usize> = HashMap::new();
    let mut stack: Vec<&str> = Vec::new();
    let mut nodes: Vec<XmlNode> = Vec::new();
    let mut lists: Vec<(String, XmlNode)> = Vec::new();
    let mut saw_root = false;

    // Folder path of the innermost node, without rekordbox's ROOT.
    let node_path = |nodes: &[XmlNode]| {
        let skip = usize::from(nodes.first().is_some_and(|n| n.name == "ROOT"));
        nodes[skip..].iter().map(|n| n.name.as_str()).collect::<Vec<_>>().join("/")
    };

    while let Some(tag) = scanner.next_tag()? {
        if tag.kind == TagKind::End {
            if tag.name == "NODE" && stack.contains(&"PLAYLISTS") && !nodes.is_empty() {
                let path = node_path(&nodes);
                if let Some(n) = nodes.pop().filter(|n| n.playlist) {
                    lists.push((path, n));
                }
            }
            if let Some(i) = stack.iter().rposition(|&s| s == tag.name) {
                stack.truncate(i);
            }
            continue;
        }
        let parent = stack.last().copied();
        match (tag.name, parent) {
            ("DJ_PLAYLISTS", _) => saw_root = true,
            ("TRACK", Some("COLLECTION")) => {
                let get = |name| tag.attr(name).map(str::trim).filter(|s| !s.is_empty());
                let e = Entry {
                    line: tag.line,
                    raw: tag.raw.to_string(),
                    title: get("Name").unwrap_or_default().to_string(),
                    artist: get("Artist").unwrap_or_default().to_string(),
                    album: get("Album").map(String::from),
                    mix: get("Mix").map(String::from),
                    duration_secs: get("TotalTime").and_then(|v| parse_duration(v, false)),
                    path: get("Location").and_then(file_path),
                    isrc: None,
                };
                if let Some(id) = get("TrackID") {
                    by_id.insert(id.to_string(), collection.len());
                }
                if let Some(loc) = get("Location") {
                    by_location.insert(loc.to_string(), collection.len());
                }
                collection.push(e);
            }
            ("NODE", _) if stack.contains(&"PLAYLISTS") => {
                nodes.push(XmlNode {
                    name: tag.attr("Name").unwrap_or_default().to_string(),
                    playlist: tag.attr("Type") == Some("1"),
                    by_location: tag.attr("KeyType") == Some("1"),
                    keys: Vec::new(),
                });
                if tag.kind == TagKind::Empty {
                    let path = node_path(&nodes);
                    if let Some(n) = nodes.pop().filter(|n| n.playlist) {
                        lists.push((path, n));
                    }
                }
            }
            ("TRACK", Some("NODE")) => {
                if let (Some(n), Some(key)) = (nodes.last_mut(), tag.attr("Key")) {
                    n.keys.push((key.to_string(), tag.line));
                }
            }
            _ => {}
        }
        if tag.kind == TagKind::Start {
            stack.push(tag.name);
        }
    }
    if !saw_root {
        return Err(Error::invalid("not a rekordbox XML file (no DJ_PLAYLISTS element)"));
    }

    // Playlist entries are the collection's, at the playlist's line; keys the collection lacks
    // stay as entries (with only a path, for location keys) so they're reported missing.
    let playlists = lists
        .into_iter()
        .map(|(path, node)| {
            let entries = node
                .keys
                .into_iter()
                .map(|(key, line)| {
                    let found = if node.by_location { by_location.get(&key) } else { by_id.get(&key) };
                    match found {
                        Some(&i) => Entry { line, ..collection[i].clone() },
                        None if node.by_location => {
                            Entry { line, raw: format!("Key=\"{key}\""), ..location_entry(&key) }
                        }
                        None => Entry { line, raw: format!("Key=\"{key}\""), ..Default::default() },
                    }
                })
                .collect();
            (path, entries)
        })
        .collect();
    Ok(XmlDoc { collection, playlists })
}

// ---------------------------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------------------------

/// How an entry was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Matched {
    pub entry: Entry,
    #[serde(flatten)]
    pub status: Status,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "ImportSummary")]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
const DURATION_SLACK: u32 = 2;
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
    by_name: HashMap<String, Vec<usize>>,
    by_isrc: HashMap<String, Vec<usize>>,
    by_title: HashMap<String, Vec<usize>>,
    by_token: HashMap<String, Vec<usize>>,
}

impl<'a> Matcher<'a> {
    pub fn new(lib: &'a Library) -> Matcher<'a> {
        let n = lib.tracks.len();
        let mut m = Matcher {
            lib,
            keys: Vec::with_capacity(n),
            parts: Vec::with_capacity(n),
            stems: Vec::with_capacity(n),
            by_name: HashMap::new(),
            by_isrc: HashMap::new(),
            by_title: HashMap::new(),
            by_token: HashMap::new(),
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
        let mut hits: HashMap<usize, usize> = HashMap::new();
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

// ---------------------------------------------------------------------------------------------
// Normalisation and similarity
// ---------------------------------------------------------------------------------------------

/// Words that mark a bracketed or dashed part of a title as a version ("(Tiesto Remix)",
/// "- Radio Edit").
const MIX_WORDS: &[&str] = &[
    "mix",
    "remix",
    "edit",
    "version",
    "dub",
    "vip",
    "rework",
    "bootleg",
    "flip",
    "refix",
    "instrumental",
    "acapella",
    "live",
    "remaster",
    "extended",
    "original",
    "radio",
    "club",
    "reprise",
    "mixed",
    "remixed",
    "acoustic",
    "unplugged",
    "demo",
    "mashup",
    "reedit",
    "remake",
];

/// Version words that don't make a different recording for a DJ's purposes, plus stop words.
const GENERIC: &[&str] = &[
    "original", "mix", "mixed", "extended", "radio", "edit", "version", "club", "main", "album", "single", "remaster",
    "digital", "explicit", "clean", "dirty", "short", "long", "full", "length", "cut", "edition", "mono", "stereo",
    "the", "a", "an", "and",
];

const FEAT_WORDS: &[&str] = &["feat", "ft", "featuring"];

fn is_generic(w: &str) -> bool {
    GENERIC.contains(&w) || (w.len() == 4 && (w.starts_with("19") || w.starts_with("20")) && digits_len(w) == 4)
}

/// [`normalize`] plus: accents dropped, apostrophes and dots removed ("Don't", "Mr."), "&" as
/// "and", other punctuation as spaces.
fn fold(s: &str) -> String {
    let lower = normalize(s);
    let mut out = String::with_capacity(lower.len());
    for c in lower.nfd() {
        match c {
            '\u{300}'..='\u{36f}' | '\'' | '\u{2018}' | '\u{2019}' | '`' | '\u{b4}' | '.' => {}
            '&' => out.push_str(" and "),
            c if c.is_alphanumeric() => out.push(c),
            _ => out.push(' '),
        }
    }
    let out: String = out.nfc().collect();
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Folded words, with spelling variants of version words unified.
fn tokens(s: &str) -> Vec<String> {
    fold(s)
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(|w| match w {
            "rmx" => "remix".to_string(),
            "acappella" | "accapella" => "acapella".to_string(),
            "remastered" => "remaster".to_string(),
            w => w.to_string(),
        })
        .collect()
}

/// The text after "feat."/"ft."/"featuring"/"with" at the start of `s` (lower-cased).
fn featuring(s: &str) -> Option<&str> {
    ["featuring ", "feat. ", "feat.", "feat ", "ft. ", "ft ", "with "].iter().find_map(|p| s.strip_prefix(p))
}

#[derive(Debug, Default)]
struct TitleParts {
    core: String,
    mix: Vec<String>,
    /// Other trailing bracketed text ("[Anjunadeep]", "(Part 1)").
    extra: Vec<String>,
    featured: Vec<String>,
}

/// Split a title into the song's name and what's said about this version of it: bracketed or
/// dashed version names ("(Extended Mix)", "- Radio Edit"), featured artists, and trailing
/// bracketed asides. Brackets that are part of the name ("(I Can't Get No) Satisfaction") stay.
fn title_parts(title: &str) -> TitleParts {
    let s = normalize(title);
    let mut parts = TitleParts::default();
    // Top-level bracket groups: (start, end, inner).
    let mut groups = Vec::new();
    let (mut depth, mut open) = (0usize, 0usize);
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' | '{' => {
                if depth == 0 {
                    open = i;
                }
                depth += 1;
            }
            ')' | ']' | '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    groups.push((open, i + 1, &s[open + 1..i]));
                }
            }
            _ => {}
        }
    }
    let mut trailing = vec![false; groups.len()];
    let mut tail = s.len();
    for k in (0..groups.len()).rev() {
        if !s[groups[k].1..tail].trim().is_empty() {
            break;
        }
        trailing[k] = true;
        tail = groups[k].0;
    }
    let mut core = String::new();
    let mut last = 0;
    for (k, &(start, end, inner)) in groups.iter().enumerate() {
        let inner = inner.trim();
        let kind = if let Some(f) = featuring(inner) {
            parts.featured.push(f.to_string());
            true
        } else if is_mixish(inner) {
            parts.mix.push(inner.to_string());
            true
        } else if trailing[k] {
            parts.extra.push(inner.to_string());
            true
        } else {
            false
        };
        if kind {
            core.push_str(&s[last..start]);
            core.push(' ');
            last = end;
        }
    }
    core.push_str(&s[last..]);
    // Spotify's "Title - Radio Edit", "Title - feat. X" (but "Dancing - With Myself" is a name).
    if let Some((i, n)) = [" - ", " – ", " — "].iter().filter_map(|sep| core.rfind(sep).map(|i| (i, sep.len()))).max()
    {
        let suffix = core[i + n..].trim();
        if let Some(f) = featuring(suffix).filter(|_| !suffix.starts_with("with ")) {
            parts.featured.push(f.to_string());
            core.truncate(i);
        } else if is_mixish(suffix) {
            parts.mix.push(suffix.to_string());
            core.truncate(i);
        }
    }
    // "Title feat. X" without brackets.
    if let Some(i) = [" featuring ", " feat. ", " feat ", " ft. "].iter().filter_map(|m| core.find(m)).min() {
        parts.featured.push(featuring(core[i + 1..].trim_start()).unwrap_or_default().to_string());
        core.truncate(i);
    }
    parts.core = core;
    parts
}

fn is_mixish(s: &str) -> bool {
    tokens(s).iter().any(|w| MIX_WORDS.contains(&w.as_str()))
}

/// Artist names, folded: "Daft Punk feat. Pharrell Williams & Nile Rodgers" gives three. A
/// leading "the" is dropped ("The Chemical Brothers" is "Chemical Brothers").
fn split_artists(s: &str) -> Vec<String> {
    let mut s = normalize(s);
    for sep in [
        " featuring ",
        " feat. ",
        " feat ",
        " ft. ",
        " ft ",
        " with ",
        " vs. ",
        " vs ",
        " x ",
        " and ",
        " presents ",
        " pres. ",
        " meets ",
    ] {
        s = s.replace(sep, "\u{1f}");
    }
    let mut out: Vec<String> = Vec::new();
    for part in s.split(['\u{1f}', ',', ';', '/', '&', '+', '(', ')', '[', ']', '、']) {
        let f = fold(part);
        let f = ["feat ", "ft ", "featuring ", "with "].iter().find_map(|p| f.strip_prefix(p)).unwrap_or(&f);
        let f = f.strip_prefix("the ").unwrap_or(f);
        if !f.is_empty() && !out.iter().any(|a| a == f) {
            out.push(f.to_string());
        }
    }
    out
}

/// One artist list contains the other.
fn artists_compatible(a: &[String], b: &[String]) -> bool {
    let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    !small.is_empty() && small.iter().all(|x| large.contains(x))
}

/// An entry or track reduced for comparison.
#[derive(Debug, Default)]
struct Key {
    /// Folded title without version, featured artists or trailing asides.
    title: String,
    title_tokens: Vec<String>,
    /// `title_tokens` plus the trailing asides ("(DVSMMM)"), which fuzzy matching weighs too.
    full_tokens: Vec<String>,
    /// Folded artist names, including artists featured in the title.
    artists: Vec<String>,
    artist_tokens: Vec<String>,
    /// Version words that make a different recording (remixer, "dub", "live"), sorted.
    mix: Vec<String>,
    /// Every version and aside word except "original"/"mix", sorted: tells "Extended Mix" from
    /// "Radio Edit" when both are in the library.
    variant: Vec<String>,
    /// Every meaningful word (no stop words, generic version words or "feat").
    bag: Vec<String>,
    duration: Option<u32>,
}

impl Key {
    fn new(title: &str, artist: &str, mix: &str, duration: Option<u32>) -> Key {
        let parts = title_parts(title);
        let mut core = fold(&parts.core);
        if core.is_empty() {
            core = fold(title);
        }
        let mut artists = split_artists(artist);
        for a in parts.featured.iter().flat_map(|f| split_artists(f)) {
            if !artists.contains(&a) {
                artists.push(a);
            }
        }
        // A mix name without version words is an aside (some taggers put a download site there).
        let (mix_mix, mix_extra) = if is_mixish(mix) { (Some(mix), None) } else { (None, Some(mix)) };
        let mix_words: Vec<String> = parts.mix.iter().map(String::as_str).chain(mix_mix).flat_map(tokens).collect();
        let extra_words: Vec<String> =
            parts.extra.iter().map(String::as_str).chain(mix_extra).flat_map(tokens).collect();
        let sorted = |mut v: Vec<String>| {
            v.sort();
            v.dedup();
            v
        };
        let distinct = sorted(mix_words.iter().filter(|w| !is_generic(w)).cloned().collect());
        let variant = sorted(
            mix_words
                .iter()
                .chain(&extra_words)
                .filter(|w| !matches!(w.as_str(), "original" | "mix"))
                .cloned()
                .collect(),
        );
        let title_tokens: Vec<String> = core.split(' ').filter(|w| !w.is_empty()).map(String::from).collect();
        let full_tokens: Vec<String> = title_tokens.iter().chain(&extra_words).cloned().collect();
        let artist_tokens: Vec<String> = artists.iter().flat_map(|a| a.split(' ')).map(String::from).collect();
        let mut bag: Vec<String> = Vec::new();
        for w in title_tokens.iter().chain(&mix_words).chain(&extra_words).chain(&artist_tokens) {
            if !is_generic(w) && !FEAT_WORDS.contains(&w.as_str()) && !bag.contains(w) {
                bag.push(w.clone());
            }
        }
        Key { title: core, title_tokens, full_tokens, artists, artist_tokens, mix: distinct, variant, bag, duration }
    }
}

/// How alike an entry and a track are, 0-1: title and artist similarity (also tried with the
/// two swapped, and as one bag of words for artists folded into titles), less a penalty when
/// their versions differ, nudged by duration.
fn score(e: &Key, t: &Key) -> f32 {
    let title = dice(&e.title_tokens, &t.title_tokens).max(dice(&e.full_tokens, &t.full_tokens));
    let structured = if e.artist_tokens.is_empty() || t.artist_tokens.is_empty() {
        0.85 * title
    } else {
        let straight = 0.6 * title + 0.4 * artist_similarity(&e.artist_tokens, &t.artist_tokens);
        let swapped =
            0.6 * dice(&e.artist_tokens, &t.title_tokens) + 0.4 * artist_similarity(&e.title_tokens, &t.artist_tokens);
        straight.max(swapped)
    };
    let mut s = structured.max(0.9 * dice(&e.bag, &t.bag));
    s -= match (e.mix.is_empty(), t.mix.is_empty()) {
        (true, true) => 0.0,
        (false, false) => (1.0 - dice(&e.mix, &t.mix)) * 0.4,
        // A remix asked for, the original found (or the reverse): never a confident match.
        _ => 0.3,
    };
    if let (Some(a), Some(b)) = (e.duration, t.duration) {
        let d = a.abs_diff(b);
        if d <= DURATION_SLACK {
            s += 0.03;
        } else if d > 60 {
            s -= 0.05;
        }
    }
    s.clamp(0.0, 1.0)
}

/// Artists: half overlap (Dice), half containment, so a full credit list still matches its
/// main artist.
fn artist_similarity(a: &[String], b: &[String]) -> f32 {
    let m = common_words(a, b) as f32;
    0.5 * dice(a, b) + 0.5 * m / a.len().min(b.len()).max(1) as f32
}

/// Sørensen-Dice coefficient over words, tolerating a typo in longer words.
fn dice(a: &[String], b: &[String]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    2.0 * common_words(a, b) as f32 / (a.len() + b.len()) as f32
}

/// Words of `a` paired with distinct words of `b`: exact pairs first, then near ones.
fn common_words(a: &[String], b: &[String]) -> usize {
    let mut used = vec![false; b.len()];
    let mut pending = Vec::new();
    let mut n = 0;
    for x in a {
        match b.iter().enumerate().position(|(j, y)| !used[j] && y == x) {
            Some(j) => {
                used[j] = true;
                n += 1;
            }
            None => pending.push(x),
        }
    }
    for x in pending {
        if let Some(j) = b.iter().enumerate().position(|(j, y)| !used[j] && near_words(x, y)) {
            used[j] = true;
            n += 1;
        }
    }
    n
}

/// One edit apart (two for long words), for words of 5+ letters without digits ("arround",
/// "colour"/"color"); numbers must match exactly.
fn near_words(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let shortest = a.len().min(b.len());
    if shortest < 5 || a.len().abs_diff(b.len()) > 2 || a.iter().chain(&b).any(char::is_ascii_digit) {
        return false;
    }
    let limit = if shortest >= 9 { 2 } else { 1 };
    // Levenshtein distance, one row at a time.
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut row = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            row[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(row[j] + 1);
        }
        prev = row;
    }
    prev[b.len()] <= limit
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Track;
    use crate::model::tests::track;

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

    fn utf16le(s: &str) -> Vec<u8> {
        let mut out = vec![0xFF, 0xFE];
        out.extend(s.encode_utf16().flat_map(u16::to_le_bytes));
        out
    }

    fn at(entries: &[Entry]) -> Vec<(String, String)> {
        entries.iter().map(|e| (e.artist.clone(), e.title.clone())).collect()
    }

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(a, t)| (a.to_string(), t.to_string())).collect()
    }

    // --- decoding and detection ---

    #[test]
    fn decodes_bom_utf16_and_legacy_encodings() {
        assert_eq!(text_of(&utf16le("a\r\nb")), "a\nb");
        let mut be = vec![0xFE, 0xFF];
        be.extend("Café".encode_utf16().flat_map(u16::to_be_bytes));
        assert_eq!(text_of(&be), "Café");
        // UTF-16 without a BOM
        let bare: Vec<u8> = "Artist - Title\n".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(text_of(&bare), "Artist - Title\n");
        assert_eq!(text_of(b"\xEF\xBB\xBFx"), "x");
        // Windows-1252: é, en dash, curly apostrophe
        assert_eq!(text_of(b"Caf\xe9 \x96 Don\x92t"), "Café – Don’t");
        assert_eq!(text_of(b"a\rb"), "a\nb");
    }

    #[test]
    fn detects_formats() {
        let d = |s: &str, hint: Option<&str>| detect(s.as_bytes(), hint);
        assert_eq!(d("<?xml version=\"1.0\"?><DJ_PLAYLISTS/>", None), Format::RekordboxXml);
        assert_eq!(d("#EXTM3U\n/a.mp3\n", Some("list.txt")), Format::M3u);
        assert_eq!(d("/Music/a.mp3\n/Music/b.mp3\n", Some("x.m3u8")), Format::M3u);
        assert_eq!(d("/Music/a.mp3\nC:\\Music\\b.flac\n", None), Format::M3u);
        assert_eq!(d("#\tTrack Title\tArtist\n1\tA\tB\n", Some("x.txt")), Format::RekordboxText);
        assert_eq!(detect(&utf16le("#\tTrack Title\tArtist\n"), None), Format::RekordboxText);
        assert_eq!(d("Track Name,Artist Name(s)\nx,y\n", None), Format::Csv);
        assert_eq!(d("anything", Some("CSV")), Format::Csv);
        assert_eq!(d("sep=;\nTitle;Artist\n", None), Format::Csv);
        assert_eq!(d("Daft Punk - One More Time\nAC/DC - Thunderstruck\n", Some("set.txt")), Format::Text);
        // A lone "Title" line isn't a table.
        assert_eq!(d("Title\nArtist - Song\n", None), Format::Text);
        assert_eq!(extension("/x/My.Playlist.M3U8"), "m3u8");
        assert_eq!(extension("xml"), "xml");
    }

    // --- M3U ---

    #[test]
    fn m3u_extinf_and_every_kind_of_path() {
        let m3u = "#EXTM3U\n\
                   #EXTINF:323,Daft Punk - One More Time\n\
                   /Users/me/Music/Daft Punk/Discovery/01 One More Time.mp3\n\
                   \n\
                   # a comment\n\
                   #EXTINF:-1 tvg-name=\"a,b\",Stream\n\
                   https://radio.example/stream.mp3?x=1\n\
                   file:///Users/me/Music/Caf%C3%A9%20del%20Mar.mp3\n\
                   file://localhost/C:/Music/Track%20Two.flac\n\
                   file://nas/share/x.mp3\n\
                   C:\\Music\\Artist - Song.mp3\n\
                   ../Music/03 - Moby - Porcelain.mp3\n\
                   Macintosh HD:Users:me:x.mp3\n";
        let e = parse(m3u.as_bytes(), Some("list.m3u8")).unwrap();
        assert_eq!(e.len(), 8);
        assert_eq!((e[0].line, e[0].duration_secs), (3, Some(323)));
        assert_eq!((e[0].artist.as_str(), e[0].title.as_str()), ("Daft Punk", "One More Time"));
        assert_eq!(e[0].path.as_deref(), Some("/Users/me/Music/Daft Punk/Discovery/01 One More Time.mp3"));
        assert_eq!(e[0].raw, "/Users/me/Music/Daft Punk/Discovery/01 One More Time.mp3");
        // A stream isn't a file; its EXTINF names it, and the quoted comma didn't split it.
        assert_eq!((e[1].path.as_deref(), e[1].title.as_str(), e[1].duration_secs), (None, "Stream", None));
        assert_eq!(e[2].path.as_deref(), Some("/Users/me/Music/Café del Mar.mp3"));
        assert_eq!(e[2].title, "Café del Mar");
        assert_eq!(e[3].path.as_deref(), Some("C:/Music/Track Two.flac"));
        assert_eq!(e[4].path.as_deref(), Some("//nas/share/x.mp3"));
        assert_eq!(e[5].path.as_deref(), Some("C:\\Music\\Artist - Song.mp3"));
        assert_eq!((e[5].artist.as_str(), e[5].title.as_str()), ("Artist", "Song"));
        // Track number dropped from the file name.
        assert_eq!((e[6].artist.as_str(), e[6].title.as_str()), ("Moby", "Porcelain"));
        assert_eq!(e[7].path.as_deref(), Some("/Macintosh HD/Users/me/x.mp3"));
        // One colon in a relative name is just a colon; other schemes aren't files.
        assert_eq!(file_path("Artist - Title: Remix.mp3").as_deref(), Some("Artist - Title: Remix.mp3"));
        assert_eq!(file_path("spotify:track:4uLU6hMCjMI75M1A2tKUQC"), None);
        assert_eq!(file_path("file:/Users/me/x.mp3").as_deref(), Some("/Users/me/x.mp3"));
    }

    #[test]
    fn m3u_album_tags_and_latin1() {
        let m3u = b"#EXTM3U\n#EXTALB:Discovery\n#EXTART:Daft Punk\n#EXTINF:200,Aerodynamic\nx.mp3\n\
                    #EXTINF:200,Caf\xe9 - Del Mar\ny.mp3\n";
        let e = parse(m3u, Some("x.m3u")).unwrap();
        assert_eq!(e[0].album.as_deref(), Some("Discovery"));
        assert_eq!((e[0].artist.as_str(), e[0].title.as_str()), ("Daft Punk", "Aerodynamic"));
        assert_eq!((e[1].artist.as_str(), e[1].title.as_str()), ("Café", "Del Mar"));
    }

    // --- tables ---

    #[test]
    fn exportify_csv() {
        let csv = "\u{feff}\"Track URI\",\"Track Name\",\"Album Name\",\"Artist Name(s)\",\"Release Date\",\"Duration (ms)\",\"Popularity\",\"ISRC\"\r\n\
                   \"spotify:track:1\",\"One More Time\",\"Discovery\",\"Daft Punk\",\"2001-03-12\",\"320357\",\"80\",\"GBDUW0000053\"\r\n\
                   \"spotify:track:2\",\"Get Lucky (feat. Pharrell Williams & Nile Rodgers) - Radio Edit\",\"Get Lucky\",\"Daft Punk,Pharrell Williams,Nile Rodgers\",\"2013\",\"248413\",\"85\",\"USQX91300108\"\r\n";
        let e = parse(csv.as_bytes(), Some("Liked.csv")).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!((e[0].line, e[0].title.as_str(), e[0].artist.as_str()), (2, "One More Time", "Daft Punk"));
        assert_eq!((e[0].album.as_deref(), e[0].duration_secs), (Some("Discovery"), Some(320)));
        assert_eq!(e[0].isrc.as_deref(), Some("GBDUW0000053"));
        assert_eq!(e[1].artist, "Daft Punk,Pharrell Williams,Nile Rodgers");
        assert_eq!(e[1].duration_secs, Some(248));
        assert!(e[1].raw.starts_with("\"spotify:track:2\""));
    }

    #[test]
    fn other_csv_dialects() {
        // TuneMyMusic
        let t = "Track name,Artist name,Album,Playlist name,Type,ISRC\nStrobe,deadmau5,For Lack,Set,Playlist,CA6D20900085\n";
        let e = parse(t.as_bytes(), Some("x.csv")).unwrap();
        assert_eq!((e[0].title.as_str(), e[0].artist.as_str()), ("Strobe", "deadmau5"));
        assert_eq!(e[0].isrc.as_deref(), Some("CA6D20900085"));
        // Beatport-style, semicolons, a mix column and a m:ss length
        let b = "Track;Artists;Mix;Label;Length\nOpus;Eric Prydz;Original Mix;Virgin;9:03\n";
        let e = parse(b.as_bytes(), None).unwrap();
        assert_eq!(
            (e[0].title.as_str(), e[0].mix.as_deref(), e[0].duration_secs),
            ("Opus", Some("Original Mix"), Some(543))
        );
        // Soundiiz-style lower case, tab separated, plus a file column
        let s = "title\tartist\talbum\tisrc\tlocation\nOpus\tEric Prydz\tOpus\t\tfile:///M/opus.mp3\n";
        let e = parse(s.as_bytes(), Some("x.tsv")).unwrap();
        assert_eq!((e[0].isrc.as_deref(), e[0].path.as_deref()), (None, Some("/M/opus.mp3")));
        // Excel's sep= line, and a header under a title row
        let x = "sep=;\nMy playlist\nTitle;Artist\nA;B\n";
        let e = parse(x.as_bytes(), Some("x.csv")).unwrap();
        assert_eq!((e[0].line, e[0].title.as_str(), e[0].artist.as_str()), (4, "A", "B"));
        // No title column at all
        let err = parse(b"foo,bar\n1,2\n", Some("x.csv")).unwrap_err();
        assert!(err.to_string().contains("foo, bar"), "{err}");
    }

    #[test]
    fn csv_quoting() {
        let csv = "Title,Artist,Album\n\
                   \"Hello, Goodbye\",\"The Beatles\",\"Magical \"\"Mystery\"\" Tour\"\n\
                   \"Two\nLines\",X, \"spaced\" \n\
                   \"half\"open,Y,\n\
                   \n\
                   ,,\n\
                   Last,Z";
        let e = parse(csv.as_bytes(), Some("x.csv")).unwrap();
        assert_eq!(e.len(), 4);
        assert_eq!(e[0].title, "Hello, Goodbye");
        assert_eq!(e[0].album.as_deref(), Some("Magical \"Mystery\" Tour"));
        assert_eq!((e[1].line, e[1].title.as_str(), e[1].album.as_deref()), (3, "Two\nLines", Some("spaced")));
        assert_eq!(e[1].raw, "\"Two\nLines\",X, \"spaced\"");
        assert_eq!((e[2].line, e[2].title.as_str()), (5, "halfopen"));
        assert_eq!((e[3].line, e[3].title.as_str(), e[3].artist.as_str()), (8, "Last", "Z"));
    }

    #[test]
    fn csv_without_artist_column_splits_titles() {
        let e = parse(b"Name\nDaft Punk - Da Funk\nIntro\n", Some("x.csv")).unwrap();
        assert_eq!(at(&e), pairs(&[("Daft Punk", "Da Funk"), ("", "Intro")]));
    }

    #[test]
    fn apple_music_text_export() {
        // Apple Music / iTunes "Export Playlist" text: UTF-16, tabs, Time in seconds, Location.
        let txt = "Name\tArtist\tComposer\tAlbum\tTime\tLocation\n\
                   Porcelain\tMoby\t\tPlay\t241\tMacintosh HD:Users:me:Music:Moby:Play:Porcelain.mp3\n";
        let e = parse(&utf16le(txt), Some("Playlist.txt")).unwrap();
        assert_eq!((e[0].title.as_str(), e[0].duration_secs), ("Porcelain", Some(241)));
        assert_eq!(e[0].path.as_deref(), Some("/Macintosh HD/Users/me/Music/Moby/Play/Porcelain.mp3"));
    }

    #[test]
    fn rekordbox_text_export() {
        let txt = "#\tArtwork\tTrack Title\tArtist\tAlbum\tGenre\tBPM\tRating\tTime\tKey\tDate Added\r\n\
                   1\t\t\"Heroes\" (Extended)\tDavid Bowie\tHeroes\tRock\t112.00\t***\t06:07\t4A\t2024-01-01\r\n\
                   2\t\tOpus\tEric Prydz\tOpus\tProgressive House\t126.00\t\t09:03\t8A\t2024-01-02\r\n";
        let e = parse(&utf16le(txt), Some("set.txt")).unwrap();
        assert_eq!(e.len(), 2);
        // Quotes are literal in rekordbox's export.
        assert_eq!(e[0].title, "\"Heroes\" (Extended)");
        assert_eq!((e[0].artist.as_str(), e[0].duration_secs, e[0].line), ("David Bowie", Some(367), 2));
        assert_eq!((e[1].title.as_str(), e[1].album.as_deref()), ("Opus", Some("Opus")));
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("5:23", false), Some(323));
        assert_eq!(parse_duration("1:02:03", false), Some(3723));
        assert_eq!(parse_duration("05:23.4", false), Some(323));
        assert_eq!(parse_duration("320357", true), Some(320));
        assert_eq!(parse_duration("320357", false), Some(320)); // too long to be seconds
        assert_eq!(parse_duration("323.6", false), Some(324));
        assert_eq!(parse_duration("0", false), None);
        assert_eq!(parse_duration("abc", false), None);
        assert_eq!(parse_duration("5:61", false), None);
    }

    // --- rekordbox XML ---

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <PRODUCT Name="rekordbox" Version="6.8.5" Company="AlphaTheta"/>
  <!-- <TRACK TrackID="99" Name="commented out"/> -->
  <COLLECTION Entries="3">
    <TRACK TrackID="1" Name="Rock &amp; Roll &#x2013; Part &#50;" Artist="Led Zeppelin" Album="IV" TotalTime="220"
      Location="file://localhost/Users/me/Music/Rock%20&amp;%20Roll.mp3" Mix="">
      <TEMPO Inizio="0.025" Bpm="170.00" Metro="4/4" Battito="1"/>
      <POSITION_MARK Name="" Type="0" Start="0.025" Num="-1"/>
    </TRACK>
    <TRACK TrackID="2" Name='Opus' Artist="Eric Prydz" TotalTime="543" Mix="Original Mix" Location="file://localhost/C:/Music/opus.mp3"/>
    <TRACK TrackID="3" Name="Strobe" Artist="deadmau5" TotalTime="637" Location="file://localhost/Volumes/USB/Contents/deadmau5/strobe.mp3"/>
  </COLLECTION>
  <PLAYLISTS>
    <NODE Type="0" Name="ROOT" Count="2">
      <NODE Type="0" Name="Shows" Count="2">
        <NODE Name="Warmup" Type="1" KeyType="0" Entries="3">
          <TRACK Key="3"/>
          <TRACK Key="1"/>
          <TRACK Key="42"/>
        </NODE>
        <NODE Name="Empty" Type="1" KeyType="0" Entries="0"/>
      </NODE>
      <NODE Name="By location" Type="1" KeyType="1" Entries="2">
        <TRACK Key="file://localhost/C:/Music/opus.mp3"/>
        <TRACK Key="file://localhost/gone/Artist%20-%20Gone.mp3"/>
      </NODE>
    </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>
"#;

    #[test]
    fn xml_collection() {
        let e = parse(XML.as_bytes(), None).unwrap();
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].title, "Rock & Roll – Part 2");
        assert_eq!(e[0].path.as_deref(), Some("/Users/me/Music/Rock & Roll.mp3"));
        assert_eq!(
            (e[0].duration_secs, e[0].line, e[0].album.as_deref(), e[0].mix.as_deref()),
            (Some(220), 6, Some("IV"), None)
        );
        assert!(e[0].raw.starts_with("<TRACK TrackID=\"1\"") && e[0].raw.ends_with('>'));
        assert_eq!((e[1].title.as_str(), e[1].mix.as_deref()), ("Opus", Some("Original Mix")));
        assert_eq!(e[1].path.as_deref(), Some("C:/Music/opus.mp3"));
        assert!(parse(b"<?xml version=\"1.0\"?><plist/>", None).is_err());
        assert!(parse(b"<?xml version=\"1.0\"?><DJ_PLAYLISTS><COLLECTION><TRACK Name=\"x", None).is_err());
    }

    #[test]
    fn xml_playlists_by_path() {
        let lists = xml_playlists(XML.as_bytes()).unwrap();
        let names: Vec<&str> = lists.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(names, ["Shows/Warmup", "Shows/Empty", "By location"]);
        let warmup = &lists[0].1;
        assert_eq!(at(warmup), pairs(&[("deadmau5", "Strobe"), ("Led Zeppelin", "Rock & Roll – Part 2"), ("", "")]));
        assert_eq!(warmup[0].line, 18);
        assert_eq!(warmup[2].raw, "Key=\"42\"");
        assert!(lists[1].1.is_empty());
        let by_loc = &lists[2].1;
        assert_eq!(by_loc[0].title, "Opus");
        assert_eq!((by_loc[1].artist.as_str(), by_loc[1].title.as_str()), ("Artist", "Gone"));
        assert_eq!(by_loc[1].path.as_deref(), Some("/gone/Artist - Gone.mp3"));
    }

    // --- plain text ---

    #[test]
    fn plain_text_lines() {
        let txt = "Tracklist:\n\
                   \n\
                   // made by hand\n\
                   # comment\n\
                   01. Daft Punk - One More Time\n\
                   2) Moby – Porcelain\n\
                   #3 Eric Prydz — Opus\n\
                   [04] deadmau5 - Strobe (10:37)\n\
                   [00:12:34] Bicep - Glue\n\
                   1:02:03 Four Tet - Baby\n\
                   05 [00:20:00] Floating Points - Silhouettes\n\
                   • Caribou - Sun\n\
                   - Burial - Archangel\n\
                   808 State - Pacific State\n\
                   10cc - I'm Not In Love\n\
                   Strobe by deadmau5\n\
                   Artist–Title\n\
                   Only A Title 4:05\n\
                   -----\n";
        let e = parse(txt.as_bytes(), Some("set.txt")).unwrap();
        assert_eq!(
            at(&e),
            pairs(&[
                ("Daft Punk", "One More Time"),
                ("Moby", "Porcelain"),
                ("Eric Prydz", "Opus"),
                ("deadmau5", "Strobe"),
                ("Bicep", "Glue"),
                ("Four Tet", "Baby"),
                ("Floating Points", "Silhouettes"),
                ("Caribou", "Sun"),
                ("Burial", "Archangel"),
                ("808 State", "Pacific State"),
                ("10cc", "I'm Not In Love"),
                ("deadmau5", "Strobe"),
                ("Artist", "Title"),
                ("", "Only A Title"),
            ])
        );
        assert_eq!((e[0].line, e[0].raw.as_str()), (5, "01. Daft Punk - One More Time"));
        assert_eq!(e[3].duration_secs, Some(637));
        assert_eq!(e[4].duration_secs, None); // a cue time, not a length
        assert_eq!(e[13].duration_secs, Some(245));
    }

    #[test]
    fn plain_text_bare_numbering() {
        // Every line numbered: the number goes, even before "2 Unlimited".
        let e = parse(b"1 Daft Punk - Da Funk\n2 2 Unlimited - Get Ready\n3 Moby - Go\n", Some("x.txt")).unwrap();
        assert_eq!(at(&e), pairs(&[("Daft Punk", "Da Funk"), ("2 Unlimited", "Get Ready"), ("Moby", "Go")]));
        // Not a numbered list: "2 Unlimited" keeps its 2.
        let e = parse(b"2 Unlimited - Get Ready\nMoby - Go\nBicep - Glue\n", Some("x.txt")).unwrap();
        assert_eq!(e[0].artist, "2 Unlimited");
        // "1 - Artist - Title"
        let e = parse(b"1 - Moby - Go\n", Some("x.txt")).unwrap();
        assert_eq!(at(&e), pairs(&[("Moby", "Go")]));
    }

    #[test]
    fn plain_text_paths() {
        let e = parse(b"Moby - Go\n/Users/me/Music/Bicep - Glue.mp3\n", Some("x.txt")).unwrap();
        assert_eq!(e[1].path.as_deref(), Some("/Users/me/Music/Bicep - Glue.mp3"));
        assert_eq!((e[1].artist.as_str(), e[1].title.as_str(), e[1].line), ("Bicep", "Glue", 2));
    }

    // --- normalisation ---

    #[test]
    fn titles_and_artists_normalise() {
        let k = Key::new("Get Lucky (feat. Pharrell Williams) [Radio Edit]", "Daft Punk", "", None);
        assert_eq!(k.title, "get lucky");
        assert_eq!(k.artists, ["daft punk", "pharrell williams"]);
        assert!(k.mix.is_empty());
        assert_eq!(k.variant, ["edit", "radio"]);
        let k = Key::new("Levels - Skrillex Remix", "Avicii", "", None);
        assert_eq!((k.title.as_str(), k.mix.clone()), ("levels", vec!["remix".to_string(), "skrillex".into()]));
        let k = Key::new("(I Can't Get No) Satisfaction - Mono Version", "The Rolling Stones", "", None);
        assert_eq!(
            (k.title.as_str(), k.artists.clone()),
            ("i cant get no satisfaction", vec!["rolling stones".to_string()])
        );
        let k = Key::new("Song Title feat. Somebody", "Árvore & Co.", "Original Mix", None);
        assert_eq!(
            (k.title.as_str(), k.artists.clone()),
            ("song title", vec!["arvore".to_string(), "co".into(), "somebody".into()])
        );
        assert!(k.mix.is_empty() && k.variant.is_empty());
        let k = Key::new("Track [Anjunadeep]", "A", "", None);
        assert_eq!((k.title.as_str(), k.variant.clone()), ("track", vec!["anjunadeep".to_string()]));
        // A title that is nothing but brackets keeps its words.
        assert_eq!(Key::new("(Untitled)", "", "", None).title, "untitled");
        let k = Key::new("Dancing - With Myself (feat. Someone) (with Other)", "Billy Idol", "", None);
        assert_eq!((k.title.as_str(), k.artists.len()), ("dancing with myself", 3));
        assert_eq!(split_artists("Simon and Garfunkel"), split_artists("Simon & Garfunkel"));
        assert_eq!(split_artists("Disclosure x Sam Smith"), ["disclosure", "sam smith"]);
        assert_eq!(split_artists("Malcolm X"), ["malcolm x"]);
        assert!(near_words("arround", "around") && near_words("colour", "color"));
        assert!(!near_words("2011", "2012") && !near_words("tone", "tune") && !near_words("strobe", "probes"));
    }

    // --- matching ---

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
