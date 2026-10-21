//! A plugin's events (`event/notify`), the services it runs (`service/start`) and what it tells
//! the user (`host/notify`).

use super::*;
use crate::{EventName, EventNotify, HostNotify, StartService, Suggested};

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
        host.notify(&HostNotify { message, operation });
        Ok(())
    });
    let s = start(hooks);
    let event =
        |drive: &str| EventNotify { event: EventName::DriveMounted, drive: Some(drive.into()), payload: json!({}) };
    s.host.notify(method::EVENT_NOTIFY, &event("early"));
    let reply: HelloReply = s.host.request(method::HELLO, &hello(PROTOCOL_VERSION, &[])).unwrap();
    assert_eq!(reply.features, ["accounts", "events", "blobs", "net", "choose"]);
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
            host.notify(&HostNotify { message: "next".into(), operation: None });
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
        host.notify(&HostNotify { message: format!("{} started", req.service), operation: None });
        loop {
            host.ctx().check()?;
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    let s = start_with(with_service(), Hooks::new().on(service));
    let reply: HelloReply = s.host.request(method::HELLO, &hello(PROTOCOL_VERSION, &[])).unwrap();
    assert_eq!(reply.features, ["accounts", "services", "blobs", "net", "choose"]);
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
