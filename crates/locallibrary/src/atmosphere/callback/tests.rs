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

#[test]
fn waits_are_said_in_seconds_then_whole_minutes() {
    let said = |secs| span(Duration::from_secs(secs));
    assert_eq!(
        [said(1), said(30), said(119), said(120), said(121)],
        ["1 second", "30 seconds", "119 seconds", "2 minutes", "3 minutes"]
    );
}

use hegel::generators as gs;
use hegel::{Generator, TestCase};

/// Someone else knocking on the listener while it waits.
#[derive(Debug, Clone, Copy)]
enum Stranger {
    Favicon,
    Post,
    NoState,
    OtherState,
}

/// What a browser sends for `target`, and the connection to read the answer from.
fn send(port: u16, method: &str, target: &str) -> TcpStream {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "{method} {target} HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: text/html\r\n\r\n").unwrap();
    s
}

fn answer_to(mut s: TcpStream) -> String {
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

/// Whatever knocks first, only the redirect carrying the sign-in's state ends the wait, with
/// its query read as it was sent: requests for anything else get a 404 and redirects without
/// the state a 400. The page the browser is shown says how it went, with the message
/// escaped, and is as long as it says. With nobody else coming, the wait ends on time.
#[hegel::test(test_cases = 25)]
fn only_the_sign_ins_own_redirect_ends_the_wait(tc: TestCase) {
    let alnum = || gs::text().alphabet("abcdefghijklmnopqrstuvwxyz0123456789").min_size(1).max_size(12);
    let state = tc.draw(alnum());
    let strangers = tc.draw(
        gs::vecs(
            gs::sampled_from(vec![Stranger::Favicon, Stranger::Post, Stranger::NoState, Stranger::OtherState])
                .print_as_debug(),
        )
        .max_size(4),
    );
    let (code, iss) = (tc.draw(alnum()), tc.draw(gs::sampled_from(vec!["https://pds.example", "https://bsky.social"])));
    let query = format!("code={code}&state={state}&iss={}", iss.replace(':', "%3A").replace('/', "%2F"));

    let (listener, _) = bind().unwrap();
    let port = listener.local_addr().unwrap().port();
    // All of them connect before the wait starts, in order: none of them waits for another.
    let knocked: Vec<(Stranger, TcpStream)> = strangers
        .into_iter()
        .map(|s| {
            let other = format!("x{state}");
            let stream = match s {
                Stranger::Favicon => send(port, "GET", "/favicon.ico"),
                Stranger::Post => send(port, "POST", &format!("/callback?{query}")),
                Stranger::NoState => send(port, "GET", &format!("/callback?code={code}")),
                Stranger::OtherState => send(port, "GET", &format!("/callback?code={code}&state={other}")),
            };
            (s, stream)
        })
        .collect();
    let browser = send(port, "GET", &format!("{CALLBACK_PATH}?{query}"));
    let request = wait(&listener, Duration::from_secs(10), &Ctx::silent(), |s| s == state).unwrap();
    assert_eq!(request.query, CallbackQuery::parse(&query));
    assert_eq!(request.query.iss.as_deref(), Some(iss));

    let ok = tc.draw(gs::booleans());
    let message = tc.draw(gs::text().max_size(30));
    request.respond(ok, &message);
    let page = answer_to(browser);
    let (head, body) = page.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("HTTP/1.1 200 OK") && head.contains("cache-control: no-store"), "{head}");
    assert!(head.contains(&format!("content-length: {}\r\n", body.len())), "{head}");
    let heading = if ok { "You're signed in to locallibrary" } else { "locallibrary couldn't sign you in" };
    assert!(body.contains(&format!("<h1>{}</h1>", escape(heading))), "{body}");
    let escaped = message.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    assert!(body.contains(&format!("<p>{escaped}</p>")), "{body}");
    for (s, stream) in knocked {
        let status = match s {
            Stranger::Favicon | Stranger::Post => "HTTP/1.1 404",
            Stranger::NoState | Stranger::OtherState => "HTTP/1.1 400",
        };
        assert!(answer_to(stream).starts_with(status), "{s:?}");
    }

    // Nobody else comes: the wait ends at its deadline, saying how long it was. Once a run it
    // waits a moment for nobody (each time would be a tenth of a second a case).
    static ONCE: std::sync::Once = std::sync::Once::new();
    let mut timeout = Duration::ZERO;
    ONCE.call_once(|| timeout = Duration::from_millis(50));
    let err = wait(&listener, timeout, &Ctx::silent(), |_| true).unwrap_err().to_string();
    assert!(err.contains(&format!("no answer from the browser within {}; sign in again", span(timeout))), "{err}");
}
