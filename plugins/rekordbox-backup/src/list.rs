//! The backups in the backup folder (`rekordbox_bak_*.zip`, rekordbox's and this plugin's
//! alike) and what each one's `contents.xml` says, read from the zip without unpacking it.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::Local;
use loclib_plugin::{Column, RpcError, RunCommand, Section, View};
use serde_json::{Value, json};

use crate::contents::{self, version_text};
use crate::music::FOLDER;
use crate::{RESTORE, Settings, size, view};

/// The most of a `contents.xml` read, so a zip claiming a huge one can't take the memory:
/// rekordbox's for 2,050 tracks is 213 kB.
pub(crate) const MOST: u64 = 64 << 20;

/// A backup's zip in the backup folder.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Found {
    pub path: PathBuf,
    pub bytes: u64,
    pub modified: Option<SystemTime>,
}

impl Found {
    pub(crate) fn name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// The day it was made: its name's (`rekordbox_bak_20261110.zip`), else its file's.
    pub(crate) fn date(&self) -> String {
        let name = self.name();
        let day = name.strip_prefix("rekordbox_bak_").and_then(|d| d.strip_suffix(".zip"));
        match day.filter(|d| d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit())) {
            Some(d) => format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..]),
            None => self
                .modified
                .map(|m| chrono::DateTime::<Local>::from(m).format("%Y-%m-%d").to_string())
                .unwrap_or_default(),
        }
    }
}

/// The backups in `dir`, newest first.
pub(crate) fn find(dir: &Path) -> Vec<Found> {
    let Ok(listed) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<Found> = listed
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.starts_with("rekordbox_bak_") && name.ends_with(".zip")
        })
        .filter_map(|e| {
            let meta = e.metadata().ok().filter(std::fs::Metadata::is_file)?;
            Some(Found { path: e.path(), bytes: meta.len(), modified: meta.modified().ok() })
        })
        .collect();
    found.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| b.path.cmp(&a.path)));
    found
}

/// The entries of the `contents.xml` in the zip at `path`.
pub(crate) fn read_contents(path: &Path) -> Result<Vec<(String, String)>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("not a zip: {e}"))?;
    let entry = zip.by_name("contents.xml").map_err(|_| "it has no contents.xml".to_string())?;
    let mut bytes = Vec::new();
    entry.take(MOST).read_to_end(&mut bytes).map_err(|e| format!("contents.xml: {e}"))?;
    contents::parse(&bytes)
}

/// The value `name` has in `values`.
pub(crate) fn value<'a>(values: &'a [(String, String)], name: &str) -> Option<&'a str> {
    values.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

/// Whether a `contents.xml` entry named `name` maps a track's music: one named by 8 hex digits.
pub(crate) fn is_track(name: &str) -> bool {
    name.len() == 8 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The earlier backup's numbers for the music roots `roots`, from the newest backup in `dir` of
/// the library `db_id` that has any. What they mean is unknown, so they're copied, never made up.
pub(crate) fn earlier_roots(dir: &Path, db_id: &str, roots: &[String]) -> Vec<(String, String)> {
    for found in find(dir) {
        let Ok(values) = read_contents(&found.path) else { continue };
        if value(&values, "DB_ID") != Some(db_id) {
            continue;
        }
        let numbers: Vec<(String, String)> =
            roots.iter().filter_map(|r| value(&values, r).map(|n| (r.clone(), n.to_string()))).collect();
        if !numbers.is_empty() {
            return numbers;
        }
    }
    Vec::new()
}

/// `backups`: a table of the backups in the backup folder.
pub(crate) fn command(req: &RunCommand) -> Result<Value, RpcError> {
    let dest = Settings::of(&req.context)?.destination()?;
    let found = find(&dest);
    let title = format!("Backups in {}", dest.display());
    if found.is_empty() {
        let text = format!("No rekordbox library backups (rekordbox_bak_*.zip) in {}.", dest.display());
        return Ok(view(View::Text { title: Some(title), text }));
    }
    let music_folder = dest.join(FOLDER).is_dir();
    let rows = found.iter().map(|f| row(f, music_folder)).collect();
    let columns = ["Backup", "Date", "Size", "Tracks", "rekordbox", "Music"].map(|c| Column::Name(c.into())).to_vec();
    let table = View::Table { title: None, columns, rows };
    let restore = View::Text { title: None, text: RESTORE.into() };
    let items = vec![Section { title, view: table }, Section { title: "Restoring".into(), view: restore }];
    Ok(view(View::Sections { title: None, items }))
}

/// A backup's row: its name, date and size, and its tracks, rekordbox version and music as its
/// `contents.xml` says (`yes` when the music folder is beside it, `missing` when it isn't).
fn row(f: &Found, music_folder: bool) -> Vec<Value> {
    let start = vec![json!(f.name()), json!(f.date()), json!(size(f.bytes))];
    let rest = match read_contents(&f.path) {
        Ok(values) => {
            let tracks = value(&values, "CONTENT_NUM").and_then(|n| n.parse::<u64>().ok());
            let version = value(&values, "BACKUP_RB_VER").and_then(|v| v.parse().ok()).map(version_text);
            let music = match (values.iter().filter(|(n, _)| is_track(n)).count(), music_folder) {
                (0, _) => "no",
                (_, true) => "yes",
                (_, false) => "missing",
            };
            [json!(tracks), json!(version), json!(music)]
        }
        Err(e) => [Value::Null, json!(format!("can't read it: {e}")), Value::Null],
    };
    start.into_iter().chain(rest).collect()
}

#[cfg(test)]
mod tests;
