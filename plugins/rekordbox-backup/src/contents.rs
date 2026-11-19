//! `contents.xml`, the manifest rekordbox puts last in a backup: a JUCE `PropertiesFile` of
//! `<VALUE name val/>` entries. One per music root (a number whose meaning is unknown), one per
//! track whose music was copied, then the library's id and the computer's folders. rekordbox is
//! built on JUCE, so the bytes follow JUCE's XML writer: a declaration, a blank line, two-space
//! indents, CRLF after every line, and attribute values escaped as `escapeIllegalXmlChars` does.

use std::path::{Path, PathBuf};

use quick_xml::XmlVersion;
use quick_xml::events::Event;

/// The format version rekordbox 7.2.11 and 7.2.14 both write.
pub(crate) const FILE_VERSION: &str = "4";

/// A `contents.xml`'s entries, in rekordbox's order (see [`Contents::values`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Contents {
    /// Each root in `directories` whose number an earlier backup of the library had, with it.
    pub roots: Vec<(String, String)>,
    /// Each track whose music is in `rekordbox_bak/`: its id as 8 hex digits, and where its copy
    /// is there (`/Artist/Album/HEXID_name.mp3`).
    pub tracks: Vec<(String, String)>,
    /// `djmdProperty.DBID`, as stored.
    pub db_id: String,
    /// Tracks rekordbox hasn't deleted, those whose file is missing included.
    pub content_num: usize,
    /// The app's version as major × 10000 + minor × 100 + patch (`70214`).
    pub rb_version: u32,
    /// `DIRECTORY_ARRAY`: the folders the music is in (see [`directories()`]).
    pub directories: Vec<String>,
    pub folders: Folders,
}

impl Contents {
    /// The entries, in the order rekordbox 7.2.14 writes them.
    pub(crate) fn values(&self) -> Vec<(String, String)> {
        let mut out = self.roots.clone();
        out.extend(self.tracks.iter().cloned());
        let f = &self.folders;
        let fixed = [
            ("DB_ID", self.db_id.clone()),
            // Always 0 in the two backups seen; what it counts is unknown.
            ("CONTENT_ID_MAX", "0".into()),
            ("CONTENT_NUM", self.content_num.to_string()),
            ("BACKUP_RB_VER", self.rb_version.to_string()),
            ("BACKUP_FILE_VER", FILE_VERSION.into()),
            ("DIRECTORY_ARRAY", self.directories.join(",")),
            ("commonApplicationDataDirectory", f.common_app_data.clone()),
        ];
        out.extend(fixed.into_iter().map(|(k, v)| (k.to_string(), v)));
        out.extend(f.user().into_iter().map(|(k, v)| (k.to_string(), v.to_string())));
        out
    }

    pub(crate) fn to_xml(&self) -> Vec<u8> {
        write(&self.values())
    }
}

/// The computer's folders a backup records, as JUCE names them, so a restore elsewhere can tell
/// where the music was.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Folders {
    pub common_app_data: String,
    pub app_data: String,
    pub music: String,
    pub movies: String,
    pub documents: String,
    pub desktop: String,
    pub home: String,
}

impl Folders {
    /// This account's folders: on macOS `/Library` and `~/Library`, on Windows `ProgramData`
    /// and the roaming `AppData`, as JUCE's special locations are.
    pub(crate) fn here() -> Option<Folders> {
        let base = directories::BaseDirs::new()?;
        let user = directories::UserDirs::new()?;
        let home = base.home_dir().to_path_buf();
        let or = |dir: Option<&Path>, name: &str| dir.map_or_else(|| home.join(name), Path::to_path_buf);
        let (common, app_data) = if cfg!(windows) {
            let program_data =
                std::env::var_os("ProgramData").map_or_else(|| PathBuf::from(r"C:\ProgramData"), PathBuf::from);
            (program_data, base.config_dir().to_path_buf())
        } else if cfg!(target_os = "macos") {
            (PathBuf::from("/Library"), home.join("Library"))
        } else {
            // rekordbox doesn't run here, so this never reaches a backup it restores.
            (PathBuf::from("/usr/share"), base.data_dir().to_path_buf())
        };
        let movies = if cfg!(windows) { "Videos" } else { "Movies" };
        let text = |p: PathBuf| p.to_string_lossy().into_owned();
        Some(Folders {
            common_app_data: text(common),
            app_data: text(app_data),
            music: text(or(user.audio_dir(), "Music")),
            movies: text(or(user.video_dir(), movies)),
            documents: text(or(user.document_dir(), "Documents")),
            desktop: text(or(user.desktop_dir(), "Desktop")),
            home: text(home.clone()),
        })
    }

    /// The `user*Directory` entries, in rekordbox's order.
    pub(crate) fn user(&self) -> [(&'static str, &str); 6] {
        [
            ("userApplicationDataDirectory", &self.app_data),
            ("userMusicDirectory", &self.music),
            ("userMoviesDirectory", &self.movies),
            ("userDocumentsDirectory", &self.documents),
            ("userDesktopDirectory", &self.desktop),
            ("userHomeDirectory", &self.home),
        ]
    }
}

/// `DIRECTORY_ARRAY`: for each audio file in `paths` (rekordbox's, so Windows ones use `/` or
/// `\` after a drive letter), the longest of the user folders holding it, else its volume
/// (`/Volumes/<name>`), drive (`F:`) or `/`, each once, in the order first met. A path that
/// isn't absolute (a streaming service's track) has none.
pub(crate) fn directories<'a>(paths: impl IntoIterator<Item = &'a str>, folders: &Folders) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for path in paths {
        let Some(root) = root(path, folders) else { continue };
        if !out.contains(&root) {
            out.push(root);
        }
    }
    out
}

/// Whether `path` starts with a Windows drive (`F:/` or `F:\`).
fn on_a_drive(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'/' | b'\\')
}

/// Whether a track's `path` is a file on a computer (absolute, on macOS or Windows), rather than
/// a streaming service's track.
pub(crate) fn is_local(path: &str) -> bool {
    path.starts_with('/') || on_a_drive(path)
}

fn root(path: &str, folders: &Folders) -> Option<String> {
    if !is_local(path) {
        return None;
    }
    let drive = on_a_drive(path);
    // Windows paths are compared as Windows does: either slash, any case.
    let norm = |s: &str| {
        let s = s.replace('\\', "/");
        if drive { s.to_lowercase() } else { s }
    };
    let file = norm(path);
    let holding = folders.user().into_iter().map(|(_, dir)| dir).filter(|dir| {
        let dir = norm(dir);
        let dir = dir.trim_end_matches('/');
        !dir.is_empty() && file.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'))
    });
    if let Some(dir) = holding.max_by_key(|dir| dir.len()) {
        return Some(dir.to_string());
    }
    if drive {
        return Some(path[..2].to_string());
    }
    match path.strip_prefix("/Volumes/").and_then(|rest| rest.split_once('/')) {
        Some((volume, _)) if !volume.is_empty() => Some(format!("/Volumes/{volume}")),
        _ => Some("/".into()),
    }
}

/// `7.2.14.0323` (or `7.2.14`) as `BACKUP_RB_VER` has it, `70214`.
pub(crate) fn rb_version(version: &str) -> Option<u32> {
    let mut parts = version.trim().split('.').map(|p| p.trim().parse::<u32>().ok());
    let (major, minor, patch) = (parts.next()??, parts.next()??, parts.next().unwrap_or(Some(0))?);
    (minor < 100 && patch < 100).then(|| major * 10000 + minor * 100 + patch)
}

/// `BACKUP_RB_VER` (`70214`) as a version (`7.2.14`).
pub(crate) fn version_text(rb_version: u32) -> String {
    format!("{}.{}.{}", rb_version / 10000, rb_version / 100 % 100, rb_version % 100)
}

/// JUCE's `XmlElement::TextFormat` default: a tag's next attribute goes on a line of its own once
/// the attributes on this one are longer than this.
const LINE_WRAP: usize = 60;

/// The file JUCE's `PropertiesFile` writes for `values`, as rekordbox writes `contents.xml`.
pub(crate) fn write(values: &[(String, String)]) -> Vec<u8> {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\r\n<PROPERTIES>");
    for (name, val) in values {
        out.push_str("\r\n  <VALUE");
        let start = out.len();
        out.push_str(" name=\"");
        escape(name, &mut out);
        out.push('"');
        if out.len() - start > LINE_WRAP {
            // Indented past `<VALUE`, as JUCE lines up wrapped attributes.
            out.push_str("\r\n        ");
        }
        out.push_str(" val=\"");
        escape(val, &mut out);
        out.push_str("\"/>");
    }
    out.push_str("\r\n</PROPERTIES>\r\n");
    out.into_bytes()
}

/// JUCE's `escapeIllegalXmlChars` for an attribute: printable ASCII as it is but for `"`, `&`,
/// `<` and `>` (named) and `^` and the backtick; everything else, line breaks and all non-ASCII
/// included, as a decimal reference to its code point. JUCE strings end at a NUL, so one ends
/// the value here too.
pub(crate) fn escape(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '\0' => return,
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '>' => out.push_str("&gt;"),
            '<' => out.push_str("&lt;"),
            ' '..='~' if c != '^' && c != '`' => out.push(c),
            _ => {
                out.push_str("&#");
                out.push_str(&u32::from(c).to_string());
                out.push(';');
            }
        }
    }
}

/// The `<VALUE name val/>` entries of a `contents.xml` (or any `PropertiesFile`), in order.
pub(crate) fn parse(bytes: &[u8]) -> Result<Vec<(String, String)>, String> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut out = Vec::new();
    loop {
        let event = reader.read_event().map_err(|e| format!("contents.xml isn't XML: {e}"))?;
        let e = match &event {
            Event::Empty(e) | Event::Start(e) if e.name().as_ref() == "VALUE" => e,
            Event::Eof => return Ok(out),
            _ => continue,
        };
        let attr = |key: &str| -> Result<String, String> {
            let found = e.attributes().flatten().find(|a| a.key.as_ref() == key);
            let value = found.map(|a| a.normalized_value(XmlVersion::Implicit1_0).map(|v| v.into_owned()));
            value.unwrap_or(Ok(String::new())).map_err(|e| format!("contents.xml has a value it can't read: {e}"))
        };
        out.push((attr("name")?, attr("val")?));
    }
}

#[cfg(test)]
mod tests;
