//! `--json` runs of doctor, history, stats, export, diff, check, `xml --only` and
//! `restore --dry-run`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rekordbox_pdb::{Database, PdbEditor};
use serde_json::{Value, json};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../locallibrary/tests/fixtures")
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
}

fn pdb(root: &Path) -> PathBuf {
    root.join("PIONEER/rekordbox/export.pdb")
}

/// The demo export (two tracks) with placeholder audio, a playlist holding both, and a history
/// list a player recorded.
fn demo_drive(dir: &Path, name: &str) -> PathBuf {
    let root = dir.join(name);
    copy_dir(&fixtures().join("rekordcrate/demo_tracks"), &root);
    let db = Database::from_file(pdb(&root)).unwrap();
    for t in &db.tracks {
        let p = root.join(t.file_path().trim_start_matches('/'));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, format!("audio {}", t.title()).repeat(100)).unwrap();
    }
    let mut ed = PdbEditor::from_file(pdb(&root)).unwrap();
    let set = ed.create_playlist("Set", 0, false).unwrap();
    let history = ed.create_history_playlist("HISTORY 001").unwrap();
    for t in &db.tracks {
        ed.add_to_playlist(set, t.id, None).unwrap();
        ed.add_to_history(history, t.id).unwrap();
    }
    ed.save(pdb(&root)).unwrap();
    root
}

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

    fn locallibrary(&self) -> Command {
        crate::locallibrary(&self.root.join("store"), &[])
    }

    /// JSON output and whether the command succeeded.
    fn run(&self, args: &[&str]) -> (Value, bool) {
        let out = self.locallibrary().arg("--json").args(args).output().unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        let v = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{args:?}: {e}\n{text}\n{}", String::from_utf8_lossy(&out.stderr)));
        (v, out.status.success())
    }

    fn json(&self, args: &[&str]) -> Value {
        self.run(args).0
    }
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn doctor_reports_and_removes_orphans_only_with_yes() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    let orphan = a.join("Contents/Old/forgotten.mp3");
    std::fs::create_dir_all(orphan.parent().unwrap()).unwrap();
    std::fs::write(&orphan, "old").unwrap();

    let v = env.json(&["doctor", path(&a)]);
    assert_eq!(v["tracks"], 2);
    assert_eq!(v["orphans"][0]["path"], "Contents/Old/forgotten.mp3");
    assert_eq!(v["unfiled"].as_array().unwrap().len(), 0);
    assert!(v["duplicates"].as_array().unwrap().is_empty());
    assert!(!a.join(".locallibrary").exists(), "a report writes nothing");

    let v = env.json(&["doctor", path(&a), "--only", "orphans,unfiled"]);
    assert!(v["duplicates"].is_null() && v["missing"].is_null());
    assert_eq!(v["orphans"].as_array().unwrap().len(), 1);

    let v = env.json(&["doctor", path(&a), "--fix-orphans"]);
    assert_eq!(v["applied"], false);
    assert!(orphan.is_file());
    let v = env.json(&["--yes", "doctor", path(&a), "--fix-orphans", "--dry-run"]);
    assert_eq!(v["applied"], false);
    let v = env.json(&["--yes", "doctor", path(&a), "--fix-orphans"]);
    assert_eq!(v["applied"], true, "{v}");
    assert_eq!(v["report"]["files_deleted"], 1);
    assert!(!orphan.exists());

    // Undo: restore --dry-run shows what comes back, without restoring.
    let v = env.json(&["restore", path(&a), "--dry-run"]);
    assert_eq!(v["plan"]["backup"]["reason"], "before removing orphaned files", "{v}");
    assert!(v["plan"]["diff"]["added_tracks"].as_array().unwrap().is_empty());
    let files = v["plan"]["files"].as_array().unwrap();
    assert!(files.iter().any(|f| f["path"] == "Contents/Old/forgotten.mp3" && f["change"] == "put back"), "{v}");
    assert_eq!(v["outcome"], "dry_run");
    assert!(!orphan.exists());
    // ...and undo puts it back.
    let v = env.json(&["--yes", "undo", path(&a)]);
    assert_eq!(v["applied"], true, "{v}");
    assert!(orphan.is_file());
    // The undo backed up what it replaced first (putting that back redoes it); nothing is left
    // to undo.
    let v = env.json(&["undo", "--list", path(&a)]);
    let backups = v["drives"][0]["backups"].as_array().unwrap();
    assert_eq!(backups.len(), 2, "{v}");
    assert_eq!(backups[0]["undoes"], backups[1]["id"], "{v}");
    assert!(backups.iter().all(|b| b["next"] == false), "{v}");
}

#[test]
fn history_stats_export_and_diff() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");

    let v = env.json(&["history", path(&a)]);
    assert_eq!(v["drive"]["lists"][0]["name"], "HISTORY 001");
    assert!(v["archive"].is_null());
    let v = env.json(&["history", path(&a), "history 001"]);
    assert_eq!(v["tracks"].as_array().unwrap().len(), 2);
    let out = env.root.join("set.txt");
    env.json(&["history", path(&a), "HISTORY 001", "-o", path(&out)]);
    assert_eq!(
        std::fs::read_to_string(&out).unwrap(),
        "01. Loopmasters - Demo Track 1\n02. Loopmasters - Demo Track 2\n"
    );
    let (v, ok) = env.run(&["history", path(&a), "-o", path(&out)]);
    assert!(!ok && v["error"]["message"].as_str().unwrap().contains("name the history list"));

    // The store keeps the list once rekordbox has deleted it from the drive.
    env.json(&["pull", path(&a)]);
    let history: Vec<u32> = Database::from_file(pdb(&a)).unwrap().history_playlists.iter().map(|h| h.id).collect();
    let mut ed = PdbEditor::from_file(pdb(&a)).unwrap();
    ed.remove_history_playlists(&history).unwrap();
    ed.save(pdb(&a)).unwrap();
    let v = env.json(&["history", path(&a), "--all"]);
    assert!(v["drive"]["lists"].as_array().unwrap().is_empty());
    assert_eq!(v["archive"]["lists"][0]["name"], "HISTORY 001");
    assert_eq!(v["archive"]["lists"][0]["first_seen"]["drive"], "A");
    let m3u = env.root.join("old.m3u8");
    env.json(&["history", path(&a), "--all", "HISTORY 001", "-o", path(&m3u), "--paths", "drive"]);
    assert!(std::fs::read_to_string(&m3u).unwrap().contains("\n/Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3\n"));

    let v = env.json(&["stats", path(&a)]);
    assert_eq!(v["tracks"], 2);
    assert_eq!(v["history_lists"], 0);

    let v = env.json(&["export", path(&a), "Set", "-o", path(&env.root.join("set.csv"))]);
    assert_eq!(v["report"]["format"], "csv");
    assert_eq!(v["report"]["files"][0]["tracks"], 2);
    let v = env.json(&["export", path(&a), "-o", path(&env.root.join("x.m3u8")), "--paths", "absolute"]);
    assert_eq!(v["report"]["files"][0]["tracks"], 2);

    // B drops a track from the playlist.
    let b = env.root.join("B");
    copy_dir(&a, &b);
    let db = Database::from_file(pdb(&b)).unwrap();
    let set = db.playlist_tree.iter().find(|p| p.name == "Set").unwrap().id;
    let mut ed = PdbEditor::from_file(pdb(&b)).unwrap();
    ed.set_playlist_entries(set, &[db.tracks[1].id]).unwrap();
    ed.save(pdb(&b)).unwrap();
    let v = env.json(&["diff", path(&a), path(&b)]);
    assert_eq!(v["changed_playlists"][0]["path"], "Set");
    assert_eq!(v["changed_playlists"][0]["removed"][0]["position"], 1);
    let snapshot = env.json(&["log"])["libraries"]["A"][0]["short_id"].as_str().unwrap().to_string();
    let v = env.json(&["diff", &format!("snapshot:{snapshot}"), path(&b), "--playlist", "Set"]);
    assert_eq!(v["changed_playlists"][0]["removed"].as_array().unwrap().len(), 1);

    let xml = env.root.join("set.xml");
    let v = &env.json(&["xml", path(&a), "-o", path(&xml), "--only", "Set"])["report"];
    assert_eq!((v["tracks"].as_i64(), v["playlists"].as_i64()), (Some(2), Some(1)));
}

#[test]
fn check_exits_with_failure_on_problems() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    let (v, ok) = env.run(&["check", path(&a), "--players", "CDJ-2000NXS2"]);
    assert!(ok, "{v}");
    assert_eq!(v["problems"], 0);
    let b = env.root.join("B");
    copy_dir(&a, &b);
    std::fs::remove_file(b.join("Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3")).unwrap();
    let (v, ok) = env.run(&["check", path(&a), path(&b), "--players", "CDJ-3000X"]);
    assert!(!ok);
    let problems: Vec<(&str, &str)> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["level"] == "problem")
        .map(|i| (i["drive"].as_str().unwrap(), i["check"].as_str().unwrap()))
        .collect();
    assert_eq!(problems, [("A", "players"), ("B", "same_library"), ("B", "files"), ("B", "players")]);
}

#[test]
fn remembered_players() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    let v = env.json(&["players"]);
    assert_eq!(v["default"].as_array().unwrap().len(), 0);
    // Like every write, it shows its plan and asks first: without --yes nothing is remembered.
    let set = ["players", "set", "cdj-3000x,CDJ-2000NXS2"];
    let out = env.locallibrary().arg("--json").args(set).output().unwrap();
    assert_eq!(out.status.code(), Some(3));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!((&v["outcome"], &v["plan"]["operation"]), (&json!("not_confirmed"), &json!("players.set")));
    assert_eq!(v["plan"]["to"], json!(["CDJ-3000X", "CDJ-2000NXS2"]));
    assert_eq!(env.json(&["players"])["default"], json!([]));
    let v = env.json(&[&["--yes"][..], &set[..]].concat());
    assert_eq!(v["report"]["default"], json!(["CDJ-3000X", "CDJ-2000NXS2"]));
    assert_eq!(env.json(&[&["--yes"][..], &set[..]].concat())["outcome"], "nothing_to_do");
    // `check` without --players uses them: the CDJ-3000X can't read a drive without OneLibrary.
    let (v, ok) = env.run(&["check", path(&a)]);
    assert!(!ok, "{v}");
    assert_eq!(v["players"][0]["from"], "default");
    let (v, ok) = env.run(&["players", "set", "CDJ-9000"]);
    assert!(!ok, "{v}");
    let (v, ok) = env.run(&["players", "clear", "--dry-run"]);
    assert!(ok && v["plan"]["to"] == json!([]) && v["outcome"] == "dry_run", "{v}");
    env.json(&["--yes", "players", "clear"]);
    let (v, ok) = env.run(&["check", path(&a)]);
    assert!(ok, "{v}");
    assert_eq!(v["players"][0]["from"], "none");
    let models = env.json(&["players", "models"]);
    assert!(models.as_array().unwrap().iter().any(|m| m["name"] == "OMNIS-DUO" && m["exfat"]["value"] == "yes"));
    assert_eq!(env.json(&["players", "models", "cdj-3000"])["exfat"]["value"]["firmware"], "1.20");
}

#[test]
fn rpc_lists_the_new_methods() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    let mut child = env.locallibrary().arg("rpc").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        for (id, method, params) in [
            (1, "version", serde_json::json!({})),
            (2, "doctor", serde_json::json!({ "drive": path(&a), "only": ["unfiled"] })),
            (3, "plays.history", serde_json::json!({ "drive": path(&a) })),
            (4, "check", serde_json::json!({ "drives": [path(&a)] })),
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
    let methods = by_id[&1]["result"]["methods"].as_array().unwrap();
    for m in [
        "doctor",
        "doctor.orphans.apply",
        "plays.archive",
        "export",
        "diff",
        "drive.restore.preview",
        "check",
        "players.set.plan",
        "players.clear.plan",
    ] {
        assert!(methods.iter().any(|x| x == m), "{m}");
    }
    assert!(by_id[&2]["result"]["unfiled"].as_array().unwrap().is_empty());
    assert_eq!(by_id[&3]["result"]["lists"][0]["name"], "HISTORY 001");
    assert_eq!(by_id[&4]["result"]["problems"], 0);
}

/// A 16-bit mono WAV holding a second-long tone, `secs` times.
fn tone(secs: u32) -> Vec<u8> {
    let rate = 8_000u32;
    let data: Vec<u8> =
        (0..rate * secs).flat_map(|i| (((i as f64 * 0.07).sin() * 8000.0) as i16).to_le_bytes()).collect();
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + data.len() as u32).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    for (v, n) in [(16u32, 4), (1, 2), (1, 2), (rate, 4), (rate * 2, 4), (2, 2), (16, 2)] {
        wav.extend(&v.to_le_bytes()[..n]);
    }
    wav.extend(b"data");
    wav.extend((data.len() as u32).to_le_bytes());
    wav.extend(data);
    wav
}

/// `mix` makes the store it keeps fingerprints in (so `status` works after it, where a cache
/// written into a bare folder once stopped the store being made), and what it writes to share
/// names no drive.
#[test]
fn mix_keeps_its_fingerprints_in_a_store_it_makes() {
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    let set = env.root.join("mixes/friday.wav");
    std::fs::create_dir_all(set.parent().unwrap()).unwrap();
    std::fs::write(&set, tone(5)).unwrap();
    let out = env.root.join("out");
    let (v, ok) = env.run(&["mix", path(&set), "--drive", path(&a), "--export", &format!("{}/", out.display())]);
    assert!(ok, "{v}");
    assert_eq!(v[0]["library"]["failed"].as_array().unwrap().len(), 2, "the placeholders don't decode: {v}");
    assert!(env.root.join("store/store.json").is_file());
    assert!(env.run(&["status"]).1, "the store works after mix");

    let written: Vec<PathBuf> = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().path()).collect();
    let name = written[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("friday_") && name.ends_with(".mix.json"), "{name}");
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(&written[0]).unwrap()).unwrap();
    assert_eq!(doc["version"], 2);
    assert!(doc.get("library").is_none() && !doc.to_string().contains("\"A\""), "{doc}");

    // Identified again into the same folder: the file there is replaced only with --replace.
    std::fs::write(&written[0], "mine").unwrap();
    let again =
        |more: &[&str]| env.run(&[&["mix", path(&set), "--drive", path(&a), "--export", path(&out)], more].concat());
    let (v, ok) = again(&[]);
    assert_eq!((ok, &v["error"]["kind"]), (false, &"conflict".into()), "{v}");
    assert_eq!(std::fs::read_to_string(&written[0]).unwrap(), "mine");
    assert!(again(&["--replace"]).1);
    assert!(std::fs::read_to_string(&written[0]).unwrap().contains("\"version\""));
}

/// `mix.identify {save}` answers with the report even when it can't be kept, and says why:
/// minutes of identifying aren't lost to a store it can't write.
#[test]
fn an_identified_mix_that_cant_be_kept_still_comes_back() {
    use std::io::Write;
    let env = Env::new();
    let a = demo_drive(&env.root, "A");
    let set = env.root.join("friday.wav");
    std::fs::write(&set, tone(5)).unwrap();
    // A file where the store keeps its reports.
    std::fs::create_dir_all(env.root.join("store")).unwrap();
    std::fs::write(env.root.join("store/reports"), "").unwrap();
    let mut child = env.locallibrary().arg("rpc").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let params = json!({ "mix": set, "drive": a, "save": true });
    let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "mix.identify", "params": params });
    writeln!(child.stdin.take().unwrap(), "{request}").unwrap();
    let out = child.wait_with_output().unwrap();
    let messages: Vec<Value> =
        String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let reply = messages.iter().find(|m| m["id"] == 1).unwrap();
    assert!(reply["result"]["plays"].is_array() && reply["result"].get("saved").is_none(), "{reply}");
    let warned = messages.iter().any(|m| m["params"]["event"].to_string().contains("wasn't kept"));
    assert!(warned, "{messages:?}");
}
