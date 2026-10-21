//! A plugin's command or operation asking a question on the command line: without a terminal (or
//! with `--json`), refused saying how to answer it, and answered with `--choose PARAM=VALUE`, the
//! value as JSON or text. The test plugin's `titles` and `playlist` ask `ask` until given its param.

use serde_json::{Value, json};

use crate::support::{Env, echo_plugin};

/// The echo manifest, its playlist's name not required: a param asked for is one the host
/// doesn't refuse the params without.
fn asking() -> String {
    let required = ", required = [\"name\"] }";
    assert!(echo_plugin().contains(required));
    echo_plugin().replace(required, " }")
}

fn env() -> Env {
    let mut env = Env::new();
    env.plugins(&[&asking()]);
    env.demo("USB");
    env
}

/// `--params` asking `param` which of `values`.
fn ask(param: &str, values: &[Value]) -> String {
    let items: Vec<Value> = values.iter().map(|v| json!({ "label": format!("is {v}"), "value": v })).collect();
    json!({ "ask": { "title": "Which one?", "param": param, "items": items } }).to_string()
}

#[test]
fn a_command_asks_and_is_answered_with_choose() {
    let env = env();
    let params = ask("pick", &[json!(1), json!("two")]);
    let titles = ["echo", "titles", "USB", "--params", &params];
    let asked = env.fails(2, &titles);
    assert!(
        asked.contains(
            "Which one?: answer with --choose pick=VALUE, VALUE one of:\n  1      is 1\n  \"two\"  is \"two\""
        ),
        "{asked}"
    );
    assert_eq!(env.ok(&[&titles[..], &["--choose", "pick=two"]].concat()).trim(), "\"two\"");
    assert_eq!(env.ok(&[&titles[..], &["--choose", "pick=1"]].concat()).trim(), "1");
    let v = env.json(&[&titles[..], &["--choose", "pick=\"two\""]].concat());
    assert_eq!(v["kind"], "text", "{v}");
    assert_eq!(v["text"], "\"two\"");
    // Not asked under --json: refused the same way.
    let r = env.run(&[&["--json"], &titles[..]].concat());
    assert_eq!(r.code, 2, "{}\n{}", r.out, r.err);
    assert!(format!("{}{}", r.out, r.err).contains("answer with --choose pick=VALUE"), "{}\n{}", r.out, r.err);
}

#[test]
fn an_operation_asks_and_is_planned_answered() {
    let env = env();
    let params = ask("name", &[json!("Warm up"), json!("Peak Time")]);
    let plan = ["echo", "playlist", "USB", "--params", &params];
    let asked = env.fails(2, &[&["--yes"], &plan[..]].concat());
    assert!(asked.contains("Which one?: answer with --choose name=VALUE, VALUE one of:"), "{asked}");
    let dry = env.json(&[&plan[..], &["--choose", "name=Peak Time", "--dry-run"]].concat());
    assert_eq!(dry["plan"]["playlists_created"], json!(["Peak Time"]), "{dry}");
    let out = env.ok(&[&["--yes"], &plan[..], &["--choose", "name=Peak Time"]].concat());
    assert!(out.contains("+ Peak Time"), "{out}");
    // A question about a value the param can't take isn't asked.
    let wrong = ask("name", &[json!(5)]);
    let err = env.fails(1, &["--yes", "echo", "playlist", "USB", "--params", &wrong]);
    assert!(err.contains("its question won't do"), "{err}");
}
