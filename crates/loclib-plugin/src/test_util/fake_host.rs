//! A fake host: a plugin's `serve` run on a thread over in-memory pipes, greeted and described as
//! locallibrary does, its calls back answered from canned answers and recorded, and each hook
//! request given a scratch folder of its own.

use std::io::{PipeReader, PipeWriter};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use rustc_hash::FxHashMap;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tempfile::TempDir;

use crate::error::ok;
use crate::{
    Cancel, EventNotify, FEATURES, Fetch, Fetched, Hello, HelloReply, HostInfo, HostNotify, Incoming, Manifest,
    PROTOCOL_VERSION, Peer, Pending, ProgressReport, RpcError, method, params,
};

/// Answers a call the plugin makes, given its params.
type Answer = Box<dyn Fn(Value) -> Response + Send>;

/// What the host does with a call of the plugin's, given to [`FakeHost::on_call`].
#[derive(Debug)]
pub enum Response {
    /// Answers it.
    Answer(Result<Value, RpcError>),
    /// Answers it, then cancels the hook request last sent: the user cancelling just as the call
    /// came back, before the plugin's next.
    AnswerThenCancel(Result<Value, RpcError>),
    /// Never answers it: a server that keeps the fetch under way until the plugin stops waiting,
    /// its hook cancelled.
    Hold,
}

/// A plugin served on a thread, past `hello` and `describe`, with a host that answers its calls
/// from what it's given and keeps what the plugin sent it. Stopped when dropped, or by
/// [`FakeHost::stop`], which also waits for it.
pub struct FakeHost {
    peer: Peer,
    hello: HelloReply,
    shared: Arc<Shared>,
    /// Serving the plugin, and answering its calls.
    threads: Mutex<Vec<JoinHandle<()>>>,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    /// The id of the hook request last sent. Held while it's sent, so a call the plugin makes for
    /// it can't cancel it before it's there.
    sent: Mutex<Option<u64>>,
}

#[derive(Default)]
struct State {
    /// By method: reads, `accounts/ensure`, and `net/fetch` for a URL with no answer of its own.
    answers: FxHashMap<String, Answer>,
    /// `net/fetch`'s, by URL.
    fetches: FxHashMap<String, Result<Fetched, RpcError>>,
    calls: Vec<(String, Value)>,
    notices: Vec<HostNotify>,
    /// The method whose next call cancels the hook request last sent.
    cancel_on: Option<String>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl FakeHost {
    /// Serves a plugin with `serve` (the `serve(input, output)` a plugin crate exports), greets it
    /// with every feature this crate knows, and checks that `describe` answers `manifest` (its
    /// `locallibrary-plugin.toml`). Panics when it doesn't, as locallibrary refuses it.
    pub fn start(manifest: &str, serve: impl FnOnce(PipeReader, PipeWriter) + Send + 'static) -> FakeHost {
        let manifest = Manifest::parse(manifest).expect("the plugin's manifest parses");
        let (plugin_in, to_plugin) = std::io::pipe().expect("a pipe to the plugin");
        let (from_plugin, plugin_out) = std::io::pipe().expect("a pipe from the plugin");
        let served = std::thread::spawn(move || serve(plugin_in, plugin_out));
        let (peer, incoming) = Peer::new(from_plugin, to_plugin);
        let shared = Arc::<Shared>::default();
        let (to, answering) = (peer.clone(), shared.clone());
        let answering = std::thread::spawn(move || answer(&to, &answering, incoming));
        let hello = Hello {
            protocol: PROTOCOL_VERSION.into(),
            host: HostInfo { name: "locallibrary".into(), version: env!("CARGO_PKG_VERSION").into() },
            features: FEATURES.iter().map(|f| f.name.into()).collect(),
        };
        let hello: HelloReply = peer.request(method::HELLO, &hello).expect("the plugin answers hello");
        let described: Manifest = peer.request(method::DESCRIBE, &json!({})).expect("the plugin describes itself");
        assert_eq!(described, manifest, "describe answers the plugin's manifest");
        FakeHost { peer, hello, shared, threads: Mutex::new(vec![served, answering]) }
    }

    /// What the plugin answered `hello`.
    pub fn hello(&self) -> &HelloReply {
        &self.hello
    }

    /// Answers each of the plugin's calls of `method` (a read, `tracks.query`) with `answer`.
    pub fn read(&self, method: &str, answer: impl Serialize) -> &Self {
        let answer = ok(answer);
        self.on_read(method, move |_| answer.clone())
    }

    /// Answers each of the plugin's calls of `method` with what `answer` makes of its params: a
    /// read that pages, an error, or `net/fetch` for any URL [`FakeHost::fetch`] has no answer
    /// for.
    pub fn on_read(&self, method: &str, answer: impl Fn(Value) -> Result<Value, RpcError> + Send + 'static) -> &Self {
        self.on_call(method, move |params| Response::Answer(answer(params)))
    }

    /// [`FakeHost::on_read`], where `respond` may also cancel the hook request once it has
    /// answered, or hold the call unanswered.
    pub fn on_call(&self, method: &str, respond: impl Fn(Value) -> Response + Send + 'static) -> &Self {
        lock(&self.shared.state).answers.insert(method.into(), Box::new(respond));
        self
    }

    /// Answers each of the plugin's `net/fetch` requests to `url` with `answer`.
    pub fn fetch(&self, url: &str, answer: Result<Fetched, RpcError>) -> &Self {
        lock(&self.shared.state).fetches.insert(url.into(), answer);
        self
    }

    /// Cancels the hook request last sent once the plugin next calls `method`, before answering
    /// it: the user cancelling while the plugin waits on that call.
    pub fn cancel_on(&self, method: &str) -> &Self {
        lock(&self.shared.state).cancel_on = Some(method.into());
        self
    }

    /// Sends a hook request (`command/run`), as the host does: `context.scratch` a new, empty
    /// folder when the plugin has the `blobs` feature, removed once the [`Sent`] is dropped. One
    /// `params` already give is kept: a folder that's gone, or `null` for a host without `blobs`.
    pub fn send(&self, method: &str, params: &impl Serialize) -> Sent {
        let mut params = serde_json::to_value(params).expect("the params are JSON");
        let given = params["context"].get("scratch").is_some();
        let blobs = self.hello.features.iter().any(|f| f == "blobs");
        let scratch = (blobs && !given).then(|| tempfile::tempdir().expect("a scratch folder"));
        if let Some(dir) = &scratch {
            params["context"]["scratch"] = json!(dir.path());
        }
        let mut sent = lock(&self.shared.sent);
        let pending = self.peer.send_request_reporting(method, &params).expect("the plugin is running");
        *sent = Some(pending.id());
        Sent { pending, scratch }
    }

    /// [`FakeHost::send`], waiting for the answer.
    pub fn request<R: DeserializeOwned>(&self, method: &str, params: &impl Serialize) -> Result<R, RpcError> {
        self.send(method, params).wait()
    }

    /// Tells the plugin of an event (`event/notify`). It handles them in order on a thread of its
    /// own: what it tells the user is in [`FakeHost::notices`] once it has, at the latest once
    /// [`FakeHost::stop`] returns. Stopping fails a call a handler is still waiting on, so a test
    /// whose events make calls waits for them first: one last event whose call it holds
    /// ([`Response::Hold`]) comes once every event before it has been handled.
    pub fn event(&self, event: &EventNotify) {
        self.peer.notify(method::EVENT_NOTIFY, event);
    }

    /// The calls the plugin has made, in order: each one's method and params.
    pub fn calls(&self) -> Vec<(String, Value)> {
        lock(&self.shared.state).calls.clone()
    }

    /// What the plugin has told the user (`host/notify`), in order.
    pub fn notices(&self) -> Vec<HostNotify> {
        lock(&self.shared.state).notices.clone()
    }

    /// Closes the plugin's input, as the host does to stop it, and waits for it to stop. A panic
    /// of the plugin's (or of an answer's) comes out here.
    pub fn stop(&self) {
        self.peer.close();
        for thread in lock(&self.threads).drain(..) {
            if let Err(panic) = thread.join() {
                std::panic::resume_unwind(panic);
            }
        }
    }
}

impl Drop for FakeHost {
    /// Stops the plugin without waiting: a test failing mustn't leave it running.
    fn drop(&mut self) {
        self.peer.close();
    }
}

/// Answers the plugin's calls and keeps what it tells the user, until its output closes.
fn answer(peer: &Peer, shared: &Shared, incoming: Receiver<Incoming>) {
    for msg in incoming {
        let req = match msg {
            Incoming::Request(req) => req,
            Incoming::Notification { method, params } if method == method::HOST_NOTIFY => {
                let notice = serde_json::from_value(params).expect("host/notify's params are a HostNotify");
                lock(&shared.state).notices.push(notice);
                continue;
            }
            Incoming::Notification { .. } => continue,
        };
        let (response, cancels) = {
            let mut state = lock(&shared.state);
            state.calls.push((req.method.clone(), req.params.clone()));
            let cancels = state.cancel_on.take_if(|m| *m == req.method).is_some();
            (state.answer(&req.method, req.params), cancels)
        };
        let cancel = || {
            let sent = lock(&shared.sent).expect("a hook request sent to cancel");
            peer.notify(method::CANCEL, &Cancel { id: sent.into() });
        };
        if cancels {
            cancel();
        }
        match response {
            Response::Answer(answer) => peer.respond(&req.id, answer),
            Response::AnswerThenCancel(answer) => {
                peer.respond(&req.id, answer);
                cancel();
            }
            Response::Hold => {}
        }
    }
}

impl State {
    /// A call's answer: a fetch's for its URL, else the method's. One with none fails, as the host
    /// fails a call it can't make: `external` for a fetch, `method_not_found` else.
    fn answer(&self, method: &str, args: Value) -> Response {
        let url = match method {
            method::NET_FETCH => match params::<Fetch>(args.clone()) {
                Ok(fetch) => Some(fetch.url),
                Err(e) => return Response::Answer(Err(e)),
            },
            _ => None,
        };
        if let Some(fetched) = url.as_ref().and_then(|url| self.fetches.get(url)) {
            return Response::Answer(fetched.clone().and_then(ok));
        }
        let Some(answer) = self.answers.get(method) else {
            return Response::Answer(Err(match url {
                Some(url) => RpcError::failed("external", format!("the fake host has no answer for {url}")),
                None => RpcError::method_not_found(method),
            }));
        };
        // An answer that panics (a test's assertion) fails the call rather than leaving the
        // plugin waiting for it.
        catch_unwind(AssertUnwindSafe(|| answer(args))).unwrap_or_else(|_| {
            Response::Answer(Err(RpcError::internal(format!("the fake host's answer to {method} panicked"))))
        })
    }
}

/// A hook request sent, until its answer comes, with its scratch folder, which is removed when
/// this is dropped.
#[must_use]
pub struct Sent {
    pending: Pending,
    scratch: Option<TempDir>,
}

impl Sent {
    /// The request's `context.scratch`: the files its answer names are here.
    pub fn scratch(&self) -> Option<&Path> {
        self.scratch.as_ref().map(TempDir::path)
    }

    /// Asks the plugin to stop it (`$/cancel`); its answer still comes.
    pub fn cancel(&self) {
        self.pending.cancel();
    }

    /// Blocks until the answer comes. Once it has, waiting again fails with `closed`.
    pub fn wait<R: DeserializeOwned>(&self) -> Result<R, RpcError> {
        self.pending.wait_timeout(Duration::MAX).expect("waiting without a limit ends with the answer")
    }

    /// [`Sent::wait`], for at most `limit`: none when the answer hasn't come by then.
    pub fn wait_timeout<R: DeserializeOwned>(&self, limit: Duration) -> Option<Result<R, RpcError>> {
        self.pending.wait_timeout(limit)
    }

    /// The progress the plugin has sent about it since last asked, in order: all of it once the
    /// answer has come.
    pub fn progress(&self) -> Vec<ProgressReport> {
        self.pending.progress().collect()
    }
}

#[cfg(test)]
mod tests;
