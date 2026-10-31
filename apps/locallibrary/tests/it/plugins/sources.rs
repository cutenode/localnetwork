//! Library sources on the command line (`collection export --from PLUGIN:SOURCE --grant FOLDER`)
//! and in the sidecar (`plugins.source.plan` / `.apply`), with the test plugin reading the files
//! in the folders it's granted.

use super::*;

/// The echo manifest granted `fs:read`, with the library source `folder` (its `per_page` option).
fn sourcing() -> String {
    let granted = echo_plugin().replace(r#""library:plan"]"#, r#""library:plan", "fs:read"]"#);
    let options = "{ type = \"object\", properties = { per_page = { type = \"integer\" } } }";
    format!("{granted}\n[[contributes.sources]]\nname = \"folder\"\ntitle = \"Audio files\"\noptions = {options}\n")
}

/// An [`Env`] with [`sourcing`]'s plugin and a folder of music: two WAV files (one titled, in a
/// playlist) and a text file.
fn music() -> (Env, std::path::PathBuf) {
    let mut env = Env::new();
    env.plugins(&[&sourcing()]);
    let music = env.outside("music");
    std::fs::create_dir_all(&music).unwrap();
    for name in ["a.wav", "b.wav"] {
        std::fs::write(music.join(name), recording(1, 1.0, 44_100, 16, 0)).unwrap();
    }
    std::fs::write(music.join("a.wav.json"), json!({ "title": "A", "artist": "Art" }).to_string()).unwrap();
    std::fs::write(music.join("notes.txt"), "not audio").unwrap();
    std::fs::write(music.join("playlists.json"), json!({ "Sets/Friday": ["a.wav"] }).to_string()).unwrap();
    (env, music)
}

/// What a plan says the source read in `music`: the folder with its links resolved, as the host
/// gives it to the plugin (without Windows' `\\?\` prefix).
fn read_from(music: &std::path::Path) -> String {
    let real = locallibrary::device::without_verbatim_prefix(music.canonicalize().unwrap());
    format!("read the files Audio files finds in {}", real.display())
}

#[test]
fn a_drive_is_built_from_a_plugins_library_source() {
    let (env, music) = music();
    let stick = env.outside("stick");
    let build = |more: &[&str]| {
        let mut args = vec!["collection", "export", path(&stick), "--from", "echo:folder", "--grant", path(&music)];
        args.extend(more);
        env.run(&args)
    };
    let dry = build(&["--dry-run", "--option", "per_page=1"]);
    assert_eq!(dry.code, 0, "{}{}", dry.out, dry.err);
    let said = format!("{}{}", dry.out, dry.err);
    assert!(said.contains("the library source echo:folder -> "), "{said}");
    assert!(said.contains("2 track(s), 1 playlist(s)"), "{said}");
    assert!(said.contains("1 track(s) left out, e.g. ") && said.contains("notes.txt"), "{said}");
    assert!(said.contains(&read_from(&music)), "{said}");
    assert!(!stick.join("PIONEER").exists(), "a dry run writes nothing");

    let done = build(&["--yes"]);
    assert_eq!(done.code, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("exported 2 track(s)"), "{}", done.out);
    let tracks = env.json(&["tracks", path(&stick)]);
    let mut titles: Vec<&str> =
        tracks["tracks"].as_array().unwrap().iter().map(|t| t["title"].as_str().unwrap()).collect();
    titles.sort();
    assert_eq!(titles, ["A", "b"]);

    let refused = |args: &[&str]| {
        let r = env.run(args);
        assert_ne!(r.code, 0, "{args:?}");
        r.err
    };
    let to = path(&stick);
    let err = refused(&["collection", "export", to, "--from", "echo", "--grant", path(&music)]);
    assert!(err.contains("--from takes PLUGIN:SOURCE, not \"echo\""), "{err}");
    let err = refused(&["collection", "export", to, "--from", "echo:nope", "--grant", path(&music)]);
    assert!(err.contains("the echo plugin has no library source \"nope\""), "{err}");
    let err = refused(&["collection", "--db", "x.db", "export", to, "--from", "echo:folder", "--grant", path(&music)]);
    assert!(err.contains("--db and --key are for rekordbox's collection"), "{err}");
    let err =
        refused(&["collection", "export", to, "--from", "echo:folder", "--grant", path(&music), "--option", "x=1"]);
    assert!(err.contains("this source has no option \"x\": its options are per_page"), "{err}");
    let err = refused(&["collection", "export", to, "--grant", path(&music)]);
    assert!(err.contains("--from"), "--grant needs --from: {err}");
    let info = env.ok(&["plugin", "info", "echo"]);
    assert!(info.contains("echo:folder") && info.contains("Library sources ("), "{info}");
}

#[test]
fn the_sidecar_builds_a_drive_from_a_library_source() {
    let (env, music) = music();
    let stick = env.outside("stick");
    let mut s = Rpc::start(env.command());
    let req = json!({ "plugin": "echo", "source": "folder", "grants": [music], "drive": path(&stick) });
    let planned = s.call(1, "plugins.source.plan", req);
    let planned = planned.get("result").cloned().unwrap_or_else(|| panic!("{planned}"));
    assert_eq!((planned["operation"].as_str(), planned["tracks"].as_u64()), (Some("echo.source.folder"), Some(2)));
    assert_eq!(planned["source"], "echo:folder");
    let wrong = s.call(2, "plugins.op.apply", json!({ "plan_id": planned["plan_id"] }));
    assert_eq!(wrong["error"]["data"]["kind"], "invalid_params", "{wrong}");
    let applied = s.call(3, "plugins.source.apply", json!({ "plan_id": planned["plan_id"] }));
    assert_eq!(applied["result"]["tracks"], 2, "{applied}");
    let listed = s.call(4, "plugins.list", json!({}));
    let echo = listed["result"].as_array().unwrap().iter().find(|p| p["id"] == "echo").cloned().unwrap();
    assert_eq!(echo["sources"], json!(["folder"]));
    assert!(echo["capabilities"].as_array().unwrap().iter().any(|c| c["capability"] == "fs:read"), "{echo}");
}

/// The titles of the tracks on the drive at `root`, sorted (none when there's no library).
fn titles(env: &Env, root: &std::path::Path) -> Vec<String> {
    let r = env.run(&["--json", "tracks", path(root)]);
    let tracks: Value = serde_json::from_str(&r.out).unwrap_or_default();
    let mut titles: Vec<String> = tracks["tracks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["title"].as_str())
        .map(String::from)
        .collect();
    titles.sort();
    titles
}

/// For any `collection export --from` drawn (with at most one of these wrong: the source's name,
/// `--db` beside it, an option it doesn't have, no folder granted or one that isn't there; to a
/// new folder or a drive holding a library, replaced or not; a dry run, `--yes` or neither; in
/// text or `--json`): what's wrong, or a library not replaced, is refused, saying why, leaving
/// the drive as it was.
/// Otherwise a dry run or a run not confirmed writes nothing (the plan says what the source read),
/// and `--yes` builds the drive from the audio files granted.
#[hegel::test(test_cases = 16, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn collection_export_from_a_source_is_checked_then_written(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (env, music) = music();
    // At most one thing wrong, so most runs get as far as the plan.
    let wrong =
        tc.draw(gs::sampled_from(vec!["", "", "", "", "db", "echo", "echo:nope", "x=1", "no grant", "nowhere"]));
    let from = if wrong.starts_with("echo") { wrong } else { "echo:folder" };
    let db = wrong == "db";
    let option = if wrong == "x=1" { Some("x=1") } else { tc.draw(gs::sampled_from(vec![None, Some("per_page=1")])) };
    let grant = match wrong {
        "no grant" => None,
        "nowhere" => Some("nowhere"),
        _ => Some("music"),
    };
    let on_demo = tc.draw(gs::booleans());
    let replace = tc.draw(gs::booleans());
    let mode = tc.draw(gs::sampled_from(vec!["--dry-run", "--yes", ""]));
    let as_json = tc.draw(gs::integers::<u8>().max_value(2)) == 0;
    let stick = if on_demo { env.demo("DEMO") } else { env.outside("stick") };
    let before = titles(&env, &stick);
    let mut args: Vec<String> = Vec::new();
    args.extend(as_json.then(|| "--json".to_string()));
    args.push("collection".into());
    if db {
        args.extend(["--db".into(), "x.db".into()]);
    }
    args.extend(["export".into(), path(&stick).into(), "--from".into(), from.into()]);
    match grant {
        Some("music") => args.extend(["--grant".into(), path(&music).into()]),
        // Relative, as typed: made absolute against where it's run, where there's no such folder.
        Some(other) => args.extend(["--grant".into(), other.into()]),
        None => {}
    }
    if let Some(o) = option {
        args.extend(["--option".into(), o.into()]);
    }
    args.extend(replace.then(|| "--replace".to_string()));
    args.extend((!mode.is_empty()).then(|| mode.to_string()));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let r = env.run(&args);
    let refused = match () {
        _ if db => Some("--db and --key are for rekordbox's collection"),
        _ if from == "echo" => Some("--from takes PLUGIN:SOURCE"),
        _ if from == "echo:nope" => Some("the echo plugin has no library source"),
        _ if option == Some("x=1") => Some("this source has no option"),
        _ if grant.is_none() => Some("grant the plugin a folder to read its library's files in"),
        _ if grant == Some("nowhere") => Some("nowhere"),
        _ if on_demo && !replace => Some("wasn't built from echo:folder; replace it"),
        _ => None,
    };
    if let Some(said) = refused {
        // In `--json` mode the error is printed as JSON, on stdout.
        let printed = if as_json { &r.out } else { &r.err };
        assert!(r.code != 0 && printed.contains(said), "{args:?} {said:?}: {}{}", r.out, r.err);
        assert_eq!(titles(&env, &stick), before, "nothing written");
        return;
    }
    let wanted = match mode {
        "--yes" => (0, vec!["A".to_string(), "b".to_string()]),
        "--dry-run" => (0, before),
        _ => (3, before),
    };
    assert_eq!((r.code, titles(&env, &stick)), wanted, "{args:?}: {}{}", r.out, r.err);
    if as_json {
        let out: Value = serde_json::from_str(&r.out).unwrap_or_else(|e| panic!("{e}: {}", r.out));
        assert_eq!(
            (out["plan"]["tracks"].as_u64(), out["applied"].as_bool()),
            (Some(2), Some(mode == "--yes")),
            "{out}"
        );
        return;
    }
    let said = format!("{}{}", r.out, r.err);
    assert!(said.contains("the library source echo:folder -> "), "{said}");
    assert!(said.contains(&read_from(&music)), "{said}");
}

/// For any `plugins.source.plan` drawn in the sidecar (at most one thing wrong: the plugin, the
/// source, no folder granted or one that isn't there; any page size; to a new folder
/// or a drive holding a library, replaced or not): what's wrong, or a library not replaced, is an
/// error and the drive stays as it was. Otherwise the plan says what the source read, and
/// `plugins.source.apply` (when it's applied) builds the drive from the audio files granted.
#[hegel::test(test_cases = 12, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_sidecar_plans_and_builds_any_source_drawn(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (env, music) = music();
    let wrong = tc.draw(gs::sampled_from(vec!["", "", "", "plugin", "source", "no grant", "nowhere"]));
    let on_demo = tc.draw(gs::booleans());
    let replace = tc.draw(gs::booleans());
    let apply = tc.draw(gs::booleans());
    let stick = if on_demo { env.demo("DEMO") } else { env.outside("stick") };
    let before = titles(&env, &stick);
    let grants = match wrong {
        "no grant" => json!([]),
        "nowhere" => json!([env.outside("nowhere")]),
        _ => json!([music]),
    };
    let options = tc.draw(gs::sampled_from(vec![json!({}), json!({ "per_page": 1 })]));
    let req = json!({
        "plugin": if wrong == "plugin" { "nope" } else { "echo" },
        "source": if wrong == "source" { "nope" } else { "folder" },
        "options": options,
        "grants": grants,
        "drive": path(&stick),
        "replace": replace,
    });
    let mut s = Rpc::start(env.command());
    let planned = s.call(1, "plugins.source.plan", req.clone());
    let refused = match wrong {
        "plugin" => Some("nope"),
        "source" => Some("the echo plugin's library source \"nope\""),
        "no grant" => Some("grant the plugin a folder"),
        "nowhere" => Some("nowhere"),
        _ if on_demo && !replace => Some("wasn't built from echo:folder; replace it"),
        _ => None,
    };
    if let Some(said) = refused {
        let message = planned["error"]["message"].as_str().unwrap_or_else(|| panic!("{req}: {planned}"));
        assert!(message.contains(said), "{req}: {said:?} isn't in {message}");
        assert_eq!(titles(&env, &stick), before, "nothing written");
        return;
    }
    let plan = planned.get("result").unwrap_or_else(|| panic!("{req}: {planned}"));
    assert_eq!((plan["operation"].as_str(), plan["tracks"].as_u64()), (Some("echo.source.folder"), Some(2)), "{plan}");
    assert_eq!(plan["source"], "echo:folder");
    if !apply {
        assert_eq!(titles(&env, &stick), before, "a plan writes nothing");
        return;
    }
    let applied = s.call(2, "plugins.source.apply", json!({ "plan_id": plan["plan_id"] }));
    assert_eq!(applied["result"]["tracks"], 2, "{applied}");
    assert_eq!(titles(&env, &stick), ["A", "b"]);
}
