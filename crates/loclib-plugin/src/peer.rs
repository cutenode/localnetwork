//! One end of a JSON-RPC 2.0 connection over a pair of byte streams (a child's stdin and stdout),
//! one message per line. Either end sends requests and notifications whenever it likes, from any
//! number of threads; one thread reads, handing responses to whoever waits for them and the
//! other end's requests and notifications to the owner.

use std::cell::Cell;
use std::io::{BufReader, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use loclib_progress::CancelToken;
use rustc_hash::FxHashMap;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::error::{RpcError, ok, result};
use crate::framing::{Line, MAX_LINE, read_line_within};
use crate::protocol::{Cancel, Partial, ProgressParams, ProgressReport, method};

/// What the other end sent besides responses.
#[derive(Debug)]
pub enum Incoming {
    /// Answer it with [`Peer::respond`].
    Request(Request),
    Notification {
        method: String,
        params: Value,
    },
}

#[derive(Debug)]
pub struct Request {
    /// A string or a number.
    pub id: Value,
    pub method: String,
    /// `{}` when it had none.
    pub params: Value,
    /// Cancelled by the other end's `$/cancel`, or when the connection closes.
    pub cancel: CancelToken,
}

/// A request sent, waiting for its response.
struct Waiter {
    response: Sender<Result<Value, RpcError>>,
    /// The other end's request it was sent while answering: cancelling that cancels this.
    within: Option<Value>,
    /// Where its `progress` and `partial` go, when it was sent with
    /// [`Peer::send_request_reporting`].
    progress: Option<(Sender<ProgressReport>, Sender<Partial>)>,
}

type Waiting = FxHashMap<u64, Waiter>;

struct Shared {
    /// `None` once [`Peer::close`]d.
    out: Mutex<Option<Box<dyn Write + Send>>>,
    next: AtomicU64,
    /// Requests sent, by id, waiting for their responses. `None` once the connection closed.
    waiting: Mutex<Option<Waiting>>,
    /// Requests received and not answered yet.
    running: Mutex<Vec<(Value, CancelToken)>>,
    /// Where what the other end got wrong is said.
    log: Box<dyn Fn(&str) + Send + Sync>,
    /// The longest line read or written: [`MAX_LINE`], or a short one in tests, which then
    /// needn't push megabytes through a pipe.
    max_line: usize,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Cheap to clone: clones share the connection.
#[derive(Clone)]
pub struct Peer(Arc<Shared>);

/// A request sent, until its response comes.
pub struct Pending {
    peer: Peer,
    id: u64,
    response: Receiver<Result<Value, RpcError>>,
    progress: Option<(Receiver<ProgressReport>, Receiver<Partial>)>,
    /// Where its `progress` goes, for this side's own (see [`Pending::reporter`]).
    reporter: Option<Sender<ProgressReport>>,
    /// Its response came: the reader drops its sender only after handing it over, so the
    /// channel alone can't say so yet.
    answered: Cell<bool>,
}

impl Pending {
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Asks the other end to stop it (`$/cancel`); its response still comes.
    pub fn cancel(&self) {
        self.peer.notify(method::CANCEL, &Cancel { id: self.id.into() });
    }

    /// Blocks until the response comes, or the connection closes.
    pub fn wait<R: DeserializeOwned>(self) -> Result<R, RpcError> {
        self.response.recv().unwrap_or_else(|_| Err(RpcError::closed())).and_then(result)
    }

    /// [`Pending::wait`] for at most `timeout`: `None` when the response hasn't come by then.
    /// Once it has, waiting again fails with `closed`.
    pub fn wait_timeout<R: DeserializeOwned>(&self, timeout: Duration) -> Option<Result<R, RpcError>> {
        if self.answered.get() {
            return Some(Err(RpcError::closed()));
        }
        match self.response.recv_timeout(timeout) {
            Ok(response) => {
                self.answered.set(true);
                Some(response.and_then(result))
            }
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => Some(Err(RpcError::closed())),
        }
    }

    /// The `progress` the other end has sent about it since last asked, in order, when it was
    /// sent with [`Peer::send_request_reporting`]: all it sent before its response, once that
    /// has come.
    pub fn progress(&self) -> impl Iterator<Item = ProgressReport> + '_ {
        self.progress.iter().flat_map(|(progress, _)| progress.try_iter())
    }

    /// Where progress this side makes for the request goes, among the other end's and in order
    /// with it (see [`Pending::progress`]): the host's, as it sends a file a hook's `net/fetch`
    /// names. None unless it was sent with [`Peer::send_request_reporting`].
    pub fn reporter(&self) -> Option<Sender<ProgressReport>> {
        self.reporter.clone()
    }

    /// [`Pending::progress`] for the view's rows (or items) the other end has sent in parts
    /// (`partial`).
    pub fn partials(&self) -> impl Iterator<Item = Partial> + '_ {
        self.progress.iter().flat_map(|(_, partials)| partials.try_iter())
    }
}

impl Peer {
    /// Starts reading `input` on a thread of its own. The other end's requests and notifications
    /// come out of the receiver, which closes when `input` does. What the other end gets wrong
    /// (a line too long, a response nothing waits for) is said on stderr, a plugin's log.
    pub fn new(input: impl Read + Send + 'static, output: impl Write + Send + 'static) -> (Peer, Receiver<Incoming>) {
        Peer::with_log(input, output, |line| eprintln!("loclib-plugin: {line}"))
    }

    /// [`Peer::new`], saying what the other end gets wrong to `log` (a host's own log).
    pub fn with_log(
        input: impl Read + Send + 'static,
        output: impl Write + Send + 'static,
        log: impl Fn(&str) + Send + Sync + 'static,
    ) -> (Peer, Receiver<Incoming>) {
        Peer::with_max_line(input, output, log, MAX_LINE)
    }

    /// [`Peer::with_log`], with lines longer than `max_line` too long.
    pub(crate) fn with_max_line(
        input: impl Read + Send + 'static,
        output: impl Write + Send + 'static,
        log: impl Fn(&str) + Send + Sync + 'static,
        max_line: usize,
    ) -> (Peer, Receiver<Incoming>) {
        let peer = Peer(Arc::new(Shared {
            out: Mutex::new(Some(Box::new(output))),
            next: AtomicU64::new(1),
            waiting: Mutex::new(Some(Waiting::default())),
            running: Mutex::default(),
            log: Box::new(log),
            max_line,
        }));
        let (tx, rx) = mpsc::channel();
        let reader = peer.clone();
        std::thread::spawn(move || reader.read(input, &tx));
        (peer, rx)
    }

    /// Writes one message as a line. One longer than [`MAX_LINE`] isn't sent: the other end
    /// would drop it.
    fn write(&self, msg: &Value) -> Result<(), RpcError> {
        let mut line = msg.to_string();
        if line.len() > self.0.max_line {
            return Err(RpcError::internal(format!("a message is longer than {}", self.max_line())));
        }
        line.push('\n');
        let mut out = lock(&self.0.out);
        let out = out.as_mut().ok_or_else(RpcError::closed)?;
        out.write_all(line.as_bytes()).and_then(|()| out.flush()).map_err(|_| RpcError::closed())
    }

    /// Closes the output, which the other end reads as the connection closing (a plugin cancels
    /// its running hooks, answers them and stops). Responses still come in until the other end
    /// closes its output.
    pub fn close(&self) {
        lock(&self.0.out).take();
    }

    /// Sends a request; [`Pending::wait`] for its response.
    pub fn send_request<P: Serialize>(&self, method: &str, params: &P) -> Result<Pending, RpcError> {
        self.send(method, params, None, false)
    }

    /// [`Peer::send_request`] for a hook: the other end's `progress` and `partial` about it come
    /// to its [`Pending::progress`] and [`Pending::partials`], not the owner.
    pub fn send_request_reporting<P: Serialize>(&self, method: &str, params: &P) -> Result<Pending, RpcError> {
        self.send(method, params, None, true)
    }

    /// Sends a request while answering the other end's request `within` (a callback during a
    /// hook). When the other end cancels `within`, this is cancelled too: `$/cancel` goes out
    /// for it, and its [`Pending::wait`] fails with `cancelled` at once. Once `within` is
    /// cancelled or answered, nothing is sent and this fails with `cancelled`.
    pub(crate) fn send_request_within<P: Serialize>(
        &self,
        within: &Value,
        method: &str,
        params: &P,
    ) -> Result<Pending, RpcError> {
        self.send(method, params, Some(within), false)
    }

    fn send<P: Serialize>(
        &self,
        method: &str,
        params: &P,
        within: Option<&Value>,
        reporting: bool,
    ) -> Result<Pending, RpcError> {
        let params = ok(params)?;
        let id = self.0.next.fetch_add(1, Ordering::Relaxed);
        let (tx, response) = mpsc::channel();
        let (progress_tx, progress) = reporting
            .then(|| {
                let ((progress_tx, progress), (partial_tx, partials)) = (mpsc::channel(), mpsc::channel());
                ((progress_tx, partial_tx), (progress, partials))
            })
            .unzip();
        let reporter = progress_tx.as_ref().map(|(progress, _)| progress.clone());
        let waiter = Waiter { response: tx, within: within.cloned(), progress: progress_tx };
        lock(&self.0.waiting).as_mut().ok_or_else(RpcError::closed)?.insert(id, waiter);
        // Cancelled before it was waited for: `on_cancel` may have missed it.
        let cancelled = within.is_some_and(|within| {
            lock(&self.0.running).iter().find(|(running, _)| running == within).is_none_or(|(_, c)| c.is_cancelled())
        });
        let sent = match cancelled {
            true => Err(RpcError::cancelled()),
            false => self.write(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })),
        };
        if let Err(e) = sent {
            self.take_waiter(id);
            return Err(e);
        }
        Ok(Pending { peer: self.clone(), id, response, progress, reporter, answered: Cell::new(false) })
    }

    fn take_waiter(&self, id: u64) -> Option<Waiter> {
        lock(&self.0.waiting).as_mut()?.remove(&id)
    }

    /// Sends a request and waits for its response.
    pub fn request<P: Serialize, R: DeserializeOwned>(&self, method: &str, params: &P) -> Result<R, RpcError> {
        self.send_request(method, params)?.wait()
    }

    /// Sends a notification. One that can't be sent is dropped, as notifications may be.
    pub fn notify<P: Serialize>(&self, method: &str, params: &P) {
        let _ =
            ok(params).and_then(|params| self.write(&json!({ "jsonrpc": "2.0", "method": method, "params": params })));
    }

    /// The longest line, as errors say it: in MiB, or bytes for one shorter (a test's).
    fn max_line(&self) -> String {
        match self.0.max_line >> 20 {
            0 => format!("{} bytes", self.0.max_line),
            mib => format!("{mib} MiB"),
        }
    }

    /// Answers the other end's request `id`.
    pub fn respond(&self, id: &Value, result: Result<Value, RpcError>) {
        lock(&self.0.running).retain(|(running, _)| running != id);
        self.answer(id, result);
    }

    /// Sends a response; when the result is too long to, an error instead.
    fn answer(&self, id: &Value, result: Result<Value, RpcError>) {
        let response = |result: Result<Value, RpcError>| match result {
            Ok(v) => json!({ "jsonrpc": "2.0", "id": id, "result": v }),
            Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
        };
        if let Err(e) = self.write(&response(result))
            && e.kind() != Some("closed")
        {
            let _ = self.write(&response(Err(e)));
        }
    }

    /// Reads messages until `input` closes, then fails the requests still waiting and cancels
    /// the ones still running.
    fn read(&self, input: impl Read, incoming: &Sender<Incoming>) {
        let mut input = BufReader::new(input);
        let mut buf = Vec::new();
        loop {
            match read_line_within(&mut input, &mut buf, self.0.max_line) {
                Ok(Line::Read) => self.handle(buf.trim_ascii(), incoming),
                Ok(Line::TooLong) => {
                    // If it was a response, nothing says to which request.
                    (self.0.log)(&format!("skipped a line longer than {}", self.max_line()));
                    let e = format!("a line is longer than {}", self.max_line());
                    self.answer(&Value::Null, Err(RpcError::parse_error(e)));
                }
                Ok(Line::End) => break,
                Err(e) => {
                    (self.0.log)(&format!("reading messages: {e}"));
                    break;
                }
            }
        }
        for (_, waiter) in lock(&self.0.waiting).take().unwrap_or_default() {
            let _ = waiter.response.send(Err(RpcError::closed()));
        }
        for (_, cancel) in lock(&self.0.running).drain(..) {
            cancel.cancel();
        }
    }

    /// Handles one line: a response goes to whoever waits for it, `$/cancel` cancels, and the
    /// rest goes to the owner.
    fn handle(&self, line: &[u8], incoming: &Sender<Incoming>) {
        if line.is_empty() {
            return;
        }
        let mut msg: Map<String, Value> = match serde_json::from_slice(line) {
            Ok(Value::Object(msg)) => msg,
            Ok(_) => return self.answer(&Value::Null, Err(RpcError::invalid_request("a message is a JSON object"))),
            Err(e) => {
                // A response must not be answered: tell whoever waits for it instead.
                if let Some(waiter) = response_id(line).and_then(|id| self.take_waiter(id)) {
                    (self.0.log)(&format!("a response that isn't JSON: {e}"));
                    let _ = waiter.response.send(Err(RpcError::parse_error(e)));
                    return;
                }
                return self.answer(&Value::Null, Err(RpcError::parse_error(e)));
            }
        };
        let id = msg.remove("id").unwrap_or_default();
        if !(id.is_null() || id.is_string() || id.is_number()) {
            return self.answer(&Value::Null, Err(RpcError::invalid_request("id must be a string or a number")));
        }
        let Some(Value::String(method)) = msg.remove("method") else {
            return self.on_response(id, msg);
        };
        let params = match msg.remove("params") {
            None | Some(Value::Null) => json!({}),
            Some(params) => params,
        };
        if id.is_null() {
            if method == method::CANCEL {
                return self.on_cancel(params);
            }
            if method == method::PROGRESS && self.on_progress(&params) {
                return;
            }
            if method == method::PARTIAL && self.on_partial(&params) {
                return;
            }
            let _ = incoming.send(Incoming::Notification { method, params });
            return;
        }
        let cancel = CancelToken::new();
        {
            let mut running = lock(&self.0.running);
            if running.iter().any(|(running, _)| *running == id) {
                drop(running);
                let e = format!("request id {id} is already in use by a running request");
                return self.answer(&id, Err(RpcError::invalid_request(e)));
            }
            running.push((id.clone(), cancel.clone()));
        }
        let _ = incoming.send(Incoming::Request(Request { id, method, params, cancel }));
    }

    /// Hands a response to whoever waits for it. A response is never answered, even a bad one.
    fn on_response(&self, id: Value, mut msg: Map<String, Value>) {
        let response = match (msg.remove("result"), msg.remove("error")) {
            (Some(result), None) => Ok(result),
            (None, Some(e)) => Err(serde_json::from_value(e)
                .unwrap_or_else(|e| RpcError::internal(format!("an error response that isn't one: {e}")))),
            _ => {
                (self.0.log)(&format!("a response to {id} with no method, and not one result or error"));
                Err(RpcError::internal("a response with both a result and an error, or neither"))
            }
        };
        let Some(waiter) = id.as_u64().and_then(|n| self.take_waiter(n)) else {
            (self.0.log)(&format!("a response to {id}, which no request is waiting for"));
            return;
        };
        let _ = waiter.response.send(response);
    }

    /// Hands `progress` to the request it's about, when that was sent to have it (see
    /// [`Peer::send_request_reporting`]); false otherwise, for the owner to have it. Handed over
    /// here, on the reading thread, it's there before the request's response.
    fn on_progress(&self, params: &Value) -> bool {
        let Ok(ProgressParams { id, event }) = serde_json::from_value(params.clone()) else { return false };
        let waiting = lock(&self.0.waiting);
        let to = id.as_u64().and_then(|n| waiting.as_ref()?.get(&n)?.progress.as_ref());
        to.is_some_and(|(to, _)| to.send(event).is_ok())
    }

    /// [`Peer::on_progress`] for `partial`.
    fn on_partial(&self, params: &Value) -> bool {
        let Ok(partial) = serde_json::from_value::<Partial>(params.clone()) else { return false };
        let waiting = lock(&self.0.waiting);
        let to = partial.id.as_u64().and_then(|n| waiting.as_ref()?.get(&n)?.progress.as_ref());
        to.is_some_and(|(_, to)| to.send(partial).is_ok())
    }

    /// Cancels the request `params.id` names, and the requests sent while answering it.
    fn on_cancel(&self, params: Value) {
        let Ok(Cancel { id }) = serde_json::from_value(params) else { return };
        if let Some((_, cancel)) = lock(&self.0.running).iter().find(|(running, _)| *running == id) {
            cancel.cancel();
        }
        let within: Vec<(u64, Waiter)> = match lock(&self.0.waiting).as_mut() {
            Some(waiting) => waiting.extract_if(|_, w| w.within.as_ref() == Some(&id)).collect(),
            None => Vec::new(),
        };
        for (n, waiter) in within {
            self.notify(method::CANCEL, &Cancel { id: n.into() });
            let _ = waiter.response.send(Err(RpcError::cancelled()));
        }
    }
}

/// The id of what looks like a response (an `"id"` with a number and no `"method"` among the
/// top-level keys) in a line that isn't JSON, so whoever waits for it can be told.
fn response_id(line: &[u8]) -> Option<u64> {
    let (mut depth, mut i, mut id, mut method) = (0usize, 0, None, false);
    while i < line.len() {
        match line[i] {
            b'"' => {
                // To the closing quote, past escaped characters, or the end of a line cut short
                // in the string (where a backslash's skip would pass it).
                let mut end = i + 1;
                while end < line.len() && line[end] != b'"' {
                    end += if line[end] == b'\\' { 2 } else { 1 };
                }
                let key = &line[i + 1..end.min(line.len())];
                i = end + 1;
                let Some(value) = line.get(i..).map(<[u8]>::trim_ascii_start).and_then(|r| r.strip_prefix(b":")) else {
                    continue;
                };
                match key {
                    b"method" if depth == 1 => method = true,
                    b"id" if depth == 1 => {
                        let value = value.trim_ascii_start();
                        let digits = value.iter().take_while(|b| b.is_ascii_digit()).count();
                        id = std::str::from_utf8(&value[..digits]).ok()?.parse().ok();
                    }
                    _ => {}
                }
                continue;
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
    id.filter(|_| !method)
}

#[cfg(test)]
mod tests;
