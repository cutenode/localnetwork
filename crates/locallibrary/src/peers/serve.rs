//! The serving end: answers [`protocol`](super::protocol) requests from the store, read only.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use tokio::sync::mpsc::UnboundedSender;

use super::protocol::{self, ALPN, NOT_FOUND, OK, REFUSED, Request, answer};
use super::{ServeEvent, ServeOptions, ServeReport, ShareCode, endpoint, runtime, short};
use crate::catalog;
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result};

/// Serve `store` until `ctx` is cancelled, telling `on_event` what happens (first the share code
/// to give the other computer).
pub fn serve(
    store: &Store,
    opts: &ServeOptions,
    ctx: &Ctx<'_>,
    on_event: &mut dyn FnMut(&ServeEvent),
) -> Result<ServeReport> {
    catalog::of(store)?;
    runtime()?.block_on(async {
        let endpoint = endpoint(opts.local_only, vec![ALPN.to_vec()]).await?;
        if !opts.local_only {
            // The code then names the relay that reaches this computer from anywhere. Without one
            // (offline, a firewall), it still works on the local network.
            let _ = tokio::time::timeout(Duration::from_secs(10), endpoint.online()).await;
        }
        let code = ShareCode::new(endpoint.addr());
        let (events, mut received) = tokio::sync::mpsc::unbounded_channel();
        let handler = Handler { store: store.clone(), token: code.token.clone(), events };
        let router = Router::builder(endpoint).accept(ALPN, handler).spawn();
        on_event(&ServeEvent::Ready { code: code.to_string() });
        let mut report = ServeReport::default();
        while ctx.check().is_ok() {
            tokio::select! {
                Some(event) = received.recv() => {
                    if let ServeEvent::Done { files, bytes, .. } = &event {
                        report.peers += 1;
                        report.files += files;
                        report.bytes += bytes;
                    }
                    on_event(&event);
                }
                () = tokio::time::sleep(Duration::from_millis(200)) => {}
            }
        }
        let _ = router.shutdown().await;
        Ok(report)
    })
}

#[derive(Debug, Clone)]
struct Handler {
    store: Store,
    token: String,
    events: UnboundedSender<ServeEvent>,
}

/// What one connection was sent.
#[derive(Default)]
struct Sent {
    snapshots: AtomicU64,
    files: AtomicU64,
    bytes: AtomicU64,
}

impl ProtocolHandler for Handler {
    async fn accept(&self, conn: Connection) -> std::result::Result<(), AcceptError> {
        let peer = short(&conn.remote_id());
        let (mut send, mut recv) = conn.accept_bi().await?;
        // Someone who connects and says nothing is turned away after a while.
        let hello = tokio::time::timeout(Duration::from_secs(30), protocol::read_request(&mut recv)).await;
        let allowed = matches!(&hello, Ok(Ok(Request::Hello { token })) if same(token, &self.token));
        if !allowed {
            let _ = answer(&mut send, REFUSED, b"").await;
            let _ = self.events.send(ServeEvent::Refused { peer });
            // The other end hangs up once it has read the refusal; closing first could lose it.
            let _ = tokio::time::timeout(Duration::from_secs(10), conn.closed()).await;
            conn.close(1u32.into(), b"refused");
            return Ok(());
        }
        let store = self.store.clone();
        let catalog = tokio::task::spawn_blocking(move || catalog::of(&store)).await.map_err(AcceptError::from_err)?;
        match catalog {
            Ok(c) => answer(&mut send, OK, &serde_json::to_vec(&c).expect("serializes")).await,
            Err(e) => answer(&mut send, NOT_FOUND, e.to_string().as_bytes()).await,
        }
        .map_err(AcceptError::from_err)?;
        let _ = self.events.send(ServeEvent::Connected { peer: peer.clone() });

        // Each request on its own stream, answered as it comes, until the other end hangs up.
        let sent = Arc::new(Sent::default());
        let mut answering = tokio::task::JoinSet::new();
        while let Ok((send, recv)) = conn.accept_bi().await {
            let (store, sent) = (self.store.clone(), sent.clone());
            answering.spawn(async move { respond(&store, send, recv, &sent).await });
        }
        while answering.join_next().await.is_some() {}
        let _ = self.events.send(ServeEvent::Done {
            peer,
            snapshots: sent.snapshots.load(Ordering::Relaxed) as usize,
            files: sent.files.load(Ordering::Relaxed) as usize,
            bytes: sent.bytes.load(Ordering::Relaxed),
        });
        Ok(())
    }
}

async fn respond(
    store: &Store,
    mut send: iroh::endpoint::SendStream,
    mut recv: iroh::endpoint::RecvStream,
    sent: &Sent,
) -> Result<()> {
    let (path, counter) = match protocol::read_request(&mut recv).await? {
        Request::Snapshot { id } => (store.snapshot_path(&id), &sent.snapshots),
        Request::Object { hash } => (store.object_path(&hash), &sent.files),
        Request::Hello { .. } => return answer(&mut send, REFUSED, b"").await,
    };
    let mut file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => return answer(&mut send, NOT_FOUND, b"").await,
    };
    send.write_all(&[OK]).await.map_err(protocol::net)?;
    let n = tokio::io::copy(&mut file, &mut send).await.map_err(|e| Error::Io { path, source: e })?;
    send.finish().map_err(protocol::net)?;
    counter.fetch_add(1, Ordering::Relaxed);
    sent.bytes.fetch_add(n, Ordering::Relaxed);
    Ok(())
}

/// `a == b`, in time that doesn't depend on where they differ.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

#[cfg(test)]
mod tests;
