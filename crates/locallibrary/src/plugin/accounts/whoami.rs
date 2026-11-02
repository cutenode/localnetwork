//! Who signed in: what an account type's whoami endpoint answers a new access token, read at its
//! JSON pointers.

use loclib_plugin::{AccountType, TokenPlace};
use oauth2::http::{Request, header};
use reqwest::Url;
use serde_json::Value;

use super::Unanswered;
use crate::plugin::process::Plugin;
use crate::{Error, Result};

/// The most redirects followed.
const MAX_REDIRECTS: usize = 5;

/// `url` with `token` as its query parameter `name`, whatever it had there.
fn with_token(url: &Url, name: &str, token: &str) -> Url {
    let mut url = url.clone();
    let kept: Vec<(String, String)> =
        url.query_pairs().filter(|(k, _)| k != name).map(|(k, v)| (k.into(), v.into())).collect();
    url.query_pairs_mut().clear().extend_pairs(kept).append_pair(name, token);
    url
}

impl Plugin {
    /// The subject and name of whoever `token` is `t`'s access token for, from its whoami
    /// endpoint: reached as the service's other endpoints are (see [`Plugin::send`]), with the
    /// token where `t` says it goes, following redirects within the endpoint's origin only, the
    /// token given again in the query when it goes there. A subject is a string or an integer
    /// (as its digits); the name falls back to it.
    pub(super) fn whoami(&self, t: &AccountType, token: &str) -> Result<(String, String)> {
        let failed = |why: String| Error::Account(self.net.scrub(&format!("asking {} who signed in: {why}", t.title)));
        let mut url = Url::parse(&t.whoami.url).map_err(|e| failed(e.to_string()))?;
        let mut hops = 0;
        let answer = loop {
            let request = match &t.token {
                TokenPlace::Scheme(scheme) => {
                    Request::get(url.as_str()).header(header::AUTHORIZATION, format!("{scheme} {token}"))
                }
                TokenPlace::Query(name) => Request::get(with_token(&url, name, token).as_str()),
            };
            let request = request.header(header::ACCEPT, "application/json").body(Vec::new());
            let answer =
                self.send(request.map_err(|e| failed(e.to_string()))?).map_err(|Unanswered(why)| failed(why))?;
            let location = answer.headers().get(header::LOCATION).and_then(|l| l.to_str().ok());
            let (Some(location), 301 | 302 | 303 | 307 | 308) = (location, answer.status().as_u16()) else {
                break answer;
            };
            if hops == MAX_REDIRECTS {
                return Err(failed(format!("too many redirects (it follows {MAX_REDIRECTS})")));
            }
            hops += 1;
            let next = url.join(location).map_err(|e| failed(e.to_string()))?;
            if next.origin() != url.origin() {
                let to = next.host_str().unwrap_or_default();
                return Err(failed(format!("it sent the token on to {to}, which isn't where it was asked")));
            }
            url = next;
        };
        if !answer.status().is_success() {
            return Err(failed(format!("it answered {}", answer.status())));
        }
        let who: Value = serde_json::from_slice(answer.body()).map_err(|_| failed("its answer isn't JSON".into()))?;
        let subject = match who.pointer(&t.whoami.subject) {
            Some(Value::String(s)) if !s.is_empty() => s.clone(),
            Some(Value::Number(n)) if n.is_i64() || n.is_u64() => n.to_string(),
            _ => return Err(failed(format!("its answer has no subject at {}", t.whoami.subject))),
        };
        let name = who.pointer(&t.whoami.name).and_then(Value::as_str).filter(|n| !n.is_empty());
        Ok((subject.clone(), name.map_or(subject, String::from)))
    }
}
