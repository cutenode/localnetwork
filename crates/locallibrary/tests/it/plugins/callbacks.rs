//! What a plugin reads by calling back, through a real plugin process (the rules themselves are
//! tested beside them, in `plugin/callbacks/tests.rs`).

use locallibrary::plugin::Options;
use loclib_plugin::Context;

use super::*;

/// What the test plugin got back from calling `method` with `params`: `{"result": ...}` or
/// `{"error": ...}`. Run without a drive, so from any thread.
fn call(f: &Fixture, method: &str, params: Value) -> Value {
    let params = json!({ "call": method, "params": params });
    match f.plugins.run_command("echo", "echo", params, Context::default(), None, &Ctx::silent(), &|_| {}).unwrap() {
        CommandOutput::Headless(v) => v,
        view => panic!("{view:?}"),
    }
}

#[test]
fn a_plugin_reads_what_its_capabilities_grant_in_pages() {
    let f = Fixture::echo();
    // The drive is found by the host, and the app given its root: which the plugin is told as
    // the drive's name, as every path it reads is said, though it named the drive by its id.
    let id = locallibrary::device::Device::open(&f.drive).unwrap().id().unwrap().to_string();
    let asked = call(&f, "drive.library", json!({ "drive": id }))["result"].clone();
    assert_eq!((asked["method"].clone(), asked["params"]["limit"].clone()), (json!("drive.library"), json!(500)));
    assert_eq!(asked["params"]["drive"], "USB");

    let denied = call(&f, "store.info", json!({}));
    assert_eq!(denied["error"]["data"]["kind"], "denied");
    assert_eq!(
        denied["error"]["message"],
        "store.info needs the store:read capability, which the echo plugin doesn't have"
    );
    let write = call(&f, "edit.plan", json!({ "drive": "USB", "edits": [] }));
    assert_eq!(write["error"]["code"], -32601);
}

/// What a plugin reads says nothing of where drives are mounted: no `root`, however deep.
#[test]
fn a_plugin_reads_no_roots() {
    let f = Fixture::new(|dir| vec![spec(dir, &granting(&["drive:info"]), &[])], Options::default());
    let info = call(&f, "drive.info", json!({ "drive": "USB" }));
    assert_eq!(info, json!({ "result": { "name": "USB", "drives": [{ "id": 1 }] } }));
}

/// Nor where anything else is: a store's copies, a set's recording, rekordbox's collection or a
/// drive last mounted are left out, and a mounted drive is said by its name, in a path or alone.
#[test]
fn a_plugin_reads_where_nothing_is() {
    let granted = granting(&["drive:info", "store:read", "sets:read", "collection:read"]);
    let f = Fixture::new(|dir| vec![spec(dir, &granted, &[])], Options::default());
    let read = |method: &str| call(&f, method, json!({}))["result"].clone();
    assert_eq!(read("store.copies"), json!([{ "name": "nas", "bytes": 1 }]));
    assert_eq!(read("store.info"), json!({ "copy_of": { "name": "n" }, "x": { "path": "kept" } }));
    assert_eq!(read("sets.get"), json!({ "recording": { "report": "r" }, "entries": [{ "path": "/Contents/a" }] }));
    assert_eq!(read("collection.info"), json!({ "bytes": 1 }));
    let said = json!({
        "offline": [{ "name": "Old" }],
        "duplicates": [{ "roots": ["USB"], "original": "USB" }],
        "warnings": [format!("USB{}PIONEER: unreadable", std::path::MAIN_SEPARATOR)],
    });
    assert_eq!(read("fleet.status"), said);
}

/// A drive a plugin names is one known here, never a path or a folder.
#[test]
fn a_plugin_names_no_folder_as_a_drive() {
    let f = Fixture::echo();
    let path = call(&f, "tracks.query", json!({ "drive": f.drive }))["error"].clone();
    assert_eq!(path["data"]["kind"], "invalid", "{path}");
    assert!(path["message"].as_str().unwrap().contains("is a path"), "{path}");
    // `src` is a folder where the tests run (the crate's), but no drive.
    assert!(Path::new("src").is_dir());
    let folder = call(&f, "tracks.query", json!({ "drive": "src" }));
    assert_eq!(folder["error"]["data"]["kind"], "not_found", "{folder}");
}

/// Params a read takes that a plugin may not give are refused, as every one not listed is.
#[test]
fn a_plugin_gives_only_the_params_listed() {
    let f = Fixture::new(|dir| vec![spec(dir, &granting(&["health:read"]), &[])], Options::default());
    let verify = call(&f, "drive.verify", json!({ "drive": "USB", "against": ["/Volumes/SD"] }))["error"].clone();
    assert_eq!(verify["data"]["kind"], "invalid_params");
    assert_eq!(verify["message"], "invalid params: a plugin doesn't give drive.verify `against`");
}

/// A read that panics is answered, and leaves room for the next.
#[test]
fn a_read_that_panics_is_answered() {
    let f = Fixture::new(|dir| vec![spec(dir, ECHO, &[])], Options { callbacks: 1, ..Options::default() });
    let e = call(&f, "playlists.tree", json!({ "drive": "USB" }))["error"].clone();
    assert_eq!(
        (e["code"].clone(), e["message"].clone()),
        (json!(-32603), json!("internal error while handling \"playlists.tree\""))
    );
    f.release.send(()).unwrap();
    assert_eq!(call(&f, "track.show", json!({ "drive": "USB" })), json!({ "result": "shown" }));
}

#[test]
fn errors_reaching_a_plugin_say_nothing_of_where_files_are() {
    let f = Fixture::echo();
    let e = call(&f, "track.matches", json!({ "drive": "USB" }))["error"].clone();
    // Said from the drive's root, and no "run it as an administrator" action.
    let path = "USB/PIONEER/rekordbox/export.pdb";
    assert_eq!(e["message"], format!("{path}: unreadable"));
    assert_eq!(e["data"], json!({ "kind": "io", "fields": { "path": path } }));
}

#[test]
fn a_plugin_has_a_few_calls_under_way_at_once() {
    let f = Fixture::new(|dir| vec![spec(dir, ECHO, &[])], Options { callbacks: 1, ..Options::default() });
    std::thread::scope(|s| {
        let first = s.spawn(|| call(&f, "track.show", json!({ "drive": "USB", "ids": [1] })));
        f.entered.lock().unwrap().recv().unwrap();
        let second = call(&f, "track.show", json!({ "drive": "USB", "ids": [1] }));
        assert_eq!(second["error"]["data"]["kind"], "busy");
        assert_eq!(second["error"]["message"], "a plugin has at most 1 calls under way at once");
        f.release.send(()).unwrap();
        assert_eq!(first.join().unwrap(), json!({ "result": "shown" }));
    });
    // Once it's answered, there's room again.
    f.release.send(()).unwrap();
    assert_eq!(call(&f, "track.show", json!({ "drive": "USB" })), json!({ "result": "shown" }));
}
