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
mod tests {
    use super::*;
    use crate::parse;

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
}
