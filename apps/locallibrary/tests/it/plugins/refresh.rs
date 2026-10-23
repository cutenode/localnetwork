//! `refresh --then` and `refresh.plan`'s `operations`: plugins' operations run as a step of the
//! refresh, in its one plan.

use super::*;

/// `--then PLUGIN.OPERATION KEY=VALUE...` runs the operation in the refresh's plan, its params read
/// as its schema says, each `--then` in the order given. One that can't be planned fails the plan,
/// and what isn't KEY=VALUE after it is a usage error.
#[test]
fn refresh_then_runs_a_plugins_operation() {
    let env = env();
    let then =
        |more: &[&'static str]| [&["refresh", "USB", "--then", "echo.playlist", "name=Fresh"][..], more].concat();
    let plan = env.ok(&then(&["--dry-run"]));
    assert!(plan.contains("Run the plugins' operations\n   • make the playlist Fresh\n"), "{plan}");
    let applied = env.json(&[&["--yes"][..], &then(&["--then", "echo.playlist", "name=Later"])].concat());
    assert_eq!(applied["outcome"], "applied", "{applied}");
    let steps = applied["report"]["steps"].as_array().unwrap();
    let ran = steps.iter().find(|s| s["step"] == "operations").unwrap();
    assert_eq!(ran["reports"].as_array().unwrap().len(), 2, "{ran}");
    let listed = env.ok(&["playlist", "ls", "USB"]);
    assert!(listed.contains("Fresh") && listed.contains("Later"), "{listed}");

    assert!(env.fails(1, &["refresh", "USB", "--then", "nope.playlist"]).contains("not found: plugin \"nope\""));
    let e = env.fails(2, &["refresh", "--then", "echo.playlist", "name=x", "USB"]);
    assert!(e.contains("then its params as KEY=VALUE, not \"USB\" (name the drive before it)"), "{e}");
    let e = env.fails(2, &["refresh", "USB", "--then", "echo.playlist", "colour=red"]);
    assert!(e.contains("this echo.playlist operation has no option \"colour\""), "{e}");
}

/// The sidecar's `refresh.plan` takes `operations`; one that can't be planned is refused, the
/// error naming it.
#[test]
fn the_sidecars_refresh_takes_operations() {
    let env = env();
    let drive = path(&env.volumes.join("USB")).to_string();
    let mut s = Rpc::start(env.command());
    let refresh = |ops: Value| json!({ "drive": drive, "skip": ["pull"], "operations": ops });
    let planned =
        s.call(1, "refresh.plan", refresh(json!([{ "operation": "echo.playlist", "params": { "name": "Rpc" } }])));
    let plan = &planned["result"];
    let step = plan["steps"].as_array().unwrap().iter().find(|s| s["step"] == "operations").unwrap();
    assert_eq!((&step["runs"], &step["plans"][0]["operation"]), (&json!(true), &json!("echo.playlist")), "{planned}");
    let applied = s.call(2, "refresh.apply", json!({ "plan_id": plan["plan_id"] }));
    assert!(applied["result"]["stopped"].is_null(), "{applied}");

    let refused = s.call(3, "refresh.plan", refresh(json!([{ "operation": "echo.nope" }])));
    assert_eq!(refused["error"]["data"]["kind"], "not_found", "{refused}");
    assert_eq!(refused["error"]["data"]["fields"]["operation"], "echo.nope", "{refused}");
}
