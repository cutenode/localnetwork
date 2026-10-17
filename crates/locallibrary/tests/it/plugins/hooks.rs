//! The hooks the host drives: operations planned and applied as edit plans, exports confirmed
//! before they overwrite, analyzers' fields kept, and settings.

use locallibrary::api::{DriveRef, LibraryRef, PluginFieldsRequest};
use locallibrary::fields::{Analyzer, FieldStore, FieldValue};
use locallibrary::op::AnyPlanned;
use locallibrary::paths;
use loclib_plugin::ExportSource;

use super::*;

impl Fixture {
    fn plan(&self, params: Value) -> Result<locallibrary::op::Planned<locallibrary::edit::EditPlan>> {
        self.plugins.plan_op("echo", "playlist", params, self.context(), Some(&self.store), &Ctx::silent())
    }
}

#[test]
fn an_operation_is_planned_and_applied_as_an_edit() {
    let f = Fixture::echo();
    let planned = f.plan(json!({ "name": "From echo" })).unwrap();
    // Named for its plugin: never one of the operations locallibrary has.
    assert_eq!((&*planned.operation, planned.destructive), ("echo.playlist", false));
    assert_eq!(planned.summary[0], "make the playlist From echo");
    assert_eq!(planned.playlists_created, ["From echo"]);
    planned.apply(None, &Ctx::silent()).unwrap();
    assert!(common::library(&f.drive).playlists.iter().any(|p| p.name == "From echo"));
    let journal = api::edits_list(&DriveRef { drive: common::path(&f.drive) }).unwrap();
    assert_eq!(journal.len(), 1, "journaled, as any edit");

    let e = f.plan(json!({})).unwrap_err();
    assert!(e.to_string().contains("\"name\" is a required property"), "{e}");
    let elsewhere =
        f.plugins.plan_op("echo", "playlist", json!({ "name": "x" }), Context::default(), None, &Ctx::silent());
    assert_eq!(elsewhere.unwrap_err().to_string(), "playlist edits a drive: say which");
    let nope = f.plugins.plan_op("echo", "nope", Value::Null, f.context(), None, &Ctx::silent());
    assert_eq!(nope.unwrap_err().code(), "not_found");
}

#[test]
fn an_operations_edits_need_their_capabilities() {
    let f = Fixture::echo();
    let given = |edit: Value| f.plan(json!({ "name": "x", "edits": [edit] })).map_err(failure);
    let tag = json!({ "op": "my_tag", "action": "delete", "tag": "Genre/House" });
    assert_eq!(
        given(tag).unwrap_err(),
        "the Echo plugin failed: its edit 0 needs the tags:plan capability, which it doesn't have"
    );
    let art = |image: Value| json!({ "op": "set_artwork", "tracks": { "ids": [1] }, "image": image });
    let e = given(art(json!({ "base64": "AAAA" }))).unwrap_err();
    assert_eq!(e, "the Echo plugin failed: its edit 0 needs the artwork:plan capability, which it doesn't have");
    let e = given(art(json!({ "path": "/tmp/cover.jpg" }))).unwrap_err();
    assert_eq!(e, "the Echo plugin failed: its edit 0 names an image file: a plugin gives an image's bytes");
    let e = given(json!({ "op": "explode" })).unwrap_err();
    assert!(e.starts_with("the Echo plugin failed: its edit 0 isn't one: "), "{e}");
    f.plan(json!({ "name": "Doomed" })).unwrap().apply(None, &Ctx::silent()).unwrap();
    let delete = json!({ "op": "delete_playlist", "playlist": "Doomed" });
    let e = given(delete.clone()).unwrap_err();
    assert_eq!(
        e,
        "the Echo plugin failed: playlist removes from the library, which needs the library:delete capability"
    );

    // Granted, the plan is destructive, as its edits are.
    let f = Fixture::new(|dir| vec![spec(dir, &granting(&["library:delete"]), &[])], Options::default());
    f.plan(json!({ "name": "Doomed" })).unwrap().apply(None, &Ctx::silent()).unwrap();
    let planned = f.plan(json!({ "name": "x", "edits": [delete] })).unwrap();
    assert!(planned.destructive());
    assert_eq!((planned.summary[0].as_str(), planned.warnings()), ("as given", &["given".to_string()][..]));
}

#[test]
fn an_operation_is_planned_strictly() {
    let f = Fixture::echo();
    // An edit that doesn't apply fails the plan: it isn't skipped, as a replay's would be.
    let rename = json!({ "op": "rename_playlist", "playlist": "Nope", "name": "Yes" });
    let e = f.plan(json!({ "name": "x", "edits": [rename] })).unwrap_err();
    assert_eq!(e.code(), "not_found", "{e}");
    // Archive drives are refused, though this thread may write to them, when it's applied too
    // (after it's become one), and writing to one anyway isn't offered.
    let planned = f.plan(json!({ "name": "x" })).unwrap();
    api::drive_role(&f.store, &api::SetRole { drive: "USB".into(), role: Some("archive".into()) }).unwrap();
    locallibrary::op::allow_archive_writes(true);
    let planning = f.plan(json!({ "name": "x" })).unwrap_err();
    let applying = planned.apply(None, &Ctx::silent()).unwrap_err();
    let still = locallibrary::op::archive_writes_allowed();
    locallibrary::op::allow_archive_writes(false);
    for e in [planning, applying] {
        assert_eq!(e.reason(), Some(locallibrary::error::Reason::Archive), "{e}");
        assert!(e.detail().actions.iter().all(|a| a.method.is_some()), "no retry: {:?}", e.detail().actions);
    }
    assert!(still, "and still may, after");
    assert!(!common::library(&f.drive).playlists.iter().any(|p| p.name == "x"));
}

/// A plan worked out from files (a tracklist) is applied only while they're as they were.
#[test]
fn an_operations_plan_is_stale_once_a_file_it_read_changes() {
    let f = Fixture::echo();
    let list = f.store.root().parent().unwrap().join("set.txt");
    std::fs::write(&list, "one\n").unwrap();
    let from = |name: &str, input: &Path| {
        let create = json!({ "op": "create_playlist", "path": name });
        f.plan(json!({ "name": name, "edits": [create], "inputs": [input] }))
    };
    let playlists = || common::library(&f.drive).playlists.into_iter().map(|p| p.name).collect::<Vec<_>>();
    from("Kept", &list).unwrap().apply(None, &Ctx::silent()).unwrap();
    assert!(playlists().contains(&"Kept".to_string()));
    let planned = from("Stale", &list).unwrap();
    std::fs::write(&list, "two\n").unwrap();
    let e = planned.apply(None, &Ctx::silent()).unwrap_err();
    assert_eq!(e.code(), "stale_plan", "{e}");
    assert!(!playlists().contains(&"Stale".to_string()), "the drive is left as it was");
    let e = from("Relative", Path::new("set.txt")).map_err(failure).unwrap_err();
    assert_eq!(e, "the Echo plugin failed: it read set.txt, which isn't an absolute path");
    // Only files: reading a pipe or a device to check it could wait forever.
    #[cfg_attr(not(unix), allow(unused_mut, reason = "only Unix adds a pipe"))]
    let mut others = vec![f.store.root().to_path_buf(), list.with_file_name("gone.txt")];
    #[cfg(unix)]
    {
        let pipe = list.with_file_name("pipe");
        let c = std::ffi::CString::new(pipe.to_str().unwrap()).unwrap();
        // SAFETY: the path is a NUL-terminated string that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        others.push(pipe);
    }
    for other in others {
        let e = from("Other", &other).map_err(failure).unwrap_err();
        assert_eq!(e, format!("the Echo plugin failed: it read {}, which isn't a file", other.display()));
    }
}

#[test]
fn an_export_is_confirmed_before_it_overwrites() {
    let f = Fixture::echo();
    let out = f.store.root().parent().unwrap().join("out.m3u");
    let source = ExportSource { drive: "USB".into(), playlists: Vec::new(), history: Vec::new() };
    let plan = |options: Value| {
        f.plugins.export_plan("echo", "m3u", source.clone(), &out, options, Some(&f.store), &Ctx::silent())
    };
    let planned = plan(Value::Null).unwrap();
    assert!(!planned.destructive && planned.overwrites.is_empty());
    assert_eq!(planned.files, std::slice::from_ref(&out));
    assert!(!out.exists(), "a dry run writes nothing");
    let written = planned.apply(None, &Ctx::silent()).unwrap();
    assert_eq!(written.files, std::slice::from_ref(&out));
    let lines: Vec<String> = common::library(&f.drive).tracks.iter().map(|t| t.file_path.clone()).collect();
    assert_eq!(std::fs::read_to_string(&out).unwrap(), format!("#EXTM3U\n{}\n", lines.join("\n")));

    let again = plan(Value::Null).unwrap();
    assert_eq!(&*again.operation, "echo.export.m3u");
    assert!(again.destructive);
    assert_eq!(again.overwrites, std::slice::from_ref(&out));
    assert_eq!(again.summary, [format!("write 1 file(s) to {}", out.display()), "overwrite 1 of them".into()]);
    std::fs::write(&out, "mine").unwrap();
    assert_eq!(again.apply(None, &Ctx::silent()).unwrap_err().code(), "stale_plan");

    // Writing other files than it said is refused, naming them (written: the plugin writes).
    let extra = out.with_file_name("extra.txt");
    let e = plan(json!({ "extra": extra })).unwrap().apply(None, &Ctx::silent()).err().unwrap();
    assert_eq!(
        failure(e),
        format!("the Echo plugin failed: it wrote what it didn't say it would: {}", extra.display())
    );
    let outside = plan(json!({ "outside": f.drive.join("x.m3u") })).map_err(failure).err().unwrap();
    assert!(outside.contains("it would write") && outside.ends_with("outside the output"), "{outside}");
    let relative =
        f.plugins.export_plan("echo", "m3u", source.clone(), Path::new("out.m3u"), Value::Null, None, &Ctx::silent());
    assert_eq!(relative.err().unwrap().to_string(), "the export's output must be an absolute path, not out.m3u");
    // Never onto a drive, or into the store.
    for (root, what) in [(&f.drive, "the drive at"), (&f.store.root().to_path_buf(), "the store at")] {
        let e = f.plugins.export_plan(
            "echo",
            "m3u",
            source.clone(),
            &root.join("x.m3u"),
            Value::Null,
            Some(&f.store),
            &Ctx::silent(),
        );
        let e = e.err().unwrap().to_string();
        assert!(e.starts_with(&format!("a plugin's export isn't written to {what}")), "{e}");
    }
    let nope = f.plugins.export_plan("echo", "nml", source, &out, Value::Null, None, &Ctx::silent());
    assert_eq!(nope.err().unwrap().code(), "not_found");
}

#[test]
fn an_analyzers_fields_are_kept_and_read_back() {
    let f = Fixture::echo();
    let tracks = common::library(&f.drive).tracks;
    let analyzed = f.plugins.analyze("echo", "size", "USB", None, &f.store, &Ctx::silent()).unwrap();
    assert_eq!((analyzed.analyzed, analyzed.kept, analyzed.failed), (tracks.len(), 0, 0));
    let again = f.plugins.analyze("echo", "size", "USB", Some(&[tracks[0].id]), &f.store, &Ctx::silent()).unwrap();
    assert_eq!((again.analyzed, again.kept), (0, 1), "a track analysed already is left as it is");

    let req = PluginFieldsRequest { library: LibraryRef::from(common::path(&f.drive)), tracks: None, plugin: None };
    let installed = f.plugins.analyzers();
    let read = api::track_fields_of(LibraryCache::none(), Some(&f.store), &req, &installed).unwrap();
    let audio = |t: &locallibrary::Track| paths::to_fs(&f.drive, &paths::key(&t.file_path));
    let bytes: Vec<(u32, Value)> =
        tracks.iter().map(|t| (t.id, json!(std::fs::metadata(audio(t)).unwrap().len()))).collect();
    let got: Vec<(u32, Value)> = read
        .tracks
        .iter()
        .map(|t| (t.track, serde_json::to_value(&t.plugins["echo"]["size"]["bytes"]).unwrap()))
        .collect();
    assert_eq!(got, bytes);
    // Another version's values aren't the installed plugin's.
    let old = Analyzer { plugin: "echo".into(), version: "0.0.9".into(), analyzer: "size".into() };
    let mine = [("bytes".to_string(), FieldValue::Integer(1))].into_iter().collect();
    FieldStore::in_store(&f.store).put(&audio(&tracks[0]), &old, &mine).unwrap();
    let read = api::track_fields_of(LibraryCache::none(), Some(&f.store), &req, &installed).unwrap();
    assert_eq!(serde_json::to_value(&read.tracks[0].plugins["echo"]["size"]["bytes"]).unwrap(), bytes[0].1);

    let e = f.plugins.analyze("echo", "size", "USB", Some(&[9999]), &f.store, &Ctx::silent()).unwrap_err();
    assert_eq!(e.code(), "not_found");
    let nope = f.plugins.analyze("echo", "tempo", "USB", None, &f.store, &Ctx::silent());
    assert_eq!(nope.unwrap_err().code(), "not_found");
}

#[test]
fn an_analyzers_fields_must_be_of_their_types() {
    let f = Fixture::new(|dir| vec![spec(dir, ECHO, &["--wrong-fields"])], Options::default());
    let tracks = common::library(&f.drive).tracks;
    std::fs::remove_file(paths::to_fs(&f.drive, &paths::key(&tracks[1].file_path))).unwrap();
    let warnings = common::Warnings::default();
    let analyzed = f.plugins.analyze("echo", "size", "USB", None, &f.store, &warnings.ctx()).unwrap();
    assert_eq!((analyzed.analyzed, analyzed.failed), (0, 2));
    let said = warnings.said();
    assert_eq!(said[0], format!("{}: its audio file isn't on the drive", tracks[1].file_path));
    let wrong = "the Echo plugin failed: its field bytes isn't the integer size declares";
    assert_eq!(said[1], format!("{}: {wrong}", tracks[0].file_path));
    let req = PluginFieldsRequest { library: LibraryRef::from(common::path(&f.drive)), tracks: None, plugin: None };
    let read = api::track_fields_of(LibraryCache::none(), Some(&f.store), &req, &f.plugins.analyzers()).unwrap();
    assert!(read.tracks.is_empty(), "nothing was kept");
}

/// The echo plugin, with an analyzer (`order`) declaring `b` before `a`, which the test plugin
/// answers the other way round.
fn ordered() -> Fixture {
    let manifest = ECHO.replace(
        "fields = { bytes = \"integer\" }\n",
        "fields = { bytes = \"integer\" }\n\n[[contributes.analyzers]]\nname = \"order\"\ntitle = \"Order\"\n\
         fields = { b = \"integer\", a = \"integer\" }\n",
    );
    Fixture::new(|dir| vec![spec(dir, &manifest, &[])], Options::default())
}

/// They're columns: kept and read back in the order the analyzer declares, whatever the answer's.
#[test]
fn an_analyzers_fields_keep_its_order() {
    let f = ordered();
    let tracks = common::library(&f.drive).tracks;
    let analyzed = f.plugins.analyze("echo", "order", "USB", None, &f.store, &Ctx::silent()).unwrap();
    assert_eq!((analyzed.analyzed, analyzed.failed), (tracks.len(), 0));
    let req = PluginFieldsRequest { library: LibraryRef::from(common::path(&f.drive)), tracks: None, plugin: None };
    let read = api::track_fields_of(LibraryCache::none(), Some(&f.store), &req, &f.plugins.analyzers()).unwrap();
    assert_eq!(read.tracks.len(), tracks.len());
    for t in &read.tracks {
        let names: Vec<&str> = t.plugins["echo"]["order"].keys().map(String::as_str).collect();
        assert_eq!(names, ["b", "a"], "track {}", t.track);
    }
}

/// A track the plugin fails on is named once, by its path on the drive, though the plugin named
/// its audio file.
#[test]
fn a_track_an_analyzer_fails_on_is_named_by_its_path_on_the_drive() {
    let f = ordered();
    let tracks = common::library(&f.drive).tracks;
    std::fs::write(paths::to_fs(&f.drive, &paths::key(&tracks[0].file_path)), "").unwrap();
    let warnings = common::Warnings::default();
    let analyzed = f.plugins.analyze("echo", "order", "USB", None, &f.store, &warnings.ctx()).unwrap();
    assert_eq!((analyzed.analyzed, analyzed.failed), (tracks.len() - 1, 1));
    assert_eq!(warnings.said(), [format!("{} is empty (the Echo plugin)", tracks[0].file_path)]);
}

#[test]
fn analysing_stops_when_cancelled() {
    let f = Fixture::echo();
    let cancel = locallibrary::CancelToken::new();
    let cancelling = cancel.clone();
    let on_event = move |_: &locallibrary::Event| cancelling.cancel();
    let e = f.plugins.analyze("echo", "size", "USB", None, &f.store, &Ctx::new(&on_event, cancel)).unwrap_err();
    assert!(matches!(e, Error::Cancelled), "{e}");
}

#[test]
fn settings_are_checked_kept_and_handed_to_hooks() {
    let f = Fixture::echo();
    let settings = |s: Option<&Store>| f.plugins.settings("echo", s).unwrap();
    assert_eq!(serde_json::to_value(settings(None)).unwrap(), json!({ "greeting": "hello" }));
    let set = |key: &str, value: Option<Value>| f.plugins.set_setting("echo", &f.store, key, value);
    assert_eq!(set("greeting", Some(json!(5))).unwrap_err().to_string(), "greeting is a string, not 5");
    assert_eq!(set("volume", Some(json!(5))).unwrap_err().code(), "not_found");
    let changed = set("greeting", Some(json!("hi"))).unwrap();
    assert_eq!(serde_json::to_value(changed).unwrap(), json!({ "greeting": "hi" }));
    let context = f.echo_params(json!({ "context": true })).unwrap();
    let id = locallibrary::Device::open(&f.drive).unwrap().id().unwrap().to_string();
    assert_eq!(context, json!({ "drive": id, "settings": { "greeting": "hi" } }));
    // Kept as written: what no longer fits the setting (an older plugin's) is its default.
    let file = f.store.root().join("plugins/echo/settings.json");
    assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(&file).unwrap()).unwrap(), json!({ "greeting": "hi" }));
    std::fs::write(&file, r#"{"greeting": 1, "gone": true}"#).unwrap();
    assert_eq!(serde_json::to_value(settings(Some(&f.store))).unwrap(), json!({ "greeting": "hello" }));
    set("greeting", Some(json!("hey"))).unwrap();
    let unset = set("greeting", None).unwrap();
    assert_eq!(serde_json::to_value(unset).unwrap(), json!({ "greeting": "hello" }));

    // One that can't be read leaves the defaults, said in the log, and is written anew.
    std::fs::write(&file, "{ not json").unwrap();
    let context = f.echo_params(json!({ "context": true })).unwrap();
    assert_eq!(context["settings"], json!({ "greeting": "hello" }));
    assert!(
        f.logs
            .lock()
            .unwrap()
            .iter()
            .any(|(id, line)| id == "echo" && line.starts_with("its settings are its defaults: "))
    );
    set("greeting", Some(json!("again"))).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&file).unwrap()).unwrap(),
        json!({ "greeting": "again" })
    );
}

/// A plan kept doesn't keep its plugin hosted: once the host is gone, it's stale.
#[test]
fn an_export_kept_doesnt_keep_its_plugin() {
    let Fixture { _dir, store, plugins, .. } = Fixture::echo();
    let out = store.root().parent().unwrap().join("out.m3u");
    let source = ExportSource { drive: "USB".into(), playlists: Vec::new(), history: Vec::new() };
    let planned = plugins.export_plan("echo", "m3u", source, &out, Value::Null, Some(&store), &Ctx::silent()).unwrap();
    drop(plugins);
    let e = planned.apply(None, &Ctx::silent()).err().unwrap();
    assert_eq!(
        (e.code(), e.to_string().as_str()),
        ("stale_plan", "the echo plugin stopped since the plan was made; plan again")
    );
    assert!(!out.exists());
}
