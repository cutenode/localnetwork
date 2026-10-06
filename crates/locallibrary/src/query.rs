//! Finding and ordering tracks: the filter language (`bpm:120-128 key:8A~ rating:>=4 -genre:house`),
//! sort orders (`key,bpm` or `-rating`), and [`TrackSelector`], which every command that acts on
//! some tracks uses to say which.
//!
//! A filter is a list of terms that must all match. A bare word matches artist, title, album,
//! genre, label, key, comment, remixer, mix name or file path; `field:value` matches one field;
//! a leading `-` negates a term; `a|b` accepts either value. Quotes keep spaces in a value
//! (`artist:"daft punk"`).
//!
//! | Field | Values |
//! | --- | --- |
//! | `artist`, `title`, `album`, `genre`, `label`, `comment`, `remixer`, `mix`, `path` | text contained in the field |
//! | `key` | a key in any notation (`8A`, `Am`, `1m`); `key:8A~` also accepts keys that mix into it |
//! | `bpm` | `128`, `120-128`, `>=124`, `~128` (within 6%, half and double time included) |
//! | `rating`, `plays`, `year`, `bitrate` | a number, range or comparison |
//! | `time` | duration: `>6:00`, `3:00-5:00`, seconds |
//! | `color` (`colour`) | `pink` ... `purple`, or `none` |
//! | `added` | date added: `2026`, `2026-09`, `>=2026-01-01`, `2026-01..2026-03` |
//! | `playlist` | in a playlist (or anywhere below a folder), by path or unique name as `--playlist` takes it |
//! | `tag` | has a My Tag (`Category/Tag` or just the tag's name) |
//! | `is` | `unfiled` (in no playlist), `played`, `unplayed` |
//! | `has` | `artwork`, `comment`, `key`, `analysis` |
//!
//! A word before a colon that names no field (`gnere:house`) is refused rather than taken as text
//! to look for: as text it would match no track, and negated, every one. Quoted, the colon is
//! text (`"re:zero"`), as it is in a link (`https://...`).

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::device::Device;
use crate::harmony::{self, Camelot};
use crate::model::{Library, Playlist, Track, TrackColor, normalize};
use crate::plays::HistoryList;
use crate::{Error, Result};

// ---------------------------------------------------------------------------------------------
// Filters
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Artist,
    Title,
    Album,
    Genre,
    Label,
    Comment,
    Remixer,
    Mix,
    Path,
    Key,
    Bpm,
    Rating,
    Plays,
    Year,
    Bitrate,
    Time,
    Color,
    Added,
    Playlist,
    Tag,
    Is,
    Has,
}

/// Every name a field goes by.
const FIELDS: &[(&str, Field)] = &[
    ("artist", Field::Artist),
    ("a", Field::Artist),
    ("title", Field::Title),
    ("t", Field::Title),
    ("album", Field::Album),
    ("genre", Field::Genre),
    ("g", Field::Genre),
    ("label", Field::Label),
    ("comment", Field::Comment),
    ("c", Field::Comment),
    ("remixer", Field::Remixer),
    ("mix", Field::Mix),
    ("path", Field::Path),
    ("file", Field::Path),
    ("key", Field::Key),
    ("k", Field::Key),
    ("bpm", Field::Bpm),
    ("tempo", Field::Bpm),
    ("rating", Field::Rating),
    ("stars", Field::Rating),
    ("r", Field::Rating),
    ("plays", Field::Plays),
    ("playcount", Field::Plays),
    ("year", Field::Year),
    ("bitrate", Field::Bitrate),
    ("time", Field::Time),
    ("length", Field::Time),
    ("duration", Field::Time),
    ("color", Field::Color),
    ("colour", Field::Color),
    ("added", Field::Added),
    ("date", Field::Added),
    ("playlist", Field::Playlist),
    ("in", Field::Playlist),
    ("tag", Field::Tag),
    ("mytag", Field::Tag),
    ("is", Field::Is),
    ("has", Field::Has),
];

impl Field {
    fn parse(s: &str) -> Option<Field> {
        FIELDS.iter().find(|(name, _)| name.eq_ignore_ascii_case(s)).map(|(_, f)| *f)
    }
}

/// The one of `names` (lowercase) that `word` is most likely a typo of: two edits away from a
/// name of four letters or more, one from a shorter one (never a one-letter alias), or, from
/// three letters, the start of it. For "did you mean" hints, here for fields and in the command
/// line for commands.
pub fn nearest<'a>(word: &str, names: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let word = word.to_lowercase();
    names
        .into_iter()
        .map(|name| (strsim::osa_distance(&word, name), name))
        .filter(|(d, name)| *d <= 2.min(name.len() / 2) || (word.len() >= 3 && name.starts_with(&word)))
        .min_by_key(|(d, _)| *d)
        .map(|(_, name)| name)
}

/// The error for `word:value` where `word` names no field, with the field it most likely meant.
fn unknown_field(tok: &str, word: &str) -> Error {
    let near = nearest(word, FIELDS.iter().map(|(name, _)| *name))
        .map(|name| format!(" (did you mean {name}?)"))
        .unwrap_or_default();
    Error::invalid(format!(
        "can't read {tok:?}: there's no field {word:?}{near}; to look for the text, put it in quotes"
    ))
}

/// A numeric condition: `128`, `120-128`, `>=124`, `<5`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Num {
    Eq(f64, f64),
    Range(f64, f64),
    Gt(f64),
    Ge(f64),
    Lt(f64),
    Le(f64),
}

impl Num {
    /// `tolerance` widens an exact value (BPM 128 means 127.5-128.5).
    fn parse(s: &str, unit: impl Fn(&str) -> Option<f64>, tolerance: f64) -> Option<Num> {
        let s = s.trim();
        for (op, make) in [
            (">=", Num::Ge as fn(f64) -> Num),
            ("<=", Num::Le as fn(f64) -> Num),
            (">", Num::Gt as fn(f64) -> Num),
            ("<", Num::Lt as fn(f64) -> Num),
        ] {
            if let Some(rest) = s.strip_prefix(op) {
                return unit(rest).map(make);
            }
        }
        if let Some((a, b)) = s.split_once("..").or_else(|| s.split_once('-').filter(|(a, _)| !a.is_empty())) {
            let (a, b) = (unit(a)?, unit(b)?);
            return Some(Num::Range(a.min(b), a.max(b)));
        }
        unit(s).map(|v| Num::Eq(v, tolerance))
    }

    fn test(self, v: f64) -> bool {
        match self {
            Num::Eq(x, tol) => (v - x).abs() <= tol,
            Num::Range(a, b) => v >= a && v <= b,
            Num::Gt(x) => v > x,
            Num::Ge(x) => v >= x,
            Num::Lt(x) => v < x,
            Num::Le(x) => v <= x,
        }
    }
}

fn number(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// `6:00`, `1:02:03` or plain seconds.
fn seconds(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.trim().split(':').collect();
    if parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    // Like `number`, nothing that isn't finite (`time:nan`, `time:inf`, `1e400`).
    parts.iter().try_fold(0.0, |acc, p| Some(acc * 60.0 + p.parse::<f64>().ok()?)).filter(|v| v.is_finite())
}

/// A date (prefix) as a sortable number: `2026-09-30` -> 20260930, `2026-09` -> 20260900.
fn date_value(s: &str, end: bool) -> Option<f64> {
    let parts: Vec<&str> = s.trim().split(['-', '/']).collect();
    let y: u32 = parts.first()?.parse().ok()?;
    let m: u32 = parts.get(1).map_or(Some(if end { 12 } else { 1 }), |m| m.parse().ok())?;
    let d: u32 = parts.get(2).map_or(Some(if end { 31 } else { 1 }), |d| d.parse().ok())?;
    (parts.len() <= 3 && (1000..=9999).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d))
        // Lazily: out-of-range parts (`999999`) would overflow before the checks reject them.
        .then(|| (y * 10_000 + m * 100 + d) as f64)
}

/// A date condition, where a bare prefix (`2026-09`) means the whole month.
fn date_cond(s: &str) -> Option<Num> {
    let s = s.trim();
    for (op, end) in [(">=", false), ("<=", true), (">", true), ("<", false)] {
        if let Some(rest) = s.strip_prefix(op) {
            let v = date_value(rest, end)?;
            return Some(match op {
                ">=" => Num::Ge(v),
                "<=" => Num::Le(v),
                ">" => Num::Gt(v),
                _ => Num::Lt(v),
            });
        }
    }
    let (a, b) = s.split_once("..").unwrap_or((s, s));
    Some(Num::Range(date_value(a, false)?, date_value(b, true)?))
}

#[derive(Debug, Clone, PartialEq)]
enum Cond {
    /// Any of these (normalised) substrings.
    Text(Vec<String>),
    Num(Num),
    /// Keys: any of these, plus compatible ones when `mixable`.
    Key(Vec<Camelot>, bool),
    /// Within 6% of this tempo, half and double time included.
    Tempo(f64),
    Color(Vec<Option<TrackColor>>),
    /// In any of these playlists or below these folders, each as [`Library::find_playlist`]
    /// finds it.
    Playlist(Vec<String>),
    Tag(Vec<String>),
    Is(Vec<String>),
    Has(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
struct Term {
    negate: bool,
    field: Option<Field>,
    cond: Cond,
}

/// A parsed filter (see the module docs for the syntax).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    terms: Vec<Term>,
}

/// Split on whitespace, keeping quoted runs together (`artist:"daft punk"`), each token with
/// where in it its first colon outside quotes is: a quoted colon is text (`"re:zero"`).
fn tokens(s: &str) -> Vec<(String, Option<usize>)> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut colon = None;
    let mut quoted = false;
    for c in s.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push((std::mem::take(&mut cur), colon.take()));
                }
            }
            c => {
                if c == ':' && !quoted && colon.is_none() {
                    colon = Some(cur.len());
                }
                cur.push(c);
            }
        }
    }
    if !cur.is_empty() {
        out.push((cur, colon));
    }
    out
}

fn color_named(s: &str) -> Option<Option<TrackColor>> {
    Some(match s.to_ascii_lowercase().as_str() {
        "none" | "no" | "-" => None,
        "pink" => Some(TrackColor::Pink),
        "red" => Some(TrackColor::Red),
        "orange" => Some(TrackColor::Orange),
        "yellow" => Some(TrackColor::Yellow),
        "green" => Some(TrackColor::Green),
        "aqua" | "cyan" => Some(TrackColor::Aqua),
        "blue" => Some(TrackColor::Blue),
        "purple" | "violet" => Some(TrackColor::Purple),
        _ => return None,
    })
}

/// A colour by name (`aqua`) or rekordbox's number (1-8), or `none`.
pub fn parse_color(s: &str) -> Result<Option<TrackColor>> {
    if let Ok(n) = s.trim().parse::<u8>() {
        return match n {
            0 => Ok(None),
            n => TrackColor::from_id(n).map(Some).ok_or_else(|| Error::invalid(format!("no colour number {n}"))),
        };
    }
    color_named(s).ok_or_else(|| {
        Error::invalid(format!("unknown colour {s:?} (pink, red, orange, yellow, green, aqua, blue, purple or none)"))
    })
}

impl Filter {
    pub fn parse(s: &str) -> Result<Filter> {
        let mut terms = Vec::new();
        for (tok, colon) in tokens(s) {
            let (negate, tok, colon) = match tok.strip_prefix('-') {
                Some(rest) if !rest.is_empty() && !rest.starts_with(|c: char| c.is_ascii_digit()) => {
                    (true, rest, colon.map(|i| i - 1))
                }
                _ => (false, tok.as_str(), colon),
            };
            let bad = |what: &str| Error::invalid(format!("can't read {tok:?}: {what}"));
            let (field, value) = match colon.map(|i| tok.split_at(i)) {
                Some((f, v)) => match Field::parse(f) {
                    Some(field) => (Some(field), &v[1..]),
                    // A word that names no field is a typo, not text to look for (see the module
                    // docs); a time (`3:30`) or a link (`https://...`) is text.
                    None if !f.is_empty() && f.chars().all(char::is_alphabetic) && !v.starts_with("://") => {
                        return Err(unknown_field(tok, f));
                    }
                    None => (None, tok),
                },
                None => (None, tok),
            };
            if value.is_empty() {
                return Err(bad("nothing after the colon"));
            }
            // A term with nothing to look for (`genre:|`, `" "`) is refused: it would match no track,
            // and negated, every track.
            let alts = || {
                let alts: Vec<String> = value.split('|').map(normalize).filter(|v| !v.is_empty()).collect();
                if alts.is_empty() { Err(bad("nothing to look for")) } else { Ok(alts) }
            };
            let cond = match field {
                None
                | Some(
                    Field::Artist
                    | Field::Title
                    | Field::Album
                    | Field::Genre
                    | Field::Label
                    | Field::Comment
                    | Field::Remixer
                    | Field::Mix
                    | Field::Path,
                ) => Cond::Text(alts()?),
                Some(Field::Key) => {
                    let (value, mixable) = match value.strip_suffix('~') {
                        Some(v) => (v, true),
                        None => (value, false),
                    };
                    let keys = value
                        .split('|')
                        .map(|k| Camelot::parse(k).ok_or_else(|| bad("not a key (8A, Am, 1m...)")))
                        .collect::<Result<Vec<_>>>()?;
                    Cond::Key(keys, mixable)
                }
                Some(Field::Bpm) => match value.strip_prefix('~') {
                    Some(v) => Cond::Tempo(number(v).ok_or_else(|| bad("not a tempo"))?),
                    None => Cond::Num(Num::parse(value, number, 0.5).ok_or_else(|| bad("not a tempo or range"))?),
                },
                Some(Field::Rating | Field::Plays | Field::Year | Field::Bitrate) => {
                    Cond::Num(Num::parse(value, number, 0.0).ok_or_else(|| bad("not a number or range"))?)
                }
                Some(Field::Time) => {
                    Cond::Num(Num::parse(value, seconds, 0.5).ok_or_else(|| bad("not a duration (6:00, >5:30)"))?)
                }
                Some(Field::Added) => {
                    Cond::Num(date_cond(value).ok_or_else(|| bad("not a date (2026, 2026-09, >=2026-01-01)"))?)
                }
                Some(Field::Color) => Cond::Color(
                    value
                        .split('|')
                        .map(|c| color_named(c).ok_or_else(|| bad("not a colour")))
                        .collect::<Result<_>>()?,
                ),
                // As given: found the way `--playlist` finds one, letter case and all.
                Some(Field::Playlist) => {
                    let specs: Vec<String> =
                        value.split('|').map(str::trim).filter(|v| !v.is_empty()).map(str::to_string).collect();
                    if specs.is_empty() {
                        return Err(bad("nothing to look for"));
                    }
                    Cond::Playlist(specs)
                }
                Some(Field::Tag) => Cond::Tag(alts()?),
                Some(f @ (Field::Is | Field::Has)) => {
                    let known: &[&str] = if f == Field::Is {
                        &["unfiled", "played", "unplayed"]
                    } else {
                        &["artwork", "art", "comment", "key", "analysis"]
                    };
                    let vals = alts()?;
                    if let Some(v) = vals.iter().find(|v| !known.contains(&v.as_str())) {
                        return Err(bad(&format!("unknown value {v:?} (one of {})", known.join(", "))));
                    }
                    if f == Field::Is { Cond::Is(vals) } else { Cond::Has(vals) }
                }
            };
            terms.push(Term { negate, field, cond });
        }
        Ok(Filter { terms })
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Whether anything in the filter needs My Tags (so callers only read them when needed).
    pub fn uses_tags(&self) -> bool {
        self.terms.iter().any(|t| matches!(t.cond, Cond::Tag(_)))
    }

    /// Refuse a filter `ctx` can't answer: one naming a playlist the library doesn't have, or
    /// needing My Tags `ctx` wasn't given. Such a term would match no track, and negated, every
    /// track. Selecting checks this; [`Filter::matches`] doesn't.
    pub fn check(&self, ctx: &QueryContext<'_>) -> Result<()> {
        for term in &self.terms {
            match &term.cond {
                Cond::Playlist(specs) => {
                    if let Some(s) = specs.iter().find(|s| ctx.library.find_playlist(s).is_none()) {
                        return Err(Error::NotFound(format!("playlist {s:?}")));
                    }
                }
                Cond::Tag(_) if ctx.tags.is_none() => {
                    return Err(Error::invalid("a tag: term needs the drive's My Tags, and none were read"));
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn matches(&self, t: &Track, ctx: &QueryContext<'_>) -> bool {
        self.terms.iter().all(|term| term.matches(t, ctx) != term.negate)
    }

    /// The library's tracks this filter matches, in library order.
    pub fn select<'a>(&self, ctx: &QueryContext<'a>) -> Vec<&'a Track> {
        ctx.library.tracks.iter().filter(|t| self.matches(t, ctx)).collect()
    }
}

impl Term {
    fn matches(&self, t: &Track, ctx: &QueryContext<'_>) -> bool {
        let has = |hay: &str, needles: &[String]| {
            let hay = normalize(hay);
            needles.iter().any(|n| hay.contains(n.as_str()))
        };
        match &self.cond {
            Cond::Text(alts) => match self.field {
                None => {
                    let hay = ctx.haystack(t);
                    alts.iter().any(|n| hay.contains(n.as_str()))
                }
                Some(Field::Artist) => has(&t.artist, alts) || has(&t.album_artist, alts),
                Some(Field::Title) => has(&t.title, alts),
                Some(Field::Album) => has(&t.album, alts),
                Some(Field::Genre) => has(&t.genre, alts),
                Some(Field::Label) => has(&t.label, alts),
                Some(Field::Comment) => has(&t.comment, alts),
                Some(Field::Remixer) => has(&t.remixer, alts),
                Some(Field::Mix) => has(&t.mix_name, alts),
                Some(Field::Path) => has(&t.file_path, alts),
                _ => false,
            },
            Cond::Key(keys, mixable) => Camelot::parse(&t.key)
                .is_some_and(|k| keys.iter().any(|want| *want == k || (*mixable && want.relation(k).is_some()))),
            Cond::Tempo(bpm) => harmony::tempo_gap(*bpm, t.bpm) <= 0.06,
            Cond::Num(n) => {
                let v = match self.field {
                    Some(Field::Bpm) => t.bpm,
                    Some(Field::Rating) => t.rating.into(),
                    Some(Field::Plays) => t.play_count.into(),
                    Some(Field::Year) => t.year.into(),
                    Some(Field::Bitrate) => t.bitrate.into(),
                    Some(Field::Time) => t.duration_secs.into(),
                    Some(Field::Added) => match date_value(&t.date_added, false) {
                        Some(v) => v,
                        None => return false,
                    },
                    _ => return false,
                };
                n.test(v)
            }
            Cond::Color(cs) => cs.contains(&t.color),
            Cond::Playlist(specs) => specs.iter().any(|s| ctx.in_playlist(s, t.id)),
            Cond::Tag(tags) => ctx.tags_of(t).iter().any(|tag| {
                let tag = normalize(tag);
                let name = tag.rsplit('/').next().unwrap_or(&tag).to_string();
                tags.iter().any(|want| tag == *want || name == *want)
            }),
            Cond::Is(vals) => vals.iter().any(|v| match v.as_str() {
                "unfiled" => ctx.playlists_of(t.id).is_empty(),
                "played" => t.play_count > 0,
                _ => t.play_count == 0,
            }),
            Cond::Has(vals) => vals.iter().any(|v| match v.as_str() {
                "artwork" | "art" => t.artwork_id.is_some(),
                "comment" => !t.comment.trim().is_empty(),
                "key" => !t.key.trim().is_empty(),
                _ => !t.analyze_path.is_empty(),
            }),
        }
    }
}

/// What filters need besides the track itself, worked out lazily and once per library.
pub struct QueryContext<'a> {
    pub library: &'a Library,
    /// My Tags by lowercase file key (see [`crate::paths::key`]), as `Category/Tag`.
    pub tags: Option<&'a HashMap<String, Vec<String>>>,
    /// The drive's history lists, from both its databases (see [`DriveContext`]); without them,
    /// the library's own.
    history: Option<&'a [HistoryList]>,
    playlists: OnceCell<HashMap<u32, Vec<String>>>,
    haystacks: OnceCell<HashMap<u32, String>>,
    /// Track ids by lowercase file key (of several rows for one file, the last).
    files: OnceCell<HashMap<String, u32>>,
    /// The tracks of each playlist a `playlist:` term names (or below each folder).
    members: RefCell<HashMap<String, HashSet<u32>>>,
}

impl<'a> QueryContext<'a> {
    pub fn new(library: &'a Library) -> Self {
        QueryContext {
            library,
            tags: None,
            history: None,
            playlists: Default::default(),
            haystacks: Default::default(),
            files: Default::default(),
            members: Default::default(),
        }
    }

    pub fn with_tags(mut self, tags: &'a HashMap<String, Vec<String>>) -> Self {
        self.tags = Some(tags);
        self
    }

    /// The drive's history lists from both its databases, for history list selectors (see
    /// [`DriveContext`]).
    pub fn with_history(mut self, history: &'a [HistoryList]) -> Self {
        self.history = Some(history);
        self
    }

    /// The track whose audio file is `file` (a device path).
    fn by_file(&self, file: &str) -> Option<u32> {
        let key = |f: &str| crate::paths::key(f).to_lowercase();
        let map = self.files.get_or_init(|| self.library.tracks.iter().map(|t| (key(&t.file_path), t.id)).collect());
        map.get(&key(file)).copied()
    }

    /// Whether track `id` is in the playlist `spec` names, or below the folder it names.
    fn in_playlist(&self, spec: &str, id: u32) -> bool {
        let mut members = self.members.borrow_mut();
        if let Some(ids) = members.get(spec) {
            return ids.contains(&id);
        }
        let lib = self.library;
        let mut ids = HashSet::new();
        let mut queue: Vec<&Playlist> = lib.find_playlist(spec).into_iter().collect();
        let mut seen = HashSet::new();
        while let Some(p) = queue.pop() {
            // A cyclic tree (corrupt data) ends here.
            if seen.insert(p.id) {
                ids.extend(p.track_ids.iter().copied());
                if p.is_folder {
                    queue.extend(lib.children(Some(p.id)));
                }
            }
        }
        let found = ids.contains(&id);
        members.insert(spec.to_string(), ids);
        found
    }

    /// Paths of the playlists a track is in.
    pub fn playlists_of(&self, id: u32) -> &[String] {
        let map = self.playlists.get_or_init(|| {
            let mut map: HashMap<u32, Vec<String>> = HashMap::new();
            for p in self.library.playlists.iter().filter(|p| !p.is_folder) {
                let path = self.library.playlist_path(p.id);
                let mut seen = HashSet::new();
                for t in p.track_ids.iter().filter(|t| seen.insert(**t)) {
                    map.entry(*t).or_default().push(path.clone());
                }
            }
            map
        });
        map.get(&id).map(Vec::as_slice).unwrap_or(&[])
    }

    fn tags_of(&self, t: &Track) -> &[String] {
        self.tags.and_then(|m| m.get(&crate::paths::key(&t.file_path).to_lowercase())).map(Vec::as_slice).unwrap_or(&[])
    }

    fn haystack(&self, t: &Track) -> &str {
        let map = self.haystacks.get_or_init(|| {
            self.library
                .tracks
                .iter()
                .map(|t| {
                    let hay = [
                        &t.artist,
                        &t.title,
                        &t.album,
                        &t.genre,
                        &t.label,
                        &t.key,
                        &t.comment,
                        &t.remixer,
                        &t.mix_name,
                        &t.file_path,
                    ]
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join("\u{1f}");
                    (t.id, normalize(&hay))
                })
                .collect()
        });
        map.get(&t.id).map(String::as_str).unwrap_or("")
    }
}

// ---------------------------------------------------------------------------------------------
// Sorting
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    Title,
    Artist,
    Album,
    Genre,
    Label,
    /// Round the Camelot wheel: 1A, 1B, 2A...
    Key,
    Bpm,
    Rating,
    Color,
    Year,
    Plays,
    Added,
    Time,
    Bitrate,
    Path,
}

/// One level of a sort order, e.g. `-rating` (highest first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SortKey {
    pub field: SortField,
    #[serde(default)]
    pub descending: bool,
}

/// Parse `key,bpm` or `-rating,title` (a leading `-` sorts that level highest first).
pub fn parse_sort(s: &str) -> Result<Vec<SortKey>> {
    let keys: Vec<SortKey> = s
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (descending, name) = match p.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, p),
            };
            let field = match name.to_ascii_lowercase().as_str() {
                "title" => SortField::Title,
                "artist" => SortField::Artist,
                "album" => SortField::Album,
                "genre" => SortField::Genre,
                "label" => SortField::Label,
                "key" => SortField::Key,
                "bpm" | "tempo" => SortField::Bpm,
                "rating" | "stars" => SortField::Rating,
                "color" | "colour" => SortField::Color,
                "year" => SortField::Year,
                "plays" | "playcount" => SortField::Plays,
                "added" | "date" => SortField::Added,
                "time" | "length" | "duration" => SortField::Time,
                "bitrate" => SortField::Bitrate,
                "path" | "file" => SortField::Path,
                other => {
                    return Err(Error::invalid(format!(
                        "can't sort by {other:?} (title, artist, album, genre, label, key, bpm, rating, color, year, plays, added, time, bitrate, path)"
                    )));
                }
            };
            Ok(SortKey { field, descending })
        })
        .collect::<Result<_>>()?;
    if keys.is_empty() {
        return Err(Error::invalid("give a sort order, e.g. key,bpm"));
    }
    Ok(keys)
}

fn compare(a: &Track, b: &Track, field: SortField) -> std::cmp::Ordering {
    let text = |x: &str, y: &str| normalize(x).cmp(&normalize(y));
    match field {
        SortField::Title => text(&a.title, &b.title),
        SortField::Artist => text(&a.artist, &b.artist),
        SortField::Album => text(&a.album, &b.album),
        SortField::Genre => text(&a.genre, &b.genre),
        SortField::Label => text(&a.label, &b.label),
        SortField::Key => harmony::key_order(&a.key).cmp(&harmony::key_order(&b.key)),
        SortField::Bpm => a.bpm.total_cmp(&b.bpm),
        SortField::Rating => a.rating.cmp(&b.rating),
        SortField::Color => a.color.map_or(u8::MAX, TrackColor::id).cmp(&b.color.map_or(u8::MAX, TrackColor::id)),
        SortField::Year => a.year.cmp(&b.year),
        SortField::Plays => a.play_count.cmp(&b.play_count),
        SortField::Added => a.date_added.cmp(&b.date_added),
        SortField::Time => a.duration_secs.cmp(&b.duration_secs),
        SortField::Bitrate => a.bitrate.cmp(&b.bitrate),
        SortField::Path => crate::paths::key(&a.file_path).cmp(&crate::paths::key(&b.file_path)),
    }
}

/// Stable sort: tracks equal under every key keep their order.
pub fn sort_tracks(tracks: &mut [&Track], keys: &[SortKey]) {
    tracks.sort_by(|a, b| {
        keys.iter()
            .map(|k| {
                let o = compare(a, b, k.field);
                if k.descending { o.reverse() } else { o }
            })
            .find(|o| o.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

// ---------------------------------------------------------------------------------------------
// Selecting tracks
// ---------------------------------------------------------------------------------------------

/// Which tracks an operation targets: the tracks that fit every field given, so a playlist and a
/// filter together pick the playlist's tracks the filter matches. At least one field must be set,
/// and a blank query, filter, album, playlist or history list is refused rather than taken to
/// mean every track.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrackSelector {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<u32>,
    /// Matches "artist - title" or path substrings; must match at least one track.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playlist: Option<String>,
    /// Every track whose album has this name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    /// A filter (see [`crate::query`]) with at least one term; must match at least one track.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    /// Device paths of the audio files (`/Contents/...`), as stable across exports.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// The tracks of a history list (what a player recorded), by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<String>,
}

impl TrackSelector {
    /// Select tracks by their device paths.
    pub fn paths(paths: impl IntoIterator<Item = String>) -> TrackSelector {
        TrackSelector { paths: paths.into_iter().collect(), ..Default::default() }
    }

    /// No field is set. Such a selector picks no tracks: selecting with it is an error, never
    /// every track.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
            && self.query.is_none()
            && self.playlist.is_none()
            && self.album.is_none()
            && self.filter.is_none()
            && self.paths.is_empty()
            && self.history.is_none()
    }

    /// Whether the filter needs My Tags (a `tag:` term).
    pub fn uses_tags(&self) -> bool {
        self.filter.as_deref().and_then(|f| Filter::parse(f).ok()).is_some_and(|f| f.uses_tags())
    }

    /// The selected track ids, sorted, without repeats, from `lib` alone: a `tag:` term is
    /// refused, as there are no My Tags to match (see [`TrackSelector::select_on`]).
    pub fn select(&self, lib: &Library) -> Result<Vec<u32>> {
        self.select_with(&QueryContext::new(lib))
    }

    /// The selected track ids on `device`, whose library is `lib`, sorted, without repeats, with
    /// what the selector needs from the drive besides its library (see [`DriveContext`]).
    pub fn select_on(&self, device: &Device, lib: &Library) -> Result<Vec<u32>> {
        self.select_with(&DriveContext::read(device, [self])?.query(lib))
    }

    pub fn select_with(&self, ctx: &QueryContext<'_>) -> Result<Vec<u32>> {
        let mut ids = self.collect(ctx, false)?;
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }

    /// The selected track ids in a meaningful order without repeats: the order of the first of
    /// ids, paths, playlist and history list given, otherwise library order. With `allow_empty`,
    /// a query, album or filter that matches nothing (or fields no track fits together) selects
    /// nothing instead of failing.
    pub fn select_ordered(&self, ctx: &QueryContext<'_>, allow_empty: bool) -> Result<Vec<u32>> {
        let mut seen = HashSet::new();
        Ok(self.collect(ctx, allow_empty)?.into_iter().filter(|id| seen.insert(*id)).collect())
    }

    fn collect(&self, ctx: &QueryContext<'_>, allow_empty: bool) -> Result<Vec<u32>> {
        if self.is_empty() {
            return Err(Error::invalid("select tracks by id, query, filter, playlist, history, album or path"));
        }
        let lib = ctx.library;
        // `ids` without those no track has, in order.
        let alive = |ids: &[u32]| lib.tracks_by_id(ids).iter().map(|t| t.id).collect::<Vec<_>>();
        // Each field's tracks, those with an order of their own first.
        let mut parts: Vec<(&str, Vec<u32>)> = Vec::new();
        if !self.ids.is_empty() {
            let found = alive(&self.ids);
            // Where `found` first differs from the ids is the first id no track has.
            if let Some((_, id)) = self.ids.iter().enumerate().find(|(i, id)| found.get(*i) != Some(id)) {
                return Err(Error::NotFound(format!("track #{id}")));
            }
            parts.push(("ids", found));
        }
        if !self.paths.is_empty() {
            let found = self
                .paths
                .iter()
                .map(|p| ctx.by_file(p).ok_or_else(|| Error::NotFound(format!("no track at {p}"))))
                .collect::<Result<_>>()?;
            parts.push(("paths", found));
        }
        if let Some(p) = &self.playlist {
            given("playlist name", p)?;
            let pl = lib.find_playlist(p).ok_or_else(|| Error::NotFound(format!("playlist {p:?}")))?;
            // Entries whose track isn't in the collection (a damaged export) select nothing.
            parts.push(("playlist", alive(&pl.track_ids)));
        }
        if let Some(h) = &self.history {
            given("history list name", h)?;
            let missing = || Error::NotFound(format!("history list {h:?}"));
            let ids = match ctx.history {
                // Both databases' lists: their tracks are found here by file, as ids differ.
                Some(lists) => crate::plays::find_list(lists, h.trim())
                    .ok_or_else(missing)?
                    .tracks
                    .iter()
                    .filter_map(|t| ctx.by_file(&t.file_path))
                    .collect(),
                None => {
                    let lists = lib.history.iter().filter(|l| !l.is_folder).map(|l| (&l.name, l));
                    alive(&crate::model::pick(lists, h.trim()).ok_or_else(missing)?.track_ids)
                }
            };
            parts.push(("history list", ids));
        }
        if let Some(q) = &self.query {
            given("query", q)?;
            let found = lib.search(q);
            if found.is_empty() && !allow_empty {
                return Err(Error::NotFound(format!("no track matches {q:?}")));
            }
            parts.push(("query", found.iter().map(|t| t.id).collect()));
        }
        if let Some(a) = &self.album {
            given("album name", a)?;
            let needle = normalize(a);
            let found: Vec<u32> = lib.tracks.iter().filter(|t| normalize(&t.album) == needle).map(|t| t.id).collect();
            if found.is_empty() && !allow_empty {
                return Err(Error::NotFound(format!("no album named {a:?}")));
            }
            parts.push(("album", found));
        }
        if let Some(f) = &self.filter {
            let filter = Filter::parse(f)?;
            if filter.is_empty() {
                return Err(Error::invalid("the filter is empty: give at least one term, e.g. bpm:>=128"));
            }
            filter.check(ctx)?;
            let found = filter.select(ctx);
            if found.is_empty() && !allow_empty {
                return Err(Error::NotFound(format!("no track matches the filter {f:?}")));
            }
            parts.push(("filter", found.iter().map(|t| t.id).collect()));
        }
        let mut parts = parts.into_iter();
        let (first, mut ids) = parts.next().expect("a field is set");
        let mut names = vec![first];
        for (name, part) in parts {
            let part: HashSet<u32> = part.into_iter().collect();
            ids.retain(|id| part.contains(id));
            names.push(name);
        }
        if ids.is_empty() && names.len() > 1 && !allow_empty {
            let (last, rest) = names.split_last().expect("two or more");
            let rest = rest.join(", the ");
            return Err(Error::NotFound(format!("no track fits the {rest} and the {last} together")));
        }
        Ok(ids)
    }
}

/// What selecting tracks on a drive reads besides its library, each only when a selector needs
/// it: the drive's My Tags (for `tag:`), and the history lists of both its databases (players that
/// read only OneLibrary record theirs there, not in `export.pdb`).
#[derive(Debug, Default)]
pub struct DriveContext {
    tags: Option<HashMap<String, Vec<String>>>,
    history: Option<Vec<HistoryList>>,
}

impl DriveContext {
    /// Read what `selectors` need from `device`.
    pub fn read<'s>(device: &Device, selectors: impl IntoIterator<Item = &'s TrackSelector>) -> Result<DriveContext> {
        let (mut tags, mut history) = (false, false);
        for s in selectors {
            tags |= s.uses_tags();
            history |= s.history.is_some();
        }
        Ok(DriveContext {
            tags: tags.then(|| crate::tags::by_track(device)).transpose()?,
            history: history.then(|| crate::plays::drive_history(device).map(|h| h.lists)).transpose()?,
        })
    }

    /// A context for selecting on `library` (the drive's) with what was read.
    pub fn query<'a>(&'a self, library: &'a Library) -> QueryContext<'a> {
        QueryContext { tags: self.tags.as_ref(), history: self.history.as_deref(), ..QueryContext::new(library) }
    }
}

/// Refuse a blank text field of a selector: an empty search or album name would take every track.
fn given(what: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::invalid(format!("the {what} is empty: say which tracks")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Playlist;
    use crate::model::tests::track;

    fn lib() -> Library {
        let mut a = track(1, "Daft Punk", "Around the World", "/Contents/a.mp3");
        a.key = "8A".into();
        a.bpm = 121.0;
        a.rating = 5;
        a.genre = "House".into();
        a.color = Some(TrackColor::Aqua);
        a.date_added = "2026-09-30".into();
        a.duration_secs = 429;
        let mut b = track(2, "Floorplan", "Never Grow Old", "/Contents/b.mp3");
        b.key = "Am".into();
        b.bpm = 128.0;
        b.rating = 3;
        b.genre = "Techno".into();
        b.play_count = 4;
        b.date_added = "2025-01-02".into();
        let mut c = track(3, "Nobody", "Untitled", "/Contents/c.mp3");
        c.key = "3B".into();
        c.bpm = 64.0;
        Library {
            tracks: vec![a, b, c],
            playlists: vec![
                Playlist {
                    id: 1,
                    parent_id: None,
                    name: "Sets".into(),
                    is_folder: true,
                    sort_order: 0,
                    track_ids: vec![],
                },
                Playlist {
                    id: 2,
                    parent_id: Some(1),
                    name: "Friday".into(),
                    is_folder: false,
                    sort_order: 0,
                    track_ids: vec![2, 1, 2],
                },
            ],
            ..Default::default()
        }
    }

    fn ids(f: &str) -> Vec<u32> {
        let lib = lib();
        let ctx = QueryContext::new(&lib);
        Filter::parse(f).unwrap().select(&ctx).iter().map(|t| t.id).collect()
    }

    #[test]
    fn filters() {
        assert_eq!(ids("daft"), [1]);
        assert_eq!(ids("artist:\"daft punk\""), [1]);
        assert_eq!(ids("-genre:house"), [2, 3]);
        assert_eq!(ids("genre:house|techno"), [1, 2]);
        assert_eq!(ids("key:8A"), [1, 2], "Am is 8A");
        assert_eq!(ids("key:9A~"), [1, 2]);
        assert_eq!(ids("bpm:120-125"), [1]);
        assert_eq!(ids("bpm:128"), [2]);
        assert_eq!(ids("bpm:>=124"), [2]);
        assert_eq!(ids("bpm:~130"), [2, 3], "64 is half time");
        assert_eq!(ids("rating:>=4"), [1]);
        assert_eq!(ids("plays:0"), [1, 3]);
        assert_eq!(ids("color:aqua"), [1]);
        assert_eq!(ids("color:none"), [2, 3]);
        assert_eq!(ids("added:2026"), [1]);
        assert_eq!(ids("added:2025-01..2025-12"), [2]);
        assert_eq!(ids("added:>=2026-01-01"), [1]);
        assert_eq!(ids("time:>6:00"), [1]);
        assert_eq!(ids("playlist:sets"), [1, 2]);
        assert_eq!(ids("playlist:sets/friday"), [1, 2]);
        assert_eq!(ids("is:unfiled"), [3]);
        assert_eq!(ids("is:played"), [2]);
        assert_eq!(ids("has:key -key:8A"), [3]);
        assert_eq!(ids(""), [1, 2, 3], "a filter with no terms lists everything (a selector refuses it)");
        for bad in
            ["key:H", "bpm:fast", "color:mauve", "is:loud", "rating:", "added:yesterday", "-genre:|", "-\" \"", "is:|"]
        {
            assert!(Filter::parse(bad).is_err(), "{bad}");
        }
    }

    /// Regression: a word before a colon that named no field was text to look for, so
    /// `-gnere:house` matched every track; `playlist:` compared full paths only, so a nested
    /// playlist's name matched nothing, and negated, everything.
    #[test]
    fn filters_fail_closed() {
        let e = Filter::parse("bpm:>120 -gnere:house").unwrap_err().to_string();
        assert!(e.contains("no field \"gnere\" (did you mean genre?)"), "{e}");
        let e = Filter::parse("dur:>6:00").unwrap_err().to_string();
        assert!(e.contains("(did you mean duration?)"), "the start of a field's name: {e}");
        let e = Filter::parse("x:1").unwrap_err().to_string();
        assert!(!e.contains("did you mean"), "not a one-letter alias: {e}");
        assert!(Filter::parse("bogus:1").unwrap_err().to_string().contains("no field \"bogus\";"));
        // A colon in quotes, after something that isn't a word, or in a field's value is text.
        assert_eq!(ids("\"daft:\""), Vec::<u32>::new());
        assert_eq!(ids("-\"never:grow\""), [1, 2, 3]);
        assert_eq!(ids("3:30 -c:30"), Vec::<u32>::new());
        assert_eq!(ids("-https://example.com"), [1, 2, 3]);
        assert_eq!(ids("artist:\"daft punk\""), [1]);

        assert_eq!(ids("playlist:Friday"), [1, 2], "by name, as --playlist takes it");
        assert_eq!(ids("-playlist:friday"), [3]);
        assert_eq!(ids("playlist:Sets/"), [1, 2], "below a folder");
        let lib = lib();
        let ctx = QueryContext::new(&lib);
        let select = |f: &str| TrackSelector { filter: Some(f.into()), ..Default::default() }.select_with(&ctx);
        assert_eq!(select("-playlist:Friday").unwrap(), [3]);
        let e = select("-playlist:Saturday|Friday").unwrap_err().to_string();
        assert!(e.contains("playlist \"Saturday\""), "{e}");
        // My Tags that weren't read can't be matched.
        assert!(select("-tag:deep").is_err());
    }

    /// Regression: each entry of a playlist was looked up with a scan of the library, so
    /// selecting a long playlist on a big library took minutes.
    #[test]
    fn long_playlists_are_selected_in_one_pass() {
        let n = 50_000u32;
        let lib = Library {
            tracks: (1..=n).map(|i| track(i, "A", "T", &format!("/Contents/{i}.mp3"))).collect(),
            playlists: vec![Playlist {
                id: 1,
                parent_id: None,
                name: "All".into(),
                is_folder: false,
                sort_order: 0,
                track_ids: (1..=n).rev().collect(),
            }],
            ..Default::default()
        };
        let start = std::time::Instant::now();
        let all = TrackSelector { playlist: Some("All".into()), ..Default::default() };
        assert_eq!(all.select_ordered(&QueryContext::new(&lib), false).unwrap().len(), n as usize);
        let tracks = lib.tracks_by_id(&lib.playlists[0].track_ids);
        assert_eq!((tracks.len(), tracks[0].id), (n as usize, n));
        assert!(start.elapsed() < std::time::Duration::from_secs(2), "{:?}", start.elapsed());
    }

    /// Regression: a history list a player recorded only in OneLibrary, on a drive that has
    /// `export.pdb` too, couldn't be selected.
    #[test]
    fn history_lists_of_both_databases_are_selected_by_file() {
        let lib = lib();
        // OneLibrary's rows: other ids, the same files.
        let mut played = vec![lib.tracks[2].clone(), lib.tracks[0].clone()];
        played[0].id = 70;
        played[1].id = 71;
        let lists = [HistoryList {
            name: "HISTORY 003".into(),
            path: "HISTORY 003".into(),
            database: "onelibrary",
            tracks: played,
            missing: 0,
            missing_at: Vec::new(),
        }];
        let sel = TrackSelector { history: Some("history 003".into()), ..Default::default() };
        assert_eq!(sel.select_ordered(&QueryContext::new(&lib).with_history(&lists), false).unwrap(), [3, 1]);
        assert!(sel.select(&lib).is_err(), "the library's own lists don't have it");
    }

    #[test]
    fn tags_filter_uses_the_tag_map() {
        let lib = lib();
        let tags = HashMap::from([("contents/b.mp3".to_string(), vec!["Genre/Deep".to_string()])]);
        let ctx = QueryContext::new(&lib).with_tags(&tags);
        let f = Filter::parse("tag:deep").unwrap();
        assert!(f.uses_tags());
        assert_eq!(f.select(&ctx).iter().map(|t| t.id).collect::<Vec<_>>(), [2]);
        let f = Filter::parse("tag:genre/deep").unwrap();
        assert_eq!(f.select(&ctx).len(), 1);
    }

    #[test]
    fn sorting() {
        let lib = lib();
        let mut v: Vec<&Track> = lib.tracks.iter().collect();
        sort_tracks(&mut v, &parse_sort("key,bpm").unwrap());
        assert_eq!(v.iter().map(|t| t.id).collect::<Vec<_>>(), [3, 1, 2]);
        sort_tracks(&mut v, &parse_sort("-rating").unwrap());
        assert_eq!(v.iter().map(|t| t.id).collect::<Vec<_>>(), [1, 2, 3]);
        assert!(parse_sort("loudness").is_err());
        assert!(parse_sort("").is_err());
    }

    #[test]
    fn selectors() {
        let lib = lib();
        let ctx = QueryContext::new(&lib);
        let s = TrackSelector { filter: Some("genre:techno".into()), ..Default::default() };
        assert_eq!(s.select(&lib).unwrap(), [2]);
        assert!(TrackSelector::default().select(&lib).is_err());
        // Fields given together narrow the selection, in the order of the first ordered one.
        let s = TrackSelector { paths: vec!["/contents/C.mp3".into()], ids: vec![3, 1], ..Default::default() };
        assert_eq!(s.select(&lib).unwrap(), [3]);
        let friday = |s: TrackSelector| TrackSelector { playlist: Some("Friday".into()), ..s };
        let s = friday(TrackSelector { filter: Some("bpm:>=100".into()), ..Default::default() });
        assert_eq!(s.select_ordered(&ctx, false).unwrap(), [2, 1]);
        let s = friday(TrackSelector { ids: vec![1, 3, 2], ..Default::default() });
        assert_eq!(s.select_ordered(&ctx, false).unwrap(), [1, 2]);
        let s = friday(TrackSelector { ids: vec![3], ..Default::default() });
        let e = s.select_ordered(&ctx, false).unwrap_err().to_string();
        assert!(e.contains("no track fits the ids and the playlist together"), "{e}");
        assert!(s.select_ordered(&ctx, true).unwrap().is_empty());
        let s = TrackSelector { filter: Some("genre:polka".into()), ..Default::default() };
        assert!(s.select_ordered(&ctx, false).is_err());
        assert!(s.select_ordered(&ctx, true).unwrap().is_empty());
        assert!(TrackSelector { ids: vec![99], ..Default::default() }.select(&lib).is_err());
        assert_eq!(parse_color("6").unwrap(), Some(TrackColor::Aqua));
        assert_eq!(parse_color("none").unwrap(), None);
        assert!(parse_color("9").is_err());
    }

    /// Regression: an empty filter or query selected every track, so `track rm --filter ""`
    /// planned to delete the whole library.
    #[test]
    fn blank_selectors_select_nothing() {
        let lib = lib();
        let ctx = QueryContext::new(&lib);
        let text = |b: &str| Some(b.to_string());
        for b in ["", "  ", "\t"] {
            for s in [
                TrackSelector { filter: text(b), ..Default::default() },
                TrackSelector { query: text(b), ..Default::default() },
                TrackSelector { album: text(b), ..Default::default() },
                TrackSelector { playlist: text(b), ..Default::default() },
                TrackSelector { history: text(b), ..Default::default() },
                TrackSelector { filter: text(b), ids: vec![1], ..Default::default() },
                TrackSelector { query: text(b), playlist: text("Friday"), ..Default::default() },
            ] {
                assert!(s.select_ordered(&ctx, true).is_err(), "{s:?}");
            }
        }
        let quotes = TrackSelector { filter: text("\"\""), ..Default::default() };
        assert!(quotes.select_ordered(&ctx, true).is_err());
    }

    /// Regression: a history list's name matched ignoring case before an exact match was looked
    /// for.
    #[test]
    fn history_lists_are_selected_by_exact_name_first() {
        let mut lib = lib();
        lib.history = ["HISTORY 001", "history 001"]
            .into_iter()
            .zip(1..)
            .map(|(name, id)| Playlist {
                id,
                parent_id: None,
                name: name.into(),
                is_folder: false,
                sort_order: id,
                track_ids: vec![id],
            })
            .collect();
        let history = |name: &str| TrackSelector { history: Some(name.into()), ..Default::default() }.select(&lib);
        assert_eq!(history("history 001").unwrap(), [2]);
        assert!(history("History 001").is_err(), "two match ignoring case");
    }
}
