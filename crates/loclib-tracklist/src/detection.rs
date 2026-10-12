//! Which format a tracklist is, and the files its lines name.

use percent_encoding::percent_decode_str;

use crate::Format;
use crate::table::{is_rekordbox_header, sniffed_header};

/// `hint`'s extension, lower-cased ("list.M3U8" and "m3u8" both give "m3u8").
fn extension(hint: &str) -> String {
    let name = hint.rsplit(['/', '\\']).next().unwrap_or(hint);
    name.rsplit_once('.').map_or(name, |(_, e)| e).to_ascii_lowercase()
}

/// Unmistakable content first (XML, `#EXTM3U`, rekordbox's header), then the file extension,
/// then softer evidence (a recognisable table header, a list of paths).
pub(crate) fn detect_text(text: &str, hint: Option<&str>) -> Format {
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

pub(crate) fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    s.get(..prefix.len()).filter(|p| p.eq_ignore_ascii_case(prefix)).map(|_| &s[prefix.len()..])
}

const AUDIO_EXTENSIONS: &[&str] =
    &["mp3", "wav", "wave", "aif", "aiff", "flac", "m4a", "mp4", "aac", "alac", "ogg", "opus", "wma"];

/// Whether a line of a plain list is a file rather than "Artist - Title".
pub(crate) fn looks_like_path(s: &str) -> bool {
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
pub(crate) fn file_path(loc: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect;
    use crate::testing::utf16le;

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

    use hegel::generators as gs;
    use hegel::{Generator, TestCase};
    use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

    /// A file's path reads back from every way a list names it: as written (POSIX, Windows,
    /// UNC), as a `file:` URL (percent-encoded, with `localhost`, a drive letter or a server),
    /// or as a classic Mac OS `Disk:Folder:file` path; and every one of those looks like a path.
    /// URLs of other schemes aren't files.
    #[hegel::test(test_cases = 300)]
    fn file_paths_read_back_from_every_spelling(tc: TestCase) {
        let segment = || gs::from_regex("[A-Za-z0-9é &'()_.-]{1,8}").fullmatch(true).filter(|s: &String| s.trim() == s);
        let dirs: Vec<String> = tc.draw(gs::vecs(segment()).max_size(3));
        let ext = tc.draw(gs::sampled_from(AUDIO_EXTENSIONS.to_vec()));
        let file = format!("{}.{ext}", tc.draw(segment()));
        let rel = dirs.iter().chain([&file]).cloned().collect::<Vec<_>>().join("/");
        let posix = format!("/{rel}");
        let url = |p: &str| utf8_percent_encode(p, NON_ALPHANUMERIC).to_string();
        let drive = tc.draw(gs::sampled_from(vec!['C', 'd']));
        let windows = format!("{drive}:\\{}", rel.replace('/', "\\"));
        let spellings = [
            (posix.clone(), posix.clone()),
            (windows.clone(), windows),
            (format!("\\\\nas\\{}", rel.replace('/', "\\")), format!("\\\\nas\\{}", rel.replace('/', "\\"))),
            (format!("file://{}", url(&posix)), posix.clone()),
            (format!("file:{posix}"), posix.clone()),
            (format!("FILE://localhost{}", url(&posix)), posix.clone()),
            (format!("file:///{drive}:{}", url(&posix)), format!("{drive}:{posix}")),
            (format!("file://LocalHost/{drive}:{posix}"), format!("{drive}:{posix}")),
            (format!("file://nas{}", url(&posix)), format!("//nas{posix}")),
        ];
        for (written, path) in spellings {
            assert_eq!(file_path(&written).as_deref(), Some(path.as_str()), "{written}");
            assert!(looks_like_path(&written), "{written}");
        }
        // The disk's name has a space, or it would be a URL scheme.
        let mac = format!("Macintosh HD:{}", rel.replace('/', ":"));
        if !rel.contains(':') && rel.contains('/') {
            assert_eq!(file_path(&mac), Some(format!("/Macintosh HD/{rel}")));
        }
        let scheme = tc.draw(gs::sampled_from(vec!["https", "spotify", "x-apple.music", "smb+tls"]));
        assert_eq!(file_path(&format!("{scheme}:{}", url(&posix))), None);
    }
}
