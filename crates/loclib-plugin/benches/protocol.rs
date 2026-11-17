//! What crossing the plugin boundary costs: a plugin served on a thread, talking JSON-RPC to the
//! host over pipes as a child process does over stdio. `describe` is the fixed cost of a call.
//! `track/analyze`, with an analyzer that answers at once, is what each track pays before any
//! analysis. `library/read` of a 1,000 and a 10,000 track library is what moving a whole library
//! costs: serialising, framing and parsing it (the hook answers a clone of a page built once).
//! `cargo bench -p loclib-plugin --bench protocol`.

use std::io::pipe;
use std::path::PathBuf;
use std::thread::JoinHandle;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use loclib_model::Library;
use loclib_model::test_util::{library, track};
use loclib_plugin::{
    Analysis, AnalyzeTrack, Context, FieldValue, Hello, HelloReply, Hooks, Host, HostInfo, LibraryPage,
    PROTOCOL_VERSION, Peer, ReadLibrary, RpcError, SourcePlaylist, SourceTrack, Track, method, serve,
};
use rustc_hash::FxHashMap;
use serde_json::{Value, json};

const MANIFEST: &str = r#"
id = "bench"
name = "Bench"
version = "0.1.0"
protocol = "^0.2"
capabilities = ["fs:read"]

[[contributes.analyzers]]
name = "tempo"
title = "Tempo and key"
version = "1"
fields = { bpm = "number", key = "string" }
roles = { bpm = "bpm", key = "key" }

[[contributes.sources]]
name = "library"
title = "A made-up library"
"#;

/// The folder the user granted: every track's audio file is under it.
const ROOT: &str = "/Users/dj/Music";

/// The host's end of a plugin served on a thread, over pipes, past `hello`.
struct Plugin {
    host: Peer,
    served: JoinHandle<()>,
}

impl Plugin {
    fn start(hooks: Hooks) -> Plugin {
        let (plugin_in, to_plugin) = pipe().unwrap();
        let (from_plugin, plugin_out) = pipe().unwrap();
        let served = std::thread::spawn(move || serve(MANIFEST, hooks, plugin_in, plugin_out));
        // These hooks never call the host back, so nothing comes in but responses.
        let (host, _) = Peer::new(from_plugin, to_plugin);
        let info = HostInfo { name: "locallibrary".into(), version: "0.1.0".into() };
        let hello = Hello { protocol: PROTOCOL_VERSION.into(), host: info, features: Vec::new() };
        host.request::<_, HelloReply>(method::HELLO, &hello).unwrap();
        Plugin { host, served }
    }

    /// Closes the plugin's input: it stops.
    fn stop(self) {
        self.host.close();
        self.served.join().unwrap();
    }
}

/// Answers a tempo and a key at once, reading no audio.
fn tempo(_: AnalyzeTrack, _: &Host) -> Result<Analysis, RpcError> {
    let fields = [("bpm".into(), FieldValue::Number(128.0)), ("key".into(), FieldValue::String("8A".into()))];
    Ok(Analysis { fields: fields.into_iter().collect() })
}

/// `lib` as a source answers it in one page: each track's audio file under `ROOT`, each playlist
/// by its folder and name.
fn page_of(lib: &Library) -> LibraryPage {
    let paths: FxHashMap<u32, PathBuf> =
        lib.tracks.iter().map(|t| (t.id, format!("{ROOT}{}", t.file_path).into())).collect();
    let tracks = lib.tracks.iter().map(|t| SourceTrack {
        path: paths[&t.id].clone(),
        title: t.title.clone(),
        artist: t.artist.clone(),
        album: t.album.clone(),
        genre: t.genre.clone(),
        isrc: t.isrc.clone(),
        year: t.year,
        rating: t.rating,
        key: Some(t.key.clone()),
        bpm: Some(t.bpm),
        ..SourceTrack::default()
    });
    let folders: FxHashMap<u32, &str> =
        lib.playlists.iter().filter(|p| p.is_folder).map(|p| (p.id, p.name.as_str())).collect();
    let playlists = lib.playlists.iter().filter(|p| !p.is_folder).map(|p| SourcePlaylist {
        path: match p.parent_id.and_then(|id| folders.get(&id)) {
            Some(folder) => format!("{folder}/{}", p.name),
            None => p.name.clone(),
        },
        tracks: p.track_ids.iter().map(|id| paths[id].clone()).collect(),
    });
    LibraryPage { tracks: tracks.collect(), playlists: playlists.collect(), ..LibraryPage::default() }
}

fn round_trip(c: &mut Criterion) {
    let plugin = Plugin::start(Hooks::new());
    let mut group = c.benchmark_group("round_trip");
    group.bench_function("describe", |b| {
        b.iter(|| plugin.host.request::<_, Value>(method::DESCRIBE, &json!({})).unwrap())
    });
    group.finish();
    plugin.stop();
}

fn track_analyze(c: &mut Criterion) {
    let plugin = Plugin::start(Hooks::new().on(tempo));
    let t = track(1, "Bicep", "Glue", "/Contents/Bicep/Bicep/04 Glue.flac");
    let audio_path = format!("{ROOT}{}", t.file_path).into();
    let analyze_path = Some(format!("{ROOT}/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT").into());
    let track = Track { id: t.id, file_path: t.file_path, audio_path, analyze_path };
    let req = AnalyzeTrack { analyzer: "tempo".into(), track, context: Context::default() };
    let mut group = c.benchmark_group("track_analyze");
    group.bench_function("bpm_and_key", |b| {
        b.iter(|| plugin.host.request::<_, Analysis>(method::ANALYZE_TRACK, &req).unwrap())
    });
    group.finish();
    plugin.stop();
}

fn library_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("library_read");
    for tracks in [1_000, 10_000] {
        let page = page_of(&library(tracks, tracks / 50));
        let plugin = Plugin::start(Hooks::new().on(move |_: ReadLibrary, _: &Host| Ok::<_, RpcError>(page.clone())));
        let req = ReadLibrary {
            source: "library".into(),
            options: Value::Null,
            roots: vec![ROOT.into()],
            cursor: None,
            context: Context::default(),
        };
        group.throughput(Throughput::Elements(tracks.into()));
        group.bench_with_input(BenchmarkId::from_parameter(tracks), &req, |b, req| {
            b.iter(|| plugin.host.request::<_, LibraryPage>(method::READ_LIBRARY, req).unwrap())
        });
        plugin.stop();
    }
    group.finish();
}

criterion_group!(benches, round_trip, track_analyze, library_read);
criterion_main!(benches);
