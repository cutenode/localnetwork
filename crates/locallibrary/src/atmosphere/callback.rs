//! The browser's half of signing in: an HTTP listener on 127.0.0.1 that waits for the
//! authorization server's redirect, or the redirect URL pasted by hand when the browser runs on
//! another computer.
//!
//! Any program on this computer can reach the listener, so only a redirect that carries the
//! sign-in's `state` ends the wait; anything else is turned away and the wait goes on.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use super::LONGEST_LOGIN_WAIT;
use crate::export::one_line;
use crate::progress::Ctx;
use crate::{Error, Result};

/// The path the redirect comes back to.
pub const CALLBACK_PATH: &str = "/callback";

/// The most of a request read: a browser's request head is a few kilobytes.
const MAX_REQUEST: u64 = 64 * 1024;

/// What the authorization server sent back.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub iss: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

impl CallbackQuery {
    pub fn parse(query: &str) -> CallbackQuery {
        let mut q = CallbackQuery::default();
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let v = decode(v);
            match decode(k).as_str() {
                "code" => q.code = Some(v),
                "state" => q.state = Some(v),
                "iss" => q.iss = Some(v),
                "error" => q.error = Some(v),
                "error_description" => q.error_description = Some(v),
                _ => {}
            }
        }
        q
    }

    /// The query of a redirect URL the user pasted (the address their browser ended up on).
    pub fn from_url(url: &str) -> Result<CallbackQuery> {
        let url = url.trim();
        let (_, rest) = url
            .split_once("?")
            .ok_or_else(|| Error::invalid("that address has no sign-in result in it (nothing after `?`)"))?;
        let query = rest.split('#').next().unwrap_or(rest);
        let q = CallbackQuery::parse(query);
        if q.code.is_none() && q.error.is_none() {
            return Err(Error::invalid("that address has no sign-in result in it (no `code` or `error`)"));
        }
        Ok(q)
    }

    /// The server's refusal, worded for people (its words on one line, without control
    /// characters, since they end up in a terminal).
    pub fn refusal(&self) -> Option<String> {
        let error = self.error.as_deref()?;
        Some(match (error, self.error_description.as_deref()) {
            ("access_denied", _) => "the sign-in was declined, so locallibrary wasn't given access".to_string(),
            (_, Some(d)) if !d.is_empty() => format!("your account's server stopped the sign-in: {}", one_line(d)),
            (e, _) => format!("your account's server stopped the sign-in ({})", one_line(e)),
        })
    }
}

/// `application/x-www-form-urlencoded` decoding: `+` is a space, then percent-decoding.
fn decode(s: &str) -> String {
    let spaced = s.replace('+', " ");
    percent_encoding::percent_decode_str(&spaced).decode_utf8_lossy().into_owned()
}

/// Bind the listener on an ephemeral 127.0.0.1 port. The server ignores the port when it
/// compares loopback redirect URIs, so any free one works.
pub fn bind() -> Result<(TcpListener, String)> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| Error::Account(format!("can't listen for the browser on 127.0.0.1: {e}")))?;
    listener.set_nonblocking(true).map_err(|e| Error::Account(format!("listener: {e}")))?;
    let port = listener.local_addr().map_err(|e| Error::Account(format!("listener: {e}")))?.port();
    Ok((listener, format!("http://127.0.0.1:{port}{CALLBACK_PATH}")))
}

/// A request to the listener, once read.
#[derive(Debug)]
pub struct Request {
    stream: TcpStream,
    pub query: CallbackQuery,
}

impl Request {
    /// Show the browser a page saying how it went.
    pub fn respond(mut self, ok: bool, message: &str) {
        let (title, heading) = if ok {
            ("Signed in", "You're signed in to locallibrary")
        } else {
            ("Not signed in", "locallibrary couldn't sign you in")
        };
        let body = page(title, heading, message);
        let _ = write!(
            self.stream,
            "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\ncache-control: no-store\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = self.stream.flush();
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn page(title: &str, heading: &str, message: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} · locallibrary</title>
<style>
  :root {{ color-scheme: light dark; font-family: system-ui, sans-serif; }}
  body {{ margin: 0; min-height: 100vh; display: grid; place-items: center; padding: 24px; box-sizing: border-box; }}
  main {{ max-width: 34rem; }}
  h1 {{ font-size: 1.5rem; margin: 0 0 .5rem; }}
  p {{ line-height: 1.5; margin: 0 0 .75rem; opacity: .85; }}
</style>
</head>
<body><main>
<h1>{}</h1>
<p>{}</p>
<p>You can close this tab and go back to locallibrary.</p>
</main></body>
</html>
"#,
        escape(heading),
        escape(message)
    )
}

/// Read one request's head (up to [`MAX_REQUEST`]). `None` for anything that isn't
/// `GET /callback?...` (a favicon request, a stray connection), which gets a 404.
fn read(stream: TcpStream) -> Option<Request> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(stream.try_clone().ok()?.take(MAX_REQUEST));
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    // Drain the headers so the browser isn't reset mid-request.
    let mut header = String::new();
    while reader.read_line(&mut header).is_ok_and(|n| n > 2) {
        header.clear();
    }
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next()?, parts.next()?);
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if method != "GET" || path != CALLBACK_PATH {
        answer(stream, "404 Not Found");
        return None;
    }
    Some(Request { stream, query: CallbackQuery::parse(query) })
}

fn answer(mut stream: TcpStream, status: &str) {
    let _ = write!(stream, "HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
}

/// Wait for the redirect that carries a `state` that `expected` accepts, until `timeout` (at most
/// an hour) or cancellation. Other requests to the callback get a 400.
pub fn wait(
    listener: &TcpListener,
    timeout: Duration,
    ctx: &Ctx<'_>,
    expected: impl Fn(&str) -> bool,
) -> Result<Request> {
    let timeout = timeout.min(LONGEST_LOGIN_WAIT);
    let deadline = Instant::now() + timeout;
    loop {
        ctx.check()?;
        if Instant::now() >= deadline {
            return Err(Error::Account(format!("no answer from the browser within {}; sign in again", span(timeout))));
        }
        match listener.accept() {
            Ok((stream, _)) => match read(stream) {
                Some(request) if request.query.state.as_deref().is_some_and(&expected) => return Ok(request),
                Some(request) => answer(request.stream, "400 Bad Request"),
                None => {}
            },
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(Error::Account(format!("listening for the browser: {e}"))),
        }
    }
}

/// "5 minutes", "30 seconds".
pub fn span(d: Duration) -> String {
    match d.as_secs() {
        1 => "1 second".into(),
        s if s < 120 => format!("{s} seconds"),
        s => format!("{} minutes", s.div_ceil(60)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_and_pasted_urls_parse() {
        let q = CallbackQuery::parse("code=abc%2B1&state=st&iss=https%3A%2F%2Fbsky.social");
        assert_eq!(q.code.as_deref(), Some("abc+1"));
        assert_eq!(q.state.as_deref(), Some("st"));
        assert_eq!(q.iss.as_deref(), Some("https://bsky.social"));
        let q = CallbackQuery::from_url(
            "http://127.0.0.1:53111/callback?error=access_denied&error_description=Access+denied&state=s#x",
        )
        .unwrap();
        assert_eq!(q.error_description.as_deref(), Some("Access denied"));
        assert!(q.refusal().unwrap().contains("declined"));
        assert!(CallbackQuery::from_url("http://127.0.0.1/callback").is_err());
        assert!(CallbackQuery::from_url("http://127.0.0.1/callback?foo=bar").is_err());
        // What the server says goes to a terminal: no escape sequences.
        let q = CallbackQuery::parse("error=x%1B&error_description=%1B%5D0%3Bowned%07%0Aline");
        let refusal = q.refusal().unwrap();
        assert!(!refusal.chars().any(char::is_control), "{refusal:?}");
        assert!(refusal.ends_with(" ]0;owned  line"), "{refusal:?}");
        let q = CallbackQuery::parse("error=bad%1B%5B2J");
        assert!(!q.refusal().unwrap().chars().any(char::is_control));
    }

    #[test]
    fn the_listener_waits_for_the_sign_ins_own_redirect() {
        let (listener, redirect) = bind().unwrap();
        assert!(redirect.starts_with("http://127.0.0.1:") && redirect.ends_with("/callback"));
        let port = listener.local_addr().unwrap().port();
        let browser = std::thread::spawn(move || {
            let get = |target: &str| {
                let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
                write!(s, "GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
                let mut out = String::new();
                let _ = std::io::Read::read_to_string(&mut s, &mut out);
                out
            };
            let favicon = get("/favicon.ico");
            // Anyone on this computer can find the port: a refusal or a code without the
            // sign-in's state is turned away, and the wait goes on.
            let no_state = get("/callback?error=access_denied&error_description=gotcha");
            let other_state = get("/callback?error=access_denied&state=guess");
            let page = get("/callback?code=c0de&state=st4te&iss=https%3A%2F%2Fpds.example");
            [favicon, no_state, other_state, page]
        });
        let request = wait(&listener, Duration::from_secs(10), &Ctx::silent(), |s| s == "st4te").unwrap();
        assert_eq!(request.query.code.as_deref(), Some("c0de"));
        assert_eq!(request.query.iss.as_deref(), Some("https://pds.example"));
        request.respond(true, "Signed in as @dj.example.com <ok>");
        let [favicon, no_state, other_state, page] = browser.join().unwrap();
        assert!(favicon.starts_with("HTTP/1.1 404"));
        assert!(no_state.starts_with("HTTP/1.1 400") && other_state.starts_with("HTTP/1.1 400"));
        assert!(page.contains("You're signed in to locallibrary"));
        assert!(page.contains("@dj.example.com &lt;ok&gt;"), "messages are escaped");
    }

    #[test]
    fn an_endless_request_is_cut_off() {
        let (listener, _) = bind().unwrap();
        let port = listener.local_addr().unwrap().port();
        let (done, finished) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            // A request line that never ends, from a connection that stays open.
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let _ = s.write_all(&vec![b'a'; 4 * MAX_REQUEST as usize]);
            let mut real = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(real, "GET /callback?code=c&state=s HTTP/1.1\r\n\r\n").unwrap();
            let _ = finished.recv();
        });
        let started = Instant::now();
        let request = wait(&listener, Duration::from_secs(30), &Ctx::silent(), |s| s == "s").unwrap();
        // Read whole, the endless line would hold the listener until its 5 second read timeout.
        assert!(started.elapsed() < Duration::from_secs(4), "{:?}", started.elapsed());
        assert_eq!(request.query.code.as_deref(), Some("c"));
        drop(done);
    }

    #[test]
    fn waiting_stops_at_the_deadline_or_when_cancelled() {
        let (listener, _) = bind().unwrap();
        let err = wait(&listener, Duration::from_millis(150), &Ctx::silent(), |_| true).unwrap_err();
        assert!(err.to_string().contains("no answer"), "{err}");
        let cancel = crate::CancelToken::new();
        cancel.cancel();
        let progress = crate::progress::NoProgress;
        let err = wait(&listener, Duration::from_secs(10), &Ctx::new(&progress, cancel), |_| true).unwrap_err();
        assert!(matches!(err, Error::Cancelled));
        // However long a caller asks for, the wait ends: no overflow working out when.
        let cancel = crate::CancelToken::new();
        cancel.cancel();
        let err = wait(&listener, Duration::MAX, &Ctx::new(&progress, cancel), |_| true).unwrap_err();
        assert!(matches!(err, Error::Cancelled));
    }
}
