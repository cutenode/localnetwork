//! loclib-plugin's typed reads (`DriveReads`) against what the host answers a plugin: a plugin
//! served here reads a demo drive, each read going to the host as the test plugin's (the plugin
//! host's checks, then the sidecar's read method), and each answer read as loclib-plugin's type
//! for it.

use std::io::pipe;
use std::time::Duration;

use locallibrary::anlz::TrackAnalysis;
use locallibrary::model::{Library, Track};
use loclib_plugin::{
    DriveInfo, Hooks, Host, Incoming, PROTOCOL_VERSION, Peer, PlaylistTree, RpcError, RunCommand, TrackArtwork,
    TracksAnalysis, TracksById, TracksQuery, method, serve,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::support::{Env, Rpc, echo_plugin, path};

/// The plugin making the reads.
const READER: &str = r#"
    id = "reader"
    name = "Reader"
    version = "0.1.0"
    protocol = "^0.2"
    capabilities = ["library:read", "analysis:read", "artwork:read", "drive:info"]

    [[contributes.commands]]
    name = "read"
    title = "Read the drive"
    mode = "headless"
"#;

/// Every read the drive USB has, in batches of more than the host takes at once: the tracks
/// with ids 1 and 2 (and the ids up to 1001, which it hasn't), their analysis (up to 101) and
/// their artwork, 52 times.
fn read(_: RunCommand, host: &Host) -> Result<Value, RpcError> {
    let usb = host.drive("USB");
    let ids = |n: u32| (1..=n).collect::<Vec<u32>>();
    let query = TracksQuery { sort: Some("-title".into()), ..TracksQuery::default() };
    Ok(json!({
        "info": usb.info()?,
        "library": usb.library()?,
        "combined": usb.combined().library()?,
        "playlists": usb.playlists()?,
        "tracks": usb.tracks(&ids(1001))?,
        "query": usb.query(&query)?,
        "analysis": usb.analysis(1)?,
        "analyses": usb.analyses(&ids(101))?,
        "artwork": usb.artwork(&[1, 2].repeat(26))?,
    }))
}

/// Whether every field `ours` has is in `theirs`, as it is there.
fn within(ours: &Value, theirs: &Value) -> bool {
    match (ours, theirs) {
        (Value::Object(ours), Value::Object(theirs)) => {
            ours.iter().all(|(k, v)| theirs.get(k).is_some_and(|their| within(v, their)))
        }
        (Value::Array(ours), Value::Array(theirs)) => {
            ours.len() == theirs.len() && ours.iter().zip(theirs).all(|(v, their)| within(v, their))
        }
        _ => ours == theirs,
    }
}

/// `answer` read as `T`, which says no field otherwise than `answer` does.
fn reads_as<T: DeserializeOwned + Serialize>(answer: &Value) {
    let read: T = serde_json::from_value(answer.clone()).unwrap_or_else(|e| panic!("{e}: {answer}"));
    let said = serde_json::to_value(read).unwrap();
    assert!(within(&said, answer), "{said}\nisn't what the host said:\n{answer}");
}

/// The host's answer to `method` read as loclib-plugin's type for it (a page's library or tracks
/// as the pager reads them).
fn check(method: &str, answer: &Value) {
    match method {
        "drive.info" => reads_as::<DriveInfo>(answer),
        "drive.library" => reads_as::<Library>(&answer["library"]),
        "playlists.tree" => reads_as::<PlaylistTree>(answer),
        "tracks.get" => reads_as::<TracksById>(answer),
        "tracks.query" => reads_as::<Vec<Track>>(&answer["tracks"]),
        "track.analysis" => reads_as::<TrackAnalysis>(answer),
        "tracks.analysis" => reads_as::<TracksAnalysis>(answer),
        "art.get" => reads_as::<Vec<TrackArtwork>>(answer),
        other => panic!("{other} isn't a read of DriveReads"),
    }
}

#[test]
fn typed_reads_are_what_the_host_answers() {
    let mut env = Env::new();
    // Granted every read, to answer the plugin's.
    let (echo, granted) = (echo_plugin(), r#"capabilities = ["library:read", "library:plan""#);
    assert!(echo.contains(granted), "{echo}");
    env.plugins(&[&echo.replace(granted, &format!(r#"{granted}, "analysis:read", "artwork:read", "drive:info""#))]);
    let usb = env.demo("USB");
    image::RgbImage::new(300, 300).save(usb.join("Contents/Loopmasters/UnknownAlbum/cover.png")).unwrap();
    let set = env.ok(&["--yes", "art", "auto", "USB"]);
    assert!(set.contains("artwork set on 2 track(s)"), "{set}");
    let mut sidecar = Rpc::start(env.command());
    let edits = json!([{ "op": "create_playlist", "path": "Sets/Friday" }]);
    let made = sidecar.call(1, "edit.apply", json!({ "drive": path(&usb), "edits": edits }));
    assert!(made["error"].is_null(), "{made}");

    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serve(READER, Hooks::new().on(read), plugin_in, plugin_out));
    let (plugin, incoming) = Peer::new(from_plugin, to_plugin);
    let hello = json!({ "protocol": PROTOCOL_VERSION, "host": { "name": "test", "version": "0" } });
    plugin.request::<_, Value>(method::HELLO, &hello).unwrap();
    let reading = plugin.send_request(method::RUN_COMMAND, &json!({ "command": "read" })).unwrap();
    // Each of its reads, as the test plugin's.
    let mut asked = Vec::new();
    let read: Value = loop {
        let Ok(Incoming::Request(callback)) = incoming.recv_timeout(Duration::from_millis(50)) else {
            match reading.wait_timeout(Duration::ZERO) {
                Some(read) => break read.unwrap(),
                None => continue,
            }
        };
        let call = json!({ "call": callback.method, "params": callback.params });
        let run = json!({ "plugin": "echo", "command": "echo", "params": call });
        let answer = sidecar.call(2 + asked.len() as i64, "plugins.run", run);
        let answer = answer.get("result").unwrap_or_else(|| panic!("{answer}"));
        let Some(result) = answer.get("result") else { panic!("{} {}: {answer}", callback.method, callback.params) };
        check(&callback.method, result);
        asked.push(callback.method);
        plugin.respond(&callback.id, Ok(result.clone()));
    };
    plugin.close();
    served.join().unwrap();

    let batches = |method: &str| asked.iter().filter(|m| *m == method).count();
    let counts = ["drive.library", "tracks.get", "tracks.query", "tracks.analysis", "art.get"].map(batches);
    assert_eq!(counts, [2, 2, 1, 2, 2], "{asked:?}");
    let read: (DriveInfo, Library, PlaylistTree, TracksById, TracksAnalysis, Vec<TrackArtwork>) =
        serde_json::from_value(json!([
            read["info"],
            read["library"],
            read["playlists"],
            read["tracks"],
            read["analyses"],
            read["artwork"]
        ]))
        .unwrap();
    let (info, library, playlists, tracks, analyses, artwork) = read;
    assert_eq!((info.volume_name.as_str(), info.has_device_library), ("USB", true));
    assert_eq!(library.tracks.len(), 2);
    let sets = playlists.playlists.iter().find(|p| p.path == "Sets").unwrap_or_else(|| panic!("{playlists:?}"));
    assert_eq!((sets.folder, sets.children[0].path.as_str()), (true, "Sets/Friday"));
    assert_eq!((tracks.tracks.len(), tracks.missing.len()), (2, 999));
    assert_eq!((analyses.tracks.len(), analyses.missing.len()), (2, 99));
    assert!(analyses.tracks[0].analysis.beats.len() > 1, "{:?}", analyses.tracks[0]);
    assert_eq!(artwork.len(), 52);
    assert!(artwork.iter().all(|a| a.image.as_ref().is_some_and(|i| i.mime == "image/jpeg")), "{artwork:?}");
}
