//! `refresh --then` and `refresh.plan`'s `operations`: plugins' operations run as a step of the
//! refresh, in its one plan.

use super::*;

/// `--then 'PLUGIN.OPERATION KEY=VALUE...'` runs the operation in the refresh's plan, its words
/// split as a shell would and its params read as its schema says, each `--then` in the order given,
/// the drive named before or after them. One that can't be planned fails the plan, and one that
/// isn't an operation and its params is a usage error naming it.
#[test]
fn refresh_then_runs_a_plugins_operation() {
    let env = env();
    let plan = env.ok(&["refresh", "USB", "--then", "echo.playlist name=Fresh", "--dry-run"]);
    assert!(plan.contains("Run the plugins' operations\n   • make the playlist Fresh\n"), "{plan}");
    let applied = env.json(&[
        "--yes",
        "refresh",
        "--then",
        "echo.playlist name=Fresh",
        "--then",
        "echo.playlist name='Late Night'",
        "USB",
    ]);
    assert_eq!(applied["outcome"], "applied", "{applied}");
    let steps = applied["report"]["steps"].as_array().unwrap();
    let ran = steps.iter().find(|s| s["step"] == "operations").unwrap();
    assert_eq!(ran["reports"].as_array().unwrap().len(), 2, "{ran}");
    let listed = env.ok(&["playlist", "ls", "USB"]);
    assert!(listed.contains("Fresh") && listed.contains("Late Night"), "{listed}");

    assert!(env.fails(1, &["refresh", "USB", "--then", "nope.playlist"]).contains("not found: plugin \"nope\""));
    let e = env.fails(2, &["refresh", "USB", "--then", "echo.playlist colour=red"]);
    assert!(e.contains("this echo.playlist operation has no option \"colour\""), "{e}");
    for bad in ["", "  ", "playlist name=x"] {
        let e = env.fails(2, &["refresh", "USB", "--then", bad]);
        assert!(e.contains(&format!("--then takes 'PLUGIN.OPERATION KEY=VALUE...', not {bad:?}")), "{e}");
    }
    let e = env.fails(2, &["refresh", "USB", "--then", "echo.playlist name='Late"]);
    assert!(e.contains(r#"--then "echo.playlist name='Late" has a quote it doesn't close"#), "{e}");
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
