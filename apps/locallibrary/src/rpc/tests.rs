use locallibrary::progress::{Phase, Steps};

use super::*;

/// Collects what the sidecar writes.
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Sink {
    fn events(&self) -> Vec<Value> {
        let text = String::from_utf8(self.0.lock().unwrap().clone()).unwrap();
        text.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()["params"]["event"].take()).collect()
    }
}

/// The request loop, writing to a sink.
fn server() -> (Server, Sink) {
    let sink = Sink::default();
    let server = Server {
        store_root: PathBuf::from("store"),
        out: Arc::new(Mutex::new(Box::new(sink.clone()))),
        jobs: Arc::default(),
        events: Arc::default(),
        watch: Arc::default(),
        reads: Pool::new(1),
        work: Pool::new(1),
        plans: Arc::default(),
    };
    (server, sink)
}

/// Blank lines are skipped; a request without a method, or whose parameters aren't an
/// object, is answered with what's wrong with it.
#[test]
fn blank_lines_are_skipped_and_bad_requests_say_why() {
    let (s, sink) = server();
    assert!(s.handle(b""));
    assert!(s.handle(br#"{"jsonrpc":"2.0","id":1}"#));
    assert!(s.handle(br#"{"jsonrpc":"2.0","id":2,"method":"rpc.describe","params":7}"#));
    let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
    let replies: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(replies.len(), 2, "nothing for the blank line: {replies:?}");
    let missing = &replies[0];
    assert_eq!((&missing["id"], &missing["error"]["code"]), (&json!(1), &json!(-32600)), "{missing}");
    assert_eq!(missing["error"]["message"], "missing method");
    let bad = &replies[1]["error"];
    assert_eq!((&replies[1]["id"], &bad["data"]["kind"]), (&json!(2), &json!("invalid_params")), "{bad}");
    assert!(bad["message"].as_str().unwrap().starts_with("invalid params: invalid type: integer `7`"), "{bad}");
    assert!(bad["data"]["fields"].get("path").is_none(), "the whole of it is wrong, not a field: {bad}");
}

fn progress() -> (RpcProgress, Sink) {
    let sink = Sink::default();
    (RpcProgress::new(json!(1), Arc::new(Mutex::new(Box::new(sink.clone())))), sink)
}

#[test]
fn a_line_longer_than_the_limit_is_skipped_to_its_end() {
    let mut text = vec![b'x'; MAX_LINE];
    text.push(b'\n');
    text.extend(vec![b'y'; MAX_LINE + 1]);
    text.extend(b"\n{}\nlast");
    let mut input = std::io::Cursor::new(text);
    let mut buf = Vec::new();
    assert_eq!(read_line(&mut input, &mut buf).unwrap(), Line::Read, "exactly the limit");
    assert_eq!(buf.len(), MAX_LINE + 1);
    assert_eq!(read_line(&mut input, &mut buf).unwrap(), Line::TooLong);
    assert!(buf.is_empty());
    assert_eq!((read_line(&mut input, &mut buf).unwrap(), &buf[..]), (Line::Read, &b"{}\n"[..]));
    assert_eq!((read_line(&mut input, &mut buf).unwrap(), &buf[..]), (Line::Read, &b"last"[..]));
    assert_eq!(read_line(&mut input, &mut buf).unwrap(), Line::End);
}

#[test]
fn items_and_bytes_are_coalesced() {
    let (p, sink) = progress();
    let ctx = Ctx::new(&p, CancelToken::new());
    ctx.phase_start(Phase::Write, 5000, 0);
    for i in 0..5000 {
        ctx.emit(Event::Item { phase: Phase::Write, path: format!("/t/{i}"), bytes: 0 });
        ctx.emit(Event::Bytes { phase: Phase::Write, bytes: 10 });
    }
    ctx.phase_end(Phase::Write);
    p.finish();

    let events = sink.events();
    assert!(events.len() < 100, "{} lines", events.len());
    let of = |kind: &'static str| events.iter().filter(move |e| e["event"] == kind);
    assert_eq!(of("item").map(|e| e["items"].as_u64().unwrap_or(1)).sum::<u64>(), 5000);
    assert_eq!(of("bytes").map(|e| e["bytes"].as_u64().unwrap()).sum::<u64>(), 50_000);
    assert_eq!(of("item").next_back().unwrap()["path"], "/t/4999", "the latest file is the one shown");
    assert_eq!(events.first().unwrap()["event"], "phase_start");
    assert_eq!(events.last().unwrap()["event"], "phase_end", "held events go out before the next one");
}

#[test]
fn events_carry_their_target_and_step() {
    let (p, sink) = progress();
    let ctx = Ctx::new(&p, CancelToken::new());
    ctx.warn("before");
    let steps = Steps::new(&ctx, vec!["name", "library"]);
    let ctx = steps.ctx();
    steps.begin("library");
    for drive in ["/Volumes/A", "/Volumes/B"] {
        ctx.for_target(drive, |ctx| ctx.emit(Event::Item { phase: Phase::Copy, path: "x".into(), bytes: 1 }));
    }
    ctx.warn("after");

    let step = json!({ "index": 2, "total": 2, "id": "library" });
    assert_eq!(
        sink.events(),
        [
            json!({ "event": "warning", "message": "before" }),
            json!({ "event": "step", "step": step }),
            json!({ "event": "item", "phase": "copy", "path": "x", "bytes": 1, "target": "/Volumes/A", "step": step }),
            json!({ "event": "item", "phase": "copy", "path": "x", "bytes": 1, "target": "/Volumes/B", "step": step }),
            json!({ "event": "warning", "message": "after", "step": step }),
        ]
    );
}

#[test]
fn whatever_applies_a_plan_clears_the_library_cache() {
    for m in [
        "edit.apply",
        "sync.apply",
        "undo.apply",
        "drive.restore",
        "art.embed",
        "store.gc",
        "players.set",
        #[cfg(feature = "account")]
        "settings.publish.apply",
        "pull",
    ] {
        assert!(!registry().get(m).unwrap().changes_no_drive(), "{m}");
    }
    for m in ["edit.plan", "undo.plan", "undo.list", "drive.backups", "players.set.plan", "mix.identify", "export"] {
        assert!(registry().get(m).unwrap().changes_no_drive(), "{m}");
    }
}

#[test]
fn a_plan_is_taken_once_by_one_worker() {
    let dir = tempfile::tempdir().unwrap();
    let store = locallibrary::Store::open_or_init(dir.path()).unwrap();
    let plans = Arc::new(Plans::default());
    let mut ids = Vec::new();
    for _ in 0..MAX_PLANS + 1 {
        let Ok(kept) = plans.keep(api::store_gc_plan(&store).unwrap()) else { panic!("kept") };
        ids.push(kept["plan_id"].as_str().unwrap().to_string());
    }
    // The oldest made room.
    assert_eq!(plans.take(&ids[0], "store.gc").err().map(|e| e.kind), Some("stale_plan"));
    let id = ids.pop().unwrap();
    let takers: Vec<_> = (0..8)
        .map(|_| {
            let (plans, id) = (plans.clone(), id.clone());
            std::thread::spawn(move || plans.take(&id, "store.gc").is_ok())
        })
        .collect();
    let taken = takers.into_iter().map(|t| t.join().unwrap()).filter(|took| *took).count();
    assert_eq!(taken, 1);
    assert_eq!(plans.take(&ids[1], "other").err().map(|e| e.kind), Some("invalid_params"));
}
