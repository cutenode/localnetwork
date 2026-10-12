//! Properties of a new Engine Library, read back with SQLite: its tracks, playlist tree and
//! entries are the library's, linked the way Engine walks them.

use std::path::{Path, PathBuf};

use hegel::TestCase;
use hegel::generators as gs;
use loclib_interop::{Content, Error, Export, Source, engine};
use loclib_model::paths::{self, PathStyle};
use loclib_model::{Library, Track};
use loclib_progress::{CancelToken, Ctx};
use rusqlite::{Connection, OptionalExtension};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::common::{self, KEYS};

/// Days from 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's `days_from_civil`), when
/// the date exists.
fn days(y: i32, m: u32, d: u32) -> Option<i64> {
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1..=12).contains(&m) || d == 0 || d > month_days[m as usize - 1] {
        return None;
    }
    let y = i64::from(y) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// `rel` followed from the folder `from`, each `..` going up one.
fn follow(from: &Path, rel: &str) -> PathBuf {
    let mut p = from.to_path_buf();
    for part in rel.split('/') {
        if part == ".." {
            assert!(p.pop(), "{rel} climbs past the root of {}", from.display());
        } else {
            p.push(part);
        }
    }
    p
}

/// The database an export wrote, opened from a temporary copy.
fn open(export: &Export) -> (tempfile::TempDir, Connection) {
    assert_eq!(export.files.len(), 1);
    let file = &export.files[0];
    assert_eq!(file.path, Path::new("/out/Engine Library/Database2/m.db"));
    let Content::Bytes(bytes) = &file.content else { panic!("the database is written out") };
    assert_eq!(file.bytes, bytes.len() as u64);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("m.db"), bytes).unwrap();
    let conn = Connection::open(dir.path().join("m.db")).unwrap();
    (dir, conn)
}

/// The rows of a linked list, `rows` being (id, next) with 0 ending it, from its head. Every row
/// must be on it once.
fn chain(rows: &[(i64, i64)]) -> Vec<i64> {
    if rows.is_empty() {
        return Vec::new();
    }
    let nexts: FxHashSet<i64> = rows.iter().map(|r| r.1).collect();
    let heads: Vec<i64> = rows.iter().map(|r| r.0).filter(|id| !nexts.contains(id)).collect();
    assert_eq!(heads.len(), 1, "one head: {rows:?}");
    let next: FxHashMap<i64, i64> = rows.iter().copied().collect();
    let mut out = vec![heads[0]];
    while let Some(&n) = next.get(out.last().unwrap()).filter(|&&n| n != 0) {
        out.push(n);
        assert!(out.len() <= rows.len(), "a loop: {rows:?}");
    }
    assert_eq!(out.len(), rows.len(), "every row is on the list: {rows:?}");
    out
}

/// Every file is one `Track` row in library order, with its tags as Engine stores them and a path
/// that leads from the `Engine Library` folder to the audio (or, for a drive, to its place on the
/// drive), each with its `PerformanceData` row; every playlist and folder is a `Playlist` under its
/// folder, siblings linked in display order with names made unique, and every playlist's tracks are
/// linked in order, repeats left out and counted. The database passes SQLite's checks.
#[hegel::test(test_cases = 100)]
fn library_reads_back(tc: TestCase) {
    let lib = common::library(&tc, 5);
    let root = PathBuf::from(tc.draw(gs::sampled_from(vec!["/out", "/Volumes/USB"])));
    // Both styles, since a case costs a new SQLite database each time.
    for style in [PathStyle::Drive, PathStyle::Relative] {
        reads_back(&lib, &root, style);
    }
}

fn reads_back(lib: &Library, root: &Path, style: PathStyle) {
    let src = Source { library: lib, root: root.to_path_buf(), analysis: None };
    let before = chrono::Utc::now().timestamp();
    let export = engine::build(&src, Path::new("/out"), style, &Ctx::silent()).unwrap();
    let after = chrono::Utc::now().timestamp();
    let (_dir, db) = open(&export);
    let check: String = db.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
    assert_eq!(check, "ok");
    let db_dir = Path::new("/out/Engine Library/Database2");
    assert_eq!(export.remove, [db_dir.join("m.db-wal"), db_dir.join("m.db-shm")]);

    let (uuid, version): (String, (u32, u32, u32)) = db
        .query_row(
            "SELECT uuid, schemaVersionMajor, schemaVersionMinor, schemaVersionPatch FROM Information",
            [],
            |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))),
        )
        .unwrap();
    assert_eq!(version, engine::SCHEMA);
    assert!(uuid::Uuid::parse_str(&uuid).is_ok(), "{uuid}");

    // One row per file, whatever the case of its path.
    let file = |t: &Track| src.audio(t).to_string_lossy().to_lowercase();
    let written = common::first_per_file(lib, file);
    let rows: Vec<TrackRow> = db
        .prepare("SELECT * FROM Track ORDER BY id")
        .unwrap()
        .query_map([], TrackRow::read)
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!((rows.len(), export.tracks), (written.len(), written.len()));
    let mut row_of_file: FxHashMap<String, i64> = FxHashMap::default();
    for (t, row) in written.iter().zip(&rows) {
        row_of_file.insert(file(t), row.id);
        match style {
            PathStyle::Drive => assert_eq!(row.path, format!("../{}", paths::key(&t.file_path))),
            _ => assert_eq!(follow(Path::new("/out/Engine Library"), &row.path), src.audio(t), "{}", row.path),
        }
        assert!(!row.path.starts_with('/'), "never absolute");
        assert_eq!(row.filename, row.path.rsplit('/').next().unwrap());
        let ext = row.filename.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
        assert_eq!(Some(row.file_type.clone()), ext);
        let text = |s: &String| (!s.is_empty()).then(|| s.clone());
        let tags = [&t.title, &t.artist, &t.album, &t.genre, &t.comment, &t.label, &t.composer, &t.remixer];
        assert_eq!(row.tags, tags.map(text));
        assert_eq!(row.length, i64::from(t.duration_secs));
        assert_eq!(row.bpm, (t.bpm > 0.0).then(|| t.bpm.round() as i64));
        assert_eq!(row.bpm_analyzed, (t.bpm > 0.0).then_some(t.bpm));
        assert_eq!(row.year, (t.year > 0).then_some(i64::from(t.year)));
        assert_eq!(row.bitrate, (t.bitrate > 0).then_some(i64::from(t.bitrate)));
        assert_eq!(row.play_order, (t.track_number > 0).then_some(i64::from(t.track_number)));
        assert_eq!(row.file_bytes, t.file_size as i64);
        assert_eq!(row.key, KEYS.iter().find(|k| k.0 == t.key).unwrap().2, "{}", t.key);
        assert_eq!(row.rating, i64::from(t.rating.min(5)) * 20);
        assert_eq!(row.is_played, t.play_count > 0);
        assert_eq!((row.is_analyzed, row.album_art_id), (false, 1), "Engine analyses it; no artwork");
        assert_eq!(row.origin_uuid, uuid);
        match common::ymd(&t.date_added).and_then(|(y, m, d)| days(y, m, d)) {
            Some(days) => assert_eq!(row.date_added, days * 86_400, "{}", t.date_added),
            None => assert!((before..=after).contains(&row.date_added), "now for {:?}", t.date_added),
        }
        let performance: Option<i64> = db
            .query_row("SELECT trackId FROM PerformanceData WHERE trackId = ?1", [row.id], |r| r.get(0))
            .optional()
            .unwrap();
        assert_eq!(performance, Some(row.id));
    }
    let engine_id: FxHashMap<u32, i64> = lib.tracks.iter().map(|t| (t.id, row_of_file[&file(t)])).collect();

    let mut seen = Seen::default();
    same_lists(&db, lib, None, 0, &engine_id, &uuid, &mut seen);
    let shared = lib.tracks.len() - written.len();
    let mut want = Vec::new();
    if seen.repeats > 0 {
        want.push(format!(
            "{} repeated playlist entr(ies) left out: an Engine playlist holds a track once",
            seen.repeats
        ));
    }
    if shared > 0 {
        want.push(format!("{shared} track row(s) share a file with another and are one Engine track"));
    }
    assert_eq!(export.warnings, want);
    let summary =
        format!("with {} tracks and {} playlists holding {} entries", written.len(), seen.lists, seen.entries);
    assert!(export.notes[0].contains(&summary), "{}", export.notes[0]);
    assert_eq!(export.playlists + export.folders, seen.lists);
}

/// What [`same_lists`] found.
#[derive(Default)]
struct Seen {
    lists: usize,
    entries: usize,
    repeats: usize,
}

/// Engine's playlists under `engine_parent` are the library's children of `parent`, in order and
/// with their entries.
fn same_lists(
    db: &Connection,
    lib: &Library,
    parent: Option<u32>,
    engine_parent: i64,
    engine_id: &FxHashMap<u32, i64>,
    uuid: &str,
    seen: &mut Seen,
) {
    let mut q = db.prepare("SELECT id, nextListId, title FROM Playlist WHERE parentListId = ?1").unwrap();
    let rows: Vec<(i64, i64, String)> =
        q.query_map([engine_parent], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect();
    let order = chain(&rows.iter().map(|r| (r.0, r.1)).collect::<Vec<_>>());
    let title: FxHashMap<i64, &str> = rows.iter().map(|r| (r.0, r.2.as_str())).collect();
    assert_eq!(title.values().collect::<FxHashSet<_>>().len(), rows.len(), "sibling names are unique");
    let children = lib.children(parent);
    assert_eq!(order.len(), children.len());
    for (p, id) in children.iter().zip(order) {
        seen.lists += 1;
        // Its own name, or that and ` (2)`, ` (3)`...
        let t = title[&id];
        let suffix = t.strip_prefix(p.name.as_str()).unwrap_or_else(|| panic!("{t:?} is {:?}'s", p.name));
        let n = suffix.strip_prefix(" (").and_then(|s| s.strip_suffix(')')).and_then(|n| n.parse::<u32>().ok());
        assert!(suffix.is_empty() || n.is_some_and(|n| n >= 2), "{t:?}");
        if p.is_folder {
            same_lists(db, lib, Some(p.id), id, engine_id, uuid, seen);
            continue;
        }
        let mut q =
            db.prepare("SELECT id, nextEntityId, trackId, databaseUuid FROM PlaylistEntity WHERE listId = ?1").unwrap();
        let ents: Vec<(i64, i64, i64, String)> = q
            .query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(ents.iter().all(|e| e.3 == uuid), "entries carry the library's UUID");
        let track: FxHashMap<i64, i64> = ents.iter().map(|e| (e.0, e.2)).collect();
        let links: Vec<(i64, i64)> = ents.iter().map(|e| (e.0, e.1)).collect();
        let got: Vec<i64> = chain(&links).iter().map(|e| track[e]).collect();
        let listed: Vec<i64> = p.track_ids.iter().filter_map(|t| engine_id.get(t).copied()).collect();
        let mut once = FxHashSet::default();
        let want: Vec<i64> = listed.iter().copied().filter(|t| once.insert(*t)).collect();
        assert_eq!(got, want);
        seen.entries += want.len();
        seen.repeats += listed.len() - want.len();
    }
}

/// The `Track` columns the export fills from the library.
struct TrackRow {
    id: i64,
    path: String,
    filename: String,
    file_type: String,
    /// title, artist, album, genre, comment, label, composer, remixer
    tags: [Option<String>; 8],
    length: i64,
    bpm: Option<i64>,
    bpm_analyzed: Option<f64>,
    year: Option<i64>,
    bitrate: Option<i64>,
    play_order: Option<i64>,
    file_bytes: i64,
    key: Option<i64>,
    rating: i64,
    is_played: bool,
    is_analyzed: bool,
    album_art_id: i64,
    origin_uuid: String,
    date_added: i64,
}

impl TrackRow {
    fn read(r: &rusqlite::Row<'_>) -> rusqlite::Result<TrackRow> {
        Ok(TrackRow {
            id: r.get("id")?,
            path: r.get("path")?,
            filename: r.get("filename")?,
            file_type: r.get("fileType")?,
            tags: [
                r.get("title")?,
                r.get("artist")?,
                r.get("album")?,
                r.get("genre")?,
                r.get("comment")?,
                r.get("label")?,
                r.get("composer")?,
                r.get("remixer")?,
            ],
            length: r.get("length")?,
            bpm: r.get("bpm")?,
            bpm_analyzed: r.get("bpmAnalyzed")?,
            year: r.get("year")?,
            bitrate: r.get("bitrate")?,
            play_order: r.get("playOrder")?,
            file_bytes: r.get("fileBytes")?,
            key: r.get("key")?,
            rating: r.get("rating")?,
            is_played: r.get("isPlayed")?,
            is_analyzed: r.get("isAnalyzed")?,
            album_art_id: r.get("albumArtId")?,
            origin_uuid: r.get("originDatabaseUuid")?,
            date_added: r.get("dateAdded")?,
        })
    }
}

/// A library can't be written with absolute paths, nor relative to an output folder that shares
/// no root with the audio, and a cancelled export stops: each fails with its typed error once
/// there's a track to place, and an empty library always works.
#[hegel::test(test_cases = 100)]
fn exports_that_cant_be_written_fail(tc: TestCase) {
    let lib = common::library(&tc, 2);
    let src = Source { library: &lib, root: PathBuf::from("/Volumes/USB"), analysis: None };
    for (output, style, cancelled) in
        [("/out", PathStyle::Absolute, false), ("out", PathStyle::Relative, false), ("/out", PathStyle::Drive, true)]
    {
        let cancel = CancelToken::new();
        if cancelled {
            cancel.cancel();
        }
        match engine::build(&src, Path::new(output), style, &Ctx { cancel, ..Ctx::silent() }) {
            Ok(export) => assert_eq!((lib.tracks.len(), export.tracks), (0, 0)),
            Err(Error::Cancelled(_)) => assert!(cancelled && !lib.tracks.is_empty()),
            Err(Error::Invalid(e)) => {
                assert!(!cancelled && !lib.tracks.is_empty());
                assert_eq!(e.contains("different drives"), style == PathStyle::Relative, "{e}");
            }
            Err(e) => panic!("{e}"),
        }
    }
}
