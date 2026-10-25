use std::io::pipe;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use hegel::TestCase;
use hegel::generators as gs;
use loclib_plugin::{
    Capability, DestinationKind, Fetch, Fetched, HelloReply, Incoming, Manifest, PROTOCOL_VERSION, Peer,
    ProgressReport, Progressed, PublishedSet, SettingType, TracklistEntry, method,
};
use loclib_progress::{Event, Phase};
use percent_encoding::percent_decode_str;
use serde_json::json;

use super::item::{IDENTIFIERS, MAX_IDENTIFIER, MAX_VALUE, header_value};
use super::upload::{AUTHORIZATION, CHECK, S3, TRIES};
use super::*;

/// How the Archive answers a request through the host, in place of what it would: a status and
/// body, or the host failing to reach it.
#[derive(Clone, Debug)]
enum Reply {
    Status(u16, String),
    Unreachable,
}

/// The Archive's S3 refusing with `code`, as its `<Error>` says it.
fn s3_error(status: u16, code: &str, message: &str) -> Reply {
    let xml = format!(
        "<?xml version='1.0' encoding='UTF-8'?>\n<Error><Code>{code}</Code><Message>{message}</Message><Resource>r</Resource><RequestId>1</RequestId></Error>"
    );
    Reply::Status(status, xml)
}

fn slow_down() -> Reply {
    s3_error(503, "SlowDown", "Please reduce your request rate.")
}

/// Which request it is (`check ID`, `limit`, `put`) and how many times it was asked before: a
/// reply in place of the Archive's, or none for what the Archive says.
type Script = Box<dyn Fn(&str, usize) -> Option<Reply> + Send>;

/// A PUT as it arrived.
#[derive(Debug)]
struct Put {
    url: String,
    headers: Vec<(String, String)>,
    bytes: Vec<u8>,
}

impl Put {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// What the fake Archive was sent.
#[derive(Default)]
struct Received {
    /// Each request's method and URL, in order.
    requests: Vec<(String, String)>,
    puts: Vec<Put>,
}

impl Received {
    fn count(&self, method: &str) -> usize {
        self.requests.iter().filter(|(m, _)| m == method).count()
    }

    /// The identifiers checked, in order.
    fn checked(&self) -> Vec<&str> {
        self.requests.iter().filter_map(|(_, u)| u.split_once("&identifier=").map(|(_, id)| id)).collect()
    }
}

/// The Archive as `check_identifier.php` and ias3 answer, with the items `taken`, but as `script`
/// says, through a host with the keys set or not.
struct Archive {
    taken: Vec<String>,
    keys: bool,
    script: Script,
    received: Received,
}

impl Archive {
    fn new() -> Archive {
        Archive { taken: Vec::new(), keys: true, script: Box::new(|_, _| None), received: Received::default() }
    }

    fn scripted(script: impl Fn(&str, usize) -> Option<Reply> + Send + 'static) -> Archive {
        Archive { script: Box::new(script), ..Archive::new() }
    }

    /// The host's answer to `fetch`.
    fn answer(&mut self, fetch: &Fetch) -> Result<Fetched, RpcError> {
        let secret = fetch.url.contains("{secret:") || fetch.headers.values().any(|v| v.contains("{secret:"));
        if secret && !self.keys {
            // As the host fails a request whose secret isn't set.
            return Err(RpcError::failed(
                "invalid",
                "its secret access isn't set (locallibrary plugin settings archive access --secret)",
            ));
        }
        let r = &mut self.received;
        r.requests.push((fetch.method.clone(), fetch.url.clone()));
        let route = match (fetch.method.as_str(), fetch.url.as_str()) {
            ("GET", url) if url.starts_with(&format!("{CHECK}?output=json&identifier=")) => {
                format!("check {}", url.rsplit_once('=').unwrap().1)
            }
            ("GET", url) if url.starts_with(&format!("{S3}/?check_limit=1&accesskey={{secret:access}}&bucket=")) => {
                "limit".into()
            }
            ("PUT", url) if url.starts_with(&format!("{S3}/")) => "put".into(),
            other => panic!("the Archive has no {other:?}"),
        };
        let asked = r.requests.iter().filter(|(m, u)| (m, u) == (&fetch.method, &fetch.url)).count() - 1;
        let (status, body) = match (self.script)(&route, asked) {
            Some(Reply::Unreachable) => return Err(RpcError::failed("external", "connection reset")),
            Some(Reply::Status(status, body)) => (status, body),
            None => as_archive(&self.taken, r, &route, fetch),
        };
        Ok(Fetched { status, headers: Default::default(), body: Some(body), blob: None })
    }
}

/// What the Archive, with the items `taken`, answers `fetch`, the request `route` (as [`Script`]
/// names it), having been sent `r`.
fn as_archive(taken: &[String], r: &mut Received, route: &str, fetch: &Fetch) -> (u16, String) {
    if let Some(id) = route.strip_prefix("check ") {
        // check_identifier.php is case insensitive.
        let code = if taken.iter().any(|t| t.eq_ignore_ascii_case(id)) { "not_available" } else { "available" };
        return (200, json!({ "type": "success", "code": code, "message": "", "identifier": id }).to_string());
    }
    if route == "limit" {
        let detail = json!({ "accesskey_ration": 1200, "bucket_ration": 40, "total_tasks_queued": 0 });
        return (200, json!({ "bucket": "b", "accesskey": "a", "over_limit": 0, "detail": detail }).to_string());
    }
    assert_eq!(fetch.headers.get("authorization").map(String::as_str), Some(AUTHORIZATION), "{fetch:?}");
    assert_eq!((&fetch.body, &fetch.body_range, fetch.multipart.is_empty()), (&None, &None, true));
    let bytes = std::fs::read(fetch.body_blob.as_ref().expect("the recording")).unwrap();
    let headers = fetch.headers.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    r.puts.push(Put { url: fetch.url.clone(), headers, bytes });
    (200, String::new())
}

/// The plugin served on a thread over pipes, as [`serve`] serves it, past `hello`, with a host
/// whose fetches `archive` answers.
struct Session {
    host: Peer,
    served: JoinHandle<()>,
    answering: JoinHandle<()>,
    hello: HelloReply,
    /// Answering its fetches: scripted afresh by [`Session::script`].
    archive: Arc<Mutex<Archive>>,
}

fn start(archive: Archive) -> Session {
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serve(plugin_in, plugin_out));
    let (host, incoming) = Peer::new(from_plugin, to_plugin);
    let archive = Arc::new(Mutex::new(archive));
    let (to, answers) = (host.clone(), archive.clone());
    let answering = std::thread::spawn(move || {
        for msg in incoming {
            let Incoming::Request(req) = msg else { continue };
            assert_eq!(req.method, method::NET_FETCH, "the plugin reads nothing else");
            let fetch: Fetch = serde_json::from_value(req.params).unwrap();
            let answer = answers.lock().unwrap().answer(&fetch).map(|f| serde_json::to_value(f).unwrap());
            to.respond(&req.id, answer);
        }
    });
    let hello = json!({ "protocol": PROTOCOL_VERSION, "host": { "name": "locallibrary", "version": "0.1.0" } });
    let hello = host.request(method::HELLO, &hello).unwrap();
    Session { host, served, answering, hello, archive }
}

/// What a publish answered, and the uploads it started and ended: each start's total bytes, and
/// how many ended.
type Outcome = (Result<Published, RpcError>, Vec<u64>, usize);

impl Session {
    /// `set/publish` to `item` as the host sends it, with the `creator` setting when there's one.
    fn publish(&self, set: &PublishedSet, recording: &Path, tracklist: &[TracklistEntry], dry_run: bool) -> Outcome {
        self.publish_by(Some("DJ Kit"), set, recording, tracklist, dry_run)
    }

    fn publish_by(
        &self,
        creator: Option<&str>,
        set: &PublishedSet,
        recording: &Path,
        tracklist: &[TracklistEntry],
        dry_run: bool,
    ) -> Outcome {
        let settings = creator.map(|c| json!({ "creator": c })).unwrap_or_else(|| json!({}));
        let req = json!({ "destination": "item", "set": set, "recording": recording, "tracklist": tracklist, "dry_run": dry_run, "context": { "settings": settings } });
        let pending = self.host.send_request_reporting(method::SET_PUBLISH, &req).unwrap();
        let answer = pending.wait_timeout(Duration::from_secs(60)).expect("an answer in a minute");
        let (mut started, mut ended) = (Vec::new(), 0);
        for p in pending.progress() {
            match p {
                ProgressReport::Known(e) => match e.event {
                    Progressed::Event(Event::PhaseStart { phase: Phase::Upload, items: 1, bytes }) => {
                        started.push(bytes)
                    }
                    Progressed::Event(Event::PhaseEnd { phase: Phase::Upload }) => ended += 1,
                    other => panic!("{other:?}"),
                },
                ProgressReport::Unknown(other) => panic!("{other:?}"),
            }
        }
        (answer, started, ended)
    }

    /// Its fetches answered by `archive` from now on.
    fn script(&self, archive: Archive) {
        *self.archive.lock().unwrap() = archive;
    }

    /// What the Archive has been sent.
    fn received(&self) -> Received {
        std::mem::take(&mut self.archive.lock().unwrap().received)
    }

    /// Closes the plugin's input: it stops. What the Archive was sent (all of it: the plugin sends
    /// nothing once it has answered).
    fn stop(self) -> Received {
        let received = self.received();
        self.host.close();
        self.served.join().unwrap();
        self.answering.join().unwrap();
        received
    }
}

/// A recording `name` of `size` bytes.
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
        kind: Some("club".into()),
        venue: Some("Warehouse".into()),
        city: Some("Leeds".into()),
        ..PublishedSet::default()
    }
}

/// The identifier [`set`] by DJ Kit is published as, when it's free.
const ID: &str = "dj-kit-2026-10-03-closing-set";

fn entry(artist: Option<&str>, title: Option<&str>, start: Option<u32>) -> TracklistEntry {
    TracklistEntry { artist: artist.map(Into::into), title: title.map(Into::into), start_seconds: start }
}

fn kind(e: &RpcError) -> (&str, &str) {
    (e.kind().unwrap_or_default(), e.message.as_str())
}

/// What a header's value says: decoded, when it's sent as `uri(...)`.
fn decoded(value: &str) -> String {
    match value.strip_prefix("uri(").and_then(|v| v.strip_suffix(')')) {
        Some(encoded) => percent_decode_str(encoded).decode_utf8().unwrap().into_owned(),
        None => value.to_string(),
    }
}

/// What a publish that went answers, its item at `id`.
fn published(id: &str, warnings: &[&str]) -> Published {
    let processing = "the Archive can take a few minutes to show it, while it processes the upload";
    let warnings = warnings.iter().chain([&processing]).map(|w| w.to_string()).collect();
    Published { url: Some(format!("https://archive.org/details/{id}")), warnings }
}

/// It reaches archive.org and its S3 alone, publishes sets at one destination, and keeps the keys
/// as secrets.
#[test]
fn the_manifest_is_a_first_party_set_destination_on_the_archive() {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    manifest.validate(true).unwrap();
    let hosts = ["archive.org", "s3.us.archive.org"].map(|h| Capability::Net(h.into()));
    assert_eq!(manifest.capabilities, hosts);
    let [destination] = &manifest.contributes.destinations[..] else { panic!("{manifest:?}") };
    assert_eq!((manifest.id.as_str(), destination.name.as_str()), ("archive", "item"));
    assert_eq!(destination.kind, DestinationKind::Set);
    let settings: Vec<(&str, SettingType)> =
        manifest.contributes.settings.iter().map(|s| (s.key.as_str(), s.kind)).collect();
    assert_eq!(
        settings,
        [("access", SettingType::Secret), ("secret", SettingType::Secret), ("creator", SettingType::String)]
    );

    // Served as a real one is: it says it's this manifest, publishing sets through fetches.
    let s = start(Archive::new());
    let has = |f: &&str| s.hello.features.iter().any(|had| had == f);
    assert!(["destinations", "net"].iter().all(has), "{:?}", s.hello.features);
    let described: Manifest = s.host.request(method::DESCRIBE, &json!({})).unwrap();
    assert_eq!(described, manifest);
    s.stop();
}

/// The recording goes up in one PUT of the file itself to the identifier checked free, the keys,
/// the item's making and its metadata in the headers, and the item's page is the answer.
#[test]
fn a_set_goes_up_in_one_put_and_answers_its_page() {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "Closing Set.FLAC", 2500);
    let tracklist = [
        entry(Some("Young Fathers"), Some("In My View"), Some(0)),
        entry(None, None, Some(330)),
        entry(Some("Chris Lake & <Friends>"), Some("Turn Off the Lights"), Some(3723)),
        entry(Some("Someone"), Some("Unplaced"), None),
    ];
    let s = start(Archive::new());
    let (answer, started, ended) = s.publish(&set(), &path, &tracklist, false);
    let r = s.stop();
    assert_eq!(answer.unwrap(), published(ID, &[]));
    let requests: Vec<(&str, String)> = vec![
        ("GET", format!("{CHECK}?output=json&identifier={ID}")),
        ("GET", format!("{S3}/?check_limit=1&accesskey={{secret:access}}&bucket={ID}")),
        ("PUT", format!("{S3}/{ID}/Closing%20Set.FLAC")),
    ];
    assert_eq!(r.requests, requests.into_iter().map(|(m, u)| (m.to_string(), u)).collect::<Vec<_>>());
    let [put] = &r.puts[..] else { panic!("{:?}", r.puts) };
    assert_eq!(put.bytes, std::fs::read(&path).unwrap());
    assert_eq!((started, ended), (vec![2500], 1));
    let description = "00:00 Young Fathers - In My View<br>05:30 ID - ID<br>1:02:03 Chris Lake &amp; &lt;Friends&gt; - Turn Off the Lights<br>Someone - Unplaced";
    let headers = [
        ("authorization", AUTHORIZATION),
        ("x-archive-auto-make-bucket", "1"),
        ("x-archive-size-hint", "2500"),
        ("x-archive-meta-mediatype", "audio"),
        ("x-archive-meta-collection", "opensource_audio"),
        ("x-archive-meta-title", "Closing set"),
        ("x-archive-meta-date", "2026-10-03"),
        ("x-archive-meta-creator", "DJ Kit"),
        ("x-archive-meta-description", description),
        ("x-archive-meta01-subject", "dj mix"),
        ("x-archive-meta02-subject", "club"),
        ("x-archive-meta03-subject", "Warehouse"),
        ("x-archive-meta04-subject", "Leeds"),
    ];
    let sent: Vec<(&str, String)> = put.headers.iter().map(|(k, v)| (k.as_str(), decoded(v))).collect();
    assert_eq!(sent, headers.map(|(k, v)| (k, v.to_string())));
    // What has spaces in it goes encoded; plain ASCII as it is.
    assert_eq!(put.header("x-archive-meta-title"), Some("uri(Closing%20set)"));
    assert_eq!(put.header("x-archive-meta-date"), Some("2026-10-03"));
    // The derive the Archive plays the item from is left to run.
    assert_eq!(put.header("x-archive-queue-derive"), None);
}

/// Without a creator, title or tracklist, the item is a DJ set at its venue (else city), dated,
/// without a description; the subjects say each thing once.
#[test]
fn a_bare_set_is_named_after_where_and_when() {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "set.mp3", 10);
    let bare = PublishedSet { title: None, kind: Some("warehouse".into()), city: None, ..set() };
    let s = start(Archive::new());
    let (answer, _, _) = s.publish_by(None, &bare, &path, &[], false);
    let r = s.stop();
    assert_eq!(answer.unwrap().url.as_deref(), Some("https://archive.org/details/djset-2026-10-03-warehouse"));
    let put = &r.puts[0];
    assert_eq!(put.header("x-archive-meta-title").map(decoded), Some("Warehouse, 2026-10-03".into()));
    assert_eq!((put.header("x-archive-meta-creator"), put.header("x-archive-meta-description")), (None, None));
    let subjects: Vec<String> =
        put.headers.iter().filter(|(k, _)| k.ends_with("-subject")).map(|(_, v)| decoded(v)).collect();
    assert_eq!(subjects, ["dj mix", "warehouse"]);

    let nowhere = PublishedSet { title: None, venue: None, city: None, kind: None, ..set() };
    let s = start(Archive::new());
    let (answer, _, _) = s.publish_by(Some("  "), &nowhere, &path, &[], false);
    let r = s.stop();
    assert_eq!(answer.unwrap().url.as_deref(), Some("https://archive.org/details/djset-2026-10-03"));
    assert_eq!(r.puts[0].header("x-archive-meta-title").map(decoded), Some("DJ set, 2026-10-03".into()));
}

/// An identifier an item has (whatever its case) is passed over for the next with a suffix,
/// saying so; with every one up to `-10` taken, nothing goes.
#[test]
fn a_taken_identifier_gets_a_suffix() {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "set.wav", 10);
    let taken = vec![ID.to_uppercase(), format!("{ID}-2")];
    let s = start(Archive { taken, ..Archive::new() });
    let (answer, _, _) = s.publish(&set(), &path, &[], false);
    let r = s.stop();
    let id = format!("{ID}-3");
    let said = format!("https://archive.org/details/{ID} is taken, so it's {id}");
    assert_eq!(answer.unwrap(), published(&id, &[&said]));
    assert_eq!(r.checked(), [ID.to_string(), format!("{ID}-2"), id.clone()]);
    assert_eq!(r.puts[0].url, format!("{S3}/{id}/set.wav"));

    let taken = (1..=IDENTIFIERS).map(|n| item::suffixed(ID, n)).collect();
    let s = start(Archive { taken, ..Archive::new() });
    let e = s.publish(&set(), &path, &[], false).0.unwrap_err();
    let r = s.stop();
    let said = format!("archive.org/details/{ID} and the next ones up to {ID}-10 are taken");
    assert_eq!(kind(&e), ("external", said.as_str()));
    assert_eq!((r.checked().len(), r.puts.len()), (IDENTIFIERS as usize, 0));
}

/// The identifier check failing, or answering what can't be read, stops the publish before
/// anything goes, saying why.
#[test]
fn an_identifier_check_that_fails_stops_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "set.wav", 10);
    let cases = [
        (Reply::Unreachable, "external", "couldn't send the identifier check to the Archive: connection reset"),
        (Reply::Status(502, "<html>".into()), "external", "the Archive failed the identifier check (502)"),
        (Reply::Status(400, String::new()), "invalid", "the Archive refused the identifier check (400)"),
        (
            Reply::Status(200, json!({ "type": "error", "code": "invalid" }).to_string()),
            "external",
            "the Archive's identifier check said invalid of dj-kit-2026-10-03-closing-set",
        ),
    ];
    for (reply, expected_kind, expected) in cases {
        let s = start(Archive::scripted(move |route, _| route.starts_with("check").then(|| reply.clone())));
        let e = s.publish(&set(), &path, &[], false).0.unwrap_err();
        let r = s.stop();
        assert_eq!(kind(&e), (expected_kind, expected));
        assert_eq!(r.requests.len(), 1);
    }
    let s = start(Archive::scripted(|route, _| route.starts_with("check").then(|| Reply::Status(200, "{}".into()))));
    let e = s.publish(&set(), &path, &[], false).0.unwrap_err();
    s.stop();
    assert!(e.message.starts_with("the Archive's identifier check can't be read: missing field `code`"), "{e:?}");
}

/// A `503 SlowDown`, a PUT that doesn't arrive or a server error sends the recording again, after
/// a wait; while S3 says it's over its limit, the PUT waits its turn without going. One that
/// fails every time fails the publish after the last try, saying why.
#[test]
fn a_slow_down_sends_it_again() {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "set.flac", 300);
    let over = Reply::Status(200, json!({ "over_limit": 1 }).to_string());
    let s = start(Archive::scripted(move |route, asked| match (route, asked) {
        ("put", 0) => Some(slow_down()),
        ("put", 1) => Some(Reply::Unreachable),
        ("limit", 2) => Some(over.clone()),
        _ => None,
    }));
    let (answer, started, ended) = s.publish(&set(), &path, &[], false);
    let r = s.stop();
    assert_eq!(answer.unwrap(), published(ID, &[]));
    assert_eq!((r.count("PUT"), r.requests.iter().filter(|(_, u)| u.contains("check_limit")).count()), (3, 4));
    assert_eq!(r.puts.len(), 1);
    assert_eq!(r.puts[0].bytes, std::fs::read(&path).unwrap());
    // Each PUT is an upload of the whole recording.
    assert_eq!((started, ended), (vec![300; 3], 3));

    let s = start(Archive::scripted(|route, _| (route == "put").then(slow_down)));
    let e = s.publish(&set(), &path, &[], false).0.unwrap_err();
    let r = s.stop();
    assert_eq!(kind(&e), ("external", "the Archive failed set.flac (503 SlowDown: Please reduce your request rate.)"));
    assert_eq!(r.count("PUT"), TRIES as usize);
    assert_eq!(TRIES, 4);

    let over = Reply::Status(200, json!({ "over_limit": 1 }).to_string());
    let s = start(Archive::scripted(move |route, _| (route == "limit").then(|| over.clone())));
    let e = s.publish(&set(), &path, &[], false).0.unwrap_err();
    let r = s.stop();
    assert_eq!(kind(&e), ("external", "the Archive's S3 is over its limit for now"));
    assert_eq!(r.count("PUT"), 0);

    // A limit that can't be read, or isn't answered, lets the PUT go.
    for reply in [Reply::Status(200, "<html>".into()), Reply::Status(500, String::new()), Reply::Unreachable] {
        let s = start(Archive::scripted(move |route, _| (route == "limit").then(|| reply.clone())));
        assert!(s.publish(&set(), &path, &[], false).0.is_ok());
        assert_eq!(s.stop().puts.len(), 1);
    }
}

/// The Archive refusing the upload says why, in its words: keys it doesn't take, a request it
/// won't, a file it takes for spam. None is sent again.
#[test]
fn the_archive_refusing_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "set.ogg", 10);
    let cases = [
        (
            s3_error(403, "InvalidAccessKeyId", "The AWS Access Key Id you provided does not exist in our records."),
            "permission",
            "the Archive refused the keys (403 InvalidAccessKeyId: The AWS Access Key Id you provided does not exist in our records.): check them at archive.org/account/s3.php",
        ),
        (s3_error(400, "InvalidBucketName", ""), "invalid", "the Archive refused set.ogg (400 InvalidBucketName)"),
        (
            Reply::Status(503, "This item appears to be spam.".into()),
            "invalid",
            "the Archive took set.ogg for spam (503)",
        ),
    ];
    for (reply, expected_kind, expected) in cases {
        let s = start(Archive::scripted(move |route, _| (route == "put").then(|| reply.clone())));
        let e = s.publish(&set(), &path, &[], false).0.unwrap_err();
        let r = s.stop();
        assert_eq!(kind(&e), (expected_kind, expected));
        assert_eq!(r.count("PUT"), 1);
    }
}

/// Without the keys set, nothing goes up: the host's error says how to set them.
#[test]
fn without_the_keys_nothing_goes() {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "set.flac", 10);
    let s = start(Archive { keys: false, ..Archive::new() });
    let e = s.publish(&set(), &path, &[], false).0.unwrap_err();
    let r = s.stop();
    assert_eq!(
        kind(&e),
        ("invalid", "its secret access isn't set (locallibrary plugin settings archive access --secret)")
    );
    assert_eq!(r.requests.len(), 1, "only the identifier check: {:?}", r.requests);
    assert!(r.puts.is_empty());
}

/// An empty recording, or one that's gone, is refused before anything goes, saying why.
#[test]
fn an_empty_recording_is_refused_first() {
    let dir = tempfile::tempdir().unwrap();
    let s = start(Archive::new());
    let empty = recording(dir.path(), "set.aif", 0);
    for dry_run in [false, true] {
        let e = s.publish(&set(), &empty, &[], dry_run).0.unwrap_err();
        assert_eq!(kind(&e), ("invalid", "set.aif is empty"));
    }
    let gone = dir.path().join("gone.mp3");
    let e = s.publish(&set(), &gone, &[], false).0.unwrap_err();
    assert_eq!(e.kind(), Some("io"), "{e:?}");
    assert!(e.message.starts_with(&gone.display().to_string()), "{e:?}");
    assert!(s.stop().requests.is_empty());
}

/// A dry run says what would go (where, how big, the metadata, and what the Archive is for) and
/// sends nothing.
#[test]
fn a_dry_run_sends_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "set.mp3", 3 << 19);
    let s = start(Archive::new());
    let tracklist = [entry(Some("A"), Some("B"), Some(1)), entry(None, None, None)];
    let (answer, started, _) = s.publish(&set(), &path, &tracklist, true);
    let rights = "the Archive is for what you have the rights to share: it takes down mixes of others' records";
    let lines = [
        format!("set.mp3 (1.5 MiB) would go to the Internet Archive as https://archive.org/details/{ID} (or that with -2 and on, if it's taken)"),
        "titled \"Closing set\", by DJ Kit, dated 2026-10-03, in Community Audio, tagged dj mix, club, Warehouse, Leeds, its 2 tracks in the description".into(),
        rights.into(),
    ];
    assert_eq!(answer.unwrap(), Published { url: None, warnings: lines.to_vec() });
    let (answer, _, _) =
        s.publish_by(None, &PublishedSet { kind: None, venue: None, city: None, ..set() }, &path, &[], true);
    let about = "titled \"Closing set\", dated 2026-10-03, in Community Audio, tagged dj mix";
    assert_eq!(answer.unwrap().warnings[1], about);
    assert!(started.is_empty());
    assert!(s.stop().requests.is_empty());
}

/// A title too long for a header is cut, saying so, before it goes.
#[test]
fn a_long_title_is_cut_saying_so() {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "set.mp3", 10);
    let long = PublishedSet { title: Some("Ω".repeat(2000)), ..set() };
    let s = start(Archive::new());
    let published = s.publish(&long, &path, &[], false).0.unwrap();
    let r = s.stop();
    assert_eq!(published.warnings[0], format!("the title is cut to the {MAX_VALUE} bytes a header holds"));
    let title = r.puts[0].header("x-archive-meta-title").unwrap();
    assert!(title.len() <= MAX_VALUE, "{}", title.len());
    // Each Ω is six bytes encoded: as many as fit in uri(...).
    assert_eq!(decoded(title), "Ω".repeat((MAX_VALUE - 5) / 6));
}

/// Where the count of the tracks left out loses a digit, the description keeps every line that
/// fits beside the shorter count.
#[test]
fn a_cut_tracklist_keeps_every_line_that_fits() {
    // Encoded, a line of 330 x's is 339 bytes ("%20-%20ID" after them) and a break ("%3Cbr%3E")
    // 8: 23 of them, "uri()" and "+ 9 more" ("%2B%209%20more") make 8,000.
    let long = entry(Some(&"x".repeat(330)), None, None);
    let item = item::of(&set(), None, &vec![long; 32]);
    let value = header_value(&item.description);
    assert_eq!(value.len(), MAX_VALUE);
    assert!(item.description.ends_with("<br>+ 9 more"), "{:?}", &item.description[item.description.len() - 30..]);
    assert_eq!(item.notes, ["the description lists 23 of the 32 tracks, as many as a header holds"]);
}

/// A recording whose name needs it is sent to a URL that names it percent-encoded.
#[test]
fn the_recordings_name_is_encoded_in_the_url() {
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "Café #1 (live)?.wav", 10);
    let s = start(Archive::new());
    s.publish(&set(), &path, &[], false).0.unwrap();
    let r = s.stop();
    assert_eq!(r.puts[0].url, format!("{S3}/{ID}/Caf%C3%A9%20%231%20%28live%29%3F.wav"));
}

/// Any name, in any script: words drawn from these, some only space or punctuation.
#[hegel::test_helper]
fn name(tc: &TestCase) -> Option<String> {
    let words = ["DJ Kit", "Ünïcødé Ärtist", "東京", " ", "--", "a", "Long ", "uri(", "<&>", &"Ω".repeat(400)]
        .map(String::from);
    let count = tc.draw(gs::integers::<usize>().max_value(40));
    let words: Vec<String> = (0..count).map(|_| tc.draw(gs::sampled_from(words.to_vec()))).collect();
    tc.draw(gs::booleans()).then(|| words.concat())
}

/// Whatever the creator and title, the identifier is one the Archive takes (lowercase letters,
/// digits and single dashes between them, starting with a letter or digit, 5 to 80 characters),
/// with every suffix tried, and has the set's date in it.
#[hegel::test(test_cases = 200)]
fn any_identifier_is_one_the_archive_takes(tc: TestCase) {
    let creator = name(&tc);
    let title = name(&tc);
    let date = format!(
        "{:04}-{:02}-{:02}",
        tc.draw(gs::integers::<u32>().max_value(9999)),
        tc.draw(gs::integers::<u32>().min_value(1).max_value(12)),
        tc.draw(gs::integers::<u32>().min_value(1).max_value(31))
    );
    let id = item::identifier(creator.as_deref(), &date, title.as_deref());
    for n in 1..=IDENTIFIERS {
        let id = item::suffixed(&id, n);
        assert!((5..=MAX_IDENTIFIER).contains(&id.len()), "{id}");
        assert!(
            id.split('-').all(|w| !w.is_empty() && w.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())),
            "{id}"
        );
    }
    assert!(id.contains(&date), "{id} {date}");
}

/// A tracklist of up to `most` entries, each maybe named, maybe placed in the recording.
#[hegel::test_helper]
fn tracklist(tc: &TestCase, most: usize) -> Vec<TracklistEntry> {
    let names =
        ["Young Fathers", "\u{dc}n\u{ef}c\u{f8}d\u{e9} Art\u{ef}st", " ", "<&>", &"Long ".repeat(80)].map(String::from);
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

/// Whatever the set, its metadata goes in headers that fit a header line, each printable ASCII,
/// saying what the item has once decoded: a title (the set's, else where and when), the tracklist
/// as the description, a line each, and the subjects. A tracklist too long keeps the lines that
/// fit, in order, and says how many more there are and why.
#[hegel::test(test_cases = 100)]
fn the_metadata_fits_its_headers(tc: TestCase) {
    let set = PublishedSet { title: name(&tc), venue: name(&tc), kind: name(&tc), ..set() };
    let creator = name(&tc);
    let tracklist = tracklist(&tc, 80);
    let item = item::of(&set, creator.as_deref(), &tracklist);
    let headers = item.headers();
    for (_, value) in &headers {
        assert!(value.len() <= MAX_VALUE && value.bytes().all(|b| b.is_ascii_graphic()), "{value}");
    }
    let said = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| decoded(v));
    assert_eq!(said("x-archive-meta-title"), Some(item.title.clone()));
    assert_eq!(said("x-archive-meta-creator"), item.creator.clone());
    let subjects: Vec<String> =
        headers.iter().filter(|(k, _)| k.ends_with("-subject")).map(|(_, v)| decoded(v)).collect();
    assert_eq!(subjects, item.subjects);
    assert_eq!(subjects[0], "dj mix");
    for (i, s) in subjects.iter().enumerate() {
        assert!(!s.is_empty() && s.trim() == s, "{subjects:?}");
        assert!(!subjects[..i].iter().any(|had| had.eq_ignore_ascii_case(s)), "{subjects:?}");
    }

    let given = |s: &Option<String>| s.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let place = given(&set.venue).unwrap_or_else(|| "Leeds".into());
    let title = given(&set.title).unwrap_or_else(|| format!("{place}, 2026-10-03"));
    assert!(title.starts_with(&item.title) && !item.title.is_empty(), "{item:?}");
    let cut = header_value(&title).len() > MAX_VALUE;
    assert_eq!(item.notes.iter().any(|n| n.starts_with("the title is cut")), cut);

    let lines: Vec<String> = tracklist
        .iter()
        .map(|e| item::line(e).replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;"))
        .collect();
    assert_eq!(said("x-archive-meta-description").unwrap_or_default(), item.description);
    let all = lines.join("<br>");
    if header_value(&all).len() <= MAX_VALUE {
        assert_eq!(item.description, all);
        assert!(!item.notes.iter().any(|n| n.starts_with("the description")), "{:?}", item.notes);
        return;
    }
    let (kept, more) = item.description.rsplit_once("<br>").unwrap_or(("", &item.description));
    let kept: Vec<&str> = if kept.is_empty() { Vec::new() } else { kept.split("<br>").collect() };
    assert_eq!(kept, lines[..kept.len()]);
    assert_eq!(more, format!("+ {} more", lines.len() - kept.len()));
    // The next line wouldn't have fit, with one fewer to say there are more of.
    let next = [&kept.join("<br>"), lines[kept.len()].as_str(), &format!("+ {} more", lines.len() - kept.len() - 1)]
        .into_iter()
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("<br>");
    assert!(header_value(&next).len() > MAX_VALUE);
    let note = format!("the description lists {} of the {} tracks, as many as a header holds", kept.len(), lines.len());
    assert!(item.notes.contains(&note), "{:?}", item.notes);
}

/// A line's time, as `05:30` or past an hour `1:02:03`, reads back as the entry's.
#[hegel::test(test_cases = 100)]
fn a_lines_time_reads_back(tc: TestCase) {
    // Where a field turns over, as often as anywhere.
    let secs = match tc.draw(gs::booleans()) {
        true => tc.draw(gs::sampled_from(vec![59, 60, 3599, 3600, 3601, 35999, 36000])),
        false => tc.draw(gs::integers::<u32>()),
    };
    let line = item::line(&entry(Some("A"), Some("B"), Some(secs)));
    let (time, name) = line.split_once(' ').unwrap();
    assert_eq!(name, "A - B");
    let fields: Vec<u64> = time.split(':').map(|f| f.parse().unwrap()).collect();
    assert!(time.split(':').skip(1).all(|f| f.len() == 2), "{line}");
    match fields[..] {
        [m, s] => assert_eq!((secs < 3600, m * 60 + s), (true, u64::from(secs)), "{line}"),
        [h, m, s] => assert_eq!((secs >= 3600, h * 3600 + m * 60 + s), (true, u64::from(secs)), "{line}"),
        _ => panic!("{line}"),
    }
}

/// What the Archive answers in place of going on, in the property below: the identifier check
/// unreachable, failing, refusing, unreadable or saying something else; S3 refusing the keys or the
/// request, or taking the file for spam.
fn check_refusals() -> Vec<(Reply, &'static str)> {
    vec![
        (Reply::Unreachable, "external"),
        (Reply::Status(502, "<html>".into()), "external"),
        (Reply::Status(400, String::new()), "invalid"),
        (Reply::Status(200, "{}".into()), "external"),
        (Reply::Status(200, json!({ "code": "invalid" }).to_string()), "external"),
    ]
}

fn put_refusals() -> Vec<(Reply, &'static str)> {
    vec![
        (s3_error(403, "InvalidAccessKeyId", "no such key"), "permission"),
        (s3_error(400, "InvalidArgument", ""), "invalid"),
        (Reply::Status(503, "appears to be spam".into()), "invalid"),
    ]
}

/// Whatever is taken and however often the Archive slows the upload down or can't be reached
/// before taking it, the recording arrives whole at the first free identifier, and the publish
/// answers its page; a limit check that can't be read lets it go. The Archive refusing a step, or
/// failing it every time, stops it there, saying why. A dry run, an empty or missing recording, or
/// a host without the keys, sends nothing up.
#[hegel::test(test_cases = 100)]
fn any_publish_sends_the_file_whole_or_stops_saying_why(tc: TestCase) {
    // 0: empty, 1: gone; else as long as it's drawn.
    let shape = tc.draw(gs::integers::<u8>().max_value(19));
    let creator = tc.draw(gs::optional(gs::just("DJ Kit")));
    let taken = tc.draw(gs::integers::<u32>().max_value(IDENTIFIERS + 1));
    let check_refused = tc.draw(gs::optional(gs::sampled_from(check_refusals())));
    // 0: always over, 1: unreadable, 2: failing, 3: unreachable; else as the Archive says.
    let limit = tc.draw(gs::integers::<u8>().max_value(9));
    let fails = tc.draw(gs::integers::<u32>().max_value(TRIES));
    let unreachable = tc.draw(gs::booleans());
    let put_refused = tc.draw(gs::optional(gs::sampled_from(put_refusals())));
    let keys = tc.draw(gs::weighted_booleans(0.9));
    let dry_run = tc.draw(gs::weighted_booleans(0.1));
    let size = tc.draw(gs::integers::<u64>().min_value(1).max_value(4000));
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), "set.flac", if shape == 0 { 0 } else { size });
    if shape == 1 {
        std::fs::remove_file(&path).unwrap();
    }

    let (check, put) = (check_refused.clone(), put_refused.clone());
    let archive = Archive::scripted(move |route, asked| match route {
        "limit" => match limit {
            0 => Some(Reply::Status(200, json!({ "over_limit": 1 }).to_string())),
            1 => Some(Reply::Status(200, "<html>".into())),
            2 => Some(Reply::Status(500, String::new())),
            3 => Some(Reply::Unreachable),
            _ => None,
        },
        "put" if (asked as u32) < fails => Some(if unreachable { Reply::Unreachable } else { slow_down() }),
        "put" => put.as_ref().map(|(reply, _)| reply.clone()),
        _ => check.as_ref().map(|(reply, _)| reply.clone()),
    });
    let base = item::of(&set(), creator, &[]).identifier;
    let taken: Vec<String> = (1..=taken.min(IDENTIFIERS)).map(|n| item::suffixed(&base, n)).collect();
    // One session for every case: stopping one waits out the SDK's progress flusher, and a failing
    // case shrinks through hundreds of them.
    static SESSION: LazyLock<Mutex<Session>> = LazyLock::new(|| Mutex::new(start(Archive::new())));
    let s = SESSION.lock().unwrap_or_else(PoisonError::into_inner);
    s.script(Archive { taken: taken.clone(), keys, ..archive });
    let (answer, started, ended) = s.publish_by(creator, &set(), &path, &[], dry_run);
    let r = s.received();
    drop(s);

    assert_eq!(started.len(), ended);
    let refused_first = match shape {
        0 => Some("invalid"),
        1 => Some("io"),
        _ => None,
    };
    if let Some(kind) = refused_first {
        assert_eq!(answer.unwrap_err().kind(), Some(kind));
        assert!(r.requests.is_empty());
        return;
    }
    if dry_run {
        let published = answer.unwrap();
        assert_eq!(published.url, None);
        assert!(published.warnings[0].contains(&format!("https://archive.org/details/{base} ")), "{published:?}");
        assert!(r.requests.is_empty());
        return;
    }
    if let Some((_, kind)) = check_refused {
        assert_eq!(answer.unwrap_err().kind(), Some(kind));
        assert_eq!(r.requests.len(), 1);
        return;
    }
    let checks = (taken.len() + 1).min(IDENTIFIERS as usize);
    assert_eq!(r.checked().len(), checks);
    if taken.len() == IDENTIFIERS as usize {
        assert_eq!(answer.unwrap_err().kind(), Some("external"));
        assert_eq!(r.requests.len(), checks);
        return;
    }
    if !keys {
        assert_eq!(answer.unwrap_err().kind(), Some("invalid"));
        assert_eq!(r.requests.len(), checks);
        return;
    }
    let puts = r.count("PUT");
    if limit == 0 {
        let e = answer.unwrap_err();
        assert_eq!((e.kind(), puts, started.len()), (Some("external"), 0, 0), "{e:?}");
        return;
    }
    assert_eq!(puts, (fails + 1).min(TRIES) as usize);
    assert!(started.iter().all(|b| *b == size) && started.len() == puts, "{started:?}");
    if fails == TRIES {
        let e = answer.unwrap_err();
        assert_eq!(e.kind(), Some("external"), "{e:?}");
        assert!(r.puts.is_empty());
        return;
    }
    if let Some((_, kind)) = put_refused {
        assert_eq!(answer.unwrap_err().kind(), Some(kind));
        assert!(r.puts.is_empty());
        return;
    }
    let id = item::suffixed(&base, taken.len() as u32 + 1);
    let published = answer.unwrap();
    assert_eq!(published.url, Some(format!("https://archive.org/details/{id}")));
    assert_eq!(published.warnings.iter().any(|w| w.ends_with(&format!(" is taken, so it's {id}"))), !taken.is_empty());
    let [put] = &r.puts[..] else { panic!("{:?}", r.puts) };
    assert_eq!(put.url, format!("{S3}/{id}/set.flac"));
    assert_eq!(put.bytes, std::fs::read(&path).unwrap());
    assert_eq!(put.header("x-archive-meta-creator"), creator.map(header_value).as_deref());
}

/// A value's header is plain only when it's printable ASCII and doesn't look encoded.
#[test]
fn a_value_is_encoded_only_when_it_needs_it() {
    assert_eq!(header_value("2026-10-03"), "2026-10-03");
    assert_eq!(header_value("uri(x)"), "uri(uri%28x%29)");
    assert_eq!(header_value("a\nb"), "uri(a%0Ab)");
}
