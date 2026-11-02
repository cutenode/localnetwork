//! A host whose `net/fetch` YouTube answers as its resumable upload does
//! (`guides/using_resumable_upload_protocol`), but as a test scripts it, and the plugin served to
//! it over pipes.

use std::io::{Read, Seek, SeekFrom, pipe};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use loclib_plugin::{
    Fetch, Fetched, HelloReply, Incoming, PROTOCOL_VERSION, Peer, Pending, ProgressReport, Progressed, Published,
    PublishedSet, RpcError, TracklistEntry, method,
};
use loclib_progress::Event;
use serde_json::{Value, json};

use crate::serve;
use crate::upload::{AUTHORIZATION, QUANTUM, START};

/// Where the fake YouTube's session is, and the video it makes.
pub(super) const SESSION: &str =
    "https://www.googleapis.com/upload/youtube/v3/videos?uploadType=resumable&upload_id=xa298";
pub(super) const VIDEO: &str = "dQw4w9WgXcQ";

/// As the host fails a request for a Google account that's signed out.
pub(super) const SIGNED_OUT: &str = "the google account dj@example.com is signed out: sign in again";

/// How YouTube answers a request through the host, in place of what it would: a status and body,
/// the host failing to reach it, or a chunk arriving of which YouTube keeps only so many bytes.
#[derive(Clone, Debug)]
pub(super) enum Reply {
    Status(u16, String),
    Unreachable,
    Keeps(u64),
}

/// Google's error, as YouTube answers one.
pub(super) fn refusal(code: u16, reason: &str, message: &str) -> Reply {
    let errors = json!([{ "domain": "youtube.video", "reason": reason, "message": message }]);
    Reply::Status(code, json!({ "error": { "code": code, "message": message, "errors": errors } }).to_string())
}

/// Which request it is (`start`, `chunk` or `status`, asking how much YouTube has) and how many of
/// that kind came before: a reply in place of YouTube's, or none for what YouTube says.
pub(super) type Script = Box<dyn Fn(&str, usize) -> Option<Reply> + Send>;

/// What the fake YouTube was sent.
#[derive(Default, Debug)]
pub(super) struct Received {
    /// Each request's kind, method and URL, in order.
    pub(super) requests: Vec<(String, String, String)>,
    /// The session's headers, but the token's.
    pub(super) started: Vec<(String, String)>,
    /// The video's resource as the session started with it.
    pub(super) resource: Option<Value>,
    /// Each chunk as it arrived: where it starts and how long it is.
    pub(super) chunks: Vec<(u64, u64)>,
    /// The video as YouTube has it.
    pub(super) video: Vec<u8>,
}

impl Received {
    pub(super) fn count(&self, kind: &str) -> usize {
        self.requests.iter().filter(|(k, ..)| k == kind).count()
    }
}

/// YouTube as a test scripts it, through a host with the account signed in or not.
pub(super) struct YouTube {
    signed_in: bool,
    script: Script,
    received: Received,
    /// The size the session was started with.
    size: u64,
}

impl YouTube {
    pub(super) fn new() -> YouTube {
        YouTube::scripted(|_, _| None)
    }

    /// Through a host the account isn't signed in on.
    pub(super) fn signed_out() -> YouTube {
        YouTube { signed_in: false, ..YouTube::new() }
    }

    pub(super) fn scripted(script: impl Fn(&str, usize) -> Option<Reply> + Send + 'static) -> YouTube {
        YouTube { signed_in: true, script: Box::new(script), received: Received::default(), size: 0 }
    }

    /// The host's answer to `fetch`.
    fn answer(&mut self, fetch: &Fetch) -> Result<Fetched, RpcError> {
        if !self.signed_in {
            return Err(RpcError::failed("access", SIGNED_OUT));
        }
        assert_eq!(fetch.headers.get("Authorization").map(String::as_str), Some(AUTHORIZATION), "{fetch:?}");
        let kind = match (fetch.method.as_str(), fetch.url.as_str()) {
            ("POST", START) => "start",
            ("PUT", SESSION) if fetch.body_blob.is_some() => "chunk",
            ("PUT", SESSION) => "status",
            other => panic!("YouTube has no {other:?}"),
        };
        let r = &mut self.received;
        let before = r.count(kind);
        r.requests.push((kind.into(), fetch.method.clone(), fetch.url.clone()));
        let reply = (self.script)(kind, before);
        let (status, headers, body) = match reply {
            Some(Reply::Unreachable) => return Err(RpcError::failed("external", "connection reset")),
            Some(Reply::Status(status, body)) => (status, Vec::new(), body),
            Some(Reply::Keeps(n)) => {
                self.chunk(fetch, Some(n));
                self.has()
            }
            None => match kind {
                "start" => self.start(fetch),
                "chunk" => {
                    self.chunk(fetch, None);
                    self.has()
                }
                _ => {
                    let range = fetch.headers.get("Content-Range").map(String::as_str);
                    assert_eq!(range, Some(format!("bytes */{}", self.size).as_str()));
                    assert_eq!(fetch.body.as_deref(), Some(""), "asked with an empty body");
                    self.has()
                }
            },
        };
        let headers = headers.into_iter().map(|(k, v): (&str, String)| (k.to_string(), v)).collect();
        Ok(Fetched { status, headers, body: Some(body), blob: None })
    }

    fn start(&mut self, fetch: &Fetch) -> (u16, Vec<(&'static str, String)>, String) {
        let r = &mut self.received;
        r.started =
            fetch.headers.iter().filter(|(k, _)| *k != "Authorization").map(|(k, v)| (k.clone(), v.clone())).collect();
        r.resource = Some(serde_json::from_str(fetch.body.as_deref().unwrap()).unwrap());
        self.size = fetch.headers["X-Upload-Content-Length"].parse().unwrap();
        (200, vec![("location", SESSION.into())], String::new())
    }

    /// Takes the chunk `fetch` sends, keeping `keeps` bytes of it when that's said.
    fn chunk(&mut self, fetch: &Fetch, keeps: Option<u64>) {
        let range = fetch.body_range.expect("a chunk sends part of the video");
        let length = range.length.expect("a chunk says how long it is");
        let said = format!("bytes {}-{}/{}", range.offset, range.offset + length - 1, self.size);
        assert_eq!(fetch.headers.get("Content-Range"), Some(&said));
        assert_eq!(fetch.headers.get("Content-Type").map(String::as_str), Some("video/mp4"));
        let r = &mut self.received;
        assert_eq!(range.offset, r.video.len() as u64, "a chunk goes on from where YouTube got to");
        let last = range.offset + length == self.size;
        assert!(
            last || length.is_multiple_of(QUANTUM),
            "a chunk but the last is a multiple of {QUANTUM} bytes: {length}"
        );
        let mut file = std::fs::File::open(fetch.body_blob.as_ref().unwrap()).unwrap();
        file.seek(SeekFrom::Start(range.offset)).unwrap();
        let mut bytes = Vec::new();
        file.take(keeps.unwrap_or(length).min(length)).read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes.len() as u64, keeps.unwrap_or(length).min(length), "the video is there to send");
        r.chunks.push((range.offset, length));
        r.video.extend(bytes);
    }

    /// What YouTube answers once it has what it has: the video once it's all there, else `308`
    /// with how much it has.
    fn has(&self) -> (u16, Vec<(&'static str, String)>, String) {
        let r = &self.received;
        let had = r.video.len() as u64;
        if had == self.size {
            let resource = r.resource.as_ref().unwrap();
            let video = json!({ "kind": "youtube#video", "id": VIDEO, "snippet": resource["snippet"],
                "status": { "uploadStatus": "uploaded", "privacyStatus": resource["status"]["privacyStatus"] } });
            return (201, Vec::new(), video.to_string());
        }
        let range = (had > 0).then(|| ("range", format!("bytes=0-{}", had - 1)));
        (308, range.into_iter().collect(), String::new())
    }
}

/// The plugin served on a thread over pipes, as [`serve`] serves it, past `hello`, with a host
/// whose fetches `youtube` answers.
pub(super) struct Session {
    pub(super) host: Peer,
    served: JoinHandle<()>,
    answering: JoinHandle<()>,
    pub(super) hello: HelloReply,
    youtube: Arc<Mutex<YouTube>>,
    /// The requests' scratch folder.
    pub(super) scratch: tempfile::TempDir,
}

pub(super) fn start(youtube: YouTube) -> Session {
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serve(plugin_in, plugin_out));
    let (host, incoming) = Peer::new(from_plugin, to_plugin);
    let youtube = Arc::new(Mutex::new(youtube));
    let (to, answers) = (host.clone(), youtube.clone());
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
    Session { host, served, answering, hello, youtube, scratch: tempfile::tempdir().unwrap() }
}

/// What a publish answered, and the progress it reported.
pub(super) type Outcome = (Result<Published, RpcError>, Vec<Event>);

/// The progress events `pending` has reported since it was last asked.
pub(super) fn events(pending: &Pending) -> Vec<Event> {
    let events = pending.progress().filter_map(|p| match p {
        ProgressReport::Known(e) => match e.event {
            Progressed::Event(event) => Some(event),
            Progressed::Step(_) => None,
        },
        ProgressReport::Unknown(_) => None,
    });
    events.collect()
}

impl Session {
    /// `set/publish` to `video` as the host sends it, with a scratch folder and `settings`: what
    /// it answered, and the progress it reported.
    pub(super) fn publish(
        &self,
        set: &PublishedSet,
        recording: &Path,
        tracklist: &[TracklistEntry],
        settings: Value,
        dry_run: bool,
    ) -> Outcome {
        let pending = self.send(set, recording, tracklist, settings, dry_run);
        let answer = pending.wait_timeout(Duration::from_secs(60)).expect("an answer in a minute");
        (answer, events(&pending))
    }

    /// [`Session::publish`]'s request, under way.
    pub(super) fn send(
        &self,
        set: &PublishedSet,
        recording: &Path,
        tracklist: &[TracklistEntry],
        settings: Value,
        dry_run: bool,
    ) -> Pending {
        let context = json!({ "scratch": self.scratch.path(), "settings": settings });
        let req = json!({ "destination": "video", "set": set, "recording": recording, "tracklist": tracklist, "dry_run": dry_run, "context": context });
        self.host.send_request_reporting(method::SET_PUBLISH, &req).unwrap()
    }

    /// Its fetches answered by `youtube` from now on.
    pub(super) fn script(&self, youtube: YouTube) {
        *self.youtube.lock().unwrap() = youtube;
    }

    /// What YouTube has been sent.
    pub(super) fn received(&self) -> Received {
        std::mem::take(&mut self.youtube.lock().unwrap().received)
    }

    /// Closes the plugin's input: it stops. What YouTube was sent.
    pub(super) fn stop(self) -> Received {
        let received = self.received();
        self.host.close();
        self.served.join().unwrap();
        self.answering.join().unwrap();
        received
    }
}
