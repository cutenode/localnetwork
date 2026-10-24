//! Plugins on the command line and in the sidecar, with the plugin host's test plugin registered
//! for these tests alone (`LOCALLIBRARY_TEST_PLUGINS`: manifests the binary serves with it).

use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;
use std::sync::LazyLock;

use loclib_plugin::Manifest;
use serde_json::{Value, json};

use crate::support::{Env, Rpc, echo_plugin, path, recording};

mod accounts;
mod checks;
mod choose;
mod destinations;
mod events;
mod imports;
mod panels;
mod props;
mod providers;
mod sources;

/// The echo manifest as these tests register it ([`echo_plugin`]): its `echo` command at
/// `locallibrary say` taking flags, and its operation at `locallibrary playlist fresh`.
fn echo() -> String {
    let params = r#"params = { type = "object", properties = { name = { type = "string" }, times = { type = "integer" }, loud = { type = "boolean" }, file = { type = "string", x-path = true } } }
cli = "say""#;
    echo_plugin()
        .replacen("params = { type = \"object\" }", params, 1)
        .replace("title = \"Make a playlist\"", "title = \"Make a playlist\"\ncli = \"playlist fresh\"")
}

/// The plugins locallibrary ships, as it lists them (before the tests' own): one for every folder in
/// the repository's `plugins/`, in alphabetical order. Read from the folders, so a plugin
/// `plugin list` leaves out fails the tests that compare the two.
static SHIPPED: LazyLock<Vec<&str>> = LazyLock::new(|| {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
    let mut ids: Vec<&str> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        // Kept for the tests' whole run, as ids they put beside their own plugins'.
        .map(|path| &*path.file_name().unwrap().to_str().unwrap().to_owned().leak())
        .collect();
    ids.sort();
    ids
});

/// Every plugin that ships is in the docs: plugins.md's list and how-it-works.md's layout. A new
/// plugin left out of them fails here.
#[test]
fn every_shipped_plugin_is_documented() {
    let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs");
    let plugins = std::fs::read_to_string(docs.join("plugins.md")).unwrap();
    let how = std::fs::read_to_string(docs.join("how-it-works.md")).unwrap();
    for id in SHIPPED.iter() {
        assert!(plugins.contains(&format!("\n- **{id}**:")), "docs/plugins.md doesn't list {id}");
        assert!(how.contains(&format!("\nplugins/{id}/ ")), "docs/how-it-works.md's layout doesn't have plugins/{id}/");
    }
}

/// An [`Env`] with the echo plugin, and the demo drive USB (two tracks, with audio).
fn env() -> Env {
    let mut env = Env::new();
    env.plugins(&[&echo()]);
    env.demo("USB");
    env
}

/// A secret setting is read from stdin (`--secret`), never from the command line, kept apart (a
/// file only you can read, here: `LOCALLIBRARY_SESSION_STORE=file`) and only ever shown as set or
/// not, by the command line and the sidecar, which takes one too.
#[test]
fn a_secret_setting_is_read_from_stdin_and_never_shown() {
    let mut env = Env::new();
    let secret = "\n[[contributes.settings]]\nkey = \"token\"\ntitle = \"Token\"\ntype = \"secret\"\n";
    env.plugins(&[&format!("{}{secret}", echo())]);
    let run = |args: &[&str], stdin: &str| {
        let mut child = env
            .command()
            .env("LOCALLIBRARY_SESSION_STORE", "file")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        let said = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
        (out.status.code().unwrap(), said(out.stdout), said(out.stderr))
    };
    let (code, _, said) = run(&["plugin", "settings", "echo", "token", "hunter2"], "");
    assert_eq!(code, 2, "{said}");
    assert!(
        said.contains("a secret isn't given on the command line, which keeps it in your shell's history"),
        "{said}"
    );
    let (code, _, said) = run(&["plugin", "settings", "echo", "greeting", "--secret"], "x\n");
    assert!(code == 2 && said.contains("greeting isn't a secret: give its value"), "{said}");
    let (code, _, said) = run(&["-y", "plugin", "settings", "echo", "token", "--secret"], "\n");
    assert!(code == 2 && said.contains("no token came on stdin"), "{said}");
    let (code, out, err) = run(&["-y", "plugin", "settings", "echo", "token", "--secret"], "hunter2\n");
    let said = out + &err;
    assert_eq!((code, said.contains("token is set"), said.contains("hunter2")), (0, true, false), "{said}");
    let (_, shown, _) = run(&["plugin", "settings", "echo"], "");
    assert!(shown.contains("token     set  Token") && !shown.contains("hunter2"), "{shown}");
    let (_, json, _) = run(&["--json", "plugin", "info", "echo"], "");
    let json: Value = serde_json::from_str(&json).unwrap();
    assert_eq!((&json["secrets"], json["settings"].get("token")), (&json!({ "token": true }), None));
    let file = env.store.join("secrets/plugins/echo.json");
    let kept = std::fs::read_to_string(&file).unwrap();
    assert!(kept.contains("hunter2") && !env.store.join("plugins/echo/settings.json").exists());

    // When whether it's set can't be known, it's said so, and the rest is shown.
    std::fs::write(&file, "not json").unwrap();
    let (code, shown, _) = run(&["plugin", "info", "echo"], "");
    assert_eq!(code, 0, "{shown}");
    assert!(shown.contains("Token: unknown") && shown.contains("Greeting: "), "{shown}");
    assert!(shown.contains("whether its secrets are set isn't known: "), "{shown}");
    let (_, json, _) = run(&["--json", "plugin", "settings", "echo"], "");
    let json: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(json["secrets"], json!({ "token": null }));
    assert!(json["secrets_unknown"].is_string() && json["settings"]["greeting"].is_string(), "{json}");
    std::fs::write(&file, kept).unwrap();

    // The sidecar takes one, and answers whether it's set.
    let mut s = Rpc::start({
        let mut c = env.command();
        c.env("LOCALLIBRARY_SESSION_STORE", "file");
        c
    });
    let set = json!({ "plugin": "echo", "key": "token", "value": "letmein" });
    let answer = s.call(1, "plugins.settings.set", set);
    assert_eq!(answer["result"]["secrets"], json!({ "token": true }));
    assert!(!answer.to_string().contains("letmein"));
    let answer = s.call(2, "plugins.settings.set", json!({ "plugin": "echo", "key": "token", "value": null }));
    assert_eq!(answer["result"]["secrets"], json!({ "token": false }));
    assert!(!file.exists());
}

/// `plugin serve` speaks the protocol on stdout and writes nothing else there; an id that's no
/// plugin's is refused.
#[test]
fn a_plugin_serves_on_stdout_alone() {
    let env = env();
    let mut child = env
        .command()
        .args(["plugin", "serve", "echo"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let hello = json!({ "protocol": loclib_plugin::PROTOCOL_VERSION, "host": { "name": "test", "version": "0" } });
    let mut stdin = child.stdin.take().unwrap();
    for (id, method, params) in [(1, "hello", hello), (2, "describe", json!({}))] {
        writeln!(stdin, "{}", json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).unwrap();
    }
    drop(stdin);
    let lines: Vec<Value> = BufReader::new(child.stdout.take().unwrap())
        .lines()
        .map(|l| serde_json::from_str(&l.unwrap()).unwrap())
        .collect();
    assert!(child.wait().unwrap().success());
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0]["result"]["plugin"], json!({ "id": "echo", "version": "0.1.0" }));
    let manifest = serde_json::to_value(Manifest::parse(&echo()).unwrap()).unwrap();
    assert_eq!(lines[1]["result"], manifest);

    let r = env.run(&["plugin", "serve", "nope"]);
    assert_eq!((r.code, r.out.as_str()), (1, ""), "{}", r.err);
    assert!(r.err.contains("not found: plugin \"nope\""), "{}", r.err);
}

/// `plugin` lists the plugins, what each may do and adds; `plugin info` has one in full, and
/// `plugin settings` shows, sets (asking first) and resets its settings.
#[test]
fn plugins_are_listed_and_their_settings_kept() {
    let ids = |listed: &Value| listed.as_array().unwrap().iter().map(|p| p["id"].clone()).collect::<Vec<_>>();
    assert_eq!(ids(&Env::new().json(&["plugin", "list"])), *SHIPPED);
    let env = env();
    let list = env.ok(&["plugin"]);
    assert!(list.contains("echo  Echo 0.1.0"), "{list}");
    assert!(list.contains("may read your libraries' tracks and playlists"), "{list}");
    let adds =
        "adds commands echo, titles, steps, wait; destinations folder, nowhere; operation playlist; analyzer size";
    assert!(list.contains(adds), "{list}");
    let listed = env.json(&["plugin", "list"]);
    assert_eq!(ids(&listed), [&SHIPPED[..], &["echo"]].concat());
    let echo = &listed[SHIPPED.len()];
    assert_eq!(echo["running"], json!(false));
    assert_eq!(echo["capabilities"][1]["capability"], "library:plan");

    let info = env.ok(&["plugin", "info", "echo"]);
    for line in ["Echo (echo) 0.1.0", "locallibrary say", "locallibrary echo titles", "locallibrary playlist fresh"] {
        assert!(info.contains(line), "{line}: {info}");
    }
    // With the menus that offer each.
    assert!(crate::support::line(&info, "locallibrary echo titles").ends_with("(menus: drive)"), "{info}");
    let fresh = crate::support::line(&info, "locallibrary playlist fresh");
    assert!(fresh.ends_with("Make a playlist (menus: track, playlist, drive)"), "{info}");
    let described = env.json(&["plugin", "info", "echo"]);
    assert_eq!(
        (&described["manifest"]["id"], &described["settings"]),
        (&json!("echo"), &json!({ "greeting": "hello" }))
    );
    assert!(env.fails(1, &["plugin", "info", "nope"]).contains("error: not found: plugin \"nope\""));

    assert!(env.ok(&["plugin", "settings", "echo"]).contains("greeting  hello  Greeting (default)"));
    assert_eq!(
        env.exits(3, &["plugin", "settings", "echo", "greeting", "hi"]).lines().next(),
        Some("• set the echo plugin's greeting to hi")
    );
    let settings = |env: &Env| env.json(&["plugin", "settings", "echo", "greeting"]);
    let none = json!({});
    assert_eq!(
        settings(&env),
        json!({ "plugin": "echo", "settings": { "greeting": "hello" }, "secrets": none, "schema": 1 })
    );
    assert!(env.ok(&["-y", "plugin", "settings", "echo", "greeting", "hi"]).contains("greeting is hi"));
    assert_eq!(settings(&env)["settings"]["greeting"], "hi");
    assert!(env.ok(&["-y", "plugin", "settings", "echo", "greeting", "--reset"]).contains("greeting is hello"));
    assert_eq!(settings(&env)["settings"]["greeting"], "hello");
    assert!(
        env.fails(2, &["plugin", "settings", "echo", "volume"]).contains("the echo plugin has no setting \"volume\"")
    );
}

/// A command runs where its manifest puts it, its params given as flags of their types (a file
/// made absolute) or as JSON; a view is drawn as a table, or given as it is with `--json`; and
/// the plugin's errors are locallibrary's, of their kinds.
#[test]
fn commands_run_with_flags_from_their_schemas() {
    let env = env();
    let said = env.json(&["say", "--name", "x", "--times", "2", "--loud", "--file", "list.txt"]);
    // Where the command ran.
    let file = std::env::current_dir().unwrap().join("list.txt");
    assert_eq!(said, json!({ "name": "x", "times": 2, "loud": true, "file": path(&file), "schema": 1 }));
    // A headless answer is printed without `--json` too.
    assert_eq!(env.ok(&["say", "--name", "x"]), "{\n  \"name\": \"x\"\n}\n");
    assert_eq!(env.json(&["echo", "echo", "--params", r#"{"times": 1}"#, "--times", "3"])["times"], 3);
    assert!(env.fails(2, &["say", "--times", "many"]).contains("--times takes a whole number, not \"many\""));
    // The plugin checks its params against its schema.
    assert!(env.fails(1, &["say", "--params", r#"{"times": "x"}"#]).contains("/times"));

    let table = env.ok(&["echo", "titles", "USB"]);
    assert_eq!(table.lines().collect::<Vec<_>>(), ["USB", "Title", "Demo Track 1", "Demo Track 2"]);
    let view = env.json(&["echo", "titles", "USB"]);
    assert_eq!((&view["kind"], &view["rows"][1]), (&json!("table"), &json!(["Demo Track 2"])));

    let failing = r#"{"fail": "not_found", "message": "no list Friday"}"#;
    let out = env.exits(1, &["--json", "echo", "echo", "--params", failing]);
    let error: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(error["error"]["kind"], "not_found", "{error}");
    assert_eq!(error["error"]["message"], "not found: no list Friday (the Echo plugin)");
}

/// A command piped into a reader that has stopped (`| head -1`) ends as a built-in one does,
/// stopped by SIGPIPE rather than failing to write.
#[cfg(unix)]
#[test]
fn a_command_whose_reader_stopped_ends_quietly() {
    use std::os::unix::process::ExitStatusExt;
    let env = env();
    let mut child =
        env.command().args(["say", "--name", "x"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.signal(), Some(libc::SIGPIPE), "{}", String::from_utf8_lossy(&out.stderr));
}

/// An operation is planned, shown and asked about as an edit is (`--dry-run`, `--yes`), then
/// applied by locallibrary, at its own place or the plugin's.
#[test]
fn operations_are_confirmed_and_applied_as_edits_are() {
    let env = env();
    let before = Env::files_in(&env.volumes);
    let plan = env.ok(&["echo", "playlist", "USB", "--name", "Fresh", "--dry-run"]);
    assert!(plan.starts_with("make the playlist Fresh\nUSB: export.pdb"), "{plan}");
    assert!(plan.contains("+ Fresh"), "{plan}");
    env.exits(3, &["playlist", "fresh", "USB", "--name", "Fresh"]);
    assert_eq!(Env::files_in(&env.volumes), before, "nothing written without a yes");

    let applied = env.json(&["--yes", "playlist", "fresh", "USB", "--name", "Fresh"]);
    assert_eq!((&applied["applied"], &applied["plan"]["operation"]), (&json!(true), &json!("echo.playlist")));
    assert!(env.ok(&["playlist", "ls", "USB"]).contains("Fresh"));
    let again = env.json(&["--yes", "echo", "playlist", "USB", "--name", "Other"]);
    assert_eq!(again["outcome"], "applied");
}

/// A plugin's format is one `export --format` takes: a dry run shows what goes in and each file
/// (what it holds, its size), writing nothing, and overwriting is asked about first, the file
/// marked (or not, with `--replace`).
#[test]
fn plugins_formats_ask_before_overwriting() {
    let env = env();
    let out = env.outside("out.m3u");
    let export = |more: &[&'static str]| {
        let mut args = vec!["export".to_string(), "USB".into(), "--format".into(), "echo-m3u".into(), "-o".into()];
        args.push(path(&out).into());
        args.extend(more.iter().map(|m| m.to_string()));
        args
    };
    let planned = env.ok(&strs(&export(&["--dry-run"])));
    assert!(planned.contains("\n• 2 tracks\n"), "{planned}");
    assert!(planned.contains(&format!("\n  {}  playlist, ", out.display())), "{planned}");
    assert!(!out.exists());
    assert!(env.ok(&strs(&export(&[]))).contains("wrote 1 file(s)"));
    assert!(std::fs::read_to_string(&out).unwrap().starts_with("#EXTM3U\n/Contents/"));
    std::fs::write(&out, "mine").unwrap();
    let asked = env.exits(3, &strs(&export(&[])));
    assert!(asked.contains("overwrite 1 of them") && asked.contains(" (already there)\n"), "{asked}");
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "mine");
    env.ok(&strs(&export(&["--replace"])));
    assert!(std::fs::read_to_string(&out).unwrap().starts_with("#EXTM3U"));
    assert!(
        env.fails(1, &["export", "USB", "-o", path(&out), "--option", "a=b"])
            .contains("--option is for plugins' formats")
    );
    assert!(env.fails(1, &strs(&export(&["--split"]))).contains("are for locallibrary's own formats"));
}

/// A plugin's format is written into folders that aren't there yet, made as it's written.
#[test]
fn plugins_formats_make_the_folders_they_write_into() {
    let env = env();
    let out = env.outside("new/dir/list.m3u");
    env.ok(&["--yes", "export", "USB", "--format", "echo-m3u", "-o", path(&out)]);
    assert!(std::fs::read_to_string(&out).unwrap().starts_with("#EXTM3U"));
}

/// `analyze` runs the analyzers on a drive's tracks once; `track` shows what they worked out.
#[test]
fn analyzers_fill_in_track_details() {
    let env = env();
    // The demo drive's audio files hold no audio the sound plugin could measure: echo's analyzer
    // only reads their size.
    assert!(env.ok(&["analyze", "USB", "--analyzer", "size"]).contains("size (echo): 2 tracks analyzed, 0 already"));
    assert!(env.ok(&["analyze", "USB", "--plugin", "echo"]).contains("0 tracks analyzed, 2 already"));
    let shown = env.json(&["track", "USB", "--track", "1"]);
    let audio = env.volumes.join("USB").join(shown[0]["track"]["file_path"].as_str().unwrap().trim_start_matches('/'));
    let bytes = std::fs::metadata(audio).unwrap().len();
    assert_eq!(shown[0]["fields"], json!({ "echo": { "size": { "bytes": bytes } } }));
    assert!(env.ok(&["track", "USB", "--track", "1"]).contains(&format!("echo.bytes  {bytes}\n")));
    assert!(env.fails(2, &["analyze", "USB", "--analyzer", "tempo"]).contains("no plugin has an analyzer \"tempo\""));
    // Without the plugin, nothing it worked out is shown.
    let mut without = env;
    without.plugins(&[]);
    assert!(without.json(&["track", "USB", "--track", "1"])[0].get("fields").is_none());
}

/// `tracks` filters and sorts by what analyzers worked out (`echo.bytes:>2000`, `-echo.bytes`),
/// as the sidecar's `tracks.query` does: a track without a value isn't matched and sorts last; a
/// field no analyzer declares, or a value its type can't hold, is refused.
#[test]
fn tracks_are_filtered_and_sorted_by_what_analyzers_worked_out() {
    let env = env();
    let listed = env.json(&["tracks", "USB"]);
    let file = |i: usize| {
        env.volumes.join("USB").join(listed["tracks"][i]["file_path"].as_str().unwrap().trim_start_matches('/'))
    };
    let ids =
        |page: &Value| page["tracks"].as_array().unwrap().iter().map(|t| t["id"].as_u64().unwrap()).collect::<Vec<_>>();
    let (first, second) = (listed["tracks"][0]["id"].as_u64().unwrap(), listed["tracks"][1]["id"].as_u64().unwrap());
    // Not analysed yet: no track has a value.
    assert_eq!(env.json(&["tracks", "USB", "-f", "echo.bytes:>0"])["total"], 0);
    std::fs::write(file(1), "x".repeat(4000)).unwrap();
    env.ok(&["analyze", "USB", "--analyzer", "size"]);
    assert_eq!(ids(&env.json(&["tracks", "USB", "-f", "echo.bytes:>2000"])), [second]);
    assert_eq!(ids(&env.json(&["tracks", "USB", "-f", "-echo.bytes:4000"])), [first]);
    assert_eq!(ids(&env.json(&["tracks", "USB", "--sort", "-echo.bytes"])), [second, first]);
    assert_eq!(ids(&env.json(&["tracks", "USB", "--sort", "echo.bytes"])), [first, second]);
    let shown = env.ok(&["tracks", "USB", "-f", "echo.bytes:>2000", "--columns", "id"]);
    assert_eq!(shown.lines().filter(|l| l.trim() == second.to_string().as_str()).count(), 1, "{shown}");
    // A track whose file changed has no value: it isn't matched, and sorts last either way.
    std::fs::write(file(0), "y".repeat(5000)).unwrap();
    assert_eq!(ids(&env.json(&["tracks", "USB", "-f", "echo.bytes:>0"])), [second]);
    assert_eq!(ids(&env.json(&["tracks", "USB", "--sort", "-echo.bytes"])), [second, first]);
    assert_eq!(ids(&env.json(&["tracks", "USB", "--sort", "echo.bytes"])), [second, first]);
    let err = env.fails(1, &["tracks", "USB", "-f", "echo.byte:1"]);
    assert!(err.contains("there's no field \"echo.byte\" (did you mean echo.bytes?)"), "{err}");
    assert!(env.fails(1, &["tracks", "USB", "-f", "echo.bytes:many"]).contains("it's a number"));
    assert!(env.fails(1, &["tracks", "USB", "--sort", "echo.byte"]).contains("did you mean echo.bytes?"));

    // The sidecar's `tracks.query` the same.
    let mut s = Rpc::start(env.command());
    let query = json!({ "drive": "USB", "filter": "echo.bytes:>0", "sort": "-echo.bytes" });
    let page = s.call(1, "tracks.query", query);
    assert_eq!(ids(&page["result"]), [second], "{page}");
    let wrong = s.call(2, "tracks.query", json!({ "drive": "USB", "filter": "echo.byte:1" }));
    assert_eq!(wrong["error"]["data"]["kind"], "invalid", "{wrong}");
}

/// The sound plugin locallibrary ships measures each track's tempo, key and loudness from its
/// audio, once, and `track` shows every column it declares as `sound.FIELD` (its waveform isn't
/// one).
#[test]
fn the_sound_plugin_measures_tracks() {
    let env = Env::new();
    let album = env.demo("USB").join("Contents/Loopmasters/UnknownAlbum");
    for (n, seed) in [(1, 7919), (2, 31)] {
        std::fs::write(album.join(format!("Demo Track {n}.mp3")), recording(seed, 30.0, 44_100, 16, 0)).unwrap();
    }
    let analyzed = env.ok(&["analyze", "USB"]);
    assert!(analyzed.contains("tempo-key-loudness (sound): 2 tracks analyzed, 0 already"), "{analyzed}");
    assert!(env.ok(&["analyze", "USB"]).contains("0 tracks analyzed, 2 already"));
    let manifest = Manifest::parse(locallibrary_plugin_sound::MANIFEST).unwrap();
    let columns = manifest.contributes.analyzers[0].fields.iter().filter(|(_, t)| !t.is_structured());
    let declared: Vec<&String> = columns.map(|(name, _)| name).collect();
    let shown = env.json(&["track", "USB", "--track", "1"]);
    let fields = shown[0]["fields"]["sound"]["tempo-key-loudness"].as_object().unwrap();
    assert_eq!(fields.keys().collect::<Vec<_>>(), declared, "{fields:?}");
    let text = env.ok(&["track", "USB", "--track", "1"]);
    for name in declared {
        assert!(text.contains(&format!("  sound.{name} ")), "{name}: {text}");
    }
}

/// The coverart plugin locallibrary ships says which servers it talks to and that it finds cover
/// art for `art fetch` and track details for `track fetch`, planning none itself; tracks the drive
/// doesn't have are refused before anything goes online.
#[test]
fn the_coverart_plugin_finds_nothing_without_going_online() {
    let env = Env::new();
    env.demo("USB");
    let info = env.ok(&["plugin", "info", "coverart"]);
    for line in [
        "Cover art (coverart) 0.1.0",
        "talk to musicbrainz.org",
        "covers  Cover Art Archive: cover art, for locallibrary art fetch",
        "tags    MusicBrainz: track details, for locallibrary track fetch",
    ] {
        assert!(info.contains(line), "{line}: {info}");
    }
    assert!(!info.contains("plan cover art changes"), "{info}");
    for args in [["art", "fetch", "USB", "--from", "coverart"], ["track", "fetch", "USB", "--from", "coverart.tags"]] {
        let missing = env.fails(1, &[&args[..], &["--track", "99", "--dry-run"]].concat());
        assert!(missing.contains("error: not found: track #99"), "{missing}");
    }
    assert!(
        env.fails(2, &["analyze", "USB", "--plugin", "coverart"])
            .contains("error: the coverart plugin has no analyzer\n")
    );
}

/// A track whose artwork is gone has `check` and `doctor` name `art fetch` beside `art auto`: the
/// coverart plugin finds covers online.
#[test]
fn check_and_doctor_name_the_coverart_plugins_art_fetch() {
    let env = Env::new();
    let usb = env.demo("USB");
    std::fs::write(env.outside("cover.jpg"), crate::support::jpeg(64)).unwrap();
    env.ok(&["--yes", "art", "set", "USB", path(&env.outside("cover.jpg")), "--track", "1"]);
    std::fs::remove_dir_all(usb.join("PIONEER/Artwork")).unwrap();

    let r = env.run(&["--json", "check", "USB"]);
    let v: Value = serde_json::from_str(&r.out).unwrap_or_else(|e| panic!("{e}: {}\n{}", r.out, r.err));
    let files = v["items"].as_array().unwrap().iter().find(|i| i["check"] == "files" && i["level"] == "warning");
    let message = files.and_then(|i| i["message"].as_str()).unwrap_or_else(|| panic!("{v}"));
    let said = "; `locallibrary art auto \"USB\"` fills in art their files embed, and `locallibrary art fetch \"USB\"` finds covers online";
    assert!(message.starts_with("1 track(s) have no artwork image: ") && message.ends_with(said), "{message}");
    let doctor = env.run(&["doctor", "USB", "--only", "missing"]);
    for hint in
        ["`locallibrary art auto \"USB\"` fills in artwork", "`locallibrary art fetch \"USB\"` finds covers online"]
    {
        assert!(doctor.out.contains(hint) || doctor.err.contains(hint), "{hint}: {}{}", doctor.out, doctor.err);
    }
}

/// A plugin whose id is a namespace of the sidecar's methods is left out, saying why, and the
/// sidecar lists it with why; every other plugin works.
#[test]
fn a_plugin_that_clashes_is_left_out() {
    let mut env = Env::new();
    env.plugins(&[&echo(), &echo().replace("id = \"echo\"", "id = \"events\"")]);
    let why = "the events plugin's id is the sidecar's events.* methods' too: its plans would be named like them";
    let r = env.run(&["status"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.err.contains(&format!("the events plugin is left out: {why}")), "{}", r.err);
    assert_eq!(env.ok(&["say", "--name", "x"]), "{\n  \"name\": \"x\"\n}\n");
    assert!(env.fails(2, &["events", "echo"]).contains("unrecognized subcommand 'events'"));
    let listed = env.json(&["plugin", "list"]);
    let ids: Vec<&Value> = listed.as_array().unwrap().iter().map(|p| &p["id"]).collect();
    assert_eq!(ids, [&SHIPPED[..], &["echo"]].concat());
    let listed = Rpc::start(env.command()).call(1, "plugins.list", Value::Null)["result"].clone();
    let left_out: Vec<(Value, Value)> =
        listed.as_array().unwrap().iter().map(|p| (p["id"].clone(), p["left_out"].clone())).collect();
    let mut kept: Vec<(Value, Value)> = SHIPPED.iter().map(|id| (json!(id), Value::Null)).collect();
    kept.push((json!("echo"), Value::Null));
    assert_eq!(left_out, [kept, vec![(json!("events"), json!(why))]].concat());
}

/// The sidecar lists and describes plugins, keeps their settings, runs their commands, plans and
/// applies their operations and exports (in a plan cache of their own), runs their analyzers and
/// reads what they worked out; a plan made from a file is stale once the file changes; and its
/// plugins stop with it.
#[test]
fn the_sidecar_hosts_plugins() {
    let env = env();
    let drive = path(&env.volumes.join("USB")).to_string();
    let mut s = Rpc::start(env.command());
    let result = |r: Value| r.get("result").cloned().unwrap_or_else(|| panic!("{r}"));
    let listed = result(s.call(1, "plugins.list", Value::Null));
    assert_eq!(listed[SHIPPED.len()]["id"], "echo");
    let described = result(s.call(2, "plugins.describe", json!({ "plugin": "echo" })));
    assert_eq!(described["manifest"]["contributes"]["commands"][0]["cli"], "say");
    let set = result(s.call(3, "plugins.settings.set", json!({ "plugin": "echo", "key": "greeting", "value": "hi" })));
    assert_eq!(set, json!({ "plugin": "echo", "settings": { "greeting": "hi" }, "secrets": {} }));
    let context = result(s.call(
        4,
        "plugins.run",
        json!({ "plugin": "echo", "command": "echo", "params": { "context": true }, "context": { "drive": "USB" } }),
    ));
    assert_eq!(context["settings"], json!({ "greeting": "hi" }));
    let reset =
        result(s.call(5, "plugins.settings.set", json!({ "plugin": "echo", "key": "greeting", "value": null })));
    assert_eq!(reset["settings"]["greeting"], "hello");
    assert_eq!(result(s.call(6, "plugins.settings", json!({ "plugin": "echo" })))["settings"]["greeting"], "hello");
    let view = result(s.call(
        7,
        "plugins.run",
        json!({ "plugin": "echo", "command": "titles", "context": { "drive": drive } }),
    ));
    assert_eq!(view["kind"], "table");

    // A core plan outlives as many plugin plans as the cache holds, and neither kind is taken
    // for the other.
    let other = path(&env.demo("B")).to_string();
    let core = result(s.call(
        8,
        "edit.plan",
        json!({ "drive": other, "edits": [{ "op": "create_playlist", "path": "Core" }] }),
    ));
    let op = |name: &str| json!({ "plugin": "echo", "operation": "playlist", "params": { "name": name }, "context": { "drive": drive } });
    let mut plugin_plans = Vec::new();
    for i in 0..70 {
        plugin_plans.push(result(s.call(100 + i, "plugins.op.plan", op(&format!("P{i}"))))["plan_id"].clone());
    }
    let wrong = s.call(9, "edit.apply", json!({ "plan_id": plugin_plans[69] }));
    assert_eq!(wrong["error"]["data"]["kind"], "invalid_params", "{wrong}");
    assert!(wrong["error"]["message"].as_str().unwrap().ends_with("is a echo.playlist plan, not edit"), "{wrong}");
    let wrong = s.call(10, "plugins.op.apply", json!({ "plan_id": core["plan_id"] }));
    assert_eq!(wrong["error"]["data"]["kind"], "invalid_params", "{wrong}");
    assert!(wrong["error"]["message"].as_str().unwrap().ends_with("is a edit plan, not plugins.op"), "{wrong}");
    result(s.call(11, "edit.apply", json!({ "plan_id": core["plan_id"] })));
    let first = s.call(12, "plugins.op.apply", json!({ "plan_id": plugin_plans[0] }));
    assert_eq!(first["error"]["data"]["kind"], "stale_plan", "the oldest plugin plans made room: {first}");
    let applied = result(s.call(13, "plugins.op.apply", json!({ "plan_id": plugin_plans[69] })));
    assert_eq!(applied["changes"], 1);
    let export =
        json!({ "plugin": "echo", "exporter": "echo-m3u", "drive": drive, "output": path(&env.outside("x.m3u")) });
    let exporting = result(s.call(15, "plugins.export.plan", export.clone()));
    let wrong = s.call(14, "plugins.op.apply", json!({ "plan_id": exporting["plan_id"] }));
    assert_eq!(wrong["error"]["data"]["kind"], "invalid_params", "{wrong}");
    let planned = result(s.call(16, "plugins.export.plan", export));
    assert_eq!(planned["operation"], "echo.export.echo-m3u");
    let file = &planned["files"][0];
    let x = path(&env.outside("x.m3u")).to_string();
    assert_eq!((&file["path"], &file["what"], &file["exists"]), (&json!(x), &json!("playlist"), &json!(false)));
    assert_eq!((&planned["counts"], &planned["notes"]), (&json!({ "tracks": 2 }), &json!([])), "{planned}");
    let written = result(s.call(17, "plugins.export.apply", json!({ "plan_id": planned["plan_id"] })));
    let bytes = std::fs::metadata(&x).unwrap().len();
    assert_eq!(written["files"], json!([{ "path": x, "what": "playlist", "bytes": bytes }]));
    assert_eq!(file["bytes"], bytes, "a dry run says its size");

    // A plan worked out from a file is applied only while the file is as it was.
    let list = env.outside("list.txt");
    std::fs::write(&list, "a").unwrap();
    let from_file = |name: &str| {
        let edits = json!([{ "op": "create_playlist", "path": name }]);
        json!({ "plugin": "echo", "operation": "playlist", "params": { "name": name, "edits": edits, "inputs": [path(&list)] }, "context": { "drive": drive } })
    };
    let planned = result(s.call(18, "plugins.op.plan", from_file("Listed")));
    std::fs::write(&list, "b").unwrap();
    let stale = s.call(19, "plugins.op.apply", json!({ "plan_id": planned["plan_id"] }));
    assert_eq!(stale["error"]["data"]["kind"], "stale_plan", "{stale}");
    result(s.call(20, "plugins.op.apply", from_file("Listed")));

    let analyzed =
        result(s.call(21, "plugins.analyze", json!({ "plugin": "echo", "analyzer": "size", "drive": drive })));
    assert_eq!(analyzed, json!({ "analyzed": 2, "kept": 0, "failed": 0 }));
    let fields = result(s.call(22, "track.fields", json!({ "drive": drive, "tracks": [1] })));
    assert!(fields["tracks"][0]["plugins"]["echo"]["size"]["bytes"].as_u64().unwrap() > 0, "{fields}");
    assert_eq!(result(s.call(23, "plugins.list", Value::Null))[SHIPPED.len()]["running"], true);

    // Slow to stop: the sidecar waits for it (within its grace) before it exits.
    let stopped = env.outside("stopped");
    let params = json!({ "linger": 1000, "stopped": path(&stopped) });
    result(s.call(24, "plugins.run", json!({ "plugin": "echo", "command": "echo", "params": params })));
    drop(s);
    assert!(stopped.exists(), "the sidecar exited before its plugin stopped");
}

/// Told to stop (SIGTERM, as a GUI closing it) as its input closes, the sidecar still exits only
/// once its plugins have stopped: the signal's thread and the main one both stop them, and
/// neither exits while the other's stop is under way.
#[cfg(unix)]
#[test]
fn a_signalled_sidecar_waits_for_its_plugins_to_stop() {
    let env = env();
    let mut s = Rpc::start(env.command());
    let stopped = env.outside("stopped");
    let params = json!({ "linger": 1000, "stopped": path(&stopped) });
    let ran = s.call(1, "plugins.run", json!({ "plugin": "echo", "command": "echo", "params": params }));
    assert!(ran.get("result").is_some(), "{ran}");
    // SAFETY: kill only sends the sidecar a signal.
    assert_eq!(unsafe { libc::kill(s.pid() as i32, libc::SIGTERM) }, 0);
    drop(s);
    assert!(stopped.exists(), "the sidecar exited before its plugin stopped");
}

fn strs(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}
