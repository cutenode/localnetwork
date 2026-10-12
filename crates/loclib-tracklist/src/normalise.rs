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

    use hegel::generators as gs;
    use hegel::{Generator, TestCase};

    /// Words that mean something to the parser: version, stop and credit words, and the
    /// separators between artists.
    fn reserved(w: &str) -> bool {
        MIX_WORDS.contains(&w)
            || GENERIC.contains(&w)
            || FEAT_WORDS.contains(&w)
            || ["with", "vs", "x", "presents", "pres", "meets", "rmx", "remastered", "acappella", "accapella"]
                .contains(&w)
    }

    /// 1 to `max` plain lower-case words the parser gives no meaning to.
    #[hegel::test_helper]
    fn plain_words(tc: &TestCase, max: usize) -> Vec<String> {
        let word = gs::from_regex("[a-z]{2,8}").fullmatch(true).filter(|w: &String| !reserved(w));
        tc.draw(gs::vecs(word).min_size(1).max_size(max))
    }

    /// Folding ignores case, accents (composed or not), apostrophes, dots, and how words are
    /// spaced or punctuated apart; "&" is "and"; and folding twice changes nothing.
    #[hegel::test(test_cases = 300)]
    fn folding_ignores_spelling(tc: TestCase) {
        let words = plain_words(&tc, 5);
        let plain = words.join(" ");
        let mut messy = String::new();
        for (k, w) in words.iter().enumerate() {
            if k > 0 {
                messy.push_str(
                    tc.draw(gs::sampled_from(vec![
                        " ", "  ", "\t", "\u{a0}", "\u{3000}", " - ", ", ", "/", " (", ") ",
                    ])),
                );
            }
            for c in w.chars() {
                messy.push(if tc.draw(gs::booleans()) { c.to_ascii_uppercase() } else { c });
                if "aeiounc".contains(c) && tc.draw(gs::booleans()) {
                    messy.push(tc.draw(gs::sampled_from(vec!['\u{301}', '\u{300}', '\u{308}', '\u{303}', '\u{327}'])));
                }
                if tc.draw(gs::weighted_booleans(0.1)) {
                    messy.push(tc.draw(gs::sampled_from(vec!['\'', '\u{2019}', '`', '.'])));
                }
            }
        }
        let messy = if tc.draw(gs::booleans()) { messy.nfc().collect() } else { messy };
        tc.note(&messy);
        assert_eq!(fold(&messy), plain);
        assert_eq!(fold(&format!("{} & {}", words[0], plain)), format!("{} and {plain}", words[0]));
        let any = tc.draw(gs::text().max_size(20));
        assert_eq!(fold(&fold(&any)), fold(&any));
    }

    /// A version written in brackets, after a dash or as the mix name is the same version: the
    /// title is the same as without it, and a remix's distinct words are its remixer and
    /// "remix". Generic versions ("Original Mix", a year's remaster) make no different recording.
    #[hegel::test(test_cases = 300)]
    fn versions_split_off_the_title(tc: TestCase) {
        let title = plain_words(&tc, 3).join(" ");
        let remixer = plain_words(&tc, 1).remove(0);
        let bare = Key::new(&title, "Someone", "", None);
        assert_eq!(bare.title, title);
        assert!(bare.mix.is_empty() && bare.variant.is_empty());
        let remix = format!("{remixer} Remix");
        let (t, mix) = match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 => (format!("{title} ({remix})"), ""),
            1 => (format!("{title} [{remix}]"), ""),
            2 => (format!("{title} - {remix}"), ""),
            3 => (format!("{title} \u{2013} {remix}"), ""),
            _ => (title.clone(), remix.as_str()),
        };
        let k = Key::new(&t, "Someone", mix, None);
        let mut want = vec![remixer, "remix".to_string()];
        want.sort();
        assert_eq!((k.title.as_str(), &k.mix), (title.as_str(), &want), "{t:?} {mix:?}");
        let generic = tc.draw(gs::sampled_from(vec![
            " (Original Mix)",
            " [Extended Mix]",
            " - Radio Edit",
            " (2012 Remaster)",
            " - Remastered 2009",
        ]));
        let k = Key::new(&format!("{title}{generic}"), "Someone", "", None);
        assert_eq!(k.title, title);
        assert!(k.mix.is_empty(), "{generic}: {:?}", k.mix);
        // A trailing aside that names no version ("[Anjunadeep]") only tells versions apart.
        let aside = plain_words(&tc, 2).join(" ");
        let k = Key::new(&format!("{title} [{aside}]"), "Someone", "", None);
        let mut want: Vec<String> = aside.split(' ').map(String::from).collect();
        want.sort();
        want.dedup();
        assert_eq!((k.title.as_str(), k.mix.len(), &k.variant), (title.as_str(), 0, &want));
        // Brackets with more of the name after them are part of it ("(I Can't Get No) Satisfaction").
        assert_eq!(Key::new(&format!("({title}) {aside}"), "Someone", "", None).title, format!("{title} {aside}"));
    }

    /// A guest is credited the same whether the title says it (in brackets, after a dash or
    /// plainly) or the artist field does: the title is the song's, the artists both.
    #[hegel::test(test_cases = 300)]
    fn featured_artists_join_the_artists(tc: TestCase) {
        let title = plain_words(&tc, 3).join(" ");
        let (artist, guest) = (plain_words(&tc, 2).join(" "), plain_words(&tc, 2).join(" "));
        tc.assume(artist != guest);
        let (t, a) = match tc.draw(gs::integers::<u8>().max_value(8)) {
            0 => (format!("{title} (feat. {guest})"), artist.clone()),
            1 => (format!("{title} [ft. {guest}]"), artist.clone()),
            2 => (format!("{title} (with {guest})"), artist.clone()),
            3 => (format!("{title} - feat. {guest}"), artist.clone()),
            4 => (format!("{title} featuring {guest}"), artist.clone()),
            5 => (title.clone(), format!("{artist} feat. {guest}")),
            6 => (title.clone(), format!("{artist}, {guest}")),
            7 => (title.clone(), format!("{artist} & {guest}")),
            _ => (title.clone(), format!("{artist} x {guest}")),
        };
        let k = Key::new(&t, &a, "", None);
        assert_eq!((k.title, k.artists), (title, vec![artist, guest]), "{t:?} by {a:?}");
    }

    /// Artist credits split into their names at every separator credit lists use, each folded
    /// and without a leading "the", once each.
    #[hegel::test(test_cases = 300)]
    fn artist_credits_split_into_names(tc: TestCase) {
        let names: Vec<String> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)))
            .map(|_| plain_words(&tc, 2).join(" "))
            .collect();
        let mut credit = String::new();
        for (k, n) in names.iter().enumerate() {
            if k > 0 {
                credit.push_str(tc.draw(gs::sampled_from(vec![
                    ", ",
                    "; ",
                    " & ",
                    " and ",
                    " feat. ",
                    " ft. ",
                    " vs. ",
                    " x ",
                    "/",
                    " + ",
                    " with ",
                    " presents ",
                    " meets ",
                ])));
            }
            let the = if tc.draw(gs::booleans()) { "The " } else { "" };
            credit.push_str(&format!("{the}{}", n.to_uppercase()));
        }
        let mut want: Vec<String> = Vec::new();
        for n in names {
            if !want.contains(&n) {
                want.push(n);
            }
        }
        assert_eq!(split_artists(&credit), want, "{credit:?}");
    }

    /// Words of five or more letters are near when one letter is changed (two in words of nine
    /// or more), not when two are in a shorter word, never when a digit differs, and nearness
    /// doesn't depend on the order of the two.
    #[hegel::test(test_cases = 300)]
    fn near_words_allow_one_typo(tc: TestCase) {
        let w: Vec<char> = tc.draw(gs::from_regex("[a-y]{5,12}").fullmatch(true)).chars().collect();
        let changed = |at: &[usize], to: char| -> String {
            w.iter().enumerate().map(|(i, &c)| if at.contains(&i) { to } else { c }).collect()
        };
        let word: String = w.iter().collect();
        let i = tc.draw(gs::integers::<usize>().max_value(w.len() - 1));
        let j = tc.draw(gs::integers::<usize>().max_value(w.len() - 1));
        assert!(near_words(&word, &word));
        let one = changed(&[i], 'z');
        assert!(near_words(&word, &one) && near_words(&one, &word));
        if i != j {
            let two = changed(&[i, j], 'z');
            assert_eq!(near_words(&word, &two), w.len() >= 9);
            assert_eq!(near_words(&two, &word), w.len() >= 9);
        }
        assert!(!near_words(&word, &changed(&[i], '7')));
    }

    /// Scores stay between 0 and 1, and anything with a title and an artist scores 1 against
    /// itself.
    #[hegel::test(test_cases = 300)]
    fn scores_peak_on_identity(tc: TestCase) {
        let key = |tc: &TestCase| {
            let field = || gs::text().max_size(16);
            let duration = tc.draw(gs::optional(gs::integers::<u32>().max_value(900)));
            Key::new(&tc.draw(field()), &tc.draw(field()), &tc.draw(field()), duration)
        };
        let (a, b) = (key(&tc), key(&tc));
        for s in [score(&a, &b), score(&b, &a), score(&a, &a)] {
            assert!((0.0..=1.0).contains(&s), "{s}");
        }
        if !a.title_tokens.is_empty() && !a.artist_tokens.is_empty() {
            assert!(score(&a, &a) > 0.999, "{a:?}");
        }
    }
}
