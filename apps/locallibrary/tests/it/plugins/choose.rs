//! A plugin's command or operation asking a question on the command line: without a terminal (or
//! with `--json`), refused saying how to answer it, and answered with `--choose PARAM=VALUE`, the
//! value as JSON or text. The test plugin's `titles` and `playlist` ask `ask` until given its param.

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
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

/// Questions asked together, without a terminal, are refused saying how to answer each; given
/// every answer, the command runs once with them all.
#[test]
fn questions_asked_together_are_each_answered_with_choose() {
    let env = env();
    let mut params: Value = serde_json::from_str(&ask("pick", &[json!(1), json!(2)])).unwrap();
    let more: Value = serde_json::from_str(&ask("other", &[json!("a"), json!("b")])).unwrap();
    params["ask"]["more"] = json!([more["ask"]]);
    let params = params.to_string();
    let titles = ["echo", "titles", "USB", "--params", &params];
    let asked = env.fails(2, &titles);
    let said = "Which one?: answer with --choose pick=VALUE, VALUE one of:\n  1  is 1\n  2  is 2\n\
                Which one?: answer with --choose other=VALUE, VALUE one of:\n  \"a\"  is \"a\"\n  \"b\"  is \"b\"";
    assert!(asked.contains(said), "{asked}");
    let both = env.ok(&[&titles[..], &["--choose", "pick=2", "--choose", "other=b"]].concat());
    assert_eq!(both.trim(), "2 \"b\"");
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

/// Any questions a command asks together (each taking one value or a list of any, of numbers and
/// text), without a terminal: refused saying how to answer each, every value aligned beside its
/// label; given every answer with `--choose`, as JSON or as text, the command runs once with them
/// all.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn any_questions_a_command_asks_are_answered_with_choose(tc: TestCase) {
    let env = env();
    let mut questions = Vec::new();
    let mut choose = Vec::new();
    let mut given = Vec::new();
    for q in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        let all = vec![json!(1), json!(22), json!("two"), json!("Peak Time")];
        let values: Vec<Value> = tc.draw(gs::vecs(gs::sampled_from(all)).min_size(1).unique(true));
        let multi = tc.draw(gs::booleans());
        let param = format!("p{q}");
        let mut question: Value = serde_json::from_str(&ask(&param, &values)).unwrap();
        question["ask"]["multi"] = json!(multi);
        questions.push(question["ask"].clone());
        let answer = match multi {
            true => json!(tc.draw(gs::subsequences(values.clone()))),
            false => tc.draw(gs::sampled_from(values.clone())),
        };
        // Text that doesn't read as JSON is taken as it is.
        let raw = match answer.as_str() {
            Some(text) if tc.draw(gs::booleans()) => text.to_string(),
            _ => answer.to_string(),
        };
        choose.extend(["--choose".to_string(), format!("{param}={raw}")]);
        given.push(answer.to_string());
    }
    let mut asked = questions[0].clone();
    asked["more"] = json!(questions[1..]);
    let params = json!({ "ask": asked }).to_string();
    let titles = ["echo", "titles", "USB", "--params", &params];
    if tc.draw(gs::booleans()) {
        let said = env.fails(2, &titles);
        for q in &questions {
            let takes = if q["multi"] == true { "a JSON list of any of" } else { "one of" };
            let values: Vec<String> = q["items"].as_array().unwrap().iter().map(|i| i["value"].to_string()).collect();
            let width = values.iter().map(String::len).max().unwrap();
            let param = q["param"].as_str().unwrap();
            let mut lines = vec![format!("Which one?: answer with --choose {param}=VALUE, VALUE {takes}:")];
            lines.extend(values.iter().map(|v| format!("  {v:<width$}  is {v}")));
            let lines = lines.join("\n");
            assert!(said.contains(&lines), "{lines}\nin:\n{said}");
        }
        return;
    }
    let choose: Vec<&str> = choose.iter().map(String::as_str).collect();
    assert_eq!(env.ok(&[&titles[..], &choose[..]].concat()).trim(), given.join(" "), "{choose:?}");
}
