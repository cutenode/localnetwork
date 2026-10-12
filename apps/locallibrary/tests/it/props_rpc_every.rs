//! Every method the sidecar has, called in every run rather than now and then: with parameters
//! drawn from its schema, an apply with plan ids that name no plan, and all of them again with a
//! file where the store should be. Each answers once, in the shape the contract gives it, or
//! with a stable error.

use hegel::TestCase;
use serde_json::{Value, json};

use crate::props_rpc::{at, contract, excluded, params_for, run_batch, violation};
use crate::support::Env;

/// The methods random calls may make, control methods left out (the server answers those), and
/// the mix index, mix reports and player updates too: on the fixture's drives and files they're
/// quick, and they read and write only those.
fn callable() -> Vec<&'static Value> {
    let methods = contract()["methods"].as_array().unwrap();
    let local = |name: &str| name != "mix.identify" && (name.starts_with("mix.") || name.starts_with("firmware."));
    methods
        .iter()
        .filter(|m| m["kind"] != "control")
        .filter(|m| m["name"].as_str().is_some_and(|name| local(name) || !excluded(name)))
        .collect()
}

/// Sends every request in one batch, and checks each is answered once: a result in its method's
/// shape, or an error with a stable kind (never a panic's). Returns the answers by request.
fn answered(fx: &Env, requests: &[(&'static Value, Value)]) -> Vec<Value> {
    let lines: Vec<Vec<u8>> = requests
        .iter()
        .enumerate()
        .map(|(id, (m, params))| {
            json!({"jsonrpc": "2.0", "id": id, "method": m["name"], "params": params}).to_string().into_bytes()
        })
        .collect();
    let (out, clean) = run_batch(fx, &lines);
    assert!(clean, "the sidecar didn't exit cleanly");
    requests
        .iter()
        .enumerate()
        .map(|(id, (m, params))| {
            let got: Vec<&Value> = out.iter().filter(|a| a.get("method").is_none() && a["id"] == id).collect();
            assert_eq!(got.len(), 1, "{} {params}: answered {} times", m["name"], got.len());
            let a = got[0].clone();
            match (a.get("result"), a.get("error")) {
                (Some(r), None) => {
                    if let Some(v) = violation(r, &format!("{}/result", at(m))) {
                        panic!("{}.result: {v}\n{params}\n{a}", m["name"]);
                    }
                }
                (None, Some(e)) => {
                    assert_ne!(e["data"]["kind"], "internal", "{} {params} panicked: {a}", m["name"]);
                    if let Some(v) = violation(&e["data"], "/error") {
                        panic!("{v}\n{a}");
                    }
                }
                _ => panic!("a result or an error: {a}"),
            }
            a
        })
        .collect()
}

/// Every method, with parameters drawn from its schema (now and then ones it doesn't take, which
/// are refused as invalid), and every apply also with a plan id that names no plan and with one
/// that isn't a string: each answers once, as the contract says, and the plan ids are refused.
#[hegel::test(test_cases = 3)]
fn every_method_answers_as_the_contract_says(tc: TestCase) {
    let fx = Env::from_template();
    let mut requests = Vec::new();
    for m in callable() {
        let (params, fits) = params_for(&tc, &fx, m);
        requests.push((m, params, fits));
    }
    let applies: Vec<&Value> = callable().into_iter().filter(|m| m["kind"] == "apply").collect();
    let no_plan: Vec<(&Value, Value)> =
        applies.iter().flat_map(|m| [(*m, json!({"plan_id": "no-such-plan"})), (*m, json!({"plan_id": 7}))]).collect();
    let sent: Vec<(&Value, Value)> =
        requests.iter().map(|(m, p, _)| (*m, p.clone())).chain(no_plan.iter().cloned()).collect();
    let answers = answered(&fx, &sent);
    for ((m, params, fits), a) in requests.iter().zip(&answers) {
        if !fits {
            assert_eq!(a["error"]["data"]["kind"], "invalid_params", "{} {params}: {a}", m["name"]);
        }
    }
    for ((m, params), a) in no_plan.iter().zip(&answers[requests.len()..]) {
        let kind = &a["error"]["data"]["kind"];
        let expected = if params["plan_id"].is_string() { "stale_plan" } else { "invalid_params" };
        assert_eq!(kind, expected, "{} {params}: {a}", m["name"]);
    }
}

/// With a file where the store should be, every method still answers as the contract says (those
/// that need the store with an error), and none replaces the file or writes into it.
#[hegel::test(test_cases = 2)]
fn a_file_in_place_of_the_store_is_left_alone(tc: TestCase) {
    let fx = Env::from_template();
    std::fs::remove_dir_all(&fx.store).unwrap();
    std::fs::write(&fx.store, "not a store").unwrap();
    let requests: Vec<(&Value, Value)> = callable()
        .into_iter()
        .map(|m| {
            let (params, _) = params_for(&tc, &fx, m);
            (m, params)
        })
        .collect();
    answered(&fx, &requests);
    assert_eq!(std::fs::read(&fx.store).unwrap(), b"not a store");
}

/// The store's own files, each read whole whenever it's needed.
const STORE_FILES: [&str; 9] = [
    "store.json",
    "libraries.json",
    "drives.json",
    "settings.json",
    "labels.json",
    "smart.json",
    "players.json",
    "pruned.json",
    "copy.json",
];

/// With some of the store's files unreadable (not JSON), every method still answers as the
/// contract says, those that need one of them with an error, and none writes over one: what
/// was in it is never lost to a method that couldn't read it.
#[hegel::test(test_cases = 4)]
fn unreadable_store_files_are_never_written_over(tc: TestCase) {
    let fx = Env::from_template();
    let broken: Vec<&str> = tc.draw(hegel::generators::subsequences(STORE_FILES.to_vec()).min_size(1));
    for name in &broken {
        std::fs::write(fx.store.join(name), "{ not json").unwrap();
    }
    let requests: Vec<(&Value, Value)> = callable()
        .into_iter()
        .map(|m| {
            let (params, _) = params_for(&tc, &fx, m);
            (m, params)
        })
        .collect();
    answered(&fx, &requests);
    for name in &broken {
        assert_eq!(std::fs::read(fx.store.join(name)).unwrap(), b"{ not json", "{name} was written over");
    }
}
