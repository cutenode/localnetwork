//! Tables: CSV and rekordbox's text export.

use crate::detection::{file_path, strip_prefix_ci};
use crate::m3u::location_entry;
use crate::plain::{clock_prefix, split_artist_title};
use crate::{Entry, Error, Format, Result};

/// One record of a table, as [`read_table`] reads it.
pub struct Record<'a> {
    /// 1-based line the record starts on.
    pub line: usize,
    /// The record's text, as in the file.
    pub raw: &'a str,
    pub fields: Vec<String>,
}

/// RFC 4180 with leniency: quotes only open at the start of a field (after optional spaces),
/// `""` inside quotes is a quote, text after a closing quote is kept, and quoted fields may span
/// lines. Without `quoting` (rekordbox's text), quotes are ordinary characters. Blank records
/// are dropped.
pub fn read_table(text: &str, delim: char, quoting: bool) -> Vec<Record<'_>> {
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
pub(crate) struct Columns {
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
pub(crate) fn is_rekordbox_header(line: &str) -> bool {
    let names: Vec<String> = line.split('\t').map(header_name).collect();
    names.iter().any(|n| n == "tracktitle" || n == "トラックタイトル")
        || (line.split('\t').next().is_some_and(|f| f.trim() == "#")
            && names.iter().any(|n| TITLE.contains(&n.as_str())))
}

/// A first line that is clearly a table header (a title column and at least one more), for
/// files without a telling extension.
pub(crate) fn sniffed_header(first: &str) -> Option<Columns> {
    let record = read_table(first, sniff_delimiter(first), true).into_iter().next()?;
    let cols = Columns::from_header(&record.fields);
    (cols.title.is_some() && cols.recognised() >= 2).then_some(cols)
}

pub(crate) fn parse_table(text: &str, format: Format) -> Result<Vec<Entry>> {
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
        return Err(Error::NoTitleColumn { found });
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
pub(crate) fn parse_duration(s: &str, ms: bool) -> Option<u32> {
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

#[cfg(test)]
mod tests;
