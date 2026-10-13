use std::io::{BufRead, PipeReader, PipeWriter, pipe};
use std::time::Duration;

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
