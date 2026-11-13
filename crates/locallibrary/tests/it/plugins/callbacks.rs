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

/// rekordbox's files are read by the host itself, not the app, for a plugin granted
/// `collection:files` alone: where they are comes as it is, though it's in the temporary folder,
/// which every other read says as `<temp>`. A copy outside the request's scratch folder is
/// refused.
#[test]
fn a_plugin_with_collection_files_reads_where_rekordbox_is() {
    let f = Fixture::new(|dir| vec![spec(dir, &granting(&["collection:files"]), &[])], Options::default());
    let master = f._dir.path().join("rekordbox/master.db");
    // SAFETY: nextest runs each test in a process of its own.
    unsafe { std::env::set_var("LOCALLIBRARY_MASTERDB", &master) };
    let at = call(&f, "collection.locations", json!({}))["result"].clone();
    assert_eq!((at["master_db"].clone(), at["master_db_exists"].clone()), (json!(master), json!(false)));
    let outside = call(&f, "collection.copy", json!({ "into": f._dir.path().join("copy.db") }))["error"].clone();
    assert_eq!(outside["data"]["kind"], "invalid", "{outside}");
    assert!(!f._dir.path().join("copy.db").exists());

    let echo = Fixture::echo();
    let denied = call(&echo, "collection.locations", json!({}))["error"].clone();
    assert_eq!(denied["data"]["kind"], "denied");
    assert_eq!(
        denied["message"],
        "collection.locations needs the collection:files capability, which the echo plugin doesn't have"
    );
}

/// Nor where anything else is: a store's copies, a set's or a mix's recording (and its cover),
/// rekordbox's collection or a drive last mounted are left out, and a mounted drive is said by its
/// name, in a path or alone.
#[test]
fn a_plugin_reads_where_nothing_is() {
    let granted = granting(&["drive:info", "store:read", "sets:read", "collection:read"]);
    let f = Fixture::new(|dir| vec![spec(dir, &granted, &[])], Options::default());
    let read = |method: &str| call(&f, method, json!({}))["result"].clone();
    assert_eq!(read("store.copies"), json!([{ "name": "nas", "bytes": 1 }]));
    assert_eq!(read("store.info"), json!({ "copy_of": { "name": "n" }, "x": { "path": "kept" } }));
    assert_eq!(read("sets.get"), json!({ "recording": { "report": "r" }, "entries": [{ "path": "/Contents/a" }] }));
    assert_eq!(read("collection.info"), json!({ "bytes": 1 }));
    let mix = json!({
        "mix": { "title": "Friday" },
        "plays": [{ "track": { "artist": "A", "title": "B", "file_path": "/Contents/a.mp3" } }],
    });
    assert_eq!(read("mix.reports.get"), mix);
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

/// A hook run on a folder opened by path, which isn't a drive known here, may name it as its
/// context does while it runs, and is told its name rather than where it is. Nothing else
/// opens up: not the folder by path, not another folder, not the folder once the hook is done.
#[test]
fn a_hook_on_a_folder_reads_only_that_folder() {
    let f = Fixture::echo();
    let elsewhere = f._dir.path().join("elsewhere");
    let folder = common::demo_drive(&elsewhere, "Export");
    common::demo_drive(&elsewhere, "Other");
    let on_folder = Context { drive: Some(folder.display().to_string()), ..Context::default() };
    let run = |context: &Context, params: Value| {
        let ran = f.plugins.run_command("echo", "echo", params, context.clone(), None, &Ctx::silent(), &|_| {});
        match ran.unwrap() {
            CommandOutput::Headless(v) => v,
            view => panic!("{view:?}"),
        }
    };
    let call =
        |context: &Context, drive: &str| run(context, json!({ "call": "tags.list", "params": { "drive": drive } }));
    assert_eq!(run(&on_folder, json!({ "context": true }))["drive"], "Export");
    // The app is given the folder's root, which the plugin is told as the folder's name.
    assert_eq!(call(&on_folder, "Export")["result"]["params"]["drive"], "Export");
    assert_eq!(call(&on_folder, "USB")["result"]["params"]["drive"], "USB");
    let path = call(&on_folder, &folder.display().to_string())["error"].clone();
    assert_eq!(path["data"]["kind"], "invalid", "{path}");
    assert_eq!(call(&on_folder, "Other")["error"]["data"]["kind"], "not_found");
    assert_eq!(call(&Context::default(), "Export")["error"]["data"]["kind"], "not_found");
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

/// Reads answering which store the app reads them in.
struct StoreReads;

impl Reads for StoreReads {
    fn read(&self, _: &str, _: Value, _: &Ctx<'_>) -> Result<Value, RpcFailure> {
        Ok(json!(api::store_root()))
    }
}

/// What a plugin reads is read in the store the host started it in, whichever store the process
/// would read otherwise.
#[test]
fn a_plugin_reads_in_its_hosts_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let specs = vec![spec(dir.path(), &granting(&["store:read"]), &[])];
    let plugins = Plugins::new(specs, Arc::new(StoreReads), Options::default()).unwrap();
    let params = json!({ "call": "store.info", "params": {} });
    let ran = plugins.run_command("echo", "echo", params, Context::default(), Some(&store), &Ctx::silent(), &|_| {});
    let CommandOutput::Headless(answer) = ran.unwrap() else { panic!("a headless command answers JSON") };
    assert_eq!(answer, json!({ "result": "<store>" }));
}

/// For any line a plugin sends once its host has let go of it (a process it started holding its
/// output), what it tells the user never reaches the front end, and what it asks is never read.
#[hegel::test(test_cases = 1)]
fn prop_what_a_plugin_sends_once_let_go_of_is_never_taken(tc: hegel::TestCase) {
    use super::events::{listening, told};
    use hegel::generators as gs;
    let message = format!("told {}", tc.draw(gs::from_regex("[a-z]{1,8}").fullmatch(true)));
    let (f, notices) = told(|dir| vec![spec(dir, &listening(&["plan.applied"]), &[])], Options::default());
    let notify = json!({ "jsonrpc": "2.0", "method": "host/notify", "params": { "message": message } });
    let ask = json!({ "jsonrpc": "2.0", "id": "late", "method": "track.show", "params": {} });
    // Logged as it's read, by whoever holds the plugin: the others were read by then.
    let last = json!({ "jsonrpc": "2.0", "id": "last", "result": 1 });
    let go = f._dir.path().join("go");
    f.echo_params(json!({ "late": { "after": go, "lines": [notify, ask, last] } })).unwrap();
    let (entered, logs) = (f.entered, f.logs.clone());
    f.plugins.shutdown();
    drop(f.plugins);
    std::fs::write(&go, "").unwrap();
    let read = "a response to \"last\", which no request is waiting for";
    eventually("its lines are read", || logs.lock().unwrap().iter().any(|(_, l)| l == read));
    // Taken, each would go on to a thread of its own: time for that to show.
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(notices.lock().unwrap().len(), 0);
    assert!(entered.lock().unwrap().try_recv().is_err(), "it isn't read");
    assert!(logs.lock().unwrap().iter().all(|(_, l)| !l.contains(&message)), "{:?}", logs.lock().unwrap());
}

/// The app's reads that don't say how a drive a plugin names is found.
struct Plain;

impl locallibrary::plugin::Reads for Plain {
    fn read(&self, method: &str, _: Value, _: &Ctx<'_>) -> locallibrary::Result<Value, locallibrary::api::RpcFailure> {
        unreachable!("nothing reads here: {method}")
    }
}

/// For any drive a plugin names, the app's reads find it as the host knows drives by default: one
/// known here (by its name) at its root, and none that isn't.
#[hegel::test(test_cases = 1)]
fn prop_a_drive_named_is_found_as_the_host_knows_drives(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use locallibrary::plugin::Reads;
    let f = Fixture::echo();
    for named in ["USB", "usb"] {
        assert_eq!(Plain.drive(named).unwrap().canonicalize().unwrap(), f.drive.canonicalize().unwrap());
    }
    let elsewhere: String = tc.draw(gs::from_regex("[A-Z]{3,8}").fullmatch(true)) + "-ELSEWHERE";
    assert_eq!(Plain.drive(&elsewhere).unwrap_err().code(), "not_found");
}
