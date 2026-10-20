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
    Analysis, AnalyzeTrack, Exported, Hello, HelloReply, OpPlan, PlanOp, PluginInfo, ProgressEvent, ProgressParams,
    ProgressReport, Progressed, RunCommand, StepInfo, StepStarted, WriteExport, check_protocol, method,
};
use crate::{Contributions, FEATURES, Incoming, Manifest, PROTOCOL_VERSION, Peer, Request, RpcError};

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
fn declared<'a>(what: &str, name: &str, mut names: impl Iterator<Item = &'a String>) -> Result<(), RpcError> {
    match names.any(|n| n == name) {
        true => Ok(()),
        false => Err(RpcError::invalid_params(format!("the manifest declares no {what} {name:?}"))),
    }
}

/// Any JSON for a headless command, a [`crate::View`] for a view command.
impl Hook for RunCommand {
    const METHOD: &'static str = method::RUN_COMMAND;
    type Answer = Value;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        declared("command", &self.command, c.commands.iter().map(|x| &x.name))
    }
}

impl Hook for AnalyzeTrack {
    const METHOD: &'static str = method::ANALYZE_TRACK;
    type Answer = Analysis;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        declared("analyzer", &self.analyzer, c.analyzers.iter().map(|x| &x.name))
    }
}

impl Hook for WriteExport {
    const METHOD: &'static str = method::WRITE_EXPORT;
    type Answer = Exported;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        declared("exporter", &self.exporter, c.exporters.iter().map(|x| &x.format))
    }
}

impl Hook for PlanOp {
    const METHOD: &'static str = method::PLAN_OP;
    type Answer = OpPlan;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        declared("operation", &self.operation, c.operations.iter().map(|x| &x.name))
    }
}

/// Runs a hook: its params, checked as its [`Hook`] says, to its answer.
type Handler = Box<dyn Fn(Value, &Contributions, &Host) -> Result<Value, RpcError> + Send + Sync>;

/// The hooks a plugin implements, by method. One it has none for answers `method_not_found`, and
/// `hello` lists the features of those it has.
#[derive(Default)]
pub struct Hooks(FxHashMap<&'static str, Handler>);

impl Hooks {
    pub fn new() -> Hooks {
        Hooks::default()
    }

    /// Answers `H`'s requests with `hook`, each on a thread of its own, and only for a
    /// contribution the manifest declares.
    pub fn on<H: Hook>(
        mut self,
        hook: impl Fn(H, &Host) -> Result<H::Answer, RpcError> + Send + Sync + 'static,
    ) -> Hooks {
        let run = move |args: Value, contributes: &Contributions, host: &Host| {
            let req: H = params(args)?;
            req.declared(contributes)?;
            ok(hook(req, host)?)
        };
        self.0.insert(H::METHOD, Box::new(run));
        self
    }

    /// The features with a method among these hooks'.
    fn features(&self) -> Vec<String> {
        let had = FEATURES.iter().filter(|f| f.methods.iter().any(|m| self.0.contains_key(m)));
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
            let (event, items) = match report {
                Report::Event { event, items } => (Progressed::Event(event), items),
                Report::Step => (Progressed::Step(StepStarted::default()), None),
            };
            let step = scope.step.map(|s| StepInfo { index: s.index, total: s.total, id: s.id.into() });
            let event = ProgressEvent { event, target: scope.target.clone(), step, items };
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
        self.peer.send_request_within(&self.id, method, params)?.wait()
    }

    /// For the hook's work: its events go to the host as `progress` (byte counts and per-file
    /// events coalesced to about ten a second, each scope apart, as the sidecar does; what's
    /// held back goes out once it has waited, or before the hook's answer), and `ctx.check()?`
    /// fails with `cancelled` once the host cancels the hook.
    pub fn ctx(&self) -> Ctx<'_> {
        Ctx::new(&*self.progress, self.cancel.clone())
    }
}

/// What [`serve`] serves: the manifest and the hooks.
struct Served {
    manifest: Manifest,
    hooks: Hooks,
}

/// Serves `hooks` over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes, then cancels the hooks still running and returns once they have answered. Until the
/// host's `hello` every request is refused; `describe` answers `manifest` (the plugin's
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
    for msg in incoming {
        // The host sends no notifications but `$/cancel`, which the peer handles.
        let Incoming::Request(req) = msg else { continue };
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
            hook if served.hooks.0.contains_key(hook) => {
                let (peer, served, flusher) = (peer.clone(), served.clone(), flusher.clone());
                running.retain(|hook: &std::thread::JoinHandle<()>| !hook.is_finished());
                running.push(std::thread::spawn(move || {
                    let Request { id, method, params, cancel } = req;
                    let host = Host::new(peer.clone(), served.clone(), id.clone(), cancel, &flusher);
                    let run = || {
                        // Cancelled before it started.
                        host.ctx().check()?;
                        served.hooks.0[method.as_str()](params, &served.manifest.contributes, &host)
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
    for hook in running {
        let _ = hook.join();
    }
    flusher.stop();
    let _ = flushing.join();
}

#[cfg(test)]
mod tests;
