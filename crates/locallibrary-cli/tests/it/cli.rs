use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::support::{PDB, club_export, copy_dir, demo_drive, fixtures, path};

struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Env {
    fn new() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        Env { _dir: dir, root }
    }

    /// Its drives are the folders in `root`, and only those: never the machine's.
    fn locallibrary(&self) -> Command {
        crate::locallibrary(&self.root.join("store"), &[&self.root])
    }

    fn json(&self, args: &[&str]) -> Value {
        let out = self.locallibrary().arg("--json").args(args).output().unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{args:?}: {e}\n{text}\n{}", String::from_utf8_lossy(&out.stderr)))
    }
}

#[test]
fn json_sync_requires_yes_and_then_applies() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    let b = env.root.join("B");
    std::fs::create_dir(&b).unwrap();

    // Without --yes, JSON mode reports the plan and writes nothing.
    let v = env.json(&["sync", path(&a), path(&b)]);
    assert_eq!(v["applied"], false);
    assert_eq!(v["plan"]["comparison"]["relation"], "left_newer");
    assert!(v["plan"]["plan"]["copies"].as_array().unwrap().len() >= 4);
    assert!(!b.join("PIONEER").exists());

    let v = env.json(&["--yes", "sync", path(&a), path(&b)]);
    assert_eq!(v["applied"], true);
    assert!(b.join("PIONEER/rekordbox/export.pdb").is_file());

    let v = env.json(&["compare", path(&a), path(&b)]);
    assert_eq!(v["relation"], "identical");
    let v = env.json(&["--yes", "sync", path(&a), path(&b)]);
    assert_eq!(v["applied"], false); // already in sync
}

#[test]
fn exit_statuses_say_what_happened() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    let b = env.root.join("B");
    std::fs::create_dir(&b).unwrap();
    let run = |args: &[&str]| {
        let out = env.locallibrary().arg("--json").args(args).output().unwrap();
        let v: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
        (out.status.code(), v)
    };
    // A plan nobody confirmed: 3. A dry run: 0.
    let (code, v) = run(&["sync", path(&a), path(&b)]);
    assert_eq!((code, &v["outcome"]), (Some(3), &Value::from("not_confirmed")), "{v}");
    let (code, v) = run(&["sync", path(&a), path(&b), "--dry-run"]);
    assert_eq!((code, &v["outcome"]), (Some(0), &Value::from("dry_run")), "{v}");
    let (code, v) = run(&["--yes", "sync", path(&a), path(&b)]);
    assert_eq!((code, &v["outcome"]), (Some(0), &Value::from("applied")), "{v}");
    // Problems found: 4.
    let orphan = a.join("Contents/Old/forgotten.mp3");
    std::fs::create_dir_all(orphan.parent().unwrap()).unwrap();
    std::fs::write(&orphan, "old").unwrap();
    let (code, v) = run(&["doctor", path(&a)]);
    assert_eq!((code, &v["outcome"]), (Some(4), &Value::from("problems")), "{v}");
    // An error: 1; usage: 2.
    let (code, v) = run(&["info", "/definitely/not/here"]);
    assert_eq!((code, &v["outcome"]), (Some(1), &Value::from("error")), "{v}");
    let (code, _) = run(&["sync", "--no-such-flag"]);
    assert_eq!(code, Some(2));
}

#[test]
fn commands_come_with_the_features_built_in() {
    let env = Env::new();
    let help = |args: &[&str]| {
        let out = env.locallibrary().args(args).output().unwrap();
        (out.status.code(), String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let (_, top) = help(&["help"]);
    let (_, settings) = help(&["settings", "--help"]);
    let (_, profile) = help(&["profile", "--help"]);
    let account = cfg!(feature = "account");
    assert_eq!(top.contains("\n  account "), account, "{top}");
    assert_eq!(settings.contains("\n  publish "), account, "{settings}");
    assert_eq!(profile.contains("\n  publish "), account, "{profile}");
    assert_eq!(top.contains("\n  tui "), cfg!(feature = "tui"), "{top}");
    // Without the account the command isn't there at all: a usage error.
    let (code, _) = help(&["account", "status"]);
    assert_eq!(code == Some(2), !account);
}

#[test]
fn errors_are_reported_with_codes() {
    let env = Env::new();
    let out = env.locallibrary().args(["--json", "info", "/definitely/not/here"]).output().unwrap();
    assert!(!out.status.success());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["code"], "not_found");

    let a = demo_drive(&env.root, "A");
    let other = env.root.join("unrelated");
    copy_dir(&fixtures().join("rekordcrate/demo_tracks"), &other);
    std::fs::copy(club_export(), other.join(PDB)).unwrap();
    let out = env.locallibrary().args(["--json", "--yes", "sync", path(&a), path(&other)]).output().unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["code"], "conflict", "different libraries must not be synced automatically");
}

#[test]
fn store_settings_and_names() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    // Every write asks first: JSON mode without --yes only shows the plan.
    let v = env.json(&["adopt", path(&a), "--name", "Main"]);
    assert_eq!((&v["applied"], &v["outcome"]), (&Value::Bool(false), &Value::from("not_confirmed")), "{v}");
    assert!(!a.join(".locallibrary/state.json").exists());
    let v = env.json(&["--yes", "adopt", path(&a), "--name", "Main"]);
    assert_eq!(v["report"]["name"], "Main", "{v}");
    // Adopted drives resolve by name.
    let v = env.json(&["pull", "Main", "-m", "first"]);
    assert_eq!(v["report"]["up_to_date"], false);
    let v = env.json(&["log"]);
    assert_eq!(v["libraries"]["Main"].as_array().unwrap().len(), 1);
    let v = env.json(&["status"]);
    let main = v["mounted"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["drive"]["name"] == "Main")
        .expect("registered drive listed");
    assert_eq!(main["relation"], "identical");
    assert_eq!(main["library"], "Main");

    env.json(&["--yes", "settings", "set", "quantize=off", "jog_mode=cdj"]);
    let v = env.json(&["settings", "show", "Main"]);
    assert_eq!(v["drift"].as_array().unwrap().len(), 2);
    let v = env.json(&["settings", "apply", "Main"]);
    assert_eq!(v["applied"], false, "{v}");
    assert_eq!(v["plan"]["targets"][0]["changes"]["changes"].as_array().unwrap().len(), 2, "{v}");
    let v = env.json(&["settings", "show", "Main"]);
    assert_eq!(v["drift"].as_array().unwrap().len(), 2);
    env.json(&["--yes", "settings", "apply", "Main"]);
    let v = env.json(&["settings", "show", "Main"]);
    assert!(v["drift"].as_array().unwrap().is_empty());
    assert_eq!(v["current"]["values"]["quantize"], "off");

    let v = env.json(&["xml", "Main", "-o", path(&env.root.join("out.xml"))]);
    assert_eq!(v["report"]["tracks"], 2);
    assert!(std::fs::read_to_string(env.root.join("out.xml")).unwrap().contains("DJ_PLAYLISTS"));
}

#[test]
fn rpc_round_trip() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    let mut child = env.locallibrary().arg("rpc").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        for (id, method, params) in [
            (1, "version", serde_json::json!({})),
            (2, "drive.library", serde_json::json!({ "drive": path(&a) })),
            (3, "settings.schema", serde_json::json!({})),
            (4, "no.such.method", serde_json::json!({})),
        ] {
            writeln!(
                stdin,
                "{}",
                serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
            )
            .unwrap();
        }
    }
    drop(child.stdin.take());
    let out = child.wait_with_output().unwrap();
    let mut by_id = std::collections::HashMap::new();
    for line in String::from_utf8(out.stdout).unwrap().lines() {
        let v: Value = serde_json::from_str(line).unwrap();
        if let Some(id) = v["id"].as_i64() {
            by_id.insert(id, v);
        }
    }
    assert!(by_id[&1]["result"]["methods"].as_array().unwrap().len() > 10);
    assert_eq!(by_id[&2]["result"]["summary"]["tracks"], 2);
    assert!(by_id[&3]["result"].as_array().unwrap().len() > 40);
    assert_eq!(by_id[&4]["error"]["code"], -32601);
}

/// A player recorded a history list on the drive.
fn add_history(root: &Path, name: &str) {
    let pdb = root.join("PIONEER/rekordbox/export.pdb");
    let first = rekordbox_pdb::Database::from_file(&pdb).unwrap().tracks[0].id;
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb).unwrap();
    let list = ed.create_history_playlist(name).unwrap();
    ed.add_to_history(list, first).unwrap();
    ed.save(&pdb).unwrap();
}

#[test]
fn watch_pulls_drives_with_new_history_and_leaves_the_rest_alone() {
    let env = Env::new();
    let volumes = env.root.join("volumes");
    let (main, old) = (demo_drive(&volumes, "Main"), demo_drive(&volumes, "Old"));
    env.json(&["pull", path(&main), "--library", "Main", "--new-library"]);
    env.json(&["pull", path(&old), "--library", "Main"]);
    env.json(&["--yes", "drive", "retire", path(&old)]);
    for root in [&main, &old] {
        add_history(root, "HISTORY 001");
    }
    demo_drive(&volumes, "Friend"); // never pulled: not one of the store's libraries

    let watch = |args: &[&str]| -> Vec<Value> {
        let out = env.locallibrary().env("LOCALLIBRARY_VOLUMES", &volumes).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    };
    let said = |events: &[Value]| -> Vec<(String, String)> {
        events.iter().map(|e| (e["name"].as_str().unwrap().into(), e["event"].as_str().unwrap().into())).collect()
    };
    // Without --pull: only said.
    let events = watch(&["--json", "watch", "--once"]);
    assert!(events.iter().all(|e| e["schema"] == 1), "{events:?}");
    let pending = events.iter().find(|e| e["event"] == "watch.pending").expect("Main has news");
    assert_eq!(pending["history_lists"], serde_json::json!(["HISTORY 001"]));

    let events = watch(&["--json", "watch", "--once", "--pull"]);
    let mut seen = said(&events);
    seen.sort();
    assert_eq!(
        seen,
        [("Friend", "watch.skipped"), ("Main", "watch.pulled"), ("Old", "watch.skipped")]
            .map(|(n, e)| (n.into(), e.into()))
    );
    let pulled = events.iter().find(|e| e["event"] == "watch.pulled").unwrap();
    assert_eq!((&pulled["library"], &pulled["history_lists"][0]), (&"Main".into(), &"HISTORY 001".into()));
    assert!(events.iter().any(|e| e["name"] == "Old" && e["reason"] == "retired"), "{events:?}");
    assert_eq!(env.json(&["library", "ls"]).as_array().unwrap().len(), 1, "no library started for Friend");

    // Human output, and nothing left to do.
    let out = env.locallibrary().env("LOCALLIBRARY_VOLUMES", &volumes).args(["watch", "--once", "--pull"]).output();
    let text = String::from_utf8(out.unwrap().stdout).unwrap();
    assert!(text.contains("Main: the store holds it (Main)"), "{text}");
    assert!(text.contains("Old: left alone: retired"), "{text}");
}

#[test]
fn sync_many_updates_every_older_drive() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    let b = env.root.join("B");
    std::fs::create_dir(&b).unwrap();
    let c = env.root.join("C");
    copy_dir(&a, &c);
    env.json(&["--yes", "sync", path(&a), path(&c)]); // C now shares A's lineage
    // A gets a newer export.
    let mut ed = rekordbox_pdb::PdbEditor::from_file(a.join("PIONEER/rekordbox/export.pdb")).unwrap();
    ed.add_track(&rekordbox_pdb::NewTrack::new("New", "/Contents/New/new.mp3")).unwrap();
    ed.save(a.join("PIONEER/rekordbox/export.pdb")).unwrap();
    std::fs::create_dir_all(a.join("Contents/New")).unwrap();
    std::fs::write(a.join("Contents/New/new.mp3"), "new audio").unwrap();

    // Order doesn't matter: the newest drive is found and copied to the rest.
    let v = env.json(&["--yes", "sync", path(&c), path(&b), path(&a)]);
    assert_eq!(v["applied"], true, "{v}");
    assert_eq!(v["report"].as_array().unwrap().len(), 2);
    for other in [&b, &c] {
        assert_eq!(env.json(&["compare", path(&a), path(other)])["relation"], "identical");
    }
}

#[test]
fn clean_shows_a_plan_then_removes_demo_samples() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    let file = "/Contents/rekordbox/GROOVE CIRCUIT FACTORY SAMPLES/House1.wav";
    let mut ed = rekordbox_pdb::PdbEditor::from_file(a.join("PIONEER/rekordbox/export.pdb")).unwrap();
    ed.add_track(&rekordbox_pdb::NewTrack {
        artist: Some("rekordbox".into()),
        ..rekordbox_pdb::NewTrack::new("House 1", file)
    })
    .unwrap();
    ed.save(a.join("PIONEER/rekordbox/export.pdb")).unwrap();
    let audio = a.join(file.trim_start_matches('/'));
    std::fs::create_dir_all(audio.parent().unwrap()).unwrap();
    std::fs::write(&audio, "demo sample").unwrap();

    let v = env.json(&["clean", path(&a)]);
    assert_eq!(v["applied"], false);
    assert_eq!(v["plan"]["tracks"][0]["name"], "rekordbox - House 1");
    assert!(audio.is_file());

    let v = env.json(&["--yes", "clean", path(&a)]);
    assert_eq!(v["applied"], true, "{v}");
    assert_eq!(v["report"]["tracks_removed"], 1);
    assert!(!audio.exists());
    assert_eq!(env.json(&["info", path(&a)])["summary"]["tracks"], 2);

    let v = env.json(&["clean", path(&a)]);
    assert_eq!(v["applied"], false);
    assert!(v["plan"]["tracks"].as_array().unwrap().is_empty());
}

#[test]
fn format_refuses_a_folder_and_never_applies_without_yes() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    // A folder is not a volume: refused before any tool runs.
    let v = env.json(&["format", path(&a)]);
    let code = v["error"]["code"].as_str().unwrap_or_default();
    assert!(matches!(code, "invalid" | "not_found"), "{v}");
    assert!(a.join("PIONEER/rekordbox/export.pdb").is_file());
}

#[test]
fn disks_lists_external_disks_as_json() {
    // The test switch hides the machine's disks with its drives.
    let env = Env::new();
    let v = env.json(&["disks"]);
    assert_eq!(v["error"]["kind"], "invalid", "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("LOCALLIBRARY_VOLUMES"), "{v}");
    // On CI, which has no one's drives, the platform's listing (diskutil, lsblk or PowerShell)
    // runs for real.
    if std::env::var_os("CI").is_some() {
        let out = env.locallibrary().env_remove("LOCALLIBRARY_VOLUMES").args(["--json", "disks"]).output().unwrap();
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert!(v.is_array(), "{v}");
    }
}

/// `LOCALLIBRARY_VOLUMES` is a test switch, and says so: drives are looked for only in the folders
/// it names, each volume once (a folder named twice, the boot volume's symlink to `/`), and never
/// where the store last saw them outside those folders.
#[test]
fn the_volumes_switch_shows_only_its_folders() {
    let env = Env::new();
    let vols = env.root.join("vols");
    demo_drive(&vols, "FAKE");
    let elsewhere = demo_drive(&env.root, "Elsewhere");
    env.json(&["--yes", "adopt", path(&elsewhere), "--name", "Probestick"]);
    #[cfg(unix)]
    std::os::unix::fs::symlink("/", vols.join("Boot")).unwrap();
    let twice = std::env::join_paths([&vols, &vols]).unwrap();
    let run = |args: &[&str]| {
        let out = env.locallibrary().env("LOCALLIBRARY_VOLUMES", &twice).args(args).output().unwrap();
        (serde_json::from_slice::<Value>(&out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
    };
    let (drives, err) = run(&["--json", "drives", "--all"]);
    let names: Vec<&str> = drives.as_array().unwrap().iter().map(|d| d["volume_name"].as_str().unwrap()).collect();
    assert_eq!(names, ["FAKE"], "{drives}");
    assert!(err.contains("LOCALLIBRARY_VOLUMES is set (a test switch)"), "{err}");
    assert_eq!(run(&["--json", "info", "FAKE"]).0["volume_name"], "FAKE");
    assert_eq!(run(&["--json", "info", "Probestick"]).0["error"]["kind"], "not_found");
}

/// Under sudo the default store is root's on most systems: a format asks for `--store` rather
/// than protect and read the wrong one, and `provision` never makes one root would own.
#[cfg(unix)]
#[test]
fn under_sudo_a_format_asks_which_store() {
    use std::os::unix::fs::MetadataExt;
    let env = Env::new();
    let uid = std::fs::metadata(&env.root).unwrap().uid();
    let sudo = |uid: u32, args: &[&str]| {
        let mut c = env.locallibrary();
        c.env_remove("LOCALLIBRARY_STORE").env("HOME", &env.root).env("SUDO_UID", uid.to_string());
        let out = c.arg("--json").args(args).output().unwrap();
        serde_json::from_slice::<Value>(&out.stdout).unwrap()
    };
    let v = sudo(uid + 1, &["drive", "format", "disk99", "--dry-run"]);
    assert!(v["error"]["message"].as_str().unwrap().contains("pass --store"), "{v}");
    let missing = env.root.join("no store");
    let v = sudo(uid, &["--store", path(&missing), "provision", "disk99", "--dry-run"]);
    assert!(v["error"]["message"].as_str().unwrap().contains("opening store"), "{v}");
    assert!(!missing.exists(), "no store made as root");
}

/// The commands moved into groups answer under their old names too, the same way.
#[test]
fn old_command_names_answer_like_the_new_ones() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    let b = demo_drive(&env.root, "B");
    env.json(&["--yes", "adopt", path(&a), "--name", "Main"]);
    env.json(&["pull", "Main", "-m", "first"]);
    // A plan's id and time differ from one run to the next, and free space as other tests write.
    fn steady(v: &mut Value) {
        match v {
            Value::Object(m) => {
                m.remove("plan_id");
                m.remove("planned_at");
                m.remove("free_space");
                m.values_mut().for_each(steady);
            }
            Value::Array(a) => a.iter_mut().for_each(steady),
            _ => {}
        }
    }
    let same = |old: &[&str], new: &[&str]| {
        let (mut o, mut n) = (env.json(old), env.json(new));
        steady(&mut o);
        steady(&mut n);
        assert_eq!(o, n, "{old:?} and {new:?} differ");
        n
    };
    same(&["info", "Main"], &["drive", "info", "Main"]);
    same(&["info", "Main"], &["drive", "Main"]);
    same(&["drive", "list"], &["drive", "ls", "--known"]);
    same(&["backups", "Main"], &["drive", "backups", "Main"]);
    same(&["log"], &["library", "log"]);
    let log = same(&["log", "Main", "-n", "1"], &["library", "Main"]);
    assert_eq!(log["libraries"]["Main"].as_array().unwrap().len(), 1, "{log}");
    same(&["history", "Main"], &["plays", "lists", "Main"]);
    same(&["history", "Main"], &["plays", "Main"]);
    same(&["stats", "Main"], &["plays", "stats", "Main"]);
    same(&["playlists", "Main"], &["playlist", "ls", "Main"]);
    same(&["playlists", "Main"], &["playlist", "Main"]);
    same(&["compare", path(&a), path(&b)], &["diff", path(&a), path(&b), "--summary"]);
    same(&["mirror", path(&a), path(&b), "--dry-run"], &["sync", "--from", path(&a), path(&b), "--dry-run"]);
    same(&["push", "Main", "--dry-run"], &["library", "push", "Main", "--dry-run"]);
    same(&["drive", "sets"], &["drive", "groups"]);
    same(&["settings", "profile"], &["settings"]);
    same(&["settings", "show", "Main"], &["settings", "Main"]);
    same(&["art", "missing", "Main"], &["art", "Main"]);
    same(&["edits", "list", "Main"], &["edits", "Main"]);
    same(&["smart", "list"], &["smart"]);
    // Subcommands renamed so they don't clash: the old spellings plan the same edits.
    env.json(&["--yes", "playlist", "new", "Main", "Friday", "--track", "1", "--track", "2"]);
    env.json(&["--yes", "playlist", "new", "Main", "Archive", "--folder"]);
    fn edit<'a>(args: &[&'a str]) -> Vec<&'a str> {
        [args, &["--dry-run"]].concat()
    }
    same(&edit(&["playlist", "rm", "Main", "Friday"]), &edit(&["playlist", "delete", "Main", "Friday"]));
    same(
        &edit(&["playlist", "mv", "Main", "Friday", "--into", "Archive"]),
        &edit(&["playlist", "move", "Main", "Friday", "--into", "Archive"]),
    );
    let moved = same(
        &edit(&["playlist", "move", "Main", "Friday", "--at", "2", "--to", "1"]),
        &edit(&["playlist", "reorder", "Main", "Friday", "--at", "2", "--to", "1"]),
    );
    assert_eq!(moved["plan"]["playlists_changed"][0]["reordered"], true, "{moved}");
    same(&edit(&["playlist", "plan", "Main", "Friday"]), &edit(&["playlist", "order", "Main", "Friday"]));
    same(
        &edit(&["playlist", "sort", "Main", "Friday", "--by", "-title"]),
        &edit(&["playlist", "sort", "Main", "Friday", "--sort", "-title"]),
    );
    env.json(&["--yes", "tags", "add", "Main", "Genre/Deep", "--track", "1"]);
    same(
        &edit(&["tags", "remove", "Main", "Genre/Deep", "--track", "1"]),
        &edit(&["tags", "untag", "Main", "Genre/Deep", "--track", "1"]),
    );
    same(&["settings", "profile"], &["settings", "base"]);

    // `gig` is the old name of the `set` role.
    assert_eq!(env.json(&["--yes", "drive", "role", "Main", "gig"])["report"]["role"], "set");
    assert_eq!(env.json(&["drive", "log", "Main"])["role"], "set");

    // Help and usage errors for the old names still work.
    let out = env.locallibrary().args(["verify", "--help"]).output().unwrap();
    assert!(out.status.success() && String::from_utf8_lossy(&out.stdout).contains("--repair"));
    let out = env.locallibrary().args(["sync", "--from", path(&a)]).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "{}", String::from_utf8_lossy(&out.stderr));
}

/// A group's mistyped subcommand is a usage error naming the one meant, whether the group takes
/// a drive (`drive lst`) or a library (`library lgo`) in its place.
#[test]
fn a_mistyped_subcommand_is_a_usage_error() {
    let env = Env::new();
    for (args, meant) in [(["drive", "lst"], "drive ls"), (["library", "lgo"], "library log")] {
        let out = env.locallibrary().args(args).output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {err}");
        assert!(err.contains(&format!("did you mean `locallibrary {meant}`?")), "{err}");
    }
}

/// `tracks` goes through the same query as RPC `tracks.query`; `--search` is free text in it.
#[test]
fn tracks_search_is_free_text_in_the_filter() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    let v = env.json(&["tracks", path(&a), "--search", "demo track - 2"]);
    assert_eq!(v["total"], 1, "{v}");
    assert_eq!(v["tracks"][0]["title"], "Demo Track 2", "{v}");
    let v = env.json(&["tracks", path(&a), "-s", "loopmasters", "--sort", "-title", "-n", "1"]);
    assert_eq!((&v["total"], v["tracks"].as_array().unwrap().len()), (&2.into(), 1), "{v}");
    assert_eq!(v["tracks"][0]["title"], "Demo Track 2", "{v}");
}

/// Output meant for reading, piped: TSV and CSV keep every value whole, report lines go to stdout,
/// `--ascii` swaps the symbols, and `--quiet` leaves out the tips.
#[test]
fn output_you_can_pipe() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    let text = |args: &[&str]| {
        let out = env.locallibrary().args(args).output().unwrap();
        (String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
    };
    let (tsv, _) = text(&["tracks", path(&a), "--format", "tsv", "--columns", "id,title,bpm"]);
    let lines: Vec<&str> = tsv.lines().collect();
    assert_eq!(lines[0], "id\ttitle\tbpm", "{tsv}");
    assert!(lines[1].starts_with("1\tDemo Track 1\t"), "{tsv}");
    assert_eq!(lines.len(), 3, "a header and the tracks, no count: {tsv}");
    let (csv, _) = text(&["tracks", path(&a), "--format", "csv", "--columns", "name"]);
    assert_eq!(csv.lines().next(), Some("name"), "{csv}");

    let (table, _) = text(&["tracks", path(&a)]);
    assert!(table.starts_with("id  name"), "a header row: {table}");

    let (report, err) = text(&["--ascii", "doctor", path(&a), "--only", "duplicates,drift"]);
    assert!(report.contains("v no duplicates") && !report.contains('✓'), "{report}");
    assert!(!err.contains("no duplicates"), "{err}");

    let xml = env.root.join("out.xml");
    let (loud, _) = text(&["xml", path(&a), "-o", path(&xml)]);
    assert!(loud.contains("Import in rekordbox"), "{loud}");
    let (quiet, _) = text(&["--quiet", "xml", path(&a), "-o", path(&xml), "--replace"]);
    assert!(quiet.contains("wrote") && !quiet.contains("Import in rekordbox"), "{quiet}");
}

/// Every command that writes prints one JSON envelope, `{schema, plan, report, applied, outcome}`;
/// checks print their report with `outcome`, errors `{schema, error: {kind, message}, outcome}`.
#[test]
fn json_shapes() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    let b = env.root.join("B");
    std::fs::create_dir(&b).unwrap();
    let keys = |v: &Value| {
        let mut k: Vec<String> =
            v.as_object().unwrap_or_else(|| panic!("not an object: {v}")).keys().cloned().collect();
        k.sort();
        k
    };
    let envelope = ["applied", "outcome", "plan", "report", "schema"];

    // Planned: not applied, then applied.
    let v = env.json(&["sync", path(&a), path(&b)]);
    assert_eq!(keys(&v), envelope, "{v}");
    assert_eq!((&v["schema"], &v["applied"], &v["report"]), (&1.into(), &false.into(), &Value::Null), "{v}");
    let v = env.json(&["--yes", "sync", path(&a), path(&b)]);
    assert_eq!(keys(&v), envelope, "{v}");
    assert!(v["report"]["report"]["copied"].as_u64().unwrap() > 0, "{v}");
    let c = env.root.join("C");
    std::fs::create_dir(&c).unwrap();
    let v = env.json(&["--yes", "sync", path(&a), path(&b), path(&c)]);
    assert_eq!(keys(&v), envelope, "{v}");
    assert!(v["report"].is_array() && v["plan"]["targets"].is_array(), "{v}");

    // Made in one step: a plan of one line, asked about like every write.
    env.json(&["--yes", "adopt", path(&a), "--name", "Main"]);
    env.json(&["pull", "Main"]);
    let v = env.json(&["drive", "role", "Main", "backup"]);
    assert_eq!(keys(&v), envelope, "{v}");
    assert_eq!((&v["report"], &v["outcome"]), (&Value::Null, &"not_confirmed".into()), "{v}");
    assert_eq!(env.json(&["drive", "log", "Main"])["role"], Value::Null, "not set without --yes");
    let v = env.json(&["--yes", "drive", "role", "Main", "backup"]);
    let id = env.json(&["drive", "log", "Main"])["id"].as_str().unwrap().to_string();
    assert_eq!(v["plan"]["summary"], serde_json::json!([format!("make Main ({id}) a backup drive")]), "{v}");
    assert_eq!((&v["report"]["role"], &v["outcome"]), (&"backup".into(), &"applied".into()));

    // Several drives, one document.
    env.json(&["--yes", "smart", "add", "Smart/All", "--filter", "bpm:>=0"]);
    let out = env.locallibrary().args(["--json", "--yes", "smart", "apply", path(&a), path(&b)]).output().unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).expect("one JSON document");
    assert_eq!(keys(&v), envelope, "{v}");
    assert_eq!((v["plan"].as_array().unwrap().len(), v["report"].as_array().unwrap().len()), (2, 2), "{v}");

    // A check: its report and the outcome.
    let v = env.json(&["drive", "verify", path(&a)]);
    assert!(v.get("plan").is_none() && v["outcome"].is_string() && v["schema"] == 1, "{v}");

    // Reads: an object always, with the schema.
    let v = env.json(&["status"]);
    assert_eq!(keys(&v), ["duplicates", "mounted", "offline", "schema"], "{v}");

    // Errors.
    let v = env.json(&["drive", "info", "/definitely/not/here"]);
    assert_eq!(keys(&v), ["error", "outcome", "schema"], "{v}");
    assert_eq!((&v["error"]["kind"], &v["outcome"]), (&"not_found".into(), &"error".into()), "{v}");
}

/// The shell asks locallibrary what to complete: mounted drives, their playlists, My Tags and
/// history lists; `completions` prints the script and `man` the pages.
#[test]
fn completions_and_man_pages() {
    let env = Env::new();
    let volumes = env.root.join("volumes");
    let a = demo_drive(&volumes, "A");
    let complete = |words: &[&str]| {
        let out = env
            .locallibrary()
            .env("COMPLETE", "fish")
            .env("LOCALLIBRARY_VOLUMES", &volumes)
            .arg("--")
            .args(words)
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap()
    };
    let drives = complete(&["locallibrary", "sync", ""]);
    assert!(drives.lines().any(|l| l.starts_with("A\t")), "{drives}");
    env.json(&["--yes", "playlist", "new", path(&a), "Sets/Friday", "--track", "1"]);
    let playlists = complete(&["locallibrary", "playlist", "add", "A", ""]);
    assert!(playlists.lines().any(|l| l.starts_with("Sets/Friday\t")), "{playlists}");
    let commands = complete(&["locallibrary", "dri"]);
    let names: Vec<&str> = commands.lines().filter_map(|l| l.split('\t').next()).collect();
    assert!(names.contains(&"drive") && !names.contains(&"drives"), "old names stay hidden: {commands}");

    let out = env.locallibrary().args(["completions", "zsh"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("#compdef locallibrary"));
    let out = env.locallibrary().args(["completions", "zsh", "--static"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("_locallibrary"));
    let out = env.locallibrary().arg("man").output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains(".TH locallibrary 1"));
    let dir = env.root.join("man");
    let out = env.locallibrary().args(["man", "-o", path(&dir)]).output().unwrap();
    assert!(out.status.success() && dir.join("locallibrary-drive-ls.1").is_file());
    assert!(!dir.join("locallibrary-help.1").exists() && !dir.join("locallibrary-drives.1").exists());
}

/// Regression: changes made in one step (rules, roles, settings, the registry) were made under
/// `--json` without `--yes`, which their help says they need; and `drive forget` didn't take a
/// mount path.
#[test]
fn one_step_changes_ask_first() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    let run = |args: &[&str]| {
        let out = env.locallibrary().arg("--json").args(args).output().unwrap();
        (out.status.code(), serde_json::from_slice::<Value>(&out.stdout).unwrap_or(Value::Null))
    };
    env.json(&["pull", path(&a), "--library", "Main", "--new-library"]);
    env.json(&["--yes", "smart", "add", "Smart/Slow", "--filter", "bpm:<100"]);
    for args in [
        &["smart", "add", "Smart/Fast", "--filter", "bpm:>=0"][..],
        &["smart", "add", "Smart/Slow", "--filter", "bpm:<90"],
        &["smart", "rm", "Smart/Slow"],
        &["settings", "set", "quantize=off"],
        &["drive", "role", path(&a), "archive"],
        &["drive", "retire", path(&a)],
        &["drive", "forget", path(&a)],
        &["drive", "rename", path(&a), "Club"],
        &["library", "rename", "Main", "Home"],
    ] {
        let (code, v) = run(args);
        assert_eq!((code, &v["outcome"]), (Some(3), &"not_confirmed".into()), "{args:?}: {v}");
        assert!(v["plan"]["summary"][0].is_string(), "{args:?}: {v}");
    }
    let rules = env.json(&["smart", "list"]);
    assert_eq!(rules["playlists"].as_array().map(Vec::len), Some(1), "{rules}");
    assert_eq!(rules["playlists"][0]["filter"], "bpm:<100");
    assert_eq!(env.json(&["drive", "log", path(&a)])["role"], Value::Null);
    assert!(env.json(&["library", "ls"]).to_string().contains("\"Main\""));
    let (code, v) = run(&["--yes", "drive", "forget", path(&a)]);
    assert_eq!((code, &v["report"]["name"]), (Some(0), &"A".into()), "{v}");
}

/// Regression: `drive reidentify` gave any drive a new identity at once, with no plan, no
/// question and nothing to show it was a copy, and its role was dropped without a word.
#[test]
fn reidentify_is_planned_and_needs_a_copy() {
    let env = Env::new();
    let volumes = env.root.join("volumes");
    let a = demo_drive(&volumes, "A");
    let run = |args: &[&str]| {
        let out = env.locallibrary().env("LOCALLIBRARY_VOLUMES", &volumes).arg("--json").args(args).output().unwrap();
        (out.status.code(), serde_json::from_slice::<Value>(&out.stdout).unwrap_or(Value::Null))
    };
    run(&["--yes", "adopt", path(&a), "--name", "Main"]);
    run(&["pull", path(&a), "--library", "Main", "--new-library"]);
    run(&["--yes", "drive", "role", path(&a), "backup"]);
    let (code, v) = run(&["--yes", "drive", "reidentify", path(&a)]);
    assert_eq!((code, &v["error"]["kind"]), (Some(1), &"conflict".into()), "not a copy: {v}");

    let copy = volumes.join("Copy");
    copy_dir(&a, &copy);
    let (code, v) = run(&["drive", "reidentify", path(&copy)]);
    assert_eq!((code, &v["outcome"]), (Some(3), &"not_confirmed".into()), "{v}");
    assert!(v["plan"]["warnings"][0].as_str().is_some_and(|w| w.contains("backup")), "{v}");
    let (code, v) = run(&["--yes", "drive", "reidentify", path(&copy)]);
    assert_eq!((code, &v["report"]["name"]), (Some(0), &"Main (copy)".into()), "{v}");
    assert_ne!(v["report"]["id"], run(&["drive", "info", path(&a)]).1["id"]);

    // The copy of an archive drive is written to with --overwrite-archive only.
    run(&["--yes", "drive", "role", path(&a), "archive"]);
    let another = volumes.join("Another");
    copy_dir(&a, &another);
    let (_, v) = run(&["--yes", "drive", "reidentify", path(&another)]);
    assert_eq!(v["error"]["reason"], "archive", "{v}");
    let (code, _) = run(&["--yes", "--overwrite-archive", "drive", "reidentify", path(&another)]);
    assert_eq!(code, Some(0));
}

/// Regression: `smart apply` and `settings apply` with no drives took every mounted drive, so one
/// archive drive among them made the whole command fail.
#[test]
fn unnamed_applies_leave_archive_drives_out() {
    let env = Env::new();
    let volumes = env.root.join("volumes");
    let (a, b) = (demo_drive(&volumes, "A"), demo_drive(&volumes, "B"));
    let run = |args: &[&str]| {
        let out = env.locallibrary().env("LOCALLIBRARY_VOLUMES", &volumes).arg("--json").args(args).output().unwrap();
        let v = serde_json::from_slice::<Value>(&out.stdout).unwrap_or(Value::Null);
        (out.status.code(), v, String::from_utf8_lossy(&out.stderr).into_owned())
    };
    run(&["pull", path(&a), "--library", "Main", "--new-library"]);
    run(&["pull", path(&b), "--library", "Main"]);
    run(&["--yes", "drive", "role", path(&b), "archive"]);
    run(&["--yes", "smart", "add", "Smart/All", "--filter", "bpm:>=0"]);
    let (code, v, err) = run(&["smart", "apply", "--dry-run"]);
    assert_eq!(code, Some(0), "{v} {err}");
    let drives: Vec<&str> = v["plan"].as_array().unwrap().iter().filter_map(|p| p["drive"].as_str()).collect();
    assert_eq!(drives, ["A"], "{v}");
    assert!(err.contains("left out B"), "{err}");
    run(&["--yes", "settings", "set", "quantize=off"]);
    let (code, v, _) = run(&["settings", "apply", "--dry-run"]);
    assert_eq!(code, Some(0), "{v}");
    assert!(v["plan"]["warnings"][0].as_str().is_some_and(|w| w.starts_with("left out B")), "{v}");
}

/// Regression: clap's usage errors bypassed `--json`, and incomplete command lines exited 1
/// (`invalid`) rather than 2 (`usage`); replaying an empty journal and a dedupe with nothing to
/// replace failed rather than having nothing to do.
#[test]
fn incomplete_command_lines_are_usage_errors() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    let run = |args: &[&str]| {
        let out = env.locallibrary().arg("--json").args(args).output().unwrap();
        (out.status.code(), serde_json::from_slice::<Value>(&out.stdout).unwrap_or(Value::Null))
    };
    for args in [
        &["tracks", "--bogus"][..],
        &["track", "rm", path(&a)],
        &["track", "set", path(&a), "--rating", "5"],
        &["playlist", "remove", path(&a), "Set"],
        &["sync", path(&a)],
        &["sync", "--from", path(&a), path(&a), path(&a)],
        &["drive", "inf"],
    ] {
        let (code, v) = run(args);
        assert_eq!(
            (code, &v["error"]["kind"], &v["outcome"]),
            (Some(2), &"usage".into(), &"error".into()),
            "{args:?}: {v}"
        );
    }
    for args in [&["edits", "replay", path(&a)][..], &["track", "dedupe", path(&a), "--metadata"]] {
        let (code, v) = run(args);
        assert_eq!((code, &v["outcome"]), (Some(0), &"nothing_to_do".into()), "{args:?}: {v}");
    }
}

/// Regression: a drive given by name lost to a folder of that name in the working directory, so
/// `sync Main Backup`, run where a `Backup/` folder was, wrote the library there.
#[test]
fn a_drive_name_is_never_taken_for_a_folder_here() {
    let env = Env::new();
    let volumes = env.root.join("volumes");
    demo_drive(&volumes, "Backup");
    let here = env.root.join("here");
    for f in ["Backup", "Loose"] {
        std::fs::create_dir_all(here.join(f)).unwrap();
    }
    let run = |args: &[&str]| {
        let mut c = env.locallibrary();
        let out = c.env("LOCALLIBRARY_VOLUMES", &volumes).current_dir(&here).arg("--json").args(args).output().unwrap();
        (out.status.code(), serde_json::from_slice::<Value>(&out.stdout).unwrap_or(Value::Null))
    };
    let (code, v) = run(&["drive", "info", "Backup"]);
    assert_eq!((code, &v["error"]["kind"]), (Some(1), &"invalid".into()), "{v}");
    assert!(v["error"]["message"].as_str().is_some_and(|m| m.contains("./Backup")), "{v}");
    let root = |v: &Value| PathBuf::from(v["root"].as_str().unwrap_or_default()).canonicalize().ok();
    let (code, v) = run(&["drive", "info", "./Backup"]);
    assert_eq!((code, root(&v)), (Some(0), here.join("Backup").canonicalize().ok()), "{v}");
    let (code, v) = run(&["drive", "info", "Loose"]);
    assert_eq!((code, root(&v)), (Some(0), here.join("Loose").canonicalize().ok()), "a folder no drive is named after");
}

/// Regression: `library log --json` keyed its object by library name, so a library named
/// `schema` was overwritten by the schema; `profile document` printed an object without it.
#[test]
fn every_json_object_keeps_its_schema() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    env.json(&["pull", path(&a), "--library", "schema", "--new-library"]);
    let v = env.json(&["library", "log", "schema"]);
    assert_eq!((v["schema"].as_u64(), v["libraries"]["schema"].as_array().map(Vec::len)), (Some(1), Some(1)), "{v}");
    let v = env.json(&["profile", "document", path(&a)]);
    assert_eq!(v["schema"], 1, "{v}");
}

/// Regression: `diff --playlist` with a name neither side has reported no differences, and the
/// hidden `compare` lacked `diff --summary`'s `--all`.
#[test]
fn diff_refuses_a_playlist_neither_side_has() {
    let env = Env::new();
    let (a, b) = (demo_drive(&env.root, "A"), demo_drive(&env.root, "B"));
    let run = |args: &[&str]| {
        let out = env.locallibrary().arg("--json").args(args).output().unwrap();
        (out.status.code(), serde_json::from_slice::<Value>(&out.stdout).unwrap_or(Value::Null))
    };
    let (code, v) = run(&["diff", path(&a), path(&b), "--playlist", "nope/none"]);
    assert_eq!((code, &v["error"]["kind"]), (Some(1), &"not_found".into()), "{v}");
    let (code, v) = run(&["compare", path(&a), path(&b), "--all"]);
    assert_eq!((code, &v["relation"]), (Some(0), &"identical".into()), "{v}");
}

/// Regression: SIGTERM (a GUI stopping the process) ended a command at once, wherever it was;
/// it stops at the next safe point, as Ctrl-C does.
#[cfg(unix)]
#[test]
fn sigterm_stops_at_a_safe_point() {
    use std::io::BufRead;
    let env = Env::new();
    let volumes = env.root.join("volumes");
    std::fs::create_dir_all(&volumes).unwrap();
    let mut child = env
        .locallibrary()
        .env("LOCALLIBRARY_VOLUMES", &volumes)
        .args(["watch", "--settle", "0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    std::io::BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
    assert!(line.contains("Watching"), "{line}");
    let kill = Command::new("kill").args(["-TERM", &child.id().to_string()]).status().unwrap();
    assert!(kill.success());
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(0), "stopped by the signal itself: {status:?}");
}

/// Regression: `pack` said which snapshot it saved the stick's state in, and which backup `undo`
/// puts back, only with `--json`.
#[test]
fn pack_says_what_it_saved_and_how_to_undo_it() {
    let env = Env::new();
    let (main, club) = (demo_drive(&env.root, "Main"), demo_drive(&env.root, "Club"));
    env.json(&["--yes", "playlist", "new", path(&main), "Friday", "--track", "1"]);
    env.json(&["pull", path(&club), "--library", "Club", "--new-library"]);
    add_history(&club, "HISTORY 001");
    let out =
        env.locallibrary().args(["--yes", "pack", path(&main), path(&club), "--playlist", "Friday"]).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{text}");
    assert!(text.contains("saved snapshot "), "{text}");
    assert!(text.contains("undo with: locallibrary undo") && text.contains("(backup "), "{text}");
}

/// `drive verify` and `drive recordings` take the one mounted drive when given none, as `export`
/// does, and ask which with several.
#[test]
fn verify_and_recordings_default_to_the_mounted_drive() {
    let env = Env::new();
    demo_drive(&env.root, "A");
    assert_eq!(env.json(&["drive", "verify"])["drive"], "A");
    assert_eq!(env.json(&["drive", "recordings"]), serde_json::json!([]));
    demo_drive(&env.root, "B");
    for (args, purpose) in [(["drive", "verify"], "verify"), (["drive", "recordings"], "take recordings from")] {
        let v = env.json(&args);
        let message = v["error"]["message"].as_str().unwrap();
        assert!(message.starts_with(&format!("say which drive to {purpose}")), "{v}");
    }
}
