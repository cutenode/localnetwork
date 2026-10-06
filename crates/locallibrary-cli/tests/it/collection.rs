//! `locallibrary collection` and the `collection.*` RPC methods, wired up. Reading a `master.db`
//! is tested in the library (`masterdb`), against databases built at test time; these never look
//! at rekordbox's own.

use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

fn locallibrary(root: &Path) -> Command {
    let mut c = crate::locallibrary(&root.join("store"), &[]);
    c.env("LOCALLIBRARY_MASTERDB", root.join("no-rekordbox/master.db")).env_remove("LOCALLIBRARY_MASTERDB_KEY");
    c
}

#[test]
fn collection_reports_what_it_cannot_read() {
    let dir = tempfile::tempdir().unwrap();
    // Without --db, LOCALLIBRARY_MASTERDB names the database; here it doesn't exist.
    let out = locallibrary(dir.path()).args(["--json", "collection"]).output().unwrap();
    assert!(!out.status.success());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["code"], "not_found", "{v}");

    // Not a rekordbox database: refused, not misread.
    let junk = dir.path().join("master.db");
    std::fs::write(&junk, vec![0x5au8; 8192]).unwrap();
    let out = locallibrary(dir.path())
        .args(["--json", "collection", "--db", junk.to_str().unwrap(), "tracks"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["code"], "invalid", "{v}");
    assert_eq!(std::fs::read(&junk).unwrap(), vec![0x5au8; 8192], "the database is only read");
}

#[test]
fn rpc_lists_and_answers_the_collection_methods() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = locallibrary(dir.path()).arg("rpc").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"version"}}"#).unwrap();
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":2,"method":"collection.info","params":{{}}}}"#).unwrap();
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
    for m in ["collection.info", "collection.library", "collection.diff"] {
        assert!(methods.iter().any(|x| x == m), "{m}");
    }
    assert_eq!(by_id[&2]["error"]["data"]["kind"], "not_found", "{}", by_id[&2]);
}
