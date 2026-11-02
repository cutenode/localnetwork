//! `settings publish`, `profile publish`, `sets publish`, `settings import --from-handle` and the
//! publish RPC methods, against a pretend data server on 127.0.0.1 (standing in for Local
//! Network's directory and website too): what each shows before it
//! publishes (and how a record differs from the one published), and what it writes. The session
//! is written straight into the store (as files, never the keychain); nothing signs in to a real
//! account or touches the network.

#![cfg(feature = "account")]

use serde_json::{Value, json};

use crate::mock_pds::{MockPds, Writes};
use crate::support::account::{DID, PUBLISHING, sign_in, test_key};
use crate::support::{Env, Rpc, path, play};

const SETTINGS: &str = "computer.localnetwork.locallibrary.settings";
const SET: &str = "computer.localnetwork.dj.set";
const DELETE: &str = "com.atproto.repo.deleteRecord";

#[test]
fn settings_publish_and_import_from_the_command_line() {
    let pds = MockPds::start();
    let env = Env::new().with_directory(&pds.url);
    env.ok(&["--yes", "settings", "set", "quantize=off", "jog_mode=cdj"]);

    // Signed out, it can be looked at but not published.
    let out = env.ok(&["settings", "publish", "--dry-run"]);
    assert!(out.contains("as it would be published") && out.contains("Jog Mode"), "{out}");
    let r = env.run(&["--yes", "settings", "publish"]);
    assert!(r.code != 0 && r.err.contains("account login <handle> --publish"), "{}", r.err);

    sign_in(&env.store, &pds.url, &PUBLISHING, json!({}));
    // Without a terminal or --yes, it shows what it would publish and stops (exit status 3).
    let r = env.run(&["settings", "publish"]);
    assert_eq!(r.code, 3, "{}", r.err);
    assert!(r.out.contains("Publish your My Settings as @dj.example.com"), "{}", r.out);
    assert!(r.out.contains(&format!("at://{DID}/{SETTINGS}/self · not published yet")), "{}", r.out);
    assert!(r.err.contains("not confirmed"), "{}", r.err);
    assert!(pds.calls("com.atproto.repo.putRecord").is_empty());

    let v = env.json(&["--yes", "settings", "publish"]);
    assert_eq!(v["report"]["written"], true);
    assert_eq!((&v["applied"], &v["outcome"]), (&json!(true), &json!("applied")));
    assert_eq!(v["plan"]["operation"], "settings.publish");
    assert_eq!(v["report"]["uri"], format!("at://{DID}/{SETTINGS}/self"));
    assert_eq!(v["plan"]["record"]["player"], json!({ "quantize": "off", "jogMode": "cdj" }));
    let (record, _) = pds.record(DID, SETTINGS, "self").unwrap();
    assert_eq!(record["player"]["quantize"], "off");
    let write = pds.calls("com.atproto.repo.putRecord").pop().unwrap();
    assert_eq!(write.header("authorization"), Some("DPoP access-token"));
    assert_eq!(write.dpop().unwrap().0["jwk"]["x"], test_key()["x"]);

    let out = env.ok(&["settings", "publish", "--yes"]);
    assert!(out.contains("already published, unchanged"), "{out}");

    // Another computer imports them, signed out, naming the account.
    let other = Env::new().with_directory(&pds.url);
    let from = ["settings", "import", "--from-handle", DID];
    let r = other.run(&[&["--json"][..], &from[..]].concat());
    let v: Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!((r.code, &v["outcome"]), (3, &json!("not_confirmed")), "{}", r.out);
    assert!(v["applied"] == false && v["plan"]["changes"].as_array().unwrap().len() == 2, "{}", r.out);
    other.json(&[&from[..], &["--dry-run"][..]].concat());
    let out = other.ok(&[&["--yes"][..], &from[..]].concat());
    assert!(out.contains("imported 2 settings from did:plc:testdj"), "{out}");
    let profile = other.json(&["settings", "profile"]);
    assert_eq!(profile["base"], json!({ "jog_mode": "cdj", "quantize": "off" }));
    // Read from the directory, never from the account's data server.
    assert!(pds.calls("com.atproto.repo.getRecord").is_empty());

    // The server refuses: say what to do.
    pds.set_writes(Writes::RefuseScope);
    env.ok(&["--yes", "settings", "set", "quantize=on"]);
    let r = env.run(&["--yes", "settings", "publish"]);
    // The library says what's wrong; the CLI says what to type.
    assert!(r.code != 0 && r.err.contains("sign in again, allowing publishing"), "{}", r.err);
    let hint = "hint: Sign in again, allowing publishing: locallibrary account login --publish";
    assert!(r.err.contains(hint), "{}", r.err);

    // A session signed in without permission to publish can't sign in again without a terminal.
    pds.set_writes(Writes::Accept);
    sign_in(&env.store, &pds.url, &["atproto"], json!({}));
    let r = env.run(&["--yes", "settings", "publish"]);
    assert!(r.code != 0, "{}", r.out);
    assert!(r.err.contains("@dj.example.com's session doesn't allow publishing yet"), "{}", r.err);
    // The plan's warning is part of the plan, on stdout with it.
    assert!(r.out.contains("this session can't publish yet"), "{}", r.out);
}

#[test]
fn sets_publish_from_the_command_line() {
    let pds = MockPds::start();
    let env = Env::new().with_directory(&pds.url);
    let declare = ["sets", "new", "--title", "Friday", "--date", "2026-10-03", "--venue", "Warehouse"];
    env.ok(&[&declare[..], &["--city", "London", "--country", "gb"]].concat());
    let tracks = ["--insert", "1:Floorplan - Never Grow Old", "--insert", "1:Someone - Dubplate"];
    env.ok(&[&["sets", "edit", "Friday"][..], &tracks].concat());

    // Signed out, it can be looked at but not published.
    let out = env.ok(&["sets", "publish", "Friday", "--dry-run", "--hide", "2"]);
    assert!(out.contains("This set, as it would be published"), "{out}");
    assert!(out.contains("  1. Floorplan - Never Grow Old") && out.contains("  2. ID - ID"), "{out}");
    let r = env.run(&["--yes", "sets", "publish", "Friday"]);
    assert!(r.code != 0 && r.err.contains("account login <handle> --publish"), "{}", r.err);

    sign_in(&env.store, &pds.url, &PUBLISHING, json!({}));
    // Without a terminal or --yes, it shows what it would publish and stops (exit status 3).
    let out = env.exits(3, &["sets", "publish", "Friday"]);
    assert!(out.contains("Publish this set as @dj.example.com"), "{out}");
    assert!(pds.calls("com.atproto.repo.putRecord").is_empty());

    let v = env.json(&["--yes", "sets", "publish", "Friday"]);
    let id = env.json(&["sets", "show", "Friday"])["id"].as_str().unwrap().to_string();
    assert_eq!((&v["outcome"], &v["plan"]["operation"]), (&json!("applied"), &json!("sets.publish")));
    assert_eq!(v["report"]["uri"], format!("at://{DID}/computer.localnetwork.dj.set/{id}"));
    let (record, _) = pds.record(DID, "computer.localnetwork.dj.set", &id).unwrap();
    assert_eq!((&record["venue"], &record["location"]["country"]), (&json!("Warehouse"), &json!("GB")));
    assert_eq!(record["tracks"][1]["title"], "Dubplate");

    let out = env.ok(&["sets", "publish", "Friday", "--yes"]);
    assert!(out.contains("this set already published, unchanged"), "{out}");
}

/// `sets publish SET --upload`, with a stand-in ffmpeg on `PATH`: a dry run shows how the recording
/// goes up and sends nothing; without a terminal or --yes nothing goes; with --yes one answer
/// covers both, so a WAV is encoded as AAC with its timed track as a chapter (not named with
/// --hide), uploaded as audio/mp4 and published, and the set's record points at it (its envelope
/// nested as `record`). Without ffmpeg, it says to install it.
#[cfg(unix)]
#[test]
fn sets_publish_upload_from_the_command_line() {
    let pds = MockPds::start();
    pds.set_account(DID, "dj.example.com");
    let env = Env::new().with_directory(&pds.url);
    let recording = env.outside("friday.wav");
    std::fs::write(&recording, loclib_mixid::test_util::wav(8000, 16, 2.0)).unwrap();
    let (id, at) = ("3m4n5o6p7q8r9", "2026-10-03T23:00:00Z");
    let set = json!({
        "id": id, "title": "Friday", "date": "2026-10-03", "recording": { "path": recording },
        "tracks": [{ "title": "Glue", "artist": "Bicep", "origin": "added", "start_seconds": 0 }],
        "created_at": at, "updated_at": at,
    });
    sign_in(&env.store, &pds.url, &PUBLISHING, json!({}));
    std::fs::write(env.store.join("sets.json"), json!({ "sets": [set] }).to_string()).unwrap();
    let (bin, nothing) = (env.outside("bin"), env.outside("empty"));
    for dir in [&bin, &nothing] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let args = env.outside("ffmpeg-args");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >'{}'\nfor last; do :; done\nprintf 'encoded audio' >\"$last\"\n\
         printf 'total_size=13\\nprogress=end\\n'\n",
        args.display()
    );
    std::fs::write(bin.join("ffmpeg"), script).unwrap();
    crate::support::set_mode(&bin.join("ffmpeg"), 0o755);
    // With `typed`, on a terminal it was typed ahead on.
    let run = |path: &std::path::Path, global: &[&str], more: &[&str], typed: Option<&str>| {
        let publish = ["sets", "publish", "Friday", "--upload"];
        let mut command = env.command();
        command.env("PATH", path).args(global).args(publish).args(more);
        // The keyboard stays open until the command's done, as a terminal would.
        let _keyboard = typed.map(|typed| {
            let (mut keyboard, stdin) = crate::support::terminal();
            std::io::Write::write_all(&mut keyboard, typed.as_bytes()).unwrap();
            command.stdin(std::process::Stdio::from(stdin));
            keyboard
        });
        let out = command.output().unwrap();
        (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
    };

    let (code, out, err) = run(&bin, &[], &["--dry-run"], None);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains("Upload this set's recording as @dj.example.com"), "{out}");
    assert!(out.contains("as AAC at 128 kbit/s in an M4A file") && out.contains("then this set's record"), "{out}");
    assert!(out.contains("Chapters  1 track\n"), "{out}");
    // Hidden, the chapter doesn't name it either.
    let (code, out, err) = run(&bin, &["--json"], &["--dry-run", "--hide", "1"], None);
    assert_eq!(code, 0, "{out}\n{err}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["plan"]["chapters"], json!([{ "start": 0, "title": "ID - ID" }]), "{out}");
    let (code, _, err) = run(&bin, &[], &[], None);
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("not confirmed"), "{err}");
    assert!(pds.calls("com.atproto.repo.uploadBlob").is_empty() && !args.exists(), "nothing goes unasked");

    // One answer covers the upload and the set's record.
    let (code, out, err) = run(&bin, &[], &[], Some("y\n"));
    assert_eq!(code, 0, "{out}\n{err}");
    assert_eq!(err.matches("[y/N]").count(), 1, "{err}");
    assert!(err.contains("Upload the recording and publish this set as @dj.example.com?"), "{err}");
    assert!(out.contains("uploaded the recording") && out.contains("published this set"), "{out}");
    let uri = format!("at://{DID}/computer.localnetwork.dj.recording/{id}");
    let ffmpeg: Vec<String> = std::fs::read_to_string(&args).unwrap().lines().map(String::from).collect();
    assert!(
        ffmpeg.windows(2).any(|w| w == ["-b:a", "128k"]) && ffmpeg.windows(2).any(|w| w == ["-movflags", "+faststart"])
    );
    assert!(
        ffmpeg.windows(3).any(|w| w[..2] == ["ffmetadata", "-i"])
            && ffmpeg.windows(2).any(|w| w == ["-map_chapters", "1"])
    );
    let (recorded, cid) = pds.record(DID, "computer.localnetwork.dj.recording", id).unwrap();
    let blob = pds.blob(recorded["audio"]["ref"]["$link"].as_str().unwrap()).unwrap();
    assert_eq!(blob, ("audio/mp4".to_string(), b"encoded audio".to_vec()));
    assert_eq!(recorded["durationSeconds"], 2);
    let (set_record, _) = pds.record(DID, "computer.localnetwork.dj.set", id).unwrap();
    assert_eq!(set_record["recording"], json!({ "uri": uri, "cid": cid }));

    // Again, as JSON: the same recording, so neither record changes, and the set's is nested.
    let (code, out, err) = run(&bin, &["--json", "--yes"], &[], None);
    assert_eq!(code, 0, "{out}\n{err}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        (&v["outcome"], &v["plan"]["operation"], &v["report"]["uri"], &v["report"]["written"]),
        (&json!("applied"), &json!("sets.upload"), &json!(uri), &json!(false))
    );
    assert_eq!(v["plan"]["encoding"], json!({ "kind": "aac", "kbps": 128, "bytes": 32000 }));
    assert_eq!(v["record"]["outcome"], "nothing_to_do");
    assert_eq!(v["record"]["plan"]["record"]["recording"], set_record["recording"]);

    std::fs::remove_file(&args).unwrap();
    let (code, _, err) = run(&nothing, &["--yes"], &[], None);
    assert_ne!(code, 0);
    assert!(err.contains("ffmpeg, which encodes the recording (WAV)") && err.contains("https://ffmpeg.org"), "{err}");
}

/// `sets publish` first runs the analyzer chosen for the MusicBrainz roles on the tracks it names
/// that have no IDs, keeps what it finds in the set and publishes it as bare UUIDs: a track it
/// fails on is a warning, tracks whose drive isn't plugged in are said once, and `--no-lookup`,
/// `--dry-run` and an unconfirmed `--json` run nothing. The sidecar's `sets.lookup` counts the
/// same, asking only about the tracks still without IDs.
#[cfg(debug_assertions)]
#[test]
fn sets_publish_looks_up_musicbrainz_ids_first() {
    const RECORDING: &str = "8dfae998-efd4-4ebe-a482-1523ff687362";
    let pds = MockPds::start();
    let mut env = Env::new().with_directory(&pds.url);
    let ids = "\n[[contributes.analyzers]]\nname = \"heard\"\ntitle = \"Heard\"\nversion = \"1\"\n\
               fields = { recording = \"string\", release = \"string\", artists = \"string[]\", \
               isrc = \"string\", credits = \"string[]\" }\n\
               roles = { recording = \"recording\", release = \"release\", artists = \"artists\", \
               isrc = \"isrc\", credits = \"credits\" }\n";
    env.plugins(&[&(crate::support::echo_plugin() + ids)]);
    // Over the musicbrainz plugin's, which would go online.
    for role in ["recording", "release", "artists", "isrc", "credits"] {
        env.ok(&["--yes", "plugin", "roles", role, "echo.heard"]);
    }
    let root = env.demo("USB");
    let tracks = locallibrary::Device::open(&root).unwrap().library().unwrap().tracks;
    env.ok(&["sets", "new", "--title", "Friday", "--date", "2026-10-03"]);
    let insert = |t: &locallibrary::Track| format!("1:{}", t.file_path);
    let (first, second) = (insert(&tracks[1]), insert(&tracks[0]));
    env.ok(&["sets", "edit", "Friday", "--drive", path(&root), "--insert", &first, "--insert", &second]);
    let store = locallibrary::Store::open(&env.store).unwrap();
    let mut book = locallibrary::sets::SetBook::load(&store).unwrap();
    let gone = locallibrary::sets::SetEntry::added("Gone - Away");
    book.sets[0].tracks.push(locallibrary::sets::SetEntry { file: Some("/Contents/gone.mp3".into()), ..gone });
    book.save(&store).unwrap();
    let id = env.json(&["sets", "show", "Friday"])["id"].as_str().unwrap().to_string();
    // What the test plugin's `heard` answers: what's in the audio file (and failing, without JSON).
    let credits = locallibrary::model::artist_credits(&tracks[0].artist);
    let artists: Vec<String> = (0..credits.len()).map(|i| format!("e0ddb3c4-0a5c-44d5-9c38-04d9a2e4e8a{i}")).collect();
    let audio = |t: &locallibrary::Track| locallibrary::paths::to_fs(&root, &locallibrary::paths::key(&t.file_path));
    let isrc = locallibrary::model::isrc(&tracks[0].isrc);
    let heard = json!({
        "recording": RECORDING.to_uppercase(), "release": null, "artists": artists, "isrc": isrc, "credits": credits,
    });
    std::fs::write(audio(&tracks[0]), heard.to_string()).unwrap();
    std::fs::write(audio(&tracks[1]), "not json").unwrap();
    let published = || pds.record(DID, "computer.localnetwork.dj.set", &id).unwrap().0["tracks"][1].clone();

    // Nothing is looked up for a set that can't be published: signed out, only shown, or not
    // confirmed (no terminal to ask in, or --json without --yes).
    let r = env.run(&["--yes", "sets", "publish", "Friday"]);
    assert!(r.code != 0 && r.err.contains("account login <handle> --publish"), "{}", r.err);
    sign_in(&env.store, &pds.url, &PUBLISHING, json!({}));
    let out = env.ok(&["sets", "publish", "Friday", "--dry-run"]);
    assert!(!out.contains("MusicBrainz"), "{out}");
    env.exits(3, &["sets", "publish", "Friday"]);
    env.exits(3, &["--json", "sets", "publish", "Friday"]);
    env.ok(&["--yes", "sets", "publish", "Friday", "--no-lookup"]);
    assert!(published().get("recordingMbId").is_none(), "{}", published());
    assert!(!env.store.join("cache/fields/echo").exists(), "nothing analysed");

    let r = env.run(&["--yes", "sets", "publish", "Friday"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.contains("found MusicBrainz IDs for 1 track\n"), "{}", r.out);
    assert!(r.out.contains("plug in the drive to look up MusicBrainz IDs for 1 track\n"), "{}", r.out);
    assert!(r.err.contains(&tracks[1].file_path), "the track it failed on: {}", r.err);
    let track = published();
    assert_eq!((&track["recordingMbId"], track.get("releaseMbId")), (&json!(RECORDING), None), "{track}");
    let paired: Vec<Value> =
        track["artists"].as_array().into_iter().flatten().map(|a| a["artistMbId"].clone()).collect();
    assert_eq!(paired, artists.iter().map(|a| json!(a)).collect::<Vec<_>>());
    assert_eq!(env.json(&["sets", "show", "Friday"])["tracks"][1]["musicbrainz"]["recording"], RECORDING);
    // With --json, what the lookup did is among the plan's notes.
    let v = env.json(&["--yes", "sets", "publish", "Friday"]);
    assert_eq!(v["plan"]["notes"], json!(["plug in the drive to look up MusicBrainz IDs for 1 track"]), "{v}");

    let mut rpc = Rpc::start(env.command());
    let looked = rpc.call(1, "sets.lookup", json!({ "set": "Friday" }));
    assert_eq!(looked["result"], json!({ "asked": 1, "found": 0, "failed": 1, "unmounted": 1 }), "{looked}");
}

#[test]
fn an_unchanged_record_needs_no_permission_to_leave_as_it_is() {
    let pds = MockPds::start();
    let env = Env::new().with_directory(&pds.url);
    env.ok(&["--yes", "settings", "set", "quantize=off"]);
    sign_in(&env.store, &pds.url, &PUBLISHING, json!({}));
    env.ok(&["--yes", "settings", "publish"]);
    // Signed in again, only to say who you are: there's nothing to publish, so nothing to allow.
    sign_in(&env.store, &pds.url, &["atproto"], json!({}));
    let out = env.ok(&["account"]);
    assert!(out.contains("signed in as the development client"), "{out}");
    let out = env.ok(&["--yes", "settings", "publish"]);
    assert!(out.contains("already published, unchanged"), "{out}");
    // Signing in again for publishing takes the sign-in's browser options.
    for flag in ["--no-browser", "--paste", "--timeout=60"] {
        env.ok(&["settings", "publish", "--dry-run", flag]);
    }
}

#[test]
fn rpc_publish_methods() {
    let pds = MockPds::start();
    let env = Env::new().with_directory(&pds.url);
    env.ok(&["--yes", "settings", "set", "quantize=off"]);
    sign_in(&env.store, &pds.url, &["atproto"], json!({}));
    let mut rpc = Rpc::start(env.command());
    let version = rpc.call(1, "version", Value::Null);
    let methods = version["result"]["methods"].as_array().unwrap();
    for m in [
        "settings.publish.plan",
        "settings.publish.apply",
        "settings.import_published.plan",
        "settings.import_published.apply",
        "profile.publish.plan",
        "profile.publish.apply",
        "sets.lookup",
        "sets.publish.plan",
        "sets.publish.apply",
    ] {
        assert!(methods.contains(&Value::from(m)), "{m}");
    }
    let plan = &rpc.call(2, "settings.publish.plan", Value::Null)["result"];
    assert_eq!(plan["missing_scope"], format!("repo:{SETTINGS}?action=create&action=update"));
    assert_eq!(plan["record"]["player"]["quantize"], "off");
    assert_eq!(rpc.call(3, "settings.publish.apply", Value::Null)["error"]["data"]["kind"], "account");
    // No sets yet.
    assert_eq!(rpc.call(4, "sets.publish.plan", json!({ "set": "last" }))["error"]["data"]["kind"], "not_found");
    // Nothing published yet.
    assert_eq!(rpc.call(5, "settings.import_published.plan", Value::Null)["error"]["data"]["kind"], "not_found");
}

#[test]
fn a_publish_plan_is_applied_by_its_id_and_only_over_what_it_read() {
    let pds = MockPds::start();
    let env = Env::new().with_directory(&pds.url);
    env.ok(&["--yes", "settings", "set", "quantize=off"]);
    sign_in(&env.store, &pds.url, &PUBLISHING, json!({}));
    let mut s = Rpc::start(env.command());
    let plan = s.call(1, "settings.publish.plan", json!({}))["result"].clone();
    assert_eq!(
        (&plan["operation"], &plan["noop"], &plan["destructive"]),
        (&json!("settings.publish"), &json!(false), &json!(false))
    );
    assert!(plan["summary"][0].as_str().unwrap().contains("publish your My Settings as @dj.example.com"), "{plan}");
    let id = plan["plan_id"].clone();
    let done = s.call(2, "settings.publish.apply", json!({ "plan_id": id }));
    assert_eq!(done["result"]["written"], true, "{done}");
    assert_eq!(pds.record(DID, SETTINGS, "self").unwrap().0["player"]["quantize"], "off");
    let again = s.call(3, "settings.publish.apply", json!({ "plan_id": id }));
    assert_eq!(again["error"]["data"]["kind"], "stale_plan", "{again}");

    // Published from another computer after the plan read it: refused, and nothing is replaced.
    env.ok(&["--yes", "settings", "set", "quantize=on"]);
    let plan = s.call(4, "settings.publish.plan", json!({}))["result"].clone();
    assert_eq!(plan["destructive"], true, "it replaces what's published: {plan}");
    let theirs = json!({ "$type": SETTINGS, "updatedAt": "2026-10-05T00:00:00.000Z" });
    pds.put(DID, SETTINGS, "self", theirs.clone());
    let refused = s.call(5, "settings.publish.apply", json!({ "plan_id": plan["plan_id"] }));
    assert_eq!(refused["error"]["data"]["kind"], "stale_plan", "{refused}");
    assert_eq!(pds.record(DID, SETTINGS, "self").unwrap().0, theirs);

    // The settings profile changed since: refused before anything is sent.
    let plan = s.call(6, "settings.publish.plan", json!({}))["result"].clone();
    env.ok(&["--yes", "settings", "set", "quantize=off"]);
    let writes = pds.calls("com.atproto.repo.putRecord").len();
    let refused = s.call(7, "settings.publish.apply", json!({ "plan_id": plan["plan_id"] }));
    assert_eq!(refused["error"]["data"]["kind"], "stale_plan", "{refused}");
    assert_eq!(pds.calls("com.atproto.repo.putRecord").len(), writes);

    // Importing applies what it showed, and not over a profile changed since.
    let import = s.call(8, "settings.import_published.plan", json!({}))["result"].clone();
    assert_eq!(import["operation"], "settings.import_published", "{import}");
    env.ok(&["--yes", "settings", "set", "jog_mode=cdj"]);
    let refused = s.call(9, "settings.import_published.apply", json!({ "plan_id": import["plan_id"] }));
    assert_eq!(refused["error"]["data"]["kind"], "stale_plan", "{refused}");
    // Theirs has no values: adding them changes nothing, replacing the base with them empties it.
    let import = s.call(10, "settings.import_published.plan", json!({}))["result"].clone();
    assert_eq!(import["noop"], true, "{import}");
    let import = s.call(11, "settings.import_published.plan", json!({ "replace": true }))["result"].clone();
    let done = s.call(12, "settings.import_published.apply", json!({ "plan_id": import["plan_id"] }));
    assert!(done["result"]["base"].as_object().is_some_and(|b| b.is_empty()), "theirs has no values: {done}");
}

#[test]
fn settings_are_shown_against_what_was_published_and_imported_elsewhere() {
    let pds = MockPds::start();
    let env = Env::ascii().with_directory(&pds.url);
    env.ok(&["--yes", "settings", "set", "quantize=off", "waveform_color=rgb", "crossfader_curve=slow-cut"]);
    sign_in(&env.store, &pds.url, &PUBLISHING, json!({}));
    // --json doesn't ask: without --yes, it's not confirmed.
    env.exits(3, &["--json", "settings", "publish"]);

    // Published from another app: no date, a value of its own, and one locallibrary doesn't know.
    let player = json!({ "quantize": "on", "madeUp": "x" });
    pds.put(DID, SETTINGS, "self", json!({ "$type": SETTINGS, "player": player }));
    let out = env.ok(&["settings", "publish", "--dry-run"]);
    assert!(out.contains(&format!("at://{DID}/{SETTINGS}/self - replaces what's published there")), "{out}");
    for line in [
        "  Player\n    Quantize                           Off (was On)\n",
        "    made_up                            no longer published (was x)\n",
        "  Display\n    Waveform Color                     RGB (new)\n",
        "  Mixer\n    Crossfader Curve                   Slow Cut (new)\n",
    ] {
        assert!(out.contains(line), "{line:?} in\n{out}");
    }
    env.ok(&["--yes", "settings", "publish"]);

    // Another computer imports them: first adding to its profile, then replacing its base.
    let other = Env::ascii().with_directory(&pds.url);
    other.ok(&["--yes", "settings", "set", "jog_mode=cdj"]);
    let from = ["settings", "import", "--from-handle", DID];
    let out = other.ok(&[&from[..], &["--dry-run"]].concat());
    assert!(out.starts_with(&format!("My Settings published by {DID} (")), "{out}");
    assert!(out.contains("    Quantize                           (not set) -> Off\n"), "{out}");
    assert!(out.contains("Importing adds these to your settings profile's base (--replace replaces it)"), "{out}");
    let out = other.ok(&[&from[..], &["--replace", "--dry-run"]].concat());
    assert!(out.contains("    Jog Mode                           CDJ -> (not set)\n"), "{out}");
    assert!(out.contains("Importing replaces your settings profile's base; per-drive overrides stay."), "{out}");
    let out = other.ok(&[&["--yes"], &from[..], &["--replace"]].concat());
    assert!(out.contains(&format!("imported 3 settings from {DID}")), "{out}");
    let out = other.ok(&[&["--yes"], &from[..]].concat());
    assert!(out.contains("  the same as your settings profile\n"), "{out}");
}

#[test]
fn a_profile_is_shown_against_what_was_published() {
    let pds = MockPds::start();
    let env = Env::ascii().with_directory(&pds.url);
    let club = env.club("Club");
    play(&club, "HISTORY 001", &[1, 2, 3, 4, 5, 6]);
    play(&club, "HISTORY 002", &[9, 10, 1, 2, 12, 7]);
    let d = path(&club);
    let out = env.ok(&["profile", "publish", d, "--dry-run"]);
    assert!(out.starts_with("Your profile, as it would be published\n"), "{out}");
    assert!(out.contains("  Tempo             "), "{out}");
    assert!(out.contains("  rounded numbers - `locallibrary profile sections` shows what each section reveals"));

    sign_in(&env.store, &pds.url, &PUBLISHING, json!({}));
    env.ok(&["--yes", "profile", "publish", d]);
    env.ok(&["profile", "headline", "Techno warm-ups"]);
    env.ok(&["profile", "exclude", "genres"]);
    env.ok(&["profile", "include", "signature"]);
    let out = env.ok(&["profile", "publish", d, "--dry-run"]);
    assert!(out.contains("  Headline          Techno warm-ups (changed)\n"), "{out}");
    assert!(out.contains("  Genres            no longer published\n"), "{out}");
    assert!(out.lines().any(|l| l.starts_with("  Signature tracks  ") && l.ends_with(" (new)")), "{out}");
}

#[test]
fn a_set_is_shown_against_what_was_published() {
    let pds = MockPds::start();
    let env = Env::ascii().with_directory(&pds.url);
    let club = env.club("Club");
    env.ok(&["sets", "new", "--title", "Friday", "--date", "2026-10-03", "--venue", "Warehouse"]);
    env.ok(&["sets", "edit", "Friday", "--insert", "1:Deluxe Cynic", "--with", "did:plc:abc", "--drive", path(&club)]);
    sign_in(&env.store, &pds.url, &PUBLISHING, json!({}));
    let out = env.ok(&["--yes", "sets", "publish", "Friday"]);
    assert!(out.contains("  With    did:plc:abc (did:plc:abc)\n"), "{out}");
    assert!(out.contains("    1. [a]pendics.shuffle - Deluxe Cynic  128.00 BPM - 5A\n"), "{out}");

    env.ok(&["sets", "edit", "Friday", "--venue", "", "--insert", "2:Someone - Dubplate"]);
    let out = env.ok(&["sets", "publish", "Friday", "--dry-run"]);
    assert!(out.contains("  Venue   no longer published\n"), "{out}");
    assert!(out.contains("  2 tracks (changed)\n"), "{out}");
}

/// Every step of signing in, publishing and unpublishing that doesn't need a real account, one
/// after another as a DJ would take them: the session is written in place of `account login`'s
/// browser round trip, and the pretend server stands in for the account's PDS, Local Network's
/// directory and its website.
#[test]
fn sign_in_publish_and_unpublish_end_to_end() {
    let pds = MockPds::start();
    pds.set_account(DID, "dj.example.com");
    let env = Env::from_template().with_directory(&pds.url);
    sign_in(&env.store, &pds.url, &PUBLISHING, json!({}));

    let out = env.ok(&["account", "status", "--check"]);
    assert!(out.contains(&format!("{} answered for this session", pds.url)), "{out}");
    let out = env.ok(&["account"]);
    assert!(out.contains("can publish your My Settings, profile, sets and their recordings;"), "{out}");

    env.ok(&["--yes", "settings", "set", "quantize=off"]);
    env.ok(&["--yes", "settings", "publish"]);
    assert_eq!(pds.record(DID, SETTINGS, "self").unwrap().0["player"]["quantize"], "off");
    env.ok(&["--yes", "profile", "publish", "A", "--no-analysis"]);
    assert!(pds.record(DID, "computer.localnetwork.dj.profile", "self").is_some());
    env.ok(&["--yes", "sets", "new", "--from", "HISTORY 001", "--title", "Friday", "--date", "2026-10-03"]);
    env.ok(&["--yes", "sets", "publish", "Friday"]);
    let id = env.json(&["sets", "show", "Friday"])["id"].as_str().unwrap().to_string();
    let uri = format!("at://{DID}/{SET}/{id}");
    assert!(pds.record(DID, SET, &id).is_some());

    // Another computer, signed out, imports the My Settings published.
    let other = Env::new().with_directory(&pds.url);
    let out = other.ok(&["--yes", "settings", "import", "--from-handle", DID]);
    assert!(out.contains(&format!("imported 1 setting from {DID}")), "{out}");

    // Deleting the set says it takes the record off the account, and does.
    let out = env.exits(3, &["sets", "delete", "Friday"]);
    assert!(out.contains(&format!("and its record on your Atmosphere account ({uri})")), "{out}");
    assert!(pds.record(DID, SET, &id).is_some(), "not confirmed: still there");
    let out = env.ok(&["--yes", "sets", "delete", "Friday"]);
    assert!(out.contains("deleted"), "{out}");
    assert!(pds.record(DID, SET, &id).is_none() && pds.indexed(DID, SET, &id).is_none());
    assert_eq!(pds.calls(DELETE).len(), 2, "told to use the server's nonce once");
    assert_eq!(env.json(&["sets"]).as_array().map(Vec::len), Some(0));
    // The rest stays.
    assert!(pds.record(DID, SETTINGS, "self").is_some());

    let out = env.ok(&["account", "logout"]);
    assert!(out.starts_with("✓ Signed out of @dj.example.com\n"), "{out}");
    assert!(env.ok(&["account"]).contains("`locallibrary account login dj.example.com` signs in"));
    assert_eq!(env.run(&["account", "status", "--check"]).code, 1);
}
