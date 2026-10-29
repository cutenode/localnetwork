//! A host whose `net/fetch` SoundCloud answers as its API does (`POST /tracks`, `GET /me/tracks`),
//! but as a test scripts it, and the plugin served to it over pipes.

use std::io::pipe;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use chrono::{TimeDelta, Utc};
use loclib_plugin::{
    Fetch, Fetched, HelloReply, Incoming, Manifest, PROTOCOL_VERSION, Peer, ProgressReport, Progressed, Published,
    PublishedSet, RpcError, TokenPlace, TracklistEntry, method,
};
use loclib_progress::Event;
use serde_json::{Value, json};

use crate::upload::{AUTHORIZATION, NEWEST, TRACKS};
use crate::{MANIFEST, serve};

/// The access token the host fills in, and the page of the track the fake SoundCloud makes.
pub(super) const TOKEN: &str = "2-290-tok";
pub(super) const PAGE: &str = "https://soundcloud.com/dj/closing-set";

/// How SoundCloud answers a request through the host, in place of what it would: a status, its
/// headers and body; the same, having made the track anyway; the host failing to reach it; or the
/// host's request cancelled while this one is under way.
#[derive(Clone, Debug)]
pub(super) enum Reply {
    Status(u16, Vec<(&'static str, String)>, String),
    MadeAnyway(u16),
    Unreachable,
    Cancelled,
}

/// SoundCloud's error, as its API answers one.
pub(super) fn refusal(code: u16, message: &str) -> String {
    let status = format!("{code} - {message}");
    json!({ "code": code, "message": status, "link": "https://developers.soundcloud.com/docs/api/explorer/open-api",
        "status": status, "errors": [{ "error_message": message }], "error": null })
    .to_string()
}

/// Which request it is (`upload`, or `newest` for the account's newest tracks) and how many of
/// that kind came before: a reply in place of SoundCloud's, or none for what SoundCloud says.
pub(super) type Script = Box<dyn Fn(&str, usize) -> Option<Reply> + Send>;

/// A form as SoundCloud got it.
#[derive(Debug)]
pub(super) struct Form {
    /// The `Authorization` header, as the host sent it.
    pub(super) authorization: String,
    /// Its text parts, in order.
    pub(super) fields: Vec<(String, String)>,
    /// Its files, in order: each part's name, the file's name and its bytes.
    pub(super) files: Vec<(String, String, Vec<u8>)>,
    /// Where the host read each file from.
    pub(super) paths: Vec<PathBuf>,
}

impl Form {
    pub(super) fn field(&self, name: &str) -> Option<&str> {
        self.fields.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// What the fake SoundCloud was sent.
#[derive(Default, Debug)]
pub(super) struct Received {
    /// Each upload's form, in order.
    pub(super) forms: Vec<Form>,
    /// How many times the account's newest tracks were asked for.
    pub(super) listed: usize,
}

/// A track of the account's: its title, when it was made (as SoundCloud says) and its page.
type Made = (String, String, String);

/// SoundCloud as a test scripts it, through a host with the account signed in or not.
pub(super) struct SoundCloud {
    signed_in: bool,
    script: Script,
    received: Received,
    /// The account's tracks, oldest first.
    pub(super) tracks: Vec<Made>,
}

impl SoundCloud {
    pub(super) fn new() -> SoundCloud {
        SoundCloud::scripted(|_, _| None)
    }

    /// Through a host the account isn't signed in on.
    pub(super) fn signed_out() -> SoundCloud {
        SoundCloud { signed_in: false, ..SoundCloud::new() }
    }

    pub(super) fn scripted(script: impl Fn(&str, usize) -> Option<Reply> + Send + 'static) -> SoundCloud {
        SoundCloud { signed_in: true, script: Box::new(script), received: Received::default(), tracks: Vec::new() }
    }

    /// With a track of the account's titled `title`, made `ago` before now.
    pub(super) fn having(mut self, title: &str, ago: TimeDelta) -> SoundCloud {
        let made = (Utc::now() - ago).format("%Y/%m/%d %H:%M:%S +0000").to_string();
        self.tracks.push((title.into(), made, "https://soundcloud.com/dj/older".into()));
        self
    }

    /// The host's answer to `fetch`.
    fn answer(&mut self, fetch: &Fetch) -> Result<Fetched, RpcError> {
        if !self.signed_in {
            // As the host fails a request for an account that isn't signed in.
            let why =
                "its SoundCloud account isn't signed in (locallibrary plugin account soundcloud soundcloud sign-in)";
            return Err(RpcError::failed("invalid", why));
        }
        let accept = fetch.headers.get("Accept").map(String::as_str);
        assert_eq!(accept, Some("application/json; charset=utf-8"), "{fetch:?}");
        let route = match (fetch.method.as_str(), fetch.url.as_str()) {
            ("POST", TRACKS) => "upload",
            ("GET", NEWEST) => "newest",
            other => panic!("SoundCloud has no {other:?}"),
        };
        let before = match route {
            "upload" => {
                let form = self.form(fetch);
                self.received.forms.push(form);
                self.received.forms.len() - 1
            }
            _ => {
                self.received.listed += 1;
                self.received.listed - 1
            }
        };
        let (status, headers, body) = match (self.script)(route, before) {
            Some(Reply::Unreachable) => return Err(RpcError::failed("external", "connection reset")),
            Some(Reply::Cancelled) => return Err(RpcError::cancelled()),
            Some(Reply::Status(status, headers, body)) => (status, headers, body),
            Some(Reply::MadeAnyway(status)) => {
                self.make();
                (status, Vec::new(), String::new())
            }
            None if route == "upload" => (201, Vec::new(), self.make().to_string()),
            None => {
                let newest = self.tracks.iter().rev().take(20);
                let collection: Vec<Value> = newest
                    .map(|(title, made, page)| json!({ "title": title, "created_at": made, "permalink_url": page }))
                    .collect();
                (200, Vec::new(), json!({ "collection": collection, "next_href": null }).to_string())
            }
        };
        let headers = headers.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        Ok(Fetched { status, headers, body: Some(body), blob: None })
    }

    /// The form `fetch` sends, its files read whole, with the token where the manifest says the
    /// host puts it.
    fn form(&self, fetch: &Fetch) -> Form {
        assert!(fetch.body.is_none() && fetch.body_blob.is_none(), "a form alone: {fetch:?}");
        assert_eq!(fetch.headers.get("Authorization").map(String::as_str), Some(AUTHORIZATION), "{fetch:?}");
        let manifest = Manifest::parse(MANIFEST).unwrap();
        let TokenPlace::Scheme(scheme) = &manifest.contributes.accounts[0].token else { panic!("in the header") };
        let mut form = Form {
            authorization: format!("{scheme} {TOKEN}"),
            fields: Vec::new(),
            files: Vec::new(),
            paths: Vec::new(),
        };
        for part in &fetch.multipart {
            match (&part.value, &part.file) {
                (Some(value), None) => form.fields.push((part.name.clone(), value.clone())),
                (None, Some(file)) => {
                    assert_eq!(part.range, None, "a file goes whole");
                    let bytes = std::fs::read(file).expect("the file is there to send");
                    form.files.push((part.name.clone(), part.filename.clone().unwrap_or_default(), bytes));
                    form.paths.push(file.clone());
                }
                _ => panic!("a part is text or a file: {part:?}"),
            }
        }
        form
    }

    /// Makes the track the last form is for: SoundCloud's answer to it.
    fn make(&mut self) -> Value {
        let title = self.received.forms.last().and_then(|f| f.field("track[title]")).unwrap().to_string();
        let made = Utc::now().format("%Y/%m/%d %H:%M:%S +0000").to_string();
        self.tracks.push((title.clone(), made.clone(), PAGE.into()));
        json!({ "kind": "track", "urn": "soundcloud:tracks:4242", "title": title, "created_at": made,
            "permalink_url": PAGE, "sharing": "private" })
    }
}

/// The plugin served on a thread over pipes, as [`serve`] serves it, past `hello`, with a host
/// whose fetches `soundcloud` answers.
pub(super) struct Session {
    pub(super) host: Peer,
    served: JoinHandle<()>,
    answering: JoinHandle<()>,
    pub(super) hello: HelloReply,
    soundcloud: Arc<Mutex<SoundCloud>>,
    /// The requests' scratch folder.
    pub(super) scratch: tempfile::TempDir,
}

pub(super) fn start(soundcloud: SoundCloud) -> Session {
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serve(plugin_in, plugin_out));
    let (host, incoming) = Peer::new(from_plugin, to_plugin);
    let soundcloud = Arc::new(Mutex::new(soundcloud));
    let (to, answers) = (host.clone(), soundcloud.clone());
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
    Session { host, served, answering, hello, soundcloud, scratch: tempfile::tempdir().unwrap() }
}

/// What a publish answered, and the progress it reported.
pub(super) type Outcome = (Result<Published, RpcError>, Vec<Event>);

impl Session {
    /// `set/publish` to `track` as the host sends it, with a scratch folder and `settings`: what
    /// it answered, and the progress it reported.
    pub(super) fn publish(
        &self,
        set: &PublishedSet,
        recording: &Path,
        tracklist: &[TracklistEntry],
        settings: Value,
        dry_run: bool,
    ) -> Outcome {
        let context = json!({ "scratch": self.scratch.path(), "settings": settings });
        let req = json!({ "destination": "track", "set": set, "recording": recording, "tracklist": tracklist,
            "dry_run": dry_run, "context": context });
        let pending = self.host.send_request_reporting(method::SET_PUBLISH, &req).unwrap();
        let answer = pending.wait_timeout(Duration::from_secs(60)).expect("an answer in a minute");
        let events = pending.progress().filter_map(|p| match p {
            ProgressReport::Known(e) => match e.event {
                Progressed::Event(event) => Some(event),
                Progressed::Step(_) => None,
            },
            ProgressReport::Unknown(_) => None,
        });
        (answer, events.collect())
    }

    /// Its fetches answered by `soundcloud` from now on.
    pub(super) fn script(&self, soundcloud: SoundCloud) {
        *self.soundcloud.lock().unwrap() = soundcloud;
    }

    /// What SoundCloud has been sent.
    pub(super) fn received(&self) -> Received {
        std::mem::take(&mut self.soundcloud.lock().unwrap().received)
    }

    /// Closes the plugin's input: it stops. What SoundCloud was sent (all of it: the plugin sends
    /// nothing once it has answered).
    pub(super) fn stop(self) -> Received {
        let received = self.received();
        self.host.close();
        self.served.join().unwrap();
        self.answering.join().unwrap();
        received
    }
}
