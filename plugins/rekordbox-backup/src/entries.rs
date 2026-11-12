//! What goes in a backup's zip and in what order, as rekordbox 7.2.14 lays it out: `master.db`,
//! the library files beside it, the settings, each track's analysis and then each track's
//! artwork, the key mappings, and `contents.xml` last. Only files that are there go in, and never
//! a folder entry.

use std::path::{Path, PathBuf};

use loclib_plugin::{CollectionLocations, CollectionTrack};
use rustc_hash::FxHashSet;
use unicode_normalization::UnicodeNormalization;

use crate::credentials;

/// The library files beside `master.db`, in order. rekordbox 7.2.14 on macOS had the middle four;
/// rekordbox 7.2.11 had `smartlist.xml` too, and rbx's list for 7.2.18 has `playlists3.sync`.
pub(crate) const LIBRARY: [&str; 6] = [
    "smartlist.xml",
    "masterPlaylists6.xml",
    "automixPlaylist6.xml",
    "networkAnalyze6.db",
    "networkRecommend.db",
    "playlists3.sync",
];

/// The settings folder's files, in order, each at the zip's root but `LightingDB/`'s.
pub(crate) const SETTINGS: [&str; 11] = [
    "rekordbox3.settings",
    "browseSetting.xml",
    "PlaySettings.xml",
    "activations.xml",
    "SamplerSettings1.xml",
    "LightingDB/macro.db3",
    "LightingDB/user.db3",
    "MYSETTING.DAT",
    "MYSETTING2.DAT",
    "DEVSETTING.DAT",
    "DJMMYSETTING.DAT",
];

/// The analysis files of one analysis, by extension, in rekordbox's order.
const ANALYSIS: [&str; 4] = ["DAT", "EXT", "2EX", "3EX"];

/// An artwork's three sizes, as suffixes to its file's stem.
const ARTWORK: [&str; 3] = ["", "_s", "_m"];

/// One of a zip's entries.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entry {
    /// Its name in the zip.
    pub name: String,
    pub body: Body,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Body {
    /// A file, as it is: its time and permissions go in too.
    File(PathBuf),
    /// A file's bytes changed (a settings file without its sign-in tokens), with its time and
    /// permissions.
    Changed(PathBuf, Vec<u8>),
    /// Bytes made for the backup (`contents.xml`).
    Made(Vec<u8>),
}

impl Body {
    /// Its size, as far as it's known without reading it.
    pub(crate) fn len(&self) -> u64 {
        match self {
            Body::File(path) => std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
            Body::Changed(_, bytes) | Body::Made(bytes) => bytes.len() as u64,
        }
    }
}

/// Takes the sign-in tokens out of a settings file (see [`credentials`]).
type Strip = fn(&[u8]) -> Result<Vec<u8>, String>;

/// Where rekordbox's files are, each folder there.
pub(crate) struct Folders<'a> {
    pub data: &'a Path,
    pub share: &'a Path,
    pub settings: &'a Path,
}

impl<'a> Folders<'a> {
    pub(crate) fn of(at: &'a CollectionLocations) -> Option<Folders<'a>> {
        Some(Folders { data: at.data.as_deref()?, share: at.share.as_deref()?, settings: at.settings.as_deref()? })
    }
}

/// The entries for a backup of the library in `at`, but `contents.xml`: `master_db` (a copy) and
/// the files of `tracks` (those rekordbox hasn't deleted, in its order), stripped of sign-in
/// tokens when `strip`. Also what it left out and why (a path leading out of `share`).
pub(crate) fn gather(
    at: &Folders<'_>,
    master_db: &Path,
    tracks: &[CollectionTrack],
    strip: bool,
) -> Result<(Vec<Entry>, Vec<String>), String> {
    let mut out = vec![Entry { name: "master.db".into(), body: Body::File(master_db.to_path_buf()) }];
    let present = |dir: &Path, name: &str| Some(dir.join(name)).filter(|p| p.is_file());
    for name in LIBRARY {
        if let Some(path) = present(at.data, name) {
            out.push(Entry { name: name.into(), body: Body::File(path) });
        }
    }
    for name in SETTINGS {
        let Some(path) = present(at.settings, name) else { continue };
        let strip_with: Option<Strip> = match name {
            "rekordbox3.settings" if strip => Some(credentials::strip_tokens),
            "activations.xml" if strip => Some(credentials::strip_accounts),
            _ => None,
        };
        let body = match strip_with {
            Some(strip) => {
                let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                Body::Changed(path, strip(&bytes).map_err(|e| format!("{name}: {e}"))?)
            }
            None => Body::File(path),
        };
        out.push(Entry { name: name.into(), body });
    }
    let (share, warnings) = share_files(at.share, tracks);
    out.extend(share.into_iter().map(|(name, path)| Entry { name, body: Body::File(path) }));
    out.extend(key_mappings(at.settings).into_iter().map(|(name, path)| Entry { name, body: Body::File(path) }));
    Ok((out, warnings))
}

/// The analysis files (each track's referenced set: `.DAT`, `.EXT`, `.2EX`, `.3EX`) and then
/// the artwork (each track's three sizes) of the tracks rekordbox hasn't deleted, those there,
/// each once: as zip names (`share/PIONEER/...`) and files. A stale analysis beside the one a
/// track names (`ANLZ0000.*` beside `ANLZ0001.DAT`) stays out, as rekordbox leaves it out.
pub(crate) fn share_files(share: &Path, tracks: &[CollectionTrack]) -> (Vec<(String, PathBuf)>, Vec<String>) {
    let live = tracks.iter().filter(|t| !t.deleted);
    let mut warnings = Vec::new();
    let mut named = |stored: &Option<String>, id: &str| {
        let stored = stored.as_deref()?;
        let inside = within_share(stored);
        if inside.is_none() {
            warnings.push(format!("track {id}: {stored} isn't in rekordbox's share folder, so it's left out"));
        }
        inside
    };
    let mut wanted: Vec<String> = Vec::new();
    for t in live.clone() {
        let Some(path) = named(&t.analysis_path, &t.id) else { continue };
        let stem = path.rsplit_once('.').map_or(path.as_str(), |(stem, _)| stem);
        wanted.extend(ANALYSIS.iter().map(|ext| format!("{stem}.{ext}")));
    }
    for t in live {
        let Some(path) = named(&t.image_path, &t.id) else { continue };
        let (stem, ext) = path.rsplit_once('.').unwrap_or((path.as_str(), ""));
        let dot = if ext.is_empty() { "" } else { "." };
        wanted.extend(ARTWORK.iter().map(|size| format!("{stem}{size}{dot}{ext}")));
    }
    let mut seen = FxHashSet::default();
    let files = wanted
        .into_iter()
        .filter(|rel| seen.insert(rel.clone()))
        .filter_map(|rel| {
            let file = share.join(rel.trim_start_matches('/'));
            file.is_file().then(|| (format!("share{rel}"), file))
        })
        .collect();
    (files, warnings)
}

/// A path as rekordbox stores it in `share` (`/PIONEER/USBANLZ/...`), when it stays there: no
/// `..` or `.`, no drive, nothing but names.
fn within_share(stored: &str) -> Option<String> {
    let rel = stored.replace('\\', "/");
    let mut names = rel.strip_prefix('/')?.split('/');
    let plain = names.all(|n| !n.is_empty() && n != "." && n != ".." && !n.contains(':'));
    plain.then_some(rel)
}

/// The key mappings (`KeyMappings/rekordbox_*.mappings`), in the order the folder lists them, as
/// rekordbox has them.
fn key_mappings(settings: &Path) -> Vec<(String, PathBuf)> {
    let Ok(listed) = std::fs::read_dir(settings.join("KeyMappings")) else { return Vec::new() };
    listed
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let ours = name.starts_with("rekordbox_") && name.ends_with(".mappings");
            ours.then(|| (format!("KeyMappings/{name}"), e.path()))
        })
        .collect()
}

/// rekordbox's name for a track's copy in `rekordbox_bak/`: `/<Artist>/<Album>/<HEXID>_<name>`,
/// the id as 8 hex digits, the artist and album as [`folder`] names them, and the file's stem cut
/// to 35 characters without the whitespace the cut leaves at its end. None for an id that isn't a
/// 32-bit number, as rekordbox's are. Characters no file name can have on macOS or Windows become
/// `_`. `contents.xml` lists the copies by this name too.
pub(crate) fn music_name(id: &str, artist: Option<&str>, album: Option<&str>, path: &str) -> Option<String> {
    let hex = format!("{:08X}", id.trim().parse::<u32>().ok()?);
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let (stem, ext) = match file.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
        _ => (file, None),
    };
    let stem: String = stem.chars().take(35).collect();
    let stem = stem.trim_end();
    let file = match ext {
        Some(ext) => format!("{hex}_{stem}.{ext}"),
        None => format!("{hex}_{stem}"),
    };
    let artist = folder(artist, "UnknownArtist");
    let album = folder(album, "UnknownAlbum");
    Some(format!("/{artist}/{album}/{}", safe(&file)))
}

/// The most characters (Unicode scalar values, as JUCE's strings count) of a folder named from a
/// tag. 48 of the widest UTF-8 characters still fit the 255 bytes a file system takes.
const FOLDER_CHARS: usize = 48;

/// Names Windows keeps for devices, whatever follows a dot.
const DEVICES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2",
    "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A folder name from a tag, as rekordbox 7.2.14 names it: composed (NFC), without its
/// whitespace, `~` as `_`, cut to [`FOLDER_CHARS`], and a last `.` as `_`, so a name of dots
/// alone (`..`) stays in its folder too. `unknown` when nothing's left. What no file name can hold
/// becomes `_`, and a name Windows keeps for a device (`CON`, `COM1.x`) gets a `_` before it.
fn folder(name: Option<&str>, unknown: &str) -> String {
    let squeezed: String = name.unwrap_or_default().nfc().filter(|c| !c.is_whitespace()).collect();
    if squeezed.is_empty() {
        return unknown.into();
    }
    let mut out = safe(&squeezed.replace('~', "_"));
    let device = out.split('.').next().is_some_and(|base| DEVICES.iter().any(|d| d.eq_ignore_ascii_case(base)));
    if device {
        out.insert(0, '_');
    }
    let mut out: String = out.chars().take(FOLDER_CHARS).collect();
    if out.ends_with('.') {
        out.pop();
        out.push('_');
    }
    out
}

/// `name` with what no file name can hold on macOS or Windows (C0 controls and
/// `/ \ : * ? " < > |`) as `_`. The C1 controls (U+0080 to U+009F) stay, as both systems take
/// them and rekordbox keeps them: a file named `JoÌ\u{88}rgen` (a decomposed `ö`'s UTF-8 read
/// as Latin-1) kept its U+0088. DEL stays too, since both systems take it.
fn safe(name: &str) -> String {
    name.chars().map(|c| if c < ' ' || r#"/\:*?"<>|"#.contains(c) { '_' } else { c }).collect()
}

#[cfg(test)]
mod tests;
