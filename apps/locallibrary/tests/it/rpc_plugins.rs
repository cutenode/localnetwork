//! The sidecar's side of plugins' questions and providers: `plugins.run` answering a view of kind
//! `choose`, `plugins.op.plan` answering `{"choose": ...}` until it's called with the param set,
//! and `plugins.provide`, `plugins.provide.plan` / `.apply` and `plugins.providers(.set)`, with the
//! plugin host's test plugin (`LOCALLIBRARY_TEST_PLUGINS`).

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use serde_json::{Value, json};

use crate::support::{Env, Rpc, echo_plugin, path};

/// The echo manifest as these tests register it: its playlist's name not required (a question is
/// asked about a param its schema doesn't require: the host checks the params first), and in place
/// of its provider one, `found`, answering as its `answer` setting says.
fn asking() -> String {
    let manifest = echo_plugin();
    let required = ", required = [\"name\"] }";
    let provider = "name = \"tags\"\ntitle = \"The tags as they are\"";
    assert!(manifest.contains(required) && manifest.contains(provider));
    manifest.replace(required, " }").replace(provider, "name = \"found\"\ntitle = \"Found\"").replacen(
        "[[contributes.settings]]",
        "[[contributes.settings]]\nkey = \"answer\"\ntitle = \"Answer\"\ntype = \"string\"\n\n[[contributes.settings]]",
        1,
    )
}

/// The sidecar with [`asking`]'s plugin, on the demo drive USB (two tracks), and the drive's path.
fn sidecar() -> (Env, Rpc, String) {
    let mut env = Env::new();
    env.plugins(&[&asking()]);
    let drive = path(&env.demo("USB")).to_string();
    let s = Rpc::start(env.command());
    (env, s, drive)
}

fn result(r: Value) -> Value {
    r.get("result").cloned().unwrap_or_else(|| panic!("{r}"))
}

fn question(param: &str, values: &[Value]) -> Value {
    let items: Vec<Value> = values.iter().map(|v| json!({ "label": v.to_string(), "value": v })).collect();
    json!({ "title": "Which?", "param": param, "items": items })
}

/// The drive's tracks, by id: their titles and years.
fn titles(s: &mut Rpc, id: i64, drive: &str) -> Vec<(u64, String, u64)> {
    let page = result(s.call(id, "tracks.query", json!({ "drive": drive })));
    let mut tracks: Vec<(u64, String, u64)> = (page["tracks"].as_array().unwrap().iter())
        .map(|t| (t["id"].as_u64().unwrap(), t["title"].as_str().unwrap().to_string(), t["year"].as_u64().unwrap()))
        .collect();
    tracks.sort();
    tracks
}

/// A view command's question and an operation's are answered to the caller, who calls again with
/// the param set; a question is never kept as a plan, nor applied.
#[test]
fn the_sidecar_answers_a_plugins_question_to_its_caller() {
    let (_env, mut s, drive) = sidecar();
    let asked = question("pick", &[json!(1), json!("two")]);
    let run = |params: Value| json!({ "plugin": "echo", "command": "titles", "params": params, "context": { "drive": drive } });
    let view = result(s.call(1, "plugins.run", run(json!({ "ask": asked }))));
    assert_eq!(view, json!({ "kind": "choose", "title": "Which?", "param": "pick", "items": asked["items"] }));
    let answered = result(s.call(2, "plugins.run", run(json!({ "ask": asked, "pick": "two" }))));
    assert_eq!(answered, json!({ "kind": "text", "text": "\"two\"" }));

    let names = question("name", &[json!("Warm up"), json!("Peak")]);
    let op = |params: Value| json!({ "plugin": "echo", "operation": "playlist", "params": params, "context": { "drive": drive } });
    let choose = result(s.call(3, "plugins.op.plan", op(json!({ "ask": names }))));
    assert_eq!(choose, json!({ "choose": { "title": "Which?", "param": "name", "items": names["items"] } }));
    let refused = s.call(4, "plugins.op.apply", op(json!({ "ask": names })));
    assert_eq!(refused["error"]["data"]["kind"], "invalid", "{refused}");
    assert_eq!(refused["error"]["message"], "Which?: it asks which first, as `name` in its params");
    let planned = result(s.call(5, "plugins.op.plan", op(json!({ "ask": names, "name": "Peak" }))));
    assert_eq!(planned["playlists_created"], json!(["Peak"]));
    let applied = result(s.call(6, "plugins.op.apply", json!({ "plan_id": planned["plan_id"] })));
    assert_eq!(applied["changes"], 1);
    let bad = s.call(7, "plugins.op.plan", op(json!({ "ask": question("name", &[json!(5)]) })));
    assert_eq!(bad["error"]["data"]["kind"], "external", "{bad}");
}

/// The providers are listed with the score a candidate alone is picked for the user at, which is
/// set, and put back; what they find for a drive's tracks is planned and applied as picked.
#[test]
fn the_sidecar_offers_what_providers_find_and_applies_the_picks() {
    let (_env, mut s, drive) = sidecar();
    let listed = result(s.call(1, "plugins.providers", Value::Null));
    let echo = json!({ "plugin": "echo", "name": "found", "title": "Found", "kind": "metadata" });
    assert!(listed["providers"].as_array().unwrap().contains(&echo), "{listed}");
    assert_eq!(listed["auto_pick"], 0.9);
    assert_eq!(result(s.call(2, "plugins.providers.set", json!({ "auto_pick": 0.4 })))["auto_pick"], 0.4);
    assert_eq!(result(s.call(3, "plugins.providers.set", json!({ "auto_pick": null })))["auto_pick"], 0.9);
    let wrong = s.call(4, "plugins.providers.set", json!({ "auto_pick": -1 }));
    assert_eq!(wrong["error"]["data"]["kind"], "invalid", "{wrong}");

    let answer =
        json!({ "each": [{ "score": 1, "source": "Tags", "label": "Retitled", "metadata": { "title": "Retitled" } }] });
    let set = json!({ "plugin": "echo", "key": "answer", "value": answer.to_string() });
    result(s.call(5, "plugins.settings.set", set));
    let req = json!({ "kind": "metadata", "drive": drive, "from": "echo.found", "replace": true });
    let provided = result(s.call(6, "plugins.provide", req));
    assert_eq!(provided["asked"], 2);
    let offers = provided["offers"].as_array().unwrap();
    assert!(offers.iter().all(|o| o["picked"] == 0 && o["candidates"][0]["provider"] == "echo.found"), "{provided}");
    let picks: Vec<Value> =
        offers.iter().map(|o| json!({ "track": o["track"], "candidate": o["candidates"][0] })).collect();
    let plan = json!({ "kind": "metadata", "drive": drive, "picks": picks, "replace": true });
    let planned = result(s.call(7, "plugins.provide.plan", plan.clone()));
    assert_eq!(planned["operation"], "plugins.provide");
    assert_eq!(planned["summary"][0], "fill in the details of 2 track(s) from Tags");
    let wrong = s.call(8, "plugins.op.apply", json!({ "plan_id": planned["plan_id"] }));
    assert_eq!(wrong["error"]["data"]["kind"], "invalid_params", "{wrong}");
    result(s.call(9, "plugins.provide.apply", json!({ "plan_id": planned["plan_id"] })));
    assert!(titles(&mut s, 10, &drive).iter().all(|(_, title, _)| title == "Retitled"));
    // Planned and applied in one go: nothing changes now.
    let again = result(s.call(11, "plugins.provide.apply", plan));
    assert_eq!(again["changes"], 0, "{again}");
    // Only the plugin named: echo finds no cover art (the coverart plugin would go online).
    let none = s.call(12, "plugins.provide", json!({ "kind": "artwork", "drive": drive, "from": "echo" }));
    assert_eq!(none["error"]["data"]["kind"], "invalid", "{none}");
    assert!(none["error"]["message"].as_str().unwrap().starts_with("echo provides no artwork: "), "{none}");
}

/// Whatever the provider finds for the tracks and whatever the setting, the sidecar offers it best
/// first, picked exactly when one alone scores the setting or more; and planned and applied, the
/// picks are what the tracks then have (a title over theirs only with `replace`, a year where they
/// have none).
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn any_candidates_are_offered_and_their_picks_applied(tc: TestCase) {
    let (_env, mut s, drive) = sidecar();
    let auto_pick = tc.draw(gs::integers::<u8>().max_value(100)) as f64 / 100.0;
    result(s.call(1, "plugins.providers.set", json!({ "auto_pick": auto_pick })));
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    let each: Vec<Value> = (0..n)
        .map(|i| {
            let score = tc.draw(gs::integers::<u8>().max_value(100)) as f64 / 100.0;
            let year = tc.draw(gs::integers::<u16>().min_value(1950).max_value(2030));
            json!({ "score": score, "source": "Tags", "label": format!("c{i}"), "metadata": { "title": format!("T{i}"), "year": year } })
        })
        .collect();
    let set = json!({ "plugin": "echo", "key": "answer", "value": json!({ "each": each }).to_string() });
    result(s.call(2, "plugins.settings.set", set));
    let replace = tc.draw(gs::booleans());
    let before = titles(&mut s, 3, &drive);
    let req = json!({ "kind": "metadata", "drive": drive, "from": "echo", "replace": replace });
    let provided = result(s.call(4, "plugins.provide", req));
    let mut picks = Vec::new();
    for o in provided["offers"].as_array().unwrap() {
        let candidates = o["candidates"].as_array().unwrap();
        let scores: Vec<f64> = candidates.iter().map(|c| c["score"].as_f64().unwrap()).collect();
        assert!(scores.windows(2).all(|w| w[0] >= w[1]), "{scores:?}");
        let sure = scores.iter().filter(|s| **s >= auto_pick).count();
        assert_eq!(o["picked"], if sure == 1 { json!(0) } else { Value::Null }, "{o}");
        if let Some(i) = tc.draw(gs::optional(gs::integers::<usize>().max_value(candidates.len() - 1))) {
            picks.push(json!({ "track": o["track"], "candidate": candidates[i] }));
        }
    }
    let plan = json!({ "kind": "metadata", "drive": drive, "picks": picks, "replace": replace });
    let planned = result(s.call(5, "plugins.provide.plan", plan));
    if planned["noop"] == false {
        result(s.call(6, "plugins.provide.apply", json!({ "plan_id": planned["plan_id"] })));
    }
    let after = titles(&mut s, 7, &drive);
    for ((id, title, year), now) in before.iter().zip(&after) {
        let pick = picks.iter().find(|p| p["track"] == *id).map(|p| &p["candidate"]["metadata"]);
        let title = match pick {
            Some(m) if replace || title.is_empty() => m["title"].as_str().unwrap().to_string(),
            _ => title.clone(),
        };
        let year = match pick {
            Some(m) if replace || *year == 0 => m["year"].as_u64().unwrap(),
            _ => *year,
        };
        assert_eq!(now, &(*id, title, year));
    }
}
