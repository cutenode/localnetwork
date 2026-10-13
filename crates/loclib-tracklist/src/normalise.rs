//! Normalising titles and artists, and how alike two are.

use loclib_model::model::normalize;
use unicode_normalization::UnicodeNormalization;

use crate::matching::DURATION_SLACK;
use crate::plain::digits_len;

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
    let mut push = |c| match c {
        '\u{300}'..='\u{36f}' | '\'' | '\u{2018}' | '\u{2019}' | '`' | '\u{b4}' | '.' => {}
        '&' => out.push_str(" and "),
        c if c.is_alphanumeric() => out.push(c),
        _ => out.push(' '),
    };
    // ASCII has no accents to take apart or put back together.
    if lower.is_ascii() {
        lower.chars().for_each(&mut push);
    } else {
        lower.nfd().for_each(&mut push);
    }
    let out: String = if out.is_ascii() { out } else { out.nfc().collect() };
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Folded words, with spelling variants of version words unified.
pub(crate) fn tokens(s: &str) -> Vec<String> {
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
        // Most names hold none of them: looking is cheaper than copying the name.
        if s.contains(sep) {
            s = s.replace(sep, "\u{1f}");
        }
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
pub(crate) fn artists_compatible(a: &[String], b: &[String]) -> bool {
    let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    !small.is_empty() && small.iter().all(|x| large.contains(x))
}

/// An entry or track reduced for comparison.
#[derive(Debug, Default)]
pub(crate) struct Key {
    /// Folded title without version, featured artists or trailing asides.
    pub(crate) title: String,
    pub(crate) title_tokens: Vec<String>,
    /// `title_tokens` plus the trailing asides ("(DVSMMM)"), which fuzzy matching weighs too.
    pub(crate) full_tokens: Vec<String>,
    /// Folded artist names, including artists featured in the title.
    pub(crate) artists: Vec<String>,
    pub(crate) artist_tokens: Vec<String>,
    /// Version words that make a different recording (remixer, "dub", "live"), sorted.
    pub(crate) mix: Vec<String>,
    /// Every version and aside word except "original"/"mix", sorted: tells "Extended Mix" from
    /// "Radio Edit" when both are in the library.
    pub(crate) variant: Vec<String>,
    /// Every meaningful word (no stop words, generic version words or "feat").
    pub(crate) bag: Vec<String>,
    pub(crate) duration: Option<u32>,
}

impl Key {
    pub(crate) fn new(title: &str, artist: &str, mix: &str, duration: Option<u32>) -> Key {
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

/// What a version unlike the one asked for costs a score: enough that a fuzzy match of it can't
/// be confident, however alike the rest is.
const MIX_MISMATCH: f32 = 0.3;

/// How alike an entry and a track are, 0-1: title and artist similarity (also tried with the
/// two swapped, and as one bag of words for artists folded into titles), less a penalty when
/// their versions differ, nudged by duration.
pub(crate) fn score(e: &Key, t: &Key) -> f32 {
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
        (false, false) => match dice(&e.mix, &t.mix) {
            // The same version, at most a typo apart.
            1.0.. => 0.0,
            // Another version ("Aurora Remix" for "Basalt Remix") is another recording, as much
            // as the original is: sharing "remix" mustn't bring it up to a confident match.
            d => ((1.0 - d) * 0.4).max(MIX_MISMATCH),
        },
        // A remix asked for, the original found (or the reverse): never a confident match.
        _ => MIX_MISMATCH,
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
    let m = common_words(a, b);
    0.5 * dice_of(a, b, m) + 0.5 * m as f32 / a.len().min(b.len()).max(1) as f32
}

/// Sørensen-Dice coefficient over words, tolerating a typo in longer words.
fn dice(a: &[String], b: &[String]) -> f32 {
    dice_of(a, b, common_words(a, b))
}

/// [`dice`] with the words `a` and `b` share already counted.
fn dice_of(a: &[String], b: &[String], common: usize) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    2.0 * common as f32 / (a.len() + b.len()) as f32
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
    // Most pairs fail these, so they're checked before the words are copied out as characters,
    // the cheapest first (a word has no more characters than bytes).
    if a.len().min(b.len()) < 5 || a.bytes().chain(b.bytes()).any(|c| c.is_ascii_digit()) {
        return false;
    }
    let (a_len, b_len) = (a.chars().count(), b.chars().count());
    let shortest = a_len.min(b_len);
    if shortest < 5 || a_len.abs_diff(b_len) > 2 {
        return false;
    }
    let b: Vec<char> = b.chars().collect();
    let limit = if shortest >= 9 { 2 } else { 1 };
    // Levenshtein distance, one row at a time.
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut row = vec![0; b.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        row[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            row[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(row[j] + 1);
        }
        std::mem::swap(&mut prev, &mut row);
    }
    prev[b.len()] <= limit
}

#[cfg(test)]
mod tests;
