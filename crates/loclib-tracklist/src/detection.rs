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
mod tests;
