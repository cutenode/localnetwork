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

/// A set of the store's with a recording on this computer, and a track for each of `unnamed`,
/// published without naming it when it says so.
fn a_set(env: &Env, link: Option<&str>, unnamed: &[bool]) -> PathBuf {
    let recording = env.outside("friday.wav");
    std::fs::write(&recording, b"RIFF").unwrap();
    let at = "2026-10-03T23:00:00Z";
    let mut set = json!({
        "id": "3m4n5o6p7q8r9",
        "title": "Friday",
        "date": "2026-10-03",
        "recording": { "path": recording },
        "tracks": unnamed
            .iter()
            .enumerate()
            .map(|(i, u)| json!({ "title": format!("Track {i}"), "artist": "Artist", "origin": "added", "unnamed": u }))
            .collect::<Vec<_>>(),
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
    a_set(&env, None, &[false, true]);
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

    a_set(&env, Some("https://old.example/friday"), &[false, true]);
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

/// Where a step of [`destinations_do_what_the_model_says_wherever_theyre_asked`] runs.
#[derive(Debug, Clone, Copy)]
enum Via {
    /// The command line, printing text or `--json`.
    Cli {
        json: bool,
    },
    Sidecar,
}

/// How a set is published in [`destinations_do_what_the_model_says_wherever_theyre_asked`].
#[derive(Debug, Clone, Copy, PartialEq)]
enum Publishing {
    /// Asked about, with no one to answer (or, in the sidecar, only planned).
    Asked,
    DryRun,
    Confirmed,
}

/// A step of [`destinations_do_what_the_model_says_wherever_theyre_asked`].
#[derive(Debug, Clone, Copy)]
enum Step {
    /// The store copied to `echo:folder`.
    Copy { dry_run: bool, via: Via },
    /// Brought back from it: on the command line into another store that has nothing (given
    /// what `echo:folder` holds), in the sidecar into the store itself.
    Restore { dry_run: bool, via: Via },
    /// The set published at `echo:nowhere`.
    Publish { how: Publishing, via: Via },
    /// The drive verified with `echo:folder`'s good copies, and repaired from them or not; first,
    /// once there's a copy, a track's audio file damaged on the drive and its good copy lost from
    /// the store, when `damage` says so.
    Verify { damage: bool, repair: bool, via: Via },
    /// A command given a destination it can't use.
    Wrong { command: u8, spec: u8 },
}

/// For any steps, on the command line and in the sidecar: a copy (planned or made) copies
/// every stored file and the snapshot the first time and nothing after, and a dry run leaves
/// nothing there; a restore needs a copy, brings back everything into a store that has nothing
/// and nothing after (nothing at all into the store itself); publishing a set always shows its
/// summary and replaces its link only once confirmed, with the link the destination answers; a
/// damaged file whose good copy the store lost is offered from the copy and rewritten when
/// repaired (each case ends doing so); and a destination that isn't one, or of the wrong kind, is
/// refused naming it.
#[hegel::test(test_cases = 12, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn destinations_do_what_the_model_says_wherever_theyre_asked(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (env, a) = env();
    let mut other = Env::new();
    other.plugins(&[&echo_plugin()]);
    locallibrary::Store::open_or_init(&other.store).unwrap();
    let files = objects(&env.store).len();
    let unnamed = tc.draw(gs::vecs(gs::booleans()).min_size(1).max_size(3));
    let mut linked = tc.draw(gs::booleans()).then(|| "https://old.example/friday".to_string());
    a_set(&env, linked.as_deref(), &unnamed);
    let tracks = unnamed.len();
    let audio = Env::files_in(&a.join("Contents")).into_iter().next().unwrap();
    let object = objects(&env.store).into_iter().find(|(_, bytes)| *bytes == audio.1).unwrap().0;
    let drive = path(&a).to_string();
    let mut s = Rpc::start(env.command());
    let (mut copied, mut restored, mut damaged) = (false, false, false);
    let n = tc.draw(gs::integers::<usize>().min_value(3).max_value(8));
    let mut steps: Vec<Step> = (0..n)
        .map(|i| {
            let via = match tc.draw(gs::integers::<u8>().max_value(2)) {
                0 => Via::Cli { json: false },
                1 => Via::Cli { json: true },
                _ => Via::Sidecar,
            };
            // A copy first: the rest has something to work with.
            let kind = if i == 0 { 0 } else { tc.draw(gs::integers::<u8>().max_value(5)) };
            match kind {
                0 => Step::Copy { dry_run: tc.draw(gs::booleans()), via },
                1 => Step::Restore { dry_run: tc.draw(gs::booleans()), via },
                2 => Step::Publish {
                    how: tc.draw(gs::sampled_from(vec![Publishing::Asked, Publishing::DryRun, Publishing::Confirmed])),
                    via,
                },
                3 | 4 => Step::Verify { damage: tc.draw(gs::booleans()), repair: tc.draw(gs::booleans()), via },
                _ => Step::Wrong {
                    command: tc.draw(gs::integers::<u8>().max_value(3)),
                    spec: tc.draw(gs::integers::<u8>().max_value(2)),
                },
            }
        })
        .collect();
    // Then, whatever came before, a file the store lost offered from the copy, and rewritten.
    steps.extend([
        Step::Copy { dry_run: false, via: Via::Sidecar },
        Step::Verify { damage: true, repair: false, via: Via::Cli { json: false } },
        Step::Verify { damage: true, repair: true, via: Via::Sidecar },
    ]);
    for (i, step) in steps.into_iter().enumerate() {
        let id = i as i64 * 10;
        let result = |r: Value| r.get("result").cloned().unwrap_or_else(|| panic!("step {i}: {r}"));
        match step {
            Step::Copy { dry_run, via } => {
                let (todo, snapshots, present) = if copied { (0, 0, files) } else { (files, 1, 0) };
                let r = match via {
                    Via::Cli { json } => {
                        let mut args = vec!["store", "copy", "--to", "echo:folder"];
                        if dry_run {
                            args.push("--dry-run");
                        }
                        if !json {
                            let out = env.ok(&args);
                            let said = match dry_run {
                                true => format!("; {snapshots} snapshot(s) of 1; {present} file(s) there already"),
                                false => format!(
                                    "and {snapshots} snapshot(s) to echo:folder; {present} file(s) were there already"
                                ),
                            };
                            let counted = match dry_run {
                                true => format!("  {todo} new file(s), "),
                                false => format!("copied {todo} file(s) ("),
                            };
                            assert!(out.contains(&said) && out.contains(&counted), "step {i}: {out}");
                            None
                        } else {
                            let v = env.json(&args);
                            Some(v[if dry_run { "plan" } else { "report" }].clone())
                        }
                    }
                    Via::Sidecar => Some(result(s.call(
                        id,
                        "plugins.store.copy",
                        json!({ "destination": "echo:folder", "dry_run": dry_run }),
                    ))),
                };
                if let Some(r) = r {
                    let counts = (&r["objects_copied"], &r["snapshots_copied"], &r["objects_present"]);
                    assert_eq!(counts, (&json!(todo), &json!(snapshots), &json!(present)), "step {i}: {r}");
                    assert_eq!(r["path"], "echo:folder");
                }
                copied |= !dry_run;
                assert_eq!(kept(&env.store).join("manifests/libraries.json").is_file(), copied, "step {i}");
            }
            Step::Restore { dry_run, via } => {
                if !copied {
                    let said = "not found: a copy of a store at echo:folder";
                    match via {
                        Via::Cli { .. } => {
                            assert!(other.fails(1, &["store", "restore", "--from", "echo:folder"]).contains(said))
                        }
                        Via::Sidecar => {
                            let r = s.call(id, "plugins.store.restore", json!({ "destination": "echo:folder" }));
                            assert_eq!(r["error"]["message"], said, "step {i}");
                        }
                    }
                    continue;
                }
                let (into, was) = match via {
                    Via::Cli { .. } => (&other, restored),
                    Via::Sidecar => (&env, true),
                };
                let (snapshots, brought) = if was { (0, 0) } else { (1, files) };
                let outcome = if was { "up to date" } else { "new here" };
                let r = match via {
                    Via::Cli { json } => {
                        crate::support::copy_dir(&kept(&env.store), &kept(&into.store));
                        let mut args = vec!["store", "restore", "--from", "echo:folder"];
                        if dry_run {
                            args.push("--dry-run");
                        }
                        match json {
                            true => into.json(&args),
                            false => {
                                let out = into.ok(&args);
                                let verb = if dry_run { "would copy" } else { "copied" };
                                let said = format!("{verb} {snapshots} snapshot(s) and {brought} file(s) (");
                                assert!(
                                    out.contains(&said)
                                        && out.contains(") from echo:folder; 0 file(s) were here already"),
                                    "step {i}: {out}"
                                );
                                assert!(out.contains(&format!("  A: {outcome}")), "step {i}: {out}");
                                json!({ "snapshots": snapshots, "files": brought, "dry_run": dry_run, "peer": "echo:folder" })
                            }
                        }
                    }
                    Via::Sidecar => result(s.call(
                        id,
                        "plugins.store.restore",
                        json!({ "destination": "echo:folder", "dry_run": dry_run }),
                    )),
                };
                let said = (&r["snapshots"], &r["files"], &r["dry_run"], &r["peer"]);
                assert_eq!(
                    said,
                    (&json!(snapshots), &json!(brought), &json!(dry_run), &json!("echo:folder")),
                    "step {i}"
                );
                if matches!(via, Via::Cli { .. }) {
                    restored |= !dry_run;
                    assert_eq!(names(&other.store).len(), if restored { files } else { 0 }, "step {i}");
                }
            }
            Step::Publish { how, via } => {
                let url = "https://sets.example/3m4n5o6p7q8r9".to_string();
                let summary = format!("publish Friday ({tracks} tracks) and its recording at Nowhere, saying where");
                match via {
                    Via::Cli { json } => {
                        let mut args = vec!["sets", "publish", "Friday", "--to", "echo:nowhere"];
                        match how {
                            Publishing::Asked => {}
                            Publishing::DryRun => args.push("--dry-run"),
                            Publishing::Confirmed => args.insert(0, "--yes"),
                        }
                        if json {
                            args.insert(0, "--json");
                        }
                        let r = env.run(&args);
                        let code = if how == Publishing::Asked { 3 } else { 0 };
                        assert_eq!(r.code, code, "step {i}: {}\n{}", r.out, r.err);
                        match json {
                            true => {
                                let v: Value = serde_json::from_str(&r.out).unwrap();
                                if how == Publishing::Confirmed {
                                    assert_eq!(v["report"]["url"], json!(url), "step {i}: {v}");
                                }
                            }
                            false => {
                                assert!(r.out.contains(&summary), "step {i}: {}", r.out);
                                let replaced =
                                    linked.as_ref().is_some_and(|l| r.out.contains(&format!("replace its link {l}")));
                                assert_eq!(replaced, linked.is_some(), "step {i}: {}", r.out);
                                let warned = format!("{tracks} tracks would be published");
                                assert!(r.out.contains(&warned) || r.err.contains(&warned), "step {i}: {}", r.out);
                                if how == Publishing::Confirmed {
                                    assert!(r.out.contains(&format!("published: {url}")), "step {i}: {}", r.out);
                                }
                            }
                        }
                    }
                    Via::Sidecar => {
                        let at = json!({ "destination": "echo:nowhere", "set": "Friday" });
                        let planned = result(s.call(id, "plugins.publish.plan", at));
                        assert_eq!(planned["operation"], "echo.publish.nowhere", "step {i}");
                        assert_eq!(
                            (&planned["destructive"], &planned["tracks"]),
                            (&json!(linked.is_some()), &json!(tracks)),
                            "step {i}"
                        );
                        assert_eq!(planned["replaces"], json!(linked), "step {i}");
                        assert_eq!(planned["summary"][0], json!(summary), "step {i}");
                        if how == Publishing::Confirmed {
                            let published = result(s.call(
                                id + 1,
                                "plugins.publish.apply",
                                json!({ "plan_id": planned["plan_id"] }),
                            ));
                            assert_eq!(published["url"], json!(url), "step {i}");
                        }
                    }
                }
                if how == Publishing::Confirmed {
                    linked = Some(url);
                }
                assert_eq!(link(&env), json!(linked), "step {i}");
            }
            Step::Verify { damage, repair, via } => {
                if damage && copied && !damaged {
                    std::fs::remove_file(&object).unwrap();
                    let mut bytes = audio.1.clone();
                    bytes[0] ^= 0xff;
                    std::fs::write(&audio.0, &bytes).unwrap();
                    damaged = true;
                }
                match via {
                    Via::Cli { json } => {
                        let mut args = vec!["drive", "verify", "A", "--from", "echo:folder"];
                        if repair {
                            args.extend(["--repair"]);
                            args.insert(0, "--yes");
                        }
                        if json {
                            args.insert(0, "--json");
                        }
                        let r = env.run(&args);
                        let code = if damaged && !repair { 4 } else { 0 };
                        assert_eq!(r.code, code, "step {i}: {}\n{}", r.out, r.err);
                        if !json && damaged {
                            assert!(r.out.contains("(good copy at echo:folder)"), "step {i}: {}", r.out);
                            assert_eq!(r.out.contains("rewrote 1 file(s)"), repair, "step {i}: {}", r.out);
                        }
                    }
                    Via::Sidecar => {
                        let at = json!({ "destination": "echo:folder", "drive": drive });
                        let report = result(s.call(id, "plugins.store.verify", at));
                        let problems = report["damaged"].clone();
                        let from = json!({ "from": "destination", "plugin": "echo", "destination": "folder" });
                        match damaged {
                            true => assert_eq!(problems[0]["repair"], from, "step {i}: {report}"),
                            false => assert_eq!(problems, json!([]), "step {i}: {report}"),
                        }
                        if repair {
                            let req = json!({ "destination": "echo:folder", "drive": drive, "problems": problems });
                            let repaired = result(s.call(id + 1, "plugins.store.repair", req));
                            assert_eq!(
                                repaired["repaired"].as_array().unwrap().len(),
                                usize::from(damaged),
                                "step {i}"
                            );
                        }
                    }
                }
                if repair {
                    damaged = false;
                }
                assert_eq!(std::fs::read(&audio.0).unwrap() == audio.1, !damaged, "step {i}");
                assert_eq!(object.is_file(), !damaged, "step {i}: the good copy is in the store again");
            }
            Step::Wrong { command, spec } => {
                let (spec, said) = [
                    ("echo", "\"echo\" names no destination: <plugin>:<destination>"),
                    ("echo:nope", "not found: the echo plugin's destination \"nope\""),
                    ("nope:folder", "not found: plugin \"nope\""),
                ][usize::from(spec)];
                let args: Vec<&str> = match command {
                    0 => vec!["store", "copy", "--to", spec],
                    1 => vec!["store", "restore", "--from", spec],
                    2 => vec!["drive", "verify", "A", "--from", spec],
                    _ => vec!["--yes", "sets", "publish", "Friday", "--to", spec],
                };
                assert!(env.fails(1, &args).contains(said), "step {i}: {args:?}");
                let (store, set) = match command {
                    3 => (
                        "echo:folder (A folder of its own) isn't a destination for a set",
                        vec!["--yes", "sets", "publish", "Friday", "--to", "echo:folder"],
                    ),
                    _ => (
                        "echo:nowhere (Nowhere, saying where) isn't a destination for a store",
                        vec!["store", "copy", "--to", "echo:nowhere"],
                    ),
                };
                assert!(env.fails(1, &set).contains(store), "step {i}: {set:?}");
            }
        }
    }
}
