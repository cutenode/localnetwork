//! A plugin's events (`event/notify`), the services it runs (`service/start`) and what it tells
//! the user (`host/notify`).

use serde_json::Map;

use super::*;
use crate::{
    AnalyzeAction, EventName, EventNotify, ExportAction, FetchAction, HostAction, HostNotify, MAX_SUGGESTED_PARAMS,
    StartService, Suggested, Suggestion,
};

/// The echo plugin's manifest, with a service.
fn with_service() -> String {
    format!("{}\n[[contributes.services]]\nname = \"beat\"\ntitle = \"Beat\"\n", echo::MANIFEST)
}

/// What it sends the host next, a notification: its method and params.
fn notified(s: &Session) -> (String, Value) {
    match s.next() {
        Incoming::Notification { method, params } => (method, params),
        Incoming::Request(req) => panic!("a request: {}", req.method),
    }
}

/// Each event, after `hello`, is handled in the order it came, by a handler that reads through
/// the host and tells the user; one sent before `hello` is dropped.
#[test]
fn events_are_handled_in_order_after_hello() {
    let hooks = Hooks::new().on_event(|e: EventNotify, host: &Host| {
        let drive = e.drive.clone().unwrap_or_default();
        let tracks: Value = host.call("tracks.query", &json!({ "drive": drive }))?;
        let message = format!("{} {} {}", e.event.name(), drive, tracks["total"]);
        let operation = Some(Suggested { name: "playlist".into(), params: json!({ "name": drive }) });
        host.notify(&HostNotify { message, operation, action: None });
        Ok(())
    });
    let s = start(hooks);
    let event =
        |drive: &str| EventNotify { event: EventName::DriveMounted, drive: Some(drive.into()), payload: json!({}) };
    s.host.notify(method::EVENT_NOTIFY, &event("early"));
    let reply: HelloReply = s.host.request(method::HELLO, &hello(PROTOCOL_VERSION, &[])).unwrap();
    assert_eq!(reply.features, ["accounts", "analysis", "events", "blobs", "net", "choose", "views"]);
    for drive in ["A", "B"] {
        s.host.notify(method::EVENT_NOTIFY, &event(drive));
    }
    for drive in ["A", "B"] {
        s.answer_tracks_query();
        let (method, params) = notified(&s);
        assert_eq!(method, method::HOST_NOTIFY);
        let notice: HostNotify = serde_json::from_value(params).unwrap();
        assert_eq!(notice.message, format!("drive.mounted {drive} 1"));
        assert_eq!(notice.operation.unwrap().params, json!({ "name": drive }));
    }
    s.stop();
}

/// An event its handler fails on, or panics on, is the plugin's to log: the next is handled.
#[test]
fn a_failing_event_doesnt_stop_the_next() {
    let hooks = Hooks::new().on_event(|e: EventNotify, host: &Host| match e.drive.as_deref() {
        Some("fails") => Err(RpcError::failed("invalid", "no")),
        Some("panics") => panic!("a handler that panics"),
        _ => {
            host.notify(&HostNotify { message: "next".into(), ..HostNotify::default() });
            Ok(())
        }
    });
    let s = greeted(hooks);
    for drive in ["fails", "panics", "fine"] {
        let event = EventNotify { event: EventName::PlanApplied, drive: Some(drive.into()), payload: Value::Null };
        s.host.notify(method::EVENT_NOTIFY, &event);
    }
    let (_, params) = notified(&s);
    assert_eq!(params["message"], "next");
    s.stop();
}

/// A service runs until the host cancels it, telling the user as it goes, then answers that it
/// was cancelled; one the manifest doesn't declare is refused.
#[test]
fn a_service_runs_until_cancelled() {
    let service = |req: StartService, host: &Host| -> Result<(), RpcError> {
        host.notify(&HostNotify { message: format!("{} started", req.service), ..HostNotify::default() });
        loop {
            host.ctx().check()?;
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    let s = start_with(with_service(), Hooks::new().on(service));
    let reply: HelloReply = s.host.request(method::HELLO, &hello(PROTOCOL_VERSION, &[])).unwrap();
    assert_eq!(reply.features, ["accounts", "analysis", "services", "blobs", "net", "choose", "views"]);
    let e = s.call(method::SERVICE_START, json!({ "service": "other" })).unwrap_err();
    assert_eq!(e.message, "invalid params: the manifest declares no service \"other\"");
    let running = s.host.send_request(method::SERVICE_START, &json!({ "service": "beat" })).unwrap();
    assert_eq!(notified(&s).1["message"], "beat started");
    assert!(running.wait_timeout::<Value>(Duration::from_millis(100)).is_none(), "it's still running");
    running.cancel();
    let e = running.wait::<Value>().unwrap_err();
    assert_eq!(e.kind(), Some("cancelled"));
    s.stop();
}

/// The host's actions echo has contributions for (its metadata provider, its exporter and its
/// analyzer), on the drive `USB`.
fn echo_actions() -> Vec<HostAction> {
    let drive = "USB".to_string();
    let fetch = FetchAction { drive: drive.clone(), tracks: vec![1, 2], provider: Some("tags".into()) };
    let options = Map::from_iter([("loud".to_string(), json!(true))]);
    let export = ExportAction { format: "m3u".into(), drive: Some(drive.clone()), options, ..ExportAction::default() };
    let analyze = AnalyzeAction { analyzer: "size".into(), drive, tracks: Vec::new() };
    vec![HostAction::TrackFetch(fetch), HostAction::Export(export), HostAction::Analyze(analyze)]
}

/// Echo's event handler suggests each of the host's actions it has contributions for, and the
/// host reads each back as it was sent; fetching cover art, which it has no provider of, it can
/// send, but the host refuses it.
#[test]
fn a_plugin_suggests_the_hosts_actions_on_its_own_contributions() {
    let art = HostAction::ArtFetch(FetchAction { drive: "USB".into(), ..FetchAction::default() });
    let actions: Vec<HostAction> = echo_actions().into_iter().chain([art]).collect();
    let sent = actions.clone();
    let hooks = echo::hooks().on_event(move |e: EventNotify, host: &Host| {
        let n = e.payload["n"].as_u64().unwrap() as usize;
        host.notify(&HostNotify::action(format!("suggestion {n}"), &sent[n]));
        Ok(())
    });
    let s = greeted(hooks);
    let manifest = Manifest::parse(echo::MANIFEST).unwrap();
    for (n, action) in actions.iter().enumerate() {
        let event =
            EventNotify { event: EventName::DriveMounted, drive: Some("USB".into()), payload: json!({ "n": n }) };
        s.host.notify(method::EVENT_NOTIFY, &event);
        let (method, params) = notified(&s);
        assert_eq!((method.as_str(), &params["message"]), (method::HOST_NOTIFY, &json!(format!("suggestion {n}"))));
        let notice: HostNotify = serde_json::from_value(params).unwrap();
        let taken = notice.suggestion(&manifest);
        match action {
            HostAction::ArtFetch(_) => {
                let refused = "it suggests the action \"art.fetch\", which it can't: no artwork provider";
                assert_eq!(taken, Err(refused.into()));
            }
            _ => assert_eq!(taken, Ok(Some(Suggestion::Action(action.clone())))),
        }
    }
    s.stop();
}

/// The host refuses a suggestion the plugin may not make, saying why: an action on an exporter,
/// an analyzer or a provider that isn't its own, on what its exporter doesn't take, with params
/// the action doesn't take, one that isn't an action, both an operation and an action, and params
/// too long. An operation alone is as it was.
#[test]
fn a_suggestion_the_plugin_may_not_make_is_refused() {
    let m = Manifest::parse(echo::MANIFEST).unwrap();
    let action = |name: &str, params: Value| HostNotify {
        message: "m".into(),
        operation: None,
        action: Some(Suggested { name: name.into(), params }),
    };
    let long = json!({ "drive": "x".repeat(MAX_SUGGESTED_PARAMS) });
    let playlist = Suggested { name: "playlist".into(), params: json!({ "name": "x" }) };
    let cases = [
        (
            action("export", json!({ "format": "xml", "drive": "USB" })),
            "\"export\", which it can't: no exporter \"xml\"",
        ),
        (
            action("export", json!({ "format": "m3u", "set": "last" })),
            "\"export\", which it can't: its exporter \"m3u\" takes a drive",
        ),
        (
            action("analyze", json!({ "analyzer": "tempo", "drive": "USB" })),
            "\"analyze\", which it can't: no analyzer \"tempo\"",
        ),
        (
            action("track.fetch", json!({ "drive": "USB", "provider": "musicbrainz" })),
            "\"track.fetch\", which it can't: no metadata provider \"musicbrainz\"",
        ),
        (
            action("track.fetch", json!({ "drive": "USB", "output": "/tmp" })),
            "\"track.fetch\", which it can't: unknown field `output`",
        ),
        (action("track.fetch", json!(["USB"])), "\"track.fetch\" with params that aren't an object"),
        (action("delete", json!({})), "\"delete\", which it can't: unknown variant `delete`"),
        (action("analyze", long), "\"analyze\" with params longer than 65536 bytes"),
        (
            action("analyze", json!({ "analyzer": "size", "drive": "/Users/me/OldBackup" })),
            "\"analyze\", which it can't: \"/Users/me/OldBackup\" isn't a drive's id or name",
        ),
        (
            action("export", json!({ "format": "m3u", "drive": "~" })),
            "\"export\", which it can't: \"~\" isn't a drive's id or name",
        ),
        (
            action("track.fetch", json!({ "drive": "..\\USB" })),
            "\"track.fetch\", which it can't: \"..\\\\USB\" isn't a drive's id or name",
        ),
    ];
    for (notice, why) in cases {
        let refused = notice.suggestion(&m).unwrap_err();
        assert!(refused.starts_with(&format!("it suggests the action {why}")), "{refused}");
    }
    let both = HostNotify { operation: Some(playlist.clone()), ..action("analyze", json!({})) };
    assert_eq!(both.suggestion(&m), Err("it suggests both an operation and an action".into()));
    let operation = HostNotify { operation: Some(playlist.clone()), ..HostNotify::default() };
    assert_eq!(operation.suggestion(&m), Ok(Some(Suggestion::Operation(playlist))));
}

/// For any action suggested, the host takes it exactly when it's one of the host's actions with
/// params it takes, on echo's own contributions, alone and short enough, and then as it was sent:
/// anything else is refused, saying so, and never taken.
#[hegel::test(test_cases = 200)]
fn only_actions_on_the_plugins_own_contributions_are_taken(tc: TestCase) {
    let m = Manifest::parse(echo::MANIFEST).unwrap();
    let name: &str = tc.draw(gs::sampled_from(vec!["art.fetch", "track.fetch", "export", "analyze", "delete"]));
    let fields = [
        (
            "drive",
            json!(tc.draw(gs::sampled_from(vec!["USB", "My drive", "/Users/me/OldBackup", "~", "..", "C:\\USB", ""]))),
        ),
        ("tracks", json!(tc.draw(gs::vecs(gs::integers::<u32>()).min_size(1).max_size(3)))),
        ("provider", json!(tc.draw(gs::sampled_from(vec!["tags", "covers"])))),
        ("format", json!(tc.draw(gs::sampled_from(vec!["m3u", "xml"])))),
        ("set", json!("last")),
        ("mix", json!("1")),
        ("analyzer", json!(tc.draw(gs::sampled_from(vec!["size", "tempo"])))),
        ("options", json!({ "loud": tc.draw(gs::booleans()) })),
        ("output", json!("/Volumes/USB")),
    ];
    // What the action takes, mostly, and now and then what it doesn't.
    let takes: &[&str] = match name {
        "art.fetch" | "track.fetch" => &["drive", "tracks", "provider"],
        "export" => &["format", "drive", "options"],
        _ => &["analyzer", "drive", "tracks"],
    };
    let mut params: Map<String, Value> = fields
        .into_iter()
        .filter(|(k, _)| tc.draw(gs::weighted_booleans(if takes.contains(k) { 0.8 } else { 0.1 })))
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    let (both, long) = (tc.draw(gs::weighted_booleans(0.1)), tc.draw(gs::weighted_booleans(0.1)));
    if long {
        params.insert("drive".into(), json!("x".repeat(MAX_SUGGESTED_PARAMS)));
    }
    // A drive by id or name: a path would be taken for a folder.
    let named = params.get("drive").and_then(Value::as_str).is_none_or(|d| {
        !d.is_empty() && !d.contains('/') && !d.contains('\\') && !d.starts_with('.') && !d.starts_with('~')
    });
    let has = |k: &str| params.contains_key(k) && (k != "drive" || named);
    let only = |takes: &[&str]| params.keys().all(|k| takes.contains(&k.as_str()));
    let is = |k: &str, v: &str| params.get(k) == Some(&json!(v));
    let fits = match name {
        "track.fetch" => {
            has("drive") && only(&["drive", "tracks", "provider"]) && (!has("provider") || is("provider", "tags"))
        }
        "export" => {
            is("format", "m3u") && has("drive") && !has("set") && !has("mix") && only(&["format", "drive", "options"])
        }
        "analyze" => is("analyzer", "size") && has("drive") && only(&["analyzer", "drive", "tracks"]),
        // No artwork provider, and no such action.
        _ => false,
    };
    let taken = fits && !both && !long;
    let params = Value::Object(params);
    let operation = both.then(|| Suggested { name: "playlist".into(), params: json!({ "name": "x" }) });
    let notice = HostNotify {
        message: "m".into(),
        operation,
        action: Some(Suggested { name: name.into(), params: params.clone() }),
    };
    match notice.suggestion(&m) {
        Ok(Some(Suggestion::Action(a))) => {
            assert!(taken, "{a:?} taken");
            assert_eq!(serde_json::to_value(&a).unwrap(), json!({ "name": name, "params": params }));
        }
        Ok(other) => panic!("{other:?}"),
        Err(why) => assert!(!taken && why.starts_with("it suggests "), "{why}"),
    }
}
