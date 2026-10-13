use serde_json::json;

use super::*;

#[test]
fn every_plan_has_its_apply_and_every_name_one_method() {
    let r = registry();
    for m in r.methods() {
        match m.kind {
            Kind::Plan | Kind::Apply => {
                let pair = m.pair.unwrap_or_else(|| panic!("{} has no pair", m.name));
                assert_eq!(r.get(pair).unwrap().pair, Some(m.name), "{}", m.name);
                assert!(m.operation.is_some(), "{}", m.name);
            }
            _ => assert!(m.pair.is_none(), "{}", m.name),
        }
        assert_eq!(m.run.is_none(), m.kind == Kind::Control, "{}", m.name);
        if let Some(instead) = m.deprecated {
            assert!(r.get(instead).is_some(), "{} points to {instead}", m.name);
        }
        for name in std::iter::once(m.name).chain(m.aliases.iter().copied()) {
            assert!(!name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || "._$/".contains(c)));
        }
    }
    // Applies and plans of the same kind of plan agree on it.
    for m in r.methods().filter(|m| m.kind == Kind::Apply) {
        assert_eq!(r.get(m.pair.unwrap()).unwrap().operation, m.operation, "{}", m.name);
    }
    assert!(r.get("$/test.wait").is_some() && !r.names().contains(&"$/test.wait"));
}

/// Methods run as a worker runs them, with a store and drives in a temporary folder. Drive
/// lookups on this thread see only those drives, never the machine's.
struct Sidecar {
    dir: tempfile::TempDir,
    plans: Plans,
}

impl Sidecar {
    fn new() -> Sidecar {
        let dir = tempfile::tempdir().unwrap();
        locallibrary::testing::volumes_on_this_thread(Some(vec![dir.path().to_path_buf()]));
        locallibrary::testing::store_root_on_this_thread(Some(dir.path().join("store")));
        Sidecar { dir, plans: Plans::default() }
    }

    fn store(&self) -> std::path::PathBuf {
        self.dir.path().join("store")
    }

    /// The demo export as the drive `name`, with placeholder audio and a history list a player
    /// recorded.
    fn demo(&self, name: &str) -> String {
        fn copy(src: &Path, dst: &Path) {
            std::fs::create_dir_all(dst).unwrap();
            for e in std::fs::read_dir(src).unwrap().map(Result::unwrap) {
                match e.file_type().unwrap().is_dir() {
                    true => copy(&e.path(), &dst.join(e.file_name())),
                    false => drop(std::fs::copy(e.path(), dst.join(e.file_name())).unwrap()),
                }
            }
        }
        let root = self.dir.path().join(name);
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/locallibrary/tests/fixtures/rekordcrate/demo_tracks");
        copy(&fixture, &root);
        let pdb = root.join("PIONEER/rekordbox/export.pdb");
        for t in &rekordbox_pdb::Database::from_file(&pdb).unwrap().tracks {
            let audio = root.join(t.file_path().trim_start_matches('/'));
            std::fs::create_dir_all(audio.parent().unwrap()).unwrap();
            std::fs::write(audio, t.title()).unwrap();
        }
        let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb).unwrap();
        let list = ed.create_history_playlist("HISTORY 001").unwrap();
        ed.add_to_history(list, 1).unwrap();
        ed.save(&pdb).unwrap();
        root.display().to_string()
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let (root, ctx) = (self.store(), Ctx::silent());
        let call = Call { root: &root, plans: &self.plans, ctx: &ctx };
        registry().get(method).unwrap().run(&call, params).expect("a worker runs it")
    }

    fn ok(&self, method: &str, params: Value) -> Value {
        self.call(method, params).unwrap_or_else(|e| panic!("{method}: {}", e.message))
    }
}

/// A drive's library, read from its database rather than the sidecar's cache.
fn library(drive: &str) -> locallibrary::Library {
    locallibrary::pdb::read_library(&Path::new(drive).join("PIONEER/rekordbox/export.pdb")).unwrap()
}

fn playlists(drive: &str) -> Vec<String> {
    library(drive).playlists.into_iter().map(|p| p.name).collect()
}

/// Reads with no store make none: they answer as if it were empty (mounted drives are still
/// there), or that there's no store when they're about the store itself.
#[test]
fn reads_without_a_store_make_none() {
    let s = Sidecar::new();
    let a = s.demo("A");
    let empty = [
        ("profile.prefs", json!({}), None),
        ("sets.played", json!({ "track": "Demo Track 1" }), Some(json!([]))),
        ("store.copies", json!({}), Some(json!([]))),
        ("library.list", json!({}), Some(json!([]))),
        ("smart.list", json!({}), Some(json!({ "playlists": [] }))),
        ("settings.profile", json!({}), Some(json!({ "base": {}, "drives": {} }))),
        ("players.show", json!({}), Some(json!({ "default": [], "libraries": [] }))),
        ("drives.known", json!({}), Some(json!([]))),
        ("drive.groups", json!({}), Some(json!([]))),
        ("settings.show", json!({ "drive": a }), None),
        ("sets.suggest", json!({}), None),
        ("drive.recognise", json!({ "drive": a }), None),
    ];
    for (method, params, want) in empty {
        let got = s.ok(method, params);
        if let Some(want) = want {
            assert_eq!(got, want, "{method}");
        }
        assert!(!s.store().exists(), "{method} made a store");
    }
    let status = s.ok("status", json!({}));
    assert_eq!((&status[0]["drive"]["volume_name"], &status[0]["library"]), (&json!("A"), &Value::Null), "{status}");
    let fleet = s.ok("fleet.status", json!({ "all": true }));
    assert_eq!(fleet["mounted"].as_array().map(Vec::len), Some(1), "{fleet}");
    assert!(!s.store().exists(), "status made a store");

    let about_the_store = [
        ("store.info", json!({})),
        ("store.history", json!({ "library": "Main" })),
        ("store.verify", json!({})),
        ("drive.log", json!({ "drive": a })),
    ];
    for (method, params) in about_the_store {
        assert_eq!(s.call(method, params).err().map(|e| e.kind), Some("not_found"), "{method}");
        assert!(!s.store().exists(), "{method} made a store");
    }
}

/// `$/test.wait` answers only in the sidecar's own tests (`LOCALLIBRARY_RPC_TEST=1`).
#[test]
fn the_test_method_is_not_there_outside_the_tests() {
    let s = Sidecar::new();
    assert_eq!(s.call("$/test.wait", json!({})).err().map(|e| e.kind), Some("method_not_found"));
}

/// An apply given a request instead of a plan's id plans and applies it at once.
#[test]
fn an_apply_given_a_request_plans_and_applies_it() {
    let s = Sidecar::new();
    assert_eq!(s.ok("store.gc.apply", json!({ "dry_run": true }))["freed_bytes"], 0);
    let unembed = s.call("art.unembed.apply", json!({})).err().map(|e| (e.kind, e.message));
    assert_eq!(unembed, Some(("not_found", "not found: no embedded art to take back".into())));
}

/// Deleting history lists by a plan's id or by request comes to the same, with or without a
/// store.
#[test]
fn history_lists_are_deleted_by_plan_or_by_request() {
    let s = Sidecar::new();
    let (a, b) = (s.demo("A"), s.demo("B"));
    assert_eq!(s.ok("plays.history", json!({ "drive": a }))["lists"].as_array().unwrap().len(), 1);
    let plan = s.ok("history.delete.plan", json!({ "drive": a, "force": true }));
    s.ok("history.delete.apply", json!({ "plan_id": plan["plan_id"] }));
    s.ok("history.delete.apply", json!({ "drive": b, "force": true }));
    assert!(library(&a).history.is_empty() && library(&b).history.is_empty());
}

/// Refused because the store is busy, a plan stays, to be applied once it isn't.
#[test]
fn a_plan_refused_while_the_store_is_locked_stays() {
    let s = Sidecar::new();
    let id = s.ok("store.gc.plan", json!({}))["plan_id"].clone();
    let lock = Store::open(s.store()).unwrap().lock().unwrap();
    let busy = s.call("store.gc.apply", json!({ "plan_id": id }));
    assert_eq!(busy.err().map(|e| e.kind), Some("locked"));
    drop(lock);
    assert!(s.ok("store.gc.apply", json!({ "plan_id": id }))["freed_bytes"].is_number());
}

/// Restoring a backup is planned as an undo, which it is, under its own name too.
#[test]
fn a_backup_is_restored_by_plan() {
    let s = Sidecar::new();
    let a = s.demo("A");
    s.ok("edit.apply", json!({ "drive": a, "edits": [{ "op": "create_playlist", "path": "Friday" }] }));
    assert_eq!(playlists(&a), ["Friday"]);
    let plan = s.ok("drive.restore.plan", json!({ "drive": a }));
    assert_eq!(plan["operation"], "undo", "{plan}");
    s.ok("drive.restore.apply", json!({ "plan_id": plan["plan_id"] }));
    assert!(playlists(&a).is_empty());
}

/// A playlist deleted from a drive comes back from the store's snapshot of it, and the drive's
/// My Tags are listed.
#[test]
fn a_playlist_comes_back_from_the_store() {
    let s = Sidecar::new();
    let a = s.demo("A");
    let friday = json!({ "op": "add_tracks", "playlist": "Friday", "tracks": { "ids": [1] } });
    s.ok("edit.apply", json!({ "drive": a, "edits": [{ "op": "create_playlist", "path": "Friday" }, friday] }));
    s.ok("pull.apply", json!({ "drive": a, "library": "Main", "new_library": true }));
    s.ok("edit.apply", json!({ "drive": a, "edits": [{ "op": "delete_playlist", "playlist": "Friday" }] }));
    assert!(playlists(&a).is_empty());
    s.ok("playlist.restore.apply", json!({ "drive": a, "playlist": "Friday" }));
    assert_eq!(playlists(&a), ["Friday"]);

    s.ok("tags.apply", json!({ "drive": a, "action": "add", "tag": "Genre/Deep", "ids": [1] }));
    let tags = s.ok("tags.list", json!({ "drive": a }));
    assert_eq!(tags["categories"][0]["name"], "Genre", "{tags}");
}
