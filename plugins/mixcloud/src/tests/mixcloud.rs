//! A host whose `net/fetch` Mixcloud answers as its upload does (mixcloud.com/developers,
//! "Uploading shows"), but as a test scripts it, and the plugin served to it over pipes.

use std::io::pipe;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use loclib_plugin::{
    Fetch, Fetched, HelloReply, Incoming, PROTOCOL_VERSION, Peer, Pending, ProgressReport, Progressed, Published,
    PublishedSet, RpcError, TracklistEntry, method,
};
use loclib_progress::{Event, Phase};
use serde_json::{Value, json};

use crate::serve;
use crate::upload::UPLOAD;

/// The show the fake Mixcloud makes, and its page.
const KEY: &str = "/djkit/closing-set/";
pub(super) const PAGE: &str = "https://www.mixcloud.com/djkit/closing-set/";

/// As the host fails a request for a Mixcloud account that's signed out.
pub(super) const SIGNED_OUT: &str = "the mixcloud account djkit is signed out: sign in again";

/// A file `name` of `size` bytes.
pub(super) fn file(dir: &Path, name: &str, size: u64) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, (0..size).map(|i| (i * 31 % 251) as u8).collect::<Vec<u8>>()).unwrap();
    path
}

/// How Mixcloud answers an upload through the host, in place of what it would: a status, headers
/// and body, or the host failing to reach it.
#[derive(Clone, Debug)]
pub(super) enum Reply {
    Status(u16, Vec<(&'static str, String)>, String),
    Unreachable,
}

/// Mixcloud's error, as its answers have one.
pub(super) fn error(status: u16, kind: &str, message: &str) -> Reply {
    Reply::Status(status, Vec::new(), json!({ "error": { "type": kind, "message": message } }).to_string())
}

/// Mixcloud over its rate limit, as its developer docs show it: `Retry-After`, and `retry_after`
/// in the body, each when it's given.
pub(super) fn rate_limited(header: Option<u64>, body: Option<u64>) -> Reply {
    let secs = header.or(body).unwrap_or_default();
    let message = format!("You have hit your rate limit. Retry after {secs} seconds.");
    let mut said = json!({ "message": message, "type": "RateLimitException" });
    if let Some(body) = body {
        said["retry_after"] = json!(body);
    }
    let headers = header.map(|h| ("retry-after", h.to_string())).into_iter().collect();
    Reply::Status(403, headers, json!({ "error": said }).to_string())
}

/// How many uploads came before this one: a reply in place of Mixcloud's, or none for what
/// Mixcloud says.
pub(super) type Script = Box<dyn Fn(usize) -> Option<Reply> + Send>;

/// A form's part as it arrived: a field's text, or a file's name and every byte of it.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Part {
    Text(String),
    File(String, Vec<u8>),
}

/// What the fake Mixcloud was sent: each upload's form, its parts by name in order.
pub(super) type Received = Vec<Vec<(String, Part)>>;

/// Mixcloud as a test scripts it, through a host with the account signed in or not.
pub(super) struct Mixcloud {
    signed_in: bool,
    script: Script,
    received: Received,
}

impl Mixcloud {
    pub(super) fn new() -> Mixcloud {
        Mixcloud::scripted(|_| None)
    }

    /// Through a host the account isn't signed in on.
    pub(super) fn signed_out() -> Mixcloud {
        Mixcloud { signed_in: false, ..Mixcloud::new() }
    }

    pub(super) fn scripted(script: impl Fn(usize) -> Option<Reply> + Send + 'static) -> Mixcloud {
        Mixcloud { signed_in: true, script: Box::new(script), received: Received::new() }
    }

    /// The host's answer to `fetch`.
    fn answer(&mut self, fetch: &Fetch) -> Result<Fetched, RpcError> {
        if !self.signed_in {
            return Err(RpcError::failed("access", SIGNED_OUT));
        }
        // The token goes in the query alone, and the form is all there is to send.
        assert_eq!((fetch.method.as_str(), fetch.url.as_str()), ("POST", UPLOAD));
        assert!(fetch.headers.is_empty(), "{:?}", fetch.headers);
        assert_eq!((&fetch.body, &fetch.body_blob, &fetch.body_range, &fetch.into_blob), (&None, &None, &None, &None));
        let form = fetch
            .multipart
            .iter()
            .map(|p| {
                let part = match (&p.value, &p.file) {
                    (Some(text), None) => Part::Text(text.clone()),
                    (None, Some(file)) => {
                        // Sent whole, as the host streams it from the file.
                        assert_eq!(p.range, None, "{p:?}");
                        assert!(file.is_absolute(), "{p:?}");
                        Part::File(p.filename.clone().expect("a file part is named"), std::fs::read(file).unwrap())
                    }
                    _ => panic!("{p:?} is text or a file"),
                };
                (p.name.clone(), part)
            })
            .collect();
        let before = self.received.len();
        self.received.push(form);
        let (status, headers, body) = match (self.script)(before) {
            Some(Reply::Unreachable) => return Err(RpcError::failed("external", "connection reset")),
            Some(Reply::Status(status, headers, body)) => (status, headers, body),
            None => as_mixcloud(&self.received[before]),
        };
        let headers = headers.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        Ok(Fetched { status, headers, body: Some(body), blob: None })
    }
}

/// What Mixcloud answers `form`: the show, once it has a recording and a name and each section is
/// a track; else what's wrong with it.
fn as_mixcloud(form: &[(String, Part)]) -> (u16, Vec<(&'static str, String)>, String) {
    let text = |name: &str| {
        form.iter().find_map(|(n, p)| match p {
            Part::Text(t) if n == name => Some(t.as_str()),
            _ => None,
        })
    };
    let mut details = serde_json::Map::new();
    let has_audio = form.iter().any(|(n, p)| n == "mp3" && matches!(p, Part::File(_, bytes) if !bytes.is_empty()));
    if !has_audio {
        details.insert("mp3".into(), json!(["This field is required."]));
    }
    if text("name").is_none_or(str::is_empty) {
        details.insert("name".into(), json!(["This field is required."]));
    }
    for (name, _) in form.iter().filter(|(n, _)| n.ends_with("-start_time")) {
        let n = name.trim_end_matches("-start_time");
        if text(&format!("{n}-artist")).is_none_or(str::is_empty)
            || text(&format!("{n}-song")).is_none_or(str::is_empty)
        {
            details.insert(n.into(), json!(["A track needs an artist and a song."]));
        }
    }
    if !details.is_empty() {
        let message = "Some of the fields you posted were invalid.";
        let body = json!({ "error": { "type": "PostValidationError", "message": message }, "details": details });
        return (400, Vec::new(), body.to_string());
    }
    let name = text("name").unwrap();
    (
        200,
        Vec::new(),
        json!({ "result": { "success": true, "message": format!("Uploaded {name}"), "key": KEY } }).to_string(),
    )
}

/// The plugin served on a thread over pipes, as [`serve`] serves it, past `hello`, with a host
/// whose fetches `mixcloud` answers.
pub(super) struct Session {
    pub(super) host: Peer,
    served: JoinHandle<()>,
    answering: JoinHandle<()>,
    pub(super) hello: HelloReply,
    mixcloud: Arc<Mutex<Mixcloud>>,
    /// The requests' scratch folder.
    pub(super) scratch: tempfile::TempDir,
}

pub(super) fn start(mixcloud: Mixcloud) -> Session {
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serve(plugin_in, plugin_out));
    let (host, incoming) = Peer::new(from_plugin, to_plugin);
    let mixcloud = Arc::new(Mutex::new(mixcloud));
    let (to, answers) = (host.clone(), mixcloud.clone());
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
    Session { host, served, answering, hello, mixcloud, scratch: tempfile::tempdir().unwrap() }
}

/// What a publish answered, and the uploads it started and ended: each start's total bytes, and
/// how many ended.
pub(super) type Outcome = (Result<Published, RpcError>, Vec<u64>, usize);

/// The uploads `pending` started (each one's bytes) and ended, of the progress it has reported.
fn uploads(pending: &Pending) -> (Vec<u64>, usize) {
    let (mut started, mut ended) = (Vec::new(), 0);
    for p in pending.progress() {
        match p {
            ProgressReport::Known(e) => match e.event {
                Progressed::Event(Event::PhaseStart { phase: Phase::Upload, items: 1, bytes }) => started.push(bytes),
                Progressed::Event(Event::PhaseEnd { phase: Phase::Upload }) => ended += 1,
                other => panic!("{other:?}"),
            },
            ProgressReport::Unknown(other) => panic!("{other:?}"),
        }
    }
    (started, ended)
}

impl Session {
    /// `set/publish` to `show` as the host sends it, with a scratch folder and `settings`.
    pub(super) fn publish(
        &self,
        set: &PublishedSet,
        recording: &Path,
        tracklist: &[TracklistEntry],
        settings: Value,
        dry_run: bool,
    ) -> Outcome {
        let pending = self.send(set, recording, tracklist, settings, dry_run, true);
        let answer = pending.wait_timeout(Duration::from_secs(60)).expect("an answer in a minute");
        let (started, ended) = uploads(&pending);
        (answer, started, ended)
    }

    /// [`Session::publish`]'s request, under way, from a host that gives a scratch folder or not.
    pub(super) fn send(
        &self,
        set: &PublishedSet,
        recording: &Path,
        tracklist: &[TracklistEntry],
        settings: Value,
        dry_run: bool,
        scratch: bool,
    ) -> Pending {
        let mut context = json!({ "settings": settings });
        if scratch {
            context["scratch"] = json!(self.scratch.path());
        }
        let req = json!({ "destination": "show", "set": set, "recording": recording, "tracklist": tracklist, "dry_run": dry_run, "context": context });
        self.host.send_request_reporting(method::SET_PUBLISH, &req).unwrap()
    }

    /// Its fetches answered by `mixcloud` from now on.
    pub(super) fn script(&self, mixcloud: Mixcloud) {
        *self.mixcloud.lock().unwrap() = mixcloud;
    }

    /// What Mixcloud has been sent since it was last asked.
    pub(super) fn received(&self) -> Received {
        std::mem::take(&mut self.mixcloud.lock().unwrap().received)
    }

    /// How many uploads Mixcloud has been sent, without forgetting them.
    pub(super) fn asked(&self) -> usize {
        self.mixcloud.lock().unwrap().received.len()
    }

    /// Closes the plugin's input: it stops. What Mixcloud was sent.
    pub(super) fn stop(self) -> Received {
        let received = self.received();
        self.host.close();
        self.served.join().unwrap();
        self.answering.join().unwrap();
        received
    }
}
