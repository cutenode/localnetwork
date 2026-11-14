use std::io::pipe;
use std::sync::{Arc, Mutex};

use loclib_model::Playlist;
use loclib_model::analysis::Beat;
use serde_json::json;

use super::*;
use crate::{Hooks, Incoming, Peer, READS, RunCommand, method, serve};

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

/// Each typed read is one a plugin may call ([`READS`]), with only params the host takes from a
/// plugin: a host that refuses it would leave the read useless.
#[test]
fn each_typed_read_is_one_a_plugin_may_call() {
    let read = |host: &Host| {
        let drive = host.drive("USB").combined();
        let query =
            TracksQuery { filter: Some("bpm:120".into()), playlist: Some("Set".into()), sort: Some("bpm".into()) };
        let _ = (drive.info(), drive.library(), drive.playlists(), drive.tracks(&[1]), drive.query(&query));
        let _ = (drive.analysis(1), drive.analyses(&[1]), drive.artwork(&[1]));
        let (c, db) = (host.collection(), std::path::Path::new("/scratch/master.db"));
        let _ = (c.locations(), c.status(), c.copy(db, true), c.tracks(Some(db)));
        Ok(())
    };
    let (_, calls) = reading(read, |_, _| Err(RpcError::failed("gone", "the drive isn't here")));
    let methods: Vec<&str> = calls.iter().map(|(method, _)| method.as_str()).collect();
    let typed = [
        "drive.info",
        "drive.library",
        "playlists.tree",
        "tracks.get",
        "tracks.query",
        "track.analysis",
        "tracks.analysis",
        "art.get",
        "collection.locations",
        "collection.status",
        "collection.copy",
        "collection.tracks",
    ];
    assert_eq!(methods, typed);
    for (method, params) in &calls {
        let read = READS.iter().find(|r| r.method == method).unwrap_or_else(|| panic!("a plugin can't call {method}"));
        for name in params.as_object().unwrap().keys() {
            assert!(read.params().any(|(n, _)| n == name), "a plugin doesn't give {method} `{name}`");
        }
    }
}

/// The drive the host refuses every read of.
const REFUSED: &str = "refused";

/// Every read of a drive whatever its size, read whole or combined, paged as the host chooses
/// (or all at once), and asked about any number of tracks: each gets what the host has, in
/// order, asking with the drive (and `combined`, but for its info); and of a drive the host
/// refuses, each read fails with the host's error.
#[hegel::test(test_cases = 15)]
fn prop_every_read_gets_what_the_host_has_or_its_error(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let n = tc.draw(gs::integers::<u32>().max_value(9));
    let size = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let paged = tc.draw(gs::booleans());
    let combined = tc.draw(gs::booleans());
    let drive = tc.draw(gs::from_regex("[A-Z]{1,6}").fullmatch(true));
    let asked = tc.draw(gs::integers::<u32>().min_value(1).max_value(MAX_IDS as u32 + 2));
    let named = drive.clone();
    let refused = RpcError::failed("denied", "no");
    let answer_refused = refused.clone();
    let answer = move |method: &str, params: &Value| {
        if params["drive"] == REFUSED {
            return Err(answer_refused.clone());
        }
        let offset = params["offset"].as_u64().unwrap_or_default() as usize;
        let all: Vec<Track> = (1..=n).map(track).collect();
        let page: Vec<Track> = match paged {
            true => all.iter().skip(offset).take(size).cloned().collect(),
            false => all.clone(),
        };
        let ids = |key: &str| -> Vec<u32> { serde_json::from_value(params[key].clone()).unwrap() };
        Ok(match method {
            "drive.info" => {
                json!({ "volume_name": params["drive"], "has_device_library": true, "has_library_plus": false })
            }
            "drive.library" => {
                let library = Library { tracks: page, ..Library::default() };
                let page = paged.then(|| json!({ "offset": offset, "count": library.tracks.len(), "total": n }));
                json!({ "summary": library.summary(), "library": library, "page": page })
            }
            "tracks.query" => json!({ "total": n, "total_duration_secs": 0, "tracks": page }),
            "playlists.tree" => json!({ "playlists": [], "history": [] }),
            "tracks.get" => {
                let (found, missing): (Vec<u32>, Vec<u32>) = ids("ids").into_iter().partition(|id| *id <= n);
                json!({ "tracks": found.into_iter().map(track).collect::<Vec<_>>(), "missing": missing })
            }
            "track.analysis" => serde_json::to_value(TrackAnalysis::default()).unwrap(),
            "tracks.analysis" => {
                let tracks = ids("ids").into_iter().map(|id| AnalysedTrack { track: id, analysis: Default::default() });
                json!({ "tracks": tracks.collect::<Vec<_>>(), "missing": [] })
            }
            _ => json!(ids("tracks").into_iter().map(|id| TrackArtwork { track: id, image: None }).collect::<Vec<_>>()),
        })
    };
    let reads = move |host: &Host| {
        let mut all = Vec::new();
        for name in [drive.as_str(), REFUSED] {
            let mut drive = host.drive(name);
            if combined {
                drive = drive.combined();
            }
            let ids: Vec<u32> = (1..=asked).collect();
            let few = &ids[..ids.len().min(MAX_ANALYSES + 1)];
            let each = [
                drive.info().map(|i| json!(i.volume_name)),
                drive.library().map(|l| json!(l.tracks.iter().map(|t| t.id).collect::<Vec<_>>())),
                drive.playlists().map(|p| json!(p.playlists.len())),
                drive.tracks(&ids).map(|t| json!([t.tracks.len(), t.missing.len()])),
                drive.query(&TracksQuery::default()).map(|t| json!(t.iter().map(|t| t.id).collect::<Vec<_>>())),
                drive.analysis(1).map(|_| json!(1)),
                drive.analyses(few).map(|a| json!(a.tracks.len())),
                drive.artwork(few).map(|a| json!(a.len())),
            ];
            all.push(each.map(|r| r.map_err(|e| e.message)));
        }
        Ok(serde_json::to_value(all).unwrap())
    };
    let (read, calls) = reading(reads, answer);
    let [ours, theirs]: [[Result<Value, String>; 8]; 2] = serde_json::from_value(read.unwrap()).unwrap();
    let ids: Vec<u32> = (1..=n).collect();
    let few = asked.min(MAX_ANALYSES as u32 + 1);
    let found = asked.min(n);
    let expected = [
        json!(named),
        json!(ids),
        json!(0),
        json!([found, asked - found]),
        json!(ids),
        json!(1),
        json!(few),
        json!(few),
    ];
    assert_eq!(ours, expected.map(Ok));
    assert!(theirs.iter().all(|r| r.as_ref().unwrap_err() == &refused.message), "{theirs:?}");
    for (method, params) in &calls {
        let combines = method != "drive.info" && combined;
        assert_eq!(params.get("combined") == Some(&json!(true)), combines, "{method} {params}");
    }
}

/// A track row as `collection.tracks` answers it.
fn row(id: usize) -> CollectionTrack {
    CollectionTrack { id: id.to_string(), path: format!("/Music/{id}.mp3"), ..CollectionTrack::default() }
}

/// rekordbox's files: each read asks for what it reads (a copy into its file, the tracks of
/// the copy named or of rekordbox's own) and gets what the host has.
#[test]
fn each_collection_read_asks_for_what_it_reads() {
    let locations = CollectionLocations {
        data: Some("/Users/me/Library/Pioneer/rekordbox".into()),
        master_db: Some("/Users/me/Library/Pioneer/rekordbox/master.db".into()),
        master_db_exists: true,
        share: Some("/Users/me/Library/Pioneer/rekordbox/share".into()),
        settings: None,
    };
    let status = CollectionStatus { running: true, version: Some("7.2.14.0323".into()) };
    let copied = CollectionCopy { bytes: 8192, db_id: Some("1234".into()), db_version: None, credentials_removed: 0 };
    // Named by its artist and album, as a backup's music folders are; a host older than them
    // leaves them out (`row`), which reads as none.
    let named = CollectionTrack { artist: Some("Daft Punk".into()), album: Some("Discovery".into()), ..row(1) };
    let tracks = CollectionTracks { db_id: Some("1234".into()), total: 2, tracks: vec![named, row(2)] };
    let answers = json!({
        "collection.locations": locations,
        "collection.status": status,
        "collection.copy": copied,
        "collection.tracks": tracks,
    });
    let read = |host: &Host| {
        let c = host.collection();
        let into = std::path::Path::new("/scratch/master.db");
        Ok((c.locations()?, c.status()?, c.copy(into, true)?, c.tracks(Some(into))?, c.tracks(None)?))
    };
    let (read, calls) = reading(read, move |method, _| Ok(answers[method].clone()));
    type Read = (CollectionLocations, CollectionStatus, CollectionCopy, CollectionTracks, CollectionTracks);
    let read: Read = serde_json::from_value(read.unwrap()).unwrap();
    assert_eq!(read, (locations, status, copied, tracks.clone(), tracks));
    let asked = [
        ("collection.locations", json!({})),
        ("collection.status", json!({})),
        ("collection.copy", json!({ "into": "/scratch/master.db", "with_credentials": true })),
        ("collection.tracks", json!({ "db": "/scratch/master.db", "offset": 0 })),
        ("collection.tracks", json!({ "offset": 0 })),
    ]
    .map(|(m, p)| (m.to_string(), p));
    assert_eq!(calls, asked);
}

/// However many track rows there are and whatever page size the host chooses, the tracks read
/// every row once, in order, each page asked for from where the last ended; and they stop at an
/// empty page, though the total said more were coming (the library changed meanwhile).
#[hegel::test(test_cases = 30)]
fn prop_collection_tracks_read_every_row_once(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let n = tc.draw(gs::integers::<usize>().max_value(40));
    let size = tc.draw(gs::integers::<usize>().min_value(1).max_value(12));
    let said = n + tc.draw(gs::integers::<usize>().max_value(2));
    let answer = move |_: &str, params: &Value| {
        let offset = params["offset"].as_u64().unwrap() as usize;
        let tracks: Vec<CollectionTrack> = (offset..n.min(offset + size)).map(row).collect();
        Ok(json!(CollectionTracks { db_id: Some("7".into()), total: said, tracks }))
    };
    let (read, calls) = reading(|host| host.collection().tracks(None), answer);
    let read: CollectionTracks = serde_json::from_value(read.unwrap()).unwrap();
    assert_eq!(read.tracks, (0..n).map(row).collect::<Vec<_>>());
    assert_eq!((read.total, read.db_id.as_deref()), (n, Some("7")));
    let offsets: Vec<usize> = calls.iter().map(|(_, p)| p["offset"].as_u64().unwrap() as usize).collect();
    let mut expected: Vec<usize> = (0..n).step_by(size).collect();
    // The empty page that ends it, when the total said more or there were none.
    if said > n || n == 0 {
        expected.push(n);
    }
    assert_eq!(offsets, expected);
}
