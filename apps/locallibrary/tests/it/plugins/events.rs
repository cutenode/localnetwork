//! Plugins' events and services on the command line, in the sidecar and in the `watch` agent:
//! what they hear of, what they tell the user, and where services run.

use serde_json::{Value, json};

use super::echo;
use crate::support::{Env, Rpc};

/// The echo manifest these tests register, granted every event, listening for `on`, and with a
/// service, `beat`.
fn listening(on: &[&str]) -> String {
    let granted = echo().replace(
        r#"capabilities = ["library:read", "library:plan""#,
        r#"capabilities = ["library:read", "library:plan", "events:drives", "events:jobs", "events:plans""#,
    );
    let on: Vec<String> = on.iter().map(|e| format!("{e:?}")).collect();
    let service = "[[contributes.services]]\nname = \"beat\"\ntitle = \"The beat\"\n";
    format!("{granted}\n[[contributes.events]]\non = [{}]\n\n{service}", on.join(", "))
}

/// The payload of each `event` the plugin logged (`echo: event <name> <drive> <payload>`) of
/// `name`, with its drive.
fn heard(err: &str, name: &str) -> Vec<(String, Value)> {
    let said = err.lines().filter_map(|l| l.strip_prefix(&format!("echo: event {name} ")));
    said.map(|rest| {
        let (drive, payload) = rest.split_once(' ').unwrap();
        (drive.to_string(), serde_json::from_str(payload).unwrap())
    })
    .collect()
}

/// A command run once tells the plugins listening of the plans it applies, the plugin's own and
/// locallibrary's, and says what they tell the user back (in their log); it never starts a
/// service, even one turned on.
#[test]
fn a_command_tells_plugins_of_the_plans_it_applies() {
    let mut env = Env::new();
    env.plugins(&[&listening(&["plan.applied"])]);
    env.demo("USB");
    env.ok(&["--yes", "plugin", "settings", "echo", "services.beat", "on"]);
    let run = env.run(&["--yes", "playlist", "fresh", "USB", "--name", "Fresh"]);
    assert_eq!(run.code, 0, "{}", run.err);
    let [(drive, payload)] = &heard(&run.err, "plan.applied")[..] else { panic!("one event: {}", run.err) };
    assert_eq!(
        (drive.as_str(), &payload["operation"], &payload["drives"]),
        ("USB", &json!("echo.playlist"), &json!(["USB"]))
    );
    assert!(run.err.contains("echo: heard plan.applied\n"), "{}", run.err);
    assert!(!run.err.contains("service beat started"), "{}", run.err);
    let run = env.run(&["--yes", "playlist", "new", "USB", "Other"]);
    assert_eq!(heard(&run.err, "plan.applied")[0].1["operation"], "edit", "{}", run.err);
    // Nothing planned, nothing told.
    let run = env.run(&["playlist", "new", "USB", "More", "--dry-run"]);
    assert!(heard(&run.err, "plan.applied").is_empty(), "{}", run.err);
}

/// A service is turned on and off among its plugin's settings, which `plugin info` and `plugin
/// list` show with the events it listens for.
#[test]
fn a_service_is_a_setting_turned_on() {
    let mut env = Env::new();
    env.plugins(&[&listening(&["drive.mounted", "plan.applied"])]);
    let settings = env.json(&["plugin", "settings", "echo"]);
    assert_eq!(settings["settings"], json!({ "greeting": "hello", "services.beat": false }));
    env.ok(&["--yes", "plugin", "settings", "echo", "services.beat", "on"]);
    assert_eq!(
        env.json(&["plugin", "settings", "echo", "services.beat"])["settings"],
        json!({ "services.beat": true })
    );
    let info = env.ok(&["plugin", "info", "echo"]);
    assert!(info.contains("Listens for:\n    drive.mounted\n    plan.applied\n"), "{info}");
    assert!(crate::support::line(&info, "    beat ").ends_with("The beat: on"), "{info}");
    let list = env.ok(&["plugin", "list"]);
    let adds = crate::support::line(&list, "adds commands echo");
    assert!(
        adds.contains("; listens for drive.mounted, plan.applied; ") && adds.contains("; service beat; "),
        "{adds}"
    );
    let listed = env.json(&["plugin", "list"]);
    let echo = listed.as_array().unwrap().iter().find(|p| p["id"] == "echo").unwrap();
    assert_eq!((&echo["events"], &echo["services"]), (&json!(["drive.mounted", "plan.applied"]), &json!(["beat"])));
    env.ok(&["--yes", "plugin", "settings", "echo", "services.beat", "--reset"]);
    assert_eq!(env.json(&["plugin", "settings", "echo"])["settings"]["services.beat"], false);
}

/// The sidecar runs a service once it's turned on, tells plugins of its requests finishing, and
/// sends what they tell the user as `plugin.notice` events.
#[test]
fn the_sidecar_runs_services_and_sends_what_plugins_say() {
    let mut env = Env::new();
    env.plugins(&[&listening(&["job.finished"])]);
    let mut rpc = Rpc::start(env.command());
    rpc.call(1, "events.subscribe", json!({ "kinds": ["plugin"] }));
    let set = json!({ "plugin": "echo", "key": "services.beat", "value": true });
    assert_eq!(rpc.call(2, "plugins.settings.set", set)["result"]["settings"]["services.beat"], true);
    let notice = rpc.event(|e| e["message"] == "beat is running");
    assert_eq!((&notice["event"], &notice["plugin"]), (&json!("plugin.notice"), &json!("echo")));
    rpc.call(3, "drives.list", Value::Null);
    let heard = rpc.event(|e| e["message"] == "heard job.finished");
    assert_eq!(heard["operation"], json!({ "name": "playlist", "params": { "name": "job.finished" } }));
}

/// A service, `suggests`, suggesting each of the host's actions echo has contributions for.
const SUGGESTS: &str = "\n[[contributes.services]]\nname = \"suggests\"\ntitle = \"Suggestions\"\n";

/// The sidecar sends the host's actions a plugin suggests as its `plugin.notice` events'
/// `action`, which a front end starts with the sidecar's own methods, filled in from it: what the
/// providers find, the export's plan (to where the user picks) and the analysis.
#[test]
fn the_sidecar_sends_the_actions_plugins_suggest() {
    let mut env = Env::new();
    env.plugins(&[&(echo() + SUGGESTS)]);
    env.demo("USB");
    let mut rpc = Rpc::start(env.command());
    rpc.call(1, "events.subscribe", json!({ "kinds": ["plugin"] }));
    rpc.call(2, "plugins.settings.set", json!({ "plugin": "echo", "key": "services.suggests", "value": true }));
    let action = |rpc: &mut Rpc, message: &str| rpc.event(|e| e["message"] == message)["action"].clone();
    let fetch = action(&mut rpc, "fetch track 1's tags");
    let tags = json!({ "drive": "USB", "tracks": [1], "provider": "tags" });
    assert_eq!(fetch, json!({ "name": "track.fetch", "params": tags }));
    let export = action(&mut rpc, "export USB");
    assert_eq!(export, json!({ "name": "export", "params": { "format": "echo-m3u", "drive": "USB" } }));
    let analyze = action(&mut rpc, "analyze USB");
    assert_eq!(analyze, json!({ "name": "analyze", "params": { "analyzer": "size", "drive": "USB" } }));

    let p = &fetch["params"];
    let provide = json!({ "kind": "metadata", "drive": p["drive"], "tracks": { "ids": p["tracks"] }, "from": "echo.tags", "replace": true });
    assert_eq!(rpc.call(3, "plugins.provide", provide)["result"]["asked"], 1);
    let (p, out) = (&export["params"], env.store.with_file_name("USB.m3u"));
    let plan = json!({ "plugin": "echo", "exporter": p["format"], "drive": p["drive"], "output": out });
    let planned = rpc.call(4, "plugins.export.plan", plan);
    assert_eq!(planned["result"]["files"][0]["path"], json!(out), "{planned}");
    let p = &analyze["params"];
    let analyzed = json!({ "plugin": "echo", "analyzer": p["analyzer"], "drive": p["drive"] });
    assert_eq!(rpc.call(5, "plugins.analyze", analyzed)["result"]["analyzed"], 2);
}

/// The `watch` agent tells plugins what it does, runs their services, says what they tell the
/// user and keeps what they suggest for the player, operations and the host's actions alike.
#[cfg(unix)]
#[test]
fn the_watch_agent_tells_plugins_and_keeps_their_suggestions() {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    use std::time::Duration;

    use crate::support::play;

    let mut env = Env::new();
    env.plugins(&[&(listening(&["drive.pulled"]) + SUGGESTS)]);
    let main = env.pulled("Main");
    play(&main, "HISTORY 002", &[]);
    env.ok(&["--yes", "plugin", "settings", "echo", "services.beat", "on"]);
    env.ok(&["--yes", "plugin", "settings", "echo", "services.suggests", "on"]);
    let mut child = env.command().args(["watch", "--pull", "--settle", "0"]).stdout(Stdio::piped()).spawn().unwrap();
    let (tx, lines) = std::sync::mpsc::channel();
    let stdout = child.stdout.take().unwrap();
    std::thread::spawn(move || BufReader::new(stdout).lines().for_each(|l| drop(tx.send(l.unwrap()))));
    // All, in any order.
    let wanted = [
        " echo: beat is running",
        " echo: heard drive.pulled (the player offers to review it)",
        " echo: analyze USB (the player offers to review it)",
    ];
    let mut said: Vec<String> = Vec::new();
    while !wanted.iter().all(|w| said.iter().any(|l| l.contains(w))) {
        let l = lines.recv_timeout(Duration::from_secs(60)).unwrap_or_else(|_| panic!("watch said {said:?}"));
        said.push(l);
    }
    let _ = child.kill();
    let _ = child.wait();
    let kept: Value =
        serde_json::from_slice(&std::fs::read(env.store.join("plugins/suggested.json")).unwrap()).unwrap();
    let kept = kept.as_array().unwrap();
    assert!(kept.iter().any(|k| k["operation"]["params"] == json!({ "name": "drive.pulled" })), "{kept:?}");
    let analyze = json!({ "name": "analyze", "params": { "analyzer": "size", "drive": "USB" } });
    assert!(kept.iter().any(|k| k["message"] == "analyze USB" && k["action"] == analyze), "{kept:?}");
}
