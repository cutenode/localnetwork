use std::io::pipe;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use hegel::TestCase;
use hegel::generators as gs;
use loclib_plugin::test_util::Canceller;
use loclib_plugin::{
    ACCOUNT_PLACEHOLDER, AccountType, ByteRange, Capability, DestinationKind, Fetch, Fetched, HelloReply, Incoming,
    Manifest, PROTOCOL_VERSION, Peer, ProgressReport, Progressed, PublishedSet, SignIn, StrongRef, TracklistEntry,
    account_placeholder, method,
};
use loclib_progress::{Event, Phase};
use serde_json::{Value, json};

use super::track::{MAX_DESCRIPTION, MAX_TITLE};
use super::upload::{API, AUTHORIZATION, TRIES, parts};
use super::*;

/// The upload the fake plyr.fm opens, and the track it publishes, with its record.
const UPLOAD: &str = "0b7c";
const TRACK: u64 = 4242;
const TRACK_URI: &str = "at://did:plc:dj/fm.plyr.track/3m4tr4ck";
const TRACK_CID: &str = "bafyreie5737gdxlw5i64vzichcalba3z2v5n6icifvx5xytvske7mr3hpm";

/// The account type the plugin asks for, as the plyr.fm account plugin provides it.
fn account_type() -> AccountType {
    let manifest = Manifest::parse(include_str!("../../accounts/plyr-account/locallibrary-plugin.toml"));
    let [account_type] = &manifest.unwrap().contributes.account_types[..] else { panic!("one account type") };
    account_type.clone()
}

/// The account's developer token, as the host fills it in.
const TOKEN: &str = "plyr-developer-token";

/// As the host fails a request for a plyr account that's signed out.
const SIGNED_OUT: &str = "the plyr account dj.example is signed out: sign in again";

/// How plyr.fm answers a request through the host, in place of what it would: a status and body,
/// the host failing to reach it, or either of those (or plyr.fm's own answer) and then the host
/// cancelling the publish.
#[derive(Clone, Debug)]
enum Reply {
    Status(u16, String),
    Unreachable,
    Cancels(Option<Box<Reply>>),
}

fn status(code: u16, detail: &str) -> Reply {
    Reply::Status(code, json!({ "detail": detail }).to_string())
}

/// Which request it is (`start`, `part 2`, `finish`, `progress`) and how many times it was asked
/// before: a reply in place of plyr.fm's, or none for what plyr.fm says.
type Script = Box<dyn Fn(&str, usize) -> Option<Reply> + Send>;

/// What the fake plyr.fm was sent.
#[derive(Default)]
struct Received {
    /// Each request's method and URL, in order.
    requests: Vec<(String, String)>,
    /// The size the upload was opened with, and the name.
    opened: Option<(u64, String)>,
    /// Each part as it arrived (sent again, again), by number.
    parts: Vec<(u64, Vec<u8>)>,
    /// The fields the track was finished with.
    finished: Vec<(String, String)>,
    /// How many requests the host refused to send, the account signed out.
    signed_out: usize,
}

impl Received {
    /// The file the parts make, each part's latest.
    fn file(&self) -> Vec<u8> {
        let mut latest: Vec<&(u64, Vec<u8>)> = Vec::new();
        for part in &self.parts {
            latest.retain(|(n, _)| *n != part.0);
            latest.push(part);
        }
        latest.sort_by_key(|(n, _)| *n);
        latest.into_iter().flat_map(|(_, bytes)| bytes.clone()).collect()
    }

    fn field(&self, name: &str) -> Option<&str> {
        self.finished.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// plyr.fm as `upload_sessions.py` and `uploads.py` answer, with parts of `part_size` bytes, but
/// as `script` says, through a host with the account signed in or not that lent the hook `lent`.
struct Plyr {
    part_size: u64,
    signed_in: bool,
    lent: Option<PathBuf>,
    script: Script,
    received: Received,
    /// Cancels the publish once the call in hand is answered, when [`Reply::Cancels`] says to.
    cancel: Arc<Canceller>,
}

impl Plyr {
    fn new(part_size: u64) -> Plyr {
        let (script, received) = (Box::new(|_: &str, _| None), Received::default());
        Plyr { part_size, signed_in: true, lent: None, script, received, cancel: Arc::default() }
    }

    fn scripted(part_size: u64, script: impl Fn(&str, usize) -> Option<Reply> + Send + 'static) -> Plyr {
        Plyr { script: Box::new(script), ..Plyr::new(part_size) }
    }

    /// The host's answer to `fetch`.
    fn answer(&mut self, fetch: &Fetch) -> Result<Fetched, RpcError> {
        // As the host fills in the account: only as the whole `Authorization` header (never in the
        // address), while it's signed in, for a request its `upload` scope covers.
        let misplaced = || RpcError::failed("invalid", "an account goes only in the Authorization header");
        let named = fetch.headers.iter().filter(|(_, v)| v.contains(ACCOUNT_PLACEHOLDER));
        let placed: Vec<_> = named.map(|(k, v)| (k.to_lowercase(), account_placeholder(v))).collect();
        if fetch.url.contains(ACCOUNT_PLACEHOLDER) || placed != [("authorization".to_string(), Some(("plyr", None)))] {
            return Err(misplaced());
        }
        if !self.signed_in {
            self.received.signed_out += 1;
            return Err(RpcError::failed("access", SIGNED_OUT));
        }
        let url = url::Url::parse(&fetch.url).unwrap();
        if !account_type().scopes["upload"].covers(&fetch.method, &url) {
            let why = format!("none of its scopes on the plyr account dj.example covers {} {url}", fetch.method);
            return Err(RpcError::failed("scope", why));
        }
        let sent = file_part(self.lent.as_deref(), fetch)?;
        let r = &mut self.received;
        r.requests.push((fetch.method.clone(), fetch.url.clone()));
        let path = fetch.url.strip_prefix(API).expect("only plyr.fm's API");
        let session = format!("/tracks/uploads/{UPLOAD}");
        let route = match (fetch.method.as_str(), path) {
            ("POST", "/tracks/uploads") => "start".to_string(),
            ("PUT", p) => format!("part {}", p.strip_prefix(&format!("{session}/parts/")).expect("a part")),
            ("POST", p) if p == format!("{session}/finish") => "finish".into(),
            ("GET", p) if p == format!("{session}/progress") => "progress".into(),
            other => panic!("plyr.fm has no {other:?}"),
        };
        let asked = r.requests.iter().filter(|(m, u)| (m, u) == (&fetch.method, &fetch.url)).count() - 1;
        let reply = match (self.script)(&route, asked) {
            Some(Reply::Cancels(then)) => {
                self.cancel.arm();
                then.map(|r| *r)
            }
            reply => reply,
        };
        let (status, body) = match reply {
            Some(Reply::Unreachable) => return Err(RpcError::failed("external", "connection reset")),
            Some(Reply::Cancels(_)) => panic!("a cancel cancels once"),
            Some(Reply::Status(status, body)) => (status, body),
            None => as_plyr(self.part_size, r, &route, fetch, sent),
        };
        Ok(Fetched { status, headers: Default::default(), body: Some(body), blob: None })
    }
}

/// What of a file `fetch` sends, read as the host reads it before sending: only the file the hook
/// was lent, and only a range inside it.
fn file_part(lent: Option<&Path>, fetch: &Fetch) -> Result<Option<Vec<u8>>, RpcError> {
    let Some(path) = &fetch.body_blob else { return Ok(None) };
    let invalid = |why: String| RpcError::failed("invalid", format!("{}: {why}", path.display()));
    if lent != Some(path.as_path()) {
        return Err(invalid("not a file it was lent".into()));
    }
    let file = std::fs::read(path).map_err(|e| invalid(e.to_string()))?;
    let size = file.len() as u64;
    let ByteRange { offset, length } = fetch.body_range.unwrap_or_default();
    let length = length.unwrap_or(size.saturating_sub(offset));
    if offset + length > size {
        return Err(invalid(format!("{length} bytes from {offset} go past its end, at {size}")));
    }
    Ok(Some(file[offset as usize..(offset + length) as usize].to_vec()))
}

/// What plyr.fm, cutting files into parts of `part_size` bytes, answers `fetch`, the request
/// `route` (as [`Script`] names it) sending the file part `sent`, having been sent `r`.
fn as_plyr(part_size: u64, r: &mut Received, route: &str, fetch: &Fetch, sent: Option<Vec<u8>>) -> (u16, String) {
    let content_type = fetch.headers.get("Content-Type").map(String::as_str);
    let refused = |status: u16, detail: String| (status, json!({ "detail": detail }).to_string());
    match route {
        "start" => {
            assert_eq!(content_type, Some("application/json"));
            let body: Value = serde_json::from_str(fetch.body.as_deref().unwrap()).unwrap();
            let size = body["size_bytes"].as_u64().unwrap();
            r.opened = Some((size, body["filename"].as_str().unwrap().into()));
            let count = size.div_ceil(part_size);
            let session =
                json!({ "upload_id": UPLOAD, "part_size_bytes": part_size, "part_count": count, "received_parts": [] });
            (200, session.to_string())
        }
        "finish" => {
            assert_eq!(content_type, Some("application/x-www-form-urlencoded"));
            let size = r.opened.as_ref().unwrap().0;
            let count = size.div_ceil(part_size);
            if (1..=count).any(|n| !r.parts.iter().any(|(p, _)| *p == n)) {
                return refused(409, "upload incomplete".into());
            }
            let form = form_urlencoded::parse(fetch.body.as_deref().unwrap().as_bytes());
            r.finished = form.into_owned().collect();
            (
                200,
                json!({ "upload_id": UPLOAD, "status": "pending", "message": "upload queued for processing" })
                    .to_string(),
            )
        }
        "progress" => {
            let processing = json!({ "upload_id": UPLOAD, "status": "processing", "phase": "pds_upload" });
            let completed = json!({
                "upload_id": UPLOAD,
                "status": "completed",
                "track_id": TRACK,
                "atproto_uri": TRACK_URI,
                "atproto_cid": TRACK_CID,
            });
            (200, format!("data: {processing}\n\ndata: {completed}\n\n"))
        }
        part => {
            assert_eq!(content_type, Some("application/octet-stream"));
            assert_eq!(fetch.body, None);
            let n: u64 = part.strip_prefix("part ").unwrap().parse().unwrap();
            let size = r.opened.as_ref().unwrap().0;
            let offset = (n - 1) * part_size;
            let expected = ByteRange { offset, length: Some(part_size.min(size - offset)) };
            if fetch.body_range != Some(expected) {
                return refused(400, format!("part {n} must be {expected:?} of the file, got {:?}", fetch.body_range));
            }
            r.parts.push((n, sent.expect("a part's bytes")));
            (200, json!({ "part_number": n, "etag": format!("e{n}") }).to_string())
        }
    }
}

/// The plugin served on a thread over pipes, as [`serve`] serves it, past `hello`, with a host
/// whose fetches `plyr` answers.
struct Session {
    host: Peer,
    served: JoinHandle<()>,
    answering: JoinHandle<()>,
    hello: HelloReply,
    /// Answering its fetches: scripted afresh by [`Session::script`].
    plyr: Arc<Mutex<Plyr>>,
}

fn start(plyr: Plyr) -> Session {
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serve(plugin_in, plugin_out));
    let (host, incoming) = Peer::new(from_plugin, to_plugin);
    let plyr = Arc::new(Mutex::new(plyr));
    let (to, answers) = (host.clone(), plyr.clone());
    let answering = std::thread::spawn(move || {
        for msg in incoming {
            let Incoming::Request(req) = msg else { continue };
            assert_eq!(req.method, method::NET_FETCH, "the plugin reads nothing else");
            let fetch: Fetch = serde_json::from_value(req.params).unwrap();
            let mut plyr = answers.lock().unwrap();
            let answer = plyr.answer(&fetch).map(|f| serde_json::to_value(f).unwrap());
            let cancel = plyr.cancel.clone();
            drop(plyr);
            cancel.respond(&to, &req.id, answer);
        }
    });
    let hello = json!({ "protocol": PROTOCOL_VERSION, "host": { "name": "locallibrary", "version": "0.1.0" } });
    let hello = host.request(method::HELLO, &hello).unwrap();
    Session { host, served, answering, hello, plyr }
}

/// What a publish answered, and the upload phase it reported, in order: `Some` of the parts and
/// bytes it started with, `None` where it ended.
type Outcome = (Result<Published, RpcError>, Vec<Option<(u64, u64)>>);

impl Session {
    /// `set/publish` to `track` as the host sends it, lending the hook `recording`.
    fn publish(&self, set: &PublishedSet, recording: &Path, tracklist: &[TracklistEntry], dry_run: bool) -> Outcome {
        self.plyr.lock().unwrap().lent = Some(recording.to_path_buf());
        let req = json!({ "destination": "track", "set": set, "recording": recording, "tracklist": tracklist, "dry_run": dry_run });
        let cancel = self.plyr.lock().unwrap().cancel.clone();
        let pending = cancel.send(|| self.host.send_request_reporting(method::SET_PUBLISH, &req).unwrap());
        let answer = pending.wait_timeout(Duration::from_secs(60)).expect("an answer in a minute");
        let upload = pending
            .progress()
            .filter_map(|p| match p {
                ProgressReport::Known(e) => match e.event {
                    Progressed::Event(Event::PhaseStart { phase: Phase::Upload, items, bytes }) => {
                        Some(Some((items, bytes)))
                    }
                    Progressed::Event(Event::PhaseEnd { phase: Phase::Upload }) => Some(None),
                    _ => None,
                },
                ProgressReport::Unknown(_) => None,
            })
            .collect();
        (answer, upload)
    }

    /// Its fetches answered by `plyr` from now on.
    fn script(&self, plyr: Plyr) {
        let mut answering = self.plyr.lock().unwrap();
        *answering = Plyr { cancel: answering.cancel.clone(), ..plyr };
    }

    /// What plyr.fm has been sent.
    fn received(&self) -> Received {
        std::mem::take(&mut self.plyr.lock().unwrap().received)
    }

    /// Closes the plugin's input: it stops. What plyr.fm was sent, read once all of it is
    /// answered: what the plugin sent before a cancel reached it may still be on its way.
    fn stop(self) -> Received {
        self.host.close();
        self.served.join().unwrap();
        self.answering.join().unwrap();
        std::mem::take(&mut self.plyr.lock().unwrap().received)
    }
}

/// A recording `name` of `size` bytes, none of its parts alike.
fn recording(dir: &Path, name: &str, size: u64) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, (0..size).map(|i| (i * 31 % 251) as u8).collect::<Vec<u8>>()).unwrap();
    path
}

fn set() -> PublishedSet {
    PublishedSet {
        id: "s1".into(),
        date: "2026-10-03".into(),
        title: Some("Closing set".into()),
        venue: Some("Warehouse".into()),
        ..PublishedSet::default()
    }
}

fn entry(artist: Option<&str>, title: Option<&str>, start: Option<u32>) -> TracklistEntry {
    TracklistEntry { artist: artist.map(Into::into), title: title.map(Into::into), start_seconds: start }
}

fn kind(e: &RpcError) -> (&str, &str) {
    (e.kind().unwrap_or_default(), e.message.as_str())
}

/// It reaches plyr.fm's API alone, publishes sets at one destination, and asks for a plyr.fm
/// account to upload with, naming it where the account plugin puts its token: the
/// `Authorization` header, after `Bearer`, as plyr.fm takes a developer token. That account plugin
/// asks plyr.fm's API whose token it is.
#[test]
fn the_manifest_is_a_built_in_set_destination_on_plyr() {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    manifest.validate(true).unwrap();
    assert_eq!(manifest.capabilities, [Capability::Net("api.plyr.fm".into())]);
    let [destination] = &manifest.contributes.destinations[..] else { panic!("{manifest:?}") };
    assert_eq!((manifest.id.as_str(), destination.name.as_str()), ("plyr", "track"));
    assert_eq!(destination.kind, DestinationKind::Set);
    let [need] = &manifest.accounts[..] else { panic!("{manifest:?}") };
    assert_eq!((need.name.as_str(), need.scopes.as_slice()), ("plyr", ["upload".to_string()].as_slice()));
    assert!(need.optional.is_empty(), "{need:?}");
    assert!(manifest.contributes.settings.is_empty(), "{:?}", manifest.contributes.settings);
    let account_type = account_type();
    assert_eq!(format!("{{account:{}}}", account_type.name), AUTHORIZATION);
    let place = account_type.place();
    assert_eq!((account_type.kind, place.header.as_deref()), (SignIn::Keys, Some("Authorization")));
    assert_eq!(place.filled(|_| TOKEN), format!("Bearer {TOKEN}"));
    let whoami = account_type.whoami.expect("plyr.fm says whose token it is");
    assert_eq!(whoami.url, format!("{API}/auth/me"));

    // Served as a real one is: it says it's this manifest, publishing sets through fetches with an
    // account.
    let s = start(Plyr::new(4));
    let has = |f: &&str| s.hello.features.iter().any(|had| had == f);
    assert!(["destinations", "net", "blobs", "accounts"].iter().all(has), "{:?}", s.hello.features);
    let described: Manifest = s.host.request(method::DESCRIBE, &json!({})).unwrap();
    assert_eq!(described, manifest);
    s.stop();
}

/// The recording goes up in plyr.fm's parts, in order, each its size but the last; then the
/// track's title, tracklist and tag, and once plyr.fm has published it, its page, named plyr.fm,
/// and its record are the answer.
#[test]
fn a_set_goes_up_in_parts_and_answers_its_page() {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "Closing Set.FLAC", 2500);
    let tracklist = [
        entry(Some("Young Fathers"), Some("In My View"), Some(0)),
        entry(None, None, Some(330)),
        entry(Some("Chris Lake"), Some("Turn Off the Lights"), Some(3723)),
        entry(Some("Someone"), Some("Unplaced"), None),
    ];
    let s = start(Plyr::new(1024));
    let (answer, upload) = s.publish(&set(), &path, &tracklist, false);
    let r = s.stop();
    let published = answer.unwrap();
    let record = StrongRef { uri: TRACK_URI.into(), cid: TRACK_CID.into() };
    let page = format!("https://plyr.fm/track/{TRACK}");
    let answer =
        Published { url: Some(page), name: Some("plyr.fm".into()), record: Some(record), warnings: Vec::new() };
    assert_eq!(published, answer);
    assert_eq!(r.opened, Some((2500, "Closing Set.FLAC".into())));
    let sizes: Vec<(u64, usize)> = r.parts.iter().map(|(n, b)| (*n, b.len())).collect();
    assert_eq!(sizes, [(1, 1024), (2, 1024), (3, 452)]);
    assert_eq!(r.file(), std::fs::read(&path).unwrap());
    assert_eq!(upload, [Some((3, 2500)), None]);
    let description =
        "00:00 Young Fathers - In My View\n05:30 ID - ID\n1:02:03 Chris Lake - Turn Off the Lights\nSomeone - Unplaced";
    let fields = [("title", "Closing set"), ("tags", r#"["dj mix"]"#), ("description", description)];
    assert_eq!(r.finished, fields.map(|(k, v)| (k.to_string(), v.to_string())));
    let session = format!("{API}/tracks/uploads/{UPLOAD}");
    let requests: Vec<(&str, String)> = vec![
        ("POST", format!("{API}/tracks/uploads")),
        ("PUT", format!("{session}/parts/1")),
        ("PUT", format!("{session}/parts/2")),
        ("PUT", format!("{session}/parts/3")),
        ("POST", format!("{session}/finish")),
        ("GET", format!("{session}/progress")),
    ];
    assert_eq!(r.requests, requests.into_iter().map(|(m, u)| (m.to_string(), u)).collect::<Vec<_>>());
}

/// A recording plyr.fm wouldn't take (another format, empty, over 1536 MiB, or gone) is refused
/// before anything goes, saying why.
#[test]
fn a_recording_plyr_wouldnt_take_is_refused_first() {
    let dir = tempfile::tempdir().unwrap();
    let big = dir.path().join("long.wav");
    std::fs::File::create(&big).unwrap().set_len(MAX_BYTES + 1).unwrap();
    let cases = [
        (
            recording(dir.path(), "set.aac", 10),
            "invalid",
            "plyr.fm takes mp3, wav, m4a, aiff, flac, webm and ogg recordings, which set.aac isn't".to_string(),
        ),
        (
            recording(dir.path(), "set", 10),
            "invalid",
            "plyr.fm takes mp3, wav, m4a, aiff, flac, webm and ogg recordings, which set isn't".into(),
        ),
        (recording(dir.path(), "set.aif", 0), "invalid", "set.aif is empty".into()),
        (big, "invalid", "plyr.fm takes recordings of up to 1536.0 MiB, and long.wav is 1536.0 MiB".into()),
    ];
    let s = start(Plyr::new(1024));
    for (path, expected_kind, expected) in &cases {
        let e = s.publish(&set(), path, &[], false).0.unwrap_err();
        assert_eq!(kind(&e), (*expected_kind, expected.as_str()), "{}", path.display());
    }
    let gone = dir.path().join("gone.mp3");
    let e = s.publish(&set(), &gone, &[], false).0.unwrap_err();
    assert_eq!(e.kind(), Some("io"), "{e:?}");
    assert!(e.message.starts_with(&gone.display().to_string()), "{e:?}");
    assert!(s.stop().requests.is_empty());
}

/// A dry run says what would go, and sends nothing.
#[test]
fn a_dry_run_sends_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "set.mp3", 3 << 19);
    let s = start(Plyr::new(1024));
    let untitled = PublishedSet { title: None, ..set() };
    let tracklist = [entry(Some("A"), Some("B"), Some(1)), entry(None, None, None)];
    let (answer, upload) = s.publish(&untitled, &path, &tracklist, true);
    let line = "set.mp3 (1.5 MiB) would go to plyr.fm as \"Warehouse, 2026-10-03\", tagged dj mix, its 2 tracks in the description";
    assert_eq!(answer.unwrap(), Published { warnings: vec![line.into()], ..Published::default() });
    let (answer, _) = s.publish(&untitled, &path, &[], true);
    let line = "set.mp3 (1.5 MiB) would go to plyr.fm as \"Warehouse, 2026-10-03\", tagged dj mix";
    assert_eq!(answer.unwrap().warnings, [line]);
    assert!(upload.is_empty());
    assert!(s.stop().requests.is_empty());
}

/// A part that doesn't arrive, or that plyr.fm fails, is sent again until it does, the others
/// once each; one that fails every time stops the upload there, before the track is finished.
#[test]
fn a_part_that_fails_is_sent_again() {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "set.wav", 300);
    let plyr = Plyr::scripted(100, |route, asked| match (route, asked) {
        ("part 2", 0) => Some(Reply::Unreachable),
        ("part 2", 1) => Some(status(503, "busy")),
        _ => None,
    });
    let s = start(plyr);
    let (answer, upload) = s.publish(&set(), &path, &[], false);
    let r = s.stop();
    assert_eq!(answer.unwrap().url, Some(format!("https://plyr.fm/track/{TRACK}")));
    let order: Vec<u64> = r.parts.iter().map(|(n, _)| *n).collect();
    assert_eq!(order, [1, 2, 3]);
    let puts = r.requests.iter().filter(|(m, _)| m == "PUT").count();
    assert_eq!(puts, 5);
    assert_eq!(r.file(), std::fs::read(&path).unwrap());
    assert_eq!(upload, [Some((3, 300)), None]);
    // Without a description, none is sent.
    assert_eq!(r.field("description"), None);

    let s = start(Plyr::scripted(100, |route, _| (route == "part 2").then(|| status(502, "bad gateway"))));
    let e = s.publish(&set(), &path, &[], false).0.unwrap_err();
    let r = s.stop();
    assert_eq!(kind(&e), ("external", "plyr.fm failed part 2 of 3 (502: bad gateway)"));
    let parts: Vec<&str> = r.requests.iter().map(|(_, u)| u.rsplit('/').next().unwrap()).collect();
    assert_eq!(parts, ["uploads", "1", "2", "2", "2", "2"]);
    assert_eq!(TRIES, 4);
}

/// With the account signed out nothing goes, and nothing is asked again: the host's error
/// (`access`, which it passes on saying how to sign in) is the answer.
#[test]
fn signed_out_nothing_goes() {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "set.flac", 10);
    let s = start(Plyr { signed_in: false, ..Plyr::new(1024) });
    let e = s.publish(&set(), &path, &[], false).0.unwrap_err();
    assert_eq!(kind(&e), ("access", SIGNED_OUT));
    let r = s.stop();
    assert_eq!((r.requests.len(), r.signed_out), (0, 1));
}

/// The plyr.fm account plugin's `upload` scope covers every request of the upload, and not the
/// rest of plyr.fm's API.
#[test]
fn plyrs_upload_scope_covers_the_upload() {
    let account_type = account_type();
    let scope = &account_type.scopes["upload"];
    let covers = |method: &str, path: &str| scope.covers(method, &url::Url::parse(&format!("{API}{path}")).unwrap());
    let session = format!("/tracks/uploads/{UPLOAD}");
    assert!(covers("POST", "/tracks/uploads"));
    assert!(covers("PUT", &format!("{session}/parts/1")));
    assert!(covers("POST", &format!("{session}/finish")));
    assert!(covers("GET", &format!("{session}/progress")));
    assert!(!covers("DELETE", &format!("/tracks/{TRACK}")));
    assert!(!covers("GET", "/auth/me"));
    assert!(!covers("POST", "/tracks/uploadsX"));
}

/// A recording cut short once the upload has started fails the upload where the host can't send
/// the next part, with its error naming the file, and nothing is asked again.
#[test]
fn a_file_that_goes_wrong_midway_fails_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "set.flac", 300);
    let cut = path.clone();
    let s = start(Plyr::scripted(100, move |route, _| {
        if route == "part 2" {
            std::fs::File::options().write(true).open(&cut).unwrap().set_len(150).unwrap();
        }
        None
    }));
    let (answer, upload) = s.publish(&set(), &path, &[], false);
    let r = s.stop();
    assert_eq!(r.parts.iter().map(|(n, _)| *n).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(r.requests.len(), 3);
    let e = answer.unwrap_err();
    assert_eq!(
        kind(&e),
        ("invalid", format!("{}: 100 bytes from 200 go past its end, at 150", path.display()).as_str())
    );
    // Failing, it never ends the upload phase.
    assert_eq!(upload, [Some((3, 300))]);
}

/// plyr.fm refusing says why, in its words: a token it doesn't take, a track it won't publish.
/// Neither is asked again.
#[test]
fn plyr_refusing_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "set.ogg", 10);
    let s = start(Plyr::scripted(1024, |route, _| (route == "start").then(|| status(401, "session_expired"))));
    let e = s.publish(&set(), &path, &[], false).0.unwrap_err();
    assert_eq!(s.stop().requests.len(), 1);
    let said = "plyr.fm refused the account's token (401: session_expired): sign in again with a new one from \
                plyr.fm/settings#developer (locallibrary account plyr sign-in)";
    assert_eq!(kind(&e), ("permission", said));

    let s = start(Plyr::scripted(1024, |route, _| (route == "finish").then(|| status(400, "invalid tags"))));
    let e = s.publish(&set(), &path, &[], false).0.unwrap_err();
    assert_eq!(s.stop().requests.len(), 3);
    assert_eq!(kind(&e), ("invalid", "plyr.fm refused the track (400: invalid tags)"));

    // An answer that isn't JSON says nothing more; one that can't be read fails.
    let s = start(Plyr::scripted(1024, |route, _| (route == "start").then(|| Reply::Status(500, "<html>".into()))));
    let e = s.publish(&set(), &path, &[], false).0.unwrap_err();
    s.stop();
    assert_eq!(kind(&e), ("external", "plyr.fm failed the upload (500)"));
    let s = start(Plyr::scripted(1024, |route, _| (route == "start").then(|| Reply::Status(200, "{}".into()))));
    let e = s.publish(&set(), &path, &[], false).0.unwrap_err();
    s.stop();
    assert!(e.message.starts_with("plyr.fm's answer can't be read: missing field `upload_id`"), "{e:?}");
    let session = json!({ "upload_id": UPLOAD, "part_size_bytes": 4, "part_count": 1 }).to_string();
    let s =
        start(Plyr::scripted(1024, move |route, _| (route == "start").then(|| Reply::Status(200, session.clone()))));
    let e = s.publish(&set(), &path, &[], false).0.unwrap_err();
    s.stop();
    assert_eq!(kind(&e), ("external", "plyr.fm asked for 1 parts of 4 bytes"));
}

/// The track plyr.fm fails to publish fails the publish, saying why; a progress that stops before
/// the upload does is read again, and plyr.fm's warnings come with the page. A record plyr.fm
/// names only half of isn't answered.
#[test]
fn the_progress_says_how_publishing_went() {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "set.m4a", 10);
    let failed = json!({ "status": "failed", "message": "upload failed", "error": "duplicate of track 7" });
    let s = start(Plyr::scripted(1024, move |route, _| {
        (route == "progress").then(|| Reply::Status(200, format!("data: {failed}\n\n")))
    }));
    let e = s.publish(&set(), &path, &[], false).0.unwrap_err();
    s.stop();
    assert_eq!(kind(&e), ("external", "plyr.fm couldn't publish the track: duplicate of track 7"));

    let done = json!({
        "status": "completed",
        "track_id": 9,
        "atproto_uri": TRACK_URI,
        "atproto_cid": null,
        "warnings": ["the PDS refused the blob"],
    });
    let s = start(Plyr::scripted(1024, move |route, asked| match (route, asked) {
        ("progress", 0) => Some(Reply::Status(200, "data: {\"status\": \"processing\"}\n\n".into())),
        ("progress", 1) => Some(Reply::Unreachable),
        ("progress", _) => Some(Reply::Status(200, format!("data: {done}\n\n"))),
        _ => None,
    }));
    let published = s.publish(&set(), &path, &[], false).0.unwrap();
    let r = s.stop();
    assert_eq!(
        published,
        Published {
            url: Some("https://plyr.fm/track/9".into()),
            name: Some("plyr.fm".into()),
            record: None,
            warnings: vec!["the PDS refused the blob".into()],
        }
    );
    assert_eq!(r.requests.iter().filter(|(m, _)| m == "GET").count(), 3);

    let s = start(Plyr::scripted(1024, |route, _| (route == "progress").then(|| Reply::Status(200, String::new()))));
    let e = s.publish(&set(), &path, &[], false).0.unwrap_err();
    let r = s.stop();
    assert_eq!(
        kind(&e),
        ("external", format!("plyr.fm's progress kept stopping before upload {UPLOAD} was published").as_str())
    );
    assert_eq!(r.requests.iter().filter(|(m, _)| m == "GET").count(), TRIES as usize);
}

/// Any file goes as plyr.fm's parts: numbered from 1, end to end, each the part's size but the
/// last, which isn't empty.
#[hegel::test(test_cases = 200)]
fn parts_tile_any_file(tc: TestCase) {
    let size = tc.draw(gs::integers::<u64>().min_value(1).max_value(MAX_BYTES));
    // plyr.fm splits a file into 1,000 parts at most.
    let part = tc.draw(gs::integers::<u64>().min_value(size.div_ceil(1000)).max_value(64 << 20));
    let all: Vec<(u64, u64, u64)> = parts(size, part).collect();
    assert_eq!(all.len() as u64, size.div_ceil(part));
    let mut next = 0;
    for (i, (n, at, len)) in all.iter().enumerate() {
        assert_eq!((*n, *at), (i as u64 + 1, next));
        let last = i + 1 == all.len();
        assert!(if last { (1..=part).contains(len) } else { *len == part }, "{all:?}");
        next += len;
    }
    assert_eq!(next, size);
}

/// A tracklist of up to `most` entries, each maybe named, maybe placed in the recording.
#[hegel::test_helper]
fn tracklist(tc: &TestCase, most: usize) -> Vec<TracklistEntry> {
    let names =
        ["Young Fathers", "\u{dc}n\u{ef}c\u{f8}d\u{e9} Art\u{ef}st", " ", &"Long ".repeat(80)].map(String::from);
    let name = |tc: &TestCase| tc.draw(gs::optional(gs::sampled_from(names.to_vec())));
    let count = tc.draw(gs::integers::<usize>().max_value(most));
    (0..count)
        .map(|_| TracklistEntry {
            artist: name(tc),
            title: name(tc),
            start_seconds: tc.draw(gs::optional(gs::integers::<u32>().max_value(5 * 3600))),
        })
        .collect()
}

/// Whatever the set, its track has a title (the set's, else where and when) and the tracklist as
/// its description, a line each, both within plyr.fm's lengths: a tracklist too long keeps the
/// lines that fit, in order, and says how many more there are and why.
#[hegel::test(test_cases = 100)]
fn the_track_is_the_set_within_plyrs_lengths(tc: TestCase) {
    let words = gs::optional(gs::sampled_from(vec!["Closing set".to_string(), "  ".into(), "Ω".repeat(200)]));
    let set = PublishedSet {
        title: tc.draw(words),
        venue: tc.draw(gs::optional(gs::sampled_from(vec!["Warehouse".to_string(), String::new()]))),
        city: tc.draw(gs::optional(gs::just("Leeds".to_string()))),
        ..set()
    };
    let tracklist = tracklist(&tc, 40);
    let t = track::of(&set, &tracklist);

    assert!(!t.title.is_empty() && t.title.len() <= MAX_TITLE, "{t:?}");
    let given = set.title.as_deref().map(str::trim).filter(|s| !s.is_empty());
    match given {
        Some(title) => assert!(title.starts_with(&t.title), "{t:?}"),
        None => {
            let place = set.venue.as_deref().filter(|v| !v.is_empty()).or(set.city.as_deref()).unwrap_or("DJ set");
            assert_eq!(t.title, format!("{place}, 2026-10-03"));
        }
    }
    let cut = given.is_some_and(|g| g.len() > MAX_TITLE);
    assert_eq!(t.notes.iter().any(|n| n.starts_with("the title is cut")), cut);

    assert!(t.description.len() <= MAX_DESCRIPTION, "{}", t.description.len());
    let lines: Vec<String> = tracklist.iter().map(TracklistEntry::line).collect();
    let all = lines.join("\n");
    if all.len() <= MAX_DESCRIPTION {
        assert_eq!(t.description, all);
        assert!(!t.notes.iter().any(|n| n.starts_with("the description")), "{:?}", t.notes);
        return;
    }
    let (kept, more) = t.description.rsplit_once('\n').unwrap_or(("", &t.description));
    let kept: Vec<&str> = kept.lines().collect();
    assert_eq!(kept, lines[..kept.len()]);
    assert_eq!(more, format!("+ {} more", lines.len() - kept.len()));
    // The next line wouldn't have fit, with one fewer to say there are more of.
    let fewer = format!("+ {} more", lines.len() - kept.len() - 1);
    assert!(t.description.len() - more.len() + lines[kept.len()].len() + 1 + fewer.len() > MAX_DESCRIPTION);
    let note = format!(
        "the description lists {} of the {} tracks, as many as plyr.fm's {MAX_DESCRIPTION} bytes hold",
        kept.len(),
        lines.len()
    );
    assert!(t.notes.contains(&note), "{:?}", t.notes);
}

/// Where the count of the tracks left out loses a digit, the description keeps every line that
/// fits beside the shorter count.
#[test]
fn a_cut_tracklist_keeps_every_line_that_fits() {
    // 831 bytes a line: six of them, each with its newline, and "+ 9 more" make 5,000 bytes.
    let long = entry(Some(&"x".repeat(826)), None, None);
    let t = track::of(&set(), &vec![long; 15]);
    assert_eq!(t.description.len(), MAX_DESCRIPTION);
    assert!(t.description.ends_with("\n+ 9 more"), "{:?}", &t.description[4900..]);
}

/// However long its lines, a tracklist longer than a description holds keeps the lines that fit,
/// in order, then how many more there are, within plyr.fm's bytes, and says so.
#[hegel::test(test_cases = 50)]
fn prop_a_tracklist_too_long_keeps_the_lines_that_fit(tc: TestCase) {
    let n = tc.draw(gs::integers::<usize>().min_value(20).max_value(60));
    let artist = |tc: &TestCase| "\u{3a9}".repeat(tc.draw(gs::integers::<usize>().min_value(150).max_value(300)));
    let tracklist: Vec<TracklistEntry> = (0..n).map(|_| entry(Some(&artist(&tc)), None, None)).collect();
    let t = track::of(&set(), &tracklist);
    let lines: Vec<String> = tracklist.iter().map(TracklistEntry::line).collect();
    assert!(t.description.len() <= MAX_DESCRIPTION);
    let (kept, more) = t.description.rsplit_once('\n').unwrap();
    let kept: Vec<&str> = kept.lines().collect();
    assert_eq!(kept, lines[..kept.len()]);
    assert_eq!(more, format!("+ {} more", n - kept.len()));
    let note = format!(
        "the description lists {} of the {n} tracks, as many as plyr.fm's {MAX_DESCRIPTION} bytes hold",
        kept.len()
    );
    assert_eq!(t.notes, [note]);
}

/// What plyr.fm answers a step with in place of going on, in the property below: refusing the
/// token, refusing the request, failing it, an answer that can't be read, or (as the progress)
/// saying the upload failed.
const REFUSALS: [(u16, &str); 5] = [
    (401, "session_expired"),
    (400, "bad"),
    (500, "<html>"),
    (200, "{}"),
    (200, "data: {\"status\": \"failed\", \"error\": \"duplicate\"}"),
];

/// Whatever plyr.fm's part size, and however often each part fails to arrive before it does,
/// the file arrives whole, in order, and the publish answers the track's page, and its record when
/// plyr.fm names both its URI and CID. A part failing every time stops it there, and plyr.fm
/// refusing a step (or failing it every time) stops it at that step, saying why, nothing finished
/// past it. A dry run, a recording plyr.fm wouldn't take, or a host with the account signed out
/// sends nothing.
#[hegel::test(test_cases = 100)]
fn any_publish_sends_the_file_whole_or_stops_saying_why(tc: TestCase) {
    let format = tc.draw(gs::sampled_from(vec!["flac", "WAV", "aif", "mp3", "ogg", "aac"]));
    // 0: empty, 1: gone, 2: over plyr.fm's limit; else as long as it's drawn.
    let shape = tc.draw(gs::integers::<u8>().max_value(19));
    let signed_in = tc.draw(gs::weighted_booleans(0.95));
    let size = tc.draw(gs::integers::<u64>().min_value(1).max_value(4000));
    // 20 parts at most: each is a request through the host, and a few show the order.
    let part = tc.draw(gs::integers::<u64>().min_value(size.div_ceil(20)).max_value(1500));
    let count = size.div_ceil(part);
    let fails: Vec<u32> = (0..count)
        .map(|_| match tc.draw(gs::weighted_booleans(0.15)) {
            true => tc.draw(gs::integers::<u32>().min_value(1).max_value(TRIES)),
            false => 0,
        })
        .collect();
    let refused = match tc.draw(gs::weighted_booleans(0.5)) {
        true => Some((
            tc.draw(gs::sampled_from(vec!["start", "finish", "progress"])),
            tc.draw(gs::sampled_from(REFUSALS.to_vec())),
        )),
        false => None,
    };
    let dry_run = tc.draw(gs::weighted_booleans(0.1));
    let tracklist = tracklist(&tc, 3);
    let unreachable = tc.draw(gs::booleans());
    // Which halves of the track's record plyr.fm's last event names: its URI, its CID.
    let names = (tc.draw(gs::weighted_booleans(0.75)), tc.draw(gs::weighted_booleans(0.75)));
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), &format!("set.{format}"), if shape == 0 { 0 } else { size });
    match shape {
        1 => std::fs::remove_file(&path).unwrap(),
        2 => std::fs::File::options().write(true).open(&path).unwrap().set_len(MAX_BYTES + 1).unwrap(),
        _ => {}
    }

    let script_fails = fails.clone();
    let plyr = Plyr::scripted(part, move |route, asked| {
        if let Some((_, (code, body))) = refused.filter(|(at, _)| *at == route) {
            let body = match code {
                400 | 401 => json!({ "detail": body }).to_string(),
                _ => body.to_string(),
            };
            return Some(Reply::Status(code, body));
        }
        if route == "progress" {
            let (uri, cid) = (names.0.then_some(TRACK_URI), names.1.then_some(TRACK_CID));
            let completed = json!({ "status": "completed", "track_id": TRACK, "atproto_uri": uri, "atproto_cid": cid });
            return Some(Reply::Status(200, format!("data: {completed}\n\n")));
        }
        let n: usize = route.strip_prefix("part ")?.parse().ok()?;
        let failing = (asked as u32) < script_fails[n - 1];
        failing.then(|| if unreachable { Reply::Unreachable } else { status(503, "busy") })
    });
    // One session for every case: stopping one waits out the SDK's progress flusher, and a failing
    // case shrinks through hundreds of them.
    static SESSION: LazyLock<Mutex<Session>> = LazyLock::new(|| Mutex::new(start(Plyr::new(1))));
    let s = SESSION.lock().unwrap_or_else(PoisonError::into_inner);
    s.script(Plyr { signed_in, ..plyr });
    let (answer, upload) = s.publish(&set(), &path, &tracklist, dry_run);
    let r = s.received();
    drop(s);

    let refused_first = match (format, shape) {
        ("aac", _) | (_, 0 | 2) => Some("invalid"),
        (_, 1) => Some("io"),
        _ => None,
    };
    if let Some(kind) = refused_first {
        assert_eq!(answer.unwrap_err().kind(), Some(kind));
        assert!(r.requests.is_empty() && upload.is_empty());
        return;
    }
    if dry_run {
        assert_eq!(answer.unwrap().url, None);
        assert!(r.requests.is_empty() && upload.is_empty());
        return;
    }
    if !signed_in {
        assert_eq!(answer.unwrap_err().kind(), Some("access"));
        assert!(r.requests.is_empty() && upload.is_empty());
        return;
    }
    // What plyr.fm refusing a step fails the publish as.
    let kind_of = |code: u16| match code {
        401 => "permission",
        400 => "invalid",
        _ => "external",
    };
    if let Some(("start", (code, _))) = refused {
        assert_eq!(answer.unwrap_err().kind(), Some(kind_of(code)));
        assert_eq!(r.requests.len(), 1, "{:?}", r.requests);
        assert!(upload.is_empty());
        return;
    }
    let dead = fails.iter().position(|f| *f == TRIES);
    let reached = dead.map_or(count, |d| d as u64);
    let arrived: Vec<u64> = r.parts.iter().map(|(n, _)| *n).collect();
    assert_eq!(arrived, (1..=reached).collect::<Vec<_>>());
    let puts = r.requests.iter().filter(|(m, _)| m == "PUT").count() as u32;
    let tries: u32 = fails.iter().take(reached as usize + usize::from(dead.is_some())).map(|f| f + 1).sum();
    assert_eq!(puts, tries - u32::from(dead.is_some()));
    // The upload phase spans the parts, ended once every one has arrived.
    let ended = dead.is_none().then_some(None);
    assert_eq!(upload, [Some(Some((count, size))), ended].into_iter().flatten().collect::<Vec<_>>());
    let gets = r.requests.iter().filter(|(m, _)| m == "GET").count() as u32;
    match (dead, refused) {
        (Some(d), _) => {
            let e = answer.unwrap_err();
            assert_eq!(e.kind(), Some("external"), "{e:?}");
            assert!(e.message.contains(&format!("part {} of {count}", d + 1)), "{e:?}");
            assert!(r.finished.is_empty());
            assert_eq!(gets, 0);
        }
        (None, Some(("finish", (code, _)))) => {
            assert_eq!(answer.unwrap_err().kind(), Some(kind_of(code)));
            assert_eq!(gets, 0);
        }
        (None, Some((_, (code, body)))) => {
            assert_eq!(answer.unwrap_err().kind(), Some(kind_of(code)));
            // Asked again only while it fails, or stops before saying how it went.
            let said = matches!(code, 400 | 401) || body.starts_with("data:");
            assert_eq!(gets, if said { 1 } else { TRIES });
        }
        (None, None) => {
            let published = answer.unwrap();
            assert_eq!(published.url, Some(format!("https://plyr.fm/track/{TRACK}")));
            let record = (names == (true, true)).then(|| StrongRef { uri: TRACK_URI.into(), cid: TRACK_CID.into() });
            assert_eq!(published.record, record);
            assert_eq!(r.file(), std::fs::read(&path).unwrap());
            let description = track::of(&set(), &tracklist).description;
            assert_eq!(r.field("description"), Some(description.as_str()).filter(|d| !d.is_empty()));
        }
    }
}

/// The publish stops as soon as plyr.fm answers what it can't go on from, saying why and
/// sending nothing more: an answer that can't be read (to starting the upload, or to finishing
/// it), the upload, the track or its progress refused (the token too), the upload failed,
/// progress that keeps stopping before the track is published or says plyr.fm couldn't publish
/// it, and the account signed out; and cancelled between parts, between asking how the upload is
/// going, or while it waits to send a part again, it answers `cancelled`.
#[hegel::test(test_cases = 1)]
fn prop_a_publish_stops_as_soon_as_plyr_cant_go_on(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "set.flac", tc.draw(gs::integers::<u64>().min_value(2001).max_value(3000)));
    let processing =
        format!("data: {}\n\n", json!({ "upload_id": UPLOAD, "status": "processing", "phase": "pds_upload" }));
    let stopped = format!("plyr.fm's progress kept stopping before upload {UPLOAD} was published");
    let failed = r#"data: {"status": "failed", "error": "duplicate"}"#;
    let unreadable = "plyr.fm's answer can't be read: ";
    let cancelled = "operation cancelled";
    // The stream long enough to read that the cancel arrives meanwhile.
    let long = Reply::Status(200, format!("{}{processing}", ": still going\n".repeat(100_000)));
    let stops = [
        ("start", Reply::Status(200, "{}".into()), "external", unreadable),
        ("start", status(404, "gone"), "invalid", "plyr.fm refused the upload (404: gone)"),
        ("start", Reply::Status(502, "<html>".into()), "external", "plyr.fm failed the upload (502)"),
        (
            "start",
            status(401, "session_expired"),
            "permission",
            "plyr.fm refused the account's token (401: session_expired): sign in again",
        ),
        ("finish", Reply::Status(200, "{}".into()), "external", unreadable),
        ("finish", status(400, "bad"), "invalid", "plyr.fm refused the track (400: bad)"),
        ("progress", Reply::Status(200, processing.clone()), "external", stopped.as_str()),
        ("progress", Reply::Status(200, failed.into()), "external", "plyr.fm couldn't publish the track: duplicate"),
        ("progress", status(400, "bad"), "invalid", "plyr.fm refused the upload's progress (400: bad)"),
        // Signed out: the host refuses the first request.
        ("", Reply::Unreachable, "access", SIGNED_OUT),
        ("part 2", Reply::Cancels(None), "cancelled", cancelled),
        ("part 2", Reply::Cancels(Some(Box::new(Reply::Unreachable))), "cancelled", cancelled),
        ("progress", Reply::Cancels(Some(Box::new(long))), "cancelled", cancelled),
    ];
    for (at, reply, kind_said, said) in stops {
        plyr_stops(&path, at, reply, kind_said, said);
    }
}

/// However plyr.fm cuts the file into parts other than as the upload is, the publish is refused,
/// saying so, before any part goes.
#[hegel::test(test_cases = 10)]
fn prop_parts_that_dont_make_the_file_are_refused(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "set.flac", 3000);
    let size = tc.draw(gs::integers::<u64>().max_value(4000));
    let count = tc.draw(gs::integers::<u64>().max_value(8));
    tc.assume(size == 0 || count != 3000u64.div_ceil(size));
    let session = json!({ "upload_id": UPLOAD, "part_size_bytes": size, "part_count": count, "received_parts": [] });
    let said = format!("plyr.fm asked for {count} parts of {size} bytes");
    plyr_stops(&path, "start", Reply::Status(200, session.to_string()), "external", &said);
}

/// The publish of `path` with plyr.fm's first answer to `at` (the progress: every one) `reply`
/// (`at` empty: the account signed out) fails as `kind_said`, its message starting `said`, and
/// nothing more is sent. On a session of its own, so a request sent before a cancel arrives goes
/// to it, not to the next.
fn plyr_stops(path: &Path, at: &'static str, reply: Reply, kind_said: &str, said: &str) {
    let replying = reply.clone();
    let replies = move |route: &str, asked| route == at && (asked == 0 || route == "progress");
    let plyr = Plyr::scripted(1000, move |route, asked| replies(route, asked).then(|| replying.clone()));
    let s = start(Plyr { signed_in: !at.is_empty(), ..plyr });
    let (answer, _) = s.publish(&set(), path, &[], false);
    let r = s.stop();
    let e = answer.unwrap_err();
    assert_eq!(e.kind(), Some(kind_said), "{e:?}");
    assert!(e.message.starts_with(said), "{e:?}, not {said:?}");
    let gets = r.requests.iter().filter(|(m, _)| m == "GET").count();
    let cancelled = kind_said == "cancelled";
    match at {
        // One more part may have gone before the cancel reached the plugin, but nothing after:
        // the part that failed isn't kept, and the upload isn't finished.
        "part 2" if cancelled => {
            let retried = matches!(reply, Reply::Cancels(Some(_)));
            let parts = if retried { 1..=2 } else { 2..=3 };
            assert!(parts.contains(&r.parts.len()) && r.finished.is_empty() && gets == 0, "{:?}", r.requests);
        }
        // The track was finished; its progress asked for once more at most.
        "progress" if cancelled => assert!((1..=2).contains(&gets) && !r.finished.is_empty(), "{:?}", r.requests),
        "" => assert_eq!((r.requests.len(), r.signed_out), (0, 1)),
        "start" => assert_eq!(r.requests.len(), 1, "{:?}", r.requests),
        "finish" => assert_eq!((gets, r.parts.len()), (0, 3), "{:?}", r.requests),
        // Asked again only while the progress stops before saying how it went.
        _ => {
            let stopped = said.starts_with("plyr.fm's progress kept stopping");
            assert_eq!(gets, if stopped { TRIES as usize } else { 1 }, "{:?}", r.requests);
        }
    }
}

/// Whatever the tracklist, a dry run says what would go: the recording, its size, the track's
/// title and tags, and how many tracks are in its description, when there are any; one plyr.fm
/// wouldn't take is refused first.
#[hegel::test(test_cases = 10)]
fn prop_a_dry_run_says_what_would_go(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "set.flac", 3000);
    let tracklist = tracklist(&tc, 4);
    let s = start(Plyr::new(1000));
    let (answer, _) = s.publish(&set(), &path, &tracklist, true);
    assert!(s.stop().requests.is_empty());
    let track = track::of(&set(), &tracklist);
    let listed = match tracklist.len() {
        0 => String::new(),
        n => format!(", its {n} tracks in the description"),
    };
    let line = format!("set.flac (0.0 MiB) would go to plyr.fm as \"{}\", tagged dj mix{listed}", track.title);
    assert_eq!(answer.unwrap().warnings, [vec![line], track.notes].concat());

    // A recording in a format plyr.fm doesn't take, too big for it (sparse: it takes no room), or
    // not there, is refused first.
    let s = start(Plyr::new(1000));
    let e = s.publish(&set(), &recording(dir.path(), "set.aac", 10), &tracklist, true).0.unwrap_err();
    let said = "plyr.fm takes mp3, wav, m4a, aiff, flac, webm and ogg recordings, which set.aac isn't";
    assert_eq!((e.kind(), e.message.as_str()), (Some("invalid"), said));
    std::fs::File::options().write(true).open(&path).unwrap().set_len(MAX_BYTES + 1).unwrap();
    let e = s.publish(&set(), &path, &tracklist, true).0.unwrap_err();
    assert!(e.kind() == Some("invalid") && e.message.starts_with("plyr.fm takes recordings of up to "), "{e:?}");
    std::fs::remove_file(&path).unwrap();
    let e = s.publish(&set(), &path, &tracklist, true).0.unwrap_err();
    assert!(e.kind() == Some("io") && e.message.starts_with(&path.display().to_string()), "{e:?}");
    assert!(s.stop().requests.is_empty());
}
