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

    let other = env.outside("list.lst");
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
    // An option the format doesn't declare is refused, as `--option` refuses it.
    let mut odd = import.clone();
    odd["options"] = json!({ "a": "b" });
    let refused = s.call(9, "plugins.import.plan", odd);
    assert_eq!(refused["error"]["message"], "this format has no options, not even \"a\"", "{refused}");
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

/// The tracklists manifest, its importer providing `provides`, with an option: `blob`, the file in
/// its scratch folder it answers in.
fn importing(provides: &[&str]) -> String {
    tracklists().replace(
        r#"provides = ["fields", "cues", "tags", "sessions"]"#,
        &format!("provides = {provides:?}\noptions = {{ type = \"object\", properties = {{ blob = {{ type = \"string\" }} }} }}"),
    )
}

/// For any file an importer reads (tracks on the drive or not, with keys, cues and My Tags, and
/// sessions), found by its extension or else named with `--format`, answered inline or in a file
/// (`--option`): a dry run says in text what `--json` plans (what it found, the edits, the
/// sessions kept, what's left out) and writes nothing, nor does a run not confirmed; confirmed,
/// it's made, and the same file again has no edits left. What the importer's manifest doesn't say
/// it provides, and an option it doesn't take, are refused.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_file_is_imported_as_its_plan_says(tc: hegel::TestCase) {
    use hegel::generators as gs;
    // Mostly every kind, so most files are read.
    let kinds = vec!["fields", "cues", "tags", "sessions"];
    let provides: Vec<&str> = match tc.draw(gs::weighted_booleans(0.7)) {
        true => kinds.clone(),
        false => tc.draw(gs::subsequences(kinds).min_size(1)),
    };
    let mut env = Env::new();
    env.plugins(&[&importing(&provides)]);
    env.demo("USB");
    let tracks = env.json(&["tracks", "USB"]);
    let paths: Vec<&str> =
        tracks["tracks"].as_array().unwrap().iter().map(|t| t["file_path"].as_str().unwrap()).collect();
    let entries: Vec<Value> = (0..tc.draw(gs::integers::<usize>().max_value(3)))
        .map(|k| {
            let on = tc.draw(gs::optional(gs::sampled_from(paths.clone())));
            let mut t =
                json!({ "match": { "path": on.map_or_else(|| format!("/Contents/nowhere/{k}.mp3"), String::from) } });
            if tc.draw(gs::booleans()) {
                t["key"] = json!(tc.draw(gs::sampled_from(vec!["8A", "3B"])));
            }
            if tc.draw(gs::booleans()) {
                let at = tc.draw(gs::integers::<u32>().max_value(5000));
                t["cues"] = json!([{ "hot_cue": tc.draw(gs::sampled_from(vec!["A", "H"])), "start_ms": at }]);
            }
            if tc.draw(gs::booleans()) {
                t["my_tags"] = json!([format!("Genre/Imp{}", tc.draw(gs::integers::<u8>().max_value(1)))]);
            }
            t
        })
        .collect();
    let logs: Vec<Value> = (0..tc.draw(gs::integers::<usize>().max_value(2)))
        .map(|i| {
            let plays: Vec<Value> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(2)))
                .map(|j| json!({ "artist": "A", "title": format!("T{i}{j}"), "started_at": format!("2026-10-03T22:{i}{j}:00Z") }))
                .collect();
            json!({ "name": format!("Night {i}"), "plays": plays })
        })
        .collect();
    let warnings: Vec<String> =
        (0..tc.draw(gs::integers::<usize>().max_value(1))).map(|w| format!("line {w}")).collect();
    let by_extension = tc.draw(gs::booleans());
    let file = env.outside(if by_extension { "read.json" } else { "read.lst" });
    std::fs::write(&file, json!({ "tracks": entries, "sessions": logs, "warnings": warnings }).to_string()).unwrap();
    let mut args = vec!["import", path(&file), "USB"];
    if !by_extension {
        // No importer reads .lst files: it's for `--format` to say.
        assert!(env.fails(2, &args).contains("say what reads it: --format FORMAT"));
        args.extend(["--format", "given"]);
    }
    if tc.draw(gs::booleans()) {
        args.extend(["--option", "blob=answer.json"]);
    }
    if tc.draw(gs::weighted_booleans(0.25)) {
        let refused = env.fails(2, &[&args[..], &["--option", "nope=1"]].concat());
        assert!(refused.contains("this format has no option \"nope\": its options are blob"), "{refused}");
    }
    let dry = [&args[..], &["--dry-run"]].concat();
    let has = |key: &str| entries.iter().any(|t| t.get(key).is_some());
    let kinds =
        [("fields", has("key")), ("cues", has("cues")), ("tags", has("my_tags")), ("sessions", !logs.is_empty())];
    if let Some((kind, _)) = kinds.iter().find(|(kind, has)| *has && !provides.contains(kind)) {
        let refused = env.fails(1, &dry);
        assert!(refused.contains(&format!("it read {kind}, which its manifest doesn't say it provides")), "{refused}");
        return;
    }

    let before = Env::files_in(&env.volumes);
    let planned = env.json(&dry);
    let plan = &planned["plan"];
    let found = entries.iter().filter(|t| !t["match"]["path"].as_str().unwrap().contains("/nowhere/")).count();
    assert_eq!(
        (plan["tracks"]["entries"].as_u64(), plan["tracks"]["found"].as_u64()),
        (Some(entries.len() as u64), Some(found as u64))
    );
    let names = |plan: &Value| -> Vec<String> {
        plan["sessions"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap().into()).collect()
    };
    assert_eq!(names(plan), logs.iter().map(|l| l["name"].as_str().unwrap().to_string()).collect::<Vec<_>>());
    let said: Vec<&str> = plan["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap()).collect();
    assert!(warnings.iter().all(|w| said.contains(&format!("Given: {w}").as_str())), "{said:?}");
    let noop = plan["noop"].as_bool().unwrap();
    assert_eq!(planned["outcome"], if noop { "nothing_to_do" } else { "dry_run" });
    let text = env.ok(&dry);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], format!("• {}", plan["summary"][0].as_str().unwrap()), "{text}");
    let changes: Vec<&str> = plan["edits"]["changes"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
    for c in &changes {
        assert!(lines.contains(&format!("  - {c}").as_str()), "{c}: {text}");
    }
    for s in plan["sessions"].as_array().unwrap() {
        let line = format!("  {} {} play(s)", s["name"].as_str().unwrap(), s["plays"].as_array().unwrap().len());
        assert!(lines.contains(&line.as_str()), "{line}: {text}");
    }
    for w in &said {
        assert!(lines.contains(&format!("! {w}").as_str()), "{w}: {text}");
    }
    assert_eq!(text.contains("nothing to import"), noop, "{text}");
    assert_eq!(Env::files_in(&env.volumes), before, "a dry run writes nothing");
    if noop {
        return;
    }
    // Without a terminal to ask on, it isn't confirmed.
    env.exits(3, &args);
    assert_eq!(Env::files_in(&env.volumes), before, "nor does a run not confirmed");
    assert!(!env.store.join("sets.json").exists());
    let applied = env.ok(&[&["--yes"][..], &args].concat());
    assert_eq!(applied.contains("change(s) written"), !changes.is_empty(), "{applied}");
    assert_eq!(applied.contains(&format!("kept {} session(s)", logs.len())), !logs.is_empty(), "{applied}");
    let again = env.json(&dry);
    assert_eq!(again["plan"]["edits"]["changes"], json!([]), "what it planned is there");
    let ids = |plan: &Value| plan["sessions"].as_array().unwrap().iter().map(|s| s["id"].clone()).collect::<Vec<_>>();
    assert_eq!(ids(&again["plan"]), ids(plan), "the same sessions");
}

/// For any set (by its title or `last`) or a recorded mix, written out in a plugin's format to a
/// file there already or not: a dry run says what it would write (the set, by its id) and writes
/// nothing; a file there already is overwritten only with `--replace` or once confirmed, never
/// without a terminal to ask on; otherwise the plugin writes it. A set's format needs `-o`, and
/// a mix's a single mix.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_set_or_a_mix_is_written_out_as_planned(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let env = tracklists_env();
    let titles: Vec<String> =
        (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3))).map(|i| format!("Night {i}")).collect();
    for (i, title) in titles.iter().enumerate() {
        env.ok(&["sets", "new", "--title", title, "--date", &format!("2026-10-0{}", i + 1)]);
    }
    let named = tc.draw(gs::sampled_from(vec!["last".to_string(), titles[0].clone()]));
    let id = env.json(&["sets", "show", &named])["id"].as_str().unwrap().to_string();
    let out = env.outside("night.txt");
    let there = tc.draw(gs::booleans());
    if there {
        std::fs::write(&out, "mine").unwrap();
    }
    let export = ["sets", "export", &named, "--format", "sheet"];
    assert!(env.fails(2, &export).contains("say where with -o"));
    let to = [&export[..], &["-o", path(&out)]].concat();
    let planned = env.json(&[&to[..], &["--dry-run"]].concat());
    assert_eq!((planned["outcome"].as_str(), planned["plan"]["destructive"].as_bool()), (Some("dry_run"), Some(there)));
    assert_eq!(planned["plan"]["notes"][1], format!("set: {id}"), "the set, by its id");
    assert_eq!(std::fs::read_to_string(&out).ok(), there.then(|| "mine".to_string()), "a dry run writes nothing");
    let (replace, yes) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    let args = [&["--yes"][..yes as usize], &to, &["--replace"][..replace as usize]].concat();
    if there && !replace && !yes {
        env.exits(3, &args);
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "mine", "not confirmed, it's left as it was");
        return;
    }
    assert!(env.ok(&args).contains(&format!("wrote 1 file(s) to {}", out.display())));
    // A set made on the command line has no tracks yet.
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "");

    if !tc.draw(gs::weighted_booleans(0.3)) {
        return;
    }
    let recording = env.outside("set.wav");
    std::fs::write(&recording, recording_of()).unwrap();
    let mix = ["mix", path(&recording), "--drive", "USB", "--format", "mixsheet", "--export", path(&out)];
    let two = env.fails(1, &[&mix[..2], &[path(&recording)], &mix[2..]].concat());
    assert!(two.contains("--format goes with a single mix"), "{two}");
    env.exits(3, &mix);
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "", "not confirmed, it's left as it was");
    let written = env.ok(&[&mix[..], &["--replace"]].concat());
    assert!(written.contains("mix: ") && written.contains("wrote 1 file(s)"), "{written}");
}
