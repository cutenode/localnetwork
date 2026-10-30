//! Where locallibrary sends requests someone else chose: the Atmosphere servers a handle or DID
//! names, and what plugins fetch (`net/fetch`). Those go only to public https addresses
//! ([`public`]): never plain http, this computer or its local network.
//!
//! What a name resolves to isn't checked: reqwest resolves a proxy's name the same way, and
//! proxies are usually on the local network. A name that leads to a private address gets no
//! request: the server there would need a certificate for that name first.

use std::net::IpAddr;

use crate::export::one_line;

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

/// `url` parsed, when it's a public https address; otherwise why not.
pub(crate) fn public(url: &str) -> Result<reqwest::Url, String> {
    let shown = one_line(url);
    let parsed = reqwest::Url::parse(url).map_err(|_| format!("{shown} isn't a web address"))?;
    if parsed.scheme() != "https" {
        return Err(format!("{shown} isn't an https address"));
    }
    let host = parsed.host_str().unwrap_or_default();
    if host.is_empty() || this_computer(&parsed) || ip(&parsed).is_some_and(local) {
        return Err(format!("{shown} is on this computer or its local network"));
    }
    Ok(parsed)
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
