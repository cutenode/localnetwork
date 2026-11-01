//! Services (`service/start`): run once the user turns them on, kept running past the idle
//! reaper, started again when they stop, and left off once they've failed too often.

use std::time::Duration;

use locallibrary::plugin::{Options, PluginNotice};

use super::*;

/// The echo plugin's manifest with a service named `name`.
fn serving(name: &str) -> String {
    format!("{ECHO}\n[[contributes.services]]\nname = \"{name}\"\ntitle = \"The {name}\"\n")
}

/// The plugin serving `name`, and what it tells the user.
fn service(name: &str, options: Options) -> (Fixture, Arc<Mutex<Vec<PluginNotice>>>) {
    let notices = Arc::new(Mutex::new(Vec::new()));
    let kept = notices.clone();
    let options = Options { notices: Arc::new(move |n| kept.lock().unwrap().push(n)), ..options };
    let manifest = serving(name);
    (Fixture::new(|dir| vec![spec(dir, &manifest, &[])], options), notices)
}

fn started(f: &Fixture, name: &str) -> usize {
    let line = format!("service {name} started");
    f.logs.lock().unwrap().iter().filter(|(_, l)| *l == line).count()
}

/// A service is a switch among its plugin's settings, off until it's turned on; turned on, it
/// runs, telling the user, and keeps its plugin from being stopped as idle until it's turned
/// off again. After the switches, each command's and operation's key in the player is a setting
/// too, text and unset until it's set. Hooks are given neither.
#[test]
fn a_service_runs_once_its_turned_on_and_keeps_its_plugin() {
    let options = Options { idle: Duration::from_millis(100), ..Options::default() };
    let (f, notices) = service("beat", options);
    let keys: Vec<String> = f.plugins.setting_list("echo").unwrap().into_iter().map(|s| s.key).collect();
    let commands = ["keys.echo", "keys.titles", "keys.steps", "keys.wait", "keys.playlist"];
    assert_eq!(keys, [&["greeting", "services.beat"][..], &commands].concat());
    let key = f.plugins.set_setting("echo", &f.store, "keys.titles", Some(json!("%"))).unwrap();
    assert_eq!((key.get("keys.titles"), key.get("keys.steps")), (Some(&json!("%")), None));
    let e = f.plugins.set_setting("echo", &f.store, "keys.titles", Some(json!(1))).unwrap_err();
    assert_eq!(e.to_string(), "keys.titles is a string, not 1");
    assert_eq!(f.plugins.settings("echo", Some(&f.store)).unwrap()["services.beat"], false);
    f.plugins.run_services(Some(&f.store));
    assert!(!f.plugins.service_running("echo", "beat") && !f.plugins.is_running("echo"), "it's off");
    let settings = f.plugins.set_setting("echo", &f.store, "services.beat", Some(json!(true))).unwrap();
    assert_eq!(settings["services.beat"], true);
    let e = f.plugins.set_setting("echo", &f.store, "services.beat", Some(json!("on"))).unwrap_err();
    assert_eq!(e.to_string(), "services.beat is a boolean, not \"on\"");
    // Hooks aren't given it.
    let context = f.echo_params(json!({ "context": true })).unwrap();
    assert_eq!(context["settings"], json!({ "greeting": "hello" }));
    f.plugins.run_services(Some(&f.store));
    f.plugins.run_services(Some(&f.store));
    eventually("it tells the user", || !notices.lock().unwrap().is_empty());
    assert_eq!(notices.lock().unwrap()[0].message, "beat is running");
    std::thread::sleep(Duration::from_millis(400));
    assert!(f.plugins.is_running("echo"), "idle, the service keeps it running");
    assert_eq!(started(&f, "beat"), 1, "started once");
    assert!(f.plugins.service_running("echo", "beat"));
    f.plugins.set_setting("echo", &f.store, "services.beat", None).unwrap();
    f.plugins.run_services(Some(&f.store));
    assert!(!f.plugins.service_running("echo", "beat"));
    eventually("it's stopped as idle", || !f.plugins.is_running("echo"));
}

/// A service that stops by itself, or crashes its plugin, is started again a while later, and
/// left off once it has failed too often in a row, saying so.
#[test]
fn a_failing_service_is_started_again_then_left_off() {
    for (name, why) in [("crash", "it stopped unexpectedly"), ("quits", "it stopped by itself")] {
        let options = Options { restarts: 2, backoff: Duration::from_millis(10), ..Options::default() };
        let (f, notices) = service(name, options);
        f.plugins.set_setting("echo", &f.store, &format!("services.{name}"), Some(json!(true))).unwrap();
        f.plugins.run_services(Some(&f.store));
        eventually("it's left off", || !notices.lock().unwrap().is_empty());
        let notice = notices.lock().unwrap()[0].clone();
        assert_eq!(notice.plugin, "echo");
        assert!(notice.message.starts_with(&format!("its {name} service is off: it failed 3 times in a row (")));
        assert!(notice.message.contains(why), "{name}");
        eventually("it's no longer running", || !f.plugins.service_running("echo", name));
        eventually("it was started three times", || started(&f, name) == 3);
        // Run again, it's started again.
        f.plugins.run_services(Some(&f.store));
        assert!(f.plugins.service_running("echo", name));
        f.plugins.stop_services();
        assert!(!f.plugins.service_running("echo", name));
    }
}

/// Shutting down stops the services and their plugins; they aren't started again until they're
/// run again.
#[test]
fn shutting_down_stops_the_services() {
    let (f, notices) = service("beat", Options::default());
    f.plugins.set_setting("echo", &f.store, "services.beat", Some(json!(true))).unwrap();
    f.plugins.run_services(Some(&f.store));
    eventually("it runs", || !notices.lock().unwrap().is_empty());
    f.plugins.shutdown();
    assert!(!f.plugins.is_running("echo") && !f.plugins.service_running("echo", "beat"));
}

/// For any turning on and off of a service, run each time, it runs exactly when it was last
/// turned on.
#[hegel::test(test_cases = 4, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_service_runs_exactly_when_its_turned_on(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (f, _) = service("beat", Options::default());
    let switches: Vec<Option<bool>> = tc.draw(gs::vecs(gs::optional(gs::booleans())).min_size(1).max_size(5));
    for on in switches {
        f.plugins.set_setting("echo", &f.store, "services.beat", on.map(Value::Bool)).unwrap();
        f.plugins.run_services(Some(&f.store));
        assert_eq!(f.plugins.service_running("echo", "beat"), on == Some(true));
    }
    f.plugins.stop_services();
    assert!(!f.plugins.service_running("echo", "beat"));
}
