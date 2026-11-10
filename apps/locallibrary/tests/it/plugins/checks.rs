//! Plugins' checks in `check` and `doctor`, their fixes run as the operations they are, guards on
//! the plans the command line and the sidecar make, and `plugin checks` (`plugins.checks*` over
//! RPC), with the test plugin's check `found` and guard `guard` answering as its `findings`
//! setting says (see the plugin host's test plugin).

use hegel::TestCase;
use hegel::generators as gs;
use serde_json::{Value, json};

use crate::support::{Env, Rpc, echo_plugin, path};

/// The echo manifest with a check `found` and a guard `guard` besides its own, each answering as
/// its `findings` setting says.
fn checking() -> String {
    let more = "[[contributes.checks]]\nname = \"found\"\ntitle = \"Found\"\n\n\
                [[contributes.checks]]\nname = \"guard\"\ntitle = \"Guard\"\nwhen = [\"apply\"]\n\n\
                [[contributes.settings]]\nkey = \"findings\"\ntitle = \"Findings\"\ntype = \"string\"\n\n\
                [[contributes.settings]]";
    echo_plugin().replacen("[[contributes.settings]]", more, 1)
}

/// An [`Env`] with [`checking`]'s plugin, and the demo drive USB.
fn env() -> Env {
    let mut env = Env::new();
    env.plugins(&[&checking()]);
    env.demo("USB");
    env
}

/// The echo plugin's checks and guards answer `answer`.
fn find(env: &Env, answer: Value) {
    env.ok(&["--yes", "plugin", "settings", "echo", "findings", &answer.to_string()]);
}

/// The items of `check USB --json` a check of the echo plugin found, and its exit status.
fn checked(env: &Env) -> (Vec<Value>, i32) {
    let r = env.run(&["--json", "check", "USB"]);
    let v: Value = serde_json::from_str(&r.out).unwrap_or_else(|e| panic!("{e}: {}\n{}", r.out, r.err));
    let items = v["items"].as_array().unwrap().iter().filter(|i| i["check"] == "echo.found").cloned().collect();
    (items, r.code)
}

/// What a plugin's check finds is in `check` and `doctor`, named after it, with the command line
/// that fixes it, which runs its plugin's operation as any is run; a problem it finds is a
/// problem (`check` exits 4 then), a warning isn't.
#[test]
fn a_plugins_findings_are_in_check_and_doctor_with_their_fix() {
    let env = env();
    let fix = json!({ "operation": "playlist", "params": { "name": "Fixed" } });
    find(&env, json!({ "findings": [{ "level": "warning", "message": "two copies", "tracks": [1], "fix": fix }] }));
    let (items, code) = checked(&env);
    let expected = json!({
        "drive": "USB", "level": "warning", "check": "echo.found", "message": "two copies", "tracks": [1],
        "fix": { "plugin": "echo", "operation": "playlist", "params": { "name": "Fixed" } },
    });
    assert_eq!(items, [expected]);
    let shown = env.run(&["check", "USB"]);
    assert_eq!(shown.code, code, "{}", shown.out);
    let fixed = r#"fix with `locallibrary echo playlist "USB" --params '{"name":"Fixed"}'`"#;
    for said in ["echo.found", "two copies", fixed] {
        assert!(shown.out.contains(said), "{said}: {}", shown.out);
    }
    // The fix is the plugin's operation.
    env.ok(&["--yes", "echo", "playlist", "USB", "--params", r#"{"name":"Fixed"}"#]);
    assert!(env.ok(&["playlist", "list", "USB"]).contains("Fixed"));
    // A problem it finds is a problem.
    find(&env, json!({ "findings": [{ "level": "problem", "message": "unplayable" }] }));
    assert_eq!(checked(&env).1, 4);
    let doctor = env.run(&["--json", "doctor", "USB", "--only", "checks"]);
    let v: Value = serde_json::from_str(&doctor.out).unwrap();
    assert_eq!(doctor.code, 4, "{}", doctor.out);
    let found: Vec<&Value> = v["checks"].as_array().unwrap().iter().filter(|i| i["check"] == "echo.found").collect();
    assert_eq!((found.len(), &found[0]["level"]), (1, &json!("problem")));
    assert!(v["duplicates"].is_null(), "only the checks ran");
    let shown = env.run(&["doctor", "USB", "--only", "checks"]);
    assert!(shown.out.contains("Plugins' checks (") && shown.out.contains("unplayable"), "{}", shown.out);
    // Turned off, it isn't run.
    env.ok(&["--yes", "plugin", "checks", "echo.found", "--off"]);
    assert!(checked(&env).0.is_empty());
    let v = env.json(&["doctor", "USB", "--only", "checks"]);
    assert!(v["checks"].as_array().unwrap().iter().all(|i| i["check"] != "echo.found"), "{v}");
}

/// A check that fails or takes longer than `plugin checks --timeout` gives it is a warning, and
/// `check` exits as it would without it.
#[test]
fn a_check_that_fails_or_takes_too_long_never_fails_the_check() {
    let env = env();
    find(&env, json!({}));
    let (_, code) = checked(&env);
    env.ok(&["--yes", "plugin", "checks", "--timeout", "0.5"]);
    for answer in [json!({ "fail": "io" }), json!({ "wait": true })] {
        find(&env, answer.clone());
        let (items, failed) = checked(&env);
        assert_eq!(failed, code, "{answer}");
        assert_eq!((items.len(), &items[0]["level"]), (1, &json!("warning")), "{answer}");
    }
    find(&env, json!({ "wait": true }));
    assert_eq!(checked(&env).0[0]["message"], "took longer than 0.5s, so it was stopped");
}

/// A guard's findings are among a plan's warnings, on the command line and over RPC; made
/// blocking, one that finds a warning stops the plan before it's asked about (over RPC, when
/// it's applied), and `--no-blocking` lets it through again.
#[test]
fn a_guard_warns_about_a_plan_and_stops_it_once_blocking() {
    let env = env();
    find(&env, json!({ "findings": [{ "level": "warning", "message": "on a Friday" }] }));
    let run = |name: &str| {
        let params = json!({ "name": name }).to_string();
        env.run(&["--yes", "echo", "playlist", "USB", "--params", &params])
    };
    let r = run("Warmup");
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    assert!(format!("{}{}", r.out, r.err).contains("Guard: on a Friday"), "{}\n{}", r.out, r.err);
    env.ok(&["--yes", "plugin", "checks", "echo.guard", "--blocking"]);
    let r = run("Cooldown");
    assert_eq!(r.code, 1, "{}\n{}", r.out, r.err);
    assert!(r.err.contains("echo.guard found something about these changes"), "{}", r.err);
    assert!(!env.ok(&["playlist", "list", "USB"]).contains("Cooldown"), "not made");
    // Not asked about, with no terminal to ask on: refused all the same.
    let said = env.fails(1, &["echo", "playlist", "USB", "--params", r#"{"name":"Cooldown"}"#]);
    assert!(said.contains("echo.guard found something"), "{said}");
    let said = env.fails(1, &["plugin", "checks", "echo.found", "--blocking"]);
    assert!(said.contains("isn't run before a plan is applied"), "{said}");

    let mut s = Rpc::start(env.command());
    let drive = path(&env.volumes.join("USB")).to_string();
    let edits = json!([{ "op": "create_playlist", "path": "Late" }]);
    let plan = s.call(1, "edit.plan", json!({ "drive": drive, "edits": edits }));
    let plan = plan.get("result").cloned().unwrap_or_else(|| panic!("{plan}"));
    assert_eq!(plan["blocked"], "echo.guard");
    assert!(plan["warnings"].as_array().unwrap().contains(&json!("Guard: on a Friday")), "{plan}");
    let applied = s.call(2, "edit.apply", json!({ "plan_id": plan["plan_id"] }));
    assert_eq!(applied["error"]["data"]["reason"], "blocked", "{applied}");
    let unblocked = s.call(3, "plugins.checks.set", json!({ "plugin": "echo", "check": "guard", "blocking": false }));
    let guard = &unblocked["result"]["checks"].as_array().unwrap().iter().find(|c| c["name"] == "guard").cloned();
    assert_eq!(guard.as_ref().map(|g| (&g["on"], &g["blocking"])), Some((&json!(true), &json!(false))), "{unblocked}");
    let plan = s.call(4, "edit.plan", json!({ "drive": drive, "edits": edits }));
    assert!(plan["result"].get("blocked").is_none(), "{plan}");
    let applied = s.call(5, "edit.apply", json!({ "plan_id": plan["result"]["plan_id"] }));
    assert!(applied.get("result").is_some(), "{applied}");
}

/// `plugin checks` lists the checks, whether each runs and blocks and how long one has, and
/// changes them as told; over RPC, `plugins.checks`, `plugins.checks.set` and
/// `plugins.checks.timeout.set`. What a check finds is in the sidecar's `check` too.
#[test]
fn plugin_checks_lists_and_changes_them() {
    let env = env();
    let listed = env.json(&["plugin", "checks"]);
    let names: Vec<(&str, bool)> = (listed["checks"].as_array().unwrap().iter())
        .map(|c| (c["name"].as_str().unwrap(), c["on"].as_bool().unwrap()))
        .collect();
    assert_eq!(names, [("tracks", true), ("untitled", true), ("edits", false), ("found", true), ("guard", true)]);
    assert_eq!(listed["timeout"], 10.0);
    let shown = env.ok(&["plugin", "checks"]);
    assert!(
        shown.contains("echo.edits") && shown.contains("off") && shown.contains("a check has 10s to answer"),
        "{shown}"
    );
    env.ok(&["--yes", "plugin", "checks", "echo.edits", "--on"]);
    let one = env.json(&["plugin", "checks", "echo.edits"]);
    assert_eq!((one["checks"].as_array().unwrap().len(), &one["checks"][0]["on"]), (1, &json!(true)));
    env.ok(&["--yes", "plugin", "checks", "echo.edits", "--reset"]);
    assert_eq!(env.json(&["plugin", "checks", "echo.edits"])["checks"][0]["on"], false);
    env.ok(&["--yes", "plugin", "checks", "--timeout", "3"]);
    assert_eq!(env.json(&["plugin", "checks"])["timeout"], 3.0);
    env.ok(&["--yes", "plugin", "checks", "--reset"]);
    assert_eq!(env.json(&["plugin", "checks"])["timeout"], 10.0);
    assert!(env.fails(2, &["plugin", "checks", "nodot"]).contains("name a check as PLUGIN.CHECK"));
    assert!(env.fails(1, &["plugin", "checks", "echo.nope"]).contains("check \"nope\""));
    let info = env.ok(&["plugin", "info", "echo"]);
    assert!(info.contains("Checks (locallibrary plugin checks):") && info.contains("Guard (when: apply)"), "{info}");
    assert!(env.ok(&["plugin"]).contains("checks untitled, edits, found, guard"));

    let mut s = Rpc::start(env.command());
    let checks = s.call(1, "plugins.checks", Value::Null);
    assert_eq!(checks["result"]["checks"].as_array().unwrap().len(), 5, "demos' and echo's: {checks}");
    let set = s.call(2, "plugins.checks.timeout.set", json!({ "timeout": 2 }));
    assert_eq!(set["result"]["timeout"], 2.0, "{set}");
    let set = s.call(3, "plugins.checks.set", json!({ "plugin": "echo", "check": "found", "on": false }));
    let found = set["result"]["checks"].as_array().unwrap().iter().find(|c| c["name"] == "found").cloned().unwrap();
    assert_eq!(found["on"], false);
    let set = s.call(4, "plugins.checks.set", json!({ "plugin": "echo", "check": "found", "reset": true }));
    assert!(set.get("result").is_some(), "{set}");
    find(&env, json!({ "findings": [{ "level": "info", "message": "said over RPC" }] }));
    let drive = path(&env.volumes.join("USB")).to_string();
    let report = s.call(5, "check", json!({ "drives": [drive] }));
    let items = report["result"]["items"].as_array().unwrap();
    assert!(items.iter().any(|i| i["check"] == "echo.found" && i["message"] == "said over RPC"), "{report}");
}

/// For any changes `plugin checks` (or the sidecar's `plugins.checks.set`) is asked for, one after
/// another, the first to a check (a check run or not, blocking or not, put back as it is by
/// default, or the time a check has), it lists every check and that time as changed so far, in
/// JSON and in words, and one check alone when it's named and nothing else is asked. Only a guard
/// can block, a check put back can't be changed in the same breath on the command line (the
/// sidecar puts it back first), only a time from a millisecond to an hour is taken, and a check
/// is named as PLUGIN.CHECK: a change refused changes nothing.
#[hegel::test(test_cases = 5, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn plugin_checks_lists_every_change_made(tc: TestCase) {
    let env = env();
    // Each check, whether it's a guard (run before a plan is applied), and whether it runs by default.
    let checks = [("untitled", false, true), ("edits", true, false), ("found", false, true), ("guard", true, true)];
    let mut settings: Vec<(bool, bool)> = checks.iter().map(|&(_, _, on)| (on, false)).collect();
    let mut timeout = 10.0;
    for step in 0..tc.draw(gs::integers::<usize>().min_value(2).max_value(4)) {
        // The command line first, then the sidecar, then anything.
        let what = match step {
            0 | 1 => step as u8,
            _ => tc.draw(gs::integers::<u8>().max_value(3)),
        };
        match what {
            // A check changed on the command line, or by the sidecar.
            0 | 1 => {
                // The first, a guard's both, so what's said of each is.
                let i = match step {
                    0 => tc.draw(gs::sampled_from(vec![1, 3])),
                    _ => tc.draw(gs::integers::<usize>().max_value(3)),
                };
                let (name, guard, default_on) = checks[i];
                let both = |tc: &TestCase| match step {
                    0 => Some(tc.draw(gs::booleans())),
                    _ => tc.draw(gs::optional(gs::booleans())),
                };
                let (mut on, mut blocking) = (both(&tc), both(&tc));
                let reset = step > 1 && tc.draw(gs::booleans());
                // The sidecar's first change turns the check on or off, whichever it isn't, and
                // isn't refused.
                if step == 1 {
                    on = Some(!settings[i].0);
                    blocking = blocking.filter(|_| guard);
                }
                let refused = match what {
                    0 => {
                        let named = format!("echo.{name}");
                        let mut args = vec!["--yes", "plugin", "checks", &named];
                        args.extend(on.map(|on| if on { "--on" } else { "--off" }));
                        args.extend(blocking.map(|b| if b { "--blocking" } else { "--no-blocking" }));
                        args.extend(reset.then_some("--reset"));
                        // --reset puts both back, so it takes neither.
                        let refused = blocking == Some(true) && !guard || reset && (on.is_some() || blocking.is_some());
                        let r = env.run(&args);
                        assert_eq!(r.code != 0, refused, "{args:?}\n{}", r.err);
                        if !refused && !reset && (on.is_some() || blocking.is_some()) {
                            let mut said: Vec<&str> = Vec::new();
                            said.extend(on.map(|on| if on { "run it" } else { "don't run it" }));
                            said.extend(blocking.map(|b| match b {
                                true => "stop changes it finds something about",
                                false => "only warn about changes",
                            }));
                            let said = format!("{named}: {}", said.join(", "));
                            assert!(r.out.contains(&said), "{said}:\n{}", r.out);
                        }
                        refused
                    }
                    _ => {
                        let mut s = Rpc::start(env.command());
                        let change =
                            json!({ "plugin": "echo", "check": name, "on": on, "blocking": blocking, "reset": reset });
                        let answer = s.call(1, "plugins.checks.set", change.clone());
                        let refused = blocking == Some(true) && !guard;
                        assert_eq!(answer.get("error").is_some(), refused, "{change}: {answer}");
                        refused
                    }
                };
                if !refused {
                    let (was_on, was_blocking) = if reset { (default_on, false) } else { settings[i] };
                    settings[i] = (on.unwrap_or(was_on), guard && blocking.unwrap_or(was_blocking));
                }
            }
            // The time a check has.
            2 => {
                // Quarter seconds, which a duration keeps exactly.
                let secs = tc.draw(gs::optional(gs::integers::<u32>().max_value(16_000))).map(|q| f64::from(q) / 4.0);
                let given = secs.map(|s| s.to_string());
                let mut args = vec!["--yes", "plugin", "checks"];
                match &given {
                    Some(secs) => args.extend(["--timeout", secs]),
                    None => args.push("--reset"),
                }
                let refused = secs.is_some_and(|s| !(0.001..=3600.0).contains(&s));
                let r = env.run(&args);
                assert_eq!(r.code != 0, refused, "{args:?}\n{}", r.err);
                if !refused {
                    timeout = secs.unwrap_or(10.0);
                }
            }
            // A check named some other way.
            _ => {
                let name = tc.draw(gs::sampled_from(&["echo", "found", "echo-found"][..]));
                let said = env.fails(2, &["--yes", "plugin", "checks", name, "--on"]);
                let says =
                    format!("name a check as PLUGIN.CHECK (`locallibrary plugin checks` lists them), not {name:?}");
                assert!(said.contains(&says), "{said}");
            }
        }
        // Named alone, a check is listed by itself.
        let i = tc.draw(gs::integers::<usize>().max_value(3));
        let one = env.json(&["plugin", "checks", &format!("echo.{}", checks[i].0)]);
        let listed: Vec<(bool, bool)> =
            one["checks"].as_array().unwrap().iter().map(|c| (c["on"] == true, c["blocking"] == true)).collect();
        assert_eq!(listed, [settings[i]], "{one}");

        let listed = env.json(&["plugin", "checks"]);
        // The echo plugin's, not the demos plugin's.
        let listed_settings: Vec<(bool, bool)> = (listed["checks"].as_array().unwrap().iter())
            .filter(|c| c["plugin"] == "echo")
            .map(|c| (c["on"].as_bool().unwrap(), c["blocking"].as_bool().unwrap()))
            .collect();
        assert_eq!((listed_settings, &listed["timeout"]), (settings.clone(), &json!(timeout)));
        let shown = env.ok(&["plugin", "checks"]);
        for (&(name, ..), &(on, blocking)) in checks.iter().zip(&settings) {
            let state = format!("{}{}", if on { "on" } else { "off" }, if blocking { ", blocking" } else { "" });
            let line = shown.lines().find_map(|l| l.strip_prefix(&format!("echo.{name} "))).unwrap();
            assert!(line.trim_start().starts_with(&format!("{state}  ")), "{shown}");
        }
        assert!(shown.contains(&format!("a check has {timeout}s to answer")), "{shown}");
    }
}

/// Whatever a plugin's check finds, `check` counts its problems and warnings with locallibrary's
/// own, and exits 4 exactly when there's a problem among them all. A finding's fix is said as the
/// command line that runs its plugin's operation, its params (any) quoted for the shell.
#[hegel::test(test_cases = 5, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn check_exits_on_a_plugins_problems_as_on_its_own(tc: TestCase) {
    let env = env();
    let levels: Vec<&str> =
        tc.draw(gs::vecs(gs::sampled_from(vec!["info", "warning", "problem"])).min_size(1).max_size(4));
    let mut fixes = Vec::new();
    let findings: Vec<Value> = (levels.iter().enumerate())
        .map(|(i, l)| {
            let mut finding = json!({ "level": l, "message": format!("finding {i}") });
            // The first always has a fix with a name.
            let name = gs::text().alphabet("ab '").max_size(6);
            let fix = match i {
                0 => Some(Some(tc.draw(name))),
                _ => tc.draw(gs::optional(gs::optional(name))),
            };
            if let Some(name) = fix {
                // A name given always has a quote, which the shell needs escaped.
                let params = name.map_or_else(|| json!({}), |n| json!({ "name": format!("{n}'s") }));
                let quoted = match params.as_object().unwrap().is_empty() {
                    true => String::new(),
                    false => format!(" --params '{}'", params.to_string().replace('\'', r"'\''")),
                };
                fixes.push(format!("fix with `locallibrary echo playlist \"USB\"{quoted}`"));
                finding["fix"] = json!({ "operation": "playlist", "params": params });
            }
            finding
        })
        .collect();
    find(&env, json!({ "findings": findings }));
    let shown = env.run(&["check", "USB"]).out;
    for fix in &fixes {
        assert!(shown.contains(fix.as_str()), "{fix}: {shown}");
    }
    let r = env.run(&["--json", "check", "USB"]);
    let v: Value = serde_json::from_str(&r.out).unwrap();
    let items = v["items"].as_array().unwrap();
    let count = |level: &str| items.iter().filter(|i| i["level"] == level).count();
    assert_eq!(
        (v["problems"].as_u64(), v["warnings"].as_u64()),
        (Some(count("problem") as u64), Some(count("warning") as u64))
    );
    let mine: Vec<&str> =
        items.iter().filter(|i| i["check"] == "echo.found").map(|i| i["level"].as_str().unwrap()).collect();
    assert_eq!(mine, levels);
    assert_eq!(r.code == 4, count("problem") > 0, "{}", r.out);
}

/// `prep`'s check runs the plugins' checks too: a problem one finds keeps the drives mounted, as
/// locallibrary's own do, but one that fails or takes too long is only a warning, and the job
/// ends as it would without it.
#[test]
fn prep_stops_on_a_plugins_problems_never_on_its_failing() {
    let env = env();
    env.ok(&["--yes", "adopt", path(&env.volumes.join("USB")), "--name", "USB"]);
    env.ok(&["pull", "USB"]);
    env.ok(&["--yes", "plugin", "checks", "--timeout", "0.5"]);
    let prep = |answer: Value| {
        find(&env, answer);
        let r = env.run(&["--json", "--yes", "prep", "USB", "--no-eject"]);
        let v: Value = serde_json::from_str(&r.out).unwrap_or_else(|e| panic!("{e}: {}\n{}", r.out, r.err));
        let report = v["report"].clone();
        let mine: Vec<Value> = report["check"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["check"] == "echo.found")
            .cloned()
            .collect();
        (r.code, report, mine)
    };
    let (code, report, _) = prep(json!({}));
    assert_eq!((&report["problems"], &report["stopped"]), (&json!(false), &Value::Null), "{report}");
    for answer in [json!({ "fail": "io" }), json!({ "wait": true })] {
        let (failed, with, mine) = prep(answer.clone());
        assert_eq!((failed, &with["problems"], &with["stopped"]), (code, &report["problems"], &Value::Null), "{with}");
        assert_eq!((mine.len(), &mine[0]["level"]), (1, &json!("warning")), "{answer}");
    }
    // Long enough to answer on a busy machine: out of time, the problem would be a warning.
    env.ok(&["--yes", "plugin", "checks", "--timeout", "60"]);
    let (stopped, with, _) = prep(json!({ "findings": [{ "level": "problem", "message": "unplayable" }] }));
    assert_eq!((stopped, &with["problems"]), (4, &json!(true)), "{with}");
}

/// A guard is asked about every plan that edits a library, given its edits: cues, a playlist
/// restored and a contact's playlist too. Made blocking, one that finds something stops each
/// before it's asked about, and over RPC when it's applied.
#[test]
fn a_blocking_guard_stops_cues_a_restore_and_a_contacts_playlist() {
    let env = env();
    let usb = env.volumes.join("USB");
    env.ok(&["--yes", "adopt", path(&usb), "--name", "USB"]);
    env.ok(&["--yes", "playlist", "new", "USB", "Keep"]);
    env.ok(&["pull", "USB"]);
    env.ok(&["--yes", "playlist", "delete", "USB", "Keep"]);
    let cues_before = env.ok(&["cues", "list", "USB", "--track", "1"]);
    env.ok(&["--yes", "plugin", "checks", "echo.edits", "--on"]);
    env.ok(&["--yes", "plugin", "checks", "echo.guard", "--blocking"]);
    find(&env, json!({ "about_edits": true, "findings": [{ "level": "warning", "message": "not tonight" }] }));
    let plans: [(&[&str], &str); 3] = [
        (&["cues", "set", "USB", "--track", "1", "--hot", "A", "--at", "1"], "cues"),
        (&["restore", "USB", "--playlist", "Keep"], "playlist.restore"),
        (&["drive", "contact", "USB", "dj@example.com", "--playlist"], "drive.contact"),
    ];
    for (args, operation) in plans {
        let r = env.run(&[&["--yes"], args].concat());
        let said = format!("{}{}", r.out, r.err);
        assert_eq!(r.code, 1, "{said}");
        assert!(r.err.contains("echo.guard found something about these changes"), "{said}");
        let counted = format!("Count a plan's edits: {operation} makes ");
        assert!(said.contains("Guard: not tonight") && said.contains(&counted), "{said}");
    }
    assert_eq!(env.ok(&["cues", "list", "USB", "--track", "1"]), cues_before, "no cue set");
    assert!(!env.ok(&["playlist", "list", "USB"]).contains("Keep"), "not restored");
    assert!(!usb.join("IF FOUND.txt").exists(), "no contact written");

    let mut s = Rpc::start(env.command());
    let edit = json!({ "op": "set", "hot_cue": "A", "at": { "ms": 1000 } });
    let plan = s.call(1, "cues.plan", json!({ "drive": path(&usb), "ids": [1], "edit": edit }));
    let plan = plan.get("result").cloned().unwrap_or_else(|| panic!("{plan}"));
    assert_eq!(plan["blocked"], "echo.guard", "{plan}");
    let applied = s.call(2, "cues.apply", json!({ "plan_id": plan["plan_id"] }));
    assert_eq!(applied["error"]["data"]["reason"], "blocked", "{applied}");
}

/// What an importer read is a plan a guard is asked about too: made blocking, one that finds
/// something stops the import before it's asked about.
#[test]
fn a_blocking_guard_stops_an_import() {
    let env = env();
    let tracks = env.json(&["tracks", "USB"]);
    let list = env.outside("list.txt");
    std::fs::write(&list, format!("{}\n", tracks["tracks"][0]["file_path"].as_str().unwrap())).unwrap();
    env.ok(&["--yes", "plugin", "checks", "echo.edits", "--on"]);
    env.ok(&["--yes", "plugin", "checks", "echo.guard", "--blocking"]);
    find(&env, json!({ "findings": [{ "level": "warning", "message": "not tonight" }] }));
    let r = env.run(&["--yes", "import", path(&list), "USB"]);
    let said = format!("{}{}", r.out, r.err);
    assert_eq!(r.code, 1, "{said}");
    assert!(r.err.contains("echo.guard found something about these changes"), "{said}");
    let counted = "Count a plan's edits: echo.import.lines makes ";
    assert!(said.contains("Guard: not tonight") && said.contains(counted), "{said}");
    let again = env.ok(&["import", path(&list), "USB", "--dry-run"]);
    assert!(again.contains("add My Tag Genre/Echo to 1 track(s)"), "not imported: {again}");
}

/// The command line a fix runs with has none of the C1 control characters (U+0080 to U+009F) JSON
/// leaves as they are, which some terminals act on: the host takes in the plugin's params
/// printable.
#[test]
fn a_fixs_command_line_has_no_c1_control_characters() {
    let env = env();
    let fix = json!({ "operation": "playlist", "params": { "name": "a\u{85}b\u{9b}2J" } });
    find(&env, json!({ "findings": [{ "level": "warning", "message": "odd", "fix": fix }] }));
    let shown = env.run(&["check", "USB"]).out;
    assert!(shown.contains(r#"--params '{"name":"ab2J"}'"#), "{shown:?}");
    assert!(!shown.contains(['\u{85}', '\u{9b}']), "{shown:?}");
}
