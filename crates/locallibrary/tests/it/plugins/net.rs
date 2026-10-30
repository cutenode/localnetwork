//! `net/fetch`: what the host fetches for a plugin, from a server on this computer (which only
//! tests may reach, `Options::test_loopback`), and what it keeps from the plugin: hosts it isn't
//! granted, redirects elsewhere, more than it may hold, and its secrets.

use locallibrary::keychain::Storage;

use super::*;
pub(super) use crate::mock_oauth::{Got, Reply, Server, ok};

fn redirect(to: String) -> Reply {
    Reply { status: 302, headers: vec![("Location", to)], body: Vec::new() }
}

/// The echo plugin, talking to this computer (`net:127.0.0.1`), at most
/// `per_second` requests a second to this computer, with a contact and a secret setting.
pub(super) fn talking(per_second: f64) -> String {
    let settings = r#"
[[contributes.settings]]
key = "contact"
title = "Contact"
type = "string"
default = "https://contact.example"

[[contributes.settings]]
key = "token"
title = "Token"
type = "secret"
"#;
    let limit = format!("\n[[net]]\nhost = \"127.0.0.1\"\nper_second = {per_second:?}\n");
    format!("{}{settings}{limit}", granting(&["net:127.0.0.1"]))
}

/// The plugin as `talking` declares it, reaching servers on this computer, its secrets in a file.
pub(super) fn fixture(manifest: &str, args: &'static [&'static str], options: Options) -> Fixture {
    let manifest = manifest.to_string();
    let options = Options { test_loopback: true, secrets: Storage::File, ..options };
    Fixture::new(move |dir| vec![spec(dir, &manifest, args)], options)
}

impl Fixture {
    /// What the plugin's `net/fetch` of `fetch` answered it: `{"result": ...}` or `{"error": ...}`.
    pub(super) fn fetch(&self, fetch: Value) -> Value {
        self.echo_params(json!({ "fetch": fetch })).unwrap()
    }
}

#[test]
fn a_plugin_fetches_from_a_server_it_may_talk_to_as_locallibrary() {
    let server = Server::new(|_, got| match got.target.as_str() {
        "/hello?q=a+b" => {
            Reply { status: 201, headers: vec![("X-Two", "1".into()), ("X-Two", "2".into())], body: b"hi".to_vec() }
        }
        _ => ok(format!("{} {}", got.method, got.body)),
    });
    let f = fixture(&talking(100.0), &[], Options::default());
    let fetch =
        json!({ "url": format!("{}/hello?q=a+b", server.url), "headers": { "User-Agent": "me", "X-One": "1" } });
    let answer = f.fetch(fetch);
    assert_eq!(answer["result"]["status"], 201, "{answer}");
    assert_eq!((&answer["result"]["body"], &answer["result"]["headers"]["x-two"]), (&json!("hi"), &json!("1, 2")));
    let got = &server.got()[0];
    let agent = format!("locallibrary/{} (+https://contact.example)", env!("CARGO_PKG_VERSION"));
    assert_eq!((got.headers["user-agent"].as_str(), got.headers["x-one"].as_str()), (agent.as_str(), "1"));
    let posted = f.fetch(json!({ "url": format!("{}/post", server.url), "method": "POST", "body": "sent" }));
    assert_eq!(posted["result"]["body"], "POST sent");

    // Without its contact, it's only locallibrary.
    f.plugins.set_setting("echo", &f.store, "contact", Some(json!(" "))).unwrap();
    f.fetch(json!({ "url": format!("{}/hello", server.url) }));
    assert_eq!(server.got()[2].headers["user-agent"], format!("locallibrary/{}", env!("CARGO_PKG_VERSION")));
}

/// Only servers the plugin's `net:` capabilities grant, at public https addresses (or this
/// computer, in tests), however a redirect leads, five at most.
#[test]
fn a_plugin_reaches_only_where_it_may() {
    let server = Server::new(|base, got| match got.target.as_str() {
        "/away" => redirect(format!("{}/there", base.replace("127.0.0.1", "localhost"))),
        "/hop/6" => ok("arrived"),
        target => match target.strip_prefix("/hop/") {
            Some(n) => redirect(format!("{base}/hop/{}", n.parse::<u8>().unwrap() + 1)),
            None => ok("here"),
        },
    });
    let f = fixture(&talking(100.0), &[], Options::default());
    let refused = |url: String| {
        let answer = f.fetch(json!({ "url": url }));
        (answer["error"]["data"]["kind"].as_str().unwrap_or_default().to_string(), answer["error"]["message"].clone())
    };
    let (kind, why) = refused(server.url.replace("127.0.0.1", "localhost"));
    assert_eq!(kind, "denied");
    assert_eq!(why, "localhost isn't a server the plugin may talk to (no net:localhost capability)");
    let (kind, why) = refused(format!("{}/away", server.url));
    assert_eq!(kind, "external");
    assert!(why.as_str().unwrap().contains("redirected to where it may not go: localhost isn't a server"), "{why}");
    let (kind, why) = refused(format!("{}/hop/0", server.url));
    assert!(kind == "external" && why.as_str().unwrap().contains("too many redirects"), "{why}");
    assert_eq!(f.fetch(json!({ "url": format!("{}/hop/1", server.url) }))["result"]["body"], "arrived", "five");
    assert_eq!(refused("not a url".into()).1, "not a url isn't a web address");
    let (kind, why) = refused("https://example.org/".into());
    assert!(kind == "denied" && why.as_str().unwrap().starts_with("example.org isn't a server"), "{why}");
    let e = f.fetch(json!({ "url": server.url.clone(), "method": "NOT A METHOD" }));
    assert_eq!(e["error"]["message"], "NOT A METHOD isn't an HTTP method");
    let e = f.fetch(json!({ "url": server.url.clone(), "headers": { "Bad Name": "x" } }));
    assert_eq!(e["error"]["message"], "\"Bad Name\" isn't a header's name");
    let e = f.fetch(json!({ "url": server.url.clone(), "headers": { "X": "a\nb" } }));
    assert_eq!(e["error"]["message"], "the x header's value can't be sent");
    let e = f.fetch(json!({ "url": server.url.clone(), "body": "a", "body_blob": "/b" }));
    assert_eq!(e["error"]["message"], "a request sends a body or a body_blob, not both");

    // A release never reaches this computer, nor do tests that don't ask to.
    let f = Fixture::new(|dir| vec![spec(dir, &talking(100.0), &[])], Options::default());
    let answer = f.fetch(json!({ "url": server.url.clone() }));
    assert!(answer["error"]["message"].as_str().unwrap().ends_with("isn't an https address"), "{answer}");
}

/// What a plugin may be answered in a message is held to `max_body`, and to text: more, or an
/// image, goes to a file in the request's scratch folder, as far as the folder has room.
#[test]
fn a_big_answer_goes_to_a_file_in_the_scratch_folder() {
    let server = Server::new(|_, got| match got.target.as_str() {
        "/big" => ok(vec![b'a'; 64]),
        "/image" => ok(vec![0xff, 0xd8, 0xff, 0x00]),
        _ => ok(got.body.clone()),
    });
    let f = fixture(&talking(100.0), &[], Options { max_body: 32, max_blobs: 100, ..Options::default() });
    let e = f.fetch(json!({ "url": format!("{}/big", server.url) }));
    assert_eq!(e["error"]["data"]["kind"], "too_big");
    assert_eq!(e["error"]["message"], "the answer is over 32 B: fetch it into_blob");
    let e = f.fetch(json!({ "url": format!("{}/image", server.url) }));
    assert_eq!(e["error"]["message"], "the answer isn't text: fetch it into_blob");

    // Into a file in this request's scratch folder (the test plugin names it there), as far as
    // the folder has room.
    let into = |to: &str, url: &str| json!({ "url": format!("{}/{url}", server.url), "into_blob": to });
    let answer = f.fetch(into("a.jpg", "image"));
    assert_eq!(answer["read"], String::from_utf8_lossy(&[0xff, 0xd8, 0xff, 0x00]).as_ref(), "{answer}");
    assert!(answer["result"]["blob"].as_str().unwrap().ends_with("a.jpg") && answer["result"]["body"].is_null());
    let e = f.fetch(json!({ "url": format!("{}/big", server.url), "into_blob": "big", "body_blob": "big" }));
    assert!(e["error"]["message"].as_str().unwrap().contains("big: "), "the file to send isn't there: {e}");
    let sent = json!({
        "fetch": { "url": format!("{}/echo", server.url), "method": "PUT", "body_blob": "sent.txt" },
        "blobs": { "sent.txt": b"sent" },
    });
    assert_eq!(f.echo_params(sent).unwrap()["result"]["body"], "sent");
    // 48 bytes held already, of the 100 a request may.
    let e = f.echo_params(json!({ "fetch": into("more", "big"), "blobs": { "held": vec![0; 48] } })).unwrap();
    assert_eq!(e["error"]["data"]["kind"], "too_big", "{e}");
    assert_eq!(e["error"]["message"], "the answer is more than the scratch folder has room for (52 B)");
    // Elsewhere: an earlier request's folder, or anywhere else.
    let answer = f.echo_params(json!({ "context": true })).unwrap();
    let scratch = PathBuf::from(answer["scratch"].as_str().unwrap());
    assert!(!scratch.exists(), "removed once the command answered");
    let e = f.fetch(into(&scratch.join("a.jpg").display().to_string(), "image"));
    assert!(e["error"]["message"].as_str().unwrap().ends_with("isn't in the scratch folder of a request under way"));
}

/// A secret goes where the request names it (a header, a query value, the body) once it's been
/// checked, and nowhere else: not in what the plugin logs, nor in an error it's told.
#[test]
fn a_secret_goes_only_where_a_request_names_it() {
    let server =
        Server::new(|_, got| ok(format!("echoed {}", got.headers.get("x-token").cloned().unwrap_or_default())));
    let f = fixture(&talking(100.0), &[], Options::default());
    let fetch = || {
        json!({
            "url": format!("{}/q?key={{secret:token}}&plain=1", server.url),
            "method": "POST",
            "headers": { "X-Token": "Bearer {secret:token}" },
            "body": "token={secret:token}",
        })
    };
    let e = f.fetch(fetch());
    assert_eq!(e["error"]["message"], "its secret token isn't set (locallibrary plugin settings echo token --secret)");
    f.plugins.set_setting("echo", &f.store, "token", Some(json!("hunter 2&"))).unwrap();
    let answer = f.fetch(fetch());
    let got = server.got().pop().unwrap();
    assert_eq!(got.target, "/q?key=hunter+2%26&plain=1");
    assert_eq!((got.headers["x-token"].as_str(), got.body.as_str()), ("Bearer hunter 2&", "token=hunter 2&"));
    // The server said it back: the plugin has it, but its log doesn't.
    assert_eq!(answer["result"]["body"], "echoed Bearer hunter 2&");
    eventually("the plugin logged it", || f.logs.lock().unwrap().iter().any(|(_, l)| l.starts_with("fetched")));
    let logs = f.logs.lock().unwrap().clone();
    assert!(logs.contains(&("echo".into(), "fetched echoed Bearer <secret>".into())), "{logs:?}");

    // Nor in an error naming the request.
    let e = f.fetch(json!({ "url": "http://127.0.0.1:1/?key={secret:token}" }));
    let said = e["error"]["message"].as_str().unwrap();
    assert!(said.contains("key=<secret>") && !said.contains("hunter"), "{said}");
    let e = f.fetch(json!({ "url": server.url.clone(), "headers": { "X": "{secret:greeting}" } }));
    assert_eq!(e["error"]["message"], "\"greeting\" isn't one of its secret settings");
    let e = f.fetch(json!({ "url": server.url.clone(), "body": "{secret:token" }));
    assert_eq!(e["error"]["message"], "a {secret:<key>} placeholder isn't closed");
    // The secret is never in the plugin's context.
    assert!(!f.echo_params(json!({ "context": true })).unwrap().to_string().contains("hunter"));

    // Started without a store, it has none set; with secrets that can't be read, the request
    // fails saying so.
    let bare = fixture(&talking(100.0), &[], Options::default());
    let params = json!({ "fetch": fetch() });
    let without = bare.plugins.run_command("echo", "echo", params, bare.context(), None, &Ctx::silent(), &|_| {});
    let CommandOutput::Headless(e) = without.unwrap() else { panic!("headless") };
    assert!(e["error"]["message"].as_str().unwrap().starts_with("its secret token isn't set"), "{e}");
    std::fs::write(f.store.root().join("secrets/plugins/echo.json"), "not json").unwrap();
    let e = f.fetch(fetch());
    assert_eq!(e["error"]["data"]["kind"], "external", "{e}");
    assert!(server.got().len() == 1, "nothing more was sent");
}

/// Requests to a host with a limit are spaced out as it says, across hooks; waiting for a turn
/// stops when the hook is cancelled.
#[test]
fn requests_to_a_limited_host_wait_their_turns() {
    let server = Server::new(|_, _| ok("x"));
    let f = fixture(&talking(10.0), &[], Options::default());
    let started = Instant::now();
    for _ in 0..3 {
        assert_eq!(f.fetch(json!({ "url": server.url.clone() }))["result"]["status"], 200);
    }
    assert!(started.elapsed() >= Duration::from_millis(200), "{:?}", started.elapsed());

    let f = fixture(&talking(1.0 / 3600.0), &[], Options::default());
    f.fetch(json!({ "url": server.url.clone() }));
    let cancel = locallibrary::progress::CancelToken::new();
    let cancelling = cancel.clone();
    let waiting = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        cancelling.cancel();
    });
    let params = json!({ "fetch": { "url": server.url.clone() } });
    let CommandOutput::Headless(answer) =
        f.command("echo", params, &Ctx::new(&locallibrary::progress::NoProgress, cancel)).unwrap()
    else {
        panic!("headless")
    };
    assert_eq!(answer["error"]["data"]["kind"], "cancelled", "{answer}");
    waiting.join().unwrap();
    assert_eq!(server.got().len(), 4, "the second never went");
}

/// Plugins limiting one host take its turns between them: two plugins asking one server keep to
/// its limit together.
#[test]
fn plugins_limiting_one_host_share_its_turns() {
    let server = Server::new(|_, _| ok("x"));
    let second = talking(2.0).replace("id = \"echo\"", "id = \"second\"");
    let options = Options { test_loopback: true, secrets: Storage::File, ..Options::default() };
    let f = Fixture::new(|dir| vec![spec(dir, &talking(2.0), &[]), spec(dir, &second, &[])], options);
    let run = |plugin: &str, params: Value| {
        let ctx = Ctx::silent();
        match f.plugins.run_command(plugin, "echo", params, f.context(), Some(&f.store), &ctx, &|_| {}).unwrap() {
            CommandOutput::Headless(v) => v,
            view => panic!("a headless command answers JSON: {view:?}"),
        }
    };
    // Both running first, so only the turns take time.
    run("echo", json!({}));
    run("second", json!({}));
    let started = Instant::now();
    for plugin in ["echo", "second"] {
        assert_eq!(run(plugin, json!({ "fetch": { "url": server.url.clone() } }))["result"]["status"], 200);
    }
    assert!(started.elapsed() >= Duration::from_millis(500), "{:?}", started.elapsed());
}

/// Tests kept offline (`Options::test_offline`) reach nothing but this computer, whatever the
/// plugin may reach.
#[test]
fn tests_kept_offline_reach_only_this_computer() {
    let server = Server::new(|_, _| ok("here"));
    let manifest = talking(100.0).replacen("\"net:127.0.0.1\"", "\"net:127.0.0.1\", \"net:example.org\"", 1);
    let f = fixture(&manifest, &[], Options { test_offline: true, ..Options::default() });
    assert_eq!(f.fetch(json!({ "url": server.url.clone() }))["result"]["body"], "here");
    let answer = f.fetch(json!({ "url": "https://example.org/" }));
    assert_eq!(answer["error"]["data"]["kind"], "denied", "{answer}");
    assert_eq!(answer["error"]["message"], "example.org isn't on this computer, and tests stay offline");
}

/// A plugin that doesn't say it has the `net` feature can't fetch, and one whose manifest limits
/// what it fetches isn't hosted without it.
#[test]
fn only_a_plugin_with_the_net_feature_fetches() {
    let server = Server::new(|_, _| ok("x"));
    let f = fixture(&granting(&["net:127.0.0.1"]), &["--without", "net"], Options::default());
    let e = f.fetch(json!({ "url": server.url.clone() }));
    assert_eq!(e["error"]["message"], "\"net/fetch\" isn't a read a plugin can call");
    assert!(server.got().is_empty());
    let f = fixture(&talking(100.0), &["--without", "net"], Options::default());
    let e = failure(f.echo_params(json!({})).unwrap_err());
    assert_eq!(e, "the Echo plugin failed: it says it doesn't have the net feature, which its manifest needs");
}

/// For any request a plugin makes (to a host it may talk to or not, naming its secret in a header
/// or a query or not at all, into a file or not) and any answer (any status, a body of any size,
/// text or not, redirected to where it may go or not), the host answers it as its rules say: a
/// host it isn't granted is refused, then a secret that isn't set, then a redirect elsewhere; an
/// answer goes to the file asked for, else is answered up to `max_body` when it's text; and a
/// secret goes to the server only where it's named.
#[hegel::test(test_cases = 15, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn any_fetch_is_answered_as_the_rules_say(tc: hegel::TestCase) {
    use hegel::generators as gs;
    // `/<status>/<length>/<text or bin>`, perhaps first redirected (`/to/<host>/...`).
    let server = Server::new(|base, got| {
        let path = got.target.split('?').next().unwrap();
        if let Some(rest) = path.strip_prefix("/to/") {
            let (host, rest) = rest.split_once('/').unwrap();
            return redirect(format!("{}/{rest}", base.replace("127.0.0.1", host)));
        }
        let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        let (status, length) = (parts[0].parse().unwrap(), parts[1].parse().unwrap());
        let body = match parts[2] {
            "text" => vec![b'a'; length],
            _ => vec![0xff; length],
        };
        Reply { status, headers: Vec::new(), body }
    });
    let f = fixture(&talking(100.0), &[], Options { max_body: 32, ..Options::default() });
    let granted = tc.draw(gs::booleans());
    let set = tc.draw(gs::booleans());
    if set {
        f.plugins.set_setting("echo", &f.store, "token", Some(json!("s3cret"))).unwrap();
    }
    let named = tc.draw(gs::sampled_from(vec!["", "header", "query"]));
    let status = tc.draw(gs::sampled_from(vec![200u16, 404, 500]));
    let length = tc.draw(gs::integers::<usize>().max_value(64));
    let text = tc.draw(gs::booleans());
    let into = tc.draw(gs::booleans());
    let redirected = tc.draw(gs::sampled_from(vec!["", "127.0.0.1", "localhost"]));

    let host = if granted { "127.0.0.1" } else { "localhost" };
    let to = if redirected.is_empty() { String::new() } else { format!("/to/{redirected}") };
    let kind = if text { "text" } else { "bin" };
    let query = if named == "query" { "?key={secret:token}" } else { "" };
    let url = format!("{}{to}/{status}/{length}/{kind}{query}", server.url.replace("127.0.0.1", host));
    let mut fetch = json!({ "url": url });
    if named == "header" {
        fetch["headers"] = json!({ "X-Token": "{secret:token}" });
    }
    if into {
        fetch["into_blob"] = json!("answer");
    }
    let answer = f.fetch(fetch);
    let error = |kind: &str| {
        assert_eq!(answer["error"]["data"]["kind"], kind, "{answer}");
        answer["error"]["message"].as_str().unwrap().to_string()
    };
    if !granted {
        error("denied");
        assert!(server.got().is_empty());
        return;
    }
    if !named.is_empty() && !set {
        assert!(error("invalid").starts_with("its secret token isn't set"));
        assert!(server.got().is_empty());
        return;
    }
    let first = &server.got()[0];
    assert_eq!(first.headers.get("x-token").map(String::as_str), (named == "header").then_some("s3cret"));
    assert_eq!(first.target.ends_with("?key=s3cret"), named == "query", "{}", first.target);
    if redirected == "localhost" {
        assert!(error("external").contains("redirected to where it may not go"));
        return;
    }
    let body = if text { "a".repeat(length) } else { String::from_utf8_lossy(&vec![0xff; length]).into_owned() };
    match (into, length > 32, text) {
        (true, ..) => assert_eq!((&answer["read"], &answer["result"]["body"]), (&json!(body), &Value::Null)),
        (false, true, _) => assert_eq!(error("too_big"), "the answer is over 32 B: fetch it into_blob"),
        (false, false, false) if length > 0 => {
            assert_eq!(error("invalid"), "the answer isn't text: fetch it into_blob")
        }
        (false, ..) => assert_eq!(answer["result"]["body"], json!(body)),
    }
    if answer.get("result").is_some() {
        assert_eq!(answer["result"]["status"], status);
    }
}

/// What a request sends from its scratch folder is a file there and nothing else: not one with
/// another name (a hard link) nor a link out of it, which could be any file on this computer.
#[test]
fn a_request_sends_only_a_file_in_its_scratch_folder() {
    let server = Server::new(|_, got| ok(got.body.clone()));
    let f = fixture(&talking(100.0), &[], Options::default());
    let outside = f.store.root().join("outside.txt");
    std::fs::write(&outside, "private").unwrap();
    let send = |made: Value| {
        let mut p = json!({ "fetch": { "url": format!("{}/echo", server.url), "method": "PUT", "body_blob": "sent" } });
        p.as_object_mut().unwrap().extend(made.as_object().unwrap().clone());
        f.echo_params(p).unwrap()
    };
    let e = send(json!({ "hard_links": { "sent": outside } }));
    let said = e["error"]["message"].as_str().unwrap();
    assert!(said.ends_with("its file \"sent\" has another name (a hard link), which could be anywhere"), "{e}");
    #[cfg(unix)]
    {
        let e = send(json!({ "links": { "sent": outside } }));
        let said = e["error"]["message"].as_str().unwrap();
        assert!(said.ends_with("its file \"sent\" leads out of the scratch folder"), "{e}");
    }
    assert!(server.got().is_empty(), "nothing was sent");
    assert_eq!(send(json!({ "blobs": { "sent": b"mine" } }))["result"]["body"], "mine");
}

/// The echo plugin as `talking` declares it, talking to this computer by its name too
/// (`net:localhost`): a second server, for redirects to lead to.
fn talking_to_two() -> String {
    talking(100.0).replace(r#""net:127.0.0.1""#, r#""net:127.0.0.1", "net:localhost""#)
}

/// A server whose `/r/<status>/<same|other>/<rest>` redirects with that status to `/<rest>` on
/// this server, or on the other (`127.0.0.1` and `localhost`, the one the request didn't go to),
/// and whose `/end` answers the request's method and body.
fn redirecting() -> Server {
    Server::new(|base, got| {
        let path = got.target.split('?').next().unwrap();
        let Some(rest) = path.strip_prefix("/r/") else { return ok(format!("{} {}", got.method, got.body)) };
        let mut parts = rest.splitn(3, '/');
        let (status, to, rest) = (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap());
        let here = got.headers["host"].split(':').next().unwrap();
        let other = if here == "127.0.0.1" { "localhost" } else { "127.0.0.1" };
        let base = base.replace("127.0.0.1", if to == "other" { other } else { here });
        Reply {
            status: status.parse().unwrap(),
            headers: vec![("Location", format!("{base}/{rest}"))],
            body: Vec::new(),
        }
    })
}

/// A secret filled into a header goes no further than the server it was for: once a redirect
/// leads to another, that header and the credentials reqwest keeps from one (`Authorization`)
/// are left out, and no request says where it came from (`Referer`, with the secret in its
/// query). A body with a secret is never sent on to another server.
#[test]
fn a_secret_never_follows_a_redirect_to_another_server() {
    let server = redirecting();
    let f = fixture(&talking_to_two(), &[], Options::default());
    f.plugins.set_setting("echo", &f.store, "token", Some(json!("s3cret"))).unwrap();
    let url = |path: &str| format!("{}{path}?key={{secret:token}}", server.url);
    let headers = json!({ "X-Token": "{secret:token}", "Authorization": "Basic plain", "Accept": "*/*" });
    let answer = f.fetch(json!({ "url": url("/r/302/same/r/302/other/end"), "headers": headers }));
    assert_eq!(answer["result"]["body"], "GET ", "{answer}");
    let got = server.got();
    let has = |i: usize, name: &str| got[i].headers.contains_key(name);
    assert_eq!(
        [0, 1, 2].map(|i| (has(i, "x-token"), has(i, "authorization"))),
        [(true, true), (true, true), (false, false)]
    );
    assert!((0..3).all(|i| has(i, "accept") && !has(i, "referer")), "{got:?}");
    assert!(got[0].target.ends_with("?key=s3cret") && !got[1].target.contains("s3cret"));

    let e = f.fetch(json!({ "url": url("/r/307/other/end"), "method": "POST", "body": "t={secret:token}" }));
    assert_eq!(e["error"]["data"]["kind"], "external", "{e}");
    assert!(e["error"]["message"].as_str().unwrap().ends_with("which its body's secret isn't for"), "{e}");
    assert_eq!(server.got().len(), 4, "the other server never had it");
    // A 303 asks again without the body, so with nothing of it.
    let answer = f.fetch(json!({ "url": url("/r/303/other/end"), "method": "POST", "body": "t={secret:token}" }));
    assert_eq!(answer["result"]["body"], "GET ", "{answer}");
}

/// For any chain of redirects (of each kind, to the same server or the other the plugin may talk
/// to) and any request through it (GET or POST, its secret in a header, the query, the body or
/// nowhere, with a plain `Authorization` too), each request the host sends has the secret's header
/// and `Authorization` only until a redirect leads to another server, the body (and the method)
/// until a 301, 302 or 303 asks again with GET, the secret in its query only at first, and no
/// `Referer`; a body with the secret is never sent to another server, and the fetch fails there.
#[hegel::test(test_cases = 20, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_redirect_takes_a_secret_no_further_than_its_server(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let server = redirecting();
    let f = fixture(&talking_to_two(), &[], Options::default());
    f.plugins.set_setting("echo", &f.store, "token", Some(json!("s3cret"))).unwrap();
    let statuses = vec![301u16, 302, 303, 307, 308];
    let hops: Vec<(u16, bool)> = tc.draw(gs::vecs(gs::tuples!(gs::sampled_from(statuses), gs::booleans())).max_size(3));
    let post = tc.draw(gs::booleans());
    let named = tc.draw(gs::sampled_from(vec!["", "header", "query", "body"]));
    let path: String =
        hops.iter().map(|(status, other)| format!("/r/{status}/{}", ["same", "other"][*other as usize])).collect();
    let query = if named == "query" { "?key={secret:token}" } else { "" };
    let mut fetch = json!({
        "url": format!("{}{}/end{query}", server.url, path),
        "method": if post { "POST" } else { "GET" },
        "headers": { "Authorization": "Basic plain" },
    });
    if named == "header" {
        fetch["headers"]["X-Token"] = json!("{secret:token}");
    }
    if post {
        fetch["body"] = json!(if named == "body" { "t={secret:token}" } else { "plain" });
    }
    let answer = f.fetch(fetch);

    // The model: what each request has (method, the secret's header, Authorization, a body).
    let (mut method, mut body, mut crossed, mut refused) = (if post { "POST" } else { "GET" }, post, false, false);
    let mut sent = vec![(method, named == "header", true, body)];
    for &(status, other) in &hops {
        if (301..=303).contains(&status) {
            (method, body) = ("GET", false);
        }
        if other && body && named == "body" {
            refused = true;
            break;
        }
        crossed |= other;
        sent.push((method, named == "header" && !crossed, !crossed, body));
    }
    let got = server.got();
    let had: Vec<_> = (got.iter())
        .map(|g| {
            (
                g.method.as_str(),
                g.headers.contains_key("x-token"),
                g.headers.contains_key("authorization"),
                !g.body.is_empty(),
            )
        })
        .collect();
    assert_eq!(had, sent, "{hops:?} {answer}");
    assert!(got.iter().all(|g| !g.headers.contains_key("referer")));
    assert!(got.iter().enumerate().all(|(i, g)| g.target.contains("s3cret") == (i == 0 && named == "query")));
    match refused {
        true => assert!(answer["error"]["message"].as_str().unwrap().ends_with("which its body's secret isn't for")),
        false => {
            // The last server says back the body it got, the secret filled in.
            let got = match (body, named) {
                (false, _) => "",
                (true, "body") => "t=s3cret",
                (true, _) => "plain",
            };
            assert_eq!(answer["result"]["body"], format!("{method} {got}"), "{answer}")
        }
    }
}

/// A mistake a plugin can make in a request.
#[derive(Debug, Clone, Copy)]
enum Mistake {
    NoAddress,
    NotGranted,
    Method,
    HeaderName,
    HeaderValue,
    TwoBodies,
    Unclosed,
    NotASecret,
    NotSet,
    SendsNothing,
    SendsOutside,
    WritesOutside,
    WritesOver,
    RangeAlone,
    PastTheEnd,
    FormAndBody,
    ValueAndFile,
    AccountInForm,
    NotAType,
}

/// For any request a plugin gets wrong (an address that isn't one or that it may not reach, a
/// method or header that can't be sent, two bodies, a placeholder that isn't closed or names none
/// of its secrets or one that isn't set, a file to send that isn't there or one to send or write
/// outside its scratch folder, a file to write that's there already, a range of no file or past
/// its file's end, a form with a body, a form's part that's both text and a file or names an
/// account, or a file part's type that isn't one), the host refuses it saying why, wherever in
/// the request the mistake is, and sends nothing.
#[hegel::test(test_cases = 30, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_request_made_wrong_is_refused_saying_why(tc: hegel::TestCase) {
    use Mistake::*;
    use hegel::generators as gs;
    let server = Server::new(|_, _| ok("x"));
    let f = fixture(&talking(100.0), &[], Options::default());
    let mistake = tc.draw(gs::sampled_from(vec![
        NoAddress,
        NotGranted,
        Method,
        HeaderName,
        HeaderValue,
        TwoBodies,
        Unclosed,
        NotASecret,
        NotSet,
        SendsNothing,
        SendsOutside,
        WritesOutside,
        WritesOver,
        RangeAlone,
        PastTheEnd,
        FormAndBody,
        ValueAndFile,
        AccountInForm,
        NotAType,
    ]));
    // Where a placeholder goes: a header, the query or the body.
    let place = tc.draw(gs::sampled_from(vec!["header", "query", "body"]));
    let placed = |text: &str| match place {
        "header" => json!({ "url": server.url.clone(), "headers": { "X": text } }),
        "query" => json!({ "url": format!("{}/?k={text}", server.url) }),
        _ => json!({ "url": server.url.clone(), "method": "POST", "body": text }),
    };
    let elsewhere = f.store.root().join("elsewhere").display().to_string();
    let (mut params, kind, said) = match mistake {
        NoAddress => (json!({ "url": "no address" }), "invalid", "no address isn't a web address".to_string()),
        NotGranted => (json!({ "url": "https://example.org/" }), "denied", "example.org isn't a server".into()),
        Method => (json!({ "url": server.url, "method": "NO WAY" }), "invalid", "NO WAY isn't an HTTP method".into()),
        HeaderName => {
            (json!({ "url": server.url, "headers": { "a b": "" } }), "invalid", "isn't a header's name".into())
        }
        HeaderValue => {
            (json!({ "url": server.url, "headers": { "X": "a\nb" } }), "invalid", "header's value can't be sent".into())
        }
        TwoBodies => (json!({ "url": server.url, "body": "a", "body_blob": "b" }), "invalid", "not both".into()),
        Unclosed => (placed("{secret:token"), "invalid", "placeholder isn't closed".into()),
        NotASecret => (placed("{secret:contact}"), "invalid", "\"contact\" isn't one of its secret settings".into()),
        NotSet => (placed("{secret:token}"), "invalid", "its secret token isn't set".into()),
        SendsNothing => (json!({ "url": server.url, "body_blob": "none" }), "invalid", "isn't there".into()),
        SendsOutside => {
            (json!({ "url": server.url, "body_blob": elsewhere }), "invalid", "isn't in the scratch".into())
        }
        WritesOutside => {
            (json!({ "url": server.url, "into_blob": elsewhere }), "invalid", "isn't in the scratch".into())
        }
        WritesOver => (json!({ "url": server.url, "into_blob": "held" }), "invalid", "exists".into()),
        RangeAlone => {
            let range = json!({ "offset": 1 });
            (json!({ "url": server.url, "body": "a", "body_range": range }), "invalid", "of a body_blob".into())
        }
        PastTheEnd => {
            let range = json!({ "offset": 2, "length": 3 });
            let said = "3 bytes from 2 go past its end, at 4".into();
            (json!({ "url": server.url, "body_blob": "held", "body_range": range }), "invalid", said)
        }
        FormAndBody => {
            let form = json!([{ "name": "a", "value": "b" }]);
            (
                json!({ "url": server.url, "body": "a", "multipart": form }),
                "invalid",
                "a multipart form or a body".into(),
            )
        }
        ValueAndFile => {
            let form = json!([{ "name": "a", "value": "b", "file": "held" }]);
            (json!({ "url": server.url, "multipart": form }), "invalid", "has a value or a file".into())
        }
        AccountInForm => {
            let form = json!([{ "name": "a", "value": "{account:main}" }]);
            (json!({ "url": server.url, "multipart": form }), "invalid", "only in the Authorization header".into())
        }
        NotAType => {
            let form = json!([{ "name": "a", "file": "held", "content_type": "not a type" }]);
            (json!({ "url": server.url, "multipart": form }), "invalid", "\"not a type\" isn't a content type".into())
        }
    };
    params = json!({ "fetch": params, "blobs": { "held": b"mine" } });
    let e = f.echo_params(params).unwrap();
    assert_eq!(e["error"]["data"]["kind"], kind, "{mistake:?}: {e}");
    let message = e["error"]["message"].as_str().unwrap();
    assert!(message.contains(&said), "{mistake:?}: {message}");
    assert!(server.got().is_empty(), "{mistake:?}: nothing was sent");
}

/// A file of `size` letters, `abc...zab...`, so a part of it shows where it's from.
pub(super) fn letters(size: usize) -> Vec<u8> {
    (0..size).map(|i| b'a' + (i % 26) as u8).collect()
}

/// For any file in the request's scratch folder and any part of it (from any offset, of any
/// length or to its end), the host sends those bytes and no others, saying how many; a part
/// going past the file's end is refused, and nothing is sent.
#[hegel::test(test_cases = 20, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_request_sends_the_part_of_a_file_its_range_says(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let server = Server::new(|_, got| ok(got.body.clone()));
    let f = fixture(&talking(100.0), &[], Options::default());
    let size = tc.draw(gs::integers::<u64>().max_value(40));
    let offset = tc.draw(gs::integers::<u64>().max_value(48));
    let length = tc.draw(gs::booleans()).then(|| tc.draw(gs::integers::<u64>().max_value(48)));
    let mut range = json!({ "offset": offset });
    if let Some(length) = length {
        range["length"] = json!(length);
    }
    let fetch =
        json!({ "url": format!("{}/part", server.url), "method": "PUT", "body_blob": "sent", "body_range": range });
    let answer = f.echo_params(json!({ "fetch": fetch, "blobs": { "sent": letters(size as usize) } })).unwrap();
    let end = length.map_or(size.max(offset), |length| offset + length);
    if end > size {
        assert_eq!(answer["error"]["data"]["kind"], "invalid", "{answer}");
        assert!(answer["error"]["message"].as_str().unwrap().contains("go past its end"), "{answer}");
        assert!(server.got().is_empty(), "nothing was sent");
        return;
    }
    let part = String::from_utf8(letters(size as usize)[offset as usize..end as usize].to_vec()).unwrap();
    assert_eq!(answer["result"]["body"], part, "{answer}");
    assert_eq!(server.got()[0].headers["content-length"], (end - offset).to_string());
}

/// A file's parts as an upload in parts sends them: from its start, the next, and the last one
/// shorter (to its end, or saying how long), and none past its end; one at its end is empty.
#[test]
fn a_file_is_sent_in_parts_to_its_end() {
    let server = Server::new(|_, got| ok(got.body.clone()));
    let f = fixture(&talking(100.0), &[], Options::default());
    let send = |range: Value| {
        let fetch =
            json!({ "url": format!("{}/part", server.url), "method": "PUT", "body_blob": "sent", "body_range": range });
        let answer = f.echo_params(json!({ "fetch": fetch, "blobs": { "sent": b"0123456789" } })).unwrap();
        match answer["result"]["body"].as_str() {
            Some(body) => Ok(body.to_string()),
            None => Err(answer["error"]["message"].as_str().unwrap().to_string()),
        }
    };
    assert_eq!(send(json!({ "length": 4 })).unwrap(), "0123");
    assert_eq!(send(json!({ "offset": 4, "length": 4 })).unwrap(), "4567");
    assert_eq!(send(json!({ "offset": 8 })).unwrap(), "89");
    assert_eq!(send(json!({ "offset": 8, "length": 2 })).unwrap(), "89");
    let e = send(json!({ "offset": 8, "length": 4 })).unwrap_err();
    assert!(e.ends_with("4 bytes from 8 go past its end, at 10"), "{e}");
    assert_eq!(send(json!({ "offset": 10 })).unwrap(), "");
    assert!(send(json!({ "offset": 11 })).unwrap_err().ends_with("0 bytes from 11 go past its end, at 10"));
    assert_eq!(server.got().len(), 5, "none went past the end");
}

/// For any form (text parts, one naming its secret or not, and a file, or part of one, in any
/// place, or not when it has text), the host sends it as `multipart/form-data` with a boundary of its own (the type the
/// plugin said left out), each part as it's named, saying how long it all is, and the secret
/// filled in.
#[hegel::test(test_cases = 20, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_form_is_sent_as_multipart_with_its_file(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let server = Server::new(|_, got| ok(got.body.clone()));
    let f = fixture(&talking(100.0), &[], Options::default());
    f.plugins.set_setting("echo", &f.store, "token", Some(json!("s3cret"))).unwrap();
    let texts: Vec<(String, String)> = tc.draw(
        gs::vecs(gs::tuples!(
            gs::from_regex("[a-z]{1,6}").fullmatch(true),
            hegel::one_of!(gs::from_regex("[a-z ]{0,8}").fullmatch(true), gs::just("{secret:token}".to_string()))
        ))
        .max_size(3),
    );
    let mut parts: Vec<Value> = texts.iter().map(|(name, value)| json!({ "name": name, "value": value })).collect();
    // Each part as it's sent: its name, what its head says besides, and its value.
    let mut sent: Vec<(String, &str, String)> =
        texts.iter().map(|(name, value)| (name.clone(), "", value.replace("{secret:token}", "s3cret"))).collect();
    // A form has a part at least.
    if texts.is_empty() || tc.draw(gs::booleans()) {
        let size = tc.draw(gs::integers::<usize>().max_value(30));
        let offset = tc.draw(gs::integers::<usize>().max_value(size));
        let at = tc.draw(gs::integers::<usize>().max_value(parts.len()));
        let mix = String::from_utf8(letters(size)[offset..].to_vec()).unwrap();
        let mut file = json!({ "name": "mp3", "file": "mix", "range": { "offset": offset } });
        // Its file's name and type, or neither.
        let head = match tc.draw(gs::booleans()) {
            true => {
                file["filename"] = json!("Friday.mp3");
                file["content_type"] = json!("audio/mpeg");
                "; filename=\"Friday.mp3\"\r\nContent-Type: audio/mpeg"
            }
            false => "",
        };
        parts.insert(at, file);
        sent.insert(at, ("mp3".into(), head, mix));
        let headers = json!({ "Content-Type": "text/plain" });
        let fetch = json!({ "url": server.url.clone(), "method": "POST", "headers": headers, "multipart": parts });
        f.echo_params(json!({ "fetch": fetch, "blobs": { "mix": letters(size) } })).unwrap();
    } else {
        f.echo_params(json!({ "fetch": { "url": server.url.clone(), "method": "POST", "multipart": parts } })).unwrap();
    }
    let got = &server.got()[0];
    let boundary = got.headers["content-type"].strip_prefix("multipart/form-data; boundary=").unwrap();
    let mut body = String::new();
    for (name, head, value) in &sent {
        body.push_str(&format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"{head}\r\n\r\n{value}\r\n"
        ));
    }
    body.push_str(&format!("--{boundary}--\r\n"));
    assert_eq!(got.body, body);
    assert_eq!(got.headers["content-length"], body.len().to_string());
}

/// A file a request sends is read again for a redirect that asks for the body again (307 and
/// 308), and isn't for one that asks again without it (303).
#[test]
fn a_file_is_sent_again_where_a_redirect_asks_for_the_body() {
    let server = redirecting();
    let f = fixture(&talking_to_two(), &[], Options::default());
    let send = |path: &str| {
        let range = json!({ "offset": 8 });
        let url = format!("{}{path}", server.url);
        let fetch = json!({ "url": url, "method": "PUT", "body_blob": "sent", "body_range": range });
        f.echo_params(json!({ "fetch": fetch, "blobs": { "sent": b"part of a mix" } })).unwrap()["result"]["body"]
            .clone()
    };
    assert_eq!(send("/r/307/same/r/308/other/end"), "PUT a mix");
    let got = server.got();
    assert_eq!(got.iter().map(|g| g.body.as_str()).collect::<Vec<_>>(), ["a mix"; 3]);
    assert_eq!(send("/r/303/same/end"), "GET ");
    assert_eq!(server.got()[3].body, "a mix", "sent to the first");
}
