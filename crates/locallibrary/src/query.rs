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
//! | `PLUGIN.FIELD` (`sound.bpm`) | what an analyzer plugin worked out: a number, range or comparison for a number (`sound.bpm:>120`; `128` is 127.5 to 128.5, `128.4` is 128.35 to 128.45), words for text (`sound.key:8A`), `true` or `false` |
//!
//! A word before a colon that names no field (`gnere:house`) is refused rather than taken as text
//! to look for: as text it would match no track, and negated, every one. Quoted, the colon is
//! text (`"re:zero"`), as it is in a link (`https://...`). A plugin's field is refused unless an
//! analyzer installed declares it, and a filter naming one needs what analyzers worked out (see
//! [`crate::fields::TrackValues`]), as `tag:` needs My Tags.
//!
//! Sort orders name a plugin's field the same way (`-sound.bpm`), where its values are at hand
//! ([`parse_sort_with`]).

use rustc_hash::{FxHashMap, FxHashSet};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::cell::{OnceCell, RefCell};

use crate::device::Device;
use crate::fields::{Declared, FieldType, FieldValue, TrackValues};
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

/// The error for `word:value` where `word` names no field, with the field (of `names`) it most
/// likely meant.
fn unknown_field<'a>(tok: &str, word: &str, names: impl IntoIterator<Item = &'a str>) -> Error {
    let near = nearest(word, names).map(|name| format!(" (did you mean {name}?)")).unwrap_or_default();
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
    /// A plugin's field (`sound.bpm`): its value as a number condition, when it reads as one, and
    /// as words.
    Plugin {
        name: String,
        num: Option<Num>,
        text: Vec<String>,
    },
}

/// `word` (before a colon) as a plugin's field: two names joined by a dot (`sound.bpm`), each
/// as plugin ids and field names are, in lowercase.
fn plugin_field(word: &str) -> Option<String> {
    let word = word.to_ascii_lowercase();
    let (plugin, field) = word.split_once('.')?;
    let name = |s: &str| {
        s.starts_with(|c: char| c.is_ascii_lowercase())
            && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    };
    (name(plugin) && name(field)).then_some(word)
}

/// How far from a number typed (`128.4`) a value still matches it: half its last digit.
fn precision(typed: &str) -> f64 {
    let decimals = typed.trim().split_once('.').map_or(0, |(_, d)| d.len());
    0.5 / 10f64.powi(decimals.min(12) as i32)
}

/// Whether `text` (normalised) has `word` (normalised) as whole words: `8a` is in `8a`, never in
/// `18a`.
fn has_words(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let after = text[i + word.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        before && after
    })
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
                        return Err(unknown_field(tok, f, FIELDS.iter().map(|(name, _)| *name)));
                    }
                    // A plugin's field (`sound.bpm`), found once its values are at hand.
                    None if plugin_field(f).is_some() => {
                        let value = &v[1..];
                        if value.is_empty() {
                            return Err(bad("nothing after the colon"));
                        }
                        let name = plugin_field(f).expect("one");
                        let num = Num::parse(value, number, precision(value));
                        let text = value.split('|').map(normalize).filter(|v| !v.is_empty()).collect();
                        terms.push(Term { negate, field: None, cond: Cond::Plugin { name, num, text } });
                        continue;
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

    /// Whether anything in the filter needs what analyzers worked out (a plugin's field).
    pub fn uses_fields(&self) -> bool {
        self.terms.iter().any(|t| matches!(t.cond, Cond::Plugin { .. }))
    }

    /// Refuse a filter `ctx` can't answer: one naming a playlist the library doesn't have, or
    /// needing My Tags or analyzers' values `ctx` wasn't given, or a plugin's field no analyzer
    /// declares, or a value its field can't hold. Such a term would match no track, and negated,
    /// every track. Selecting checks this; [`Filter::matches`] doesn't.
    pub fn check(&self, ctx: &QueryContext<'_>) -> Result<()> {
        for term in &self.terms {
            match &term.cond {
                Cond::Plugin { name, .. } if ctx.fields.is_none() => {
                    return Err(Error::invalid(format!(
                        "a {name}: term needs what analyzers worked out, and none was read"
                    )));
                }
                Cond::Plugin { name, num, text } => {
                    let declared = &ctx.fields.expect("given").declared;
                    let Some(&kind) = declared.get(name) else {
                        return Err(unknown_field(name, name, declared.keys().map(String::as_str)));
                    };
                    let bad = |what: &str| Err(Error::invalid(format!("can't read the {name} term: {what}")));
                    let switch = |t: &String| ["true", "false", "yes", "no"].contains(&t.as_str());
                    match kind {
                        FieldType::Number | FieldType::Integer if num.is_none() => {
                            return bad("it's a number: give one, a range or a comparison (>120, 120-128)");
                        }
                        FieldType::Boolean if text.is_empty() || !text.iter().all(switch) => {
                            return bad("it's true or false");
                        }
                        _ if text.is_empty() && num.is_none() => return bad("nothing to look for"),
                        _ => {}
                    }
                }
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
            Cond::Plugin { name, num, text } => {
                let words = |s: &str| {
                    let s = normalize(s);
                    text.iter().any(|w| has_words(&s, w))
                };
                match ctx.fields.and_then(|f| f.get(t.id, name)) {
                    // Worked out, with nothing to show: it matches no comparison. Nor does a
                    // beat grid, phrases or a waveform, which are no columns.
                    None | Some(FieldValue::Null | FieldValue::Structured(_)) => false,
                    Some(FieldValue::Integer(v)) => num.is_some_and(|n| n.test(*v as f64)),
                    Some(FieldValue::Number(v)) => num.is_some_and(|n| n.test(*v)),
                    Some(FieldValue::Boolean(b)) => {
                        let says = |w: &String| matches!(w.as_str(), "true" | "yes") == *b;
                        text.iter().any(|w| ["true", "false", "yes", "no"].contains(&w.as_str()) && says(w))
                    }
                    Some(FieldValue::String(s)) => words(s),
                    Some(FieldValue::Strings(all)) => all.iter().any(|s| words(s)),
                }
            }
        }
    }
}

/// What filters need besides the track itself, worked out lazily and once per library.
pub struct QueryContext<'a> {
    pub library: &'a Library,
    /// My Tags by lowercase file key (see [`crate::paths::key`]), as `Category/Tag`.
    pub tags: Option<&'a FxHashMap<String, Vec<String>>>,
    /// What analyzers worked out for the tracks, for plugins' fields (`sound.bpm`).
    pub fields: Option<&'a TrackValues>,
    /// The drive's history lists, from both its databases (see [`DriveContext`]); without them,
    /// the library's own.
    history: Option<&'a [HistoryList]>,
    playlists: OnceCell<FxHashMap<u32, Vec<String>>>,
    haystacks: OnceCell<FxHashMap<u32, String>>,
    /// Track ids by lowercase file key (of several rows for one file, the last).
    files: OnceCell<FxHashMap<String, u32>>,
    /// The tracks of each playlist a `playlist:` term names (or below each folder).
    members: RefCell<FxHashMap<String, FxHashSet<u32>>>,
}

impl<'a> QueryContext<'a> {
    pub fn new(library: &'a Library) -> Self {
        QueryContext {
            library,
            tags: None,
            fields: None,
            history: None,
            playlists: Default::default(),
            haystacks: Default::default(),
            files: Default::default(),
            members: Default::default(),
        }
    }

    pub fn with_tags(mut self, tags: &'a FxHashMap<String, Vec<String>>) -> Self {
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
        let key = crate::paths::file_key;
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
        let mut ids = FxHashSet::default();
        let mut queue: Vec<&Playlist> = lib.find_playlist(spec).into_iter().collect();
        let mut seen = FxHashSet::default();
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
            let mut map: FxHashMap<u32, Vec<String>> = FxHashMap::default();
            for p in self.library.playlists.iter().filter(|p| !p.is_folder) {
                let path = self.library.playlist_path(p.id);
                let mut seen = FxHashSet::default();
                for t in p.track_ids.iter().filter(|t| seen.insert(**t)) {
                    map.entry(*t).or_default().push(path.clone());
                }
            }
            map
        });
        map.get(&id).map(Vec::as_slice).unwrap_or(&[])
    }

    fn tags_of(&self, t: &Track) -> &[String] {
        self.tags.and_then(|m| m.get(&crate::paths::file_key(&t.file_path))).map(Vec::as_slice).unwrap_or(&[])
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
    /// A plugin's field: tracks without a value for it go last.
    Field(FieldName),
}

/// A plugin's field as a sort names it (`sound.bpm`), kept for good: sort keys stay `Copy`
/// (front ends pass them about), and there are few names to keep, the fields the plugins declare
/// and those a saved sort names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct FieldName(&'static str);

impl FieldName {
    pub fn new(name: &str) -> FieldName {
        static NAMES: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
        let mut names = NAMES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let kept = names.iter().find(|n| **n == name).copied().unwrap_or_else(|| {
            let n: &'static str = Box::leak(name.to_string().into_boxed_str());
            names.push(n);
            n
        });
        FieldName(kept)
    }

    pub fn as_str(self) -> &'static str {
        self.0
    }
}

impl<'de> Deserialize<'de> for FieldName {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<FieldName, D::Error> {
        String::deserialize(d).map(|s| FieldName::new(&s))
    }
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
    parse_sort_with(s, None)
}

/// [`parse_sort`], taking the plugins' fields `declared` (`-sound.bpm`) too, when they're given.
pub fn parse_sort_with(s: &str, declared: Option<&Declared>) -> Result<Vec<SortKey>> {
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
                other if plugin_field(other).is_some() => match declared {
                    Some(d) if d.contains_key(other) => SortField::Field(FieldName::new(other)),
                    Some(d) => return Err(unknown_field(other, other, d.keys().map(String::as_str))),
                    None => {
                        return Err(Error::invalid(format!(
                            "can't sort by {other:?} here: a plugin's field sorts track listings only"
                        )));
                    }
                },
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

/// How two values of a plugin's field compare: numbers by size, text by its words, a list by its
/// first, `false` before `true`.
pub fn compare_values(a: &FieldValue, b: &FieldValue) -> std::cmp::Ordering {
    let number = |v: &FieldValue| match v {
        FieldValue::Integer(n) => Some(*n as f64),
        FieldValue::Number(n) => Some(*n),
        _ => None,
    };
    let text = |v: &FieldValue| match v {
        FieldValue::String(s) => normalize(s),
        FieldValue::Strings(all) => all.first().map(|s| normalize(s)).unwrap_or_default(),
        FieldValue::Boolean(b) => b.to_string(),
        n => number(n).map(|n| n.to_string()).unwrap_or_default(),
    };
    match (number(a), number(b)) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        _ => text(a).cmp(&text(b)),
    }
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
        // By its values (see `sort_tracks_with`).
        SortField::Field(_) => std::cmp::Ordering::Equal,
    }
}

/// Stable sort: tracks equal under every key keep their order.
pub fn sort_tracks(tracks: &mut [&Track], keys: &[SortKey]) {
    sort_tracks_with(tracks, keys, None);
}

/// [`sort_tracks`] with what analyzers worked out, for keys naming a plugin's field: tracks
/// without a value for it to show go last, either way (all of them, without `values`).
pub fn sort_tracks_with(tracks: &mut [&Track], keys: &[SortKey], values: Option<&TrackValues>) {
    tracks.sort_by(|a, b| {
        keys.iter()
            .map(|k| {
                let directed = |o: std::cmp::Ordering| if k.descending { o.reverse() } else { o };
                match k.field {
                    // Those without a value last, whichever way it's sorted.
                    SortField::Field(name) => {
                        let value = |t: &Track| values.and_then(|v| v.shown(t.id, name.as_str()));
                        match (value(a), value(b)) {
                            (Some(x), Some(y)) => directed(compare_values(x, y)),
                            (x, y) => x.is_none().cmp(&y.is_none()),
                        }
                    }
                    field => directed(compare(a, b, field)),
                }
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
        let mut seen = FxHashSet::default();
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
                return Err(Error::no_track(*id));
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
            let part: FxHashSet<u32> = part.into_iter().collect();
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
    tags: Option<FxHashMap<String, Vec<String>>>,
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
mod tests;
