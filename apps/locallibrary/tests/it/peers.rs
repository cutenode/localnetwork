//! `store serve` and `store fetch`, between two stores on this computer's own network.

use std::io::{BufRead, BufReader};
use std::process::Stdio;

use serde_json::Value;

use crate::support::Env;

#[test]
fn a_store_is_served_and_fetched_on_the_local_network() {
    let laptop = Env::new();
    laptop.pulled("Home");
    let studio = Env::new();
    let mut serving =
        laptop.command().args(["--json", "store", "serve", "--local"]).stdout(Stdio::piped()).spawn().unwrap();
    let mut lines = BufReader::new(serving.stdout.take().unwrap()).lines();
    let ready: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    assert_eq!(ready["event"], "ready", "{ready}");
    let code = ready["code"].as_str().unwrap();

    let out = studio.ok(&["store", "fetch", code, "--local", "--dry-run"]);
    assert!(out.contains("would copy 1 snapshot(s)") && out.contains("Home: new here"), "{out}");
    let out = studio.ok(&["store", "fetch", code, "--local"]);
    assert!(out.contains("copied 1 snapshot(s)") && out.contains("Home: new here"), "{out}");
    assert!(studio.ok(&["library", "ls"]).contains("Home  1 snapshot(s)"));
    let out = studio.ok(&["store", "fetch", code, "--local", "--library", "home"]);
    assert!(out.contains("Home: up to date"), "{out}");

    let r = studio.run(&["store", "fetch", "not-a-code"]);
    assert!(r.code != 0 && r.err.contains("isn't a share code"), "{}", r.err);
    serving.kill().unwrap();
    serving.wait().unwrap();
}
