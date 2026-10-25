//! The item a set becomes on the Internet Archive: its identifier, and its metadata as the
//! `x-archive-meta-*` headers ias3 makes the item with (archive.org/developers/ias3.html). A
//! header's value is sent as it is when it's printable ASCII, else as `uri(...)` of it
//! percent-encoded, as the `internetarchive` library sends it, and each fits a header line.

use loclib_plugin::{PublishedSet, TracklistEntry};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use unicode_normalization::UnicodeNormalization;

/// The longest identifier. The Archive's S3 takes 3 to 80 characters (`internetarchive`'s
/// `validate_s3_identifier`) and its metadata 5 to 100: letters, digits, `-`, `_` and `.`,
/// starting with a letter or digit.
pub(crate) const MAX_IDENTIFIER: usize = 80;

/// How many identifiers are tried before giving up: the set's, then it with `-2` to `-10`.
pub(crate) const IDENTIFIERS: u32 = 10;

/// The most of an identifier the creator takes, so the date always fits after it.
const MAX_CREATOR: usize = 32;

/// The longest a header's value is sent. The Archive says of no limit, but the servers in front
/// of one commonly take header lines of 8 KiB (nginx's `large_client_header_buffers`, Apache's
/// `LimitRequestFieldSize`), which leaves room for the header's name.
pub(crate) const MAX_VALUE: usize = 8000;

/// Community Audio, the collection anyone may upload audio to.
pub(crate) const COLLECTION: &str = "opensource_audio";

/// What every set is listed under, before its kind, venue and city.
pub(crate) const TAG: &str = "dj mix";

/// Where a set's tracks part in the description, which the Archive shows as HTML.
const BREAK: &str = "<br>";

/// What a value percent-encoded in `uri(...)`, or a file's name in a URL, keeps as it is: what
/// Python's `quote` keeps.
pub(crate) const KEPT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

/// A set's item, and what was cut to fit, in a line each.
#[derive(Debug)]
pub(crate) struct Item {
    /// What it's called on the Archive, unless that's taken (see [`suffixed`]).
    pub(crate) identifier: String,
    pub(crate) title: String,
    /// The DJ, as the `creator` setting names them.
    pub(crate) creator: Option<String>,
    pub(crate) date: String,
    /// Empty for a set without a tracklist.
    pub(crate) description: String,
    pub(crate) subjects: Vec<String>,
    pub(crate) notes: Vec<String>,
}

impl Item {
    /// Its metadata as ias3's headers: one of each, and a numbered one for each subject.
    pub(crate) fn headers(&self) -> Vec<(String, String)> {
        let one = [
            ("mediatype", Some("audio")),
            ("collection", Some(COLLECTION)),
            ("title", Some(self.title.as_str())),
            ("date", Some(self.date.as_str())),
            ("creator", self.creator.as_deref()),
            ("description", Some(self.description.as_str()).filter(|d| !d.is_empty())),
        ];
        let one = one.into_iter().filter_map(|(name, value)| Some((format!("x-archive-meta-{name}"), value?)));
        let subjects =
            self.subjects.iter().enumerate().map(|(i, s)| (format!("x-archive-meta{:02}-subject", i + 1), s.as_str()));
        one.chain(subjects).map(|(name, value)| (name, header_value(value))).collect()
    }
}

/// `set`'s item, by the DJ `creator`: titled as the set, else where and when it was played
/// ("Warehouse, 2026-10-03"), its tracklist a line each, tagged as a DJ mix and with the set's
/// kind, venue and city. A tracklist too long for a header keeps the tracks that fit, saying how
/// many more there are.
pub(crate) fn of(set: &PublishedSet, creator: Option<&str>, tracklist: &[TracklistEntry]) -> Item {
    let given = |s: Option<&str>| s.map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let place = given(set.venue.as_deref()).or_else(|| given(set.city.as_deref()));
    let title = given(set.title.as_deref())
        .unwrap_or_else(|| format!("{}, {}", place.as_deref().unwrap_or("DJ set"), set.date));
    let creator = given(creator);
    let identifier = identifier(creator.as_deref(), &set.date, given(set.title.as_deref()).or(place).as_deref());
    let mut notes = Vec::new();
    let mut cut = |what: &str, text: String| {
        let (fitted, was_cut) = fit(text);
        if was_cut {
            notes.push(format!("the {what} is cut to the {MAX_VALUE} bytes a header holds"));
        }
        fitted
    };
    let title = cut("title", title);
    let creator = creator.map(|c| cut("creator", c));
    let date = cut("date", set.date.clone());
    let mut subjects: Vec<String> = vec![TAG.into()];
    for s in [&set.kind, &set.venue, &set.city].into_iter().filter_map(|s| given(s.as_deref())) {
        let s = cut("subject", s);
        if !subjects.iter().any(|had| had.eq_ignore_ascii_case(&s)) {
            subjects.push(s);
        }
    }
    let (description, note) = description(tracklist);
    notes.extend(note);
    Item { identifier, title, creator, date, description, subjects, notes }
}

/// The identifier a set's item is called by, `<creator>-<date>-<title>` (the title, else its
/// venue or city), each lowercase letters and digits between dashes (`dj-kit-2026-10-03-warehouse`),
/// `djset` for a set without a creator, and short enough to take a `-10`.
pub(crate) fn identifier(creator: Option<&str>, date: &str, title: Option<&str>) -> String {
    let mut creator = creator.map(slug).filter(|c| !c.is_empty()).unwrap_or_else(|| "djset".into());
    creator.truncate(MAX_CREATOR);
    let parts = [creator.trim_end_matches('-').to_string(), slug(date), title.map(slug).unwrap_or_default()];
    let mut id = parts.iter().filter(|p| !p.is_empty()).map(String::as_str).collect::<Vec<_>>().join("-");
    id.truncate(MAX_IDENTIFIER - suffix(IDENTIFIERS).len());
    id.trim_end_matches('-').to_string()
}

/// `id` as the `n`th identifier tried: as it is first, then with `-n`.
pub(crate) fn suffixed(id: &str, n: u32) -> String {
    format!("{id}{}", suffix(n))
}

fn suffix(n: u32) -> String {
    if n == 1 { String::new() } else { format!("-{n}") }
}

/// `text` in lowercase ASCII letters and digits, a dash between each run of them: accents taken
/// off, and what has no ASCII letter left out.
fn slug(text: &str) -> String {
    let ascii = text.nfd().filter(char::is_ascii).collect::<String>().to_ascii_lowercase();
    ascii.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-")
}

/// What `text` is sent as in a header: as it is when it's printable ASCII (and doesn't look
/// encoded), else as ias3's `uri(...)` of it percent-encoded.
pub(crate) fn header_value(text: &str) -> String {
    if quoted(text) { format!("uri({})", utf8_percent_encode(text, KEPT)) } else { text.into() }
}

fn quoted(text: &str) -> bool {
    !text.bytes().all(|b| b.is_ascii_graphic()) || text.starts_with("uri(")
}

/// How long `text` is once encoded, without `uri(` and `)`.
fn encoded_len(text: &str) -> usize {
    utf8_percent_encode(text, KEPT).map(str::len).sum()
}

/// `text` cut to what fits a header ([`MAX_VALUE`]), and whether it was.
fn fit(text: String) -> (String, bool) {
    if header_value(&text).len() <= MAX_VALUE {
        return (text, false);
    }
    // Cut as it's sent whole: a cut that no longer needs encoding is shorter still.
    let encoded = quoted(&text);
    let mut room = if encoded { MAX_VALUE - "uri()".len() } else { MAX_VALUE };
    let kept = text
        .char_indices()
        .take_while(|(_, c)| {
            let len = if encoded { encoded_len(c.encode_utf8(&mut [0; 4])) } else { 1 };
            room.checked_sub(len).map(|left| room = left).is_some()
        })
        .last()
        .map_or(0, |(at, c)| at + c.len_utf8());
    (text[..kept].to_string(), true)
}

/// The description: `tracklist` a line each, as HTML. One too long for a header keeps the lines
/// that fit, and a last saying how many more there are, which the note says too.
fn description(tracklist: &[TracklistEntry]) -> (String, Option<String>) {
    let lines: Vec<String> = tracklist.iter().map(|e| escape(&line(e))).collect();
    let all = lines.join(BREAK);
    if header_value(&all).len() <= MAX_VALUE {
        return (all, None);
    }
    // Every line has a space, so it's all encoded: each takes what it does encoded.
    let more = |kept: usize| format!("+ {} more", lines.len() - kept);
    let mut used = "uri()".len();
    let kept = lines
        .iter()
        .enumerate()
        .take_while(|(i, l)| {
            used += encoded_len(l) + encoded_len(BREAK);
            used + encoded_len(&more(i + 1)) <= MAX_VALUE
        })
        .count();
    let note = format!("the description lists {kept} of the {} tracks, as many as a header holds", lines.len());
    let description = lines[..kept].iter().map(|l| format!("{l}{BREAK}")).collect::<String>() + &more(kept);
    (description, Some(note))
}

/// `text` as HTML text.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A tracklist's line: `05:30 Artist - Title`, `1:02:03` past an hour, without a time when it's
/// not known. A track published unnamed is `ID - ID`, as DJs write one.
pub(crate) fn line(entry: &TracklistEntry) -> String {
    let named = |s: &Option<String>| s.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("ID").to_string();
    let name = format!("{} - {}", named(&entry.artist), named(&entry.title));
    match entry.start_seconds {
        Some(s) if s >= 3600 => format!("{}:{:02}:{:02} {name}", s / 3600, s / 60 % 60, s % 60),
        Some(s) => format!("{:02}:{:02} {name}", s / 60, s % 60),
        None => name,
    }
}
