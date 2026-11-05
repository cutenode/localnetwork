//! Test support for a fake host: cancelling a request the plugin is serving right after the host
//! has answered one of the plugin's calls, the way a user's cancel can land between two calls.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

use crate::{Cancel, Peer, Pending, RpcError, method};

/// The request a fake host cancels, and whether the answer it's sending now cancels it.
///
/// A harness sends the request to cancel through [`Canceller::send`], calls
/// [`Canceller::arm`] while it works out the answer that cancels, and answers every call
/// through [`Canceller::respond`].
#[derive(Debug, Default)]
pub struct Canceller {
    /// The id of the request last sent through [`Canceller::send`]. Held while it's sent, so a
    /// call the plugin makes for it can't read it before it's there.
    sent: Mutex<Option<u64>>,
    /// Whether [`Canceller::respond`] cancels that request once it has answered.
    armed: AtomicBool,
}

impl Canceller {
    /// Sends a request with `send` as the one a later cancel goes to.
    pub fn send(&self, send: impl FnOnce() -> Pending) -> Pending {
        let mut sent = self.sent.lock().unwrap();
        let pending = send();
        *sent = Some(pending.id());
        pending
    }

    /// Makes the next [`Canceller::respond`] cancel the request it was sent for.
    pub fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }

    /// Answers the plugin's call `id` with `answer` through `host`, and then, when armed, cancels
    /// the request [`Canceller::send`] sent.
    pub fn respond(&self, host: &Peer, id: &Value, answer: Result<Value, RpcError>) {
        host.respond(id, answer);
        if !self.armed.swap(false, Ordering::SeqCst) {
            return;
        }
        let sent = self.sent.lock().unwrap().expect("a request sent through Canceller::send to cancel");
        host.notify(method::CANCEL, &Cancel { id: sent.into() });
    }
}
