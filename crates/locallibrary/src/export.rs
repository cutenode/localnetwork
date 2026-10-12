//! Track lists as files other software reads: M3U8 playlists (`#EXTM3U` / `#EXTINF`), CSV for
//! spreadsheets, and plain text (`01. Artist - Title`) for a set list or a message.
//!
//! Everything here is a pure function returning the file's text; [`crate::api::export_playlist`]
//! picks the tracks (a playlist, a folder, a history list or the whole library) and writes it.
//!
//! M3U8 entries point at the audio on the drive in one of three ways ([`PathStyle`]): relative to
//! the playlist file (the playlist keeps working when it sits on the drive and the drive mounts
//! elsewhere), as the drive's own absolute path (`/Contents/...`), or as an absolute path on this
//! computer through the drive's mount point.

use rustc_hash::FxHashSet;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use loclib_model::paths::PathStyle;

use crate::model::{Library, Playlist, Track};
use crate::paths;
use crate::{Error, Result, fsutil};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "ExportFormat")]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Format {
    #[default]
    M3u8,
    Csv,
    Txt,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::M3u8 => "m3u8",
            Format::Csv => "csv",
            Format::Txt => "txt",
        }
    }

    /// The format a file name asks for (`.m3u8`/`.m3u`, `.csv`, `.txt`).
    pub fn from_path(path: &Path) -> Option<Format> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "m3u8" | "m3u" => Some(Format::M3u8),
            "csv" => Some(Format::Csv),
            "txt" => Some(Format::Txt),
            _ => None,
        }
    }
}

/// Turns a track's drive path into the path written to a file.
#[derive(Debug, Clone)]
pub struct Locator {
    pub style: PathStyle,
    /// Where the drive is mounted.
    pub drive_root: PathBuf,
    /// The output file's folder (for [`PathStyle::Relative`]).
    pub base: PathBuf,
}

impl Locator {
    /// A locator for a file written to `output`.
    pub fn new(style: PathStyle, drive_root: &Path, output: &Path) -> Locator {
        let base = output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        Locator { style, drive_root: fsutil::resolved(drive_root), base: fsutil::resolved(base) }
    }

    pub fn locate(&self, file_path: &str) -> String {
        let key = paths::key(file_path);
        let absolute = || paths::to_fs(&self.drive_root, &key).display().to_string();
        match self.style {
            PathStyle::Drive => paths::device_path(&key),
            PathStyle::Absolute => absolute(),
            PathStyle::Relative => relative(&self.base, &paths::to_fs(&self.drive_root, &key)).unwrap_or_else(absolute),
        }
    }
}

/// `to` relative to the folder `from`, `/`-separated. `None` when they share no root.
fn relative(from: &Path, to: &Path) -> Option<String> {
    let a: Vec<Component> = from.components().filter(|c| *c != Component::CurDir).collect();
    let b: Vec<Component> = to.components().filter(|c| *c != Component::CurDir).collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if common == 0 || !matches!(a.first(), Some(Component::RootDir | Component::Prefix(_))) {
        return None;
    }
    let ups = (common..a.len()).map(|_| "..".to_string());
    let downs = b[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned());
    Some(ups.chain(downs).collect::<Vec<_>>().join("/"))
}

/// The file's text in `format`. `title` names an M3U8 playlist (`#PLAYLIST:`).
pub fn render(format: Format, title: &str, tracks: &[&Track], locator: &Locator) -> String {
    match format {
        Format::M3u8 => m3u8(title, tracks, locator),
        Format::Csv => csv(tracks, locator),
        Format::Txt => text(tracks),
    }
}

/// One line, without characters that would break a line-based format (or, in text a server
/// sent, play tricks on a terminal).
pub(crate) fn one_line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// Extended M3U, UTF-8: `#EXTINF:<seconds>,<Artist - Title>` before each path.
pub fn m3u8(title: &str, tracks: &[&Track], locator: &Locator) -> String {
    let mut out = String::from("#EXTM3U\n");
    if !title.is_empty() {
        let _ = writeln!(out, "#PLAYLIST:{}", one_line(title));
    }
    for t in tracks {
        let _ = writeln!(out, "#EXTINF:{},{}", t.duration_secs, one_line(&t.display_name()));
        let _ = writeln!(out, "{}", one_line(&locator.locate(&t.file_path)));
    }
    out
}

/// CSV (RFC 4180) with a header row.
pub fn csv(tracks: &[&Track], locator: &Locator) -> String {
    const HEADER: &[&str] = &[
        "#",
        "Title",
        "Artist",
        "Album",
        "Genre",
        "Label",
        "BPM",
        "Key",
        "Rating",
        "Duration",
        "Year",
        "Play Count",
        "Date Added",
        "Comment",
        "Path",
    ];
    let mut out = String::new();
    csv_line(&mut out, HEADER);
    for (i, t) in tracks.iter().enumerate() {
        let row = [
            (i + 1).to_string(),
            t.title.clone(),
            t.artist.clone(),
            t.album.clone(),
            t.genre.clone(),
            t.label.clone(),
            if t.bpm > 0.0 { format!("{:.2}", t.bpm) } else { String::new() },
            t.key.clone(),
            t.rating.to_string(),
            duration(t.duration_secs),
            if t.year > 0 { t.year.to_string() } else { String::new() },
            t.play_count.to_string(),
            t.date_added.clone(),
            t.comment.clone(),
            locator.locate(&t.file_path),
        ];
        csv_line(&mut out, &row);
    }
    out
}

/// Add a CSV row (RFC 4180: quoted when it must be, CRLF after).
pub(crate) fn csv_line(out: &mut String, fields: &[impl AsRef<str>]) {
    for (i, f) in fields.iter().map(AsRef::as_ref).enumerate() {
        if i > 0 {
            out.push(',');
        }
        match f.contains([',', '"', '\n', '\r']) {
            true => {
                let _ = write!(out, "\"{}\"", f.replace('"', "\"\""));
            }
            false => out.push_str(f),
        }
    }
    out.push_str("\r\n");
}

fn duration(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// `01. Artist - Title`, one per line.
pub fn text(tracks: &[&Track]) -> String {
    numbered(tracks.iter().map(|t| t.display_name()))
}

/// `01. name`, one per line, numbered with at least two digits.
pub(crate) fn numbered(names: impl ExactSizeIterator<Item = String>) -> String {
    let width = names.len().to_string().len().max(2);
    let mut out = String::new();
    for (i, name) in names.enumerate() {
        let _ = writeln!(out, "{:0width$}. {}", i + 1, one_line(&name));
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Picking tracks
// ---------------------------------------------------------------------------------------------

/// One output file's worth of tracks.
#[derive(Debug, Clone)]
pub struct Selection<'a> {
    /// Playlist path (or the name given for the whole library).
    pub name: String,
    /// For one file per playlist: the file's path below the output folder, without extension.
    pub file: PathBuf,
    pub tracks: Vec<&'a Track>,
}

/// A playlist's tracks in order (entries whose track is gone are skipped).
pub fn playlist_tracks<'a>(lib: &'a Library, p: &Playlist) -> Vec<&'a Track> {
    lib.tracks_by_id(&p.track_ids)
}

/// Playlists below a folder, depth first in display order, with their path below it.
fn playlists_below<'a>(lib: &'a Library, folder: &Playlist) -> Vec<(&'a Playlist, Vec<String>)> {
    fn walk<'a>(
        lib: &'a Library,
        id: u32,
        prefix: &[String],
        depth: usize,
        out: &mut Vec<(&'a Playlist, Vec<String>)>,
    ) {
        // `select` is public: a library built by hand can hold a loop no reader untangled.
        if depth > crate::model::MAX_DEPTH {
            return;
        }
        for p in lib.children(Some(id)) {
            let mut path = prefix.to_vec();
            path.push(p.name.clone());
            if p.is_folder {
                walk(lib, p.id, &path, depth + 1, out);
            } else {
                out.push((p, path));
            }
        }
    }
    let mut out = Vec::new();
    walk(lib, folder.id, &[], 0, &mut out);
    out
}

/// What to write for `playlist` (a playlist, or a folder: one file per playlist with `split`,
/// otherwise every track below it once), or the whole library (named `library_name`) when it's
/// `None`.
pub fn select<'a>(
    lib: &'a Library,
    playlist: Option<&str>,
    split: bool,
    library_name: &str,
) -> Result<Vec<Selection<'a>>> {
    let Some(query) = playlist else {
        return Ok(vec![Selection {
            name: library_name.to_string(),
            file: PathBuf::from(paths::sanitize_component(library_name)),
            tracks: lib.tracks.iter().collect(),
        }]);
    };
    let p = lib.find_playlist(query).ok_or_else(|| Error::NotFound(format!("playlist {query:?}")))?;
    let path = lib.playlist_path(p.id);
    if !p.is_folder {
        return Ok(vec![Selection {
            file: PathBuf::from(paths::sanitize_component(&p.name)),
            name: path,
            tracks: playlist_tracks(lib, p),
        }]);
    }
    let below = playlists_below(lib, p);
    if !split {
        let mut seen = FxHashSet::default();
        let tracks = below.iter().flat_map(|(q, _)| playlist_tracks(lib, q)).filter(|t| seen.insert(t.id)).collect();
        return Ok(vec![Selection { file: PathBuf::from(paths::sanitize_component(&p.name)), name: path, tracks }]);
    }
    let mut used = FxHashSet::default();
    Ok(below
        .into_iter()
        .map(|(q, parts)| {
            let mut file: PathBuf = parts.iter().map(|c| paths::sanitize_component(c)).collect();
            // Two playlists whose names sanitise alike get " (2)", " (3)"...
            let base = file.clone();
            let mut n = 2;
            while !used.insert(file.to_string_lossy().to_lowercase()) {
                file = base.with_file_name(format!(
                    "{} ({n})",
                    base.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()
                ));
                n += 1;
            }
            Selection { name: lib.playlist_path(q.id), file, tracks: playlist_tracks(lib, q) }
        })
        .collect())
}

#[cfg(test)]
mod tests;
