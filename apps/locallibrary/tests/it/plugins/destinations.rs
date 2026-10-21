//! Plugins' destinations on the command line and in the sidecar, with the test plugin's (echo's):
//! `folder`, a store destination keeping what it's given in a folder where it runs, and
//! `nowhere`, a set destination publishing nowhere and saying where.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::support::{Env, Rpc, echo_plugin, path};

/// A store with the library A (the drive A, pulled), and the echo plugin.
fn env() -> (Env, PathBuf) {
    let mut env = Env::new();
    env.plugins(&[&echo_plugin()]);
    let a = env.pulled("A");
    (env, a)
}

/// Where echo's `folder` keeps the copy of the store at `store`.
fn kept(store: &Path) -> PathBuf {
    store.join("plugins/echo/work/destination")
}

/// The stored files of the store at `store`, with their contents.
fn objects(store: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    Env::files_in(&store.join("objects"))
}

/// The names of the stored files of the store at `store`, each whole (its content hashing to it).
fn names(store: &Path) -> Vec<String> {
    let whole = |(path, bytes): &(PathBuf, Vec<u8>)| {
        let name = path.strip_prefix(store.join("objects")).unwrap().to_string_lossy().replace(['/', '\\'], "");
        (locallibrary::Hash::of(bytes).to_hex() == name).then_some(name)
    };
    objects(store).iter().map(|o| whole(o).unwrap_or_default()).collect()
}

/// The store is copied to a plugin's destination (planned with --dry-run first), and a store that
/// lost everything brings it back from there: the same libraries, snapshots and stored files.
#[test]
fn the_store_is_copied_to_a_plugins_destination_and_brought_back() {
    let (env, _) = env();
    let planned = env.json(&["store", "copy", "--to", "echo:folder", "--dry-run"]);
    let files = planned["plan"]["objects_copied"].as_u64().unwrap();
    assert!(files > 0, "{planned}");
    assert!(!kept(&env.store).exists(), "a dry run copies nothing");
    let out = env.ok(&["store", "copy", "--to", "echo:folder"]);
    assert!(out.contains(&format!("copied {files} file(s)")) && out.contains("to echo:folder"), "{out}");
    let again = env.json(&["store", "copy", "--to", "echo:folder"]);
    assert_eq!((&again["report"]["objects_copied"], &again["report"]["objects_present"]), (&json!(0), &json!(files)));
    assert!(kept(&env.store).join("manifests/libraries.json").is_file());

    let mut other = Env::new();
    other.plugins(&[&echo_plugin()]);
    locallibrary::Store::open_or_init(&other.store).unwrap();
    crate::support::copy_dir(&kept(&env.store), &kept(&other.store));
    let dry = other.json(&["store", "restore", "--from", "echo:folder", "--dry-run"]);
    assert_eq!((&dry["dry_run"], &dry["files"]), (&json!(true), &json!(files)), "{dry}");
    assert!(objects(&other.store).is_empty(), "a dry run brings nothing back");
    let out = other.ok(&["store", "restore", "--from", "echo:folder"]);
    assert!(out.contains("from echo:folder") && out.contains("A: new here"), "{out}");
    assert_eq!(names(&other.store), names(&env.store));
    assert!(names(&env.store).iter().all(|n| !n.is_empty()));
    let libraries = |env: &Env| env.json(&["library", "list"]);
    assert_eq!(libraries(&other)[0]["head"], libraries(&env)[0]["head"]);
    let out = other.ok(&["store", "restore", "--from", "echo:folder"]);
    assert!(out.contains("A: up to date"), "{out}");
}

/// A destination of another kind, or none, is refused, and so is a folder and a destination at
/// once, or what a destination can't do (mirror, verify).
#[test]
fn only_a_store_destination_takes_a_copy() {
    let (env, _) = env();
    let err = env.fails(1, &["store", "copy", "--to", "echo:nowhere"]);
    assert!(err.contains("echo:nowhere (Nowhere, saying where) isn't a destination for a store"), "{err}");
    let err = env.fails(1, &["store", "copy", "--to", "echo"]);
    assert!(err.contains("\"echo\" names no destination: <plugin>:<destination>"), "{err}");
    let err = env.fails(1, &["store", "restore", "--from", "echo:folder"]);
    assert!(err.contains("not found: a copy of a store at echo:folder"), "{err}");
    let dir = env.outside("copy");
    for args in [
        &["store", "copy", path(&dir), "--to", "echo:folder"][..],
        &["store", "copy", "--to", "echo:folder", "--mirror"][..],
        &["store", "copy", "--to", "echo:folder", "--verify"][..],
        &["store", "copy"][..],
    ] {
        env.fails(2, args);
    }
}

/// A damaged file whose good copy the store lost, but its copy at a destination has, is offered
/// from there and rewritten, once it's fetched into the store.
#[test]
fn a_drive_is_repaired_from_a_plugins_copy_of_the_store() {
    let (env, a) = env();
    env.ok(&["store", "copy", "--to", "echo:folder"]);
    let audio = Env::files_in(&a.join("Contents")).into_iter().next().unwrap();
    let object = objects(&env.store).into_iter().find(|(_, bytes)| *bytes == audio.1).unwrap();
    std::fs::remove_file(&object.0).unwrap();
    let mut damaged = audio.1.clone();
    damaged[0] ^= 0xff;
    std::fs::write(&audio.0, &damaged).unwrap();
    let without = env.exits(4, &["drive", "verify", "A"]);
    assert!(!without.contains("good copy at"), "{without}");
    let with = env.exits(4, &["drive", "verify", "A", "--from", "echo:folder"]);
    assert!(with.contains("(good copy at echo:folder)"), "{with}");
    let out = env.ok(&["--yes", "drive", "verify", "A", "--from", "echo:folder", "--repair"]);
    assert!(out.contains("rewrote 1 file(s)"), "{out}");
    assert_eq!(std::fs::read(&audio.0).unwrap(), audio.1);
    assert!(object.0.is_file(), "the good copy is in the store again");
}

/// A set of the store's with a recording on this computer.
fn a_set(env: &Env, link: Option<&str>) -> PathBuf {
    let recording = env.outside("friday.wav");
    std::fs::write(&recording, b"RIFF").unwrap();
    let at = "2026-10-03T23:00:00Z";
    let mut set = json!({
        "id": "3m4n5o6p7q8r9",
        "title": "Friday",
        "date": "2026-10-03",
        "recording": { "path": recording },
        "tracks": [
            { "title": "Archangel", "artist": "Burial", "origin": "added" },
            { "title": "ID", "artist": "ID", "origin": "added", "unnamed": true },
        ],
        "created_at": at,
        "updated_at": at,
    });
    if let Some(link) = link {
        set["link"] = json!(link);
    }
    std::fs::write(env.store.join("sets.json"), json!({ "sets": [set] }).to_string()).unwrap();
    recording
}

fn link(env: &Env) -> Value {
    env.json(&["sets", "show", "Friday"])["link"].clone()
}

/// Publishing a set at a plugin's destination shows the plugin's dry run and always asks first:
/// not confirmed (or a dry run), nothing is published; confirmed, the link it answers is the set's.
#[test]
fn a_set_is_published_at_a_plugins_destination_once_confirmed() {
    let (env, _) = env();
    a_set(&env, None);
    let r = env.run(&["sets", "publish", "Friday", "--to", "echo:nowhere"]);
    assert_eq!(r.code, 3, "{}\n{}", r.out, r.err);
    assert!(r.out.contains("publish Friday (2 tracks) and its recording at Nowhere, saying where"), "{}", r.out);
    assert!(
        r.err.contains("2 tracks would be published") || r.out.contains("2 tracks would be published"),
        "{}",
        r.out
    );
    assert_eq!(link(&env), Value::Null);
    env.ok(&["sets", "publish", "Friday", "--to", "echo:nowhere", "--dry-run"]);
    assert_eq!(link(&env), Value::Null);
    let out = env.ok(&["--yes", "sets", "publish", "Friday", "--to", "echo:nowhere"]);
    assert!(out.contains("published: https://sets.example/3m4n5o6p7q8r9"), "{out}");
    assert_eq!(link(&env), json!("https://sets.example/3m4n5o6p7q8r9"));
    let err = env.fails(1, &["--yes", "sets", "publish", "Friday", "--to", "echo:folder"]);
    assert!(err.contains("isn't a destination for a set"), "{err}");
}

/// The sidecar copies the store to a destination and back, verifies and repairs a drive from it,
/// and plans and applies publishing a set, its plan saying it replaces the link the set had.
#[test]
fn the_sidecar_copies_restores_repairs_and_publishes_through_destinations() {
    let (env, a) = env();
    let mut s = Rpc::start(env.command());
    let result = |r: Value| r.get("result").cloned().unwrap_or_else(|| panic!("{r}"));
    let dry = result(s.call(1, "plugins.store.copy", json!({ "destination": "echo:folder", "dry_run": true })));
    assert!(dry["objects_copied"].as_u64().unwrap() > 0 && !kept(&env.store).exists(), "{dry}");
    let copied = result(s.call(2, "plugins.store.copy", json!({ "destination": "echo:folder" })));
    assert_eq!(copied["complete"], true, "{copied}");
    let restored = result(s.call(3, "plugins.store.restore", json!({ "destination": "echo:folder" })));
    assert_eq!((&restored["files"], &restored["peer"]), (&json!(0), &json!("echo:folder")), "{restored}");
    let audio = Env::files_in(&a.join("Contents")).into_iter().next().unwrap();
    let object = objects(&env.store).into_iter().find(|(_, bytes)| *bytes == audio.1).unwrap();
    std::fs::remove_file(&object.0).unwrap();
    let mut damaged = audio.1.clone();
    damaged[0] ^= 0xff;
    std::fs::write(&audio.0, &damaged).unwrap();
    let drive = path(&a).to_string();
    let report = result(s.call(4, "plugins.store.verify", json!({ "destination": "echo:folder", "drive": drive })));
    let problems = report["damaged"].clone();
    assert_eq!(
        problems[0]["repair"],
        json!({ "from": "destination", "plugin": "echo", "destination": "folder" }),
        "{report}"
    );
    let repaired = result(s.call(
        5,
        "plugins.store.repair",
        json!({ "destination": "echo:folder", "drive": drive, "problems": problems }),
    ));
    assert_eq!(repaired["repaired"].as_array().unwrap().len(), 1, "{repaired}");

    a_set(&env, Some("https://old.example/friday"));
    let planned = result(s.call(6, "plugins.publish.plan", json!({ "destination": "echo:nowhere", "set": "Friday" })));
    assert_eq!(
        (&planned["operation"], &planned["destructive"]),
        (&json!("echo.publish.nowhere"), &json!(true)),
        "{planned}"
    );
    assert_eq!(planned["replaces"], "https://old.example/friday");
    let published = result(s.call(7, "plugins.publish.apply", json!({ "plan_id": planned["plan_id"] })));
    assert_eq!(published["url"], "https://sets.example/3m4n5o6p7q8r9");
    assert_eq!(link(&env), json!("https://sets.example/3m4n5o6p7q8r9"));
}
