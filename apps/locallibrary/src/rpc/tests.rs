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
        let Ok(kept) = plans.keep("store.gc.plan", api::store_gc_plan(&store).unwrap()) else { panic!("kept") };
        ids.push(kept["plan_id"].as_str().unwrap().to_string());
    }
    // The oldest made room.
    assert_eq!(plans.take("store.gc.apply", &ids[0], "store.gc").err().map(|e| e.kind), Some("stale_plan"));
    let id = ids.pop().unwrap();
    let takers: Vec<_> = (0..8)
        .map(|_| {
            let (plans, id) = (plans.clone(), id.clone());
            std::thread::spawn(move || plans.take("store.gc.apply", &id, "store.gc").is_ok())
        })
        .collect();
    let taken = takers.into_iter().map(|t| t.join().unwrap()).filter(|took| *took).count();
    assert_eq!(taken, 1);
    assert_eq!(plans.take("other.apply", &ids[1], "other").err().map(|e| e.kind), Some("invalid_params"));
}

/// Plugins' plans are kept apart: as many as there's room for don't push out one of
/// locallibrary's own, and a plan id given to the other kind of apply is refused as one, naming
/// what it is.
#[test]
fn plugins_plans_are_kept_apart() {
    let dir = tempfile::tempdir().unwrap();
    let store = locallibrary::Store::open_or_init(dir.path()).unwrap();
    let plans = Plans::default();
    let id = |kept: Result<Value, RpcError>| kept.unwrap()["plan_id"].as_str().unwrap().to_string();
    let own = id(plans.keep("store.gc.plan", api::store_gc_plan(&store).unwrap()));
    let theirs: Vec<String> =
        (0..MAX_PLANS + 1).map(|_| id(plans.keep("plugins.op.plan", api::store_gc_plan(&store).unwrap()))).collect();
    let refused = |method: &str, id: &str, operation: &str| plans.take(method, id, operation).err().map(|e| e.message);
    let message = refused("plugins.op.apply", &own, "plugins.op").unwrap();
    assert_eq!(message, format!("invalid params: plan {own} is a store.gc plan, not plugins.op"));
    let message = refused("store.gc.apply", &theirs[1], "store.gc").unwrap();
    assert_eq!(message, format!("invalid params: plan {} is a store.gc plan, not store.gc", theirs[1]));
    assert!(refused("plugins.op.apply", &theirs[0], "plugins.op").unwrap().starts_with("there's no plan"));
    assert!(plans.take("store.gc.apply", &own, "store.gc").is_ok());
    // A plugin's operation and export are each applied by their own apply.
    assert_eq!(applied_as("plugins.op.apply", "echo.playlist"), "plugins.op");
    assert_eq!(applied_as("plugins.export.apply", "echo.export.m3u"), "plugins.export");
    assert_eq!(applied_as("store.gc.apply", "store.gc"), "store.gc");
}

use hegel::TestCase;
use hegel::generators as gs;

/// A plan of any operation, standing in for a real one where only its id and operation matter.
struct Pretend {
    id: String,
    operation: String,
}

impl AnyPlanned for Pretend {
    fn id(&self) -> &str {
        &self.id
    }
    fn operation(&self) -> &str {
        &self.operation
    }
    fn summary(&self) -> &[String] {
        &[]
    }
    fn warnings(&self) -> &[String] {
        &[]
    }
    fn destructive(&self) -> bool {
        false
    }
    fn is_noop(&self) -> bool {
        true
    }
    fn drives(&self) -> &[locallibrary::op::Touched] {
        &[]
    }
    fn planned_at(&self) -> DateTime<Utc> {
        DateTime::UNIX_EPOCH
    }
    fn to_json(&self) -> Value {
        json!({ "plan_id": self.id, "operation": self.operation })
    }
    fn check(&self, _: Option<&locallibrary::Store>) -> locallibrary::Result<()> {
        Ok(())
    }
    fn apply_json(&self, _: Option<&locallibrary::Store>, _: &Ctx<'_>) -> locallibrary::Result<Value> {
        Ok(Value::Null)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The apply a plan of `operation` kept by `method` is for, as the protocol has it: a plan of
/// the sidecar's own by the apply of its operation, a plugin's export (`<plugin>.export.<format>`)
/// by `plugins.export`, a drive built from its library source (`<plugin>.source.<source>`) by
/// `plugins.source` and any other plugin operation by `plugins.op`.
fn applied_by(method: &str, operation: &str) -> String {
    if !method.starts_with("plugins.") {
        return operation.to_string();
    }
    let parts: Vec<&str> = operation.split('.').collect();
    match (parts.len(), parts.get(1)) {
        (3, Some(&"export")) => "plugins.export".into(),
        (3, Some(&"source")) => "plugins.source".into(),
        _ => "plugins.op".into(),
    }
}

/// Plans kept by the sidecar's own `*.plan` methods and by plugins', and taken by applies of
/// either kind, in any order, agree with a model of two caches: each keeps its newest
/// [`MAX_PLANS`]; a plan is taken once, by an apply of its own operation from the cache it was
/// kept in; a plan of another operation is refused and stays; one from the other cache is refused
/// saying what plan it is; any other id is stale.
#[hegel::test(test_cases = 40)]
fn plans_are_kept_and_taken_as_modelled(tc: TestCase) {
    let plans = Plans::new();
    // (method that keeps or applies, operation) for each cache: the sidecar's own and plugins'.
    let theirs = [
        ("plugins.op.plan", "echo.playlist"),
        ("plugins.op.plan", "echo.export.m3u"),
        ("plugins.export.plan", "tidy.export.csv"),
        ("plugins.source.plan", "tidy.source.folder"),
    ];
    let makers = [&[("store.gc.plan", "store.gc"), ("edit.plan", "edit")][..], &theirs[..]];
    let applies = [("store.gc.apply", "store.gc"), ("edit.apply", "edit")];
    let their_applies = [
        ("plugins.op.apply", "plugins.op"),
        ("plugins.export.apply", "plugins.export"),
        ("plugins.source.apply", "plugins.source"),
    ];
    let applies = [&applies[..], &their_applies[..]];
    let mut model: [VecDeque<(String, String)>; 2] = Default::default();
    let mut made = 0;
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(12)) {
        let cache = tc.draw(gs::integers::<usize>().max_value(1));
        if tc.draw(gs::booleans()) {
            // Enough at once to push the oldest out now and then.
            for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(MAX_PLANS / 2)) {
                let (method, operation) = tc.draw(gs::sampled_from(makers[cache].to_vec()));
                made += 1;
                let id = format!("p{made}");
                let json = plans.keep(method, Pretend { id: id.clone(), operation: operation.into() }).unwrap();
                assert_eq!(json["plan_id"], id.as_str());
                if model[cache].len() == MAX_PLANS {
                    model[cache].pop_front();
                }
                model[cache].push_back((id, operation.into()));
            }
            continue;
        }
        let (method, operation) = tc.draw(gs::sampled_from(applies[cache].to_vec()));
        let known: Vec<String> = model.iter().flatten().map(|(id, _)| id.clone()).collect();
        let id = match known.is_empty() || tc.draw(gs::weighted_booleans(0.2)) {
            true => format!("p{}", tc.draw(gs::integers::<usize>().max_value(made + 1))),
            false => tc.draw(gs::sampled_from(known)),
        };
        let taken = plans.take(method, &id, operation);
        let said = format!("{method} {id}: {:?}", taken.as_ref().err().map(|e| &e.message));
        let mine = model[cache].iter().position(|(kept, _)| *kept == id);
        let other = model[1 - cache].iter().find(|(kept, _)| *kept == id);
        match (mine, other) {
            (Some(i), _) if applied_by(method, &model[cache][i].1) == operation => {
                let (_, plan) = taken.unwrap_or_else(|_| panic!("{said}"));
                assert_eq!(plan.id(), id, "{said}");
                model[cache].remove(i);
            }
            (Some(i), _) => {
                let e = taken.err().unwrap_or_else(|| panic!("{said}"));
                let what = format!("invalid params: plan {id} is a {} plan, not {operation}", model[cache][i].1);
                assert_eq!((e.kind, e.message), ("invalid_params", what), "{said}");
            }
            (None, Some((_, made))) => {
                let e = taken.err().unwrap_or_else(|| panic!("{said}"));
                let what = format!("invalid params: plan {id} is a {made} plan, not {operation}");
                assert_eq!((e.kind, e.message), ("invalid_params", what), "{said}");
            }
            (None, None) => assert_eq!(taken.err().map(|e| e.kind), Some("stale_plan"), "{said}"),
        }
    }
}

/// A plugin asking to read gets what the read method answers when a worker runs it, and any
/// method that isn't a listed read (a plan, an apply, a change, one the server answers itself, a
/// hidden one, a name no method has) is not found: it never runs.
#[hegel::test(test_cases = 40)]
fn only_listed_reads_are_run_for_plugins(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    locallibrary::testing::volumes_on_this_thread(Some(vec![dir.path().to_path_buf()]));
    let store = dir.path().join("store");
    locallibrary::testing::store_root_on_this_thread(Some(store.clone()));
    // Reads whose answers come from this test's own folder (or from nothing at all): others read
    // the computer running the tests.
    let reads = ["version", "players.models", "settings.schema", "store.info", "library.list", "drives.known"];
    let others: Vec<&str> = registry()
        .methods()
        .filter(|m| m.kind != Kind::Read || m.hidden)
        .flat_map(|m| std::iter::once(m.name).chain(m.aliases.iter().copied()))
        .chain(["no.such", "", "plugins"])
        .collect();
    let name = match tc.draw(gs::booleans()) {
        true => tc.draw(gs::sampled_from(reads.to_vec())),
        false => tc.draw(gs::sampled_from(others)),
    };
    let ctx = Ctx::silent();
    let got = read(name, json!({}), &ctx);
    if !reads.contains(&name) {
        let e = got.err().unwrap_or_else(|| panic!("{name} ran"));
        assert_eq!((e.kind, e.message), ("method_not_found", format!("unknown method {name:?}")), "{name}");
        assert!(!store.exists(), "{name} made a store");
        return;
    }
    let (plans, call_ctx) = (Plans::default(), Ctx::silent());
    let call = Call { root: &store, plans: &plans, ctx: &call_ctx };
    let ran = registry().get(name).unwrap().run(&call, json!({})).unwrap();
    let kind = |r: &Result<Value, RpcError>| r.as_ref().map_err(|e| e.kind).cloned();
    assert_eq!(kind(&got), kind(&ran), "{name}");
}

/// Which apply takes a plan, for any operation: the plan's own operation for the sidecar's own
/// methods, and for plugins' `plugins.export` for an export (`<plugin>.export.<format>`) and
/// `plugins.op` for anything else.
#[hegel::test(test_cases = 100)]
fn a_plan_is_applied_by_the_apply_of_its_kind(tc: TestCase) {
    let word = || gs::sampled_from(vec!["echo", "export", "m3u", "playlist", "store", "gc", ""]);
    let parts = tc.draw(gs::vecs(word()).min_size(1).max_size(4));
    let operation = parts.join(".");
    let method = tc.draw(gs::sampled_from(vec!["store.gc.apply", "plugins.op.apply", "plugins.export.apply", "edit"]));
    assert_eq!(applied_as(method, &operation), applied_by(method, &operation), "{method} {operation}");
}
