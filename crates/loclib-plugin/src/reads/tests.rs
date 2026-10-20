use std::io::pipe;
use std::sync::{Arc, Mutex};

use loclib_model::Playlist;
use loclib_model::analysis::Beat;
use serde_json::json;

use super::*;
use crate::{Hooks, Incoming, Peer, RunCommand, method, serve};

/// A plugin with a command to read with.
const MANIFEST: &str = r#"
    id = "reader"
    name = "Reader"
    version = "0.1.0"
    protocol = "^0.2"
    capabilities = ["library:read"]

    [[contributes.commands]]
    name = "read"
    title = "Read"
    mode = "headless"
"#;

/// A callback: its method and params.
type Call = (String, Value);

/// Runs `read` in a command, the host answering each callback with
/// `answer`: what `read` made of them, and the callbacks, in order.
fn reading<T: Serialize>(
    read: impl Fn(&Host) -> Result<T, RpcError> + Send + Sync + 'static,
    answer: impl Fn(&str, &Value) -> Result<Value, RpcError> + Send + 'static,
) -> (Result<Value, RpcError>, Vec<Call>) {
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let hooks = Hooks::new().on(move |_: RunCommand, host: &Host| Ok(serde_json::to_value(read(host)?).unwrap()));
    let served = std::thread::spawn(move || serve(MANIFEST, hooks, plugin_in, plugin_out));
    let (host, incoming) = Peer::new(from_plugin, to_plugin);
    let hello = json!({ "protocol": crate::PROTOCOL_VERSION, "host": { "name": "test", "version": "0" } });
    host.request::<_, Value>(method::HELLO, &hello).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (to, log) = (host.clone(), calls.clone());
    let answering = std::thread::spawn(move || {
        for msg in incoming {
            let Incoming::Request(req) = msg else { continue };
            log.lock().unwrap().push((req.method.clone(), req.params.clone()));
            to.respond(&req.id, answer(&req.method, &req.params));
        }
    });
    let result = host.request(method::RUN_COMMAND, &json!({ "command": "read" }));
    host.close();
    served.join().unwrap();
    answering.join().unwrap();
    let calls = calls.lock().unwrap().clone();
    (result, calls)
}

fn track(id: u32) -> Track {
    Track { id, title: format!("Track {id}"), ..Track::default() }
}

/// The ids of the tracks in `v` (a list of them, or an object with `tracks`).
fn ids(v: &Value) -> Vec<u64> {
    let tracks = v.get("tracks").unwrap_or(v).as_array().unwrap();
    tracks.iter().map(|t| t["id"].as_u64().unwrap()).collect()
}

/// `offset`'s page (of two) of a library of five tracks, as `drive.library` answers it.
fn library_page(params: &Value) -> Value {
    let offset = params["offset"].as_u64().unwrap() as usize;
    let tracks: Vec<Track> = (1..=5).map(track).skip(offset).take(2).collect();
    let playlist =
        Playlist { id: 1, parent_id: None, name: "Set".into(), is_folder: false, sort_order: 0, track_ids: vec![1, 5] };
    let library = Library { tracks, playlists: vec![playlist], ..Library::default() };
    let page = json!({ "offset": offset, "count": library.tracks.len(), "total": 5 });
    json!({ "summary": library.summary(), "library": library, "page": page })
}

#[test]
fn the_library_is_read_a_page_at_a_time() {
    let (read, calls) = reading(|host| host.drive("USB").library(), |_, params| Ok(library_page(params)));
    let read = read.unwrap();
    assert_eq!(ids(&read), [1, 2, 3, 4, 5]);
    assert_eq!(read["playlists"][0]["name"], "Set", "the library's playlists come with its tracks");
    let pages: Vec<Call> =
        [0, 2, 4].map(|o| ("drive.library".to_string(), json!({ "drive": "USB", "offset": o }))).into();
    assert_eq!(calls, pages);

    // Combined; and from a host that gives every track at once, read once.
    let (read, calls) = reading(
        |host| host.drive("USB").combined().library(),
        |_, _| Ok(json!({ "summary": LibrarySummary::default(), "library": Library::default() })),
    );
    assert_eq!(ids(&read.unwrap()), Vec::<u64>::new());
    assert_eq!(calls, [("drive.library".to_string(), json!({ "drive": "USB", "combined": true, "offset": 0 }))]);
}

/// A query reads every page of what it matches, and stops at an empty one, though the total said
/// more were coming (the library changed).
#[test]
fn a_query_reads_every_page() {
    let query = TracksQuery { playlist: Some("Set".into()), sort: Some("-bpm".into()), ..TracksQuery::default() };
    let page = |params: &Value| {
        let offset = params["offset"].as_u64().unwrap() as u32;
        let tracks: Vec<Track> = (offset + 1..=offset + 2).filter(|id| *id <= 3).map(track).collect();
        Ok(json!({ "total": 9, "total_duration_secs": 0, "tracks": tracks }))
    };
    let (read, calls) = reading(move |host| host.drive("USB").query(&query), move |_, params| page(params));
    assert_eq!(ids(&read.unwrap()), [1, 2, 3]);
    let asked = |o: u32| {
        ("tracks.query".to_string(), json!({ "drive": "USB", "offset": o, "playlist": "Set", "sort": "-bpm" }))
    };
    assert_eq!(calls, [asked(0), asked(2), asked(3)]);

    let (read, calls) = reading(
        |host| host.drive("USB").query(&TracksQuery { filter: Some("bpm:120".into()), ..TracksQuery::default() }),
        |_, _| Ok(json!({ "total": 1, "total_duration_secs": 0, "tracks": [track(7)] })),
    );
    assert_eq!(ids(&read.unwrap()), [7], "the total is read");
    assert_eq!(calls, [("tracks.query".to_string(), json!({ "drive": "USB", "offset": 0, "filter": "bpm:120" }))]);
}

/// Tracks, analyses and artwork are asked for in batches the host takes, and come back as one
/// answer, in the order asked.
#[test]
fn tracks_analyses_and_artwork_come_in_batches() {
    let asked = |params: &Value, key: &str| -> Vec<u32> { serde_json::from_value(params[key].clone()).unwrap() };
    let answer = move |method: &str, params: &Value| {
        Ok(match method {
            "tracks.get" => {
                let (found, missing): (Vec<u32>, Vec<u32>) =
                    asked(params, "ids").into_iter().partition(|id| id % 2 == 0);
                json!({ "tracks": found.into_iter().map(track).collect::<Vec<_>>(), "missing": missing })
            }
            "tracks.analysis" => {
                let tracks = asked(params, "ids")
                    .into_iter()
                    .map(|id| AnalysedTrack { track: id, analysis: Default::default() });
                json!({ "tracks": tracks.collect::<Vec<_>>(), "missing": [] })
            }
            _ => {
                let image =
                    |id: u32| (id == 0).then(|| ArtworkImage { mime: "image/jpeg".into(), base64: "AA==".into() });
                let tracks = asked(params, "tracks").into_iter().map(|id| TrackArtwork { track: id, image: image(id) });
                json!(tracks.collect::<Vec<_>>())
            }
        })
    };
    let ids_to = |n: u32| (0..n).collect::<Vec<u32>>();
    let read = move |host: &Host| {
        let drive = host.drive("USB");
        let tracks = drive.tracks(&ids_to(2001))?;
        Ok((tracks, drive.analyses(&ids_to(201))?, drive.artwork(&ids_to(101))?))
    };
    let (read, calls) = reading(read, answer);
    let (tracks, analyses, artwork): (TracksById, TracksAnalysis, Vec<TrackArtwork>) =
        serde_json::from_value(read.unwrap()).unwrap();
    let (even, odd): (Vec<u32>, Vec<u32>) = ids_to(2001).into_iter().partition(|id| id % 2 == 0);
    assert_eq!(tracks.tracks.iter().map(|t| t.id).collect::<Vec<_>>(), even);
    assert_eq!(tracks.missing, odd);
    assert_eq!(analyses.tracks.iter().map(|a| a.track).collect::<Vec<_>>(), ids_to(201));
    assert_eq!(artwork.iter().map(|a| a.track).collect::<Vec<_>>(), ids_to(101));
    assert_eq!(artwork[0].image.as_ref().unwrap().base64, "AA==");
    let batches: Vec<(&str, usize)> = calls
        .iter()
        .map(|(m, p)| (m.as_str(), p.get("ids").or(p.get("tracks")).unwrap().as_array().unwrap().len()))
        .collect();
    let expected = [
        ("tracks.get", 1000),
        ("tracks.get", 1000),
        ("tracks.get", 1),
        ("tracks.analysis", 100),
        ("tracks.analysis", 100),
        ("tracks.analysis", 1),
        ("art.get", 50),
        ("art.get", 50),
        ("art.get", 1),
    ];
    assert_eq!(batches, expected);
    assert_eq!(calls[3].1["waveform"], "none", "several tracks' analyses come without waveforms");
    assert!(calls.iter().all(|(_, p)| p["drive"] == "USB" && p.get("combined").is_none()), "{calls:?}");
}

/// What each read asks the host for, with `combined` only for the library's reads.
#[test]
fn each_read_asks_for_what_it_reads() {
    let info = DriveInfo {
        volume_name: "USB".into(),
        id: None,
        name: Some("Friday".into()),
        library_id: None,
        has_device_library: true,
        has_library_plus: false,
        exported_at: None,
        summary: None,
        total_space: Some(64),
        free_space: Some(8),
    };
    let node = PlaylistNode {
        id: 1,
        name: "Sets".into(),
        path: "Sets".into(),
        folder: true,
        tracks: 0,
        duration_secs: 0,
        children: Vec::new(),
    };
    let tree = PlaylistTree { playlists: vec![node], history: Vec::new() };
    let beats = vec![Beat { beat_number: 1, bpm: 128.0, time_ms: 0 }];
    let analysis = TrackAnalysis { beats, ..TrackAnalysis::default() };
    let answers = json!({ "drive.info": info, "playlists.tree": tree, "track.analysis": analysis });
    let read = |host: &Host| {
        let drive = host.drive("USB").combined();
        Ok((drive.info()?, drive.playlists()?, drive.analysis(3)?))
    };
    let (read, calls) = reading(read, move |method, _| Ok(answers[method].clone()));
    assert_eq!(
        serde_json::from_value::<(DriveInfo, PlaylistTree, TrackAnalysis)>(read.unwrap()).unwrap(),
        (info, tree, analysis)
    );
    let asked = [
        ("drive.info", json!({ "drive": "USB" })),
        ("playlists.tree", json!({ "drive": "USB", "combined": true })),
        ("track.analysis", json!({ "drive": "USB", "combined": true, "track": 3, "waveform": "none" })),
    ]
    .map(|(m, p)| (m.to_string(), p));
    assert_eq!(calls, asked);
}

/// The host's error comes back as it is, and stops the read.
#[test]
fn a_reads_error_comes_back_as_it_is() {
    let refused = RpcError::failed("denied", "art.get needs the artwork:read capability");
    let answer = refused.clone();
    let (read, calls) = reading(|host| host.drive("USB").artwork(&[1; 60]), move |_, _| Err(answer.clone()));
    assert_eq!(read.unwrap_err(), refused);
    assert_eq!(calls.len(), 1);
}
