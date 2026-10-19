use std::io::{BufRead, BufReader, PipeReader, PipeWriter, pipe};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hegel::TestCase;
use hegel::generators as gs;
use image::codecs::jpeg::JpegEncoder;
use image::{ImageFormat, RgbImage};
use loclib_model::test_util::track;
use loclib_plugin::{Incoming, PROTOCOL_VERSION, Peer, method};

use super::*;

/// MusicBrainz's answers, as it gave them (trimmed to what a search result has): a release
/// search for Bicep's album "Bicep", and a recording search for the ISRC of Young Fathers' "In My
/// View", which is on its single, the album "Cocoa Sugar" and two compilations.
const RELEASES: &str = include_str!("../fixtures/release-search.json");
const RECORDINGS: &str = include_str!("../fixtures/recording-search.json");
/// The release groups of those.
const BICEP: &str = "6f34e134-49d5-4e56-b3e0-4fe6b9900380";
const COCOA_SUGAR: &str = "95aea47d-79b6-483a-8abe-fccfdfa23eae";
const IN_MY_VIEW: &str = "41c11242-e68c-49bb-ba10-e7905ec25ecd";
const ISRC: &str = "GBCFB1700586";

/// How many tracks the fake host gives in a page of `tracks.query` (locallibrary gives 500): a
/// few, so paging shows.
const PAGE: usize = 2;

/// Time that passes only when the plugin sleeps, or (`frozen`) never: then a sleep waits a moment
/// for real, so a request waiting its turn waits until it's cancelled.
struct FakeClock {
    start: Instant,
    passed: Mutex<Duration>,
    frozen: bool,
}

impl FakeClock {
    fn new(frozen: bool) -> Arc<FakeClock> {
        Arc::new(FakeClock { start: Instant::now(), passed: Mutex::default(), frozen })
    }

    fn passed(&self) -> Duration {
        *self.passed.lock().unwrap()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        self.start + self.passed()
    }

    fn sleep(&self, d: Duration) {
        match self.frozen {
            true => std::thread::sleep(Duration::from_millis(1)),
            false => *self.passed.lock().unwrap() += d,
        }
    }
}

/// A request the fake server got.
struct Req {
    /// The server's own URL, for redirects.
    base: String,
    path: String,
    /// Its `query` param, decoded.
    query: String,
}

struct Reply {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

fn ok(body: impl Into<Vec<u8>>) -> Reply {
    Reply { status: 200, headers: Vec::new(), body: body.into() }
}

fn status(status: u16) -> Reply {
    Reply { status, headers: Vec::new(), body: b"<html>no</html>".to_vec() }
}

fn redirect(status: u16, to: String) -> Reply {
    Reply { status, headers: vec![("Location", to)], body: Vec::new() }
}

/// No answer: the connection is closed once the request is read, as a server that fell over (or
/// a request that timed out) leaves it.
fn cut() -> Reply {
    Reply { status: 0, headers: Vec::new(), body: Vec::new() }
}

/// What the fake server was asked: the path, the `query` param, the User-Agent, and how much fake
/// time had passed.
#[derive(Debug, Clone)]
struct Got {
    path: String,
    query: String,
    agent: String,
    at: Duration,
}

/// An HTTP server on this computer standing in for MusicBrainz and the Cover Art Archive (and
/// archive.org), answering each request as `route` says and keeping what it was asked. It says
/// on `heard` each time it's asked.
struct Server {
    url: String,
    got: Arc<Mutex<Vec<Got>>>,
    heard: Receiver<()>,
}

fn server(clock: Arc<FakeClock>, route: impl Fn(&Req) -> Reply + Send + 'static) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let got: Arc<Mutex<Vec<Got>>> = Arc::default();
    let (tell, heard) = channel();
    let (base, log) = (url.clone(), got.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            answer(stream.unwrap(), &base, &route, &log, &clock, &tell);
        }
    });
    Server { url, got, heard }
}

fn answer(
    mut stream: TcpStream,
    base: &str,
    route: &impl Fn(&Req) -> Reply,
    log: &Mutex<Vec<Got>>,
    clock: &FakeClock,
    tell: &Sender<()>,
) {
    let mut lines = BufReader::new(stream.try_clone().unwrap()).lines().map(Result::unwrap);
    let target = lines.next().unwrap().split(' ').nth(1).unwrap().to_string();
    let headers: Vec<String> = lines.take_while(|l| !l.is_empty()).collect();
    let agent = headers.iter().find_map(|h| h.strip_prefix("user-agent: ")).unwrap_or_default().to_string();
    let url = reqwest::Url::parse(&format!("{base}{target}")).unwrap();
    let query = url.query_pairs().find(|(k, _)| k == "query").map(|(_, v)| v.into_owned()).unwrap_or_default();
    let req = Req { base: base.into(), path: url.path().into(), query };
    log.lock().unwrap().push(Got { path: req.path.clone(), query: req.query.clone(), agent, at: clock.passed() });
    let _ = tell.send(());
    let Reply { status, headers, body } = route(&req);
    if status == 0 {
        return;
    }
    let mut head = format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(&body));
}

impl Server {
    fn got(&self) -> Vec<Got> {
        self.got.lock().unwrap().clone()
    }

    /// The searches MusicBrainz was asked: each one's path and query.
    fn searches(&self) -> Vec<(String, String)> {
        let got = self.got().into_iter().filter(|g| g.path.starts_with("/ws/2/"));
        got.map(|g| (g.path, g.query)).collect()
    }

    /// The covers the Cover Art Archive was asked for.
    fn covers(&self) -> Vec<String> {
        self.got().into_iter().filter(|g| g.path.starts_with("/release-group/")).map(|g| g.path).collect()
    }
}

/// A JPEG of `width` x `height`, a gradient, or noise.
fn jpeg(width: u32, height: u32, noise: bool) -> Vec<u8> {
    let image = RgbImage::from_fn(width, height, |x, y| match noise {
        true => {
            let n = (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)).wrapping_mul(2_246_822_519);
            image::Rgb([(n >> 8) as u8, (n >> 16) as u8, (n >> 24) as u8])
        }
        false => image::Rgb([(x % 256) as u8, (y % 256) as u8, 128]),
    });
    let mut out = Vec::new();
    image.write_with_encoder(JpegEncoder::new_with_quality(&mut out, 90)).unwrap();
    out
}

fn no_releases() -> Reply {
    ok(r#"{"created":"2026-10-13T20:42:22.360Z","count":0,"offset":0,"releases":[]}"#)
}

/// MusicBrainz, which has Bicep's "Bicep" (searched by name) and "In My View" (by its ISRC), and
/// the Cover Art Archive, which has a cover for each release group, redirecting to archive.org,
/// which redirects to the server holding it, as they do.
fn services_as_they_are(req: &Req) -> Reply {
    let base = &req.base;
    match req.path.as_str() {
        "/ws/2/release/" if req.query.starts_with(r#"release:"Bicep""#) => ok(RELEASES),
        "/ws/2/release/" => no_releases(),
        "/ws/2/recording/" if req.query == format!("isrc:{ISRC}") => ok(RECORDINGS),
        "/ws/2/recording/" => ok(r#"{"created":"2026-10-13T20:42:23.943Z","count":0,"offset":0,"recordings":[]}"#),
        path => match path.strip_prefix("/release-group/").and_then(|p| p.strip_suffix("/front-500")) {
            Some(id) => redirect(307, format!("{base}/download/mbid-{id}/mbid-{id}-1_thumb500.jpg")),
            None if path.starts_with("/download/") => redirect(302, format!("{base}/0/items{path}")),
            None if path.starts_with("/0/items/") => ok(jpeg(500, 400, false)),
            None => status(404),
        },
    }
}

/// A fake host's library: its tracks and playlists (by name, with their tracks' ids).
#[derive(Clone, Default)]
struct Drive {
    tracks: Vec<Track>,
    playlists: Vec<(String, Vec<u32>)>,
}

/// The plugin served on a thread over pipes, past `hello`, with a host whose drive `USB` is
/// `drive` and that answers its reads as locallibrary does, and services standing in for
/// MusicBrainz's and the Cover Art Archive.
struct Session {
    host: Peer,
    served: JoinHandle<()>,
    answering: JoinHandle<()>,
    /// The reads the plugin called back for: each one's method and params.
    calls: Arc<Mutex<Vec<(String, Value)>>>,
}

fn start(drive: Drive, services: Services, clock: Arc<dyn Clock>) -> Session {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    let plugin = Coverart::new(&manifest, services, clock);
    start_with(drive, move |input, output| loclib_plugin::serve(&manifest, plugin, input, output))
}

/// [`start`], the plugin served by `serving` (as [`serve`] serves it, say).
fn start_with(drive: Drive, serving: impl FnOnce(PipeReader, PipeWriter) + Send + 'static) -> Session {
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serving(plugin_in, plugin_out));
    let (host, incoming) = Peer::new(from_plugin, to_plugin);
    let calls: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
    let (to, log) = (host.clone(), calls.clone());
    let answering = std::thread::spawn(move || {
        for msg in incoming {
            let Incoming::Request(req) = msg else { continue };
            log.lock().unwrap().push((req.method.clone(), req.params.clone()));
            to.respond(&req.id, read(&drive, &req.method, req.params));
        }
    });
    let hello = json!({ "protocol": PROTOCOL_VERSION, "host": { "name": "locallibrary", "version": "0.1.0" } });
    host.request::<_, Value>(method::HELLO, &hello).unwrap();
    Session { host, served, answering, calls }
}

/// A read as the host answers a plugin with `library:read`: only the params it may give, on a
/// drive it knows.
fn read(drive: &Drive, method: &str, params: Value) -> Result<Value, RpcError> {
    let allowed: &[&str] = match method {
        "tracks.query" => &["drive", "combined", "filter", "playlist", "sort", "offset", "limit"],
        "tracks.get" => &["drive", "combined", "ids"],
        _ => return Err(RpcError::failed("denied", format!("{method} needs a capability the plugin doesn't have"))),
    };
    let Value::Object(params) = params else { return Err(RpcError::invalid_params("expected an object")) };
    if let Some(name) = params.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(RpcError::invalid_params(format!("a plugin doesn't give {method} `{name}`")));
    }
    if params["drive"] != "USB" {
        return Err(RpcError::failed("not_found", format!("not found: drive {}", params["drive"])));
    }
    let by_id = |id: u64| drive.tracks.iter().find(|t| u64::from(t.id) == id).cloned();
    if method == "tracks.get" {
        let ids: Vec<u64> = params["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
        let missing: Vec<u64> = ids.iter().copied().filter(|&id| by_id(id).is_none()).collect();
        let tracks: Vec<Track> = ids.into_iter().filter_map(by_id).collect();
        return Ok(json!({ "missing": missing, "tracks": tracks }));
    }
    let tracks: Vec<Track> = match params.get("playlist").and_then(Value::as_str) {
        Some(name) => {
            let (_, ids) = drive
                .playlists
                .iter()
                .find(|(n, _)| n == name)
                .ok_or_else(|| RpcError::failed("not_found", format!("not found: playlist {name:?}")))?;
            ids.iter().filter_map(|&id| by_id(id.into())).collect()
        }
        None => drive.tracks.clone(),
    };
    let offset = params.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let page: Vec<&Track> = tracks.iter().skip(offset).take(PAGE).collect();
    Ok(json!({ "total": tracks.len(), "total_duration_secs": 0, "tracks": page }))
}

impl Session {
    fn send(&self, params: Value, context: Value) -> loclib_plugin::Pending {
        let op = json!({ "operation": "fetch", "params": params, "context": context });
        self.host.send_request(method::PLAN_OP, &op).unwrap()
    }

    fn plan_with(&self, params: Value, context: Value) -> Result<OpPlan, RpcError> {
        self.send(params, context).wait()
    }

    /// `fetch` with `params` on the drive the user is on.
    fn plan(&self, params: Value) -> Result<OpPlan, RpcError> {
        self.plan_with(params, json!({ "drive": "USB" }))
    }

    /// Closes the plugin's input: it stops. Answers the reads it called back for.
    fn stop(self) -> Vec<(String, Value)> {
        self.host.close();
        self.served.join().unwrap();
        self.answering.join().unwrap();
        Arc::try_unwrap(self.calls).unwrap().into_inner().unwrap()
    }
}

fn services(server: &Server) -> Services {
    Services { musicbrainz: server.url.clone(), coverartarchive: server.url.clone() }
}

/// The plugin, with a host whose drive is `drive` and the services as `route` has them.
fn run(drive: Drive, route: impl Fn(&Req) -> Reply + Send + 'static) -> (Session, Server) {
    let clock = FakeClock::new(false);
    let server = server(clock.clone(), route);
    (start(drive, services(&server), clock), server)
}

/// A track of `album` (by `artist`), at `/Contents/<artist>/<title>.mp3`.
fn tagged(id: u32, artist: &str, album: &str, title: &str) -> Track {
    Track { album: album.into(), ..track(id, artist, title, &format!("/Contents/{artist}/{title}.mp3")) }
}

fn with_art(t: Track) -> Track {
    let art = format!("/PIONEER/Artwork/00001/a{}.jpg", t.id);
    Track { artwork_id: Some(t.id), artwork_path: Some(art), ..t }
}

fn drive(tracks: Vec<Track>) -> Drive {
    Drive { tracks, playlists: Vec::new() }
}

/// Each edit's tracks and its image's size, checking it's a `set_artwork` edit as locallibrary's
/// `edit.plan` takes one, giving the tracks by file and the image's bytes, a JPEG.
fn set(plan: &OpPlan) -> Vec<(Vec<String>, (u32, u32))> {
    let edit = |e: &Value| {
        // In whichever order serde_json keeps them: insertion order once another crate in the
        // build turns on its preserve_order, as locallibrary does.
        let mut keys: Vec<&str> = e.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["image", "op", "tracks"], "{e}");
        assert_eq!(e["op"], "set_artwork");
        assert_eq!(e["tracks"].as_object().unwrap().len(), 1, "only paths: {e}");
        assert_eq!(e["image"].as_object().unwrap().len(), 1, "only base64: {e}");
        let paths = e["tracks"]["paths"].as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect();
        let bytes = BASE64_STANDARD.decode(e["image"]["base64"].as_str().unwrap()).unwrap();
        assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::Jpeg);
        let image = image::load_from_memory(&bytes).unwrap();
        (paths, (image.width(), image.height()))
    };
    plan.edits.iter().map(edit).collect()
}

fn kind(e: &RpcError) -> (&str, &str) {
    (e.kind().unwrap_or_default(), e.message.as_str())
}

const AGENT: &str = "locallibrary-plugin-coverart/0.1.0 ( https://localnetwork.computer )";

/// It reads the drive's tracks and plans artwork, and says which servers it talks to.
#[test]
fn the_manifest_is_a_first_party_one_that_reads_and_plans_artwork() {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    manifest.validate(true).unwrap();
    let capabilities: Vec<String> = manifest.capabilities.iter().map(ToString::to_string).collect();
    assert_eq!(
        capabilities,
        ["library:read", "artwork:plan", "net:musicbrainz.org", "net:coverartarchive.org", "net:archive.org"]
    );
    let ops: Vec<&str> = manifest.contributes.operations.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(ops, ["fetch"]);

    // Served as a real one is: it says it's this manifest.
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serve(plugin_in, plugin_out));
    let (host, _incoming) = Peer::new(from_plugin, to_plugin);
    let hello = json!({ "protocol": PROTOCOL_VERSION, "host": { "name": "locallibrary", "version": "0.1.0" } });
    host.request::<_, Value>(method::HELLO, &hello).unwrap();
    let described: Manifest = host.request(method::DESCRIBE, &json!({})).unwrap();
    assert_eq!(described, manifest);
    host.close();
    served.join().unwrap();
}

/// The albums whose tracks have no artwork are each looked up once (by an ISRC when a track has
/// one), and their covers set on those tracks, at the size players show.
#[test]
fn albums_without_artwork_get_their_covers_one_search_each() {
    let tracks = vec![
        tagged(1, "Bicep", "Bicep", "Glue"),
        with_art(tagged(2, "Bicep", "Bicep", "Aura")),
        Track { isrc: "GB-CFB-17-00586".into(), ..tagged(3, "Young Fathers", "Cocoa Sugar", "In My View") },
        tagged(4, "Young Fathers", "Cocoa Sugar", "Lord"),
        // Letter case and spacing don't make another album.
        tagged(5, "BICEP", " bicep", "Opal"),
    ];
    let (session, server) = run(drive(tracks), services_as_they_are);
    let plan = session.plan(Value::Null).unwrap();
    assert_eq!(plan.summary, "set cover art from the Cover Art Archive on 4 track(s) of 2 album(s)");
    assert_eq!(plan.warnings, Vec::<String>::new());
    assert_eq!(
        set(&plan),
        [
            (vec!["/Contents/Bicep/Glue.mp3".into(), "/Contents/BICEP/Opal.mp3".into()], (240, 240)),
            (
                vec!["/Contents/Young Fathers/In My View.mp3".into(), "/Contents/Young Fathers/Lord.mp3".into()],
                (240, 240)
            ),
        ]
    );
    assert_eq!(
        server.searches(),
        [
            ("/ws/2/release/".into(), r#"release:"Bicep" AND artist:"Bicep""#.into()),
            ("/ws/2/recording/".into(), format!("isrc:{ISRC}")),
        ]
    );
    assert_eq!(
        server.covers(),
        [format!("/release-group/{BICEP}/front-500"), format!("/release-group/{COCOA_SUGAR}/front-500")]
    );
    // Every request, redirects followed included, says who's asking.
    let got = server.got();
    assert_eq!(got.len(), 2 + 2 * 3);
    assert!(got.iter().all(|g| g.agent == AGENT), "{got:?}");
    // The whole drive, a page at a time.
    let calls = session.stop();
    let reads: Vec<(&str, &Value)> = calls.iter().map(|(m, p)| (m.as_str(), &p["offset"])).collect();
    assert_eq!(reads, [("tracks.query", &json!(0)), ("tracks.query", &json!(2)), ("tracks.query", &json!(4))]);
}

/// With `replace`, tracks that have artwork get the cover too.
#[test]
fn replace_gives_tracks_with_artwork_the_cover_too() {
    let tracks = vec![with_art(tagged(1, "Bicep", "Bicep", "Glue")), with_art(tagged(2, "Bicep", "Bicep", "Aura"))];
    let (session, _server) = run(drive(tracks.clone()), services_as_they_are);
    assert_eq!(session.plan(json!({})).unwrap().summary, "no track chosen needs cover art");
    let plan = session.plan(json!({ "replace": true })).unwrap();
    let paths = vec!["/Contents/Bicep/Glue.mp3".to_string(), "/Contents/Bicep/Aura.mp3".into()];
    assert_eq!(set(&plan), [(paths, (240, 240))]);
}

/// Which tracks: the ones the params name, else the playlist they name, else the selection, else
/// the playlist open. Ids the drive doesn't have are warned about.
#[test]
fn it_takes_the_tracks_or_playlist_chosen() {
    let tracks: Vec<Track> = (1..=5).map(|id| tagged(id, "Nobody", &format!("Album {id}"), "Song")).collect();
    let playlists = vec![("Warmup".to_string(), vec![2, 3, 4]), ("Peak".to_string(), vec![5])];
    let (session, server) = run(Drive { tracks, playlists }, |_: &Req| no_releases());
    let albums = |plan: OpPlan| plan.warnings;
    let named =
        |ids: &[u32]| ids.iter().map(|id| format!("Nobody - Album {id}: not on MusicBrainz")).collect::<Vec<_>>();

    let plan = session.plan(json!({ "tracks": [1, 9] })).unwrap();
    assert_eq!(albums(plan), [vec!["track #9 isn't on the drive".to_string()], named(&[1])].concat());
    assert_eq!(albums(session.plan(json!({ "playlist": "Warmup" })).unwrap()), named(&[2, 3, 4]));
    let context = json!({ "drive": "USB", "playlist": "Peak", "tracks": [3] });
    assert_eq!(albums(session.plan_with(json!({}), context.clone()).unwrap()), named(&[3]));
    assert_eq!(albums(session.plan_with(json!({ "playlist": "Warmup" }), context.clone()).unwrap()), named(&[3]));
    let context = json!({ "drive": "USB", "playlist": "Peak" });
    assert_eq!(albums(session.plan_with(json!({}), context).unwrap()), named(&[5]));
    let plan = session.plan(json!({ "playlist": "Nowhere" })).unwrap_err();
    assert_eq!(kind(&plan), ("not_found", "not found: playlist \"Nowhere\""));
    assert_eq!(server.searches().len(), 1 + 3 + 1 + 1 + 1);

    let calls = session.stop();
    assert_eq!(calls[0], ("tracks.get".to_string(), json!({ "drive": "USB", "ids": [1, 9] })));
    assert_eq!(calls[1], ("tracks.query".to_string(), json!({ "drive": "USB", "offset": 0, "playlist": "Warmup" })));
}

/// A track without an album is looked up by its title, as its single; one without either can't
/// be.
#[test]
fn a_track_without_an_album_gets_its_singles_cover() {
    let tracks = vec![
        Track { isrc: ISRC.into(), ..tagged(1, "Young Fathers", "", "In My View") },
        tagged(2, "Young Fathers", "", ""),
    ];
    let (session, server) = run(drive(tracks), services_as_they_are);
    let plan = session.plan(Value::Null).unwrap();
    assert_eq!(set(&plan), [(vec!["/Contents/Young Fathers/In My View.mp3".to_string()], (240, 240))]);
    assert_eq!(plan.warnings, ["track #2 has no album or title to look up"]);
    assert_eq!(server.covers(), [format!("/release-group/{IN_MY_VIEW}/front-500")]);
}

/// An ISRC MusicBrainz doesn't know, or that isn't on a release of the album's name, is looked
/// up by name instead.
#[test]
fn an_isrc_not_found_falls_back_to_the_albums_name() {
    let tracks = vec![
        Track { isrc: "USRC17607839".into(), ..tagged(1, "Bicep", "Bicep", "Glue") },
        Track { isrc: ISRC.into(), ..tagged(2, "Young Fathers", "Bicep", "In My View") },
        // Not an ISRC: only searched by name.
        Track { isrc: "none".into(), ..tagged(3, "Bicep", "Isles", "Apricots") },
    ];
    let (session, server) = run(drive(tracks), services_as_they_are);
    let plan = session.plan(Value::Null).unwrap();
    let paths = vec!["/Contents/Bicep/Glue.mp3".to_string(), "/Contents/Young Fathers/In My View.mp3".into()];
    assert_eq!(set(&plan), [(paths, (240, 240))]);
    assert_eq!(plan.warnings, ["Bicep - Isles: not on MusicBrainz"]);
    let queries: Vec<String> = server.searches().into_iter().map(|(_, q)| q).collect();
    assert_eq!(
        queries,
        [
            "isrc:USRC17607839",
            r#"release:"Bicep" AND artist:"Bicep""#,
            &format!("isrc:{ISRC}"),
            r#"release:"Bicep" AND artist:"Young Fathers""#,
            r#"release:"Isles" AND artist:"Bicep""#,
        ]
    );
    // Both albums are the one release group: its cover is downloaded once.
    assert_eq!(server.covers().len(), 1);
}

/// An album MusicBrainz has no release of that name for, or several release groups of, is left
/// out with a warning naming them.
#[test]
fn no_match_and_several_are_warnings() {
    let route = |req: &Req| match req.query.as_str() {
        r#"release:"Bicep" AND artist:"Bicep""# => {
            let mut found: Value = serde_json::from_str(RELEASES).unwrap();
            found["releases"][1]["release-group"] =
                json!({ "id": "f0000000-0000-4000-8000-000000000001", "title": "Bicep", "primary-type": "Single" });
            ok(found.to_string())
        }
        _ => no_releases(),
    };
    let tracks = vec![tagged(1, "Bicep", "Bicep", "Glue"), tagged(2, "", "Say \"Hi\" \\o/", "Hi")];
    let (session, server) = run(drive(tracks), route);
    let plan = session.plan(Value::Null).unwrap();
    assert_eq!(plan.summary, "found no cover art for the 2 album(s) looked up");
    assert_eq!(plan.edits, Vec::<Value>::new());
    let base = &server.url;
    assert_eq!(
        plan.warnings,
        [
            format!(
                "Bicep - Bicep: left out, as MusicBrainz has several of that name: Bicep Album {base}/release-group/{BICEP}, \
                 Bicep Single {base}/release-group/f0000000-0000-4000-8000-000000000001"
            ),
            "Say \"Hi\" \\o/: not on MusicBrainz".into(),
        ]
    );
    // Quotes and backslashes are escaped in the phrase; with no artist, only the name is asked.
    assert_eq!(server.searches()[1].1, r#"release:"Say \"Hi\" \\o/""#);
    assert_eq!(server.covers(), Vec::<String>::new());
}

/// Titles match though MusicBrainz writes curly apostrophes and Unicode hyphens; release groups
/// whose ids aren't MusicBrainz ids are passed over.
#[test]
fn titles_match_as_musicbrainz_spells_them() {
    let route = |req: &Req| {
        match req.path.as_str() {
        "/ws/2/release/" => ok(json!({ "releases": [
            { "title": "Don\u{2019}t Stop \u{2010} Live", "release-group": { "id": "../../x", "title": "Don\u{2019}t Stop" } },
            { "title": "Don\u{2019}t  Stop \u{2010} Live", "release-group": { "id": BICEP, "title": "Don\u{2019}t Stop" } },
            { "title": "Something else", "release-group": { "id": IN_MY_VIEW, "title": "Something else" } },
            { "title": "Don't Stop - Live" },
        ] })
        .to_string()),
        _ => services_as_they_are(req),
    }
    };
    let (session, server) = run(drive(vec![tagged(1, "Band", "don't stop - live", "Intro")]), route);
    let plan = session.plan(Value::Null).unwrap();
    assert_eq!(set(&plan), [(vec!["/Contents/Band/Intro.mp3".to_string()], (240, 240))]);
    assert_eq!(server.covers(), [format!("/release-group/{BICEP}/front-500")]);
}

/// A cover that's missing, doesn't download, isn't an image or is too small is a warning, and so
/// is a redirect to a server the manifest doesn't name.
#[test]
fn covers_that_cant_be_had_are_warnings() {
    let route = |req: &Req| {
        let album = |n: u32| format!("release:\"Album {n}\"");
        let group = |n: u32| format!("f0000000-0000-4000-8000-00000000000{n}");
        let one = |n: u32| {
            let release = json!({ "title": format!("Album {n}"), "release-group": { "id": group(n), "title": "x" } });
            ok(json!({ "releases": [release] }).to_string())
        };
        let port = req.base.rsplit(':').next().unwrap();
        match req.path.as_str() {
            "/ws/2/release/" => (1..=6).find(|&n| req.query.starts_with(&album(n))).map_or_else(no_releases, one),
            p if p.contains(&group(1)) => status(404),
            p if p.contains(&group(2)) => status(500),
            p if p.contains(&group(3)) => ok("<html>not an image</html>"),
            p if p.contains(&group(4)) => {
                let mut tiny = Vec::new();
                RgbImage::new(15, 300).write_to(&mut std::io::Cursor::new(&mut tiny), ImageFormat::Png).unwrap();
                ok(tiny)
            }
            p if p.contains(&group(5)) => redirect(307, format!("http://localhost:{port}/0/items/x.jpg")),
            p if p.contains(&group(6)) => redirect(302, format!("{}{p}", req.base)),
            _ => status(404),
        }
    };
    let tracks = (1..=6).map(|n| tagged(n, "", &format!("Album {n}"), "Song")).collect();
    let (session, _server) = run(drive(tracks), route);
    let plan = session.plan(Value::Null).unwrap();
    assert_eq!(plan.edits, Vec::<Value>::new());
    let (refused, warnings) = (&plan.warnings[4], &plan.warnings[5..]);
    assert_eq!(
        plan.warnings[..4],
        [
            "Album 1: the Cover Art Archive has no front cover for it",
            "Album 2: its cover didn't download: the Cover Art Archive answered 500",
            "Album 3: its cover isn't an image: The image format could not be determined",
            "Album 4: its cover is too small (15 x 300 px)",
        ]
    );
    assert!(refused.starts_with("Album 5: its cover didn't download: "), "{refused}");
    assert!(refused.ends_with("redirected to localhost, which the plugin doesn't talk to"), "{refused}");
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with("Album 6: its cover didn't download: "), "{warnings:?}");
    assert!(warnings[0].ends_with("too many redirects"), "{warnings:?}");
}

/// MusicBrainz is asked a second apart at most, across operations; the Cover Art Archive isn't
/// held to that.
#[test]
fn musicbrainz_is_asked_once_a_second() {
    let tracks = vec![
        tagged(1, "Bicep", "Bicep", "Glue"),
        Track { isrc: "USRC17607839".into(), ..tagged(2, "Bicep", "Isles", "Apricots") },
        tagged(3, "Young Fathers", "Cocoa Sugar", "Lord"),
    ];
    let clock = FakeClock::new(false);
    let server = server(clock.clone(), services_as_they_are);
    let session = start(drive(tracks), services(&server), clock.clone());
    session.plan(Value::Null).unwrap();
    session.plan(Value::Null).unwrap();
    let secs = |path: &str| -> Vec<f64> {
        server.got().into_iter().filter(|g| g.path.starts_with(path)).map(|g| g.at.as_secs_f64()).collect()
    };
    assert_eq!(secs("/ws/2/"), [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
    // Bicep's cover, asked for once the search for it has answered, each time.
    assert_eq!(secs("/release-group/"), [0.0, 4.0]);
    assert_eq!(clock.passed(), Duration::from_secs(7));
}

/// Cancelled while waiting its turn to ask MusicBrainz, the plan stops, asking no more.
#[test]
fn cancelling_while_waiting_for_musicbrainz_stops_the_plan() {
    // Time stands still: the second search's turn never comes.
    let clock = FakeClock::new(true);
    let server = server(clock.clone(), services_as_they_are);
    // An ISRC MusicBrainz doesn't know: then the album is searched for by name, its turn to come.
    let tracks = vec![Track { isrc: "USRC17607839".into(), ..tagged(1, "Bicep", "Bicep", "Glue") }];
    let session = start(drive(tracks), services(&server), clock);
    let pending = session.send(Value::Null, json!({ "drive": "USB" }));
    server.heard.recv().unwrap();
    pending.cancel();
    let answer = pending.wait_timeout(Duration::from_secs(10)).expect("it stops");
    assert_eq!(answer.map(|_: OpPlan| ()).unwrap_err(), RpcError::cancelled());
    assert_eq!(server.searches().len(), 1);
}

/// MusicBrainz out of reach fails the plan: no album can be looked up.
#[test]
fn musicbrainz_out_of_reach_fails_the_plan() {
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let services = Services { musicbrainz: url.clone(), coverartarchive: url };
    let session = start(drive(vec![tagged(1, "Bicep", "Bicep", "Glue")]), services, FakeClock::new(false));
    let e = session.plan(Value::Null).unwrap_err();
    assert_eq!(e.kind(), Some("external"));
    assert!(e.message.starts_with("couldn't reach MusicBrainz: "), "{e}");
}

/// A search MusicBrainz refuses as busy is sent again on its next turns, a few times, then its
/// album is left out with a warning; an answer that isn't a search result fails the plan.
#[test]
fn musicbrainz_refusing_is_tried_again_then_a_warning() {
    let refused = Arc::new(Mutex::new(0));
    let refusing = refused.clone();
    let route = move |req: &Req| match req.query.as_str() {
        q if q.contains("Junk") => ok("<html>maintenance</html>"),
        q if q.contains("Isles") => status(500),
        q if q.contains("Bicep") => status(503),
        // Busy once.
        _ if req.path.starts_with("/ws/2/") && std::mem::replace(&mut *refusing.lock().unwrap(), 1) == 0 => status(503),
        _ => services_as_they_are(req),
    };
    let tracks = vec![
        tagged(1, "Bicep", "Bicep", "Glue"),
        Track { isrc: ISRC.into(), ..tagged(2, "Young Fathers", "Cocoa Sugar", "Lord") },
        tagged(3, "Bicep", "Isles", "Apricots"),
    ];
    let clock = FakeClock::new(false);
    let server = server(clock.clone(), route.clone());
    let session = start(drive(tracks), services(&server), clock);
    let plan = session.plan(Value::Null).unwrap();
    assert_eq!(set(&plan).len(), 1);
    assert_eq!(
        plan.warnings,
        [
            "Bicep - Bicep: left out, as MusicBrainz answered 503",
            "Bicep - Isles: left out, as MusicBrainz answered 500"
        ]
    );
    let searches: Vec<(String, f64)> = server
        .got()
        .into_iter()
        .filter(|g| g.path.starts_with("/ws/2/"))
        .map(|g| (g.query, g.at.as_secs_f64()))
        .collect();
    let bicep = r#"release:"Bicep" AND artist:"Bicep""#.to_string();
    let isrc = format!("isrc:{ISRC}");
    let isles = r#"release:"Isles" AND artist:"Bicep""#.to_string();
    assert_eq!(
        searches,
        [(bicep.clone(), 0.0), (bicep.clone(), 1.0), (bicep, 2.0), (isrc.clone(), 3.0), (isrc, 4.0), (isles, 5.0)]
    );
    assert_eq!(*refused.lock().unwrap(), 1);

    let (session, _server) = run(drive(vec![tagged(1, "Bicep", "Junk", "Glue")]), route);
    let e = session.plan(Value::Null).unwrap_err();
    assert_eq!(e.kind(), Some("external"));
    assert!(e.message.starts_with("MusicBrainz answered what isn't a search result: "), "{e}");
}

/// The `contact` setting is how MusicBrainz reaches whoever runs it; blank, the project's.
#[test]
fn the_contact_setting_goes_in_the_user_agent() {
    let (session, server) = run(drive(vec![tagged(1, "Bicep", "Isles", "Glue")]), services_as_they_are);
    let settings = |contact: &str| json!({ "drive": "USB", "settings": { "contact": contact } });
    session.plan_with(Value::Null, settings("dj@example.com")).unwrap();
    session.plan_with(Value::Null, settings("  ")).unwrap();
    let agents: Vec<String> = server.got().into_iter().map(|g| g.agent).collect();
    assert_eq!(agents, ["locallibrary-plugin-coverart/0.1.0 ( dj@example.com )", AGENT]);
}

/// It plans on the drive the user is on: without one, there's nothing to plan.
#[test]
fn it_needs_a_drive() {
    let (session, server) = run(drive(Vec::new()), services_as_they_are);
    let e = session.plan_with(json!({ "replace": true }), json!({})).unwrap_err();
    assert_eq!(kind(&e), ("invalid_params", "invalid params: say which drive"));
    let e = session.plan(json!({ "replace": "yes" })).unwrap_err();
    assert_eq!(e.kind(), Some("invalid_params"));
    assert_eq!(server.got().len(), 0);
    assert_eq!(session.stop(), Vec::new());
}

/// A plan's covers fit in one message: once they add up to most of one, the albums left aren't
/// looked up, for another run.
#[test]
fn a_plan_holds_as_many_covers_as_fit_in_one_message() {
    const ALBUMS: u32 = 200;
    let cover = jpeg(covers::SIZE, covers::SIZE, true);
    let route = move |req: &Req| match req.query.strip_prefix("release:\"Album ") {
        Some(n) => {
            let n: u32 = n.trim_end_matches('"').parse().unwrap();
            let group = json!({ "id": format!("f0000000-0000-4000-8000-{n:012}"), "title": "x" });
            ok(json!({ "releases": [{ "title": format!("Album {n}"), "release-group": group }] }).to_string())
        }
        None => ok(cover.clone()),
    };
    let tracks = (1..=ALBUMS).map(|n| tagged(n, "", &format!("Album {n}"), "Song")).collect();
    let (session, server) = run(drive(tracks), route);
    let plan = session.plan(Value::Null).unwrap();
    let covers = plan.edits.len();
    let left = ALBUMS as usize - covers;
    assert!(left > 0 && covers > 1, "{covers} covers");
    assert_eq!(plan.warnings, [format!("{left} album(s) left for another run: a plan holds no more covers")]);
    assert_eq!(server.searches().len(), covers);
    assert!(serde_json::to_string(&plan).unwrap().len() < loclib_plugin::framing::MAX_LINE);
}

/// Outside tests, time is the computer's.
#[test]
fn the_system_clock_sleeps() {
    let before = SystemClock.now();
    SystemClock.sleep(Duration::from_millis(5));
    assert!(SystemClock.now() >= before + Duration::from_millis(5));
}

/// An ISRC search MusicBrainz keeps refusing leaves the album out with a warning, with no search
/// by name after it.
#[test]
fn an_isrc_search_refused_leaves_the_album_out() {
    let route = |req: &Req| match req.query.starts_with("isrc:") {
        true => status(503),
        false => services_as_they_are(req),
    };
    let tracks = vec![Track { isrc: ISRC.into(), ..tagged(1, "Young Fathers", "Cocoa Sugar", "Lord") }];
    let (session, server) = run(drive(tracks), route);
    let plan = session.plan(Value::Null).unwrap();
    assert!(set(&plan).is_empty());
    assert_eq!(plan.warnings, ["Young Fathers - Cocoa Sugar: left out, as MusicBrainz answered 503"]);
    let queries: Vec<String> = server.searches().into_iter().map(|(_, q)| q).collect();
    assert_eq!(queries, [format!("isrc:{ISRC}"), format!("isrc:{ISRC}"), format!("isrc:{ISRC}")]);
}

// Properties: drawn drives against drawn services, the plan foreseen from what was drawn.

/// The release groups drawn searches find, by index: shaped as MusicBrainz's ids are, so their
/// covers are asked for.
const GROUPS: [&str; 3] = [
    "a0000000-0000-4000-8000-000000000000",
    "a0000000-0000-4000-8000-000000000001",
    "a0000000-0000-4000-8000-000000000002",
];
/// How many albums drawn tracks are of: `Album 0`, `Album 1`...
const ALBUMS: usize = 3;

/// The ISRC of album `k`'s tracks.
fn isrc_of(k: usize) -> String {
    format!("GBAAA260000{k}")
}

/// A drawn track and what it's known to be: of which album (`k`, by its name in some letter case
/// and spacing, as its album or as the title of a single), by which artist (spellings of one are
/// one), and whether its ISRC tag is one.
#[derive(Debug, Clone)]
struct Drawn {
    track: Track,
    album: Option<usize>,
    artist: usize,
    isrc: bool,
}

#[hegel::test_helper]
fn drawn_track(tc: &TestCase, id: u32) -> Drawn {
    let album = match tc.draw(gs::weighted_booleans(0.9)) {
        true => Some(tc.draw(gs::integers::<usize>().max_value(ALBUMS - 1))),
        false => None,
    };
    let spellings = |k: usize| vec![format!("Album {k}"), format!("ALBUM {k}"), format!("  album  {k} ")];
    let name = album.map(|k| tc.draw(gs::sampled_from(spellings(k))));
    let single = tc.draw(gs::booleans());
    let (album_tag, title) = match name {
        None => (String::new(), String::new()),
        Some(name) if single => (String::new(), name),
        Some(name) => (name, "Song".to_string()),
    };
    let (artist_tag, mut artist) =
        tc.draw(gs::sampled_from(vec![("Bicep", 0usize), ("BICEP", 0), (" bicep ", 0), ("Young Fathers", 1), ("", 2)]));
    // An album artist goes before the track's.
    let album_artist = tc.draw(gs::booleans());
    if album_artist {
        artist = 3;
    }
    let k = album.unwrap_or_default();
    let isrcs = vec![
        (String::new(), false),
        (isrc_of(k), true),
        (format!("gb-aaa-26-0000{k}"), true),
        (format!("GB AAA 26 0000{k}"), true),
        (format!("GBAAA26000{k}"), false),
        ("none".to_string(), false),
    ];
    let (isrc_tag, isrc) = tc.draw(gs::sampled_from(isrcs));
    let mut track = Track {
        album: album_tag,
        album_artist: if album_artist { "Various Artists".into() } else { String::new() },
        isrc: isrc_tag,
        ..track(id, artist_tag, &title, &format!("/Contents/{id}.mp3"))
    };
    if tc.draw(gs::weighted_booleans(0.3)) {
        track = with_art(track);
    }
    Drawn { track, album, artist, isrc }
}

/// How MusicBrainz answers a search: `503` (busy) `busy` times, then `answer`; `Retry-After`
/// with what it refuses, or not.
#[derive(Debug, Clone)]
struct Search {
    busy: usize,
    retry_after: bool,
    answer: Answer,
}

#[derive(Debug, Clone)]
enum Answer {
    /// Releases named as the album is, of these release groups (by index; a release group can be
    /// on several), beside some that aren't (named otherwise, not of a release group, or of one
    /// whose id isn't MusicBrainz's).
    Groups(Vec<usize>),
    Status(u16),
    /// A page that isn't a search result.
    Garbage,
    Cut,
}

#[hegel::test_helper]
fn drawn_search(tc: &TestCase) -> Search {
    let busy = tc.draw(gs::sampled_from(vec![0usize, 0, 0, 1, 2, 3]));
    let retry_after = tc.draw(gs::booleans());
    let group = || gs::integers::<usize>().max_value(GROUPS.len() - 1);
    // Mostly the album, on its own (named once, or on two releases), so its cover is asked for.
    let answer = match tc
        .draw(gs::sampled_from(vec!["one", "one", "one", "one", "none", "several", "refused", "garbage", "cut"]))
    {
        "one" => Answer::Groups(vec![tc.draw(group()); tc.draw(gs::integers::<usize>().min_value(1).max_value(2))]),
        "none" => Answer::Groups(Vec::new()),
        "several" => Answer::Groups(tc.draw(gs::vecs(group()).min_size(2).max_size(3))),
        "refused" => Answer::Status(tc.draw(gs::sampled_from(vec![404u16, 429, 500]))),
        "garbage" => Answer::Garbage,
        _ => Answer::Cut,
    };
    Search { busy, retry_after, answer }
}

/// How the Cover Art Archive gives a release group's front cover.
#[derive(Debug, Clone)]
enum Cover {
    /// A JPEG (or PNG) of this size, after this many redirects between its own servers.
    Image {
        width: u32,
        height: u32,
        png: bool,
        hops: u32,
    },
    Status(u16),
    Garbage,
    /// A redirect to a server the plugin doesn't talk to.
    Away,
    /// Redirected to itself, for ever.
    Loop,
    Cut,
}

#[hegel::test_helper]
fn drawn_cover(tc: &TestCase) -> Cover {
    let side = |from: u32, to: u32| gs::integers::<u32>().min_value(from).max_value(to);
    let kinds = vec!["image", "image", "tiny", "image", "tiny", "refused", "garbage", "away", "loop", "cut"];
    match tc.draw(gs::sampled_from(kinds)) {
        kind @ ("image" | "tiny") => {
            let (mut width, mut height) = (tc.draw(side(16, 300)), tc.draw(side(16, 300)));
            // Too small one way or the other.
            if kind == "tiny" {
                // Mostly just too small.
                let tiny = gs::sampled_from(vec![15u32, 15, 15, 1, 8]);
                match tc.draw(gs::booleans()) {
                    true => width = tc.draw(tiny),
                    false => height = tc.draw(tiny),
                }
            }
            let png = tc.draw(gs::booleans());
            let hops = tc.draw(gs::integers::<u32>().max_value(2));
            Cover::Image { width, height, png, hops }
        }
        "refused" => Cover::Status(tc.draw(gs::sampled_from(vec![404u16, 500, 503]))),
        "garbage" => Cover::Garbage,
        "away" => Cover::Away,
        "loop" => Cover::Loop,
        _ => Cover::Cut,
    }
}

/// The services drawn: each album's release and recording searches, and each release group's
/// cover.
#[derive(Debug, Clone)]
struct Answers {
    releases: Vec<Search>,
    recordings: Vec<Search>,
    covers: Vec<Cover>,
}

#[hegel::test_helper]
fn drawn_answers(tc: &TestCase) -> Answers {
    Answers {
        releases: (0..ALBUMS).map(|_| drawn_search(tc)).collect(),
        recordings: (0..ALBUMS).map(|_| drawn_search(tc)).collect(),
        covers: (0..GROUPS.len()).map(|_| drawn_cover(tc)).collect(),
    }
}

/// How MusicBrainz spells album `k`, which is as the tracks do but for letter case and spacing.
fn spelled_by_musicbrainz(k: usize) -> String {
    format!("album  {k}")
}

/// A search's answer as MusicBrainz gives it: a release search's, or a recording search's naming
/// the releases each recording is on.
fn search_result(k: usize, groups: &[usize], recording: bool) -> String {
    let named = spelled_by_musicbrainz(k);
    let group = |g: usize| {
        let kind = (g.is_multiple_of(2)).then_some("Album");
        json!({ "id": GROUPS[g], "title": format!("Group {g}"), "primary-type": kind })
    };
    let mut releases: Vec<Value> =
        groups.iter().map(|&g| json!({ "title": named, "release-group": group(g) })).collect();
    releases.extend([
        json!({ "title": "Something else", "release-group": group(0) }),
        json!({ "title": named }),
        json!({ "title": named, "release-group": { "id": "../../x", "title": "x" } }),
    ]);
    match recording {
        true => json!({ "recordings": [{ "releases": releases }, {}] }).to_string(),
        false => json!({ "releases": releases }).to_string(),
    }
}

fn picture(width: u32, height: u32, png: bool) -> Vec<u8> {
    match png {
        true => {
            let mut out = Vec::new();
            RgbImage::new(width, height).write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png).unwrap();
            out
        }
        false => jpeg(width, height, false),
    }
}

/// MusicBrainz and the Cover Art Archive answering as `answers` has them.
fn answering(answers: Answers) -> impl Fn(&Req) -> Reply + Send + 'static {
    let asked: Mutex<Vec<String>> = Mutex::default();
    move |req: &Req| {
        let (base, path) = (&req.base, req.path.as_str());
        // Which album: the last digit of the name searched for, or of the ISRC.
        let album = |of: &str| of.chars().last().and_then(|c| c.to_digit(10)).unwrap() as usize;
        let search = match path {
            "/ws/2/release/" => Some((&answers.releases, false, album(req.query.split('"').nth(1).unwrap()))),
            "/ws/2/recording/" => Some((&answers.recordings, true, album(&req.query))),
            _ => None,
        };
        if let Some((searches, recording, k)) = search {
            let search = &searches[k];
            let key = format!("{path}?{}", req.query);
            let times = {
                let mut asked = asked.lock().unwrap();
                asked.push(key.clone());
                asked.iter().filter(|k| **k == key).count()
            };
            let refused = |code: u16| match search.retry_after {
                true => Reply { headers: vec![("Retry-After", "1".into())], ..status(code) },
                false => status(code),
            };
            if times <= search.busy {
                return refused(503);
            }
            return match &search.answer {
                Answer::Groups(groups) => ok(search_result(k, groups, recording)),
                Answer::Status(code) => refused(*code),
                Answer::Garbage => ok("<html>maintenance</html>"),
                Answer::Cut => cut(),
            };
        }
        let image = |g: usize| match answers.covers[g] {
            Cover::Image { width, height, png, .. } => ok(picture(width, height, png)),
            _ => unreachable!(),
        };
        if let Some(id) = path.strip_prefix("/release-group/").and_then(|p| p.strip_suffix("/front-500")) {
            let g = GROUPS.iter().position(|known| *known == id).unwrap();
            let port = base.rsplit(':').next().unwrap();
            return match answers.covers[g] {
                Cover::Image { hops: 0, .. } => image(g),
                Cover::Image { hops, .. } => redirect(307, format!("{base}/hop/{g}/{}", hops - 1)),
                Cover::Status(code) => status(code),
                Cover::Garbage => ok("<html>not an image</html>"),
                Cover::Away => redirect(302, format!("http://localhost:{port}/elsewhere/{g}")),
                Cover::Loop => redirect(302, format!("{base}/loop/{g}")),
                Cover::Cut => cut(),
            };
        }
        let mut parts = path.split('/').skip(1);
        match (parts.next(), parts.next().and_then(|g| g.parse::<usize>().ok()), parts.next()) {
            (Some("hop"), Some(g), Some("0")) => image(g),
            (Some("hop"), Some(g), Some(n)) => {
                redirect(302, format!("{base}/hop/{g}/{}", n.parse::<u32>().unwrap() - 1))
            }
            (Some("loop"), Some(_), None) => redirect(302, format!("{base}{path}")),
            _ => status(404),
        }
    }
}

/// A warning foreseen: exactly this, or (where it holds reqwest's words) starting and ending so.
#[derive(Debug, Clone, PartialEq)]
enum Warned {
    Exactly(String),
    Around(String, &'static str),
}

impl Warned {
    fn matches(&self, warning: &str) -> bool {
        match self {
            Warned::Exactly(w) => w == warning,
            Warned::Around(start, end) => warning.starts_with(start.as_str()) && warning.ends_with(end),
        }
    }
}

/// A plan foreseen: its summary, warnings and edits (each one's tracks, and its image's size).
type Foreseen = (String, Vec<Warned>, Vec<(Vec<String>, (u32, u32))>);

/// An error a plan fails with: its kind, and how its message starts.
type Refusal = (&'static str, String);

/// What a plan should come to: the plan or the error it fails with (its kind, and how its
/// message starts), with the searches and cover downloads it takes.
struct Outcome {
    plan: Result<Foreseen, Refusal>,
    searches: Vec<(String, String)>,
    covers: Vec<String>,
}

/// An album as the plan looks it up: shown as its first track tags it, searched by its first ISRC.
struct Looked {
    k: usize,
    artist: String,
    name: String,
    isrc: bool,
    paths: Vec<String>,
}

impl Looked {
    fn shown(&self) -> String {
        match self.artist.is_empty() {
            true => self.name.clone(),
            false => format!("{} - {}", self.artist, self.name),
        }
    }
}

/// What `chosen`'s tracks come to, the ones needing art (all, with `replace`) looked up on the
/// services `answers` stands for at `mb`, after the warnings `warned` already.
fn foresee(answers: &Answers, chosen: &[&Drawn], replace: bool, mut warned: Vec<Warned>, mb: &str) -> Outcome {
    let mut albums: Vec<((usize, usize), Looked)> = Vec::new();
    for d in chosen.iter().filter(|d| replace || d.track.artwork_path.is_none()) {
        let t = &d.track;
        let Some(k) = d.album else {
            warned.push(Warned::Exactly(format!("track #{} has no album or title to look up", t.id)));
            continue;
        };
        let at = match albums.iter().position(|(key, _)| *key == (d.artist, k)) {
            Some(at) => at,
            None => {
                let tagged =
                    |tags: [&str; 2]| tags.into_iter().map(str::trim).find(|s| !s.is_empty()).unwrap_or("").to_string();
                let (artist, name) = (tagged([&t.album_artist, &t.artist]), tagged([&t.album, &t.title]));
                albums.push(((d.artist, k), Looked { k, artist, name, isrc: false, paths: Vec::new() }));
                albums.len() - 1
            }
        };
        let album = &mut albums[at].1;
        album.isrc |= d.isrc;
        album.paths.push(t.file_path.clone());
    }

    let (mut searches, mut covers) = (Vec::new(), Vec::new());
    // Each cover downloaded, by release group: its tracks, or why there's none.
    let mut got: Vec<(usize, Result<Vec<String>, Warned>)> = Vec::new();
    let mut asked: Vec<(String, String)> = Vec::new();
    // A search's release groups (deduplicated), or the status it was refused with; or the error
    // the plan fails with.
    let mut search = |entity: &str, search: &Search, query: String| {
        let path = format!("/ws/2/{entity}/");
        for tries in 1..=3 {
            asked.push((path.clone(), query.clone()));
            searches.push((path.clone(), query.clone()));
            let times = asked.iter().filter(|a| a.0 == path && a.1 == query).count();
            if times <= search.busy {
                match tries < 3 {
                    true => continue,
                    false => return Ok(Err(503)),
                }
            }
            return match &search.answer {
                Answer::Groups(groups) => {
                    let mut found: Vec<usize> = Vec::new();
                    for &g in groups {
                        if !found.contains(&g) {
                            found.push(g);
                        }
                    }
                    Ok(Ok(found))
                }
                Answer::Status(code) => Ok(Err(*code)),
                Answer::Garbage => Err(("external", "MusicBrainz answered what isn't a search result: ".to_string())),
                Answer::Cut => Err(("external", "couldn't reach MusicBrainz: ".to_string())),
            };
        }
        unreachable!()
    };
    let mut fail = None;
    for (_, album) in &albums {
        let shown = album.shown();
        let mut found = Ok(Vec::new());
        if album.isrc {
            match search("recording", &answers.recordings[album.k], format!("isrc:{}", isrc_of(album.k))) {
                Ok(answer) => found = answer,
                Err(e) => {
                    fail = Some(e);
                    break;
                }
            }
        }
        if found.as_ref().is_ok_and(Vec::is_empty) {
            let mut query = format!("release:\"{}\"", album.name);
            if !album.artist.is_empty() {
                query = format!("{query} AND artist:\"{}\"", album.artist);
            }
            match search("release", &answers.releases[album.k], query) {
                Ok(answer) => found = answer,
                Err(e) => {
                    fail = Some(e);
                    break;
                }
            }
        }
        let g = match found {
            Err(code) => {
                warned.push(Warned::Exactly(format!("{shown}: left out, as MusicBrainz answered {code}")));
                continue;
            }
            Ok(groups) if groups.is_empty() => {
                warned.push(Warned::Exactly(format!("{shown}: not on MusicBrainz")));
                continue;
            }
            Ok(groups) if groups.len() > 1 => {
                let named = groups.iter().map(|&g| {
                    let kind = if g.is_multiple_of(2) { "Album" } else { "release" };
                    format!("Group {g} {kind} {mb}/release-group/{}", GROUPS[g])
                });
                let named = named.collect::<Vec<_>>().join(", ");
                warned.push(Warned::Exactly(format!(
                    "{shown}: left out, as MusicBrainz has several of that name: {named}"
                )));
                continue;
            }
            Ok(groups) => groups[0],
        };
        if !got.iter().any(|(had, _)| *had == g) {
            covers.push(format!("/release-group/{}/front-500", GROUPS[g]));
            let failed = |why: &str| -> Result<Vec<String>, Warned> { Err(Warned::Exactly(why.to_string())) };
            let cut_off = |end: &'static str| -> Result<Vec<String>, Warned> {
                Err(Warned::Around("its cover didn't download: ".into(), end))
            };
            let cover = match answers.covers[g] {
                Cover::Image { width, height, .. } if width < 16 || height < 16 => {
                    failed(&format!("its cover is too small ({width} x {height} px)"))
                }
                Cover::Image { .. } => Ok(Vec::new()),
                Cover::Status(404) => failed("the Cover Art Archive has no front cover for it"),
                Cover::Status(code) => {
                    failed(&format!("its cover didn't download: the Cover Art Archive answered {code}"))
                }
                Cover::Garbage => failed("its cover isn't an image: The image format could not be determined"),
                Cover::Away => cut_off("redirected to localhost, which the plugin doesn't talk to"),
                Cover::Loop => cut_off("too many redirects"),
                Cover::Cut => cut_off(""),
            };
            got.push((g, cover));
        }
        match &mut got.iter_mut().find(|(had, _)| *had == g).unwrap().1 {
            Ok(paths) => paths.extend(album.paths.iter().cloned()),
            Err(Warned::Exactly(why)) => warned.push(Warned::Exactly(format!("{shown}: {why}"))),
            Err(Warned::Around(why, end)) => warned.push(Warned::Around(format!("{shown}: {why}"), end)),
        }
    }
    if let Some(e) = fail {
        return Outcome { plan: Err(e), searches, covers };
    }
    let edits: Vec<(Vec<String>, (u32, u32))> =
        got.into_iter().filter_map(|(_, cover)| Some((cover.ok()?, (covers::SIZE, covers::SIZE)))).collect();
    let tracks: usize = edits.iter().map(|(paths, _)| paths.len()).sum();
    let summary = match (edits.len(), albums.len()) {
        (_, 0) => "no track chosen needs cover art".to_string(),
        (0, n) => format!("found no cover art for the {n} album(s) looked up"),
        (found, _) => format!("set cover art from the Cover Art Archive on {tracks} track(s) of {found} album(s)"),
    };
    Outcome { plan: Ok((summary, warned, edits)), searches, covers }
}

/// The tracks a plan is asked for, as the user chooses them: by the params, the context, or both.
#[derive(Debug)]
struct Asked {
    params: Value,
    context: Value,
    /// The tracks of the drive's playlist `Set`, by id (some of them not on the drive).
    set: Vec<u32>,
}

#[hegel::test_helper]
fn drawn_ask(tc: &TestCase, n: u32) -> Asked {
    // Mostly the whole drive, so most plans have albums to look up.
    let some = |p: f64| tc.draw(gs::weighted_booleans(p));
    let ids = || gs::vecs(gs::integers::<u32>().min_value(1).max_value(n + 1)).unique(true).min_size(1).max_size(6);
    let playlist = || gs::sampled_from(vec!["Set", "Set", "Nowhere"]);
    let mut params = json!({});
    if some(0.2) {
        params["tracks"] = json!(tc.draw(ids()));
    }
    if some(0.2) {
        params["playlist"] = json!(tc.draw(playlist()));
    }
    if let Some(replace) = tc.draw(gs::optional(gs::booleans())) {
        params["replace"] = json!(replace);
    }
    if params == json!({}) && tc.draw(gs::booleans()) {
        params = Value::Null;
    }
    let mut context = json!({});
    match some(0.9) {
        true => context["drive"] = json!("USB"),
        false => {
            if let Some(drive) = tc.draw(gs::optional(gs::just("Other"))) {
                context["drive"] = json!(drive);
            }
        }
    }
    if some(0.2) {
        context["tracks"] = json!(tc.draw(ids()));
    }
    if some(0.2) {
        context["playlist"] = json!(tc.draw(playlist()));
    }
    if let Some(contact) = tc.draw(gs::optional(gs::sampled_from(vec!["dj@example.com", "  "]))) {
        context["settings"] = json!({ "contact": contact });
    }
    let set = gs::vecs(gs::integers::<u32>().min_value(1).max_value(n + 1)).unique(true).max_size(8);
    Asked { params, context, set: tc.draw(set) }
}

/// The ids in `v` (an array, or missing).
fn ids_in(v: &Value) -> Vec<u32> {
    v.as_array().map(|a| a.iter().map(|id| id.as_u64().unwrap() as u32).collect()).unwrap_or_default()
}

/// A read the plugin calls back for: its method and params.
type Read = (String, Value);

/// What the user chose of `drawn`, as the host gives it to the plugin: the tracks (or the error
/// the plan fails with), the warnings for ids the drive hasn't, and the reads asked for them.
fn choice<'d>(drawn: &'d [Drawn], asked: &Asked) -> (Result<Vec<&'d Drawn>, Refusal>, Vec<Warned>, Vec<Read>) {
    let (params, context) = (&asked.params, &asked.context);
    let by_id = |id: &u32| drawn.iter().find(|d| d.track.id == *id);
    let Some(drive) = context["drive"].as_str() else {
        return (Err(("invalid_params", "invalid params: say which drive".into())), Vec::new(), Vec::new());
    };
    let not_on_drive = || Err(("not_found", format!("not found: drive {drive:?}")));
    let (mut ids, selected) = (ids_in(&params["tracks"]), ids_in(&context["tracks"]));
    if ids.is_empty() {
        ids = selected;
    }
    if !ids.is_empty() {
        let read = vec![("tracks.get".to_string(), json!({ "drive": drive, "ids": ids }))];
        if drive != "USB" {
            return (not_on_drive(), Vec::new(), read);
        }
        let missing = ids.iter().filter(|id| by_id(id).is_none());
        let warned = missing.map(|id| Warned::Exactly(format!("track #{id} isn't on the drive"))).collect();
        return (Ok(ids.iter().filter_map(by_id).collect()), warned, read);
    }
    let playlist = params["playlist"].as_str().or(context["playlist"].as_str());
    let page = |offset: usize| {
        let mut query = json!({ "drive": drive, "offset": offset });
        if let Some(name) = playlist {
            query["playlist"] = json!(name);
        }
        ("tracks.query".to_string(), query)
    };
    let tracks: Vec<&Drawn> = match playlist {
        _ if drive != "USB" => return (not_on_drive(), Vec::new(), vec![page(0)]),
        Some("Nowhere") => {
            return (Err(("not_found", "not found: playlist \"Nowhere\"".into())), Vec::new(), vec![page(0)]);
        }
        Some(_) => asked.set.iter().filter_map(by_id).collect(),
        None => drawn.iter().collect(),
    };
    // A page at a time, never past the last.
    let reads = (0..tracks.len().max(1)).step_by(PAGE).map(page).collect();
    (Ok(tracks), Vec::new(), reads)
}

/// For any drive (tracks with and without artwork, in albums or singles, by artists spelled
/// several ways, with ISRCs or not), however the user chooses its tracks, and however MusicBrainz
/// and the Cover Art Archive answer (found, not found, several, busy, refusing, garbage, cut off;
/// covers missing, broken, tiny, redirected within the archive's servers, away from them or for
/// ever), the plan is what their answers make it: each album needing art looked up once (by ISRC
/// first), each release group's cover downloaded once and set on those tracks (all of them, with
/// `replace`), a warning for each album left out, never a request to a server it doesn't talk
/// to, MusicBrainz asked a second apart, and every request saying who's asking.
#[hegel::test(test_cases = 60)]
fn any_drive_gets_the_covers_the_services_answer(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>().max_value(8));
    let drawn: Vec<Drawn> = (1..=n).map(|id| drawn_track(&tc, id)).collect();
    let asked = drawn_ask(&tc, n);
    let answers = drawn_answers(&tc);

    let tracks = drawn.iter().map(|d| d.track.clone()).collect();
    let playlists = vec![("Set".to_string(), asked.set.clone())];
    let (session, server) = run(Drive { tracks, playlists }, answering(answers.clone()));
    let planned = session.plan_with(asked.params.clone(), asked.context.clone());

    let replace = asked.params["replace"].as_bool().unwrap_or(false);
    let (tracks, warned, reads) = choice(&drawn, &asked);
    let foreseen = match tracks {
        Ok(tracks) => foresee(&answers, &tracks, replace, warned, &server.url),
        Err(e) => Outcome { plan: Err(e), searches: Vec::new(), covers: Vec::new() },
    };
    match (&planned, &foreseen.plan) {
        (Ok(plan), Ok((summary, warned, edits))) => {
            assert_eq!(&plan.summary, summary);
            assert_eq!(plan.warnings.len(), warned.len(), "{:?} for {warned:?}", plan.warnings);
            for (warning, foreseen) in plan.warnings.iter().zip(warned) {
                assert!(foreseen.matches(warning), "{warning:?}, not {foreseen:?}");
            }
            assert_eq!(&set(plan), edits);
            assert!(plan.inputs.is_empty());
        }
        (Err(e), Err((kind, start))) => {
            assert_eq!(e.kind(), Some(*kind), "{e:?}");
            assert!(e.message.starts_with(start.as_str()), "{e:?}");
        }
        (planned, foreseen) => panic!("planned {planned:?}, not {foreseen:?}"),
    }
    assert_eq!(server.searches(), foreseen.searches);
    assert_eq!(server.covers(), foreseen.covers);

    let got = server.got();
    assert!(got.iter().all(|g| !g.path.starts_with("/elsewhere/")), "{got:?}");
    let contact = asked.context["settings"]["contact"].as_str().map(str::trim).filter(|c| !c.is_empty());
    let agent = contact.map_or(AGENT.to_string(), |c| format!("locallibrary-plugin-coverart/0.1.0 ( {c} )"));
    assert!(got.iter().all(|g| g.agent == agent), "{got:?}");
    // MusicBrainz's turns, a second apart; covers are downloaded without waiting.
    let at: Vec<Duration> = got.iter().filter(|g| g.path.starts_with("/ws/2/")).map(|g| g.at).collect();
    assert_eq!(at, (0..at.len() as u64).map(Duration::from_secs).collect::<Vec<_>>());
    assert_eq!(session.stop(), reads);
}

/// Cancelled while it waits its turn to ask MusicBrainz again, whatever it was looking up, the
/// plan stops with `cancelled`, asking MusicBrainz nothing more.
#[hegel::test(test_cases = 10)]
fn cancelling_any_plan_waiting_its_turn_stops_it(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>().min_value(1).max_value(8));
    let drawn: Vec<Drawn> = (1..=n).map(|id| drawn_track(&tc, id)).collect();
    let answers = drawn_answers(&tc);
    let asked = Asked { params: json!({ "replace": true }), context: json!({ "drive": "USB" }), set: Vec::new() };
    let (tracks, warned, _) = choice(&drawn, &asked);
    // A second search, which a first that fails the plan would leave unasked.
    let foreseen = foresee(&answers, &tracks.unwrap(), true, warned, "");
    tc.assume(foreseen.searches.len() > 1);

    // Time stands still: the second search's turn never comes.
    let clock = FakeClock::new(true);
    let server = server(clock.clone(), answering(answers));
    let tracks = drawn.into_iter().map(|d| d.track).collect();
    let session = start(drive(tracks), services(&server), clock);
    let pending = session.send(asked.params, asked.context);
    server.heard.recv().unwrap();
    pending.cancel();
    let answer = pending.wait_timeout(Duration::from_secs(10)).expect("it stops");
    assert_eq!(answer.map(|_: OpPlan| ()).unwrap_err(), RpcError::cancelled());
    assert_eq!(server.searches(), foreseen.searches[..1]);
}

/// On the computer's clock too, MusicBrainz is asked a second apart: a plan searching for each of
/// a few albums takes a second for each after the first.
#[hegel::test(test_cases = 3)]
fn the_computers_clock_spaces_musicbrainzs_searches_too(tc: TestCase) {
    let albums = tc.draw(gs::integers::<u32>().min_value(1).max_value(2));
    let tracks = (1..=albums).map(|k| tagged(k, "", &format!("Album {k}"), "Song")).collect();
    let server = server(FakeClock::new(false), |_: &Req| no_releases());
    let session = start(drive(tracks), services(&server), Arc::new(SystemClock));
    let began = Instant::now();
    let plan = session.plan(Value::Null).unwrap();
    let took = began.elapsed();
    assert_eq!(plan.summary, format!("found no cover art for the {albums} album(s) looked up"));
    assert_eq!(server.searches().len(), albums as usize);
    assert!(took >= Duration::from_secs(u64::from(albums - 1)), "{took:?}");
}

/// Served as a real plugin is, talking to MusicBrainz and the Cover Art Archive themselves, a plan
/// with nothing to look up asks them nothing: params of the wrong types, and a plan without a
/// drive, are refused; tracks that all have artwork or nothing to look up by plan nothing, saying
/// why.
#[hegel::test(test_cases = 15)]
fn served_for_real_it_plans_nothing_it_neednt_look_up(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>().max_value(5));
    let mut tracks = Vec::new();
    let mut nameless = Vec::new();
    for id in 1..=n {
        match tc.draw(gs::booleans()) {
            true => tracks.push(with_art(tagged(id, "Bicep", "Bicep", "Glue"))),
            false => {
                tracks.push(tagged(id, "Bicep", " ", ""));
                nameless.push(format!("track #{id} has no album or title to look up"));
            }
        }
    }
    let (params, typed) = tc.draw(gs::sampled_from(vec![
        ("null", true),
        ("{}", true),
        (r#"{ "replace": false, "playlist": null }"#, true),
        (r#"{ "replace": "yes" }"#, false),
        (r#"{ "tracks": "1" }"#, false),
        (r#"{ "playlist": 5 }"#, false),
    ]));
    let params: Value = serde_json::from_str(params).unwrap();
    let on_drive = tc.draw(gs::booleans());
    let session = start_with(drive(tracks), serve);
    let context = if on_drive { json!({ "drive": "USB" }) } else { json!({}) };
    let planned = session.plan_with(params, context);
    match (typed, on_drive) {
        (false, _) => assert_eq!(planned.unwrap_err().kind(), Some("invalid_params")),
        (true, false) => {
            assert_eq!(kind(&planned.unwrap_err()), ("invalid_params", "invalid params: say which drive"))
        }
        (true, true) => {
            let plan = planned.unwrap();
            assert_eq!(plan.summary, "no track chosen needs cover art");
            assert_eq!(plan.warnings, nameless);
            assert!(plan.edits.is_empty());
        }
    }
    // The drive's tracks, a page at a time, once there's a plan to make.
    let pages = (0..n.max(1) as usize).step_by(PAGE).count();
    let reads = session.stop();
    assert_eq!(reads.len(), if typed && on_drive { pages } else { 0 }, "{reads:?}");
}
