//! Playlists, folders, history lists and whole libraries as M3U8, CSV or text files.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::resolve_drive;
use crate::export::{self, Format, Locator, PathStyle, Selection};
use crate::model::Track;
use crate::plays::{self, HistoryList};
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result, fsutil};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ExportPlaylist {
    pub drive: String,
    /// Playlist or folder (path like `Folder/Name`, or a unique name). The whole library when
    /// neither this nor `history` is given.
    #[serde(default)]
    pub playlist: Option<String>,
    /// A history list (name or path) instead.
    #[serde(default)]
    pub history: Option<String>,
    /// Also look for `history` among the lists archived in the store.
    #[serde(default)]
    pub archived: bool,
    /// Default: from `output`'s extension, else M3U8.
    #[serde(default)]
    pub format: Option<Format>,
    /// The file to write; with `split`, the folder to write into.
    pub output: PathBuf,
    #[serde(default)]
    pub paths: PathStyle,
    /// For a folder: one file per playlist below `output`, instead of one file with every track.
    #[serde(default)]
    pub split: bool,
    /// Replace files already there (refused otherwise).
    #[serde(default)]
    pub replace: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ExportedFile {
    pub path: PathBuf,
    /// The playlist (or history list, or library) it holds.
    pub name: String,
    pub tracks: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ExportReport {
    pub format: Format,
    pub files: Vec<ExportedFile>,
}

/// Write a playlist, folder, history list or the whole library of a drive to a file.
pub fn export_playlist(store: Option<&Store>, req: &ExportPlaylist, ctx: &Ctx<'_>) -> Result<ExportReport> {
    let d = resolve_drive(&req.drive)?;
    let format = req.format.or_else(|| Format::from_path(&req.output)).unwrap_or_default();
    let library;
    let list: HistoryList;
    let selections: Vec<Selection> = match &req.history {
        Some(query) => {
            let on_drive = plays::drive_history(&d)?.lists;
            list = match plays::find_list(&on_drive, query) {
                Some(l) => l.clone(),
                None if req.archived => {
                    let store = store.ok_or_else(|| Error::invalid("archived history lists need the store"))?;
                    let library = d.state().and_then(|s| s.library_id);
                    let archive = plays::archive(store, library, ctx)?;
                    plays::find_list(archive.lists.iter().map(|a| &a.list), query)
                        .cloned()
                        .ok_or_else(|| Error::NotFound(format!("history list {query:?}")))?
                }
                None => return Err(Error::NotFound(format!("history list {query:?} on {}", d.name()))),
            };
            let file = PathBuf::from(crate::paths::sanitize_component(&list.name));
            vec![Selection { name: list.path.clone(), file, tracks: list.tracks.iter().collect::<Vec<&Track>>() }]
        }
        None => {
            library = crate::convert::export_library(&d)?;
            export::select(&library, req.playlist.as_deref(), req.split, &d.name())?
        }
    };
    let paths: Vec<PathBuf> = selections
        .iter()
        .map(|s| match req.split {
            true => with_extension(&req.output.join(&s.file), format),
            false => req.output.clone(),
        })
        .collect();
    fsutil::refuse_to_replace(&paths.iter().map(PathBuf::as_path).collect::<Vec<_>>(), req.replace)?;
    let mut files = Vec::new();
    for (s, path) in selections.iter().zip(paths) {
        let text = export::render(format, &s.name, &s.tracks, &Locator::new(req.paths, d.root(), &path));
        fsutil::write_atomic(&path, text.as_bytes())?;
        files.push(ExportedFile { path, name: s.name.clone(), tracks: s.tracks.len() });
    }
    Ok(ExportReport { format, files })
}

/// `name` + `.ext` (names may contain dots of their own).
fn with_extension(path: &Path, format: Format) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!("{name}.{}", format.extension()))
}
