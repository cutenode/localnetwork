//! `locallibrary tags` and the `tags.*` RPC methods.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

/// The demo export (with rekordbox's default My Tags in exportExt.pdb) and placeholder audio.
fn demo_drive(dir: &Path) -> PathBuf {
    let root = dir.join("Demo");
    copy_dir(&fixtures().join("rekordcrate/demo_tracks"), &root);
    let db = rekordbox_pdb::Database::from_file(root.join("PIONEER/rekordbox/export.pdb")).unwrap();
    for t in &db.tracks {
        let p = root.join(t.file_path().trim_start_matches('/'));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, format!("audio {}", t.title()).repeat(100)).unwrap();
    }
    root
}

fn locallibrary(dir: &Path) -> Command {
    crate::locallibrary(&dir.join("store"), &[])
}

fn json(dir: &Path, args: &[&str]) -> Value {
    let out = locallibrary(dir).arg("--json").args(args).output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{args:?}: {e}\n{text}\n{}", String::from_utf8_lossy(&out.stderr)))
}

/// Track names carrying `Category/Tag`, from `tags --json`.
fn tracks_with(list: &Value, path: &str) -> Vec<String> {
    let tag = list["categories"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|c| c["tags"].as_array().unwrap())
        .find(|t| t["path"] == path)
        .unwrap_or_else(|| panic!("{path} in {list}"));
    tag["tracks"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
}

#[test]
fn tags_are_listed_planned_and_applied() {
    let dir = tempfile::tempdir().unwrap();
    let drive = demo_drive(dir.path());
    let d = drive.to_str().unwrap();

    let list = json(dir.path(), &["tags", d]);
    assert_eq!(list["sources"], json!(["export_ext"]));
    assert_eq!(list["categories"].as_array().unwrap().len(), 4);
    assert!(tracks_with(&list, "Genre/Techno").is_empty());

    // Without --yes, JSON mode only plans.
    let v = json(dir.path(), &["tags", "add", d, "Genre/Techno", "--query", "Demo Track"]);
    assert_eq!(v["applied"], false);
    assert_eq!(v["plan"]["tracks"].as_array().unwrap().len(), 2);
    assert!(tracks_with(&json(dir.path(), &["tags", "list", d]), "Genre/Techno").is_empty());

    let v = json(dir.path(), &["--yes", "tags", "add", d, "Genre/Techno", "--query", "Demo Track"]);
    assert_eq!(v["applied"], true, "{v}");
    assert_eq!(v["report"]["databases"], json!(["export_ext"]));
    let list = json(dir.path(), &["tags", d]);
    assert_eq!(tracks_with(&list, "Genre/Techno"), ["Loopmasters - Demo Track 1", "Loopmasters - Demo Track 2"]);

    json(dir.path(), &["--yes", "tags", "rename", d, "Genre/Techno", "Hard Techno"]);
    json(dir.path(), &["--yes", "tags", "remove", d, "Genre/Hard Techno", "--track", "1"]);
    let list = json(dir.path(), &["tags", d]);
    assert_eq!(tracks_with(&list, "Genre/Hard Techno"), ["Loopmasters - Demo Track 2"]);

    let v = json(dir.path(), &["--yes", "tags", "delete", d, "Genre"]);
    assert_eq!(v["error"]["code"], "invalid");
    let v = json(dir.path(), &["--yes", "tags", "delete", d, "Genre/Hard Techno", "--dry-run"]);
    assert_eq!(v["applied"], false);
    json(dir.path(), &["--yes", "tags", "delete", d, "Genre/Hard Techno"]);
    let list = json(dir.path(), &["tags", d]);
    let genre = &list["categories"][0]["tags"];
    assert!(genre.as_array().unwrap().iter().all(|t| t["name"] != "Hard Techno"));
}

#[test]
fn rpc_tags_methods() {
    let dir = tempfile::tempdir().unwrap();
    let drive = demo_drive(dir.path());
    let d = drive.to_str().unwrap();
    let mut child = locallibrary(dir.path()).arg("rpc").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        let edit = json!({ "drive": d, "action": "add", "tag": "Situation/Lounge", "ids": [2] });
        writeln!(stdin, "{}", json!({ "jsonrpc": "2.0", "id": 1, "method": "tags.plan", "params": edit })).unwrap();
        writeln!(stdin, "{}", json!({ "jsonrpc": "2.0", "id": 2, "method": "version" })).unwrap();
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
    assert_eq!(by_id[&1]["result"]["tag"], "Situation/Lounge", "{}", by_id[&1]);
    assert_eq!(by_id[&1]["result"]["tracks"][0]["id"], 2);
    let methods = by_id[&2]["result"]["methods"].as_array().unwrap();
    for m in ["tags.list", "tags.plan", "tags.apply"] {
        assert!(methods.iter().any(|x| x == m), "{m}");
    }

    // Applied in a session of its own (requests in one session run concurrently).
    let mut child = locallibrary(dir.path()).arg("rpc").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        let edit = json!({ "drive": d, "action": "add", "tag": "Situation/Lounge", "ids": [2] });
        writeln!(stdin, "{}", json!({ "jsonrpc": "2.0", "id": 3, "method": "tags.apply", "params": edit })).unwrap();
    }
    drop(child.stdin.take());
    let out = child.wait_with_output().unwrap();
    let line = String::from_utf8(out.stdout).unwrap();
    let v: Value = serde_json::from_str(line.lines().last().unwrap()).unwrap();
    assert_eq!(v["result"]["tracks_changed"], 1, "{v}");
    let list = json(dir.path(), &["tags", d]);
    assert_eq!(tracks_with(&list, "Situation/Lounge"), ["Loopmasters - Demo Track 2"]);
}
