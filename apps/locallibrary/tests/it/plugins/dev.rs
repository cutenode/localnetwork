//! Plugins locallibrary doesn't ship: given for a run (`--dev-plugin`, `LOCALLIBRARY_DEV_PLUGINS`)
//! and tried with `plugin check`. Their program is the binary itself, which run with no arguments
//! serves the test plugin with the manifest `LOCALLIBRARY_TEST_SERVE` names.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::*;
use crate::support::Run;

/// The program a plugin is given with: the binary cargo built for the tests.
fn program() -> PathBuf {
    assert_cmd::cargo::cargo_bin("locallibrary")
}

/// `manifest`, written beside the drives as `name`.
fn written(env: &Env, name: &str, manifest: &str) -> PathBuf {
    let file = env.outside(name);
    std::fs::write(&file, manifest).unwrap();
    file
}

/// The plugin `manifest` describes, run as [`program`], as `--dev-plugin` and
/// `LOCALLIBRARY_DEV_PLUGINS` take it.
fn given(manifest: &Path) -> OsString {
    std::env::join_paths([manifest, &program()]).unwrap()
}

/// `args` run in `env`, the program serving the manifest in `served`.
fn run(env: &Env, served: &Path, args: &[&OsStr]) -> Run {
    let out = env.command().env("LOCALLIBRARY_TEST_SERVE", served).args(args).output().unwrap();
    Run {
        out: String::from_utf8(out.stdout).unwrap(),
        err: String::from_utf8(out.stderr).unwrap(),
        code: out.status.code().unwrap(),
    }
}

/// `args` run with the plugin `manifest` given for the run.
fn run_given(env: &Env, manifest: &Path, args: &[&str]) -> Run {
    let given = given(manifest);
    let all: Vec<&OsStr> =
        [OsStr::new("--dev-plugin"), given.as_os_str()].into_iter().chain(args.iter().map(OsStr::new)).collect();
    run(env, manifest, &all)
}

/// The ids `plugins` (a listing) has.
fn ids(listed: &Value) -> Vec<&str> {
    listed.as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect()
}

/// A plugin given for the run is listed after those locallibrary ships, and its commands run, on
/// the command line and in the sidecar, from the flag or the environment; each run warns it's
/// there, with its capabilities. Without it, it's gone.
#[test]
fn a_plugin_given_for_the_run_answers_its_commands() {
    let env = Env::new();
    env.demo("USB");
    let manifest = written(&env, "plugin.toml", &echo_plugin());
    let r = run_given(&env, &manifest, &["--json", "echo", "echo", "--params", r#"{"n": 1}"#]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(serde_json::from_str::<Value>(&r.out).unwrap()["n"], 1, "{}", r.out);
    let warning =
        "the echo plugin isn't one locallibrary ships: it runs as you, with capabilities library:read, library:plan";
    assert!(r.err.contains(warning), "{}", r.err);

    let r = run_given(&env, &manifest, &["--json", "plugin", "list"]);
    assert_eq!(ids(&serde_json::from_str(&r.out).unwrap()), [&SHIPPED[..], &["echo"]].concat(), "{}", r.err);
    let mut sidecar = env.command();
    sidecar.env("LOCALLIBRARY_DEV_PLUGINS", given(&manifest)).env("LOCALLIBRARY_TEST_SERVE", &manifest);
    let mut rpc = Rpc::start(sidecar);
    assert_eq!(ids(&rpc.call(1, "plugins.list", Value::Null)["result"]), [&SHIPPED[..], &["echo"]].concat());
    let answer = rpc.call(2, "plugins.run", json!({ "plugin": "echo", "command": "echo", "params": { "n": 2 } }));
    assert_eq!(answer["result"], json!({ "n": 2 }), "{answer}");
    drop(rpc);

    assert!(env.fails(2, &["echo", "echo"]).contains("unrecognized subcommand 'echo'"));
}

/// A plugin given for the run is held to a third party's rules, and can't have the id of one
/// locallibrary ships or clash with locallibrary: one that breaks any fails every command, saying
/// why, and so does a flag without a program or a manifest that can't be read.
#[test]
fn a_plugin_given_for_the_run_that_cant_run_fails_the_run() {
    let env = Env::new();
    let fails = |name: &str, manifest: &str| {
        let file = written(&env, name, manifest);
        let r = run_given(&env, &file, &["status"]);
        assert_eq!((r.code, r.out.as_str()), (1, ""), "{}", r.err);
        (file, r.err)
    };
    let placed =
        echo_plugin().replacen("title = \"Answer the params\"", "title = \"Answer the params\"\ncli = \"say\"", 1);
    let (_, err) = fails("placed.toml", &placed);
    let why =
        "the echo plugin's manifest: contributes.commands[0].cli: only plugins locallibrary ships add CLI commands";
    assert!(err.contains(why), "{err}");
    let unbacked = echo_plugin().replace("\"library:plan\"]", "\"library:plan\", \"library:delete-unbacked\"]");
    let (_, err) = fails("unbacked.toml", &unbacked);
    assert!(err.contains("capabilities[2]: only plugins locallibrary ships delete without a backup"), "{err}");
    let (file, err) = fails("sound.toml", &echo_plugin().replace("id = \"echo\"", "id = \"sound\""));
    let why = "the sound plugin is one locallibrary ships: another can't take its id, or its settings and access to your accounts";
    assert!(err.contains(&format!("the plugin in {} can't run: {why}", file.display())), "{err}");
    let (_, err) = fails("events.toml", &echo_plugin().replace("id = \"echo\"", "id = \"events\""));
    assert!(err.contains("the events plugin's id is the sidecar's events.* methods' too"), "{err}");

    let file = written(&env, "plugin.toml", &echo_plugin());
    let r = run(&env, &file, &["--json".as_ref(), "--dev-plugin".as_ref(), file.as_os_str(), "status".as_ref()]);
    let error: Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!((r.code, &error["error"]["kind"]), (1, &json!("invalid")), "{error}");
    let separator = if cfg!(windows) { ';' } else { ':' };
    let why = format!("give each plugin's manifest and the program to run it with, as MANIFEST{separator}PROGRAM");
    assert!(error["error"]["message"].as_str().unwrap().ends_with(&why), "{error}");
    let missing = env.outside("missing.toml");
    let r = run_given(&env, &missing, &["status"]);
    assert_eq!(r.code, 1, "{}", r.err);
    assert!(r.err.contains(&missing.display().to_string()), "{}", r.err);
}

/// `plugin check` passes a plugin that would run, listing the features it and locallibrary both
/// have, and lists what's wrong with one that wouldn't, with exit status 4: a manifest breaking a
/// third party's rules, the id of a plugin locallibrary ships, a program describing itself otherwise
/// than its manifest, or one that can't be started.
#[test]
fn plugin_check_says_whether_a_plugin_would_run() {
    let env = Env::new();
    let manifest = written(&env, "plugin.toml", &echo_plugin());
    let check = |manifest: &Path, served: &Path, json: bool| {
        let mut args: Vec<&OsStr> = vec!["plugin".as_ref(), "check".as_ref(), manifest.as_os_str()];
        let program = program();
        args.extend(["--".as_ref(), program.as_os_str()]);
        if json {
            args.insert(0, "--json".as_ref());
        }
        run(&env, served, &args)
    };
    let r = check(&manifest, &manifest, false);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.contains("the echo plugin 0.1.0 would run"), "{}", r.out);
    let r = check(&manifest, &manifest, true);
    let checked: Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!((r.code, &checked["problems"], &checked["outcome"]), (0, &json!([]), &json!("applied")), "{checked}");
    assert!(checked["features"].as_array().unwrap().contains(&json!("commands")), "{checked}");

    let problems = |manifest: &Path, served: &Path| {
        let r = check(manifest, served, true);
        let checked: Value = serde_json::from_str(&r.out).unwrap();
        assert_eq!((r.code, &checked["outcome"]), (4, &json!("problems")), "{checked}");
        checked["problems"].clone()
    };
    let retitled = written(&env, "retitled.toml", &echo_plugin().replace("Answer the params", "Say it back"));
    let otherwise = "the Echo plugin failed: it describes itself otherwise than the manifest locallibrary has for it";
    assert_eq!(problems(&manifest, &retitled), json!([otherwise]));
    let placed =
        echo_plugin().replacen("title = \"Answer the params\"", "title = \"Answer the params\"\ncli = \"say\"", 1);
    let placed = written(&env, "placed.toml", &placed);
    let why =
        "the echo plugin's manifest: contributes.commands[0].cli: only plugins locallibrary ships add CLI commands";
    assert_eq!(problems(&placed, &placed), json!([why]));
    let sound = written(&env, "sound.toml", &echo_plugin().replace("id = \"echo\"", "id = \"sound\""));
    let why = "the sound plugin is one locallibrary ships: another can't take its id, or its settings and access to your accounts";
    assert_eq!(problems(&sound, &sound), json!([why]));

    let r = env.run(&["plugin", "check", path(&manifest), "--", path(&env.outside("no-such-program"))]);
    assert_eq!(r.code, 4, "{}", r.err);
    assert!(r.out.contains("the Echo plugin failed: it couldn't be started: "), "{}", r.out);
}
