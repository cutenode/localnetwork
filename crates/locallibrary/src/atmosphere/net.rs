//! Requests to Atmosphere servers, through clients from here.
//!
//! Where a request goes is often someone else's say-so: a DID document names the account's data
//! server, that server's metadata names its authorization server and endpoints, and any of them
//! can answer with a redirect. Those requests only go to public https addresses
//! ([`crate::net::public`]). So do signing in and a client document the user names. The
//! signed-in account's own servers, and a data server named to import settings from (which may be
//! on this computer, for testing), are taken as they are, but their redirects too only go to
//! public https addresses. Answers are read up to [`MAX_BODY`].

use jacquard_common::SmolStr;
use jacquard_common::http_client::HttpClient;
use jacquard_oauth::scopes::Scopes;

use crate::export::one_line;
pub(crate) use crate::net::{public, with_causes};

/// The most of an answer read: far more than any record, DID document or server metadata.
pub(crate) const MAX_BODY: usize = 1 << 20;

/// The most redirects followed.
const MAX_REDIRECTS: usize = 5;

/// Why a request wasn't sent, or its answer not read.
#[derive(Debug, thiserror::Error)]
pub(crate) enum NetError {
    #[error("{0}")]
    Refused(String),
    #[error("the answer is over {} MB", MAX_BODY >> 20)]
    TooBig,
    #[error("{}", with_causes(.0))]
    Http(#[from] reqwest::Error),
}

/// A client builder whose redirects only go to public https addresses, at most five.
pub(crate) fn client() -> reqwest::ClientBuilder {
    let redirects = reqwest::redirect::Policy::custom(|attempt| {
        match redirect_refused(attempt.previous().len(), attempt.url().as_str()) {
            Some(why) => attempt.error(why),
            None => attempt.follow(),
        }
    });
    reqwest::Client::builder().redirect(redirects)
}

/// Why a redirect to `url` isn't followed, if it isn't, after `previous` requests in its chain
/// (reqwest counts the first one, which wasn't a redirect).
fn redirect_refused(previous: usize, url: &str) -> Option<String> {
    if previous > MAX_REDIRECTS {
        return Some("too many redirects".into());
    }
    public(url).err().map(|why| format!("redirected to an address that isn't public: {why}"))
}

/// An answer's body, up to [`MAX_BODY`].
pub(crate) async fn body(mut response: reqwest::Response) -> Result<Vec<u8>, NetError> {
    let mut out = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if out.len() + chunk.len() > MAX_BODY {
            return Err(NetError::TooBig);
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// The HTTP client jacquard's identity resolution, OAuth and XRPC go through: [`client`]'s, and
/// with `public_only` (servers found from someone's handle or DID) only to [`public`] addresses.
#[derive(Clone)]
pub(crate) struct Http {
    client: reqwest::Client,
    public_only: bool,
}

impl Http {
    pub(crate) fn new(public_only: bool) -> crate::Result<Http> {
        let client = client().build().map_err(|e| crate::Error::Account(format!("the network: {e}")))?;
        Ok(Http { client, public_only })
    }
}

impl HttpClient for Http {
    type Error = NetError;

    async fn send_http(&self, request: http::Request<Vec<u8>>) -> Result<http::Response<Vec<u8>>, NetError> {
        let url = request.uri().to_string();
        if self.public_only {
            public(&url).map_err(NetError::Refused)?;
        }
        let (parts, sent) = request.into_parts();
        let response = self.client.request(parts.method, &url).headers(parts.headers).body(sent).send().await?;
        let mut answer = http::Response::builder().status(response.status());
        if let Some(headers) = answer.headers_mut() {
            headers.extend(response.headers().clone());
        }
        let body = body(response).await?;
        readable_scopes(&body)?;
        answer.body(body).map_err(|e| NetError::Refused(e.to_string()))
    }
}

/// jacquard-oauth 0.12.2 panics on a token answer that grants a scope it can't parse
/// (`client.rs:344-346`); refusing such an answer here makes signing in fail instead.
fn readable_scopes(body: &[u8]) -> Result<(), NetError> {
    let Ok(answer) = serde_json::from_slice::<serde_json::Value>(body) else { return Ok(()) };
    match answer["scope"].as_str() {
        Some(scope) if answer["access_token"].is_string() && Scopes::new(SmolStr::new(scope)).is_err() => {
            Err(NetError::Refused(format!(
                "the server granted scopes this version of locallibrary can't read ({})",
                one_line(scope)
            )))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
