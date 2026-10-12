//! M3U and M3U8 playlists, and plain lists of files.

use percent_encoding::percent_decode_str;

use crate::Entry;
use crate::detection::{file_path, strip_prefix_ci};
use crate::plain::{split_artist_title, strip_decoration};

pub(crate) fn parse_m3u(text: &str) -> Vec<Entry> {
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
pub(crate) fn location_entry(loc: &str) -> Entry {
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

pub(crate) fn strip_extension(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty() && (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            stem
        }
        _ => name,
    }
}

#[cfg(test)]
mod tests;
