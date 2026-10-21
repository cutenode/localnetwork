//! Events (`event/notify`) told to the plugins listening for them, and what they tell the user
//! back (`host/notify`).

use std::time::{Duration, Instant};

use locallibrary::api::{JobEvent, Notice, WatchAction};
use locallibrary::plugin::{Event, Options, PluginNotice, keep_suggested, take_suggested};
use locallibrary::watch::{MountedDrive, WatchEvent};
use loclib_plugin::{EventName, Suggested};
use serde_json::json;

use super::*;

/// The echo plugin's manifest, listening for `on` (granted).
fn listening(on: &[&str]) -> String {
    let events: Vec<String> = on.iter().map(|e| format!("{e:?}")).collect();
    format!(
        "{}\n[[contributes.events]]\non = [{}]\n",
        granting(&["events:drives", "events:jobs", "events:plans"]),
        events.join(", ")
    )
}

/// The plugins `specs` makes, and what they tell the user.
fn told(specs: impl FnOnce(&Path) -> Vec<PluginSpec>, options: Options) -> (Fixture, Arc<Mutex<Vec<PluginNotice>>>) {
    let notices = Arc::new(Mutex::new(Vec::new()));
    let kept = notices.clone();
    let options = Options { notices: Arc::new(move |n| kept.lock().unwrap().push(n)), ..options };
    (Fixture::new(specs, options), notices)
}

/// The fixture drive, as the watcher describes it.
fn mounted(f: &Fixture) -> MountedDrive {
    let id = locallibrary::Device::open(&f.drive).unwrap().id();
    MountedDrive { root: f.drive.clone(), id, name: "USB".into(), has_export: true }
}

/// What the plugin logged of the events it was told of: each one's name, drive and payload.
fn heard(f: &Fixture) -> Vec<(String, String, Value)> {
    let logs = f.logs.lock().unwrap();
    let heard = logs.iter().filter_map(|(_, l)| {
        let mut said = l.strip_prefix("event ")?.splitn(3, ' ');
        let (name, drive) = (said.next()?.to_string(), said.next()?.to_string());
        Some((name, drive, serde_json::from_str(said.next()?).ok()?))
    });
    heard.collect()
}

/// A plugin is told of an event it listens for, by the drive's id and without where the drive
/// is, and tells the user it heard it, suggesting one of its operations; an event it doesn't
/// listen for isn't sent, and a plugin listening for nothing isn't started.
#[test]
fn a_plugin_is_told_of_what_it_listens_for() {
    let (f, notices) = told(
        |dir| {
            let other = ECHO.replace("id = \"echo\"", "id = \"other\"");
            vec![spec(dir, &listening(&["drive.mounted", "job.finished"]), &[]), spec(dir, &other, &[])]
        },
        Options::default(),
    );
    let drive = mounted(&f);
    assert!(f.plugins.listens(EventName::DriveMounted));
    assert!(!f.plugins.listens(EventName::DriveUnmounted));
    for notice in
        [Notice::Watch(WatchEvent::Detached(drive.clone())), Notice::Watch(WatchEvent::Attached(drive.clone()))]
    {
        for event in Event::of(&notice) {
            f.plugins.deliver(&event, Some(&f.store));
        }
    }
    let finished = JobEvent::Finished { id: json!(7), method: "sync.apply".into(), ok: true, kind: None };
    for event in Event::of(&Notice::Job(finished)) {
        f.plugins.deliver(&event, Some(&f.store));
    }
    eventually("it hears both", || heard(&f).len() == 2);
    let id = drive.id.unwrap().to_string();
    let said = heard(&f);
    let payload = json!({ "id": id, "name": "USB", "has_export": true });
    assert_eq!(said[0], ("drive.mounted".into(), id.clone(), payload));
    assert_eq!(said[1], ("job.finished".into(), "-".into(), json!({ "id": 7, "method": "sync.apply", "ok": true })));
    eventually("it tells the user", || notices.lock().unwrap().len() == 2);
    let suggested = Suggested { name: "playlist".into(), params: json!({ "name": "drive.mounted" }) };
    let first =
        PluginNotice { plugin: "echo".into(), message: "heard drive.mounted".into(), operation: Some(suggested) };
    assert_eq!(notices.lock().unwrap()[0], first);
    assert!(!f.plugins.is_running("other"), "listening for nothing, it isn't started");
}

/// A drive pulled is `drive.pulled`, and one back from a set `set.returned` too; a plan applied
/// is `plan.applied`, on the drive it wrote. What else the sidecar says, plugins aren't told.
#[test]
fn events_are_what_the_host_says_happens() {
    let drive = MountedDrive { root: "/Volumes/USB".into(), id: None, name: "USB".into(), has_export: true };
    let changed = WatchEvent::Changed { drive: drive.clone(), changed: vec![locallibrary::watch::DriveChange::State] };
    let names = |n: Notice| Event::of(&n).into_iter().map(|e| (e.name, e.drive)).collect::<Vec<_>>();
    assert_eq!(names(Notice::Watch(changed)), [(EventName::DriveChanged, Some("USB".into()))]);
    let store = WatchEvent::StoreChanged { root: "/store".into(), changed: vec!["sets"] };
    assert_eq!(names(Notice::Watch(store)), []);
    let waiting = WatchAction::Waiting { drive: drive.clone(), reason: "locked".into() };
    assert_eq!(names(Notice::Watched(waiting)), []);
    let started = JobEvent::Started { id: json!(1), method: "drives.list".into(), started: chrono::Utc::now() };
    assert_eq!(names(Notice::Job(started)), []);
}

/// Once too many events wait for a plugin the oldest are dropped, saying so, and adding one never
/// waits for the plugin, even one slow to start: the one it's sending goes once it has, then the
/// newest that waited.
#[test]
fn a_plugins_events_wait_in_a_queue_of_their_own() {
    let mut go = PathBuf::new();
    let specs = |dir: &Path| {
        go = dir.join("go");
        vec![spec(dir, &listening(&["plan.applied"]), &["--paused", &go.display().to_string()])]
    };
    let (f, _) = told(specs, Options { events: 2, ..Options::default() });
    let event = |n: usize| Event { name: EventName::PlanApplied, drive: None, payload: json!({ "n": n }) };
    let said = |what: &str| f.logs.lock().unwrap().iter().filter(|(_, l)| l.starts_with(what)).count();
    f.plugins.deliver(&event(0), Some(&f.store));
    eventually("it's sending the first, to the plugin starting", || said("paused") == 1);
    let started = Instant::now();
    for n in 1..5 {
        f.plugins.deliver(&event(n), Some(&f.store));
    }
    assert!(started.elapsed() < Duration::from_millis(150), "it doesn't wait for the plugin");
    assert_eq!(said("dropped a plan.applied event: 2 were waiting"), 2);
    std::fs::write(&go, "").unwrap();
    eventually("it hears the rest", || heard(&f).len() == 3);
    let sent: Vec<Value> = heard(&f).into_iter().map(|(_, _, payload)| payload["n"].clone()).collect();
    assert_eq!(sent, [0, 3, 4]);
}

/// Shutting down waits a while for the events left to be sent, then drops them, saying so: the
/// plugin isn't started again for them once it has stopped. One that can't be sent is logged.
#[test]
fn shutting_down_drops_the_events_left_waiting() {
    let options =
        Options { handshake: Duration::from_millis(300), grace: Duration::from_millis(100), ..Options::default() };
    let specs = |dir: &Path| {
        let never = dir.join("never").display().to_string();
        vec![spec(dir, &listening(&["plan.applied"]), &["--paused", &never])]
    };
    let (f, _) = told(specs, options);
    let event = Event { name: EventName::PlanApplied, drive: None, payload: json!({}) };
    for _ in 0..3 {
        f.plugins.deliver(&event, Some(&f.store));
    }
    f.plugins.shutdown();
    let said = |what: &str| f.logs.lock().unwrap().iter().filter(|(_, l)| l.starts_with(what)).count();
    let started = said("paused");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(said("paused"), started, "it isn't started again");
    assert!(!f.plugins.is_running("echo"));
    let logs = f.logs.lock().unwrap().clone();
    let dropped: Vec<&str> = logs.iter().filter_map(|(_, l)| l.strip_prefix("it's stopping: ")).collect();
    let [dropped] = dropped[..] else { panic!("dropped {dropped:?}") };
    assert!(dropped.ends_with(" of its events waiting weren't sent"), "{dropped}");
    assert!(said("the plan.applied event wasn't sent: ") >= 1);
}

/// What a plugin tells the user is a line of text, cut short past a few hundred characters; an
/// empty one isn't told, and a suggestion with params that aren't an object, or that are too
/// long, is left out of it.
#[test]
fn what_a_plugin_tells_the_user_is_a_short_line() {
    let (f, notices) = told(|dir| vec![spec(dir, &listening(&["plan.applied"]), &[])], Options::default());
    let suggests = |message: &str, params: Value| json!({ "message": message, "operation": { "name": "playlist", "params": params } });
    let tells = [
        json!({ "message": " " }),
        suggests("not an object", json!([1])),
        suggests("too long", json!({ "name": "x".repeat(70 << 10) })),
        json!({ "message": format!("a\u{1b}[31mred\nline {}", "x".repeat(600)) }),
    ];
    for tell in tells {
        let event = Event { name: EventName::PlanApplied, drive: None, payload: json!({ "tell": tell }) };
        f.plugins.deliver(&event, Some(&f.store));
    }
    eventually("it tells the user", || notices.lock().unwrap().len() == 3);
    let notices = notices.lock().unwrap().clone();
    let said: Vec<(&str, bool)> = notices[..2].iter().map(|n| (n.message.as_str(), n.operation.is_some())).collect();
    assert_eq!(said, [("not an object", false), ("too long", false)]);
    assert!(notices[2].message.starts_with("aredline xxx"));
    assert_eq!(notices[2].message.chars().count(), 500);
    let logs = f.logs.lock().unwrap().clone();
    for refused in [
        "it tells the user nothing",
        "it suggests \"playlist\" with params that aren't an object",
        "it suggests \"playlist\" with params longer than 65536 bytes",
    ] {
        assert!(logs.iter().any(|(_, l)| l == refused), "{refused}");
    }
}

/// What a plugin tells the user is suggestion only when it's one of its operations; the watch
/// agent keeps suggestions for the player, which takes them once.
#[test]
fn a_suggestion_is_one_of_the_plugins_operations() {
    let manifest = listening(&["plan.applied"]).replace("name = \"playlist\"", "name = \"other\"");
    let (f, notices) = told(|dir| vec![spec(dir, &manifest, &[])], Options::default());
    let event = Event { name: EventName::PlanApplied, drive: Some("USB".into()), payload: json!({}) };
    f.plugins.deliver(&event, Some(&f.store));
    eventually("it tells the user", || !notices.lock().unwrap().is_empty());
    let notice = notices.lock().unwrap()[0].clone();
    assert_eq!((notice.message.as_str(), notice.operation.is_none()), ("heard plan.applied", true));
    let logs = f.logs.lock().unwrap().clone();
    assert!(logs.iter().any(|(_, l)| l == "it suggests \"playlist\", which isn't one of its operations"));
    assert_eq!(take_suggested(&f.store).unwrap(), []);
    let suggested = Suggested { name: "playlist".into(), params: json!({}) };
    for n in 0..locallibrary::plugin::MAX_SUGGESTED + 2 {
        let kept = PluginNotice { message: n.to_string(), operation: Some(suggested.clone()), ..notice.clone() };
        keep_suggested(&f.store, &kept).unwrap();
    }
    let taken = take_suggested(&f.store).unwrap();
    assert_eq!(taken.len(), locallibrary::plugin::MAX_SUGGESTED);
    assert_eq!(taken[0].message, "2", "the oldest go");
    assert_eq!(take_suggested(&f.store).unwrap(), [], "taken once");
}

/// Shutting down sends the events still waiting first, as a command run once needs.
#[test]
fn shutting_down_sends_the_events_waiting() {
    let (f, _) = told(|dir| vec![spec(dir, &listening(&["plan.applied"]), &[])], Options::default());
    let event = Event { name: EventName::PlanApplied, drive: None, payload: json!({ "operation": "edit" }) };
    f.plugins.deliver(&event, Some(&f.store));
    f.plugins.shutdown();
    assert!(!f.plugins.is_running("echo"));
    eventually("it hears it", || !heard(&f).is_empty());
    assert_eq!(heard(&f), [("plan.applied".into(), "-".into(), json!({ "operation": "edit" }))]);
}

/// For any events, a plugin listening for some of them hears exactly those, in the order they
/// happened, each with its drive and payload.
#[hegel::test(test_cases = 5, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_plugin_hears_what_it_listens_for_in_order(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let on: Vec<EventName> = tc.draw(gs::subsequences(EventName::ALL.to_vec()).min_size(1));
    let names: Vec<&str> = on.iter().map(|e| e.name()).collect();
    let (f, _) = told(|dir| vec![spec(dir, &listening(&names), &[])], Options::default());
    let drawn = gs::tuples!(gs::sampled_from(EventName::ALL.to_vec()), gs::optional(gs::from_regex("[A-Z]{1,4}")));
    let events: Vec<(EventName, Option<String>)> = tc.draw(gs::vecs(drawn).max_size(8));
    for (i, (name, drive)) in events.iter().enumerate() {
        let event = Event { name: *name, drive: drive.clone(), payload: json!({ "n": i }) };
        f.plugins.deliver(&event, Some(&f.store));
    }
    let expected: Vec<(String, String, Value)> = events
        .iter()
        .enumerate()
        .filter(|(_, (name, _))| on.contains(name))
        .map(|(i, (name, drive))| (name.name().into(), drive.clone().unwrap_or("-".into()), json!({ "n": i })))
        .collect();
    f.plugins.shutdown();
    eventually("it hears them all", || heard(&f).len() == expected.len());
    assert_eq!(heard(&f), expected);
}

/// For any suggestions kept, the player takes the newest [`MAX_SUGGESTED`], oldest first.
#[hegel::test(test_cases = 20)]
fn the_newest_suggestions_are_kept(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use locallibrary::plugin::MAX_SUGGESTED;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path()).unwrap();
    let messages: Vec<String> = tc.draw(gs::vecs(gs::text().max_size(8)).max_size(MAX_SUGGESTED * 2));
    let operation = Some(Suggested { name: "playlist".into(), params: json!({}) });
    for message in &messages {
        let notice = PluginNotice { plugin: "echo".into(), message: message.clone(), operation: operation.clone() };
        keep_suggested(&store, &notice).unwrap();
    }
    let taken: Vec<String> = take_suggested(&store).unwrap().into_iter().map(|n| n.message).collect();
    assert_eq!(taken, messages[messages.len().saturating_sub(MAX_SUGGESTED)..]);
}
