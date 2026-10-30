use std::io::pipe;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use hegel::TestCase;
use hegel::generators as gs;
use image::codecs::jpeg::JpegEncoder;
use image::{ImageFormat, RgbImage};
use loclib_plugin::{
    EventName, Fetch, Fetched, HelloReply, Incoming, Listener, Manifest, NetLimit, PROTOCOL_VERSION, Peer, Provider,
    ProviderKind, Suggestion, method,
};
use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;

/// MusicBrainz's answers, as it gave them (trimmed to what a search result has): a release
/// search for Bicep's album "Bicep", and a recording search for the ISRC of Young Fathers' "In My
/// View", which is on its single, the album "Cocoa Sugar" and two compilations. Shaped by hand as
/// MusicBrainz answers: the label info (Ninja Tune) of the release search's first two releases,
/// and a lookup of Bicep's release group with `inc=genres`, whose votes are made up so that
/// electronic and techno have the most.
const RELEASES: &str = include_str!("../fixtures/release-search.json");
const RECORDINGS: &str = include_str!("../fixtures/recording-search.json");
const GENRES: &str = include_str!("../fixtures/release-group-genres.json");
/// The release groups of those.
const BICEP: &str = "6f34e134-49d5-4e56-b3e0-4fe6b9900380";
const COCOA_SUGAR: &str = "95aea47d-79b6-483a-8abe-fccfdfa23eae";
const IN_MY_VIEW: &str = "41c11242-e68c-49bb-ba10-e7905ec25ecd";
const ISRC: &str = "GBCFB1700586";

/// Where the plugin searches MusicBrainz, and asks the Cover Art Archive for covers.
const SEARCH: &str = "https://musicbrainz.org/ws/2/";
const COVERS: &str = "https://coverartarchive.org/release-group/";

/// A fetch the plugin asked the fake host for.
#[derive(Debug, Clone)]
struct Req {
    url: String,
    /// The URL but its query.
    path: String,
    /// Its `query` param, decoded.
    query: String,
}

/// How the server answers a fetch, through the host: a status and a body, or the host's
/// failure to get an answer.
enum Reply {
    Answer(u16, Vec<u8>),
    Fails(RpcError),
    /// No answer yet: the fetch is under way until the plugin's hook is cancelled.
    Held,
}

fn ok(body: impl Into<Vec<u8>>) -> Reply {
    Reply::Answer(200, body.into())
}

fn status(status: u16) -> Reply {
    Reply::Answer(status, b"<html>no</html>".to_vec())
}

/// The host got no answer: `external` (the server is out of reach), `too_big`, `denied`...
fn fails(kind: &str) -> Reply {
    Reply::Fails(RpcError::failed(kind, format!("no answer ({kind})")))
}

/// What a fetch of `url` asks.
fn asked(url: &str) -> Req {
    let (path, params) = url.split_once('?').unwrap_or((url, ""));
    let query = form_urlencoded::parse(params.as_bytes()).find(|(k, _)| k == "query");
    Req { url: url.into(), path: path.into(), query: query.map(|(_, v)| v.into_owned()).unwrap_or_default() }
}

/// The host's answer to a fetch the server answered with `reply`, as the host gives it: the body
/// as text, or in the file `into_blob` names. None while it's held.
fn fetched(reply: Reply, into_blob: Option<PathBuf>) -> Option<Result<Value, RpcError>> {
    let (status, body) = match reply {
        Reply::Answer(status, body) => (status, body),
        Reply::Fails(e) => return Some(Err(e)),
        Reply::Held => return None,
    };
    let fetched = Fetched { status, headers: IndexMap::new(), body: None, blob: None };
    let fetched = match into_blob {
        Some(file) => std::fs::File::create_new(&file)
            .and_then(|mut f| f.write_all(&body))
            .map(|()| Fetched { blob: Some(file), ..fetched })
            .map_err(|e| RpcError::failed("invalid", e.to_string())),
        None => String::from_utf8(body)
            .map(|body| Fetched { body: Some(body), ..fetched })
            .map_err(|_| RpcError::failed("invalid", "the answer isn't text: fetch it into_blob")),
    };
    Some(fetched.map(|f| serde_json::to_value(f).unwrap()))
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

/// Where the plugin looks a release group's genres up.
fn genres_of(group: &str) -> String {
    format!("{SEARCH}release-group/{group}")
}

/// MusicBrainz, which has Bicep's "Bicep" (searched by name, with genres) and "In My View" (by
/// its ISRC), and the Cover Art Archive, which has a cover for each release group.
fn services_as_they_are(req: &Req) -> Reply {
    match req.path.strip_prefix(SEARCH) {
        Some("release/") if req.query.starts_with(r#"release:"Bicep""#) => ok(RELEASES),
        Some("release/") => no_releases(),
        Some("recording/") if req.query == format!("isrc:{ISRC}") => ok(RECORDINGS),
        Some("recording/") => ok(r#"{"created":"2026-10-13T20:42:23.943Z","count":0,"offset":0,"recordings":[]}"#),
        _ if req.path == genres_of(BICEP) => ok(GENRES),
        Some(group) if group.starts_with("release-group/") => ok(r#"{"title":"x","genres":[]}"#),
        _ if req.path.starts_with(COVERS) && req.path.ends_with("/front-500") => ok(jpeg(500, 400, false)),
        _ => status(404),
    }
}

/// The plugin served on a thread over pipes, as [`serve`] serves it, past `hello`, with a host
/// that answers its fetches as `route` has MusicBrainz and the Cover Art Archive answer them, and
/// refuses any read it calls back for (a provider is given the tracks: it needs none).
struct Session {
    host: Peer,
    served: JoinHandle<()>,
    answering: JoinHandle<()>,
    /// The reads the plugin called back for: each one's method and params.
    calls: Arc<Mutex<Vec<(String, Value)>>>,
    /// The fetches it asked for.
    got: Arc<Mutex<Vec<Req>>>,
    /// Told of each fetch once it's asked.
    heard: Receiver<()>,
    /// The scratch folders the host made for the requests, the latest last: kept until the test
    /// has read them, as the host keeps one until it has read the answer.
    scratches: Mutex<Vec<TempDir>>,
}

fn start(route: impl Fn(&Req) -> Reply + Send + 'static) -> Session {
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serve(plugin_in, plugin_out));
    let (host, incoming) = Peer::new(from_plugin, to_plugin);
    let calls: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
    let got: Arc<Mutex<Vec<Req>>> = Arc::default();
    let (tell, heard) = channel();
    let (to, reads, fetches) = (host.clone(), calls.clone(), got.clone());
    let answering = std::thread::spawn(move || {
        for msg in incoming {
            let Incoming::Request(req) = msg else { continue };
            if req.method != method::NET_FETCH {
                reads.lock().unwrap().push((req.method.clone(), req.params.clone()));
                to.respond(&req.id, Err(RpcError::failed("denied", "no reads here")));
                continue;
            }
            let fetch: Fetch = serde_json::from_value(req.params).unwrap();
            let asked = asked(&fetch.url);
            fetches.lock().unwrap().push(asked.clone());
            let _ = tell.send(());
            if let Some(answer) = fetched(route(&asked), fetch.into_blob) {
                to.respond(&req.id, answer);
            }
        }
    });
    let hello = json!({ "protocol": PROTOCOL_VERSION, "host": { "name": "locallibrary", "version": "0.1.0" } });
    host.request::<_, Value>(method::HELLO, &hello).unwrap();
    Session { host, served, answering, calls, got, heard, scratches: Mutex::default() }
}

impl Session {
    /// Asks for covers (or tags, as `kind` says) for `tracks`, with a scratch folder of its own
    /// unless `context` gives one (or, `null`, none).
    fn send(&self, kind: ProviderKind, tracks: &[TrackTags], mut context: Value) -> loclib_plugin::Pending {
        if context.get("scratch").is_none() {
            let scratch = TempDir::new().unwrap();
            context["scratch"] = json!(scratch.path());
            self.scratches.lock().unwrap().push(scratch);
        }
        let provider = match kind {
            ProviderKind::Artwork => "covers",
            ProviderKind::Metadata => "tags",
        };
        let req = json!({ "provider": provider, "kind": kind, "tracks": tracks, "context": context });
        self.host.send_request(method::PROVIDE, &req).unwrap()
    }

    fn provide_with(&self, tracks: &[TrackTags], context: Value) -> Result<Candidates, RpcError> {
        self.send(ProviderKind::Artwork, tracks, context).wait()
    }

    /// Asks for tags for `tracks`.
    fn tags(&self, tracks: &[TrackTags], context: Value) -> Result<Candidates, RpcError> {
        self.send(ProviderKind::Metadata, tracks, context).wait()
    }

    fn provide(&self, tracks: &[TrackTags]) -> Result<Candidates, RpcError> {
        self.provide_with(tracks, json!({ "drive": "USB" }))
    }

    /// The scratch folder of the latest request.
    fn scratch(&self) -> PathBuf {
        self.scratches.lock().unwrap().last().unwrap().path().into()
    }

    fn got(&self) -> Vec<Req> {
        self.got.lock().unwrap().clone()
    }

    /// The searches MusicBrainz was asked: each one's path and query.
    fn searches(&self) -> Vec<(String, String)> {
        let got = self.got().into_iter();
        got.filter_map(|g| Some((g.path.strip_prefix("https://musicbrainz.org")?.into(), g.query))).collect()
    }

    /// The covers the Cover Art Archive was asked for.
    fn covers(&self) -> Vec<String> {
        self.got()
            .into_iter()
            .filter_map(|g| Some(g.path.strip_prefix("https://coverartarchive.org")?.into()))
            .collect()
    }

    /// Closes the plugin's input: it stops. Answers the reads it called back for.
    fn stop(self) -> Vec<(String, Value)> {
        self.host.close();
        self.served.join().unwrap();
        self.answering.join().unwrap();
        Arc::try_unwrap(self.calls).unwrap().into_inner().unwrap()
    }
}

/// Track `id`'s tags, of `album` (by `artist`).
fn tags(id: u32, artist: &str, album: &str, title: &str) -> TrackTags {
    TrackTags { id, artist: artist.into(), album: album.into(), title: title.into(), ..TrackTags::default() }
}

/// A candidate found: its track, score, label and file.
type Seen = (u32, f64, String, String);

/// Each candidate's track, score, label and file, checking it's a cover from the Cover Art
/// Archive as locallibrary takes one: an image given as a file in the request's `scratch`
/// folder, by its name, a JPEG of the size players show. The folder holds those files and no
/// others.
fn found(c: &Candidates, scratch: &Path) -> Vec<Seen> {
    let mut files = Vec::new();
    let seen = c.candidates.iter().map(|c| {
        assert_eq!((c.source.as_str(), &c.metadata), (SOURCE, &None), "{c:?}");
        let blob = c.artwork.as_ref().expect("an image").blob.clone();
        assert!(!blob.contains(['/', '\\']), "a name in the folder: {blob}");
        let bytes = std::fs::read(scratch.join(&blob)).unwrap();
        assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::Jpeg);
        let image = image::load_from_memory(&bytes).unwrap();
        assert_eq!((image.width(), image.height()), (covers::SIZE, covers::SIZE));
        if !files.contains(&blob) {
            files.push(blob.clone());
        }
        (c.track, c.score, c.label.clone(), blob)
    });
    let seen: Vec<Seen> = seen.collect();
    let there = std::fs::read_dir(scratch).unwrap().filter(|f| f.as_ref().unwrap().path().is_file()).count();
    assert_eq!(there, files.len(), "{:?}", std::fs::read_dir(scratch).unwrap().collect::<Vec<_>>());
    seen
}

/// The file a release group's cover is in.
fn cover(group: &str) -> String {
    format!("cover-{group}.jpg")
}

fn kind(e: &RpcError) -> (&str, &str) {
    (e.kind().unwrap_or_default(), e.message.as_str())
}

/// It reads the tracks it's given and provides artwork and metadata, says which servers it talks
/// to and asks the host to keep to MusicBrainz's limit, and hears of drives changing. It plans
/// nothing: the host sets what's chosen.
#[test]
fn the_manifest_is_a_first_party_artwork_and_metadata_provider() {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    manifest.validate(true).unwrap();
    let capabilities: Vec<String> = manifest.capabilities.iter().map(ToString::to_string).collect();
    assert_eq!(
        capabilities,
        [
            "library:read",
            "events:drives",
            "net:musicbrainz.org",
            "net:coverartarchive.org",
            "net:archive.org",
            "net:*.archive.org"
        ]
    );
    let on = vec![EventName::DriveMounted, EventName::DriveChanged];
    assert_eq!(manifest.contributes.events, [Listener { on }]);
    assert_eq!(manifest.net, [NetLimit { host: "musicbrainz.org".into(), per_second: 1.0 }]);
    let covers = Provider { name: "covers".into(), title: SOURCE.into(), kind: ProviderKind::Artwork };
    let tags = Provider { name: "tags".into(), title: TAGS_SOURCE.into(), kind: ProviderKind::Metadata };
    assert_eq!(manifest.contributes.providers, [covers, tags]);
    assert!(manifest.contributes.operations.is_empty() && manifest.contributes.commands.is_empty());

    // Served as a real one is: it says it's this manifest, providing, sending fetches and
    // answering blobs.
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serve(plugin_in, plugin_out));
    let (host, _incoming) = Peer::new(from_plugin, to_plugin);
    let hello = json!({ "protocol": PROTOCOL_VERSION, "host": { "name": "locallibrary", "version": "0.1.0" } });
    let hello: HelloReply = host.request(method::HELLO, &hello).unwrap();
    let has = |f: &&str| hello.features.iter().any(|had| had == f);
    assert!(["providers", "events", "blobs", "net"].iter().all(has), "{:?}", hello.features);
    let described: Manifest = host.request(method::DESCRIBE, &json!({})).unwrap();
    assert_eq!(described, manifest);
    host.close();
    served.join().unwrap();
}

/// The plugin served over pipes past `hello`, told `events` in order while the host answers each
/// `drive.library` it asks with `tracks`: what it suggests, and how many times it read the library.
fn told(events: &[EventNotify], tracks: &[loclib_model::Track]) -> (Vec<HostNotify>, usize) {
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serve(plugin_in, plugin_out));
    let (host, incoming) = Peer::new(from_plugin, to_plugin);
    let hello = json!({ "protocol": PROTOCOL_VERSION, "host": { "name": "locallibrary", "version": "0.1.0" } });
    host.request::<_, Value>(method::HELLO, &hello).unwrap();
    for e in events {
        host.notify(method::EVENT_NOTIFY, e);
    }
    // One last event that suggests something, so everything before it has been handled.
    host.notify(method::EVENT_NOTIFY, &databases_changed("LAST"));
    let (mut suggested, mut reads) = (Vec::new(), 0);
    for msg in incoming {
        match msg {
            Incoming::Request(req) => {
                assert_eq!(req.method, "drive.library");
                reads += 1;
                let mut tracks = tracks.to_vec();
                if req.params["drive"] == "LAST" {
                    tracks = vec![loclib_model::Track { id: 1, ..Default::default() }];
                }
                let total = tracks.len();
                let library = loclib_model::Library { tracks, ..Default::default() };
                host.respond(&req.id, Ok(json!({ "library": library, "page": { "total": total } })));
            }
            Incoming::Notification { method: m, params } if m == method::HOST_NOTIFY => {
                let notice: HostNotify = serde_json::from_value(params).unwrap();
                if notice.action.as_ref().is_some_and(|a| a.params["drive"] == "LAST") {
                    break;
                }
                suggested.push(notice);
            }
            Incoming::Notification { .. } => {}
        }
    }
    host.close();
    served.join().unwrap();
    (suggested, reads - 1)
}

/// `drive.changed` for `drive`, its databases changed.
fn databases_changed(drive: &str) -> EventNotify {
    EventNotify {
        event: EventName::DriveChanged,
        drive: Some(drive.into()),
        payload: json!({ "changed": ["databases"] }),
    }
}

/// Once a drive's databases change, it suggests fetching covers from its own provider for the
/// tracks without artwork (the host leaves out those it suggested before). A drive whose tracks all
/// have artwork, and a change to a drive's state alone, suggest nothing: the state alone isn't
/// even read.
#[test]
fn a_drive_whose_databases_changed_has_covers_suggested_for_its_bare_tracks() {
    let track = |id: u32, artwork_id: Option<u32>| loclib_model::Track { id, artwork_id, ..Default::default() };
    let (suggested, reads) = told(&[databases_changed("USB")], &[track(1, Some(9)), track(2, None), track(3, None)]);
    let fetch = FetchAction { drive: "USB".into(), tracks: vec![2, 3], provider: Some("covers".into()) };
    let expected = HostNotify::action("new tracks have no cover art", &HostAction::ArtFetch(fetch));
    assert_eq!((suggested, reads), (vec![expected.clone()], 1));
    // The host takes it: it's on its own provider.
    let taken = expected.suggestion(&Manifest::parse(MANIFEST).unwrap()).unwrap();
    assert!(matches!(taken, Some(Suggestion::Action(HostAction::ArtFetch(_)))), "{taken:?}");
    let state = EventNotify { payload: json!({ "changed": ["state"] }), ..databases_changed("USB") };
    let (suggested, reads) = told(&[state, databases_changed("USB")], &[track(1, Some(9))]);
    assert_eq!((suggested, reads), (vec![], 1));
}

/// A drive plugged in with an export (perhaps exported on another computer) has covers suggested
/// for its bare tracks as one whose databases changed does; one without an export isn't read.
#[test]
fn a_drive_plugged_in_has_covers_suggested_for_its_bare_tracks() {
    let mounted = |has_export: bool| EventNotify {
        event: EventName::DriveMounted,
        drive: Some("USB".into()),
        payload: json!({ "name": "USB", "has_export": has_export }),
    };
    let bare = [loclib_model::Track { id: 4, ..Default::default() }];
    let (suggested, reads) = told(&[mounted(false), mounted(true)], &bare);
    let fetch = FetchAction { drive: "USB".into(), tracks: vec![4], provider: Some("covers".into()) };
    let expected = HostNotify::action("new tracks have no cover art", &HostAction::ArtFetch(fetch));
    assert_eq!((suggested, reads), (vec![expected], 1));
}

/// For any library, what it suggests is the tracks without artwork, in the library's order, the
/// first [`MAX_SUGGESTED_TRACKS`] of them, and nothing when there are none.
#[hegel::test(test_cases = 20)]
fn it_suggests_the_first_tracks_without_artwork(tc: TestCase) {
    let n = tc.draw(gs::sampled_from(vec![0, 1, 3, 40, MAX_SUGGESTED_TRACKS * 2]));
    // Every `every`th track has artwork, after the first `offset`.
    let (every, offset) =
        (tc.draw(gs::integers::<usize>().min_value(1).max_value(4)), tc.draw(gs::integers::<usize>().max_value(3)));
    let tracks: Vec<loclib_model::Track> = (0..n)
        .map(|i| {
            let artwork_id = (i >= offset && (i - offset) % every == 0).then_some(1);
            loclib_model::Track { id: i as u32 + 1, artwork_id, ..Default::default() }
        })
        .collect();
    let (suggested, _) = told(&[databases_changed("USB")], &tracks);
    let bare = tracks.iter().filter(|t| t.artwork_id.is_none()).map(|t| t.id);
    let expected: Vec<u32> = bare.take(MAX_SUGGESTED_TRACKS).collect();
    let tracks_of =
        |n: &HostNotify| serde_json::from_value::<Vec<u32>>(n.action.clone().unwrap().params["tracks"].clone());
    let got: Vec<Vec<u32>> = suggested.iter().map(|n| tracks_of(n).unwrap()).collect();
    assert_eq!(got, if expected.is_empty() { vec![] } else { vec![expected] });
}

/// Each album is looked up once (by an ISRC when a track has one), and its cover offered for its
/// tracks at the size players show, downloaded once: surer found by an ISRC than by a name.
#[test]
fn albums_get_their_covers_one_search_each() {
    let tracks = [
        tags(1, "Bicep", "Bicep", "Glue"),
        TrackTags { isrc: "GB-CFB-17-00586".into(), ..tags(3, "Young Fathers", "Cocoa Sugar", "In My View") },
        tags(4, "Young Fathers", "Cocoa Sugar", "Lord"),
        // Letter case and spacing don't make another album.
        tags(5, "BICEP", " bicep", "Opal"),
    ];
    let session = start(services_as_they_are);
    let candidates = session.provide(&tracks).unwrap();
    assert_eq!(candidates.warnings, Vec::<String>::new());
    let bicep = |t| (t, BY_NAME, "Bicep, album".to_string(), cover(BICEP));
    let cocoa = |t| (t, BY_ISRC, "Cocoa Sugar, album".to_string(), cover(COCOA_SUGAR));
    assert_eq!(found(&candidates, &session.scratch()), [bicep(1), bicep(5), cocoa(3), cocoa(4)]);
    assert_eq!(
        session.searches(),
        [
            ("/ws/2/release/".into(), r#"release:"Bicep" AND artist:"Bicep""#.into()),
            ("/ws/2/recording/".into(), format!("isrc:{ISRC}")),
        ]
    );
    assert_eq!(
        session.covers(),
        [format!("/release-group/{BICEP}/front-500"), format!("/release-group/{COCOA_SUGAR}/front-500")]
    );
    // The query in the URL, as a form encodes it.
    let urls: Vec<String> = session.got().into_iter().map(|g| g.url).collect();
    assert_eq!(urls.len(), 4);
    assert_eq!(
        urls[0],
        "https://musicbrainz.org/ws/2/release/?query=release%3A%22Bicep%22+AND+artist%3A%22Bicep%22&fmt=json"
    );
    assert_eq!(urls[2], format!("https://musicbrainz.org/ws/2/recording/?query=isrc%3A{ISRC}&fmt=json"));
    assert_eq!(session.stop(), Vec::new(), "it reads nothing");
}

/// A track without an album is looked up by its title, as its single; one without either can't
/// be.
#[test]
fn a_track_without_an_album_gets_its_singles_cover() {
    let tracks = [
        TrackTags { isrc: ISRC.into(), ..tags(1, "Young Fathers", "", "In My View") },
        tags(2, "Young Fathers", "", ""),
    ];
    let session = start(services_as_they_are);
    let candidates = session.provide(&tracks).unwrap();
    assert_eq!(found(&candidates, &session.scratch()), [(1, BY_ISRC, "In My View, single".into(), cover(IN_MY_VIEW))]);
    assert_eq!(candidates.warnings, ["track #2 has no album or title to look up"]);
    assert_eq!(session.covers(), [format!("/release-group/{IN_MY_VIEW}/front-500")]);
}

/// An ISRC MusicBrainz doesn't know, or that isn't on a release of the album's name, is looked
/// up by name instead.
#[test]
fn an_isrc_not_found_falls_back_to_the_albums_name() {
    let tracks = [
        TrackTags { isrc: "USRC17607839".into(), ..tags(1, "Bicep", "Bicep", "Glue") },
        TrackTags { isrc: ISRC.into(), ..tags(2, "Young Fathers", "Bicep", "In My View") },
        // Not an ISRC: only searched by name.
        TrackTags { isrc: "none".into(), ..tags(3, "Bicep", "Isles", "Apricots") },
    ];
    let session = start(services_as_they_are);
    let candidates = session.provide(&tracks).unwrap();
    let bicep = |t| (t, BY_NAME, "Bicep, album".to_string(), cover(BICEP));
    assert_eq!(found(&candidates, &session.scratch()), [bicep(1), bicep(2)]);
    assert_eq!(candidates.warnings, ["Bicep - Isles: not on MusicBrainz"]);
    let queries: Vec<String> = session.searches().into_iter().map(|(_, q)| q).collect();
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
    assert_eq!(session.covers().len(), 1);
}

/// An album MusicBrainz has several release groups of that name for gets a cover of each, none
/// sure, for the user to choose from; one it has none for is a warning.
#[test]
fn several_release_groups_are_each_offered_and_none_a_warning() {
    let single = "f0000000-0000-4000-8000-000000000001";
    let route = move |req: &Req| match req.query.as_str() {
        r#"release:"Bicep" AND artist:"Bicep""# => {
            let mut found: Value = serde_json::from_str(RELEASES).unwrap();
            found["releases"][1]["release-group"] = json!({ "id": single, "title": "Bicep", "primary-type": "Single" });
            ok(found.to_string())
        }
        q if q.starts_with("release:") => no_releases(),
        _ => services_as_they_are(req),
    };
    let tracks = [tags(1, "Bicep", "Bicep", "Glue"), tags(2, "", "Say \"Hi\" \\o/", "Hi")];
    let session = start(route);
    let candidates = session.provide(&tracks).unwrap();
    assert_eq!(
        found(&candidates, &session.scratch()),
        [
            (1, ONE_OF_SEVERAL, "Bicep, album".into(), cover(BICEP)),
            (1, ONE_OF_SEVERAL, "Bicep, single".into(), cover(single))
        ]
    );
    assert_eq!(candidates.warnings, ["Say \"Hi\" \\o/: not on MusicBrainz"]);
    // Quotes and backslashes are escaped in the phrase; with no artist, only the name is asked.
    assert_eq!(session.searches()[1].1, r#"release:"Say \"Hi\" \\o/""#);
    assert_eq!(session.covers().len(), 2);
}

/// Of however many release groups an album could be, the covers of the first few are offered: each
/// is a download.
#[test]
fn at_most_a_few_release_groups_are_offered() {
    let group = |n: usize| format!("f0000000-0000-4000-8000-{n:012}");
    let route = move |req: &Req| match req.path.starts_with(SEARCH) {
        true => {
            let releases: Vec<Value> = (0..MAX_GROUPS + 2)
                .map(|n| json!({ "title": "Many", "release-group": { "id": group(n), "title": format!("Many {n}") } }))
                .collect();
            ok(json!({ "releases": releases }).to_string())
        }
        false => ok(jpeg(300, 300, true)),
    };
    let session = start(route);
    let candidates = session.provide(&[tags(1, "", "Many", "Song")]).unwrap();
    let offered: Vec<Seen> =
        (0..MAX_GROUPS).map(|n| (1, ONE_OF_SEVERAL, format!("Many {n}, release"), cover(&group(n)))).collect();
    assert_eq!(found(&candidates, &session.scratch()), offered);
    assert_eq!(session.covers().len(), MAX_GROUPS);
}

/// Titles match though MusicBrainz writes curly apostrophes and Unicode hyphens; release groups
/// whose ids aren't MusicBrainz ids are passed over.
#[test]
fn titles_match_as_musicbrainz_spells_them() {
    let route = |req: &Req| {
        match req.path.strip_prefix(SEARCH) {
        Some("release/") => ok(json!({ "releases": [
            { "title": "Don\u{2019}t Stop \u{2010} Live", "release-group": { "id": "../../x", "title": "Don\u{2019}t Stop" } },
            { "title": "Don\u{2019}t  Stop \u{2010} Live", "release-group": { "id": BICEP, "title": "Don\u{2019}t Stop" } },
            { "title": "Something else", "release-group": { "id": IN_MY_VIEW, "title": "Something else" } },
            { "title": "Don't Stop - Live" },
        ] })
        .to_string()),
        _ => services_as_they_are(req),
    }
    };
    let session = start(route);
    let candidates = session.provide(&[tags(1, "Band", "don't stop - live", "Intro")]).unwrap();
    assert_eq!(
        found(&candidates, &session.scratch()),
        [(1, BY_NAME, "Don\u{2019}t Stop, release".into(), cover(BICEP))]
    );
    assert_eq!(session.covers(), [format!("/release-group/{BICEP}/front-500")]);
}

/// A cover that's missing, doesn't download (the host gets no answer) or isn't an image, or is too
/// small, is a warning, naming the release group when the album could be several; each is asked
/// for once.
#[test]
fn covers_that_cant_be_had_are_warnings() {
    let group = |n: u32| format!("f0000000-0000-4000-8000-00000000000{n}");
    let route = move |req: &Req| {
        let album = |n: u32| format!("release:\"Album {n}\"");
        // Album `n`'s releases, of these release groups.
        let releases = |n: u32, groups: &[u32]| {
            let named =
                |&g: &u32| json!({ "title": format!("Album {n}"), "release-group": { "id": group(g), "title": "x" } });
            ok(json!({ "releases": groups.iter().map(named).collect::<Vec<_>>() }).to_string())
        };
        match req.path.as_str() {
            _ if req.query.starts_with(&album(7)) => releases(7, &[1, 7]),
            p if p.starts_with(SEARCH) => {
                (1..=6).find(|&n| req.query.starts_with(&album(n))).map_or_else(no_releases, |n| releases(n, &[n]))
            }
            p if p.contains(&group(1)) => status(404),
            p if p.contains(&group(2)) => status(500),
            p if p.contains(&group(3)) => ok("<html>not an image</html>"),
            p if p.contains(&group(4)) => {
                let mut tiny = Vec::new();
                RgbImage::new(15, 300).write_to(&mut std::io::Cursor::new(&mut tiny), ImageFormat::Png).unwrap();
                ok(tiny)
            }
            p if p.contains(&group(5)) => fails("external"),
            p if p.contains(&group(6)) => fails("too_big"),
            _ => ok(jpeg(240, 240, false)),
        }
    };
    let tracks: Vec<TrackTags> = (1..=7).map(|n| tags(n, "", &format!("Album {n}"), "Song")).collect();
    let session = start(route);
    let candidates = session.provide(&tracks).unwrap();
    assert_eq!(found(&candidates, &session.scratch()), [(7, ONE_OF_SEVERAL, "x, release".into(), cover(&group(7)))]);
    assert_eq!(
        candidates.warnings,
        [
            "Album 1: the Cover Art Archive has no front cover for it",
            "Album 2: its cover didn't download: the Cover Art Archive answered 500",
            "Album 3: its cover isn't an image: The image format could not be determined",
            "Album 4: its cover is too small (15 x 300 px)",
            "Album 5: its cover didn't download: no answer (external)",
            "Album 6: its cover didn't download: no answer (too_big)",
            "Album 7, x, release: the Cover Art Archive has no front cover for it",
        ]
    );
    assert_eq!(session.covers().len(), 7, "group 1's once");
}

/// Cancelled while a fetch is under way (a search, or a cover's download), the hook stops, asking
/// for no more.
#[test]
fn cancelling_while_a_fetch_is_under_way_stops_it() {
    for held in [SEARCH, COVERS] {
        let route = move |req: &Req| match req.path.starts_with(held) {
            true => Reply::Held,
            false => services_as_they_are(req),
        };
        let session = start(route);
        let pending = session.send(ProviderKind::Artwork, &[tags(1, "Bicep", "Bicep", "Glue")], json!({}));
        let asked = if held == SEARCH { 1 } else { 2 };
        for _ in 0..asked {
            session.heard.recv().unwrap();
        }
        pending.cancel();
        let answer = pending.wait_timeout(Duration::from_secs(10)).expect("it stops");
        assert_eq!(answer.map(|_: Candidates| ()).unwrap_err(), RpcError::cancelled());
        assert_eq!(session.got().len(), asked);
    }
}

/// MusicBrainz out of reach fails the hook, as the host failed the search: no album can be
/// looked up.
#[test]
fn musicbrainz_out_of_reach_fails_it() {
    for failure in FAILURES {
        let session = start(move |_: &Req| fails(failure));
        let e = session.provide(&[tags(1, "Bicep", "Bicep", "Glue")]).unwrap_err();
        assert_eq!(kind(&e), (failure, format!("couldn't reach MusicBrainz: no answer ({failure})").as_str()));
    }
}

/// A search MusicBrainz refuses as busy is sent again, a few times, then its album is left out
/// with a warning; an answer that isn't a search result fails the hook.
#[test]
fn musicbrainz_refusing_is_tried_again_then_a_warning() {
    let refused = Arc::new(Mutex::new(0));
    let refusing = refused.clone();
    let route = move |req: &Req| match req.query.as_str() {
        q if q.contains("Junk") => ok("<html>maintenance</html>"),
        q if q.contains("Isles") => status(500),
        q if q.contains("Bicep") => status(503),
        // Busy once.
        _ if req.path.starts_with(SEARCH) && std::mem::replace(&mut *refusing.lock().unwrap(), 1) == 0 => status(503),
        _ => services_as_they_are(req),
    };
    let tracks = [
        tags(1, "Bicep", "Bicep", "Glue"),
        TrackTags { isrc: ISRC.into(), ..tags(2, "Young Fathers", "Cocoa Sugar", "Lord") },
        tags(3, "Bicep", "Isles", "Apricots"),
    ];
    let session = start(route.clone());
    let candidates = session.provide(&tracks).unwrap();
    assert_eq!(found(&candidates, &session.scratch()).len(), 1);
    assert_eq!(
        candidates.warnings,
        [
            "Bicep - Bicep: left out, as MusicBrainz answered 503",
            "Bicep - Isles: left out, as MusicBrainz answered 500"
        ]
    );
    let searches: Vec<String> = session.searches().into_iter().map(|(_, q)| q).collect();
    let bicep = r#"release:"Bicep" AND artist:"Bicep""#.to_string();
    let isrc = format!("isrc:{ISRC}");
    let isles = r#"release:"Isles" AND artist:"Bicep""#.to_string();
    assert_eq!(searches, [bicep.clone(), bicep.clone(), bicep, isrc.clone(), isrc, isles]);
    assert_eq!(*refused.lock().unwrap(), 1);

    let session = start(route);
    let e = session.provide(&[tags(1, "Bicep", "Junk", "Glue")]).unwrap_err();
    assert_eq!(e.kind(), Some("external"));
    assert!(e.message.starts_with("MusicBrainz answered what isn't a search result: "), "{e}");
}

/// Its covers go in a folder the host gives it for them: without one, it looks nothing up.
#[test]
fn it_needs_a_scratch_folder() {
    let session = start(services_as_they_are);
    let e = session.provide_with(&[tags(1, "Bicep", "Bicep", "Glue")], json!({ "scratch": null })).unwrap_err();
    assert_eq!(
        kind(&e),
        (
            "invalid_params",
            "invalid params: the host gives the plugin no scratch folder for the covers (the blobs feature)"
        )
    );
    assert_eq!(session.got().len(), 0);
    assert_eq!(session.stop(), Vec::new());
}

/// A cover it can't write to the scratch folder fails the hook: its candidates would have no image.
#[test]
fn a_cover_it_cant_write_fails_it() {
    let session = start(services_as_they_are);
    let scratch = TempDir::new().unwrap();
    // A folder in the way.
    std::fs::create_dir(scratch.path().join(cover(BICEP))).unwrap();
    let e =
        session.provide_with(&[tags(1, "Bicep", "Bicep", "Glue")], json!({ "scratch": scratch.path() })).unwrap_err();
    assert_eq!(e.kind(), Some("io"));
    assert!(e.message.starts_with(&format!("{}: ", cover(BICEP))), "{e}");
}

/// It finds covers for however many albums it's asked about: they go to the host as files, so the
/// answer doesn't grow with them.
#[test]
fn it_finds_covers_for_any_number_of_albums() {
    const MANY: u32 = 20;
    let image = jpeg(covers::SIZE, covers::SIZE, false);
    let route = move |req: &Req| match req.query.strip_prefix("release:\"Album ") {
        Some(n) => {
            let n: u32 = n.trim_end_matches('"').parse().unwrap();
            let group = json!({ "id": format!("f0000000-0000-4000-8000-{n:012}"), "title": "x" });
            ok(json!({ "releases": [{ "title": format!("Album {n}"), "release-group": group }] }).to_string())
        }
        None => ok(image.clone()),
    };
    let tracks: Vec<TrackTags> = (1..=MANY).map(|n| tags(n, "", &format!("Album {n}"), "Song")).collect();
    let session = start(route);
    let candidates = session.provide(&tracks).unwrap();
    assert_eq!(candidates.warnings, Vec::<String>::new());
    assert_eq!(found(&candidates, &session.scratch()).len(), MANY as usize);
    let images: u64 = std::fs::read_dir(session.scratch()).unwrap().map(|f| f.unwrap().metadata().unwrap().len()).sum();
    let message = serde_json::to_string(&candidates).unwrap().len() as u64;
    assert!(message * 10 < images, "an answer of {message} bytes for {images} bytes of covers");
}

/// An ISRC search MusicBrainz keeps refusing leaves the album out with a warning, with no search
/// by name after it.
#[test]
fn an_isrc_search_refused_leaves_the_album_out() {
    let route = |req: &Req| match req.query.starts_with("isrc:") {
        true => status(503),
        false => services_as_they_are(req),
    };
    let session = start(route);
    let candidates =
        session.provide(&[TrackTags { isrc: ISRC.into(), ..tags(1, "Young Fathers", "Cocoa Sugar", "Lord") }]).unwrap();
    assert!(found(&candidates, &session.scratch()).is_empty());
    assert_eq!(candidates.warnings, ["Young Fathers - Cocoa Sugar: left out, as MusicBrainz answered 503"]);
    let queries: Vec<String> = session.searches().into_iter().map(|(_, q)| q).collect();
    assert_eq!(queries, [format!("isrc:{ISRC}"), format!("isrc:{ISRC}"), format!("isrc:{ISRC}")]);
}

/// A tags candidate found: its track, score, label and tags.
type Tagged = (u32, f64, String, TrackMetadata);

/// Each candidate's track, score, label and tags, checking it's tags from MusicBrainz.
fn tagged(c: &Candidates) -> Vec<Tagged> {
    let each = c.candidates.iter().map(|c| {
        assert_eq!((c.source.as_str(), &c.artwork), (TAGS_SOURCE, &None), "{c:?}");
        (c.track, c.score, c.label.clone(), c.metadata.clone().expect("tags"))
    });
    each.collect()
}

/// Tags of an album: its title, year and label.
fn album_tags(album: &str, year: u16, label: Option<&str>) -> TrackMetadata {
    TrackMetadata {
        album: Some(album.into()),
        year: Some(year),
        label: label.map(Into::into),
        ..TrackMetadata::default()
    }
}

/// Asked for tags, each album is looked up once, as for covers, and its release's title, year and
/// label (found by name) offered for each of its tracks with its release group's top genre (the
/// first of equals), and the recording's title and artist for the track whose ISRC found it, but
/// not the ISRC, which it has. No cover is downloaded, and no scratch folder is needed.
#[test]
fn albums_get_their_tags_one_search_each() {
    let tracks = [
        tags(1, "Bicep", "Bicep", "Glue"),
        TrackTags { isrc: "GB-CFB-17-00586".into(), ..tags(3, "Young Fathers", "Cocoa Sugar", "untitled") },
        tags(4, "Young Fathers", "Cocoa Sugar", "Lord"),
        tags(5, "BICEP", " bicep", "Opal"),
    ];
    let session = start(services_as_they_are);
    let candidates = session.tags(&tracks, json!({ "drive": "USB", "scratch": null })).unwrap();
    assert_eq!(candidates.warnings, Vec::<String>::new());
    let bicep = TrackMetadata { genre: Some("electronic".into()), ..album_tags("Bicep", 2017, Some("Ninja Tune")) };
    let cocoa = album_tags("Cocoa Sugar", 2018, None);
    let in_my_view =
        TrackMetadata { title: Some("In My View".into()), artist: Some("Young Fathers".into()), ..cocoa.clone() };
    let (by_name, by_isrc) = ("Bicep, album".to_string(), "Cocoa Sugar, album".to_string());
    assert_eq!(
        tagged(&candidates),
        [
            (1, BY_NAME, by_name.clone(), bicep.clone()),
            (5, BY_NAME, by_name, bicep),
            (3, BY_ISRC, by_isrc.clone(), in_my_view),
            (4, BY_ISRC, by_isrc, cocoa),
        ]
    );
    assert_eq!(
        session.searches(),
        [
            ("/ws/2/release/".into(), r#"release:"Bicep" AND artist:"Bicep""#.into()),
            (format!("/ws/2/release-group/{BICEP}"), String::new()),
            ("/ws/2/recording/".into(), format!("isrc:{ISRC}")),
            (format!("/ws/2/release-group/{COCOA_SUGAR}"), String::new()),
        ]
    );
    assert_eq!(session.got()[1].url, format!("{}?inc=genres&fmt=json", genres_of(BICEP)));
    assert_eq!(session.covers(), Vec::<String>::new());
    assert_eq!(session.stop(), Vec::new(), "it reads nothing");
}

/// A recording's artist is its credit's names joined as the credit joins them. Its title and
/// artist are only the track's whose ISRC found it; a genre MusicBrainz won't give is a warning.
#[test]
fn a_recordings_credit_is_for_its_track_alone() {
    let route = |req: &Req| match req.path.strip_prefix(SEARCH) {
        Some("recording/") => {
            let mut found: Value = serde_json::from_str(RECORDINGS).unwrap();
            found["recordings"][0]["artist-credit"] =
                json!([{ "name": "Young Fathers", "joinphrase": " & " }, { "name": "Mitchell", "artist": {} }]);
            ok(found.to_string())
        }
        Some(group) if group.starts_with("release-group/") => status(404),
        _ => services_as_they_are(req),
    };
    let tracks = [
        TrackTags { isrc: ISRC.into(), ..tags(1, "Young Fathers", "Cocoa Sugar", "In My View") },
        TrackTags { isrc: "GBCFB1700587".into(), ..tags(2, "Young Fathers", "Cocoa Sugar", "Lord") },
    ];
    let session = start(route);
    let candidates = session.tags(&tracks, json!({})).unwrap();
    let cocoa = album_tags("Cocoa Sugar", 2018, None);
    let in_my_view = TrackMetadata {
        title: Some("In My View".into()),
        artist: Some("Young Fathers & Mitchell".into()),
        ..cocoa.clone()
    };
    let label = "Cocoa Sugar, album".to_string();
    assert_eq!(tagged(&candidates), [(1, BY_ISRC, label.clone(), in_my_view), (2, BY_ISRC, label, cocoa)]);
    assert_eq!(candidates.warnings, ["Young Fathers - Cocoa Sugar: no genre, as MusicBrainz answered 404"]);
    assert_eq!(std::fs::read_dir(session.scratch()).unwrap().count(), 0, "nothing downloaded");
}

/// An album that could be several release groups gets each one's tags, from the earliest of its
/// releases' years and the first label one names (`[no label]` is none), with no genre: none is
/// surely the album's. Two albums that are one release group have it looked up once, for its
/// genre and the year it first came out, which goes before the search's.
#[test]
fn several_release_groups_each_give_tags_without_a_genre() {
    let single = "f0000000-0000-4000-8000-000000000001";
    let route = move |req: &Req| match req.query.as_str() {
        r#"release:"Bicep" AND artist:"Bicep""# => {
            let mut found: Value = serde_json::from_str(RELEASES).unwrap();
            found["releases"][1]["release-group"] = json!({ "id": single, "title": "Bicep", "primary-type": "Single" });
            found["releases"][1]["label-info"] =
                json!([{ "label": { "name": "[no label]" } }, { "catalog-number": "X" }]);
            found["releases"][2]["title"] = json!("Bicep");
            found["releases"][2]["release-group"] = found["releases"][0]["release-group"].clone();
            found["releases"][2]["date"] = json!("2016");
            found["releases"][2]["label-info"] = json!([{ "label": { "name": "Feel My Bicep" } }]);
            ok(found.to_string())
        }
        _ => services_as_they_are(req),
    };
    let tracks = [tags(1, "Bicep", "Bicep", "Glue"), tags(2, "", "Isles", "Atlas")];
    let session = start(route);
    let candidates = session.tags(&tracks, json!({})).unwrap();
    assert_eq!(
        tagged(&candidates),
        [
            (1, ONE_OF_SEVERAL, "Bicep, album".into(), album_tags("Bicep", 2016, Some("Ninja Tune"))),
            (1, ONE_OF_SEVERAL, "Bicep, single".into(), album_tags("Bicep", 2017, None)),
        ]
    );
    assert_eq!(candidates.warnings, ["Isles: not on MusicBrainz"]);
    assert!(session.searches().iter().all(|(path, _)| !path.contains("release-group")), "{:?}", session.searches());

    // "Bicep" by Bicep, and by no one: one release group, looked up once.
    let route = |req: &Req| match req.path == genres_of(BICEP) {
        true => {
            let mut looked: Value = serde_json::from_str(GENRES).unwrap();
            looked["first-release-date"] = json!("2015-06-12");
            ok(looked.to_string())
        }
        false => services_as_they_are(req),
    };
    let session = start(route);
    let candidates = session.tags(&[tags(1, "Bicep", "Bicep", "Glue"), tags(2, "", "Bicep", "Atlas")], json!({}));
    let found: Vec<(Option<String>, Option<u16>)> =
        tagged(&candidates.unwrap()).into_iter().map(|t| (t.3.genre, t.3.year)).collect();
    assert_eq!(found, vec![(Some("electronic".to_string()), Some(2015)); 2]);
    let looked_up = session.got().into_iter().filter(|g| g.path == genres_of(BICEP)).count();
    assert_eq!(looked_up, 1);
}

/// A track without an album, looked up by its title, gets tags only from a single of that name:
/// an album of that name is another record (its title track's), which is a warning, and isn't
/// looked up.
#[test]
fn a_track_without_an_album_gets_only_a_singles_tags() {
    let tracks =
        [tags(1, "Bicep", "", "Bicep"), TrackTags { isrc: ISRC.into(), ..tags(2, "Young Fathers", " ", "In My View") }];
    let session = start(services_as_they_are);
    let candidates = session.tags(&tracks, json!({})).unwrap();
    let in_my_view = TrackMetadata {
        title: Some("In My View".into()),
        artist: Some("Young Fathers".into()),
        album: Some("In My View".into()),
        ..TrackMetadata::default()
    };
    assert_eq!(tagged(&candidates), [(2, BY_ISRC, "In My View, single".into(), in_my_view)]);
    let warned = "Bicep - Bicep: track #1 has no album, and MusicBrainz has no single of that name";
    assert_eq!(candidates.warnings, [warned]);
    let looked_up: Vec<String> =
        session.searches().into_iter().map(|(path, _)| path).filter(|p| p.contains("release-group")).collect();
    assert_eq!(looked_up, [format!("/ws/2/release-group/{IN_MY_VIEW}")]);
}

// Properties: drawn tracks against drawn services, the candidates foreseen from what was drawn.

/// The release groups drawn searches find, by index: shaped as MusicBrainz's ids are, so their
/// covers are asked for.
const GROUPS: [&str; 3] = [
    "a0000000-0000-4000-8000-000000000000",
    "a0000000-0000-4000-8000-000000000001",
    "a0000000-0000-4000-8000-000000000002",
];
/// Their primary types: a track without an album takes tags only from the single.
const KINDS: [Option<&str>; 3] = [Some("Album"), Some("Single"), None];
/// How many albums drawn tracks are of: `Album 0`, `Album 1`...
const ALBUMS: usize = 3;

/// The ISRC of album `k`'s tracks, and of another of its tracks, which MusicBrainz has on the
/// same releases.
fn isrc_of(k: usize) -> String {
    format!("GBAAA260000{k}")
}

fn other_isrc_of(k: usize) -> String {
    format!("GBAAA260001{k}")
}

/// A drawn track and what it's known to be: of which album (`k`, by its name in some letter case
/// and spacing, as its album or as the title of a single), by which artist (spellings of one are
/// one), its ISRC, when its tag is one, and whether it has no album tag (a single's title).
#[derive(Debug, Clone)]
struct Drawn {
    tags: TrackTags,
    album: Option<usize>,
    artist: usize,
    isrc: Option<String>,
    single: bool,
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
        (String::new(), None),
        (isrc_of(k), Some(isrc_of(k))),
        (format!("gb-aaa-26-0000{k}"), Some(isrc_of(k))),
        (format!("GB AAA 26 0000{k}"), Some(isrc_of(k))),
        (other_isrc_of(k), Some(other_isrc_of(k))),
        (format!("GBAAA26000{k}"), None),
        ("none".to_string(), None),
    ];
    let (isrc_tag, isrc) = tc.draw(gs::sampled_from(isrcs));
    let single = album_tag.is_empty();
    let tags = TrackTags {
        album: album_tag,
        album_artist: if album_artist { "Various Artists".into() } else { String::new() },
        isrc: isrc_tag,
        ..tags(id, artist_tag, "", &title)
    };
    Drawn { tags, album, artist, isrc, single }
}

/// How MusicBrainz answers a request: `503` (busy) `busy` times, then `answer`.
#[derive(Debug, Clone)]
struct Search {
    busy: usize,
    answer: Answer,
}

#[derive(Debug, Clone)]
enum Answer {
    /// A search's releases named as the album is, beside some that aren't (named otherwise, not
    /// of a release group, or of one whose id isn't MusicBrainz's).
    Releases(Vec<Rel>),
    /// A release group's lookup: its genres (each one's name and votes), and the date it first
    /// came out, if MusicBrainz knows.
    Lookup(Vec<(&'static str, u32)>, Option<&'static str>),
    Status(u16),
    /// A page that isn't a search result.
    Garbage,
    /// The host gets no answer, failing so.
    Fails(&'static str),
}

/// A release named as the album is: of release group `group` (by index; a release group can be
/// on several), out on `date`, naming these labels (none: a catalog number alone).
#[derive(Debug, Clone)]
struct Rel {
    group: usize,
    date: Option<&'static str>,
    labels: Vec<Option<&'static str>>,
}

#[hegel::test_helper]
fn drawn_release(tc: &TestCase, group: usize) -> Rel {
    let date = tc.draw(gs::sampled_from(vec![None, Some("2017"), Some("2018-03"), Some("2016-09-01")]));
    let label = || gs::sampled_from(vec![Some("Ninja Tune"), Some("XL"), Some("[no label]"), None]);
    let labels = (0..tc.draw(gs::integers::<usize>().max_value(2))).map(|_| tc.draw(label())).collect();
    Rel { group, date, labels }
}

/// A search's releases: mostly the album, on its own (named once, or on two releases), so its
/// cover or genre is asked for.
#[hegel::test_helper]
fn drawn_releases(tc: &TestCase) -> Answer {
    let group = || gs::integers::<usize>().max_value(GROUPS.len() - 1);
    let groups = match tc.draw(gs::sampled_from(vec!["one", "one", "one", "one", "none", "several"])) {
        "one" => vec![tc.draw(group()); tc.draw(gs::integers::<usize>().min_value(1).max_value(2))],
        "none" => Vec::new(),
        _ => tc.draw(gs::vecs(group()).min_size(2).max_size(3)),
    };
    Answer::Releases(groups.into_iter().map(|g| drawn_release(tc, g)).collect())
}

#[hegel::test_helper]
fn drawn_lookup(tc: &TestCase) -> Answer {
    let genre = || gs::sampled_from(vec!["electronic", "house", "techno"]);
    let votes = || gs::integers::<u32>().min_value(1).max_value(3);
    let genres = (0..tc.draw(gs::integers::<usize>().max_value(3))).map(|_| (tc.draw(genre()), tc.draw(votes())));
    let genres = genres.collect();
    Answer::Lookup(genres, tc.draw(gs::sampled_from(vec![None, Some("2015"), Some("2019-04-01")])))
}

/// A request MusicBrainz answers with what `found` draws, or refuses, garbles or doesn't answer.
#[hegel::test_helper]
fn drawn_search(tc: &TestCase, found: fn(&TestCase) -> Answer) -> Search {
    let busy = tc.draw(gs::sampled_from(vec![0usize, 0, 0, 1, 2, 3]));
    let answer =
        match tc.draw(gs::sampled_from(vec!["found", "found", "found", "found", "refused", "garbage", "fails"])) {
            "found" => found(tc),
            "refused" => Answer::Status(tc.draw(gs::sampled_from(vec![404u16, 429, 500]))),
            "garbage" => Answer::Garbage,
            _ => Answer::Fails(tc.draw(gs::sampled_from(FAILURES.to_vec()))),
        };
    Search { busy, answer }
}

/// The ways the host fails to get an answer.
const FAILURES: [&str; 3] = ["external", "too_big", "denied"];

/// How the Cover Art Archive gives a release group's front cover.
#[derive(Debug, Clone)]
enum Cover {
    /// A JPEG (or PNG) of this size, and whether the scratch folder has a folder where the plugin
    /// would write it.
    Image {
        width: u32,
        height: u32,
        png: bool,
        unwritable: bool,
    },
    Status(u16),
    Garbage,
    Fails(&'static str),
}

#[hegel::test_helper]
fn drawn_cover(tc: &TestCase) -> Cover {
    let side = |from: u32, to: u32| gs::integers::<u32>().min_value(from).max_value(to);
    let kinds = vec!["image", "image", "tiny", "image", "tiny", "refused", "garbage", "fails", "fails"];
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
            // Rarely: it fails the hook.
            let unwritable = tc.draw(gs::weighted_booleans(0.05));
            Cover::Image { width, height, png, unwritable }
        }
        "refused" => Cover::Status(tc.draw(gs::sampled_from(vec![404u16, 500, 503]))),
        "garbage" => Cover::Garbage,
        _ => Cover::Fails(tc.draw(gs::sampled_from(FAILURES.to_vec()))),
    }
}

/// The services drawn: each album's release and recording searches and its recording's artist
/// credit (each artist's name and what joins it to the next), and each release group's genres
/// and cover.
#[derive(Debug, Clone)]
struct Answers {
    releases: Vec<Search>,
    recordings: Vec<Search>,
    credits: Vec<Vec<(&'static str, &'static str)>>,
    genres: Vec<Search>,
    covers: Vec<Cover>,
}

#[hegel::test_helper]
fn drawn_answers(tc: &TestCase) -> Answers {
    let artist = || gs::sampled_from(vec!["Young Fathers", "Bicep", "Romy"]);
    let join = || gs::sampled_from(vec!["", " & ", " feat. "]);
    let credit = || {
        let artists = tc.draw(gs::integers::<usize>().max_value(2));
        (0..artists).map(|_| (tc.draw(artist()), tc.draw(join()))).collect()
    };
    Answers {
        releases: (0..ALBUMS).map(|_| drawn_search(tc, drawn_releases)).collect(),
        recordings: (0..ALBUMS).map(|_| drawn_search(tc, drawn_releases)).collect(),
        credits: (0..ALBUMS).map(|_| credit()).collect(),
        genres: (0..GROUPS.len()).map(|_| drawn_search(tc, drawn_lookup)).collect(),
        covers: (0..GROUPS.len()).map(|_| drawn_cover(tc)).collect(),
    }
}

/// How MusicBrainz spells album `k`, which is as the tracks do but for letter case and spacing.
fn spelled_by_musicbrainz(k: usize) -> String {
    format!("album  {k}")
}

/// The title of the recording album `k`'s ISRCs name.
fn recording_of(k: usize) -> String {
    format!("Song {k}")
}

/// A search's answer as MusicBrainz gives it: a release search's (its releases naming their
/// labels), or a recording search's naming the releases each recording is on, with its artist
/// `credit`.
fn search_result(k: usize, rels: &[Rel], credit: Option<&[(&str, &str)]>) -> String {
    let named = spelled_by_musicbrainz(k);
    let group = |g: usize| json!({ "id": GROUPS[g], "title": format!("Group {g}"), "primary-type": KINDS[g] });
    let release = |r: &Rel| {
        let mut release = json!({ "title": named, "release-group": group(r.group) });
        if let Some(date) = r.date {
            release["date"] = json!(date);
        }
        if credit.is_none() {
            let info = |l: &Option<&str>| match l {
                Some(name) => {
                    json!({ "catalog-number": "CAT 1", "label": { "id": "b0000000-0000-4000-8000-000000000000", "name": name } })
                }
                None => json!({ "catalog-number": "CAT 1" }),
            };
            release["label-info"] = r.labels.iter().map(info).collect();
        }
        release
    };
    let mut releases: Vec<Value> = rels.iter().map(release).collect();
    releases.extend([
        json!({ "title": "Something else", "release-group": group(0), "date": "1999" }),
        json!({ "title": named }),
        json!({ "title": named, "release-group": { "id": "../../x", "title": "x" } }),
    ]);
    match credit {
        Some(credit) => {
            let credit: Vec<Value> =
                credit.iter().map(|(name, join)| json!({ "name": name, "joinphrase": join })).collect();
            let recording = json!({ "title": recording_of(k), "artist-credit": credit, "releases": releases });
            json!({ "recordings": [recording, { "title": "Other" }] }).to_string()
        }
        None => json!({ "releases": releases }).to_string(),
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
        // Which album: the last digit of the name searched for, or of the ISRC; or which release
        // group, by its id.
        let album = |of: &str| of.chars().last().and_then(|c| c.to_digit(10)).unwrap() as usize;
        let group = |id: &str| GROUPS.iter().position(|known| *known == id).unwrap();
        let search = match req.path.strip_prefix(SEARCH) {
            Some("release/") => Some((&answers.releases, None, album(req.query.split('"').nth(1).unwrap()))),
            Some("recording/") => {
                let k = album(&req.query);
                Some((&answers.recordings, Some(answers.credits[k].as_slice()), k))
            }
            Some(lookup) => Some((&answers.genres, None, group(lookup.strip_prefix("release-group/").unwrap()))),
            None => None,
        };
        if let Some((searches, credit, k)) = search {
            let search = &searches[k];
            let times = {
                let mut asked = asked.lock().unwrap();
                asked.push(req.url.clone());
                asked.iter().filter(|url| **url == req.url).count()
            };
            if times <= search.busy {
                return status(503);
            }
            return match &search.answer {
                Answer::Releases(rels) => ok(search_result(k, rels, credit)),
                Answer::Lookup(genres, date) => {
                    let genres: Vec<Value> =
                        genres.iter().map(|(name, count)| json!({ "name": name, "count": count })).collect();
                    // MusicBrainz says an empty date when it doesn't know one.
                    let date = date.unwrap_or_default();
                    let looked = json!({ "id": GROUPS[k], "first-release-date": date, "genres": genres });
                    ok(looked.to_string())
                }
                Answer::Status(code) => status(*code),
                Answer::Garbage => ok("<html>maintenance</html>"),
                Answer::Fails(kind) => fails(kind),
            };
        }
        let id = req.path.strip_prefix(COVERS).and_then(|p| p.strip_suffix("/front-500")).unwrap();
        match answers.covers[group(id)] {
            Cover::Image { width, height, png, .. } => ok(picture(width, height, png)),
            Cover::Status(code) => status(code),
            Cover::Garbage => ok("<html>not an image</html>"),
            Cover::Fails(kind) => fails(kind),
        }
    }
}

/// The label of release group `g`'s candidates, as drawn searches name it.
fn label_of(g: usize) -> String {
    let kind = KINDS[g].unwrap_or("release").to_lowercase();
    format!("Group {g}, {kind}")
}

/// An error the hook fails with: its kind, and how its message starts.
type Refusal = (&'static str, String);

/// The candidates foreseen (each cover's track, score, label and file, and each tags' track,
/// score, label and tags) and the warnings.
type Foreseen = (Vec<Seen>, Vec<Tagged>, Vec<String>);

/// What asking about tracks should come to: the candidates and warnings, or the error it fails
/// with, with the fetches it asks for (each one's URL but its query, and its `query` param), in
/// order.
struct Outcome {
    found: Result<Foreseen, Refusal>,
    fetches: Vec<(String, String)>,
}

/// An album as the plugin looks it up: shown as its first track tags it, searched by its first
/// ISRC, with each track's id, ISRC, and whether it has no album tag.
struct Looked {
    k: usize,
    artist: String,
    name: String,
    isrc: Option<String>,
    tracks: Vec<(u32, Option<String>, bool)>,
}

impl Looked {
    fn shown(&self) -> String {
        match self.artist.is_empty() {
            true => self.name.clone(),
            false => format!("{} - {}", self.artist, self.name),
        }
    }
}

/// What MusicBrainz answers a request as `search` has it: what it found, or the status it was
/// refused with (after trying again while it's busy); or the error the hook fails with, `what`
/// it should have answered saying.
fn ask<'a>(
    fetches: &mut Vec<(String, String)>,
    path: String,
    query: String,
    search: &'a Search,
    what: &str,
) -> Result<Result<&'a Answer, u16>, Refusal> {
    for tries in 1..=3 {
        fetches.push((path.clone(), query.clone()));
        let times = fetches.iter().filter(|a| a.0 == path && a.1 == query).count();
        if times <= search.busy {
            match tries < 3 {
                true => continue,
                false => return Ok(Err(503)),
            }
        }
        return match &search.answer {
            Answer::Status(code) => Ok(Err(*code)),
            Answer::Garbage => Err(("external", format!("MusicBrainz answered what isn't a {what}: "))),
            Answer::Fails(kind) => Err((*kind, format!("couldn't reach MusicBrainz: no answer ({kind})"))),
            found => Ok(Ok(found)),
        };
    }
    unreachable!()
}

/// A release group looked up: its genre and first year, or the status MusicBrainz refused it with.
type LookedUp = Result<(Option<String>, Option<u16>), u16>;

/// What asking the provider of `kind` about `drawn`'s tracks comes to, on the services `answers`
/// stands for.
fn foresee(answers: &Answers, drawn: &[Drawn], kind: ProviderKind) -> Outcome {
    let mut warned = Vec::new();
    let mut albums: Vec<((usize, usize), Looked)> = Vec::new();
    for d in drawn {
        let t = &d.tags;
        let Some(k) = d.album else {
            warned.push(format!("track #{} has no album or title to look up", t.id));
            continue;
        };
        let at = match albums.iter().position(|(key, _)| *key == (d.artist, k)) {
            Some(at) => at,
            None => {
                let tagged =
                    |tags: [&str; 2]| tags.into_iter().map(str::trim).find(|s| !s.is_empty()).unwrap_or("").to_string();
                let (artist, name) = (tagged([&t.album_artist, &t.artist]), tagged([&t.album, &t.title]));
                albums.push(((d.artist, k), Looked { k, artist, name, isrc: None, tracks: Vec::new() }));
                albums.len() - 1
            }
        };
        let album = &mut albums[at].1;
        album.isrc = album.isrc.take().or(d.isrc.clone());
        album.tracks.push((t.id, d.isrc.clone(), d.single));
    }

    let mut fetches: Vec<(String, String)> = Vec::new();
    // Each cover downloaded, by release group: whether there's one, or why not; and each release
    // group looked up: its genre and first year, or the status it was refused with.
    let mut got: Vec<(usize, Result<(), String>)> = Vec::new();
    let mut lookups: Vec<(usize, LookedUp)> = Vec::new();
    let (mut found, mut tagged): (Vec<Seen>, Vec<Tagged>) = Default::default();
    let releases = |answer: &Answer| match answer {
        Answer::Releases(rels) => rels.clone(),
        other => panic!("a search answers releases, not {other:?}"),
    };
    let mut fail = None;
    'albums: for (_, album) in &albums {
        let shown = album.shown();
        let (mut rels, mut by_isrc) = (Ok(Vec::new()), false);
        if let Some(isrc) = &album.isrc {
            let path = format!("{SEARCH}recording/");
            match ask(&mut fetches, path, format!("isrc:{isrc}"), &answers.recordings[album.k], "search result") {
                Ok(answer) => {
                    rels = answer.map(releases);
                    by_isrc = rels.as_ref().is_ok_and(|r| !r.is_empty());
                }
                Err(e) => {
                    fail = Some(e);
                    break;
                }
            }
        }
        if rels.as_ref().is_ok_and(Vec::is_empty) {
            let mut query = format!("release:\"{}\"", album.name);
            if !album.artist.is_empty() {
                query = format!("{query} AND artist:\"{}\"", album.artist);
            }
            match ask(&mut fetches, format!("{SEARCH}release/"), query, &answers.releases[album.k], "search result") {
                Ok(answer) => rels = answer.map(releases),
                Err(e) => {
                    fail = Some(e);
                    break;
                }
            }
        }
        let rels = match rels {
            Err(code) => {
                warned.push(format!("{shown}: left out, as MusicBrainz answered {code}"));
                continue;
            }
            Ok(rels) if rels.is_empty() => {
                warned.push(format!("{shown}: not on MusicBrainz"));
                continue;
            }
            Ok(rels) => rels,
        };
        // Each release group once, in order, with the earliest year of its releases and the first
        // label they name (only a release search's name any).
        let mut groups: Vec<(usize, Option<u16>, Option<&str>)> = Vec::new();
        for r in &rels {
            let year = r.date.map(|d| d[..4].parse().unwrap());
            let label = r.labels.iter().flatten().copied().find(|l| *l != "[no label]").filter(|_| !by_isrc);
            match groups.iter_mut().find(|(g, ..)| *g == r.group) {
                Some((_, had_year, had_label)) => {
                    *had_year = [*had_year, year].into_iter().flatten().min();
                    *had_label = had_label.or(label);
                }
                None => groups.push((r.group, year, label)),
            }
        }
        let score = match (groups.len(), by_isrc) {
            (1, true) => BY_ISRC,
            (1, false) => BY_NAME,
            _ => ONE_OF_SEVERAL,
        };
        if kind == ProviderKind::Metadata {
            // A track without an album takes only the single's tags.
            let takes = |single: bool, g: usize| !single || KINDS[g] == Some("Single");
            let looked_up = match groups.as_slice() {
                [(g, ..)] if album.tracks.iter().any(|t| takes(t.2, *g)) => {
                    if !lookups.iter().any(|(had, _)| had == g) {
                        let path = format!("{SEARCH}release-group/{}", GROUPS[*g]);
                        match ask(&mut fetches, path, String::new(), &answers.genres[*g], "release group") {
                            Ok(Ok(Answer::Lookup(voted, date))) => {
                                let most = voted.iter().map(|(_, votes)| *votes).max();
                                let top = voted.iter().find(|(_, votes)| Some(*votes) == most);
                                let year = date.map(|d| d[..4].parse().unwrap());
                                lookups.push((*g, Ok((top.map(|(name, _)| name.to_string()), year))));
                            }
                            Ok(Ok(other)) => panic!("a lookup answers a release group, not {other:?}"),
                            Ok(Err(code)) => lookups.push((*g, Err(code))),
                            Err(e) => {
                                fail = Some(e);
                                break 'albums;
                            }
                        }
                    }
                    match &lookups.iter().find(|(had, _)| had == g).unwrap().1 {
                        Ok(looked_up) => Some(looked_up.clone()),
                        Err(code) => {
                            warned.push(format!("{shown}: no genre, as MusicBrainz answered {code}"));
                            None
                        }
                    }
                }
                _ => None,
            };
            let (genre, first_year) = looked_up.unwrap_or_default();
            for (t, ..) in album.tracks.iter().filter(|t| !groups.iter().any(|(g, ..)| takes(t.2, *g))) {
                warned.push(format!("{shown}: track #{t} has no album, and MusicBrainz has no single of that name"));
            }
            let artist: String = answers.credits[album.k].iter().flat_map(|(name, join)| [*name, *join]).collect();
            for &(g, year, label) in &groups {
                for (t, isrc, _) in album.tracks.iter().filter(|t| takes(t.2, g)) {
                    let recording = by_isrc && *isrc == album.isrc;
                    let tags = TrackMetadata {
                        title: recording.then(|| recording_of(album.k)),
                        artist: recording.then(|| artist.clone()),
                        album: Some(spelled_by_musicbrainz(album.k)),
                        year: first_year.or(year),
                        genre: genre.clone(),
                        label: label.map(Into::into),
                        isrc: None,
                    };
                    tagged.push((*t, score, label_of(g), tags));
                }
            }
            continue;
        }
        for &(g, ..) in &groups {
            if !got.iter().any(|(had, _)| *had == g) {
                fetches.push((format!("{COVERS}{}/front-500", GROUPS[g]), String::new()));
                let cover = match answers.covers[g] {
                    Cover::Image { width, height, .. } if width < 16 || height < 16 => {
                        Err(format!("its cover is too small ({width} x {height} px)"))
                    }
                    Cover::Image { unwritable: true, .. } => {
                        fail = Some(("io", format!("{}: ", cover(GROUPS[g]))));
                        break 'albums;
                    }
                    Cover::Image { .. } => Ok(()),
                    Cover::Status(404) => Err("the Cover Art Archive has no front cover for it".into()),
                    Cover::Status(code) => {
                        Err(format!("its cover didn't download: the Cover Art Archive answered {code}"))
                    }
                    Cover::Garbage => Err("its cover isn't an image: The image format could not be determined".into()),
                    Cover::Fails(kind) => Err(format!("its cover didn't download: no answer ({kind})")),
                };
                got.push((g, cover));
            }
            match &got.iter().find(|(had, _)| *had == g).unwrap().1 {
                Ok(()) => found.extend(album.tracks.iter().map(|&(t, ..)| (t, score, label_of(g), cover(GROUPS[g])))),
                Err(why) if groups.len() > 1 => warned.push(format!("{shown}, {}: {why}", label_of(g))),
                Err(why) => warned.push(format!("{shown}: {why}")),
            }
        }
    }
    match fail {
        Some(e) => Outcome { found: Err(e), fetches },
        None => Outcome { found: Ok((found, tagged, warned)), fetches },
    }
}

/// For any tracks asked about (in albums or singles, by artists spelled several ways, with ISRCs
/// or not), of either provider, and however MusicBrainz and the Cover Art Archive answer (found,
/// not found, several, busy, refusing, garbage, out of reach; covers missing, broken, tiny, or not
/// to be had, or that can't be written; genres and first dates or none, or not to be had), the
/// candidates are what their answers make them: each album looked up once (by ISRC first), surely
/// when it's the only release group, more so by an ISRC. For covers, each release group's cover
/// downloaded once, a file in the scratch folder offered for each of the album's tracks, and a
/// warning for each album with none; without a scratch folder, nothing is looked up. For tags,
/// each release group's album, year and label (by name), its genre and first year looked up once
/// when it's the only one, and the recording's title and artist for the tracks with the ISRC
/// that found it; a track without an album gets only a single's, else a warning. No file is
/// written.
#[hegel::test(test_cases = 60)]
fn any_tracks_get_what_the_services_answer(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>().max_value(8));
    let drawn: Vec<Drawn> = (1..=n).map(|id| drawn_track(&tc, id)).collect();
    let answers = drawn_answers(&tc);
    let kind = tc.draw(gs::sampled_from(vec![ProviderKind::Artwork, ProviderKind::Metadata]));
    // The scratch folder the host makes (rarely none), with a folder in the way of the covers
    // that can't be written.
    let given = tc.draw(gs::weighted_booleans(0.95));
    let scratch = TempDir::new().unwrap();
    for (drawn, id) in answers.covers.iter().zip(GROUPS) {
        if let Cover::Image { unwritable: true, .. } = drawn {
            std::fs::create_dir(scratch.path().join(cover(id))).unwrap();
        }
    }
    let context = match given {
        true => json!({ "scratch": scratch.path() }),
        false => json!({ "scratch": null }),
    };

    let session = start(answering(answers.clone()));
    let tracks: Vec<TrackTags> = drawn.iter().map(|d| d.tags.clone()).collect();
    let provided = session.send(kind, &tracks, context).wait();
    let foreseen = match (given, kind) {
        (false, ProviderKind::Artwork) => {
            let refused =
                "invalid params: the host gives the plugin no scratch folder for the covers (the blobs feature)";
            Outcome { found: Err(("invalid_params", refused.into())), fetches: Vec::new() }
        }
        _ => foresee(&answers, &drawn, kind),
    };
    came_to(session, &provided, &foreseen, kind, scratch.path());
}

/// Checks that asking the provider of `kind` came to what was `foreseen`: the candidates (covers
/// in `scratch`, or tags, with none there) and warnings, or the error, and the fetches asked for,
/// with nothing read.
fn came_to(
    session: Session,
    provided: &Result<Candidates, RpcError>,
    foreseen: &Outcome,
    kind: ProviderKind,
    scratch: &Path,
) {
    match (provided, &foreseen.found) {
        (Ok(candidates), Ok((seen, foreseen_tags, warned))) => {
            match kind {
                ProviderKind::Artwork => assert_eq!(&found(candidates, scratch), seen),
                ProviderKind::Metadata => assert_eq!(&tagged(candidates), foreseen_tags),
            }
            assert_eq!(&candidates.warnings, warned);
        }
        (Err(e), Err((kind, start))) => {
            assert_eq!(e.kind(), Some(*kind), "{e:?}");
            assert!(e.message.starts_with(start.as_str()), "{e:?}");
        }
        (provided, foreseen) => panic!("provided {provided:?}, not {foreseen:?}"),
    }
    if kind == ProviderKind::Metadata {
        let files = std::fs::read_dir(scratch).unwrap().filter(|f| f.as_ref().unwrap().path().is_file());
        assert_eq!(files.count(), 0, "it downloads no cover for tags");
    }
    let fetches: Vec<(String, String)> = session.got().into_iter().map(|g| (g.path, g.query)).collect();
    assert_eq!(fetches, foreseen.fetches);
    assert_eq!(session.stop(), Vec::new(), "it reads nothing");
}

/// However many albums are one release group (one name, by different artists), however
/// MusicBrainz answers its lookup, it's looked up once (tried again while MusicBrainz is busy):
/// each album gets its genre and first year, or a warning.
#[hegel::test(test_cases = 20)]
fn albums_of_one_release_group_have_it_looked_up_once(tc: TestCase) {
    let artists = ["Bicep", "Young Fathers", "", "Romy"];
    let n = tc.draw(gs::integers::<usize>().min_value(2).max_value(artists.len()));
    let drawn: Vec<Drawn> = (0..n)
        .map(|i| Drawn {
            tags: tags(i as u32 + 1, artists[i], "Album 0", "Song"),
            album: Some(0),
            artist: i,
            isrc: None,
            single: false,
        })
        .collect();
    let g = tc.draw(gs::integers::<usize>().max_value(GROUPS.len() - 1));
    let mut answers = drawn_answers(&tc);
    answers.releases[0] = Search { busy: 0, answer: Answer::Releases(vec![drawn_release(&tc, g)]) };
    let foreseen = foresee(&answers, &drawn, ProviderKind::Metadata);
    let looked_up = foreseen.fetches.iter().filter(|(path, _)| *path == genres_of(GROUPS[g])).count();
    assert_eq!(looked_up, (answers.genres[g].busy + 1).min(3), "foreseen once");

    let session = start(answering(answers));
    let tracks: Vec<TrackTags> = drawn.into_iter().map(|d| d.tags).collect();
    let provided = session.send(ProviderKind::Metadata, &tracks, json!({})).wait();
    let scratch = session.scratch();
    came_to(session, &provided, &foreseen, ProviderKind::Metadata, &scratch);
}

/// Cancelled while a fetch is under way, whichever it is (a search, a genre's lookup or a cover's
/// download) and whatever it was looking up, the hook stops with `cancelled`, asking for nothing
/// more.
#[hegel::test(test_cases = 10)]
fn cancelling_while_any_fetch_is_under_way_stops_it(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>().min_value(1).max_value(8));
    let drawn: Vec<Drawn> = (1..=n).map(|id| drawn_track(&tc, id)).collect();
    let answers = drawn_answers(&tc);
    let kind = tc.draw(gs::sampled_from(vec![ProviderKind::Artwork, ProviderKind::Metadata]));
    // The fetches up to one that fails the hook, which are the same whatever the covers' folder.
    let foreseen = foresee(&answers, &drawn, kind);
    tc.assume(!foreseen.fetches.is_empty());
    let held = tc.draw(gs::integers::<usize>().max_value(foreseen.fetches.len() - 1));

    let (route, fetched) = (answering(answers), AtomicUsize::new(0));
    let route = move |req: &Req| match fetched.fetch_add(1, Ordering::Relaxed) == held {
        true => Reply::Held,
        false => route(req),
    };
    let session = start(route);
    let tracks: Vec<TrackTags> = drawn.into_iter().map(|d| d.tags).collect();
    let pending = session.send(kind, &tracks, json!({}));
    for _ in 0..=held {
        session.heard.recv().unwrap();
    }
    pending.cancel();
    let answer = pending.wait_timeout(Duration::from_secs(10)).expect("it stops");
    assert_eq!(answer.map(|_: Candidates| ()).unwrap_err(), RpcError::cancelled());
    let fetches: Vec<(String, String)> = session.got().into_iter().map(|g| (g.path, g.query)).collect();
    assert_eq!(fetches, foreseen.fetches[..=held]);
}

/// Served as a real plugin is, a request with nothing to look up asks for no fetch: one that
/// doesn't read, or asks another provider (or another kind of one), is refused; tracks with
/// nothing to look up by come back without a candidate, a warning each, whichever provider asks.
#[hegel::test(test_cases = 15)]
fn served_for_real_it_looks_up_nothing_it_neednt(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>().max_value(5));
    let tracks: Vec<TrackTags> =
        (1..=n).map(|id| tags(id, "Bicep", tc.draw(gs::sampled_from(vec!["", " "])), "")).collect();
    let nameless: Vec<String> = (1..=n).map(|id| format!("track #{id} has no album or title to look up")).collect();
    let (shape, said) = tc.draw(gs::sampled_from(vec![
        ("fine", None),
        ("provider", Some("invalid params: the manifest declares no provider \"lyrics\"")),
        ("kind", Some("invalid params: covers provides no metadata")),
        ("tags kind", Some("invalid params: tags provides no artwork")),
        ("tracks", None),
    ]));
    let scratch = TempDir::new().unwrap();
    let context = json!({ "scratch": scratch.path() });
    let (provider, of) = tc.draw(gs::sampled_from(vec![("covers", "artwork"), ("tags", "metadata")]));
    let mut req = json!({ "provider": provider, "kind": of, "tracks": tracks, "context": context });
    match shape {
        "provider" => req["provider"] = json!("lyrics"),
        "kind" => req = json!({ "provider": "covers", "kind": "metadata", "tracks": tracks }),
        "tags kind" => req = json!({ "provider": "tags", "kind": "artwork", "tracks": tracks }),
        "tracks" => req["tracks"] = json!("1"),
        _ => {}
    }
    let session = start(services_as_they_are);
    let answer: Result<Candidates, RpcError> = session.host.request(method::PROVIDE, &req);
    match (shape, said) {
        ("fine", _) => assert_eq!(answer.unwrap(), Candidates { candidates: Vec::new(), warnings: nameless }),
        (_, Some(said)) => assert_eq!(kind(&answer.unwrap_err()), ("invalid_params", said)),
        _ => assert_eq!(answer.unwrap_err().kind(), Some("invalid_params")),
    }
    assert_eq!(session.got().len(), 0);
    assert_eq!(session.stop(), Vec::new());
}
