//! Plain text: one track per line.

use crate::Entry;
use crate::detection::looks_like_path;
use crate::m3u::location_entry;

pub(crate) fn parse_plain(text: &str) -> Vec<Entry> {
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

pub(crate) fn digits_len(s: &str) -> usize {
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
pub(crate) fn strip_decoration(s: &str, numbered: bool) -> (&str, Option<u32>) {
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
pub(crate) fn clock_prefix(s: &str) -> Option<(usize, u32)> {
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
pub(crate) fn split_artist_title(s: &str, allow_by: bool) -> (String, String) {
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

#[cfg(test)]
mod tests {
    use crate::parse;
    use crate::testing::{at, pairs};

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
}
