//! Engine DJ (Denon DJ, Numark, RANE): a new Engine Library, `Engine Library/Database2/m.db`.
//!
//! The library is plain SQLite. This writes schema 3.0.2, what Engine DJ Desktop and Engine OS
//! 4.5 and 5.0 create (`engine.sql`, from libdjinterop's description; newer Engine versions
//! upgrade older libraries when they open them):
//!
//! * `Information`: the library's UUID and schema version.
//! * `Track`: one row per audio file (`path` is unique): `path` relative to the `Engine Library`
//!   folder (`../Contents/Artist/x.mp3`), `length` in seconds, `bpm` rounded and `bpmAnalyzed`
//!   exact, `key` 0-23 round the Camelot wheel from C major (0) and A minor (1) up a fifth at a
//!   time, `rating` 0-100, dates as Unix seconds, `albumArtId` 1 for none (the empty
//!   `AlbumArt` row 1).
//! * `PerformanceData`: a row per track (a trigger adds it) for the analysis Engine keeps in
//!   compressed blobs: beat grid, cues, loops, waveforms. They're left empty and `isAnalyzed` is 0,
//!   so Engine analyses the tracks itself: the blobs' layout (samples rather than milliseconds,
//!   sample counts rekordbox doesn't keep) isn't something to guess at in someone's library.
//! * `Playlist`: a tree (`parentListId`, 0 at the top) whose siblings are a linked list
//!   (`nextListId`, 0 for the last; triggers keep it when rows are inserted before a sibling or at
//!   the end). A rekordbox folder becomes a playlist holding playlists. Names are unique among
//!   siblings.
//! * `PlaylistEntity`: a playlist's tracks as a linked list (`nextEntityId`, 0 for the last), each
//!   track at most once per playlist, with the library's UUID.

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::Path;

use rusqlite::{Connection, params};

use loclib_model::harmony::Camelot;
use loclib_model::paths::{self, PathStyle};
use loclib_model::{Library, Track};
use loclib_progress::{Ctx, Event, Phase};

use crate::{Error, Export, Result, Source};

/// The schema version written.
pub const SCHEMA: (u32, u32, u32) = (3, 0, 2);

/// `albumArtId` for "no album art": the empty row every library starts with.
const NO_ALBUM_ART: i64 = 1;

/// Engine's key number: round the Camelot wheel from 8B (C major, 0) and 8A (A minor, 1).
fn key_number(key: &str) -> Option<i64> {
    let c = Camelot::parse(key)?;
    Some((i64::from(c.number) - 8).rem_euclid(12) * 2 + i64::from(!c.major))
}

/// `2026-09-30` as Unix seconds at midnight UTC.
fn unix_date(s: &str) -> Option<i64> {
    let d = chrono::NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()?;
    Some(d.and_hms_opt(0, 0, 0)?.and_utc().timestamp())
}

fn sql(e: rusqlite::Error) -> Error {
    Error::Invalid(format!("building the Engine Library: {e}"))
}

/// A new Engine Library in `output` (absolute, with symlinks resolved: the database finds tracks
/// relative to it) with `src`'s tracks, playlists and folders, its tracks found as `style` says:
/// relative to the library (`Relative`) or to the drive's root, for a library copied there
/// (`Drive`). Engine Library paths are never absolute.
pub fn build(src: &Source<'_>, output: &Path, style: PathStyle, ctx: &Ctx<'_>) -> Result<Export> {
    let lib = src.library;
    let library_dir = output.join("Engine Library");
    let db_path = library_dir.join("Database2").join("m.db");
    let dir = tempfile::tempdir().map_err(|e| Error::Io { path: std::env::temp_dir(), source: e })?;
    let scratch = dir.path().join("m.db");
    let mut conn = Connection::open(&scratch).map_err(sql)?;
    // The database is built in a scratch file and read back into memory, so nothing needs to
    // reach the disk before it's done: no syncs, and the whole library (schema included) in one
    // transaction rather than a journal and its syncs for each statement.
    conn.execute_batch("PRAGMA journal_mode = DELETE; PRAGMA synchronous = OFF; PRAGMA foreign_keys = ON;")
        .map_err(sql)?;
    let tx = conn.transaction().map_err(sql)?;
    tx.execute_batch(include_str!("engine.sql")).map_err(sql)?;
    let uuid = uuid::Uuid::new_v4().to_string();
    let mut indicator = [0u8; 8];
    getrandom::fill(&mut indicator).map_err(|e| Error::Invalid(format!("no randomness available: {e}")))?;
    tx.execute(
        "INSERT INTO Information (uuid, schemaVersionMajor, schemaVersionMinor, schemaVersionPatch,
             currentPlayedIndiciator, lastRekordBoxLibraryImportReadCounter) VALUES (?1, ?2, ?3, ?4, ?5, 0)",
        params![uuid, SCHEMA.0, SCHEMA.1, SCHEMA.2, i64::from_le_bytes(indicator) & i64::MAX],
    )
    .map_err(sql)?;
    tx.execute("INSERT INTO AlbumArt (id, hash, albumArt) VALUES (1, '', NULL)", []).map_err(sql)?;

    // Where the database will say each track is.
    let located = |t: &Track| -> Result<String> {
        let key = paths::key(&t.file_path);
        match style {
            PathStyle::Drive => Ok(format!("../{key}")),
            PathStyle::Relative => {
                let audio = src.audio(t);
                paths::relative(&library_dir, &audio).ok_or_else(|| {
                    Error::Invalid(format!(
                        "{} and {} are on different drives; give paths relative to the drive and copy Engine Library to the drive",
                        library_dir.display(),
                        audio.display()
                    ))
                })
            }
            PathStyle::Absolute => {
                Err(Error::Invalid("Engine Library paths are relative: use relative or drive".into()))
            }
        }
    };

    let now = chrono::Utc::now().timestamp();
    let mut engine_ids: FxHashMap<u32, i64> = FxHashMap::default();
    let mut by_file: FxHashMap<String, i64> = FxHashMap::default();
    ctx.phase_start(Phase::Scan, lib.tracks.len() as u64, 0);
    for t in &lib.tracks {
        ctx.check()?;
        ctx.emit(Event::Item { phase: Phase::Scan, path: t.file_path.clone(), bytes: 0 });
        let path = located(t)?;
        if let Some(id) = by_file.get(&crate::file_key(t)) {
            engine_ids.insert(t.id, *id);
            continue;
        }
        let filename = path.rsplit('/').next().unwrap_or_default().to_string();
        let file_type = filename.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        let text = |s: &str| (!s.is_empty()).then(|| s.to_string());
        let added = unix_date(&t.date_added).unwrap_or(now);
        tx.prepare_cached(
            "INSERT INTO Track (playOrder, length, bpm, year, path, filename, bitrate, bpmAnalyzed, albumArtId,
                 fileBytes, title, artist, album, genre, comment, label, composer, remixer, key, rating, albumArt,
                 timeLastPlayed, isPlayed, fileType, isAnalyzed, dateCreated, dateAdded, isAvailable,
                 isMetadataOfPackedTrackChanged, isPerfomanceDataOfPackedTrackChanged, playedIndicator,
                 isMetadataImported, pdbImportKey, streamingSource, uri, isBeatGridLocked, originDatabaseUuid,
                 originTrackId, streamingFlags, explicitLyrics, lastEditTime)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, NULL,
                 NULL, ?21, ?22, 0, ?23, ?23, 1, 0, 0, NULL, 1, 0, NULL, NULL, 0, ?24, 0, 0, 0, ?25)",
        )
        .map_err(sql)?
        .execute(params![
            (t.track_number > 0).then_some(t.track_number),
            t.duration_secs,
            (t.bpm > 0.0).then(|| t.bpm.round() as i64),
            (t.year > 0).then_some(t.year),
            path,
            filename,
            (t.bitrate > 0).then_some(t.bitrate),
            (t.bpm > 0.0).then_some(t.bpm),
            NO_ALBUM_ART,
            t.file_size as i64,
            text(&t.title),
            text(&t.artist),
            text(&t.album),
            text(&t.genre),
            text(&t.comment),
            text(&t.label),
            text(&t.composer),
            text(&t.remixer),
            key_number(&t.key),
            i64::from(t.rating.min(5)) * 20,
            t.play_count > 0,
            file_type,
            added,
            uuid,
            now,
        ])
        .map_err(sql)?;
        let id = tx.last_insert_rowid();
        by_file.insert(crate::file_key(t), id);
        engine_ids.insert(t.id, id);
    }
    ctx.phase_end(Phase::Scan);

    let (lists, entries, repeats) = write_playlists(&tx, lib, &engine_ids, &uuid, now)?;
    tx.commit().map_err(sql)?;
    let check: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0)).map_err(sql)?;
    let broken: i64 = conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r.get(0)).map_err(sql)?;
    if check != "ok" || broken > 0 {
        return Err(Error::Invalid(format!(
            "the Engine Library failed its check: {check}, {broken} broken reference(s)"
        )));
    }
    drop(conn);
    let bytes = std::fs::read(&scratch).map_err(|e| Error::Io { path: scratch.clone(), source: e })?;

    let mut export = Export::new(lib);
    export.tracks = by_file.len();
    export.notes.push(format!(
        "A new Engine Library (schema {}.{}.{}, Engine DJ 4.5 and later) with {} tracks and {lists} playlists holding {entries} \
         entries. In Engine DJ Desktop, add the output folder's Engine Library as a source (Engine DJ upgrades it if it's \
         newer), then drag its playlists into your collection.",
        SCHEMA.0,
        SCHEMA.1,
        SCHEMA.2,
        by_file.len()
    ));
    export.notes.push(match style {
        PathStyle::Drive => "Tracks are found relative to the drive: copy the Engine Library folder to the drive's root, \
                             next to Contents/, and players and Engine DJ read it from there."
            .into(),
        _ => format!(
            "Tracks are found relative to {}: keep the Engine Library folder there, or export with `PathStyle::Drive` to \
             put it on the drive.",
            library_dir.display()
        ),
    });
    export.notes.push(
        "Beat grids, cues, loops and waveforms aren't written (Engine keeps them in its own encoded form): Engine \
         analyses the tracks when it first reads them. Artwork, colours and history lists aren't carried."
            .into(),
    );
    if repeats > 0 {
        export
            .warnings
            .push(format!("{repeats} repeated playlist entr(ies) left out: an Engine playlist holds a track once"));
    }
    if lib.tracks.len() > by_file.len() {
        export.warnings.push(format!(
            "{} track row(s) share a file with another and are one Engine track",
            lib.tracks.len() - by_file.len()
        ));
    }
    for stale in ["m.db-wal", "m.db-shm"] {
        export.remove.push(db_path.with_file_name(stale));
    }
    export.file(db_path, "Engine Library database: tracks and playlists", bytes);
    Ok(export)
}

/// Every playlist and folder (as playlists holding playlists), siblings in order, and their
/// entries. Returns the playlists written, the entries, and the repeats left out.
fn write_playlists(
    tx: &rusqlite::Transaction<'_>,
    lib: &Library,
    ids: &FxHashMap<u32, i64>,
    uuid: &str,
    now: i64,
) -> Result<(usize, usize, usize)> {
    let (mut lists, mut entries, mut repeats) = (0, 0, 0);
    let mut stack: Vec<(Option<u32>, i64)> = vec![(None, 0)];
    while let Some((parent, engine_parent)) = stack.pop() {
        let mut names = FxHashSet::default();
        for p in lib.children(parent) {
            // Siblings need distinct names.
            let mut name = p.name.clone();
            let mut n = 2;
            while !names.insert(name.clone()) {
                name = format!("{} ({n})", p.name);
                n += 1;
            }
            // nextListId 0 appends: the triggers point the previous last sibling at the new row.
            tx.prepare_cached(
                "INSERT INTO Playlist (title, parentListId, isPersisted, nextListId, lastEditTime, isExplicitlyExported)
                 VALUES (?1, ?2, 1, 0, ?3, 1)",
            )
            .and_then(|mut s| s.execute(params![name, engine_parent, now]))
            .map_err(sql)?;
            let list = tx.last_insert_rowid();
            lists += 1;
            if p.is_folder {
                stack.push((Some(p.id), list));
                continue;
            }
            let mut seen = FxHashSet::default();
            let mut previous: Option<i64> = None;
            for track in p.track_ids.iter().filter_map(|id| ids.get(id)) {
                if !seen.insert(*track) {
                    repeats += 1;
                    continue;
                }
                tx.prepare_cached(
                    "INSERT INTO PlaylistEntity (listId, trackId, databaseUuid, nextEntityId, membershipReference)
                     VALUES (?1, ?2, ?3, 0, 0)",
                )
                .and_then(|mut s| s.execute(params![list, track, uuid]))
                .map_err(sql)?;
                let entity = tx.last_insert_rowid();
                if let Some(prev) = previous {
                    tx.prepare_cached("UPDATE PlaylistEntity SET nextEntityId = ?1 WHERE id = ?2")
                        .and_then(|mut s| s.execute(params![entity, prev]))
                        .map_err(sql)?;
                }
                previous = Some(entity);
                entries += 1;
            }
        }
    }
    Ok((lists, entries, repeats))
}

#[cfg(test)]
mod tests;
