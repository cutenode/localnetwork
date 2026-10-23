//! Events (`event/notify`) told to the plugins listening for them, and what they tell the user
//! back (`host/notify`).

use std::time::{Duration, Instant};

use locallibrary::api::{JobEvent, Notice, WatchAction};
use locallibrary::plugin::{Event, Options, PluginNotice, forget_suggested, keep_suggested, suggested};
use locallibrary::watch::{MountedDrive, WatchEvent};
use loclib_plugin::{
    AnalyzeAction, EventName, ExportAction, FetchAction, HostAction, HostNotify, Suggested, Suggestion,
};
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
    let first = PluginNotice {
        plugin: "echo".into(),
        message: "heard drive.mounted".into(),
        suggests: Some(Suggestion::Operation(suggested)),
    };
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
    let said: Vec<(&str, bool)> = notices[..2].iter().map(|n| (n.message.as_str(), n.suggests.is_some())).collect();
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

/// What a plugin tells the user is suggestion only when it's one of its operations; the player
/// and the watch agent keep suggestions until each is reviewed or dismissed, the newest few. A
/// file of them that can't be read is left as it is.
#[test]
fn a_suggestion_is_one_of_the_plugins_operations() {
    let manifest = listening(&["plan.applied"]).replace("name = \"playlist\"", "name = \"other\"");
    let (f, notices) = told(|dir| vec![spec(dir, &manifest, &[])], Options::default());
    let event = Event { name: EventName::PlanApplied, drive: Some("USB".into()), payload: json!({}) };
    f.plugins.deliver(&event, Some(&f.store));
    eventually("it tells the user", || !notices.lock().unwrap().is_empty());
    let notice = notices.lock().unwrap()[0].clone();
    assert_eq!((notice.message.as_str(), notice.suggests.is_none()), ("heard plan.applied", true));
    let logs = f.logs.lock().unwrap().clone();
    assert!(logs.iter().any(|(_, l)| l == "it suggests \"playlist\", which isn't one of its operations"));
    assert_eq!(suggested(&f.store).unwrap(), []);
    let suggests = Suggested { name: "playlist".into(), params: json!({}) };
    let kept = |n: usize| PluginNotice {
        message: n.to_string(),
        suggests: Some(Suggestion::Operation(suggests.clone())),
        ..notice.clone()
    };
    for n in 0..locallibrary::plugin::MAX_SUGGESTED + 2 {
        keep_suggested(&f.store, &kept(n)).unwrap();
    }
    let all = suggested(&f.store).unwrap();
    assert_eq!(all.len(), locallibrary::plugin::MAX_SUGGESTED);
    assert_eq!(all[0].message, "2", "the oldest go");
    assert_eq!(suggested(&f.store).unwrap(), all, "kept until reviewed or dismissed");
    let left = forget_suggested(&f.store, &kept(5)).unwrap();
    assert_eq!(left, [&all[..3], &all[4..]].concat());
    assert_eq!(forget_suggested(&f.store, &kept(0)).unwrap(), left, "one not kept changes nothing");
    let file = f.store.root().join("plugins/suggested.json");
    std::fs::write(&file, "not json").unwrap();
    assert!(suggested(&f.store).is_err() && keep_suggested(&f.store, &kept(1)).is_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "not json");
}

/// The host's actions the echo plugin may suggest and some it may not (fetching cover art it has
/// no provider of, another plugin's export format, an analyzer's with params too long, and an
/// action with an operation too), each as `host/notify` says it, with what the host takes of it.
fn actions() -> Vec<(Value, Option<HostAction>)> {
    let fetch = FetchAction { drive: "USB".into(), tracks: vec![1], provider: Some("tags".into()) };
    let export = ExportAction { format: "m3u".into(), drive: Some("USB".into()), ..ExportAction::default() };
    let analyze = AnalyzeAction { analyzer: "size".into(), drive: "USB".into(), tracks: Vec::new() };
    let taken = [HostAction::TrackFetch(fetch), HostAction::Export(export), HostAction::Analyze(analyze)];
    let taken = taken.into_iter().map(|a| (serde_json::to_value(HostNotify::action("taken", &a)).unwrap(), Some(a)));
    let action =
        |name: &str, params: Value| json!({ "message": "refused", "action": { "name": name, "params": params } });
    let mut both = action("analyze", json!({ "analyzer": "size", "drive": "USB" }));
    both["operation"] = json!({ "name": "playlist", "params": { "name": "x" } });
    let refused = [
        action("art.fetch", json!({ "drive": "USB" })),
        action("export", json!({ "format": "xml", "drive": "USB" })),
        action("analyze", json!({ "analyzer": "size", "drive": "x".repeat(70 << 10) })),
        both,
    ];
    taken.chain(refused.into_iter().map(|r| (r, None))).collect()
}

/// A plugin may suggest one of the host's actions on its own contributions instead of an
/// operation: the front end is told it, and it's kept for the player as it is until it's
/// reviewed or dismissed. One it may not suggest is left out, logged, and the line still told.
#[test]
fn a_plugin_may_suggest_the_hosts_actions() {
    let (f, notices) = told(|dir| vec![spec(dir, &listening(&["plan.applied"]), &[])], Options::default());
    let actions = actions();
    for (tell, _) in &actions {
        let event = Event { name: EventName::PlanApplied, drive: None, payload: json!({ "tell": tell }) };
        f.plugins.deliver(&event, Some(&f.store));
    }
    eventually("it tells the user", || notices.lock().unwrap().len() == actions.len());
    let notices = notices.lock().unwrap().clone();
    let said: Vec<(&str, Option<Suggestion>)> =
        notices.iter().map(|n| (n.message.as_str(), n.suggests.clone())).collect();
    let expected: Vec<(&str, Option<Suggestion>)> = actions
        .iter()
        .map(|(_, a)| (if a.is_some() { "taken" } else { "refused" }, a.clone().map(Suggestion::Action)))
        .collect();
    assert_eq!(said, expected);
    let logs = f.logs.lock().unwrap().clone();
    for refused in [
        "it suggests the action \"art.fetch\", which it can't: no artwork provider",
        "it suggests the action \"export\", which it can't: no exporter \"xml\"",
        "it suggests the action \"analyze\" with params longer than 65536 bytes",
        "it suggests both an operation and an action",
    ] {
        assert!(logs.iter().any(|(_, l)| l == refused), "{refused}");
    }
    keep_suggested(&f.store, &notices[0]).unwrap();
    assert_eq!(suggested(&f.store).unwrap(), [notices[0].clone()], "kept as it is");
    assert_eq!(forget_suggested(&f.store, &notices[0]).unwrap(), []);
}

/// For any suggestions a plugin makes, of actions it may suggest or not, the front end is told
/// each line in order, with exactly the actions it may suggest: none it may not ever reaches it,
/// so none can be planned.
#[hegel::test(test_cases = 5, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn only_the_actions_a_plugin_may_suggest_reach_the_front_end(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let all = actions();
    let picked: Vec<usize> = tc.draw(gs::vecs(gs::integers::<usize>().max_value(all.len() - 1)).max_size(8));
    let (f, notices) = told(|dir| vec![spec(dir, &listening(&["plan.applied"]), &[])], Options::default());
    for &i in &picked {
        let event = Event { name: EventName::PlanApplied, drive: None, payload: json!({ "tell": all[i].0 }) };
        f.plugins.deliver(&event, Some(&f.store));
    }
    eventually("it tells the user", || notices.lock().unwrap().len() == picked.len());
    let got: Vec<Option<Suggestion>> = notices.lock().unwrap().iter().map(|n| n.suggests.clone()).collect();
    let expected: Vec<Option<Suggestion>> = picked.iter().map(|&i| all[i].1.clone().map(Suggestion::Action)).collect();
    assert_eq!(got, expected);
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

/// For any suggestions kept and forgotten, what's kept is the newest [`MAX_SUGGESTED`] kept,
/// oldest first, without each forgotten (the oldest the same as it).
#[hegel::test(test_cases = 20)]
fn the_newest_suggestions_are_kept_until_forgotten(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use locallibrary::plugin::MAX_SUGGESTED;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path()).unwrap();
    let changes: Vec<(bool, String)> = tc.draw(
        gs::vecs(gs::tuples!(gs::booleans(), gs::sampled_from(vec![String::new(), "a".into(), "b".into()])))
            .max_size(MAX_SUGGESTED * 3),
    );
    let operation = Some(Suggestion::Operation(Suggested { name: "playlist".into(), params: json!({}) }));
    let mut expected: Vec<PluginNotice> = Vec::new();
    for (keep, message) in changes {
        let notice = PluginNotice { plugin: "echo".into(), message, suggests: operation.clone() };
        let now = match keep {
            true => {
                expected.push(notice.clone());
                expected.drain(..expected.len().saturating_sub(MAX_SUGGESTED));
                keep_suggested(&store, &notice).unwrap()
            }
            false => {
                if let Some(i) = expected.iter().position(|n| *n == notice) {
                    expected.remove(i);
                }
                forget_suggested(&store, &notice).unwrap()
            }
        };
        assert_eq!(now, expected);
    }
    assert_eq!(suggested(&store).unwrap(), expected);
}

/// Where [`suggestions_kept_at_once_by_two_processes_are_all_kept`] runs as one of the two:
/// the store, then what this one's suggestions start with.
const KEEPER: &str = "LOCALLIBRARY_TEST_SUGGESTION_KEEPER";

/// Two processes (the player and the `watch` agent, say) keeping suggestions at the same time
/// lose none of them: each keeps its own, all at once, and every one is kept.
#[test]
fn suggestions_kept_at_once_by_two_processes_are_all_kept() {
    const EACH: usize = locallibrary::plugin::MAX_SUGGESTED / 2;
    let notice = |message: String| PluginNotice {
        plugin: "echo".into(),
        message,
        suggests: Some(Suggestion::Operation(Suggested { name: "playlist".into(), params: json!({}) })),
    };
    if let Some(keeper) = std::env::var_os(KEEPER) {
        let keeper = keeper.into_string().unwrap();
        let (root, tag) = keeper.rsplit_once('\n').unwrap();
        let store = Store::open(Path::new(root)).unwrap();
        while !Path::new(root).join("go").exists() {
            std::thread::sleep(Duration::from_millis(1));
        }
        for n in 0..EACH {
            keep_suggested(&store, &notice(format!("{tag} {n}"))).unwrap();
        }
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path()).unwrap();
    let name = "plugins::events::suggestions_kept_at_once_by_two_processes_are_all_kept";
    let keepers: Vec<std::process::Child> = ["a", "b"]
        .into_iter()
        .map(|tag| {
            let keeper = format!("{}\n{tag}", dir.path().display());
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([name, "--exact", "--nocapture"])
                .env(KEEPER, keeper)
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    std::fs::write(dir.path().join("go"), "").unwrap();
    for mut keeper in keepers {
        assert!(keeper.wait().unwrap().success(), "a keeper failed");
    }
    let mut kept: Vec<String> = suggested(&store).unwrap().into_iter().map(|n| n.message).collect();
    kept.sort();
    let mut expected: Vec<String> =
        ["a", "b"].iter().flat_map(|t| (0..EACH).map(move |n| format!("{t} {n}"))).collect();
    expected.sort();
    assert_eq!(kept, expected, "none lost");
}
