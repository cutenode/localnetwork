use std::io::{BufRead, PipeReader, PipeWriter, pipe};
use std::time::Duration;

use hegel::TestCase;
use hegel::generators::{self as gs, Generator};

use super::*;
use crate::code;

/// The other end of a [`Peer`], as raw lines.
struct Raw {
    to_peer: Option<PipeWriter>,
    from_peer: BufReader<PipeReader>,
}

impl Raw {
    fn send(&mut self, line: &str) {
        let to = self.to_peer.as_mut().expect("still open");
        to.write_all(line.as_bytes()).unwrap();
        to.write_all(b"\n").unwrap();
    }

    fn recv(&mut self) -> Value {
        let mut line = String::new();
        self.from_peer.read_line(&mut line).unwrap();
        assert!(line.ends_with('\n'), "one message per line: {line:?}");
        serde_json::from_str(&line).unwrap()
    }

    /// Closes the peer's input.
    fn close(&mut self) {
        self.to_peer = None;
    }
}

fn connect() -> (Peer, Receiver<Incoming>, Raw) {
    let (peer_in, to_peer) = pipe().unwrap();
    let (from_peer, peer_out) = pipe().unwrap();
    let (peer, incoming) = Peer::new(peer_in, peer_out);
    (peer, incoming, Raw { to_peer: Some(to_peer), from_peer: BufReader::new(from_peer) })
}

fn next(incoming: &Receiver<Incoming>) -> Incoming {
    incoming.recv_timeout(Duration::from_secs(10)).expect("a message")
}

fn request(incoming: &Receiver<Incoming>) -> Request {
    match next(incoming) {
        Incoming::Request(r) => r,
        other => panic!("not a request: {other:?}"),
    }
}

#[test]
fn a_request_gets_its_response_or_error() {
    let (peer, _incoming, mut raw) = connect();
    let p = peer.clone();
    let asked = std::thread::spawn(move || p.request::<_, Vec<u32>>("tracks.query", &json!({ "drive": "A" })));
    let req = raw.recv();
    assert_eq!(req["jsonrpc"], "2.0");
    assert_eq!(req["method"], "tracks.query");
    assert_eq!(req["params"], json!({ "drive": "A" }));
    raw.send(&json!({ "jsonrpc": "2.0", "id": req["id"], "result": [1, 2] }).to_string());
    assert_eq!(asked.join().unwrap().unwrap(), [1, 2]);

    let p = peer.clone();
    let asked = std::thread::spawn(move || p.request::<_, Value>("drive.info", &json!({})));
    let req = raw.recv();
    let e = RpcError::failed("not_found", "there's no drive A");
    raw.send(&json!({ "jsonrpc": "2.0", "id": req["id"], "error": e }).to_string());
    assert_eq!(asked.join().unwrap().unwrap_err(), e);

    // A plain JSON-RPC error, without data, keeps its code and message.
    let p = peer.clone();
    let asked = std::thread::spawn(move || p.request::<_, Value>("drive.info", &json!({})));
    let req = raw.recv();
    raw.send(&json!({ "jsonrpc": "2.0", "id": req["id"], "error": { "code": -32601, "message": "nope" } }).to_string());
    let e = asked.join().unwrap().unwrap_err();
    assert_eq!((e.code, e.message.as_str(), e.kind()), (code::METHOD_NOT_FOUND, "nope", None));

    // A result of the wrong shape is the other end's bug.
    let p = peer.clone();
    let asked = std::thread::spawn(move || p.request::<_, Vec<u32>>("drive.info", &json!({})));
    let req = raw.recv();
    raw.send(&json!({ "jsonrpc": "2.0", "id": req["id"], "result": "no" }).to_string());
    let e = asked.join().unwrap().unwrap_err();
    assert_eq!((e.code, e.kind()), (code::INTERNAL, Some("internal")));
    assert!(e.message.starts_with("unexpected result: "), "{e}");

    // So is an error that isn't a JSON-RPC error object.
    let asked = std::thread::spawn(move || peer.request::<_, Value>("drive.info", &json!({})));
    let req = raw.recv();
    raw.send(&json!({ "jsonrpc": "2.0", "id": req["id"], "error": "oops" }).to_string());
    let e = asked.join().unwrap().unwrap_err();
    assert!(e.message.starts_with("an error response that isn't one: "), "{e}");
}

#[test]
fn concurrent_requests_each_get_their_own_response() {
    let (peer, _incoming, mut raw) = connect();
    let asked: Vec<_> = (0..8u32)
        .map(|n| {
            let peer = peer.clone();
            std::thread::spawn(move || (n, peer.request::<_, u32>("double", &json!({ "n": n }))))
        })
        .collect();
    let mut requests: Vec<Value> = (0..8).map(|_| raw.recv()).collect();
    let mut ids: Vec<_> = requests.iter().map(|r| r["id"].as_u64().unwrap()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 8, "every request has an id of its own");
    // Answered in another order than asked.
    requests.reverse();
    for r in requests {
        let n = r["params"]["n"].as_u64().unwrap();
        raw.send(&json!({ "jsonrpc": "2.0", "id": r["id"], "result": n * 2 }).to_string());
    }
    for a in asked {
        let (n, doubled) = a.join().unwrap();
        assert_eq!(doubled.unwrap(), n * 2);
    }
}

#[test]
fn the_other_ends_requests_and_notifications_reach_the_owner() {
    let (peer, incoming, mut raw) = connect();
    raw.send(r#"{"jsonrpc":"2.0","id":"a","method":"track/analyze","params":{"analyzer":"size"}}"#);
    let req = request(&incoming);
    assert_eq!((req.id.clone(), req.method.as_str()), (json!("a"), "track/analyze"));
    assert_eq!(req.params, json!({ "analyzer": "size" }));
    peer.respond(&req.id, Ok(json!({ "fields": {} })));
    assert_eq!(raw.recv(), json!({ "jsonrpc": "2.0", "id": "a", "result": { "fields": {} } }));

    // Params default to an object.
    raw.send(r#"{"jsonrpc":"2.0","method":"progress"}"#);
    match next(&incoming) {
        Incoming::Notification { method, params } => assert_eq!((method.as_str(), params), ("progress", json!({}))),
        other => panic!("{other:?}"),
    }
    raw.send(r#"{"jsonrpc":"2.0","id":7,"method":"describe","params":null}"#);
    let req = request(&incoming);
    assert_eq!((req.id, req.params), (json!(7), json!({})));
}

#[test]
fn cancel_reaches_the_request_it_names() {
    let (peer, incoming, mut raw) = connect();
    raw.send(r#"{"jsonrpc":"2.0","id":1,"method":"command/run"}"#);
    raw.send(r#"{"jsonrpc":"2.0","id":2,"method":"command/run"}"#);
    let (one, two) = (request(&incoming), request(&incoming));
    raw.send(r#"{"jsonrpc":"2.0","method":"$/cancel","params":{"id":2}}"#);
    // Neither of these names a running request.
    raw.send(r#"{"jsonrpc":"2.0","method":"$/cancel","params":{"id":3}}"#);
    raw.send(r#"{"jsonrpc":"2.0","method":"$/cancel","params":{}}"#);
    // Once the cancels have been read: the peer handles them in order.
    raw.send(r#"{"jsonrpc":"2.0","method":"ping"}"#);
    assert!(matches!(next(&incoming), Incoming::Notification { .. }));
    assert!(two.cancel.is_cancelled());
    assert!(!one.cancel.is_cancelled());

    // A request answered is no longer running: its id is free again.
    peer.respond(&one.id, Ok(Value::Null));
    raw.recv();
    raw.send(r#"{"jsonrpc":"2.0","id":1,"method":"command/run"}"#);
    assert_eq!(request(&incoming).id, json!(1));
}

#[test]
fn an_id_a_running_request_has_is_refused() {
    let (_peer, incoming, mut raw) = connect();
    raw.send(r#"{"jsonrpc":"2.0","id":"x","method":"command/run"}"#);
    request(&incoming);
    raw.send(r#"{"jsonrpc":"2.0","id":"x","method":"op/plan"}"#);
    let e = raw.recv();
    assert_eq!((e["id"].clone(), e["error"]["code"].clone()), (json!("x"), json!(code::INVALID_REQUEST)));
    assert_eq!(e["error"]["message"], "request id \"x\" is already in use by a running request");
}

/// A request sent while answering one that's cancelled, or answered, isn't sent.
#[test]
fn a_request_within_a_cancelled_one_isnt_sent() {
    let (peer, incoming, mut raw) = connect();
    raw.send(r#"{"jsonrpc":"2.0","id":"hook","method":"command/run"}"#);
    let hook = request(&incoming);
    hook.cancel.cancel();
    let e = peer.send_request_within(&hook.id, "tracks.query", &json!({})).err();
    assert_eq!(e, Some(RpcError::cancelled()));
    let e = peer.send_request_within(&json!("other"), "tracks.query", &json!({})).err();
    assert_eq!(e, Some(RpcError::cancelled()));
    // Nothing was sent: the next line is this one.
    peer.notify("last", &json!({}));
    assert_eq!(raw.recv()["method"], "last");
}

#[test]
fn pending_requests_can_be_cancelled() {
    let (peer, _incoming, mut raw) = connect();
    let pending = peer.send_request("command/run", &json!({})).unwrap();
    let req = raw.recv();
    assert_eq!(req["id"], pending.id());
    pending.cancel();
    assert_eq!(raw.recv(), json!({ "jsonrpc": "2.0", "method": "$/cancel", "params": { "id": pending.id() } }));
    raw.send(&json!({ "jsonrpc": "2.0", "id": pending.id(), "error": RpcError::cancelled() }).to_string());
    assert_eq!(pending.wait::<Value>().unwrap_err().kind(), Some("cancelled"));
}

/// A message split over lines (pretty-printed JSON) is lines that aren't messages: each gets a
/// parse error, and the peer carries on.
#[test]
fn a_message_with_a_newline_in_it_is_refused() {
    let (_peer, incoming, mut raw) = connect();
    raw.send("{\n\"jsonrpc\": \"2.0\",\n\"method\": \"progress\"\n}");
    for _ in 0..4 {
        let e = raw.recv();
        assert_eq!((e["id"].clone(), e["error"]["code"].clone()), (Value::Null, json!(code::PARSE_ERROR)));
        assert!(e["error"]["message"].as_str().unwrap().starts_with("parse error: "));
    }
    raw.send("");
    raw.send(r#"{"jsonrpc":"2.0","method":"after"}"#);
    assert!(matches!(next(&incoming), Incoming::Notification { method, .. } if method == "after"));
}

#[test]
fn messages_that_arent_requests_responses_or_notifications_are_refused() {
    let (_peer, _incoming, mut raw) = connect();
    let refused = |raw: &mut Raw, line: &str| {
        raw.send(line);
        let e = raw.recv();
        assert_eq!(e["error"]["code"], code::INVALID_REQUEST, "{line}");
        (e["id"].clone(), e["error"]["message"].as_str().unwrap().to_string())
    };
    assert_eq!(refused(&mut raw, "[1, 2]"), (Value::Null, "a message is a JSON object".into()));
    let bad_id = r#"{"jsonrpc":"2.0","id":{},"method":"m"}"#;
    assert_eq!(refused(&mut raw, bad_id), (Value::Null, "id must be a string or a number".into()));
}

/// A response is never answered, even a bad one: whoever waits for it is told instead.
#[test]
fn a_bad_response_fails_its_request_unanswered() {
    let (peer, incoming, mut raw) = connect();
    let asked = |peer: &Peer| {
        let p = peer.clone();
        std::thread::spawn(move || p.request::<_, Value>("m", &json!({})))
    };
    for bad in [r#"{"jsonrpc":"2.0","id":ID}"#, r#"{"jsonrpc":"2.0","id":ID,"result":1,"error":{}}"#] {
        let asking = asked(&peer);
        let id = raw.recv()["id"].clone();
        raw.send(&bad.replace("ID", &id.to_string()));
        let e = asking.join().unwrap().unwrap_err();
        assert_eq!(e.message, "a response with both a result and an error, or neither", "{bad}");
    }
    // A response that isn't JSON, but says which request it answers.
    let asking = asked(&peer);
    let id = raw.recv()["id"].clone();
    raw.send(&format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{"name":"a\"}}"}}"#));
    assert_eq!(asking.join().unwrap().unwrap_err().code, code::PARSE_ERROR);
    // None of them was answered: the next line the other end gets is this one's.
    raw.send(r#"{"jsonrpc":"2.0","method":"after"}"#);
    assert!(matches!(next(&incoming), Incoming::Notification { .. }));
    peer.notify("last", &json!({}));
    assert_eq!(raw.recv()["method"], "last");
}

#[test]
fn a_response_id_is_found_in_a_line_that_isnt_json() {
    for (line, id) in [
        (r#"{"jsonrpc":"2.0","id":12,"result":{"#, Some(12)),
        (r#"{"result":{"id":3,"x":"\"id\":4"},"id" : 5,"#, Some(5)),
        (r#"{"error":{"code":1},"id":7"#, Some(7)),
        (r#"{"id":7,"method":"m","params":{"#, None),
        (r#"{"id":"seven","result":"#, None),
        (r#"{"result":[{"id":1}],"#, None),
        (r#"{"id":"#, None),
        (r#"{"x":"never ends"#, None),
        // Cut short on a string's backslash (hegel's example for
        // `a_response_id_is_found_in_any_line_cut_short`).
        (r#"{"jsonrpc":"2.0","id":0,"result":"\"#, Some(0)),
        ("not json at all", None),
    ] {
        assert_eq!(response_id(line.as_bytes()), id, "{line}");
    }
}

/// A response nobody waits for (an id never sent, or the other end's parse error) is dropped.
#[test]
fn responses_nobody_waits_for_are_dropped() {
    let (peer, incoming, mut raw) = connect();
    raw.send(r#"{"jsonrpc":"2.0","id":99,"result":1}"#);
    raw.send(
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"parse error","data":{"kind":"parse_error"}}}"#,
    );
    raw.send(r#"{"jsonrpc":"2.0","method":"after"}"#);
    assert!(matches!(next(&incoming), Incoming::Notification { .. }));
    // The peer still works.
    let p = peer.clone();
    let asked = std::thread::spawn(move || p.request::<_, u32>("m", &json!({})));
    let req = raw.recv();
    raw.send(&json!({ "jsonrpc": "2.0", "id": req["id"], "result": 3 }).to_string());
    assert_eq!(asked.join().unwrap().unwrap(), 3);
}

#[test]
fn a_line_too_long_is_skipped() {
    let (_peer, incoming, mut raw) = connect();
    let mut to = raw.to_peer.take().unwrap();
    let writer = std::thread::spawn(move || {
        to.write_all(&vec![b'a'; MAX_LINE + 1]).unwrap();
        to.write_all(b"\n{\"jsonrpc\":\"2.0\",\"method\":\"after\"}\n").unwrap();
        to
    });
    let e = raw.recv();
    assert_eq!(e["error"]["code"], code::PARSE_ERROR);
    assert_eq!(e["error"]["message"], "parse error: a line is longer than 16 MiB");
    assert!(matches!(next(&incoming), Incoming::Notification { method, .. } if method == "after"));
    drop(writer.join().unwrap());
}

#[test]
fn a_message_too_long_to_send_isnt_sent() {
    let (peer, incoming, mut raw) = connect();
    let huge = "a".repeat(MAX_LINE);
    let e = peer.request::<_, Value>("m", &json!({ "text": huge })).unwrap_err();
    assert_eq!(e.message, "a message is longer than 16 MiB");

    // A response too long becomes an error.
    raw.send(r#"{"jsonrpc":"2.0","id":1,"method":"command/run"}"#);
    let req = request(&incoming);
    peer.respond(&req.id, Ok(json!(huge)));
    let r = raw.recv();
    assert_eq!((r["id"].clone(), r["error"]["code"].clone()), (json!(1), json!(code::INTERNAL)));
}

#[test]
fn closing_the_input_ends_everything_in_flight() {
    let (peer, incoming, mut raw) = connect();
    raw.send(r#"{"jsonrpc":"2.0","id":1,"method":"command/run"}"#);
    let running = request(&incoming);
    let p = peer.clone();
    let asked = std::thread::spawn(move || p.request::<_, Value>("tracks.query", &json!({})));
    raw.recv();
    raw.close();
    assert_eq!(asked.join().unwrap().unwrap_err(), RpcError::closed());
    assert!(incoming.recv_timeout(Duration::from_secs(10)).is_err(), "the owner's receiver closes");
    assert!(running.cancel.is_cancelled());
    assert_eq!(peer.send_request("m", &json!({})).err(), Some(RpcError::closed()));
}

#[test]
fn a_closed_output_fails_requests() {
    let (peer, _incoming, raw) = connect();
    drop(raw.from_peer);
    assert_eq!(peer.request::<_, Value>("m", &json!({})).unwrap_err(), RpcError::closed());
    // Nothing to answer with: dropped.
    peer.respond(&json!(1), Ok(Value::Null));
    peer.notify("progress", &json!({}));
}

/// A hook's progress comes to it, before its response; progress about other requests still goes
/// to the owner.
#[test]
fn a_reporting_request_gets_its_progress_before_its_response() {
    let (peer, incoming, mut raw) = connect();
    let pending = peer.send_request_reporting("command/run", &json!({})).unwrap();
    let id = raw.recv()["id"].clone();
    assert!(pending.wait_timeout::<Value>(Duration::from_millis(10)).is_none(), "no response yet");
    let warning = json!({ "event": "warning", "message": "slow" });
    raw.send(&json!({ "jsonrpc": "2.0", "method": "progress", "params": { "id": id, "event": warning } }).to_string());
    raw.send(&json!({ "jsonrpc": "2.0", "method": "progress", "params": { "id": 99, "event": warning } }).to_string());
    raw.send(&json!({ "jsonrpc": "2.0", "id": id, "result": 1 }).to_string());
    assert_eq!(pending.wait_timeout::<u32>(Duration::from_secs(10)).unwrap().unwrap(), 1);
    let reports: Vec<Value> = pending.progress().map(|r| serde_json::to_value(r).unwrap()).collect();
    assert_eq!(reports, std::slice::from_ref(&warning));
    match next(&incoming) {
        Incoming::Notification { params, .. } => assert_eq!(params["id"], 99),
        other => panic!("{other:?}"),
    }
    // Answered: waiting again finds the connection's end of it closed.
    assert_eq!(pending.wait_timeout::<u32>(Duration::ZERO).unwrap().unwrap_err().kind(), Some("closed"));

    // A request sent without asking for its progress leaves it to the owner.
    let pending = peer.send_request("command/run", &json!({})).unwrap();
    let id = raw.recv()["id"].clone();
    raw.send(&json!({ "jsonrpc": "2.0", "method": "progress", "params": { "id": id, "event": warning } }).to_string());
    assert!(matches!(next(&incoming), Incoming::Notification { .. }));
    assert_eq!(pending.progress().count(), 0);
}

/// Regression: the reader drops a response's sender only after handing the response over, so
/// waiting again straight after it came could time out instead of failing with `closed`.
#[test]
fn waiting_again_after_the_response_fails_even_before_the_sender_goes() {
    let (peer, _incoming, mut raw) = connect();
    let pending = peer.send_request("m", &json!({})).unwrap();
    raw.recv();
    // As the reader does, mid-way: the response handed over, its sender not yet dropped.
    let waiting = lock(&peer.0.waiting);
    waiting.as_ref().unwrap()[&pending.id()].response.send(Ok(json!(1))).unwrap();
    assert_eq!(pending.wait_timeout::<u32>(Duration::ZERO).unwrap().unwrap(), 1);
    assert_eq!(pending.wait_timeout::<u32>(Duration::ZERO).unwrap().unwrap_err().kind(), Some("closed"));
    drop(waiting);
}

/// What the other end gets wrong goes to the log a host gives.
#[test]
fn what_the_other_end_gets_wrong_is_logged() {
    let (peer_in, mut to_peer) = pipe().unwrap();
    let (_from_peer, peer_out) = pipe().unwrap();
    let logged = Arc::new(Mutex::new(Vec::new()));
    let log = logged.clone();
    let (_peer, incoming) = Peer::with_log(peer_in, peer_out, move |line| log.lock().unwrap().push(line.to_string()));
    to_peer
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":5,\"result\":1}\n{\"jsonrpc\":\"2.0\",\"method\":\"ping\"}\n")
        .unwrap();
    assert!(matches!(next(&incoming), Incoming::Notification { .. }), "read on");
    assert_eq!(*logged.lock().unwrap(), ["a response to 5, which no request is waiting for"]);
}

// Properties.

/// The log a peer writes to, as lines.
type Log = Arc<Mutex<Vec<String>>>;

/// [`connect`], with the peer's input wrapped in `wrap` and what it logs kept.
fn connect_logged<R: Read + Send + 'static>(
    wrap: impl FnOnce(PipeReader) -> R,
) -> (Peer, Receiver<Incoming>, Raw, Log) {
    let (peer_in, to_peer) = pipe().unwrap();
    let (from_peer, peer_out) = pipe().unwrap();
    let logged = Log::default();
    let log = logged.clone();
    let (peer, incoming) =
        Peer::with_log(wrap(peer_in), peer_out, move |line| log.lock().unwrap().push(line.to_string()));
    (peer, incoming, Raw { to_peer: Some(to_peer), from_peer: BufReader::new(from_peer) }, logged)
}

impl Raw {
    /// The peer's lines up to the notification `method`, which `peer` sends now: whatever the
    /// peer's reader answered before it.
    fn until(&mut self, peer: &Peer, method: &str) -> Vec<Value> {
        peer.notify(method, &json!({}));
        std::iter::from_fn(|| Some(self.recv())).take_while(|line| line["method"] != method).collect()
    }
}

/// The owner's messages up to the notification `method`, which `raw` sends now: whatever the
/// peer read before it.
fn owners_until(raw: &mut Raw, incoming: &Receiver<Incoming>, method: &str) -> Vec<Incoming> {
    raw.send(&json!({ "jsonrpc": "2.0", "method": method }).to_string());
    std::iter::from_fn(|| Some(next(incoming)))
        .take_while(|m| !matches!(m, Incoming::Notification { method: m, .. } if m == method))
        .collect()
}

/// Any JSON: strings with quotes, backslashes, braces and `"id":` in them, and objects with
/// `id` and `method` keys of their own.
#[hegel::test_helper]
fn json_value(tc: &TestCase, depth: u8) -> Value {
    let kinds = if depth == 0 { 4 } else { 6 };
    match tc.draw(gs::integers::<u8>().max_value(kinds)) {
        0 => Value::Null,
        1 => tc.draw(gs::booleans()).into(),
        2 => tc.draw(gs::integers::<i64>()).into(),
        3 => tc.draw(gs::floats::<f64>().allow_nan(false).allow_infinity(false)).into(),
        4 => match tc.draw(gs::booleans()) {
            true => tc.draw(gs::sampled_from(vec!["\"id\":4,", "\\", "{\"id\":", "}]", "\n", "method"])).into(),
            false => tc.draw(gs::text().max_size(6)).into(),
        },
        5 => (0..tc.draw(gs::integers::<usize>().max_value(3))).map(|_| json_value(tc, depth - 1)).collect(),
        _ => {
            let keys =
                tc.draw(gs::vecs(gs::sampled_from(vec!["id", "method", "result", "a"])).max_size(3).unique(true));
            Value::Object(keys.into_iter().map(|k| (k.to_string(), json_value(tc, depth - 1))).collect())
        }
    }
}

/// An id the other end might give a request: a few numbers and strings, so they repeat.
#[hegel::test_helper]
fn request_id(tc: &TestCase) -> Value {
    match tc.draw(gs::booleans()) {
        true => tc.draw(gs::integers::<u8>().max_value(3)).into(),
        false => tc.draw(gs::sampled_from(vec!["a", "b", ""])).into(),
    }
}

/// Params as the other end might send them: none, `null` (both `{}` for the owner) or any JSON.
#[hegel::test_helper]
fn sent_params(tc: &TestCase) -> (Option<Value>, Value) {
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => (None, json!({})),
        1 => (Some(Value::Null), json!({})),
        _ => {
            let v = json_value(tc, 2);
            let read = if v.is_null() { json!({}) } else { v.clone() };
            (Some(v), read)
        }
    }
}

/// What the owner should get.
#[derive(Debug, PartialEq)]
enum Owned {
    Request { id: Value, method: String, params: Value },
    Notification { method: String, params: Value },
}

/// Any lines the other end sends, good or bad, each get what JSON-RPC says they should: a request
/// or notification reaches the owner (once per running id), `$/cancel` cancels what it names, a
/// response nobody waits for is dropped and logged, a blank line is skipped, and the rest are
/// answered with a parse error or an invalid request, with a null id but for a request whose id
/// is in use. Nothing else is answered.
#[hegel::test(test_cases = 80)]
fn any_lines_get_what_json_rpc_says_they_should(tc: TestCase) {
    let (peer, incoming, mut raw, logged) = connect_logged(|r| r);
    let (mut owned, mut answers, mut logs) = (Vec::new(), Vec::new(), 0);
    let mut running: Vec<Value> = Vec::new();
    let mut cancelled: Vec<Value> = Vec::new();
    let kinds = vec![
        "blank",
        "not json",
        "cut short",
        "not an object",
        "bad id",
        "request",
        "notification",
        "cancel",
        "response",
    ];
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(12)) {
        let kind = tc.draw(gs::sampled_from(kinds.clone()));
        let mut message = Map::from_iter([("jsonrpc".to_string(), json!("2.0"))]);
        let line = match kind {
            "blank" => tc.draw(gs::text().alphabet(" \t\r").max_size(3)),
            "not json" => format!("}}{}", tc.draw(gs::text().max_size(8).filter(|s: &String| !s.contains('\n')))),
            "cut short" => {
                let whole =
                    json!({ "jsonrpc": "2.0", "id": tc.draw(gs::integers::<u32>()), "result": json_value(&tc, 2) })
                        .to_string();
                let cut = tc.draw(gs::integers::<usize>().min_value(1).max_value(whole.len() - 1));
                // On a character boundary.
                let cut = (0..=cut).rev().find(|i| whole.is_char_boundary(*i)).unwrap().max(1);
                whole[..cut].to_string()
            }
            "not an object" => {
                let v = tc.draw(gs::sampled_from(vec![json!(null), json!([1, 2]), json!("m"), json!(3), json!(true)]));
                v.to_string()
            }
            "bad id" => {
                let id = tc.draw(gs::sampled_from(vec![json!(true), json!([]), json!({ "n": 1 })]));
                json!({ "jsonrpc": "2.0", "id": id, "method": "m" }).to_string()
            }
            "request" | "notification" => {
                let method =
                    tc.draw(gs::sampled_from(vec!["command/run".to_string(), "progress".into(), "ping".into()]));
                let (sent, read) = sent_params(&tc);
                if kind == "request" {
                    message.insert("id".into(), request_id(&tc));
                }
                message.insert("method".into(), method.clone().into());
                if let Some(sent) = sent {
                    message.insert("params".into(), sent);
                }
                match message.get("id").cloned() {
                    None => owned.push(Owned::Notification { method, params: read }),
                    Some(id) if running.contains(&id) => answers.push(json!({
                        "id": id,
                        "code": code::INVALID_REQUEST,
                        "message": format!("request id {id} is already in use by a running request"),
                    })),
                    Some(id) => {
                        running.push(id.clone());
                        owned.push(Owned::Request { id, method, params: read });
                    }
                }
                Value::Object(message).to_string()
            }
            "cancel" => {
                let params = match tc.draw(gs::booleans()) {
                    true => {
                        let id = request_id(&tc);
                        if running.contains(&id) && !cancelled.contains(&id) {
                            cancelled.push(id.clone());
                        }
                        json!({ "id": id })
                    }
                    false => json!({ "nothing": "named" }),
                };
                json!({ "jsonrpc": "2.0", "method": method::CANCEL, "params": params }).to_string()
            }
            _ => {
                // Nothing was sent: no response has anyone waiting for it.
                let id = match tc.draw(gs::integers::<u8>().max_value(2)) {
                    0 => Value::Null,
                    1 => tc.draw(gs::integers::<u32>()).into(),
                    _ => json!("x"),
                };
                message.insert("id".into(), id);
                let (result, error) = tc.draw(gs::tuples!(gs::booleans(), gs::booleans()));
                if result {
                    message.insert("result".into(), json_value(&tc, 1));
                }
                if error {
                    message.insert("error".into(), json!({ "code": -32000, "message": "no" }));
                }
                // Neither or both is logged too.
                logs += if result == error { 2 } else { 1 };
                Value::Object(message).to_string()
            }
        };
        match kind {
            "not json" | "cut short" => answers.push(json!({ "id": null, "code": code::PARSE_ERROR })),
            "not an object" => answers
                .push(json!({ "id": null, "code": code::INVALID_REQUEST, "message": "a message is a JSON object" })),
            "bad id" => answers.push(
                json!({ "id": null, "code": code::INVALID_REQUEST, "message": "id must be a string or a number" }),
            ),
            _ => {}
        }
        raw.send(&line);
    }
    let got: Vec<Owned> = owners_until(&mut raw, &incoming, "sentinel")
        .into_iter()
        .map(|m| match m {
            Incoming::Request(r) => {
                assert_eq!(r.cancel.is_cancelled(), cancelled.contains(&r.id), "{:?}", r.id);
                Owned::Request { id: r.id, method: r.method, params: r.params }
            }
            Incoming::Notification { method, params } => Owned::Notification { method, params },
        })
        .collect();
    assert_eq!(got, owned);
    let lines = raw.until(&peer, "last");
    assert_eq!(lines.len(), answers.len(), "{lines:?}");
    for (line, expected) in lines.iter().zip(&answers) {
        let mut said = json!({ "id": line["id"], "code": line["error"]["code"] });
        // A parse error's message is serde_json's.
        if expected.get("message").is_some() {
            said["message"] = line["error"]["message"].clone();
        }
        assert_eq!(&said, expected);
    }
    assert_eq!(logged.lock().unwrap().len(), logs, "{:?}", logged.lock().unwrap());
}

/// How the other end answers a request.
#[derive(Debug, Clone)]
enum Answer {
    Result(u32),
    Error(RpcError),
    /// Neither a result nor an error, or both.
    Muddled(bool),
    /// An `error` that isn't a JSON-RPC error object.
    NotAnError,
    /// A result of another shape than the one asked for.
    Misshapen,
    /// A line that isn't JSON, cut short after the id.
    CutShort,
}

#[hegel::test_helper]
fn answer(tc: &TestCase) -> Answer {
    match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => Answer::Result(tc.draw(gs::integers())),
        1 => {
            let message = tc.draw(gs::text().max_size(6));
            Answer::Error(match tc.draw(gs::booleans()) {
                true => RpcError::failed(&tc.draw(gs::from_regex("[a-z_]{1,8}")), message),
                // A plain JSON-RPC error, without data.
                false => RpcError { code: tc.draw(gs::integers::<i64>()), message, data: None },
            })
        }
        2 => Answer::Muddled(tc.draw(gs::booleans())),
        3 => Answer::NotAnError,
        4 => Answer::Misshapen,
        _ => Answer::CutShort,
    }
}

impl Answer {
    /// The line answering request `id` this way.
    fn line(&self, id: u64) -> String {
        let response = |key: &str, value: Value| json!({ "jsonrpc": "2.0", "id": id, key: value }).to_string();
        match self {
            Answer::Result(n) => response("result", json!(n)),
            Answer::Error(e) => response("error", json!(e)),
            Answer::Muddled(false) => json!({ "jsonrpc": "2.0", "id": id }).to_string(),
            Answer::Muddled(true) => json!({ "jsonrpc": "2.0", "id": id, "result": 1, "error": {} }).to_string(),
            Answer::NotAnError => response("error", json!("oops")),
            Answer::Misshapen => response("result", json!({ "n": 1 })),
            Answer::CutShort => format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{"name":"a\"}}"#),
        }
    }

    /// Whether `got` is what waiting for an answer this way gives.
    fn check(&self, got: &Result<u32, RpcError>) {
        let error = |code: i64, start: &str| match got {
            Err(e) => assert!(e.code == code && e.message.starts_with(start), "{self:?}: {e:?}"),
            Ok(n) => panic!("{self:?}: {n}"),
        };
        match self {
            Answer::Result(n) => assert_eq!(got, &Ok(*n)),
            Answer::Error(e) => assert_eq!(got, &Err(e.clone())),
            Answer::Muddled(_) => error(code::INTERNAL, "a response with both a result and an error, or neither"),
            Answer::NotAnError => error(code::INTERNAL, "an error response that isn't one: "),
            Answer::Misshapen => error(code::INTERNAL, "unexpected result: "),
            Answer::CutShort => error(code::PARSE_ERROR, "parse error: "),
        }
    }
}

/// Requests sent from several threads at once each get their own response, however the other
/// end orders and words them, and never one meant for another: each has an id of its own, no
/// response is there before it's sent, and once one has come, waiting again finds it gone.
/// Responses to ids never sent are dropped on the way.
#[hegel::test(test_cases = 40)]
fn each_request_gets_its_own_response_under_any_interleaving(tc: TestCase) {
    let (peer, _incoming, mut raw) = connect();
    let sizes = tc.draw(gs::vecs(gs::integers::<usize>().min_value(1).max_value(3)).min_size(1).max_size(4));
    let answers: Vec<Vec<Answer>> = sizes.iter().map(|&k| (0..k).map(|_| answer(&tc)).collect()).collect();
    let total: usize = sizes.iter().sum();
    let order = tc.draw(gs::permutations((0..total).collect::<Vec<_>>()));
    let strays = tc.draw(gs::vecs(gs::integers::<u64>().min_value(1000)).max_size(3));
    let (sent, all_sent) = mpsc::channel();
    let threads: Vec<_> = answers
        .iter()
        .enumerate()
        .map(|(t, answers)| {
            let (peer, sent, k) = (peer.clone(), sent.clone(), answers.len());
            std::thread::spawn(move || {
                let pending: Vec<Pending> =
                    (0..k).map(|n| peer.send_request("m", &json!({ "t": t, "n": n })).unwrap()).collect();
                for p in &pending {
                    assert!(p.wait_timeout::<u32>(Duration::ZERO).is_none(), "nothing has answered it yet");
                }
                sent.send(()).unwrap();
                // Half waited for with a timeout: once that's had the response, there's none left.
                pending
                    .into_iter()
                    .map(|p| {
                        let id = p.id();
                        let got = match id % 2 == 0 {
                            true => p.wait::<u32>(),
                            false => {
                                let got = p.wait_timeout(Duration::from_secs(60)).expect("answered");
                                let again = p.wait_timeout::<u32>(Duration::ZERO).unwrap().unwrap_err();
                                assert_eq!(again, RpcError::closed());
                                got
                            }
                        };
                        (id, got)
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    for _ in 0..answers.len() {
        all_sent.recv().unwrap();
    }
    let requests: Vec<Value> = (0..total).map(|_| raw.recv()).collect();
    for (i, stray) in order.into_iter().zip(strays.into_iter().map(Some).chain(std::iter::repeat(None))) {
        let r = &requests[i];
        let (t, n) = (r["params"]["t"].as_u64().unwrap() as usize, r["params"]["n"].as_u64().unwrap() as usize);
        raw.send(&answers[t][n].line(r["id"].as_u64().unwrap()));
        if let Some(stray) = stray {
            raw.send(&json!({ "jsonrpc": "2.0", "id": stray, "result": 0 }).to_string());
        }
    }
    let mut ids = Vec::new();
    for (t, thread) in threads.into_iter().enumerate() {
        for (n, (id, got)) in thread.join().unwrap().into_iter().enumerate() {
            answers[t][n].check(&got);
            ids.push(id);
        }
    }
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "every request has an id of its own");
}

/// Progress and partial rows the other end sends about a request sent for its progress come to
/// that request, all of them and each in order, by the time its response has; those about
/// anything else (another request, one never sent, or that aren't what the peer can read) go to
/// the owner, in order.
#[hegel::test(test_cases = 40)]
fn progress_reaches_its_request_in_order_before_the_answer(tc: TestCase) {
    let (peer, incoming, mut raw) = connect();
    let reporting: Vec<Pending> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)))
        .map(|_| peer.send_request_reporting("command/run", &json!({})).unwrap())
        .collect();
    let plain = peer.send_request("command/run", &json!({})).unwrap();
    let ids: Vec<u64> = (0..=reporting.len()).map(|_| raw.recv()["id"].as_u64().unwrap()).collect();
    let mut to_each = vec![Vec::new(); reporting.len()];
    let mut parts_to_each = vec![Vec::new(); reporting.len()];
    let mut to_owner = Vec::new();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(10)) {
        // Partial rows or items instead, or something that isn't them.
        if tc.draw(gs::booleans()) {
            let part = match tc.draw(gs::integers::<u8>().max_value(2)) {
                0 => json!({ "rows": [[tc.draw(gs::integers::<u8>())]] }),
                1 => json!({ "items": [{ "label": tc.draw(gs::text().max_size(6)) }] }),
                _ => json!({ "rows": "not rows" }),
            };
            let readable = part["rows"] != "not rows";
            let to = tc.draw(gs::integers::<usize>().max_value(ids.len()));
            let mut params = part.clone();
            params["id"] = ids.get(to).copied().unwrap_or(9999).into();
            match to < reporting.len() && readable {
                true => parts_to_each[to].push(part),
                false => to_owner.push((method::PARTIAL, params.clone())),
            }
            raw.send(&json!({ "jsonrpc": "2.0", "method": method::PARTIAL, "params": params }).to_string());
            continue;
        }
        let phase = tc.draw(gs::sampled_from(vec!["scan", "copy", "teleport"]));
        let event = match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 => json!({ "event": "warning", "message": tc.draw(gs::text().max_size(6)) }),
            1 => json!({ "event": "phase_start", "phase": phase, "items": tc.draw(gs::integers::<u32>()), "bytes": 0 }),
            2 => json!({ "event": "item", "phase": phase, "path": "a.mp3", "bytes": 1, "target": "USB", "items": 3 }),
            3 => json!({ "event": "step", "step": { "index": 1, "total": 2, "id": "scan" } }),
            _ => json!({ "event": "rewind", "seconds": tc.draw(gs::integers::<u8>()) }),
        };
        // One of the requests sent for progress, the plain one, one never sent, or none.
        let to = tc.draw(gs::integers::<usize>().max_value(ids.len() + 1));
        let params = match to {
            _ if to < reporting.len() => json!({ "id": ids[to], "event": event }),
            _ if to == reporting.len() => json!({ "id": ids[to], "event": event }),
            _ if to == ids.len() => json!({ "id": 9999, "event": event }),
            _ => json!({ "event": event }),
        };
        match to < reporting.len() {
            true => to_each[to].push(event),
            false => to_owner.push((method::PROGRESS, params.clone())),
        }
        raw.send(&json!({ "jsonrpc": "2.0", "method": method::PROGRESS, "params": params }).to_string());
    }
    for id in &ids {
        raw.send(&json!({ "jsonrpc": "2.0", "id": id, "result": 1 }).to_string());
    }
    for ((pending, sent), parts) in reporting.into_iter().zip(to_each).zip(parts_to_each) {
        assert_eq!(pending.wait_timeout::<u32>(Duration::from_secs(60)).unwrap(), Ok(1));
        let got: Vec<Value> = pending.progress().map(|r| serde_json::to_value(r).unwrap()).collect();
        assert_eq!(got, sent);
        let got: Vec<Value> = pending
            .partials()
            .map(|p| {
                let mut p = serde_json::to_value(p).unwrap();
                p.as_object_mut().unwrap().remove("id");
                p
            })
            .collect();
        assert_eq!(got, parts);
    }
    assert_eq!((plain.progress().count(), plain.partials().count()), (0, 0));
    let owned: Vec<(&str, Value)> = owners_until(&mut raw, &incoming, "sentinel")
        .into_iter()
        .map(|m| match m {
            Incoming::Notification { method, params } if method == method::PROGRESS => (method::PROGRESS, params),
            Incoming::Notification { method, params } if method == method::PARTIAL => (method::PARTIAL, params),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(owned, to_owner);
}

/// An input that fails once its pipe has closed, as a broken one would.
struct Broken(PipeReader);

impl Read for Broken {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.0.read(buf)? {
            0 => Err(std::io::Error::other("unplugged")),
            n => Ok(n),
        }
    }
}

/// However the connection ends (the input closing, breaking, or the peer closing its output),
/// what's in flight ends with it: once the input has gone, requests waiting fail with `closed`,
/// the other end's requests still running are cancelled (not those answered), the owner's
/// receiver closes and nothing more can be sent; once the output has, nothing more can be sent
/// either, but responses still come in.
#[hegel::test(test_cases = 40)]
fn however_the_connection_ends_what_is_in_flight_ends_with_it(tc: TestCase) {
    let how = tc.draw(gs::sampled_from(vec!["input closes", "input breaks", "output closes"]));
    let (peer, incoming, mut raw, logged) = match how {
        "input breaks" => connect_logged(Broken),
        _ => connect_logged(|r| r),
    };
    let running: Vec<(Request, bool)> = (0..tc.draw(gs::integers::<usize>().max_value(3)))
        .map(|n| {
            raw.send(&json!({ "jsonrpc": "2.0", "id": n, "method": "command/run" }).to_string());
            (request(&incoming), tc.draw(gs::booleans()))
        })
        .collect();
    for (r, answered) in &running {
        if *answered {
            peer.respond(&r.id, Ok(Value::Null));
            assert_eq!(raw.recv()["id"], r.id);
        }
    }
    let pending: Vec<Pending> = (0..tc.draw(gs::integers::<usize>().max_value(3)))
        .map(|_| peer.send_request("m", &json!({})).unwrap())
        .collect();
    let ids: Vec<Value> = pending.iter().map(|_| raw.recv()["id"].clone()).collect();
    if how == "output closes" {
        peer.close();
        assert_eq!(peer.send_request("m", &json!({})).err(), Some(RpcError::closed()));
        // Neither of these can go out: dropped.
        peer.notify("progress", &json!({}));
        for (r, _) in &running {
            peer.respond(&r.id, Ok(Value::Null));
        }
        let mut rest = String::new();
        assert_eq!(raw.from_peer.read_line(&mut rest).unwrap(), 0, "the other end reads the end: {rest:?}");
        for (p, id) in pending.into_iter().zip(ids) {
            raw.send(&json!({ "jsonrpc": "2.0", "id": id, "result": 7 }).to_string());
            assert_eq!(p.wait::<u32>(), Ok(7));
        }
        return;
    }
    raw.close();
    for p in pending {
        assert_eq!(p.wait::<Value>().unwrap_err(), RpcError::closed());
    }
    assert!(incoming.recv().is_err(), "the owner's receiver closes");
    for (r, answered) in &running {
        assert_eq!(r.cancel.is_cancelled(), !answered, "{:?}", r.id);
    }
    assert_eq!(peer.send_request("m", &json!({})).err(), Some(RpcError::closed()));
    let broke = logged.lock().unwrap().iter().any(|l| l == "reading messages: unplugged");
    assert_eq!(broke, how == "input breaks");
}

/// A message is sent whole on one line exactly when it fits in [`MAX_LINE`] bytes, and refused
/// otherwise with nothing written; a response too long goes out as an error instead. A line too
/// long coming in is skipped and answered with a parse error, and the peer reads on.
#[hegel::test(test_cases = 6)]
fn a_message_goes_out_whole_when_it_fits_in_a_line_and_one_too_long_is_skipped(tc: TestCase) {
    let (peer, incoming, mut raw, logged) = connect_logged(|r| r);
    // The first request's id is 1: with it, the line is `{"jsonrpc":"2.0","id":1,"method":"m","params":"..."}`.
    let overhead = json!({ "jsonrpc": "2.0", "id": 1, "method": "m", "params": "" }).to_string().len();
    let over = tc.draw(gs::integers::<i64>().min_value(-2).max_value(2));
    let text = "a".repeat((MAX_LINE as i64 + over) as usize - overhead);
    // Sent from a thread of its own, so this one can read what's sent meanwhile.
    let (p, params) = (peer.clone(), text.clone());
    let sending = std::thread::spawn(move || {
        let sent = p.send_request("m", &params).map(|p| p.id());
        p.notify("sent", &json!({}));
        sent
    });
    let lines: Vec<Value> = std::iter::from_fn(|| Some(raw.recv())).take_while(|l| l["method"] != "sent").collect();
    match sending.join().unwrap() {
        Ok(id) => {
            assert!(over <= 0, "{over}");
            let [line] = &lines[..] else { panic!("{} lines", lines.len()) };
            assert_eq!((line["id"].as_u64(), line["params"].as_str().map(str::len)), (Some(id), Some(text.len())));
        }
        Err(e) => {
            assert!(over > 0, "{over}");
            assert_eq!((e.code, e.message.as_str()), (code::INTERNAL, "a message is longer than 16 MiB"));
            assert!(lines.is_empty(), "nothing went out");
        }
    }
    raw.send(r#"{"jsonrpc":"2.0","id":"r","method":"command/run"}"#);
    let r = request(&incoming);
    // The same for a response: `{"jsonrpc":"2.0","id":"r","result":"..."}`.
    let overhead = json!({ "jsonrpc": "2.0", "id": "r", "result": "" }).to_string().len();
    let over = tc.draw(gs::integers::<i64>().min_value(-2).max_value(2));
    let text = "a".repeat((MAX_LINE as i64 + over) as usize - overhead);
    let (p, len) = (peer.clone(), text.len());
    let responding = std::thread::spawn(move || p.respond(&r.id, Ok(json!(text))));
    let response = raw.recv();
    responding.join().unwrap();
    assert_eq!(response["id"], "r");
    match response.get("result") {
        Some(result) => {
            assert_eq!((over <= 0, result.as_str().map(str::len)), (true, Some(len)))
        }
        None => assert_eq!((over > 0, &response["error"]["code"]), (true, &json!(code::INTERNAL))),
    }
    let long = (MAX_LINE as i64 + 1 + tc.draw(gs::integers::<i64>().max_value(1 << 16).min_value(0))) as usize;
    let mut to = raw.to_peer.take().unwrap();
    let writer = std::thread::spawn(move || {
        to.write_all(&vec![b'x'; long]).unwrap();
        to.write_all(b"\n{\"jsonrpc\":\"2.0\",\"method\":\"after\"}\n").unwrap();
        to
    });
    let e = raw.recv();
    assert_eq!((e["id"].clone(), e["error"]["code"].clone()), (Value::Null, json!(code::PARSE_ERROR)));
    assert_eq!(e["error"]["message"], "parse error: a line is longer than 16 MiB");
    assert!(matches!(next(&incoming), Incoming::Notification { method, .. } if method == "after"), "read on");
    drop(writer.join().unwrap());
    assert_eq!(*logged.lock().unwrap(), ["skipped a line longer than 16 MiB"]);
}

/// Requests sent while answering the other end's requests are cancelled with them: when the other
/// end cancels a request, each one sent within it fails with `cancelled` at once and `$/cancel`
/// goes out for it, while those within other requests get their responses. Once a request is
/// cancelled or answered, nothing more is sent within it.
#[hegel::test(test_cases = 40)]
fn requests_sent_within_a_cancelled_one_are_cancelled_with_it(tc: TestCase) {
    let (peer, incoming, mut raw) = connect();
    let hooks: Vec<Request> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)))
        .map(|n| {
            raw.send(&json!({ "jsonrpc": "2.0", "id": format!("h{n}"), "method": "command/run" }).to_string());
            request(&incoming)
        })
        .collect();
    let mut within: Vec<(usize, Pending, u64)> = Vec::new();
    for (h, hook) in hooks.iter().enumerate() {
        for _ in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
            let p = peer.send_request_within(&hook.id, "tracks.query", &json!({ "hook": h })).unwrap();
            let id = raw.recv()["id"].as_u64().unwrap();
            assert_eq!(id, p.id());
            within.push((h, p, id));
        }
    }
    let cancelled =
        tc.draw(gs::vecs(gs::integers::<usize>().max_value(hooks.len() - 1)).unique(true).max_size(hooks.len()));
    for &h in &cancelled {
        raw.send(&json!({ "jsonrpc": "2.0", "method": method::CANCEL, "params": { "id": hooks[h].id } }).to_string());
    }
    assert!(owners_until(&mut raw, &incoming, "sentinel").is_empty());
    let mut expected: Vec<u64> = within.iter().filter(|(h, ..)| cancelled.contains(h)).map(|(.., id)| *id).collect();
    // A pending request can be cancelled from this end too (its response still comes).
    if let Some((_, p, id)) = within.iter().find(|(h, ..)| !cancelled.contains(h)) {
        p.cancel();
        expected.push(*id);
    }
    let mut cancels: Vec<u64> = raw
        .until(&peer, "last")
        .iter()
        .map(|l| {
            assert_eq!(l["method"], method::CANCEL);
            l["params"]["id"].as_u64().unwrap()
        })
        .collect();
    cancels.sort();
    expected.sort();
    assert_eq!(cancels, expected);
    for (h, p, id) in within {
        if cancelled.contains(&h) {
            assert_eq!(p.wait::<u32>(), Err(RpcError::cancelled()));
        } else {
            raw.send(&json!({ "jsonrpc": "2.0", "id": id, "result": h }).to_string());
            assert_eq!(p.wait::<usize>(), Ok(h));
        }
    }
    for (h, hook) in hooks.iter().enumerate() {
        assert_eq!(hook.cancel.is_cancelled(), cancelled.contains(&h));
        if !cancelled.contains(&h) && tc.draw(gs::booleans()) {
            peer.respond(&hook.id, Ok(Value::Null));
            assert_eq!(raw.recv()["id"], hook.id);
        } else if !cancelled.contains(&h) {
            continue;
        }
        // Cancelled or answered: nothing more goes out within it.
        assert_eq!(peer.send_request_within(&hook.id, "tracks.query", &json!({})).err(), Some(RpcError::cancelled()));
    }
    assert_eq!(raw.until(&peer, "end"), Vec::<Value>::new());
}

/// In a line that isn't JSON, the id of what looks like a response is found wherever it is among
/// the top-level keys, past strings with quotes and braces in them and ids nested deeper, unless
/// there's a `method` before the line ends (a request's) or the id isn't a number.
#[hegel::test(test_cases = 200)]
fn a_response_id_is_found_in_any_line_cut_short(tc: TestCase) {
    let id = tc.draw(gs::integers::<u64>());
    let string_id = tc.draw(gs::weighted_booleans(0.1));
    let mut keys = vec!["jsonrpc", "id", "result"];
    if tc.draw(gs::weighted_booleans(0.2)) {
        keys.push("method");
    }
    let order = tc.draw(gs::permutations(keys));
    let mut line = String::from("{");
    let mut after_id = 0;
    let mut method_at = None;
    for (i, key) in order.iter().enumerate() {
        if i > 0 {
            line.push(',');
        }
        // Whitespace JSON allows around the colon.
        let colon = tc.draw(gs::sampled_from(vec![":", " : ", ":\t"]));
        let value = match *key {
            "jsonrpc" => json!("2.0"),
            "id" if string_id => json!(id.to_string()),
            "id" => json!(id),
            "method" => json!("m"),
            _ => json_value(&tc, 2),
        };
        if *key == "method" {
            method_at = Some(line.len());
        }
        line.push_str(&format!("\"{key}\"{colon}{value}"));
        if *key == "id" {
            after_id = line.len();
        }
    }
    let cut = tc.draw(gs::integers::<usize>().min_value(after_id).max_value(line.len()));
    let cut = (cut..=line.len()).find(|i| line.is_char_boundary(*i)).unwrap();
    let line = &line[..cut];
    // A `method` key counts once its colon is in.
    let method = method_at.is_some_and(|at| {
        let rest = line.get(at..).and_then(|rest| rest.strip_prefix("\"method\""));
        rest.is_some_and(|rest| rest.trim_start().starts_with(':'))
    });
    let expected = (!string_id && !method).then_some(id);
    assert_eq!(response_id(line.as_bytes()), expected, "{line}");
}

/// Every string in `v` the test sent (`x-...`), in capitals.
fn shout(v: &mut Value) {
    match v {
        Value::String(s) if s.starts_with("x-") => *s = s.to_uppercase(),
        Value::Array(list) => list.iter_mut().for_each(shout),
        Value::Object(map) => map.values_mut().for_each(shout),
        _ => {}
    }
}

/// For any text the other end sends in a response (its result or its error), a notification,
/// progress or a partial, what reads it gets it as the intake left it; in a request, as it was
/// sent.
#[hegel::test(test_cases = 40)]
fn what_the_other_end_tells_goes_through_the_intake(tc: TestCase) {
    let (peer_in, to_peer) = pipe().unwrap();
    let (from_peer, peer_out) = pipe().unwrap();
    let mut raw = Raw { to_peer: Some(to_peer), from_peer: BufReader::new(from_peer) };
    let (peer, incoming) = Peer::with_intake(peer_in, peer_out, |_| {}, shout);
    // What the intake changes: text of the test's, not the protocol's own (`warning`).
    let said: String = tc.draw(gs::from_regex("x-[a-z]{1,6}").fullmatch(true));
    let told = said.to_uppercase();
    match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => {
            let pending = peer.send_request("command/run", &json!({})).unwrap();
            let id = raw.recv()["id"].clone();
            raw.send(&json!({ "jsonrpc": "2.0", "id": id, "result": { "label": said } }).to_string());
            assert_eq!(pending.wait::<Value>().unwrap(), json!({ "label": told }));
        }
        1 => {
            let pending = peer.send_request("command/run", &json!({})).unwrap();
            let id = raw.recv()["id"].clone();
            raw.send(&json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": said } }).to_string());
            assert_eq!(pending.wait::<Value>().unwrap_err().message, told);
        }
        2 => {
            raw.send(&json!({ "jsonrpc": "2.0", "method": "host/notify", "params": { "message": said } }).to_string());
            let Incoming::Notification { params, .. } = next(&incoming) else { panic!("a notification") };
            assert_eq!(params, json!({ "message": told }));
        }
        3 | 4 => {
            let pending = peer.send_request_reporting("command/run", &json!({})).unwrap();
            let id = raw.recv()["id"].clone();
            let (method, params) = match tc.draw(gs::booleans()) {
                true => (method::PARTIAL, json!({ "id": id, "items": [{ "label": said }] })),
                false => (method::PROGRESS, json!({ "id": id, "event": { "event": "warning", "message": said } })),
            };
            raw.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }).to_string());
            raw.send(&json!({ "jsonrpc": "2.0", "id": id, "result": 1 }).to_string());
            assert_eq!(pending.wait_timeout::<u32>(Duration::from_secs(60)).unwrap(), Ok(1));
            let got: Vec<Value> = match method {
                method::PARTIAL => pending.partials().map(|p| serde_json::to_value(p.items).unwrap()).collect(),
                _ => pending.progress().map(|r| serde_json::to_value(r).unwrap()["message"].clone()).collect(),
            };
            let want = match method {
                method::PARTIAL => json!([{ "label": told }]),
                _ => json!(told),
            };
            assert_eq!(got, [want]);
        }
        _ => {
            raw.send(
                &json!({ "jsonrpc": "2.0", "id": 1, "method": "net/fetch", "params": { "url": said } }).to_string(),
            );
            assert_eq!(request(&incoming).params, json!({ "url": said }), "a request goes on as it was sent");
        }
    }
}
