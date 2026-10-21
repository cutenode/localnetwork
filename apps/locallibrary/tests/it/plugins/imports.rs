//! Plugins' importers on the command line (`import`), and their formats for sets and mixes
//! (`sets export --format`, `mix --format`).

use super::*;

/// The echo manifest as these tests register it, granting `sets:read`, with an importer
/// answering a JSON file as it is (`given`), and a set's and a mix's format writing a tracklist.
fn tracklists() -> String {
    echo().replace("capabilities = [\"library:read\", \"library:plan\"]", "capabilities = [\"library:read\", \"library:plan\", \"sets:read\"]").replace(
        "[[contributes.providers]]",
        "[[contributes.importers]]\nformat = \"given\"\ntitle = \"Given\"\nextension = \"json\"\n\
         provides = [\"fields\", \"cues\", \"tags\", \"sessions\"]\n\n\
         [[contributes.exporters]]\nformat = \"sheet\"\ntitle = \"Set sheet\"\nextension = \"txt\"\nsource = \"set\"\n\n\
         [[contributes.exporters]]\nformat = \"mixsheet\"\ntitle = \"Mix sheet\"\nextension = \"txt\"\nsource = \"mix\"\n\n\
         [[contributes.providers]]",
    )
}

fn tracklists_env() -> Env {
    let mut env = Env::new();
    env.plugins(&[&tracklists()]);
    env.demo("USB");
    env
}

/// A file is read with the importer for its extension (or the one `--format` names), what it
/// says added to the tracks it finds as an edit, shown and asked about first, with how many it
/// didn't find; once added, the same file has nothing more to add. The plays it logged are kept.
#[test]
fn a_file_is_imported_as_an_edit() {
    let env = tracklists_env();
    let tracks = env.json(&["tracks", "USB"]);
    let first = tracks["tracks"][0]["file_path"].as_str().unwrap().to_string();
    let list = env.outside("list.txt");
    std::fs::write(&list, format!("{first}\n/Contents/nowhere.mp3\n")).unwrap();
    let planned = env.ok(&["import", path(&list), "USB", "--dry-run"]);
    assert!(planned.contains("import 1 of 2 track(s) found on USB"), "{planned}");
    assert!(planned.contains("add My Tag Genre/Echo to 1 track(s)"), "{planned}");
    assert!(planned.contains("1 of the 2 track(s) left out: 1 aren't on the drive, 0 could be several"), "{planned}");
    let applied = env.ok(&["--yes", "import", path(&list), "USB"]);
    assert!(applied.contains("change(s) written"), "{applied}");
    assert!(env.ok(&["import", path(&list), "USB"]).contains("nothing to import"));

    let log = env.outside("log.json");
    let read = json!({ "sessions": [{ "name": "Friday", "plays": [{ "artist": "A", "title": "B", "started_at": "2026-10-03T22:00:00Z" }] }] });
    std::fs::write(&log, read.to_string()).unwrap();
    let kept = env.ok(&["--yes", "import", path(&log), "USB", "--format", "given"]);
    assert!(kept.contains("kept 1 session(s)"), "{kept}");
    let book = std::fs::read_to_string(env.store.join("sets.json")).unwrap();
    assert!(book.contains("\"tool\": \"echo.given\"") && book.contains("\"Friday\""), "{book}");
    let json = env.json(&["--yes", "import", path(&log), "USB"]);
    assert_eq!(json["report"]["sessions"].as_array().map(Vec::len), Some(1), "{json}");

    let other = env.outside("list.nml");
    std::fs::write(&other, "").unwrap();
    assert!(env.fails(2, &["import", path(&other), "USB"]).contains("say what reads it: --format FORMAT"));
    assert!(env.fails(2, &["import", path(&other), "USB", "--format", "nope"]).contains("invalid value 'nope'"));
    let option = env.fails(2, &["import", path(&list), "USB", "--option", "a=b"]);
    assert!(option.contains("this format has no options"), "{option}");
}

/// A set's format writes out a set, the plugin told which; a mix's a mix, its report kept for it.
#[test]
fn a_set_or_a_mix_is_written_in_a_plugins_format() {
    let env = tracklists_env();
    env.ok(&["sets", "new", "--title", "Friday", "--date", "2026-10-03"]);
    let out = env.outside("friday.txt");
    let written = env.ok(&["--yes", "sets", "export", "last", "--format", "sheet", "-o", path(&out)]);
    assert!(written.contains("wrote 1 file(s)"), "{written}");
    let id = env.json(&["sets", "show", "last"])["id"].as_str().unwrap().to_string();
    let planned = env.ok(&["sets", "export", "last", "--format", "sheet", "-o", path(&out), "--dry-run"]);
    assert!(planned.contains(&format!("set: {id}")), "{planned}");
    assert!(env.fails(2, &["sets", "export", "last", "--format", "sheet"]).contains("say where with -o"));
    assert!(env.fails(1, &["sets", "export", "last", "--dry-run"]).contains("--dry-run is for plugins' formats"));
    let wrong = env.fails(2, &["sets", "export", "last", "--format", "mixsheet", "-o", path(&out)]);
    assert!(wrong.contains("invalid value 'mixsheet'"), "a mix's format isn't a set's: {wrong}");

    let recording = env.outside("set.wav");
    std::fs::write(&recording, recording_of()).unwrap();
    let sheet = env.outside("mix.txt");
    let args = ["--yes", "mix", path(&recording), "--drive", "USB", "--format", "mixsheet", "--export", path(&sheet)];
    let written = env.ok(&args);
    assert!(written.contains("wrote 1 file(s)") && written.contains("mix: "), "{written}");
    assert!(sheet.is_file());
    let json = env.json(&args);
    assert_eq!(json["plan"]["exporter"], "mixsheet", "what the plugin wrote is what's answered: {json}");
    let two =
        env.fails(1, &["mix", path(&recording), path(&recording), "--format", "mixsheet", "--export", path(&sheet)]);
    assert!(two.contains("--format goes with a single mix"), "{two}");
}

/// A short recording, of nothing on the drive.
fn recording_of() -> Vec<u8> {
    recording(7, 12.0, 44_100, 16, 0)
}

/// The sidecar plans and applies an import, writes out a set in a plugin's format, and tells a
/// plugin the set the user is on.
#[test]
fn the_sidecar_imports_and_writes_out_sets() {
    let env = tracklists_env();
    let drive = path(&env.volumes.join("USB")).to_string();
    let mut s = Rpc::start(env.command());
    let result = |r: Value| r.get("result").cloned().unwrap_or_else(|| panic!("{r}"));
    let log = env.outside("log.json");
    let read = json!({ "sessions": [{ "name": "Friday", "plays": [{ "artist": "A", "title": "B", "started_at": "2026-10-03T22:00:00Z" }] }] });
    std::fs::write(&log, read.to_string()).unwrap();
    let import = json!({ "plugin": "echo", "importer": "given", "file": path(&log), "drive": drive });
    let planned = result(s.call(1, "plugins.import.plan", import.clone()));
    assert_eq!(planned["operation"], "echo.import.given");
    assert_eq!(planned["plays"], json!({ "entries": 1, "found": 0, "ambiguous": 0, "missing": 1 }));
    let applied = result(s.call(2, "plugins.import.apply", json!({ "plan_id": planned["plan_id"] })));
    assert_eq!((applied["edits"].is_null(), applied["sessions"].as_array().map(Vec::len)), (true, Some(1)));
    let listed = result(s.call(3, "sessions.list", json!({})));
    assert!(listed["sessions"].as_array().unwrap().iter().any(|s| s["source"] == "echo.given"), "{listed}");
    result(s.call(4, "plugins.import.apply", import));

    let set = result(s.call(5, "sets.new", json!({ "title": "Friday", "date": "2026-10-03" })))["set"].clone();
    let out = path(&env.outside("friday.txt")).to_string();
    let export = json!({ "plugin": "echo", "exporter": "sheet", "set": "Friday", "output": out });
    let planned = result(s.call(6, "plugins.export.plan", export));
    assert_eq!(planned["notes"], json!(["drive: ", format!("set: {}", set["id"].as_str().unwrap())]), "{planned}");
    let wrong = s.call(7, "plugins.export.plan", json!({ "plugin": "echo", "exporter": "sheet", "output": out }));
    assert_eq!(wrong["error"]["data"]["kind"], "invalid", "{wrong}");
    let context = json!({ "drive": "USB", "set": set["id"] });
    let run = json!({ "plugin": "echo", "command": "echo", "params": { "context": true }, "context": context });
    assert_eq!(result(s.call(8, "plugins.run", run))["set"], set["id"]);
}
