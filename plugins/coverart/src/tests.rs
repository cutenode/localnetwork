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
    Fetch, Fetched, HelloReply, Incoming, Manifest, NetLimit, PROTOCOL_VERSION, Peer, Provider, ProviderKind, method,
};
use serde_json::{Value, json};
use tempfile::TempDir;

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

/// MusicBrainz, which has Bicep's "Bicep" (searched by name) and "In My View" (by its ISRC), and
/// the Cover Art Archive, which has a cover for each release group.
fn services_as_they_are(req: &Req) -> Reply {
    match req.path.strip_prefix(SEARCH) {
        Some("release/") if req.query.starts_with(r#"release:"Bicep""#) => ok(RELEASES),
        Some("release/") => no_releases(),
        Some("recording/") if req.query == format!("isrc:{ISRC}") => ok(RECORDINGS),
        Some("recording/") => ok(r#"{"created":"2026-10-13T20:42:23.943Z","count":0,"offset":0,"recordings":[]}"#),
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
    /// Asks for covers for `tracks`, with a scratch folder of its own unless `context` gives one
    /// (or, `null`, none).
    fn send(&self, tracks: &[TrackTags], mut context: Value) -> loclib_plugin::Pending {
        if context.get("scratch").is_none() {
            let scratch = TempDir::new().unwrap();
            context["scratch"] = json!(scratch.path());
            self.scratches.lock().unwrap().push(scratch);
        }
        let req = json!({ "provider": "covers", "kind": "artwork", "tracks": tracks, "context": context });
        self.host.send_request(method::PROVIDE, &req).unwrap()
    }

    fn provide_with(&self, tracks: &[TrackTags], context: Value) -> Result<Candidates, RpcError> {
        self.send(tracks, context).wait()
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

/// It reads the tracks it's given and provides artwork, says which servers it talks to and asks
/// the host to keep to MusicBrainz's limit. It plans nothing: the host sets what's chosen.
#[test]
fn the_manifest_is_a_first_party_artwork_provider() {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    manifest.validate(true).unwrap();
    let capabilities: Vec<String> = manifest.capabilities.iter().map(ToString::to_string).collect();
    assert_eq!(
        capabilities,
        ["library:read", "net:musicbrainz.org", "net:coverartarchive.org", "net:archive.org", "net:*.archive.org"]
    );
    assert_eq!(manifest.net, [NetLimit { host: "musicbrainz.org".into(), per_second: 1.0 }]);
    let provider = Provider { name: "covers".into(), title: SOURCE.into(), kind: ProviderKind::Artwork };
    assert_eq!(manifest.contributes.providers, [provider]);
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
    assert!(["providers", "blobs", "net"].iter().all(has), "{:?}", hello.features);
    let described: Manifest = host.request(method::DESCRIBE, &json!({})).unwrap();
    assert_eq!(described, manifest);
    host.close();
    served.join().unwrap();
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
        let pending = session.send(&[tags(1, "Bicep", "Bicep", "Glue")], json!({}));
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

// Properties: drawn tracks against drawn services, the candidates foreseen from what was drawn.

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
    tags: TrackTags,
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
    let tags = TrackTags {
        album: album_tag,
        album_artist: if album_artist { "Various Artists".into() } else { String::new() },
        isrc: isrc_tag,
        ..tags(id, artist_tag, "", &title)
    };
    Drawn { tags, album, artist, isrc }
}

/// How MusicBrainz answers a search: `503` (busy) `busy` times, then `answer`.
#[derive(Debug, Clone)]
struct Search {
    busy: usize,
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
    /// The host gets no answer, failing so.
    Fails(&'static str),
}

#[hegel::test_helper]
fn drawn_search(tc: &TestCase) -> Search {
    let busy = tc.draw(gs::sampled_from(vec![0usize, 0, 0, 1, 2, 3]));
    let group = || gs::integers::<usize>().max_value(GROUPS.len() - 1);
    // Mostly the album, on its own (named once, or on two releases), so its cover is asked for.
    let answer = match tc
        .draw(gs::sampled_from(vec!["one", "one", "one", "one", "none", "several", "refused", "garbage", "fails"]))
    {
        "one" => Answer::Groups(vec![tc.draw(group()); tc.draw(gs::integers::<usize>().min_value(1).max_value(2))]),
        "none" => Answer::Groups(Vec::new()),
        "several" => Answer::Groups(tc.draw(gs::vecs(group()).min_size(2).max_size(3))),
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
        // Which album: the last digit of the name searched for, or of the ISRC.
        let album = |of: &str| of.chars().last().and_then(|c| c.to_digit(10)).unwrap() as usize;
        let search = match req.path.strip_prefix(SEARCH) {
            Some("release/") => Some((&answers.releases, false, album(req.query.split('"').nth(1).unwrap()))),
            Some("recording/") => Some((&answers.recordings, true, album(&req.query))),
            _ => None,
        };
        if let Some((searches, recording, k)) = search {
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
                Answer::Groups(groups) => ok(search_result(k, groups, recording)),
                Answer::Status(code) => status(*code),
                Answer::Garbage => ok("<html>maintenance</html>"),
                Answer::Fails(kind) => fails(kind),
            };
        }
        let id = req.path.strip_prefix(COVERS).and_then(|p| p.strip_suffix("/front-500")).unwrap();
        match answers.covers[GROUPS.iter().position(|known| *known == id).unwrap()] {
            Cover::Image { width, height, png, .. } => ok(picture(width, height, png)),
            Cover::Status(code) => status(code),
            Cover::Garbage => ok("<html>not an image</html>"),
            Cover::Fails(kind) => fails(kind),
        }
    }
}

/// The label of release group `g`'s cover, as drawn searches name it.
fn label_of(g: usize) -> String {
    let kind = if g.is_multiple_of(2) { "album" } else { "release" };
    format!("Group {g}, {kind}")
}

/// An error the hook fails with: its kind, and how its message starts.
type Refusal = (&'static str, String);

/// What asking about tracks should come to: the candidates (each one's track, score, label and
/// file) and warnings, or the error it fails with, with the fetches it asks for (each one's URL
/// but its query, and its `query` param), in order.
struct Outcome {
    found: Result<(Vec<Seen>, Vec<String>), Refusal>,
    fetches: Vec<(String, String)>,
}

/// An album as the plugin looks it up: shown as its first track tags it, searched by its first
/// ISRC.
struct Looked {
    k: usize,
    artist: String,
    name: String,
    isrc: bool,
    tracks: Vec<u32>,
}

impl Looked {
    fn shown(&self) -> String {
        match self.artist.is_empty() {
            true => self.name.clone(),
            false => format!("{} - {}", self.artist, self.name),
        }
    }
}

/// What asking about `drawn`'s tracks comes to, on the services `answers` stands for.
fn foresee(answers: &Answers, drawn: &[Drawn]) -> Outcome {
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
                albums.push(((d.artist, k), Looked { k, artist, name, isrc: false, tracks: Vec::new() }));
                albums.len() - 1
            }
        };
        let album = &mut albums[at].1;
        album.isrc |= d.isrc;
        album.tracks.push(t.id);
    }

    let mut fetches: Vec<(String, String)> = Vec::new();
    // Each cover downloaded, by release group: whether there's one, or why not.
    let mut got: Vec<(usize, Result<(), String>)> = Vec::new();
    let mut found: Vec<Seen> = Vec::new();
    // A search's release groups (deduplicated), or the status it was refused with; or the error
    // the hook fails with.
    let search = |fetches: &mut Vec<(String, String)>, entity: &str, search: &Search, query: String| {
        let path = format!("{SEARCH}{entity}/");
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
                Answer::Fails(kind) => Err((*kind, format!("couldn't reach MusicBrainz: no answer ({kind})"))),
            };
        }
        unreachable!()
    };
    let mut fail = None;
    'albums: for (_, album) in &albums {
        let shown = album.shown();
        let (mut found_groups, mut by_isrc) = (Ok(Vec::new()), false);
        if album.isrc {
            match search(&mut fetches, "recording", &answers.recordings[album.k], format!("isrc:{}", isrc_of(album.k)))
            {
                Ok(answer) => {
                    by_isrc = answer.as_ref().is_ok_and(|g| !g.is_empty());
                    found_groups = answer;
                }
                Err(e) => {
                    fail = Some(e);
                    break;
                }
            }
        }
        if found_groups.as_ref().is_ok_and(Vec::is_empty) {
            let mut query = format!("release:\"{}\"", album.name);
            if !album.artist.is_empty() {
                query = format!("{query} AND artist:\"{}\"", album.artist);
            }
            match search(&mut fetches, "release", &answers.releases[album.k], query) {
                Ok(answer) => found_groups = answer,
                Err(e) => {
                    fail = Some(e);
                    break;
                }
            }
        }
        let groups = match found_groups {
            Err(code) => {
                warned.push(format!("{shown}: left out, as MusicBrainz answered {code}"));
                continue;
            }
            Ok(groups) if groups.is_empty() => {
                warned.push(format!("{shown}: not on MusicBrainz"));
                continue;
            }
            Ok(groups) => groups,
        };
        let score = match (groups.len(), by_isrc) {
            (1, true) => BY_ISRC,
            (1, false) => BY_NAME,
            _ => ONE_OF_SEVERAL,
        };
        for &g in groups.iter().take(MAX_GROUPS) {
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
                Ok(()) => found.extend(album.tracks.iter().map(|&t| (t, score, label_of(g), cover(GROUPS[g])))),
                Err(why) if groups.len() > 1 => warned.push(format!("{shown}, {}: {why}", label_of(g))),
                Err(why) => warned.push(format!("{shown}: {why}")),
            }
        }
    }
    match fail {
        Some(e) => Outcome { found: Err(e), fetches },
        None => Outcome { found: Ok((found, warned)), fetches },
    }
}

/// For any tracks asked about (in albums or singles, by artists spelled several ways, with ISRCs
/// or not), and however MusicBrainz and the Cover Art Archive answer (found, not found, several,
/// busy, refusing, garbage, out of reach; covers missing, broken, tiny, or not to be had, or that
/// can't be written), the candidates are what their answers make them: each album looked up once
/// (by ISRC first), each release group's cover downloaded once, a file in the scratch folder
/// offered for each of the album's tracks (surely when it's the only one, more so by an ISRC), and
/// a warning for each album with none. Without a scratch folder, nothing is looked up.
#[hegel::test(test_cases = 60)]
fn any_tracks_get_the_covers_the_services_answer(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>().max_value(8));
    let drawn: Vec<Drawn> = (1..=n).map(|id| drawn_track(&tc, id)).collect();
    let answers = drawn_answers(&tc);
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
    let provided = session.provide_with(&tracks, context);
    let foreseen = match given {
        true => foresee(&answers, &drawn),
        false => {
            let refused =
                "invalid params: the host gives the plugin no scratch folder for the covers (the blobs feature)";
            Outcome { found: Err(("invalid_params", refused.into())), fetches: Vec::new() }
        }
    };
    match (&provided, &foreseen.found) {
        (Ok(candidates), Ok((seen, warned))) => {
            assert_eq!(&found(candidates, scratch.path()), seen);
            assert_eq!(&candidates.warnings, warned);
        }
        (Err(e), Err((kind, start))) => {
            assert_eq!(e.kind(), Some(*kind), "{e:?}");
            assert!(e.message.starts_with(start.as_str()), "{e:?}");
        }
        (provided, foreseen) => panic!("provided {provided:?}, not {foreseen:?}"),
    }
    let fetches: Vec<(String, String)> = session.got().into_iter().map(|g| (g.path, g.query)).collect();
    assert_eq!(fetches, foreseen.fetches);
    assert_eq!(session.stop(), Vec::new(), "it reads nothing");
}

/// Cancelled while a fetch is under way, whichever it is (a search or a cover's download) and
/// whatever it was looking up, the hook stops with `cancelled`, asking for nothing more.
#[hegel::test(test_cases = 10)]
fn cancelling_while_any_fetch_is_under_way_stops_it(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>().min_value(1).max_value(8));
    let drawn: Vec<Drawn> = (1..=n).map(|id| drawn_track(&tc, id)).collect();
    let answers = drawn_answers(&tc);
    // The fetches up to one that fails the hook, which are the same whatever the covers' folder.
    let foreseen = foresee(&answers, &drawn);
    tc.assume(!foreseen.fetches.is_empty());
    let held = tc.draw(gs::integers::<usize>().max_value(foreseen.fetches.len() - 1));

    let (route, fetched) = (answering(answers), AtomicUsize::new(0));
    let route = move |req: &Req| match fetched.fetch_add(1, Ordering::Relaxed) == held {
        true => Reply::Held,
        false => route(req),
    };
    let session = start(route);
    let tracks: Vec<TrackTags> = drawn.into_iter().map(|d| d.tags).collect();
    let pending = session.send(&tracks, json!({}));
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
/// doesn't read, or asks another provider (or kind), is refused; tracks with nothing to look up
/// by come back without a candidate, a warning each.
#[hegel::test(test_cases = 15)]
fn served_for_real_it_looks_up_nothing_it_neednt(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>().max_value(5));
    let tracks: Vec<TrackTags> =
        (1..=n).map(|id| tags(id, "Bicep", tc.draw(gs::sampled_from(vec!["", " "])), "")).collect();
    let nameless: Vec<String> = (1..=n).map(|id| format!("track #{id} has no album or title to look up")).collect();
    let (shape, said) = tc.draw(gs::sampled_from(vec![
        ("fine", None),
        ("provider", Some("invalid params: the manifest declares no provider \"tags\"")),
        ("kind", Some("invalid params: covers provides no metadata")),
        ("tracks", None),
    ]));
    let scratch = TempDir::new().unwrap();
    let context = json!({ "scratch": scratch.path() });
    let mut req = json!({ "provider": "covers", "kind": "artwork", "tracks": tracks, "context": context });
    match shape {
        "provider" => req["provider"] = json!("tags"),
        "kind" => req["kind"] = json!("metadata"),
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
