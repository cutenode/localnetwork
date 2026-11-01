//! Writing a plugin in Rust: a handler for each kind of hook it implements, in [`Hooks`], handed
//! to [`serve`].

use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Duration;

use loclib_progress::{CancelToken, Coalesced, Ctx, Flusher, Report, Scope};
use rustc_hash::FxHashMap;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::{ok, params};
use crate::protocol::{
    Fetch, Fetched, Hello, HelloReply, Partial, PluginInfo, ProgressEvent, ProgressParams, ProgressReport,
    check_protocol, method,
};
use crate::{
    Contributions, EventNotify, FEATURES, HostNotify, Incoming, ListItem, Manifest, PROTOCOL_VERSION, Peer, Request,
    RpcError,
};

/// A request the host sends a hook: its method, what it answers, and the contribution it's for,
/// which the manifest must declare.
pub trait Hook: DeserializeOwned + 'static {
    /// Its method (`command/run`).
    const METHOD: &'static str;
    type Answer: Serialize;
    /// Whether `contributes` declares the contribution it's for: `invalid_params` if not.
    fn declared(&self, contributes: &Contributions) -> Result<(), RpcError>;
}

/// `name` is one of the manifest's contributions of this kind (`what`).
pub(crate) fn declared<'a>(
    what: &str,
    name: &str,
    mut names: impl Iterator<Item = &'a String>,
) -> Result<(), RpcError> {
    match names.any(|n| n == name) {
        true => Ok(()),
        false => Err(RpcError::invalid_params(format!("the manifest declares no {what} {name:?}"))),
    }
}

/// Runs a hook: its params, checked as its [`Hook`] says, to its answer.
type Handler = Box<dyn Fn(Value, &Contributions, &Host) -> Result<Value, RpcError> + Send + Sync>;

/// Handles a notification from the host: its params.
type Notified = Box<dyn Fn(Value, &Host) -> Result<(), RpcError> + Send + Sync>;

/// The hooks a plugin implements, by method, and the host's notifications it handles. A request
/// it has none for answers `method_not_found` (a notification is dropped), and `hello` lists the
/// features of those it has.
#[derive(Default)]
pub struct Hooks {
    hooks: FxHashMap<&'static str, Handler>,
    notified: FxHashMap<&'static str, Notified>,
}

impl Hooks {
    pub fn new() -> Hooks {
        Hooks::default()
    }

    /// Answers `H`'s requests with `hook`, each on a thread of its own, and only for a
    /// contribution the manifest declares. A hook answers its answer, or what makes one (an
    /// operation's [`crate::OpPlan`]).
    pub fn on<H: Hook, A: Into<H::Answer>>(
        mut self,
        hook: impl Fn(H, &Host) -> Result<A, RpcError> + Send + Sync + 'static,
    ) -> Hooks {
        let run = move |args: Value, contributes: &Contributions, host: &Host| {
            let req: H = params(args)?;
            req.declared(contributes)?;
            ok(hook(req, host)?.into())
        };
        self.hooks.insert(H::METHOD, Box::new(run));
        self
    }

    /// Handles the host's `event/notify` with `handle`, one event at a time in the order they
    /// came, on a thread of their own. Its [`Host`] answers no request: it calls back and sends
    /// `host/notify` as a hook's does, and nothing cancels it. What it fails with goes to the
    /// plugin's log.
    pub fn on_event(
        mut self,
        handle: impl Fn(EventNotify, &Host) -> Result<(), RpcError> + Send + Sync + 'static,
    ) -> Hooks {
        let run = move |args: Value, host: &Host| handle(params(args)?, host);
        self.notified.insert(method::EVENT_NOTIFY, Box::new(run));
        self
    }

    /// The features with a method among these hooks', and the SDK's own (`blobs`, `net`, what a
    /// hook may send the host, see [`Host`]; `choose`, a question it may answer).
    fn features(&self) -> Vec<String> {
        let handled = |m: &&str| self.hooks.contains_key(m) || self.notified.contains_key(m);
        let had = FEATURES.iter().filter(|f| f.methods.is_empty() || f.methods.iter().any(handled));
        had.map(|f| f.name.into()).collect()
    }
}

/// How often a hook's coalesced progress is sent, at most, as the RPC sidecar does.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// The host, as a hook sees it: calling back for what it may read, reporting progress and
/// noticing it was cancelled.
pub struct Host {
    peer: Peer,
    served: Arc<Served>,
    /// The host's request the hook is answering.
    id: Value,
    cancel: CancelToken,
    progress: Arc<Coalesced>,
}

impl Host {
    fn new(peer: Peer, served: Arc<Served>, id: Value, cancel: CancelToken, flusher: &Flusher) -> Host {
        let (to, about) = (peer.clone(), id.clone());
        let send = move |scope: &Scope, report: Report| {
            let event = ProgressEvent::new(scope, report);
            to.notify(method::PROGRESS, &ProgressParams { id: about.clone(), event: ProgressReport::Known(event) });
        };
        let progress = Arc::new(Coalesced::new(PROGRESS_EVERY, send));
        flusher.watch(&progress);
        Host { peer, served, id, cancel, progress }
    }

    /// The plugin's manifest, as [`serve`] read it.
    pub fn manifest(&self) -> &Manifest {
        &self.served.manifest
    }

    /// Calls a core read method (`tracks.query`, `drive.info`...) with the params and result the
    /// locallibrary RPC sidecar has for it. The host refuses what the manifest's capabilities
    /// don't grant. Once the host cancels the hook, this fails with `cancelled` (a call under way
    /// is cancelled too).
    pub fn call<P: Serialize, R: DeserializeOwned>(&self, method: &str, params: &P) -> Result<R, RpcError> {
        // An event's answers no request, which could be cancelled.
        match self.id.is_null() {
            true => self.peer.send_request(method, params)?.wait(),
            false => self.peer.send_request_within(&self.id, method, params)?.wait(),
        }
    }

    /// For the hook's work: its events go to the host as `progress` (byte counts and per-file
    /// events coalesced to about ten a second, each scope apart, as the sidecar does; what's
    /// held back goes out once it has waited, or before the hook's answer), and `ctx.check()?`
    /// fails with `cancelled` once the host cancels the hook.
    pub fn ctx(&self) -> Ctx<'_> {
        Ctx::new(&*self.progress, self.cancel.clone())
    }

    /// `net/fetch`: the host makes an HTTP request for the hook, to a server the manifest's
    /// `net:` capabilities grant (see [`Fetch`]).
    pub fn fetch(&self, req: &Fetch) -> Result<Fetched, RpcError> {
        self.call(method::NET_FETCH, req)
    }

    /// Sends the host the next rows of the table a view command will answer (`partial`), to
    /// show while it works out the rest. The view it answers still has them, first.
    pub fn partial_rows(&self, rows: Vec<Vec<Value>>) {
        self.peer.notify(method::PARTIAL, &Partial { id: self.id.clone(), rows, items: Vec::new() });
    }

    /// [`Host::partial_rows`] for a list's items.
    pub fn partial_items(&self, items: Vec<ListItem>) {
        self.peer.notify(method::PARTIAL, &Partial { id: self.id.clone(), rows: Vec::new(), items });
    }

    /// Tells the user something (`host/notify`), from an event or a service: a line, and maybe
    /// an operation of the plugin's or one of the host's actions to review
    /// ([`HostNotify::action`]), which the host plans only if they choose to.
    pub fn notify(&self, notice: &HostNotify) {
        self.peer.notify(method::HOST_NOTIFY, notice);
    }
}

/// What [`serve`] serves: the manifest and the hooks.
struct Served {
    manifest: Manifest,
    hooks: Hooks,
}

/// Serves `hooks` over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes, then cancels the hooks still running and returns once they have answered and the
/// events sent are handled. Until the host's `hello` every request is refused (and every
/// notification dropped); `describe` answers `manifest` (the plugin's
/// `locallibrary-plugin.toml`), which must parse.
///
/// Cancelling is cooperative: a hook that never checks its `ctx()` runs to its end. A host should
/// give a cancelled hook a while to answer and then kill the plugin.
pub fn serve(manifest: &str, hooks: Hooks, input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    let manifest = Manifest::parse(manifest).expect("the plugin's manifest parses");
    let (peer, incoming) = Peer::new(input, output);
    let features = hooks.features();
    let served = Arc::new(Served { manifest, hooks });
    let flusher = Arc::new(Flusher::new());
    let flushing = flusher.clone();
    let flushing = std::thread::spawn(move || flushing.run(PROGRESS_EVERY));
    let mut greeted = false;
    let mut running = Vec::new();
    // The host's notifications, handled in order on a thread of their own.
    let (notify, notified) = std::sync::mpsc::channel::<(String, Value)>();
    let handling = (!served.hooks.notified.is_empty()).then(|| {
        let (peer, served, flusher) = (peer.clone(), served.clone(), flusher.clone());
        std::thread::spawn(move || {
            for (method, params) in notified {
                let host = Host::new(peer.clone(), served.clone(), Value::Null, CancelToken::new(), &flusher);
                let run = || served.hooks.notified[method.as_str()](params, &host);
                let failed = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
                    Ok(Ok(())) => None,
                    Ok(Err(e)) => Some(e.message),
                    Err(_) => Some("internal error".into()),
                };
                host.progress.finish();
                if let Some(why) = failed {
                    eprintln!("loclib-plugin: handling {method}: {why}");
                }
            }
        })
    });
    for msg in incoming {
        let req = match msg {
            Incoming::Request(req) => req,
            // Before `hello`, or one it has no handler for, it's dropped (`$/cancel` the peer
            // handles).
            Incoming::Notification { method, params } => {
                if greeted && served.hooks.notified.contains_key(method.as_str()) {
                    let _ = notify.send((method, params));
                }
                continue;
            }
        };
        let result = match req.method.as_str() {
            method::HELLO if greeted => Err(RpcError::invalid_request("hello was sent already")),
            method::HELLO => params(req.params).and_then(|hello: Hello| {
                check_protocol(&hello.protocol)?;
                greeted = true;
                let m = &served.manifest;
                ok(HelloReply {
                    protocol: PROTOCOL_VERSION.into(),
                    plugin: PluginInfo { id: m.id.clone(), version: m.version.clone() },
                    // The host's are ignored: what it lacks, it never calls.
                    features: features.clone(),
                })
            }),
            _ if !greeted => Err(RpcError::invalid_request("hello first")),
            method::DESCRIBE => ok(&served.manifest),
            hook if served.hooks.hooks.contains_key(hook) => {
                let (peer, served, flusher) = (peer.clone(), served.clone(), flusher.clone());
                running.retain(|hook: &std::thread::JoinHandle<()>| !hook.is_finished());
                running.push(std::thread::spawn(move || {
                    let Request { id, method, params, cancel } = req;
                    let host = Host::new(peer.clone(), served.clone(), id.clone(), cancel, &flusher);
                    let run = || {
                        // Cancelled before it started.
                        host.ctx().check()?;
                        served.hooks.hooks[method.as_str()](params, &served.manifest.contributes, &host)
                    };
                    // A bug in the plugin must not leave the host waiting forever for a response.
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).unwrap_or_else(|_| {
                        Err(RpcError::internal(format!("internal error while handling {method:?}")))
                    });
                    host.progress.finish();
                    peer.respond(&id, result);
                }));
                continue;
            }
            other => Err(RpcError::method_not_found(other)),
        };
        peer.respond(&req.id, result);
    }
    drop(notify);
    for hook in running.into_iter().chain(handling) {
        let _ = hook.join();
    }
    flusher.stop();
    let _ = flushing.join();
}

#[cfg(test)]
mod tests;
