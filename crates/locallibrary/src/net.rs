//! Where locallibrary sends requests someone else chose: the Atmosphere servers a handle or DID
//! names, and what plugins fetch (`net/fetch`). Those go only to public https addresses
//! ([`public`]): never plain http, this computer or its local network.
//!
//! What a name resolves to isn't checked: reqwest resolves a proxy's name the same way, and
//! proxies are usually on the local network. A name that leads to a private address gets no
//! request: the server there would need a certificate for that name first.
//!
//! Requests that can be cancelled go out on a runtime of their own ([`Requests`]), so they stop at
//! once, even while a server has stopped reading what they send; the rest go out on one shared
//! [`runtime`].

use std::io::Read;
use std::net::IpAddr;
use std::sync::OnceLock;
use std::time::Duration;

use crate::export::one_line;
use crate::progress::CancelToken;

/// How often a request notices being cancelled.
pub(crate) const CHECK_EVERY: Duration = Duration::from_millis(50);
/// How much of a file a request's body reads at a time.
const CHUNK: usize = 64 << 10;

/// The async runtime requests that can't be cancelled go out on; everything outside the network
/// stays synchronous.
pub(crate) fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("network")
            .enable_all()
            .build()
            .expect("start the network runtime")
    })
}

/// Requests that stop as soon as they're cancelled, on a runtime of their own. Dropped, it takes
/// their connections with it, and the files their bodies were reading: hyper keeps a connection
/// whose request was dropped until it has sent what it buffered, which it never does to a server
/// that has stopped reading. So their client keeps no connection for later ones.
pub(crate) struct Requests(tokio::runtime::Runtime);

impl Requests {
    pub(crate) fn new() -> std::io::Result<Requests> {
        tokio::runtime::Builder::new_current_thread().enable_all().build().map(Requests)
    }

    /// What the future `f` makes gives, or `None` once `cancel` is cancelled (checked every
    /// [`CHECK_EVERY`], and once `f` is done: a body [`streamed`] fails as soon as it's
    /// cancelled). `f` is called on the runtime: reqwest starts a request's timer there.
    pub(crate) fn run<F: Future>(&self, f: impl FnOnce() -> F, cancel: &CancelToken) -> Option<F::Output> {
        let cancelled = async {
            while !cancel.is_cancelled() {
                tokio::time::sleep(CHECK_EVERY).await;
            }
        };
        let out = self.0.block_on(async {
            tokio::select! {
                out = f() => Some(out),
                () = cancelled => None,
            }
        });
        out.filter(|_| !cancel.is_cancelled())
    }
}

/// `reader` as a request's body, read a chunk at a time as it's sent, off the runtime's own
/// threads (a file's reads block). Its length is the request's `Content-Length` to say. Once
/// `cancel` is, it fails at the next chunk: a server reading it gets no more than was read,
/// where [`Requests::run`] would notice only at its next check, sending meanwhile.
pub(crate) fn streamed(reader: impl Read + Send + 'static, cancel: CancelToken) -> reqwest::Body {
    let chunks = futures_util::stream::try_unfold((reader, cancel), |(mut reader, cancel)| async move {
        let read = tokio::task::spawn_blocking(move || {
            if cancel.is_cancelled() {
                return Err(std::io::Error::other("cancelled"));
            }
            let mut chunk = vec![0; CHUNK];
            let n = reader.read(&mut chunk)?;
            chunk.truncate(n);
            Ok((n > 0).then_some((chunk, (reader, cancel))))
        });
        read.await.map_err(std::io::Error::other)?
    });
    reqwest::Body::wrap_stream(chunks)
}

/// An error and what caused it, each cause once: reqwest's own message leaves the cause out,
/// and so do some of jacquard's, while others name it.
pub(crate) fn with_causes(e: &(dyn std::error::Error + 'static)) -> String {
    let mut out = e.to_string();
    let mut cause = e.source();
    while let Some(c) = cause {
        let said = c.to_string();
        if !out.contains(&said) {
            out = format!("{out}: {said}");
        }
        cause = c.source();
    }
    out
}

/// `Ok` when `url` is a public https address; otherwise why not.
pub(crate) fn public(url: &str) -> Result<(), String> {
    let shown = one_line(url);
    let parsed = reqwest::Url::parse(url).map_err(|_| format!("{shown} isn't a web address"))?;
    if parsed.scheme() != "https" {
        return Err(format!("{shown} isn't an https address"));
    }
    let host = parsed.host_str().unwrap_or_default();
    if host.is_empty() || this_computer(&parsed) || ip(&parsed).is_some_and(local) {
        return Err(format!("{shown} is on this computer or its local network"));
    }
    Ok(())
}

/// Whether `url` names this computer: `localhost` or a name below it, or a loopback address.
pub(crate) fn this_computer(url: &reqwest::Url) -> bool {
    let host = url.host_str().unwrap_or_default().trim_end_matches('.');
    host == "localhost" || host.ends_with(".localhost") || ip(url).is_some_and(|ip| ip.to_canonical().is_loopback())
}

/// The address `url` names, when it's given as one.
fn ip(url: &reqwest::Url) -> Option<IpAddr> {
    url.host_str()?.trim_start_matches('[').trim_end_matches(']').parse().ok()
}

/// This computer, a private network, or a link's own addresses.
fn local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified(),
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6.to_ipv4_mapped().is_some_and(|v4| local(IpAddr::V4(v4)))
        }
    }
}

#[cfg(test)]
mod tests;
