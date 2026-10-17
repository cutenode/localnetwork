//! Writing a plugin in Rust: implement [`Plugin`] and hand it to [`serve`].

use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Duration;

use loclib_progress::{CancelToken, Coalesced, Ctx, Flusher, Report, Scope};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::{ok, params};
use crate::protocol::{
    Analysis, AnalyzeTrack, Exported, Hello, HelloReply, OpPlan, PlanOp, PluginInfo, ProgressEvent, ProgressParams,
    ProgressReport, Progressed, RunCommand, StepInfo, StepStarted, WriteExport, check_protocol, method,
};
use crate::{Incoming, Manifest, PROTOCOL_VERSION, Peer, Request, RpcError};

/// A plugin's hooks, one for each kind of contribution. Each runs on a thread of its own, and
/// only for a contribution the manifest declares; one the plugin doesn't implement answers
/// `method_not_found`.
pub trait Plugin: Send + Sync + 'static {
    /// `command/run`: any JSON for a headless command, a [`crate::View`] for a view command.
    fn run_command(&self, req: RunCommand, host: &Host) -> Result<Value, RpcError> {
        let _ = (req, host);
        Err(RpcError::method_not_found(method::RUN_COMMAND))
    }

    /// `track/analyze`.
    fn analyze_track(&self, req: AnalyzeTrack, host: &Host) -> Result<Analysis, RpcError> {
        let _ = (req, host);
        Err(RpcError::method_not_found(method::ANALYZE_TRACK))
    }

    /// `export/write`.
    fn write_export(&self, req: WriteExport, host: &Host) -> Result<Exported, RpcError> {
        let _ = (req, host);
        Err(RpcError::method_not_found(method::WRITE_EXPORT))
    }

    /// `op/plan`.
    fn plan_op(&self, req: PlanOp, host: &Host) -> Result<OpPlan, RpcError> {
        let _ = (req, host);
        Err(RpcError::method_not_found(method::PLAN_OP))
    }
}

/// How often a hook's coalesced progress is sent, at most, as the RPC sidecar does.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// The host, as a hook sees it: calling back for what it may read, reporting progress and
/// noticing it was cancelled.
pub struct Host {
    peer: Peer,
    /// The host's request the hook is answering.
    id: Value,
    cancel: CancelToken,
    progress: Arc<Coalesced>,
}

impl Host {
    fn new(peer: Peer, id: Value, cancel: CancelToken, flusher: &Flusher) -> Host {
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
        Host { peer, id, cancel, progress }
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

/// Serves `plugin` over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes, then cancels the hooks still running and returns once they have answered. Until the
/// host's `hello` every request is refused; `describe` answers `manifest`.
///
/// Cancelling is cooperative: a hook that never checks its `ctx()` runs to its end. A host should
/// give a cancelled hook a while to answer and then kill the plugin.
pub fn serve(
    manifest: &Manifest,
    plugin: impl Plugin,
    input: impl Read + Send + 'static,
    output: impl Write + Send + 'static,
) {
    let (peer, incoming) = Peer::new(input, output);
    let shared = Arc::new((manifest.clone(), plugin));
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
                ok(HelloReply {
                    protocol: PROTOCOL_VERSION.into(),
                    plugin: PluginInfo { id: manifest.id.clone(), version: manifest.version.clone() },
                    // None yet. The host's are ignored, as none of them changes what a plugin does.
                    features: Vec::new(),
                })
            }),
            _ if !greeted => Err(RpcError::invalid_request("hello first")),
            method::DESCRIBE => ok(manifest),
            method::RUN_COMMAND | method::ANALYZE_TRACK | method::WRITE_EXPORT | method::PLAN_OP => {
                let (peer, shared, flusher) = (peer.clone(), shared.clone(), flusher.clone());
                running.retain(|hook: &std::thread::JoinHandle<()>| !hook.is_finished());
                running.push(std::thread::spawn(move || {
                    let (manifest, plugin) = &*shared;
                    let Request { id, method, params, cancel } = req;
                    let host = Host::new(peer.clone(), id.clone(), cancel, &flusher);
                    // A bug in the plugin must not leave the host waiting forever for a response.
                    let run = || hook(manifest, plugin, &method, params, &host);
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

/// `name` is one of the manifest's contributions of this kind (`what`).
fn declared<'a>(what: &str, name: &str, mut names: impl Iterator<Item = &'a String>) -> Result<(), RpcError> {
    match names.any(|n| n == name) {
        true => Ok(()),
        false => Err(RpcError::invalid_params(format!("the manifest declares no {what} {name:?}"))),
    }
}

/// Runs the hook `method` asks for.
fn hook(manifest: &Manifest, plugin: &impl Plugin, method: &str, args: Value, host: &Host) -> Result<Value, RpcError> {
    // Cancelled before it started.
    host.ctx().check()?;
    let c = &manifest.contributes;
    match method {
        method::RUN_COMMAND => {
            let req: RunCommand = params(args)?;
            declared("command", &req.command, c.commands.iter().map(|x| &x.name))?;
            plugin.run_command(req, host)
        }
        method::ANALYZE_TRACK => {
            let req: AnalyzeTrack = params(args)?;
            declared("analyzer", &req.analyzer, c.analyzers.iter().map(|x| &x.name))?;
            ok(plugin.analyze_track(req, host)?)
        }
        method::WRITE_EXPORT => {
            let req: WriteExport = params(args)?;
            declared("exporter", &req.exporter, c.exporters.iter().map(|x| &x.format))?;
            ok(plugin.write_export(req, host)?)
        }
        _ => {
            let req: PlanOp = params(args)?;
            declared("operation", &req.operation, c.operations.iter().map(|x| &x.name))?;
            ok(plugin.plan_op(req, host)?)
        }
    }
}

#[cfg(test)]
mod tests;
