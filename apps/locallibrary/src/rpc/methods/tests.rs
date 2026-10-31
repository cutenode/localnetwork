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
        ("drive.space", json!({ "drive": a }), None),
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
    // Collecting and taking back art change what a store holds: there has to be one.
    Store::open_or_init(s.store()).unwrap();
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
    Store::open_or_init(s.store()).unwrap();
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

use hegel::TestCase;
use hegel::generators as gs;

/// Every file under `dir`, with what's in it.
fn files_under(dir: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    walkdir::WalkDir::new(dir)
        .sort_by_file_name()
        .into_iter()
        .map(Result::unwrap)
        .filter(|e| e.file_type().is_file())
        .map(|e| (e.path().to_path_buf(), std::fs::read(e.path()).unwrap()))
        .collect()
}

/// With drives looked for only in a folder, as here, this computer's disks are hidden from the
/// methods that would touch them: listing, formatting and checking a disk are refused whatever
/// drive or disk is named, and a drive that's a folder rather than a mounted volume has nothing
/// to eject. None of them writes to the drive.
#[hegel::test(test_cases = 8)]
fn the_computers_disks_stay_out_of_reach_while_its_drives_are_hidden(tc: TestCase) {
    let s = Sidecar::new();
    let a = s.demo("A");
    let before = files_under(Path::new(&a));
    let gone = s.dir.path().join("Gone").display().to_string();
    let drive = tc.draw(gs::sampled_from(vec![a.clone(), "A".into(), "Nope".into(), gone]));
    let disk =
        tc.draw(gs::sampled_from(vec![drive.clone(), "disk4".into(), "/dev/sdb".into(), "PhysicalDrive2".into()]));
    let identity = tc.draw(gs::text().max_size(12));
    let found = drive == a || drive == "A";
    let hidden = "LOCALLIBRARY_VOLUMES is set, which hides this machine's drives";
    let methods =
        vec!["disks.list", "format.plan", "format.apply", "drive.fsck", "drive.eject.plan", "drive.eject.apply"];
    for method in tc.draw(gs::permutations(methods)) {
        let (params, kind, saying) = match method {
            "disks.list" => (json!({}), "invalid", hidden),
            "format.plan" => (json!({ "drive": disk }), "invalid", hidden),
            "format.apply" => (json!({ "drive": disk, "identity": identity }), "invalid", hidden),
            "drive.fsck" if found => (json!({ "drive": drive, "repair": tc.draw(gs::booleans()) }), "invalid", hidden),
            _ if !found => (json!({ "drive": drive, "drives": [drive] }), "not_found", ""),
            _ => (json!({ "drives": [drive] }), "invalid", "isn't the root of a volume mounted from a disk"),
        };
        let e = s.call(method, params.clone()).err().unwrap_or_else(|| panic!("{method} {params} ran"));
        assert_eq!(e.kind, kind, "{method} {params}: {}", e.message);
        assert!(e.message.contains(saying), "{method} {params}: {}", e.message);
        let after = files_under(Path::new(&a));
        let changed: Vec<_> =
            after.iter().chain(&before).filter(|f| !before.contains(f) || !after.contains(f)).collect();
        assert!(changed.is_empty(), "{method} {params} wrote {:?}", changed.iter().map(|f| &f.0).collect::<Vec<_>>());
    }
}

/// A drive's My Tags as its databases have them.
fn tags_on(drive: &str) -> Value {
    let tags = api::tags_list(&api::DriveRef { drive: drive.into() }).unwrap();
    json!({ "categories": tags.categories, "differences": tags.differences })
}

/// A run of My Tag edits, each applied to drive A by its plan's id and to B (the same export)
/// straight from the request, is refused or not alike and leaves both with the same tags on the
/// same tracks.
#[hegel::test(test_cases = 8)]
fn a_tag_edit_from_its_request_does_what_its_plan_does(tc: TestCase) {
    let s = Sidecar::new();
    let (a, b) = (s.demo("A"), s.demo("B"));
    let tags = vec!["Genre/Deep", "Genre/Techno", "Mood/Dark", "Genre", "Nope/Never"];
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(5)) {
        let tag = tc.draw(gs::sampled_from(tags.clone()));
        let ids = tc.draw(gs::subsequences(vec![1, 2]));
        let edit = match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => json!({ "action": "add", "tag": tag, "ids": ids }),
            1 => json!({ "action": "remove", "tag": tag, "ids": ids }),
            2 => json!({ "action": "rename", "tag": tag, "to": tc.draw(gs::sampled_from(tags.clone())) }),
            _ => json!({ "action": "delete", "tag": tag }),
        };
        let on = |drive: &str| {
            let mut e = edit.clone();
            e["drive"] = json!(drive);
            e
        };
        let by_id = s.call("tags.plan", on(&a)).and_then(|p| s.call("tags.apply", json!({ "plan_id": p["plan_id"] })));
        let by_request = s.call("tags.apply", on(&b));
        let kind = |r: &Result<Value, RpcError>| r.as_ref().err().map(|e| e.kind);
        assert_eq!(kind(&by_id), kind(&by_request), "{edit}: {by_id:?} {by_request:?}");
        assert_eq!(tags_on(&a), tags_on(&b), "{edit}");
    }
}

/// A plan refused because the store is busy, however often it's tried, stays: once the store is
/// free it applies by its id, once. A request applied straight away while the store is busy is
/// refused the same way.
#[hegel::test(test_cases = 10)]
fn a_plan_refused_while_the_store_is_busy_applies_once_it_is_free(tc: TestCase) {
    let s = Sidecar::new();
    Store::open_or_init(s.store()).unwrap();
    let id = s.ok("store.gc.plan", json!({}))["plan_id"].clone();
    let lock = Store::open(s.store()).unwrap().lock().unwrap();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        let params = match tc.draw(gs::booleans()) {
            true => json!({ "plan_id": id }),
            false => json!({ "dry_run": tc.draw(gs::booleans()) }),
        };
        let busy = s.call("store.gc.apply", params.clone());
        assert_eq!(busy.err().map(|e| e.kind), Some("locked"), "{params}");
    }
    drop(lock);
    assert!(s.ok("store.gc.apply", json!({ "plan_id": id }))["freed_bytes"].is_number());
    let again = s.call("store.gc.apply", json!({ "plan_id": id }));
    assert_eq!(again.err().map(|e| e.kind), Some("stale_plan"));
}

/// `$/test.wait` isn't there outside the sidecar's own tests, whatever it's asked to wait for.
#[hegel::test(test_cases = 10)]
fn the_test_method_is_never_there_outside_the_tests(tc: TestCase) {
    let s = Sidecar::new();
    let params = json!({
        "ms": tc.draw(gs::integers::<u64>().max_value(50)),
        "items": tc.draw(gs::integers::<u64>().max_value(5)),
    });
    let e = s.call("$/test.wait", params.clone()).err().unwrap_or_else(|| panic!("{params} waited"));
    assert_eq!(e.kind, "method_not_found", "{params}");
}

/// The result of applying `params` by a plan's id (planned with `<op>.plan` first) or straight from
/// the request, as drawn.
#[hegel::test_helper]
fn plan_or_request(tc: &TestCase, s: &Sidecar, op: &str, params: Value) -> Result<Value, RpcError> {
    if !tc.draw(gs::booleans()) {
        return s.call(&format!("{op}.apply"), params);
    }
    let plan = s.call(&format!("{op}.plan"), params)?;
    s.call(&format!("{op}.apply"), json!({ "plan_id": plan["plan_id"] }))
}

/// Recordings a player made on a drive, whatever's in them, collected by a plan or straight from
/// the request, are each copied into the folder byte for byte; with `delete` they're gone from
/// the drive, and without it they stay.
#[hegel::test(test_cases = 10)]
fn recordings_are_copied_off_the_drive_as_they_were(tc: TestCase) {
    let s = Sidecar::new();
    let a = s.demo("A");
    let made: Vec<(String, Vec<u8>)> = (0..tc.draw(gs::integers::<usize>().max_value(3)))
        .map(|i| (format!("ALPHATHETA REC/REC00{i}.WAV"), tc.draw(gs::binary().max_size(64))))
        .collect();
    for (path, bytes) in &made {
        let p = Path::new(&a).join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    let to = s.dir.path().join("Recordings");
    let delete = tc.draw(gs::booleans());
    let params = json!({ "drive": a, "to": to, "delete": delete });
    let report = plan_or_request(&tc, &s, "recordings.collect", params).unwrap();
    let collected = report["collected"].as_array().unwrap();
    assert_eq!(collected.len(), made.len(), "{report}");
    for (path, bytes) in &made {
        let c = collected.iter().find(|c| c["recording"] == path.as_str());
        let copy = c.and_then(|c| c["copied_to"].as_str()).unwrap_or_else(|| panic!("{path}: {report}"));
        assert!(Path::new(copy).starts_with(&to), "{copy}");
        assert_eq!(&std::fs::read(copy).unwrap(), bytes, "{path}");
        assert_eq!(Path::new(&a).join(path).exists(), !delete, "{path}");
    }
}

/// What of a library two drives should agree on once one is made like the other.
fn contents(drive: &str) -> Value {
    let l = library(drive);
    let tracks: Vec<Value> = l.tracks.iter().map(|t| json!([t.title, t.key, t.bpm, t.file_path])).collect();
    json!({ "tracks": tracks, "playlists": l.playlists.iter().map(|p| &p.name).collect::<Vec<_>>() })
}

/// A blank drive provisioned like A, by a plan or straight from the request, holds A's library
/// afterwards (its tracks and playlists) and is known by the name it was given.
#[hegel::test(test_cases = 4)]
fn a_drive_provisioned_like_another_holds_its_library(tc: TestCase) {
    let s = Sidecar::new();
    let a = s.demo("A");
    s.ok("edit.apply", json!({ "drive": a, "edits": [{ "op": "create_playlist", "path": "Friday" }] }));
    let target = s.dir.path().join("B");
    std::fs::create_dir(&target).unwrap();
    let name = tc.draw(gs::sampled_from(vec!["Spare", "Back Up", "Ünïcödé"]));
    let params = json!({ "target": target, "like": a, "name": name, "verify": tc.draw(gs::booleans()) });
    let report = plan_or_request(&tc, &s, "provision", params).unwrap();
    let target = target.display().to_string();
    assert_eq!(contents(&target), contents(&a), "{report}");
    let known = s.ok("drives.known", json!({}));
    let names: Vec<&str> = known.as_array().unwrap().iter().filter_map(|d| d["name"].as_str()).collect();
    assert!(names.contains(&name), "{known}");
}

/// Prep without ejecting, by a plan or straight from the request, runs the steps a plan says will
/// run, in that order, and leaves the drive saved in the store.
#[hegel::test(test_cases = 4)]
fn prep_runs_the_steps_its_plan_says(tc: TestCase) {
    let s = Sidecar::new();
    let a = s.demo("A");
    let params = json!({ "drives": [a], "eject": false, "audio": tc.draw(gs::booleans()) });
    let plan = s.ok("prep.plan", params.clone());
    let runs: Vec<&Value> =
        plan["steps"].as_array().unwrap().iter().filter(|st| st["runs"] == true).map(|st| &st["step"]).collect();
    let report = plan_or_request(&tc, &s, "prep", params).unwrap();
    let ran: Vec<&Value> = report["steps"].as_array().unwrap().iter().map(|st| &st["step"]).collect();
    assert_eq!(ran, runs, "{report}");
    let libraries = s.ok("library.list", json!({}));
    assert_eq!(libraries.as_array().map(Vec::len), Some(1), "{libraries}");
}

/// Scored against tempo annotations, rekordbox's stored tempos earn Accuracy1 for exactly the
/// tracks annotated with their own tempo, and every annotated track is counted; with no
/// annotations at all, there's nothing to score.
#[hegel::test(test_cases = 10)]
fn stored_tempos_score_for_the_tracks_annotated_with_them(tc: TestCase) {
    let s = Sidecar::new();
    let a = s.demo("A");
    let tempos = s.dir.path().join("tempo");
    std::fs::create_dir(&tempos).unwrap();
    let (mut annotated, mut right) = (0, 0);
    for t in &library(&a).tracks {
        let id = Path::new(&t.file_path).file_stem().unwrap().to_string_lossy().to_string();
        let Some(right_tempo) = tc.draw(gs::optional(gs::booleans())) else { continue };
        let bpm = if right_tempo { t.bpm } else { t.bpm * 1.5 };
        std::fs::write(tempos.join(format!("{id}.LOFI.bpm")), format!("{bpm}")).unwrap();
        annotated += 1;
        right += usize::from(right_tempo);
    }
    let got = s.call("analysis.bench", json!({ "drive": a, "tempos": tempos }));
    if annotated == 0 {
        assert_eq!(got.err().map(|e| e.message), Some("no key or tempo annotations were found".into()));
        return;
    }
    let report = got.unwrap();
    let scored = &report["tempos"]["rekordbox"];
    assert_eq!(scored["tracks"], annotated, "{report}");
    assert_eq!(scored["score"].as_f64(), Some(right as f64 / annotated as f64), "{report}");
}

/// A mix that isn't there is not found, however it's asked to be identified.
#[hegel::test(test_cases = 10)]
fn a_mix_that_is_not_there_is_not_found(tc: TestCase) {
    let s = Sidecar::new();
    let a = s.demo("A");
    let mix = s.dir.path().join(tc.draw(gs::sampled_from(vec!["mix.wav", "set.mp3", "nothing"])));
    let params = json!({
        "mix": mix,
        "drive": tc.draw(gs::optional(gs::just(a))),
        "extras": tc.draw(gs::booleans()),
        "save": tc.draw(gs::booleans()),
    });
    let e = s.call("mix.identify", params.clone()).err().unwrap_or_else(|| panic!("{params} was identified"));
    assert_eq!((e.kind, e.message), ("not_found", format!("not found: {}", mix.display())), "{params}");
}

/// A cover that isn't an image is refused before the mix is looked for.
#[test]
fn a_cover_that_isnt_an_image_is_refused() {
    let s = Sidecar::new();
    let notes = s.dir.path().join("notes.txt");
    std::fs::write(&notes, "not an image").unwrap();
    for method in ["mix.identify", "mix.export"] {
        let params = json!({ "mix": s.dir.path().join("gone.wav"), "cover": notes });
        let e = s.call(method, params).err().unwrap_or_else(|| panic!("{method} took it"));
        assert_eq!(e.kind, "invalid", "{method}: {}", e.message);
        assert!(e.message.contains("notes.txt can't be a cover"), "{method}: {}", e.message);
    }
}

/// Library files on a drive damaged in place (the same size and modification time, other
/// bytes, as a failing flash drive leaves them) are what verifying the drive finds damaged, and
/// repairing what it found puts each back as the store has it: the drive verifies clean again.
#[hegel::test(test_cases = 6)]
fn repairing_what_verify_found_puts_the_library_back(tc: TestCase) {
    let s = Sidecar::new();
    let a = s.demo("A");
    let pulled = s.ok("pull.apply", json!({ "drive": a, "library": "Main", "new_library": true }));
    // A damaged database no longer matches the snapshot of the drive, so name it.
    let check = json!({ "drive": a, "snapshot": pulled["snapshot"]["id"] });
    // Its databases and analysis files (the players' settings files aren't the library's).
    let library_files: Vec<(std::path::PathBuf, Vec<u8>)> =
        ["PIONEER/rekordbox", "PIONEER/USBANLZ"].iter().flat_map(|d| files_under(&Path::new(&a).join(d))).collect();
    let damaged = tc.draw(gs::subsequences(library_files.clone()).min_size(1));
    for (path, bytes) in &damaged {
        let at = tc.draw(gs::integers::<usize>().max_value(bytes.len() - 1));
        let mut worse = bytes.clone();
        worse[at] ^= tc.draw(gs::integers::<u8>().min_value(1));
        let mtime = filetime::FileTime::from_last_modification_time(&std::fs::metadata(path).unwrap());
        std::fs::write(path, worse).unwrap();
        filetime::set_file_mtime(path, mtime).unwrap();
    }
    let relative = |p: &Path| p.strip_prefix(&a).unwrap().to_string_lossy().replace('\\', "/");
    let found = s.ok("drive.verify", check.clone());
    let mut paths: Vec<&str> = found["damaged"].as_array().unwrap().iter().filter_map(|d| d["path"].as_str()).collect();
    paths.sort();
    let mut want: Vec<String> = damaged.iter().map(|(p, _)| relative(p)).collect();
    want.sort();
    assert_eq!(paths, want, "{found}");
    let repaired = s.ok("drive.repair", json!({ "drive": a, "problems": found["damaged"] }));
    assert_eq!(repaired["failed"], json!([]), "{repaired}");
    let again = s.ok("drive.verify", check);
    assert_eq!(again["damaged"], json!([]), "{again}");
    for (path, bytes) in &library_files {
        assert_eq!(&std::fs::read(path).unwrap(), bytes, "{}", relative(path));
    }
}
