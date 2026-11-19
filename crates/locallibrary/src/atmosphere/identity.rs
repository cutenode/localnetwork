//! Finding an account's DID from its handle, and its servers from the DID, for signing in and
//! [`super::repo::resolve`].
//!
//! A handle is resolved from its own DNS (the `_atproto.<handle>` TXT record, asked of the DNS
//! servers the system is set to use) or its own `/.well-known/atproto-did`, never by asking
//! Bluesky's AppView, which jacquard falls back to by default. The DNS lookup is ours:
//! jacquard 0.12.2's `with_system_dns` builds a resolver with no name servers, which finds
//! nothing, and doesn't take one from outside.

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use jacquard_common::BosStr;
use jacquard_common::http_client::HttpClient;
use jacquard_common::types::did::Did;
use jacquard_common::types::string::Handle;
use jacquard_identity::JacquardResolver;
use jacquard_identity::resolver::{
    self, DidDocResponse, HandleStep, IdentityError, IdentityErrorKind, IdentityResolver, ResolverOptions,
};
use jacquard_oauth::dpop::DpopExt;
use jacquard_oauth::resolver::OAuthResolver;

use super::net::Http;
use crate::Result;
use crate::net::runtime;

/// jacquard's resolver, with a handle looked up in DNS first.
pub(crate) struct Resolver {
    /// Asked for a handle's `/.well-known/atproto-did` alone, a DID's document, and every request.
    jacquard: JacquardResolver<Http>,
    /// `None` when the system's DNS settings can't be read: handles are then found by their web
    /// address alone.
    dns: Option<TokioResolver>,
}

/// The resolver, whose requests go only to public https addresses with `public_only` (signing in,
/// where the servers are whatever the handle's DID document says).
pub(crate) fn resolver(public_only: bool) -> Result<Resolver> {
    let _guard = runtime().enter();
    let dns = TokioResolver::builder_tokio().and_then(|builder| builder.build()).ok();
    Ok(Resolver::new(Http::new(public_only)?, dns))
}

impl Resolver {
    fn new(http: Http, dns: Option<TokioResolver>) -> Resolver {
        // jacquard's own DNS step asks Cloudflare's DNS over HTTPS (without its `dns` feature) and
        // its last step asks Bluesky's AppView, so of a handle it's only asked the web address.
        let options = ResolverOptions { handle_order: vec![HandleStep::HttpsWellKnown], ..ResolverOptions::default() };
        Resolver { jacquard: JacquardResolver::new(http, options), dns }
    }

    /// The DID `handle`'s `_atproto` TXT record names, or why DNS gives none.
    async fn dns_did(&self, handle: &str) -> std::result::Result<Did, String> {
        let name = format!("_atproto.{handle}");
        let Some(dns) = &self.dns else {
            return Err(format!("{name} wasn't looked up: this computer's DNS settings couldn't be read"));
        };
        let answer = match dns.txt_lookup(format!("{name}.")).await {
            Ok(answer) => answer,
            Err(e) if e.is_no_records_found() => return Err(format!("DNS has no {name} record")),
            Err(e) => return Err(format!("DNS couldn't be asked for {name} ({e})")),
        };
        let txts = answer.answers().iter().filter_map(|record| match &record.data {
            RData::TXT(txt) => Some(txt.txt_data.iter().map(|s| String::from_utf8_lossy(s).into_owned())),
            _ => None,
        });
        did_in(txts.flatten()).ok_or_else(|| format!("DNS's {name} record has no did=… in it"))
    }
}

/// A handle neither its DNS nor its web address names a DID for: why DNS didn't, as
/// [`Resolver::dns_did`] says.
#[derive(Debug)]
pub(crate) struct HandleNotFound {
    pub(crate) handle: String,
    pub(crate) dns: String,
}

impl std::fmt::Display for HandleNotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}, and https://{}/.well-known/atproto-did doesn't give a DID", self.dns, self.handle)
    }
}

impl std::error::Error for HandleNotFound {}

/// The DID in a handle's TXT strings: the first `did=` one that holds a valid DID, as jacquard
/// reads them.
fn did_in(txts: impl IntoIterator<Item = String>) -> Option<Did> {
    txts.into_iter().find_map(|txt| Did::new_owned(txt.strip_prefix("did=")?).ok())
}

impl IdentityResolver for Resolver {
    fn options(&self) -> &ResolverOptions {
        self.jacquard.options()
    }

    async fn resolve_handle<S: BosStr + Sync>(&self, handle: &Handle<S>) -> resolver::Result<Did> {
        let dns = match self.dns_did(handle.as_str()).await {
            Ok(did) => return Ok(did),
            Err(why) => why,
        };
        self.jacquard.resolve_handle(handle).await.map_err(|e| match e.kind() {
            // jacquard's own word for it says neither what was asked nor what each answered.
            IdentityErrorKind::HandleResolutionExhausted => {
                let found = HandleNotFound { handle: handle.as_str().to_lowercase(), dns };
                IdentityError::new(IdentityErrorKind::HandleResolutionExhausted, Some(Box::new(found)))
            }
            _ => e,
        })
    }

    async fn resolve_did_doc<S: BosStr + Sync>(&self, did: &Did<S>) -> resolver::Result<DidDocResponse> {
        self.jacquard.resolve_did_doc(did).await
    }
}

impl HttpClient for Resolver {
    type Error = IdentityError;

    async fn send_http(
        &self,
        request: http::Request<Vec<u8>>,
    ) -> std::result::Result<http::Response<Vec<u8>>, IdentityError> {
        self.jacquard.send_http(request).await
    }
}

impl OAuthResolver for Resolver {}

impl DpopExt for Resolver {}

#[cfg(test)]
mod tests;
