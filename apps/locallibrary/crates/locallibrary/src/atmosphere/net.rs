//! Requests to Atmosphere servers, through clients from here.
//!
//! Where a request goes is often someone else's say-so: a DID document names the account's data
//! server, that server's metadata names its authorization server and endpoints, and any of them
//! can answer with a redirect. Those requests only go to public https addresses ([`public`]):
//! never plain http, this computer or its local network. So do signing in and a client document
//! the user names. The signed-in account's own servers, and a data server named to import
//! settings from (which may be on this computer, for testing), are taken as they are, but their
//! redirects too only go to public https addresses. Answers are read up to [`MAX_BODY`].
//!
//! What a name resolves to isn't checked: reqwest resolves a proxy's name the same way, and
//! proxies are usually on the local network. A name that leads to a private address gets no
//! request: the server there would need a certificate for that name first.

use std::net::IpAddr;

use jacquard_common::SmolStr;
use jacquard_common::http_client::HttpClient;
use jacquard_oauth::scopes::Scopes;

use crate::export::one_line;

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
mod tests {
    use super::*;

    #[test]
    fn only_public_https_addresses_are_public() {
        for ok in
            ["https://bsky.social/xrpc", "https://pds.example.com:8443/", "https://8.8.8.8/", "https://[2606:4700::1]/"]
        {
            assert_eq!(public(ok), Ok(()), "{ok}");
        }
        for refused in [
            "http://bsky.social/",
            "ftp://bsky.social/",
            "file:///etc/hosts",
            "https://localhost/",
            "https://pds.localhost./",
            "https://127.0.0.1:2583/",
            "https://0x7f.1/",
            "https://10.0.0.1/",
            "https://192.168.1.1/",
            "https://172.16.0.1/",
            "https://169.254.169.254/latest/meta-data",
            "https://0.0.0.0/",
            "https://[::1]/",
            "https://[fd00::1]/",
            "https://[fe80::1]/",
            "https://[::ffff:192.168.1.1]/",
            "not a url",
        ] {
            assert!(public(refused).is_err(), "{refused}");
        }
        assert!(!public("https://a\u{1b}]0;x/").unwrap_err().chars().any(char::is_control));
    }

    /// Regression: the fifth redirect was refused, as reqwest counts the first request among the
    /// ones before it.
    #[test]
    fn five_redirects_are_followed_and_no_more() {
        let url = "https://pds.example/";
        assert_eq!(redirect_refused(5, url), None, "the fifth");
        assert!(redirect_refused(6, url).is_some(), "the sixth");
        assert!(redirect_refused(1, "https://127.0.0.1/").is_some());
    }

    #[test]
    fn an_unreadable_granted_scope_is_refused_before_jacquard_reads_it() {
        let answer = |scope: &str| serde_json::json!({ "access_token": "a", "scope": scope }).to_string();
        assert!(readable_scopes(answer("atproto repo:computer.localnetwork.locallibrary.settings").as_bytes()).is_ok());
        let err = readable_scopes(answer("atproto foo:bar").as_bytes()).unwrap_err();
        assert!(err.to_string().contains("can't read (atproto foo:bar)"), "{err}");
        // Anything else passes through untouched.
        assert!(readable_scopes(br#"{"scope":"foo:bar"}"#).is_ok());
        assert!(readable_scopes(b"<html>").is_ok());
        // jacquard would panic on it.
        assert!(Scopes::new(SmolStr::new("atproto foo:bar")).is_err());

        // Every answer jacquard gets goes through the check.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/oauth/token", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            use std::io::{BufRead, Write};
            let (mut conn, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(conn.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let body = answer("atproto foo:bar");
            let _ = write!(conn, "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{body}", body.len());
        });
        let http = Http::new(false).unwrap();
        let request = http::Request::post(url).body(Vec::new()).unwrap();
        let err = crate::atmosphere::block_on(http.send_http(request)).unwrap_err();
        assert!(err.to_string().contains("can't read"), "{err}");
    }
}
