//! The plugin served over pipes to a host that answers its reads as locallibrary does, its files
//! compared with what loclib-interop writes for the same library, root and analysis (the core
//! exports' `Source`), as `locallibrary export --format traktor|serato|engine` writes them.

use std::io::pipe;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use hegel::TestCase;
use hegel::generators as gs;
use loclib_interop::{Content, File};
use loclib_model::analysis::{Beat, Cue};
use loclib_model::model::{ONELIBRARY_ONLY, Playlist};
use loclib_model::test_util::track;
use loclib_plugin::{Capability, ExportFrom, Incoming, Manifest, PROTOCOL_VERSION, Peer, check_params, method};
use loclib_progress::Ctx;
use serde_json::json;
use tempfile::TempDir;

use super::*;

/// The most tracks the host gives a plugin in a page of `drive.library`.
const PAGE: usize = 500;

/// The id OneLibrary gave the track only it has, as the host's combined library numbers it.
const PLUS: u32 = ONELIBRARY_ONLY | 1;

/// A drive `USB`: its folder, holding the audio (an MP3 Serato's tag can go into for each track,
/// but a FLAC file for the third), the library exports read and each track's beat grid and cues;
/// and the sets and mixes the store keeps, by id (none unless a test adds them).
struct Drive {
    dir: TempDir,
    root: PathBuf,
    library: Library,
    analysis: FxHashMap<u32, TrackAnalysis>,
    kept: FxHashMap<String, Value>,
}

/// An MP3 lofty reads: an ID3v2.3 tag with a title, then MPEG-1 layer III frames of silence.
fn mp3(title: &str) -> Vec<u8> {
    let mut frame = b"TIT2".to_vec();
    frame.extend_from_slice(&(1 + title.len() as u32).to_be_bytes());
    frame.extend_from_slice(&[0, 0, 0]);
    frame.extend_from_slice(title.as_bytes());
    let size = frame.len() as u32;
    let mut out = b"ID3\x03\x00\x00".to_vec();
    out.extend((0..4).rev().map(|i| ((size >> (7 * i)) & 0x7f) as u8));
    out.extend(frame);
    for _ in 0..8 {
        let mut f = vec![0u8; 417];
        f[..4].copy_from_slice(&[0xff, 0xfb, 0x90, 0x00]);
        out.extend(f);
    }
    out
}

fn list(id: u32, parent_id: Option<u32>, name: &str, track_ids: Vec<u32>) -> Playlist {
    let is_folder = track_ids.is_empty();
    Playlist { id, parent_id, name: name.into(), is_folder, sort_order: id, track_ids }
}

/// Tracks 1 and 2 (two demo tracks), a FLAC track 3 and track [`PLUS`] only OneLibrary has;
/// `Shows/Friday` (2, 1), a top-level `All` (1, 2, 3), `Booth` (the OneLibrary track) and a
/// history list. Track 1 has a memory cue, a memory loop, a named hot cue A and a looping hot cue
/// E; track 2 a beat grid alone; the OneLibrary track a hot cue B. Traktor collections are
/// stamped at one time (`SOURCE_DATE_EPOCH`), so one planned, written and built here is the same
/// to the byte however the clock moves between.
fn drive() -> Drive {
    static STAMPED: std::sync::Once = std::sync::Once::new();
    // SAFETY: nextest runs each test in a process of its own, and this runs before its first
    // plugin thread starts.
    STAMPED.call_once(|| unsafe { std::env::set_var("SOURCE_DATE_EPOCH", "1790000000") });
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("USB");
    let paths = [
        (1, "Demo Track 1", "/Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3"),
        (2, "Demo Track 2", "/Contents/Loopmasters/UnknownAlbum/Demo Track 2.mp3"),
        (3, "Lossless", "/Contents/Other/lossless.flac"),
        (PLUS, "Only Plus", "/Contents/Plus/Only Plus.mp3"),
    ];
    let mut tracks = Vec::new();
    for (id, title, path) in paths {
        let audio = root.join(paths::key(path));
        std::fs::create_dir_all(audio.parent().unwrap()).unwrap();
        let bytes = if path.ends_with(".flac") { b"fLaC not really".to_vec() } else { mp3(title) };
        std::fs::write(audio, bytes).unwrap();
        tracks.push(Track {
            key: "Fm".into(),
            date_added: "2026-01-02".into(),
            ..track(id, "Loopmasters", title, path)
        });
    }
    let library = Library {
        tracks,
        playlists: vec![
            list(1, None, "Shows", vec![]),
            list(2, Some(1), "Friday", vec![2, 1]),
            list(3, None, "All", vec![1, 2, 3]),
            list(4, None, "Booth", vec![PLUS]),
        ],
        history_playlists: 1,
        history: vec![list(5, None, "HISTORY 001", vec![1])],
    };
    let grid = |bpm| (0..8).map(|i| Beat { beat_number: i % 4 + 1, bpm, time_ms: 25 + u32::from(i) * 469 }).collect();
    let cue = |hot_cue, start_ms, end_ms, comment: &str| Cue {
        hot_cue,
        start_ms,
        end_ms,
        comment: comment.into(),
        color: hot_cue.map(|_| [0xe6, 0x28, 0x28]),
        color_id: 0,
        loop_beats: None,
    };
    let cues = vec![
        Cue { color_id: 6, ..cue(None, 25, None, "Intro") },
        Cue { loop_beats: Some((8, 1)), ..cue(None, 30_025, Some(33_775), "") },
        cue(Some(0), 15_025, None, "Vocal ♪"),
        Cue { loop_beats: Some((1, 2)), ..cue(Some(4), 60_025, Some(60_259), "") },
    ];
    let analysis = FxHashMap::from_iter([
        (1, TrackAnalysis { beats: grid(128.0), cues, ..Default::default() }),
        (2, TrackAnalysis { beats: grid(124.0), ..Default::default() }),
        (
            PLUS,
            TrackAnalysis { beats: grid(126.0), cues: vec![cue(Some(1), 1_000, None, "Drop")], ..Default::default() },
        ),
    ]);
    Drive { dir, root, library, analysis, kept: FxHashMap::default() }
}

/// A read callback: its method and params.
type Call = (String, Value);

/// The plugin served on a thread, over pipes, past `hello`, with a host whose drive `USB` is
/// `drive` and that answers its reads as locallibrary does.
struct Session {
    host: Peer,
    served: JoinHandle<()>,
    answering: JoinHandle<()>,
    calls: Arc<Mutex<Vec<Call>>>,
    /// Where the drive is mounted, as the host says in each export's source.
    root: PathBuf,
}

fn start(drive: &Drive) -> Session {
    start_cancelling(drive, None)
}

/// [`start`], with a host that cancels the export once the plugin calls `cancel_at`.
fn start_cancelling(drive: &Drive, cancel_at: Option<&'static str>) -> Session {
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serve(plugin_in, plugin_out));
    let (host, incoming) = Peer::new(from_plugin, to_plugin);
    let calls: Arc<Mutex<Vec<Call>>> = Arc::default();
    let (to, log) = (host.clone(), calls.clone());
    let (library, analysis, kept) = (drive.library.clone(), drive.analysis.clone(), drive.kept.clone());
    let answering = std::thread::spawn(move || {
        for msg in incoming {
            let Incoming::Request(req) = msg else { continue };
            log.lock().unwrap().push((req.method.clone(), req.params.clone()));
            if cancel_at == Some(req.method.as_str()) {
                // The export: the host's second request, after `hello`.
                to.notify(method::CANCEL, &json!({ "id": 2 }));
            }
            to.respond(&req.id, read(&library, &analysis, &kept, &req.method, req.params));
        }
    });
    let hello = json!({ "protocol": PROTOCOL_VERSION, "host": { "name": "locallibrary", "version": "0.1.0" } });
    host.request::<_, Value>(method::HELLO, &hello).unwrap();
    Session { host, served, answering, calls, root: drive.root.clone() }
}

/// A read as the host answers a plugin: the reads the manifest's capabilities grant, only with
/// the params a plugin may give each, of the drive `USB`, `drive.library`'s tracks a page at a
/// time and at most 100 tracks' analysis at once; and the sets and mixes `kept`, by id.
fn read(
    lib: &Library,
    analysis: &FxHashMap<u32, TrackAnalysis>,
    kept: &FxHashMap<String, Value>,
    method: &str,
    params: Value,
) -> Result<Value, RpcError> {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    let (capability, allowed): (Capability, &[&str]) = match method {
        "drive.library" => (Capability::LibraryRead, &["drive", "combined", "offset", "limit"]),
        "tracks.analysis" => (Capability::AnalysisRead, &["drive", "combined", "ids", "waveform", "encoding"]),
        "sets.get" => (Capability::SetsRead, &["set", "venue"]),
        "mix.reports.get" => (Capability::SetsRead, &["id"]),
        _ => return Err(RpcError::failed("denied", format!("{method:?} isn't a read a plugin can call"))),
    };
    assert!(manifest.capabilities.contains(&capability), "{method} needs {capability}");
    let Value::Object(params) = params else { return Err(RpcError::invalid_params("expected an object")) };
    if let Some(name) = params.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(RpcError::invalid_params(format!("a plugin doesn't give {method} `{name}`")));
    }
    if capability == Capability::SetsRead {
        let id = params.get("set").or(params.get("id")).and_then(Value::as_str).unwrap_or_default();
        return kept.get(id).cloned().ok_or_else(|| RpcError::failed("not_found", format!("not found: {id:?}")));
    }
    if params["drive"] != "USB" {
        return Err(RpcError::failed("not_found", format!("not found: drive {}", params["drive"])));
    }
    match method {
        "drive.library" => {
            let offset = params.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
            let limit = params.get("limit").and_then(Value::as_u64).map_or(PAGE, |n| (n as usize).min(PAGE));
            let tracks: Vec<Track> = lib.tracks.iter().skip(offset).take(limit).cloned().collect();
            let page = json!({ "offset": offset, "count": tracks.len(), "total": lib.tracks.len() });
            Ok(json!({ "summary": lib.summary(), "library": Library { tracks, ..lib.clone() }, "page": page }))
        }
        _ => {
            let ids: Vec<u32> = serde_json::from_value(params["ids"].clone()).unwrap();
            if ids.len() > loclib_plugin::MAX_ANALYSES {
                return Err(RpcError::failed("invalid", format!("at most 100 tracks at a time, not {}", ids.len())));
            }
            let (found, missing): (Vec<u32>, Vec<u32>) = ids.into_iter().partition(|id| lib.track(*id).is_some());
            let tracks: Vec<Value> = found
                .into_iter()
                .map(|id| {
                    let a = analysis.get(&id).cloned().unwrap_or_default();
                    let view =
                        json!({ "beats": a.beats, "cues": a.cues, "preview": null, "detail": null, "structure": null });
                    json!({ "track": id, "analysis": view })
                })
                .collect();
            Ok(json!({ "tracks": tracks, "missing": missing }))
        }
    }
}

impl Session {
    fn export(&self, format: &str, output: &Path, options: Value, dry_run: bool) -> Result<Exported, RpcError> {
        self.export_of(format, json!({ "drive": "USB" }), output, options, dry_run)
    }

    /// An export of `source`, which the host tells where the drive is mounted.
    fn export_of(
        &self,
        format: &str,
        mut source: Value,
        output: &Path,
        options: Value,
        dry_run: bool,
    ) -> Result<Exported, RpcError> {
        source["root"] = json!(self.root);
        let write = json!({
            "exporter": format,
            "source": source,
            "output": output,
            "options": options,
            "dry_run": dry_run,
            "context": { "drive": "USB" },
        });
        self.host.request(method::WRITE_EXPORT, &write)
    }

    /// The reads made so far, by method.
    fn calls(&self, method: &str) -> Vec<Value> {
        self.calls.lock().unwrap().iter().filter(|(m, _)| m == method).map(|(_, p)| p.clone()).collect()
    }

    fn stop(self) {
        self.host.close();
        self.served.join().unwrap();
        self.answering.join().unwrap();
    }
}

/// What the core exports gave loclib-interop for `drive`: the library with OneLibrary's own
/// tracks numbered for files, then narrowed to `only`; the audio under `root`, symlinks resolved;
/// the analysis, unless skipped.
struct Expected {
    library: Library,
    analysis: FxHashMap<u32, TrackAnalysis>,
}

fn expected(drive: &Drive, only: &[&str]) -> Expected {
    let mut library = drive.library.clone();
    let old = library.number_for_files();
    let only: Vec<String> = only.iter().map(|s| s.to_string()).collect();
    let library = library.subset(&only).unwrap();
    let analysis = library
        .tracks
        .iter()
        .filter_map(|t| Some((t.id, drive.analysis.get(old.get(&t.id).unwrap_or(&t.id))?.clone())))
        .collect();
    Expected { library, analysis }
}

impl Expected {
    fn build(&self, root: &Path, skip_analysis: bool, write: impl FnOnce(&Source<'_>) -> Export) -> Export {
        let load = |t: &Track| self.analysis.get(&t.id).cloned().unwrap_or_default();
        let analysis: &dyn Fn(&Track) -> TrackAnalysis = &load;
        let source =
            Source { library: &self.library, root: resolved(root), analysis: (!skip_analysis).then_some(analysis) };
        write(&source)
    }
}

/// Each file below `dir` and its bytes, by path.
fn files(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for e in entries.map(Result::unwrap) {
        match e.file_type().unwrap().is_dir() {
            true => out.extend(files(&e.path())),
            false => out.push((e.path(), std::fs::read(e.path()).unwrap())),
        }
    }
    out.sort();
    out
}

/// Every row of an Engine Library's tables, but for what differs each time one is made: its
/// UUID and random indicator, and when its rows were edited.
fn engine_rows(db: &Path) -> Vec<Vec<(String, rusqlite::types::Value)>> {
    let db = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let varies = ["uuid", "currentPlayedIndiciator", "lastEditTime", "originDatabaseUuid", "databaseUuid"];
    let mut out = Vec::new();
    for table in ["Information", "Track", "Playlist", "PlaylistEntity", "AlbumArt", "PerformanceData"] {
        let mut stmt = db.prepare(&format!("SELECT * FROM {table} ORDER BY rowid")).unwrap();
        let names: Vec<String> = stmt.column_names().into_iter().map(String::from).collect();
        let rows = stmt
            .query_map([], |r| {
                Ok((0..names.len())
                    .filter(|i| !varies.contains(&names[*i].as_str()))
                    .map(|i| (names[i].clone(), r.get::<_, rusqlite::types::Value>(i).unwrap()))
                    .collect::<Vec<_>>())
            })
            .unwrap();
        out.extend(rows.map(Result::unwrap));
    }
    // Its `Information` row, at least: the tables were read.
    assert!(!out.is_empty(), "{out:?}");
    out
}

/// What `export` holds, file by file (sorted, as [`written`] lists them), as the plugin was to
/// write it to `output` (its paths below `out`, `output` with symlinks resolved): the bytes, and
/// an audio file's copy tagged as loclib-interop says, compared as [`readable`] says.
fn contents(export: &Export, out: &Path, output: &Path) -> Vec<(PathBuf, String)> {
    let dir = tempfile::tempdir().unwrap();
    let mut files: Vec<(PathBuf, String)> = export
        .files
        .iter()
        .map(|File { path, content, .. }| {
            let shown = output.join(path.strip_prefix(out).unwrap_or(path));
            let bytes = match content {
                Content::Bytes(bytes) => bytes.clone(),
                Content::TaggedCopy { from, markers } => {
                    let copy = dir.path().join("copy");
                    std::fs::copy(from, &copy).unwrap();
                    if let Some(m) = markers {
                        serato::write_markers(&copy, from, m).unwrap();
                    }
                    std::fs::read(copy).unwrap()
                }
            };
            (shown, readable(path, &bytes))
        })
        .collect();
    files.sort();
    files
}

/// A file's bytes as compared: NML as text, an Engine Library as its rows, anything else as it
/// is.
fn readable(path: &Path, bytes: &[u8]) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some("nml") => String::from_utf8(bytes.to_vec()).unwrap(),
        Some("db") => {
            let dir = tempfile::tempdir().unwrap();
            let db = dir.path().join("m.db");
            std::fs::write(&db, bytes).unwrap();
            format!("{:?}", engine_rows(&db))
        }
        _ => format!("{bytes:?}"),
    }
}

/// The files written below `output`, as [`contents`] compares them.
fn written(output: &Path) -> Vec<(PathBuf, String)> {
    files(output).into_iter().map(|(path, bytes)| (path.clone(), readable(&path, &bytes))).collect()
}

/// The files an export answers, by path.
fn paths(e: &Exported) -> Vec<PathBuf> {
    e.files.iter().map(|f| f.path.clone()).collect()
}

#[test]
fn the_manifest_is_valid_and_asks_for_what_the_exports_read() {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    manifest.validate(true).unwrap();
    let sources: Vec<(&str, ExportFrom)> =
        manifest.contributes.exporters.iter().map(|e| (e.format.as_str(), e.source)).collect();
    assert_eq!(
        sources,
        [
            ("traktor", ExportFrom::Drive),
            ("serato", ExportFrom::Drive),
            ("engine", ExportFrom::Drive),
            ("traktor-set", ExportFrom::Set),
            ("serato-set", ExportFrom::Set),
            ("engine-set", ExportFrom::Set),
            ("traktor-mix", ExportFrom::Mix),
            ("serato-mix", ExportFrom::Mix),
            ("engine-mix", ExportFrom::Mix),
        ]
    );
    assert!(manifest.contributes.exporters.iter().all(|e| e.folder));
    // A set's or a mix's exporter takes the options its drive's exporter does.
    for e in &manifest.contributes.exporters[3..] {
        let drives = manifest.contributes.exporters.iter().find(|d| e.format.starts_with(&format!("{}-", d.format)));
        assert_eq!(e.options, drives.unwrap().options, "{}", e.format);
    }
    // Where the drive is mounted comes with the export: no drive:info.
    let capabilities = [Capability::LibraryRead, Capability::AnalysisRead, Capability::SetsRead];
    assert_eq!(manifest.capabilities, capabilities);
}

/// The collection is the one loclib-interop writes from the library exports read (OneLibrary's
/// own track and its cues too), with every track's beat grid and cues; planned without writing
/// anything, written below the output only, and the drive is only read.
#[test]
fn a_traktor_collection_is_what_loclib_interop_writes() {
    let drive = drive();
    let before = files(&drive.root);
    let session = start(&drive);
    let output = drive.dir.path().join("for traktor");
    let planned = session.export("traktor", &output, json!({}), true).unwrap();
    assert_eq!(paths(&planned), [output.join("collection.nml")]);
    assert!(!output.exists(), "a dry run writes nothing");

    let done = session.export("traktor", &output, json!({}), false).unwrap();
    assert_eq!(done, planned);
    let want = expected(&drive, &[]);
    let export = want.build(&drive.root, false, |s| traktor::build(s, &output, &Ctx::silent()).unwrap());
    assert_eq!(written(&output), contents(&export, &output, &output));
    assert_eq!((export.tracks, export.playlists, export.folders, export.beat_grids, export.cues), (4, 3, 1, 3, 5));
    // What loclib-interop says of it: each file's contents and size, the counts, the notes and
    // (every track's audio being there) its warnings.
    let described: Vec<(&str, u64)> = done.files.iter().map(|f| (f.what.as_str(), f.bytes)).collect();
    assert_eq!(described, export.files.iter().map(|f| (f.what.as_str(), f.bytes)).collect::<Vec<_>>());
    let counts: Vec<(&str, u64)> = done.counts.iter().map(|(label, n)| (label.as_str(), *n)).collect();
    assert_eq!(counts, [("tracks", 4), ("playlists", 3), ("folders", 1), ("beat grids", 3), ("cues", 5)]);
    assert_eq!((&done.notes, &done.warnings), (&export.notes, &export.warnings));
    assert_eq!(files(&drive.root), before, "the drive is only read");

    let library = session.calls("drive.library");
    assert!(library.iter().all(|p| p["combined"] == true && p["drive"] == "USB"), "{library:?}");
    let analysis = session.calls("tracks.analysis");
    assert_eq!(analysis.len(), 2, "planned, then written");
    assert_eq!(analysis[0], json!({ "drive": "USB", "combined": true, "ids": [1, 2, 3, PLUS], "waveform": "none" }));
    session.stop();
}

/// Only the playlists chosen (and their folders and tracks), its tracks found where a copy of
/// the drive's `Contents/` will be, without analysis: none is read, and the tracks with no audio
/// there are named.
#[test]
fn only_some_playlists_elsewhere_without_analysis() {
    let drive = drive();
    let session = start(&drive);
    let output = drive.dir.path().join("out");
    let elsewhere = drive.dir.path().join("Music Copy");
    let options = json!({ "location_root": elsewhere, "skip_analysis": true });
    let source = json!({ "drive": "USB", "playlists": ["Shows", "Booth"] });
    let done = session.export_of("traktor", source, &output, options, false).unwrap();
    let export = expected(&drive, &["Shows", "Booth"])
        .build(&elsewhere, true, |s| traktor::build(s, &output, &Ctx::silent()).unwrap());
    assert_eq!((export.tracks, export.playlists, export.cues), (3, 2, 0));
    assert_eq!(written(&output), contents(&export, &output, &output));
    let missing = format!("3 track(s) have no audio under {}", resolved(&elsewhere).display());
    assert!(done.warnings.iter().any(|w| w.starts_with(&missing)), "{:?}", done.warnings);
    assert!(session.calls("tracks.analysis").is_empty());
    session.stop();
}

/// A crate for each playlist and folder, the ones loclib-interop writes: the crates list the
/// drive's audio, and the notes say how to get cue points across.
#[test]
fn serato_crates_are_what_loclib_interop_writes() {
    let drive = drive();
    let session = start(&drive);
    let output = drive.dir.path().join("for serato");
    let done = session.export("serato", &output, json!({}), false).unwrap();
    let export = expected(&drive, &[])
        .build(&drive.root, false, |s| serato::build::<Error>(s, &output, None, &Ctx::silent()).unwrap());
    assert_eq!(written(&output), contents(&export, &output, &output));
    let crates =
        ["Shows", "Shows%%Friday", "All", "Booth"].map(|n| output.join(format!("_Serato_/Subcrates/{n}.crate")));
    assert_eq!(paths(&done), crates);
    assert!(done.notes.iter().any(|n| n.contains("write_tags writes them into copies")), "{:?}", done.notes);
    assert!(session.calls("tracks.analysis").is_empty(), "crates alone carry no analysis");
    session.stop();
}

/// With `write_tags`, the audio is copied below the output, MP3 copies with Serato's cue tag
/// (as loclib-interop tags them), and the crates list the copies; the drive is only read, and a
/// location root, which the crates couldn't list, is refused.
#[test]
fn serato_tags_go_into_copies_only() {
    let drive = drive();
    let before = files(&drive.root);
    let session = start(&drive);
    let output = drive.dir.path().join("for serato");
    let tags = json!({ "write_tags": true });
    let planned = session.export("serato", &output, tags.clone(), true).unwrap();
    assert!(!output.exists(), "a dry run copies nothing");
    let done = session.export("serato", &output, tags, false).unwrap();
    assert_eq!(done, planned);
    let out = resolved(&output);
    let audio = |t: &Track| Ok::<_, Error>(paths::to_fs(&drive.root, &paths::key(&t.file_path)));
    let copies = serato::Copies { root: out.clone(), audio: &audio };
    let export = expected(&drive, &[])
        .build(&drive.root, false, |s| serato::build(s, &output, Some(&copies), &Ctx::silent()).unwrap());
    assert_eq!(written(&output), contents(&export, &out, &output));
    assert_eq!((export.cues, export.files.len()), (4, 8), "hot cue A, the two loops and the OneLibrary track's B");
    for (copy, original) in [
        ("Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3", mp3("Demo Track 1")),
        ("Contents/Other/lossless.flac", b"fLaC not really".to_vec()),
    ] {
        let copy = std::fs::read(output.join(copy)).unwrap();
        assert_eq!(copy.len() > original.len(), original.starts_with(b"ID3"), "only the MP3 is tagged");
    }
    let copy = done.files.iter().find(|f| f.path == output.join("Contents/Plus/Only Plus.mp3")).unwrap();
    assert_eq!(copy.what, "copy of the audio, with Serato's cue tag");
    assert!(
        done.warnings.iter().any(|w| w.contains("1 cue(s) left out")),
        "the memory cue has no place: {:?}",
        done.warnings
    );
    assert!(done.warnings.iter().any(|w| w.contains("1 track(s) aren't MP3, AIFF or WAV")), "{:?}", done.warnings);
    assert_eq!(files(&drive.root), before, "the drive is only read");

    let elsewhere = json!({ "write_tags": true, "location_root": drive.dir.path().join("x") });
    let refused = session.export("serato", &output, elsewhere, true).unwrap_err();
    assert_eq!((refused.code, refused.kind()), (code::INVALID_PARAMS, Some("invalid")));
    session.stop();
}

/// The Engine Library loclib-interop writes (but for its UUID and edit times), its tracks found
/// from where it's written, or from the drive's root; one there already goes with its WAL.
#[test]
fn an_engine_library_is_what_loclib_interop_writes() {
    let drive = drive();
    let session = start(&drive);
    let output = drive.dir.path().join("for engine");
    let done = session.export("engine", &output, json!({}), false).unwrap();
    let db = output.join("Engine Library/Database2/m.db");
    assert_eq!(paths(&done), std::slice::from_ref(&db));
    let out = resolved(&output);
    let want = expected(&drive, &[]);
    let export =
        want.build(&drive.root, false, |s| engine::build(s, &out, PathStyle::Relative, &Ctx::silent()).unwrap());
    assert_eq!(written(&output), contents(&export, &out, &output));
    assert!(done.notes[1].ends_with("or export with paths = drive to put it on the drive."), "{:?}", done.notes);
    assert!(session.calls("tracks.analysis").is_empty(), "Engine keeps no analysis");

    let wal = db.with_file_name("m.db-wal");
    std::fs::write(&wal, b"stale").unwrap();
    let on_drive = json!({ "paths": "drive" });
    assert_eq!(paths(&session.export("engine", &output, on_drive.clone(), true).unwrap()), std::slice::from_ref(&db));
    assert!(wal.exists(), "a dry run removes nothing");
    session.export("engine", &output, on_drive, false).unwrap();
    assert!(!wal.exists());
    let export = want.build(&drive.root, false, |s| engine::build(s, &out, PathStyle::Drive, &Ctx::silent()).unwrap());
    assert_eq!(written(&output), contents(&export, &out, &output));
    session.stop();
}

/// More tracks than a page holds are read a page at a time, and their analysis a hundred tracks
/// at a time.
#[test]
fn a_big_library_is_read_in_pages() {
    let mut drive = drive();
    let first = drive.library.tracks[0].clone();
    drive.library.tracks.extend((10..511).map(|id| Track {
        id,
        file_path: format!("/Contents/{id}.mp3"),
        ..first.clone()
    }));
    drive.library.playlists[2].track_ids.extend(10..511);
    let session = start(&drive);
    let output = drive.dir.path().join("out");
    session.export("traktor", &output, json!({}), false).unwrap();
    let pages: Vec<Value> = session.calls("drive.library").iter().map(|p| p["offset"].clone()).collect();
    assert_eq!(pages, [0, 500]);
    let sizes: Vec<usize> =
        session.calls("tracks.analysis").iter().map(|p| p["ids"].as_array().unwrap().len()).collect();
    assert_eq!(sizes, [100, 100, 100, 100, 100, 5]);
    let export =
        expected(&drive, &[]).build(&drive.root, false, |s| traktor::build(s, &output, &Ctx::silent()).unwrap());
    assert_eq!(export.tracks, 505);
    assert_eq!(written(&output), contents(&export, &output, &output));
    session.stop();
}

/// What the formats can't take is refused before anything is read: history lists, a location
/// root that isn't absolute, a playlist the drive doesn't have, and (by the exporters' schemas)
/// options of another format's.
#[test]
fn what_cant_be_exported_is_refused() {
    let drive = drive();
    let session = start(&drive);
    let output = drive.dir.path().join("out");
    let history = json!({ "drive": "USB", "history": ["HISTORY 001"] });
    let refusals = [
        session.export_of("traktor", history, &output, json!({}), true),
        session.export("engine", &output, json!({ "location_root": "Music" }), true),
        session.export_of("serato", json!({ "drive": "USB", "playlists": ["Sunday"] }), &output, json!({}), true),
    ];
    let messages: Vec<String> = refusals
        .into_iter()
        .map(|r| {
            let e = r.unwrap_err();
            assert_eq!((e.code, e.kind()), (code::INVALID_PARAMS, Some("invalid")), "{e:?}");
            e.message
        })
        .collect();
    assert_eq!(
        messages,
        [
            "Traktor doesn't take history lists",
            "the location root must be an absolute path, not Music",
            "not found: playlist \"Sunday\""
        ]
    );
    assert!(!output.exists());
    let gone = session.export_of("traktor", json!({ "drive": "Gone" }), &output, json!({}), true).unwrap_err();
    assert_eq!(gone.kind(), Some("not_found"), "the host's error comes back as it is");

    let manifest = Manifest::parse(MANIFEST).unwrap();
    let options =
        |format: &str| manifest.contributes.exporters.iter().find(|e| e.format == format).unwrap().options.clone();
    for (format, wrong) in [
        ("traktor", json!({ "write_tags": true })),
        ("traktor", json!({ "paths": "drive" })),
        ("serato", json!({ "paths": "drive" })),
        ("engine", json!({ "paths": "absolute" })),
        ("engine", json!({ "write_tags": true })),
    ] {
        assert!(check_params(options(format).as_ref(), &wrong).is_err(), "{format} {wrong}");
    }
    assert!(check_params(options("serato").as_ref(), &json!({ "write_tags": true, "skip_analysis": true })).is_ok());
    session.stop();
}

/// Cancelled while reading, it stops, writing nothing.
#[test]
fn a_cancelled_export_writes_nothing() {
    let drive = drive();
    let session = start_cancelling(&drive, Some("drive.library"));
    let output = drive.dir.path().join("out");
    let e = session.export("traktor", &output, json!({}), false).unwrap_err();
    assert_eq!(e.kind(), Some("cancelled"));
    assert!(!output.exists());
    session.stop();
}

/// A track whose audio is a link out of the drive isn't copied.
#[cfg(unix)]
#[test]
fn audio_outside_the_drive_isnt_copied() {
    let drive = drive();
    let audio = drive.root.join("Contents/Loopmasters/UnknownAlbum/Demo Track 2.mp3");
    let outside = drive.dir.path().join("secret.mp3");
    std::fs::rename(&audio, &outside).unwrap();
    std::os::unix::fs::symlink(&outside, &audio).unwrap();
    let session = start(&drive);
    let output = drive.dir.path().join("out");
    let e = session.export("serato", &output, json!({ "write_tags": true }), true).unwrap_err();
    assert_eq!(e.kind(), Some("invalid"));
    let file = "/Contents/Loopmasters/UnknownAlbum/Demo Track 2.mp3";
    assert_eq!(e.message, format!("refusing {file:?}: it resolves outside the drive"));
    session.stop();
}

#[test]
fn loclib_interops_errors_keep_their_kinds() {
    let io = Error::Io { path: "/out/a".into(), source: std::io::Error::other("full") };
    let ctx = Ctx::silent();
    let kinds =
        [failure(io, &ctx), failure(Error::Tags("no".into()), &ctx), failure(Error::Invalid("no".into()), &ctx)]
            .map(|e| (e.code, e.kind().map(String::from), e.message));
    assert_eq!(
        kinds,
        [
            (code::FAILED, Some("io".into()), "/out/a: full".into()),
            (code::FAILED, Some("tags".into()), "tags: no".into()),
            (code::FAILED, Some("invalid".into()), "no".into()),
        ]
    );
    let cancel = loclib_progress::CancelToken::new();
    cancel.cancel();
    let cancelled =
        failure(Error::Cancelled(loclib_progress::Cancelled), &Ctx::new(&loclib_progress::NoProgress, cancel));
    assert_eq!(cancelled, RpcError::cancelled());
}

/// For any of the three formats, any choice of playlists, any options its schema takes, the audio
/// of any tracks gone from the drive and (for Engine) an old library's WAL in the way: what
/// can't be exported is refused, saying why, before anything is written; otherwise the dry run
/// names exactly the files the export then writes (and writes nothing, removing nothing), and
/// what it writes is what loclib-interop builds for the same library, root and analysis, with
/// its counts and warnings, and one naming the tracks whose audio isn't where the files say.
#[hegel::test(test_cases = 60)]
fn any_export_writes_what_its_dry_run_names(tc: TestCase) {
    let mut drive = drive();
    let format = tc.draw(gs::sampled_from(vec!["traktor", "serato", "engine"]));
    for t in &drive.library.tracks {
        if tc.draw(gs::weighted_booleans(0.2)) {
            std::fs::remove_file(drive.root.join(paths::key(&t.file_path))).unwrap();
        }
    }
    // More tracks than a page of the library holds, or than a request for analysis takes, their
    // audio not on the drive.
    if tc.draw(gs::weighted_booleans(0.4)) {
        let first = drive.library.tracks[0].clone();
        let more = (10..511).map(|id| Track { id, file_path: format!("/Contents/{id}.mp3"), ..first.clone() });
        drive.library.tracks.extend(more);
        drive.library.playlists[2].track_ids.extend(10..511);
    }
    // The second track's audio a link out of the drive.
    let linked_track = "/Contents/Loopmasters/UnknownAlbum/Demo Track 2.mp3";
    let audio = drive.root.join(paths::key(linked_track));
    let linked = cfg!(unix) && audio.exists() && tc.draw(gs::weighted_booleans(0.15));
    if linked {
        let outside = drive.dir.path().join("secret.mp3");
        std::fs::rename(&audio, &outside).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &audio).unwrap();
    }
    // The host cancels the first export once the plugin reads this.
    let cancel_at = tc.draw(gs::sampled_from(vec![None, None, None, Some("drive.library"), Some("tracks.analysis")]));
    // The whole library, or some of its playlists.
    let mut lists = match tc.draw(gs::booleans()) {
        true => Vec::new(),
        false => tc.draw(
            gs::vecs(gs::sampled_from(vec!["Shows", "Booth", "All", "Shows/Friday"]))
                .unique(true)
                .min_size(1)
                .max_size(3),
        ),
    };
    // A playlist the drive hasn't, and a history list, which no format takes.
    if tc.draw(gs::weighted_booleans(0.1)) {
        lists.push("Sunday");
    }
    let history = tc.draw(gs::weighted_booleans(0.05));
    let mut source = json!({ "drive": "USB", "playlists": lists });
    if history {
        source["history"] = json!(["HISTORY 001"]);
    }
    let elsewhere = drive.dir.path().join("Music Copy");
    let mut options = json!({});
    let root = tc.draw(gs::sampled_from(vec![
        None,
        None,
        None,
        None,
        Some(elsewhere.to_string_lossy().into_owned()),
        Some("Music".into()),
    ]));
    if let Some(root) = &root {
        options["location_root"] = json!(root);
    }
    if format != "engine" && tc.draw(gs::weighted_booleans(0.4)) {
        options["skip_analysis"] = json!(tc.draw(gs::weighted_booleans(0.6)));
    }
    if format == "serato" && tc.draw(gs::weighted_booleans(0.7)) {
        options["write_tags"] = json!(tc.draw(gs::weighted_booleans(0.8)));
    }
    if format == "engine"
        && let Some(style) = tc.draw(gs::optional(gs::sampled_from(vec!["relative", "drive"])))
    {
        options["paths"] = json!(style);
    }
    let manifest = Manifest::parse(MANIFEST).unwrap();
    let schema = manifest.contributes.exporters.iter().find(|e| e.format == format).unwrap().options.clone();
    // The schema takes every option drawn, but a relative root, which the plugin refuses too.
    let relative = root.as_deref() == Some("Music");
    assert_eq!(check_params(schema.as_ref(), &options).is_ok(), !relative, "{options}");
    let output = drive.dir.path().join("out");
    let wal = output.join("Engine Library/Database2/m.db-wal");
    if format == "engine" && tc.draw(gs::booleans()) {
        std::fs::create_dir_all(wal.parent().unwrap()).unwrap();
        std::fs::write(&wal, b"stale").unwrap();
    }

    let session = start_cancelling(&drive, cancel_at);
    let before = files(&output);
    let planned = session.export_of(format, source.clone(), &output, options.clone(), true);
    assert_eq!(files(&output), before, "a dry run writes and removes nothing");
    let (skip, tags, on_drive) =
        (options["skip_analysis"] == true, options["write_tags"] == true, options["paths"] == "drive");
    let software: Software = serde_json::from_value(json!(format)).unwrap();
    // What stops the export, in the order it finds it: what it can't take, then (once it has
    // read the library) a playlist the drive hasn't, the cancel, and audio out of the drive. Each
    // is `invalid`, refused as invalid params but audio out of the drive, which loclib-interop
    // finds: an export that failed.
    let want = (!lists.contains(&"Sunday")).then(|| expected(&drive, &lists));
    let carried = match software {
        Software::Traktor => true,
        Software::Serato => tags,
        _ => false,
    };
    let analysed = carried && !skip && want.as_ref().is_some_and(|w| !w.library.tracks.is_empty());
    let has_linked = want.as_ref().is_some_and(|w| w.library.tracks.iter().any(|t| t.file_path == linked_track));
    let refused = |why: String| Some(Some((code::INVALID_PARAMS, why)));
    let stopped = |cancel: Option<&str>| match () {
        _ if history => refused(format!("{} doesn't take history lists", software.name())),
        _ if relative => refused("the location root must be an absolute path, not Music".to_string()),
        _ if tags && root.is_some() => {
            refused("a location root doesn't go with writing tags: the crates list the tagged copies".into())
        }
        _ if cancel == Some("drive.library") => Some(None),
        _ if want.is_none() => refused("not found: playlist \"Sunday\"".into()),
        _ if cancel == Some("tracks.analysis") && analysed => Some(None),
        _ if linked && tags && has_linked => {
            Some(Some((code::FAILED, format!("refusing {linked_track:?}: it resolves outside the drive"))))
        }
        _ => None,
    };
    // Cancelled, it stops, and the next export goes on as if it hadn't been asked.
    let planned = match stopped(cancel_at) {
        Some(None) => {
            assert_eq!(planned.unwrap_err().kind(), Some("cancelled"));
            assert_eq!(files(&output), before, "nothing is written");
            session.export_of(format, source.clone(), &output, options.clone(), true)
        }
        _ => planned,
    };
    if let Some(Some((said, why))) = stopped(None) {
        let e = planned.unwrap_err();
        assert_eq!((e.code, e.kind(), e.message.as_str()), (said, Some("invalid"), why.as_str()));
        let e = session.export_of(format, source, &output, options, false).unwrap_err();
        assert_eq!(e.message, why);
        assert_eq!(files(&output), before, "nothing is written");
        session.stop();
        return;
    }
    let planned = planned.unwrap();
    let done = session.export_of(format, source, &output, options, false).unwrap();
    // locallibrary gives at most 100 tracks' analysis at once.
    let asked: Vec<usize> =
        session.calls("tracks.analysis").iter().map(|p| p["ids"].as_array().unwrap().len()).collect();
    assert!(asked.iter().all(|&n| n <= 100), "{asked:?}");
    session.stop();
    assert_eq!(done, planned);
    let written_paths: Vec<PathBuf> = files(&output).into_iter().map(|(path, _)| path).collect();
    let mut named = paths(&done);
    named.sort();
    assert_eq!(written_paths, named, "the dry run named what's written, and only that");
    assert!(!wal.exists());

    let root = root.map_or(drive.root.clone(), PathBuf::from);
    is_built(&want.unwrap(), &Asked { software, root, skip, tags, on_drive }, &drive, &output, &done, Vec::new());
}

/// An export's format and options, as the plugin was asked for it: the folder its tracks' audio
/// will be in, and whether it skips analysis, writes Serato's tags or Engine's paths from the
/// drive's root.
struct Asked {
    software: Software,
    root: PathBuf,
    skip: bool,
    tags: bool,
    on_drive: bool,
}

/// What `done` wrote below `output` and said of it is what loclib-interop builds of `want` as
/// `asked` (Serato's copies made of `drive`'s audio): the files and their sizes, the counts, the
/// notes (naming the plugin's options), and the warnings: `left_out` first, then
/// loclib-interop's, then one naming the tracks whose audio isn't where the files say.
fn is_built(want: &Expected, asked: &Asked, drive: &Drive, output: &Path, done: &Exported, left_out: Vec<String>) {
    let Asked { software, ref root, skip, tags, on_drive } = *asked;
    let out = resolved(output);
    let export = want.build(root, skip, |s| match software {
        Software::Traktor => traktor::build(s, output, &Ctx::silent()).unwrap(),
        Software::Serato if tags => {
            let audio = |t: &Track| Ok::<_, Error>(paths::to_fs(&drive.root, &paths::key(&t.file_path)));
            let copies = serato::Copies { root: out.clone(), audio: &audio };
            serato::build(s, output, Some(&copies), &Ctx::silent()).unwrap()
        }
        Software::Serato => serato::build::<Error>(s, output, None, &Ctx::silent()).unwrap(),
        _ => {
            let style = if on_drive { PathStyle::Drive } else { PathStyle::Relative };
            engine::build(s, &out, style, &Ctx::silent()).unwrap()
        }
    });
    assert_eq!(written(output), contents(&export, &out, output));
    let described: Vec<(&str, u64)> = done.files.iter().map(|f| (f.what.as_str(), f.bytes)).collect();
    assert_eq!(described, export.files.iter().map(|f| (f.what.as_str(), f.bytes)).collect::<Vec<_>>());
    let counts: Vec<(&str, u64)> = done.counts.iter().map(|(label, n)| (label.as_str(), *n)).collect();
    let e = &export;
    let want_counts = [
        ("tracks", e.tracks),
        ("playlists", e.playlists),
        ("folders", e.folders),
        ("beat grids", e.beat_grids),
        ("cues", e.cues),
    ];
    assert_eq!(counts, want_counts.map(|(label, n)| (label, n as u64)));
    // The notes name the plugin's options, not loclib-interop's types.
    assert_eq!(done.notes.len(), export.notes.len());
    assert!(done.notes.iter().all(|n| !n.contains('`')), "{:?}", done.notes);
    let missing: Vec<&Track> =
        want.library.tracks.iter().filter(|t| !root.join(paths::key(&t.file_path)).is_file()).collect();
    let mut warnings = left_out;
    warnings.extend(export.warnings.iter().cloned());
    if let Some(first) = missing.first() {
        warnings.push(format!(
            "{} track(s) have no audio under {} (e.g. {}); they're listed anyway",
            missing.len(),
            resolved(root).display(),
            first.file_path
        ));
    }
    assert_eq!(done.warnings, warnings);
}

/// The part of a path that's there is resolved, symlinks too, and a `..` takes away the folder
/// the OS would be in. Regression: what wasn't there was appended as it was, `..` left in, so
/// `/tmp/new/../out` and `/private/tmp/out` didn't compare.
#[test]
fn the_part_that_exists_is_resolved() {
    let dir = TempDir::new().unwrap();
    let real = dunce::canonicalize(dir.path()).unwrap();
    assert_eq!(resolved(&dir.path().join("new/../out/x")), real.join("out/x"));
    #[cfg(unix)]
    {
        std::fs::create_dir_all(real.join("deep/er")).unwrap();
        std::os::unix::fs::symlink(real.join("deep/er"), real.join("up")).unwrap();
        assert_eq!(resolved(&real.join("up/../x")), real.join("deep/x"));
        assert_eq!(resolved(&real.join("new/../up/x")), real.join("deep/er/x"));
    }
}

/// The set and mix exporters.
mod sets;
