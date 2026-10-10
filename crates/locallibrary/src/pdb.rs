//! Reading `export.pdb` into the [`Library`] model.
//!
//! Parsing is delegated to the (vendored) `rekordbox-pdb` crate. That crate indexes raw byte
//! buffers directly, so a truncated or corrupt file from a half-written USB drive could panic; we
//! contain that with `catch_unwind` and surface a normal parse error instead.

use rustc_hash::FxHashMap;
use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::Once;

use rekordbox_pdb::Database;
use schemars::JsonSchema;
use serde::Serialize;

use crate::error::IoContext;
use crate::model::{Library, Playlist, Track, TrackColor, untangle};
use crate::{Error, Result};

/// Header fields useful for telling exports apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PdbHeader {
    pub page_size: u32,
    pub num_tables: u32,
    /// Incremented by rekordbox on every save; higher means a later revision of the *same* file.
    pub sequence: u32,
}

pub fn read_header(bytes: &[u8]) -> Option<PdbHeader> {
    if bytes.len() < 0x1c {
        return None;
    }
    let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    Some(PdbHeader { page_size: u32_at(4), num_tables: u32_at(8), sequence: u32_at(0x14) })
}

thread_local! {
    static QUIET_PANICS: Cell<bool> = const { Cell::new(false) };
}

/// Run `f`, catching panics without printing them. The panic hook is installed once and
/// consults a thread-local flag, so concurrent callers can't clobber each other's hooks.
pub(crate) fn catch_quietly<T>(f: impl FnOnce() -> T) -> std::thread::Result<T> {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if !QUIET_PANICS.with(Cell::get) {
                previous(info)
            }
        }));
    });
    QUIET_PANICS.with(|q| q.set(true));
    let result = panic::catch_unwind(AssertUnwindSafe(f));
    QUIET_PANICS.with(|q| q.set(false));
    result
}

/// Run database-editor code, converting a panic (the editor indexes raw bytes and can panic on
/// malformed input) into an error so a corrupt drive can't take the whole process down.
pub(crate) fn guard_editor<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    match catch_quietly(f) {
        Ok(r) => r,
        Err(_) => Err(Error::Pdb("the database is malformed; it was left unchanged".into())),
    }
}

/// Parse a `Database`, converting panics on malformed input into errors.
pub fn parse_database(bytes: Vec<u8>, path: &Path) -> Result<Database> {
    validate(&bytes, path)?;
    match catch_quietly(|| Database::from_bytes(bytes)) {
        Ok(Ok(db)) => Ok(db),
        Ok(Err(e)) => Err(Error::parse(path, e)),
        Err(_) => Err(Error::parse(path, "malformed database (truncated or corrupt pages)")),
    }
}

/// Check what the vendored readers take on trust, for `export.pdb` and `exportExt.pdb` alike
/// (one layout): the page size, the table directory, and that every table's page chain stays in
/// the file and reaches its last page without a cycle.
pub(crate) fn validate(bytes: &[u8], path: &Path) -> Result<()> {
    let header = read_header(bytes).ok_or_else(|| Error::parse(path, "file too short"))?;
    let page_size = header.page_size as usize;
    if !(512..=65536).contains(&page_size) || !bytes.len().is_multiple_of(page_size) {
        return Err(Error::parse(path, format!("unexpected page size {page_size} for {} bytes", bytes.len())));
    }
    if header.num_tables == 0 || 0x1c + header.num_tables as usize * 16 > page_size {
        return Err(Error::parse(path, "invalid table directory"));
    }
    // Every table's page chain must stay inside the file and reach its last page without cycles.
    let pages = bytes.len() / page_size;
    let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()) as usize;
    for t in 0..header.num_tables as usize {
        let entry = 0x1c + t * 16;
        let (first, last) = (u32_at(entry + 8), u32_at(entry + 12));
        let mut page = first;
        let mut steps = 0;
        loop {
            if page >= pages {
                return Err(Error::parse(path, format!("table {t} points past the end of the file")));
            }
            if page == last {
                break;
            }
            steps += 1;
            if steps > pages {
                return Err(Error::parse(path, format!("table {t} has a cyclic page chain")));
            }
            page = u32_at(page * page_size + 12);
        }
    }
    Ok(())
}

pub fn read_library(path: &Path) -> Result<Library> {
    let bytes = std::fs::read(path).at(path)?;
    let db = parse_database(bytes, path)?;
    Ok(library_from_database(&db))
}

pub fn library_from_database(db: &Database) -> Library {
    let artists: FxHashMap<u32, &str> = db.artists.iter().map(|a| (a.id, a.name.as_str())).collect();
    let albums: FxHashMap<u32, (&str, u32)> =
        db.albums.iter().map(|a| (a.id, (a.name.as_str(), a.artist_id))).collect();
    let genres: FxHashMap<u32, &str> = db.genres.iter().map(|g| (g.id, g.name.as_str())).collect();
    let labels: FxHashMap<u32, &str> = db.labels.iter().map(|l| (l.id, l.name.as_str())).collect();
    let keys: FxHashMap<u32, &str> = db.keys.iter().map(|k| (k.id, k.name.as_str())).collect();
    let artwork: FxHashMap<u32, &str> = db.artwork.iter().map(|a| (a.id, a.path.as_str())).collect();
    let name = |m: &FxHashMap<u32, &str>, id: u32| m.get(&id).copied().unwrap_or_default().to_string();

    let tracks = db
        .tracks
        .iter()
        .map(|t| {
            let (album, album_artist_id) = albums.get(&t.album_id).copied().unwrap_or(("", 0));
            Track {
                id: t.id,
                title: t.title().to_string(),
                artist: name(&artists, t.artist_id),
                album: album.to_string(),
                album_artist: name(&artists, album_artist_id),
                genre: name(&genres, t.genre_id),
                label: name(&labels, t.label_id),
                key: name(&keys, t.key_id),
                remixer: name(&artists, t.remixer_id),
                original_artist: name(&artists, t.original_artist_id),
                composer: name(&artists, t.composer_id),
                comment: t.comment().to_string(),
                mix_name: t.mix_name().to_string(),
                isrc: t.strings[0].clone(),
                bpm: t.tempo as f64 / 100.0,
                duration_secs: t.duration as u32,
                year: t.year,
                track_number: t.track_number,
                disc_number: t.disc_number,
                rating: t.rating,
                color: TrackColor::from_id(t.color_id),
                play_count: t.play_count,
                bitrate: t.bitrate,
                sample_rate: t.sample_rate,
                sample_depth: t.sample_depth,
                file_size: t.file_size as u64,
                file_path: t.file_path().to_string(),
                analyze_path: t.analyze_path().to_string(),
                date_added: t.date_added().to_string(),
                release_date: t.release_date().to_string(),
                artwork_id: (t.artwork_id != 0 && artwork.contains_key(&t.artwork_id)).then_some(t.artwork_id),
                artwork_path: artwork.get(&t.artwork_id).map(|p| p.to_string()),
            }
        })
        .collect();

    let mut entries: FxHashMap<u32, Vec<(u32, u32)>> = FxHashMap::default();
    for e in &db.playlist_entries {
        entries.entry(e.playlist_id).or_default().push((e.entry_index, e.track_id));
    }
    let playlists = untangle(
        db.playlist_tree
            .iter()
            .map(|n| {
                let mut list = entries.remove(&n.id).unwrap_or_default();
                list.sort_unstable();
                Playlist {
                    id: n.id,
                    parent_id: (n.parent_id != 0).then_some(n.parent_id),
                    name: n.name.clone(),
                    is_folder: n.is_folder,
                    sort_order: n.sort_order,
                    track_ids: list.into_iter().map(|(_, t)| t).collect(),
                }
            })
            .collect(),
    );

    let mut history_entries: FxHashMap<u32, Vec<(u32, u32)>> = FxHashMap::default();
    for e in &db.history_entries {
        history_entries.entry(e.playlist_id).or_default().push((e.entry_index, e.track_id));
    }
    let history = db
        .history_playlists
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let mut list = history_entries.remove(&h.id).unwrap_or_default();
            list.sort_unstable();
            Playlist {
                id: h.id,
                parent_id: None,
                name: h.name.clone(),
                is_folder: false,
                sort_order: i as u32,
                track_ids: list.into_iter().map(|(_, t)| t).collect(),
            }
        })
        .collect();

    Library { tracks, playlists, history_playlists: db.history_playlists.len(), history }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_garbage_without_panicking() {
        let p = Path::new("x.pdb");
        assert!(parse_database(vec![], p).is_err());
        assert!(parse_database(vec![0u8; 100], p).is_err());
        // Valid-looking header, garbage pages.
        let mut buf = vec![0xffu8; 4096 * 4];
        buf[0..4].copy_from_slice(&0u32.to_le_bytes());
        buf[4..8].copy_from_slice(&4096u32.to_le_bytes());
        buf[8..12].copy_from_slice(&3u32.to_le_bytes());
        assert!(parse_database(buf, p).is_err());
    }
}
