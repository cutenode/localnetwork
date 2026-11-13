//! Property tests of publishing beside `props_cli_account`'s: My Settings planned against what's
//! published, publishing signed out or late in a session, `sets publish --upload`, and `settings
//! import --from-handle`, against a pretend data server on 127.0.0.1 (signed in as a sign-in with
//! `--session-file` leaves it, never the keychain).

#![cfg(feature = "account")]

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use indexmap::IndexMap;
use serde_json::{Value, json};

use crate::mock_pds::MockPds;
use crate::support::Env;
use crate::support::account::{DID, PUBLISHING, sign_in};

const SETTINGS: &str = "computer.localnetwork.locallibrary.settings";

/// A setting: (key, the record's field, group, label, the values it takes with their labels).
type Setting = (&'static str, &'static str, &'static str, &'static str, &'static [(&'static str, &'static str)]);

/// Settings of each group a My Settings record has.
const POOL: [Setting; 4] = [
    ("quantize", "quantize", "player", "Quantize", &[("off", "Off"), ("on", "On")]),
    ("jog_mode", "jogMode", "player", "Jog Mode", &[("cdj", "CDJ"), ("vinyl", "Vinyl")]),
    ("lcd_brightness", "lcdBrightness", "display", "LCD Brightness", &[("1", "1"), ("3", "3"), ("5", "5")]),
    (
        "channel_fader_curve",
        "channelFaderCurve",
        "mixer",
        "Channel Fader Curve",
        &[("linear", "Linear"), ("steep-top", "Steep Top")],
    ),
];

/// A value drawn for each setting of [`POOL`], or none: (key, value id) for those drawn.
fn draw_settings(tc: &TestCase) -> IndexMap<&'static str, &'static str> {
    POOL.iter()
        .filter_map(|(key, _, _, _, choices)| {
            let i = tc.draw(gs::integers::<usize>().max_value(choices.len()));
            choices.get(i).map(|(id, _)| (*key, *id))
        })
        .collect()
}

/// The label a value of `key` shows as.
fn shown(key: &str, id: &str) -> &'static str {
    let (_, _, _, _, choices) = POOL.iter().find(|(k, ..)| *k == key).unwrap();
    choices.iter().find(|(i, _)| *i == id).unwrap().1
}

/// The settings profile's base is exactly `values`.
fn set_base(fx: &Env, values: &IndexMap<&str, &str>) {
    for (key, ..) in POOL {
        fx.run(&["--yes", "settings", "unset", key]);
    }
    let pairs: Vec<String> = values.iter().map(|(k, v)| format!("{k}={v}")).collect();
    if !pairs.is_empty() {
        fx.ok(&[&["--yes", "settings", "set"][..], &pairs.iter().map(String::as_str).collect::<Vec<_>>()].concat());
    }
}

/// A My Settings record holding `values`, and `unknown` fields this version doesn't know.
fn settings_record(values: &IndexMap<&str, &str>, unknown: usize) -> Value {
    let mut r = json!({ "$type": SETTINGS, "updatedAt": "2026-09-01T00:00:00Z" });
    for (key, id) in values {
        let (_, field, group, ..) = POOL.iter().find(|(k, ..)| k == key).unwrap();
        r[*group][*field] = json!(id);
    }
    for i in 0..unknown {
        r["player"][format!("somethingNewer{i}")] = json!("x");
    }
    r
}

/// Whatever My Settings the account published before (or none) and whatever the settings
/// profile holds now, the printed plan has a heading for each group with a setting in either, a
/// line per setting it publishes with its value's label, marked new or with the value it
/// replaces where there's a record to replace, and a line for each setting published before that
/// it no longer publishes.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn a_settings_plan_marks_each_setting_against_the_published_one(tc: TestCase) {
    let pds = MockPds::start();
    let fx = Env::new().with_directory(&pds.url);
    sign_in(&fx.store, &pds.url, &PUBLISHING, json!({}));
    let mut local = draw_settings(&tc);
    if local.is_empty() {
        local.insert("quantize", "on");
    }
    set_base(&fx, &local);
    let published = tc.draw(gs::booleans()).then(|| draw_settings(&tc));
    if let Some(p) = &published {
        pds.put(DID, SETTINGS, "self", settings_record(p, 0));
    }

    let words = ["settings", "publish", "--dry-run"];
    let v = fx.json(&words);
    let out = fx.ok(&words);
    let said = format!("local {local:?}, published {published:?}\n{out}\n{v}");
    assert!(out.starts_with("Publish your My Settings as @dj.example.com\n"), "{said}");
    assert_eq!(v["plan"]["current"].is_null(), published.is_none(), "{said}");
    for (key, field, group, label, _) in POOL {
        let line = out.lines().find(|l| l.starts_with(&format!("    {label:<34} ")));
        let was = published.as_ref().and_then(|p| p.get(key));
        match (local.get(key), was) {
            (Some(id), _) => {
                assert_eq!(v["plan"]["record"][group][field], *id, "{key}: {said}");
                let note = match (&published, was) {
                    (None, _) => String::new(),
                    (Some(_), None) => " (new)".into(),
                    (Some(_), Some(w)) if w != id => format!(" (was {})", shown(key, w)),
                    _ => String::new(),
                };
                assert_eq!(line, Some(format!("    {label:<34} {}{note}", shown(key, id)).as_str()), "{key}: {said}");
            }
            (None, Some(w)) => {
                let gone = format!("    {label:<34} no longer published (was {})", shown(key, w));
                assert_eq!(line, Some(gone.as_str()), "{key}: {said}");
            }
            (None, None) => assert!(line.is_none(), "{key}: {said}"),
        }
    }
    for (group, title) in [("player", "Player"), ("display", "Display"), ("mixer", "Mixer")] {
        let any = |s: &IndexMap<&str, &str>| s.keys().any(|k| POOL.iter().any(|(pk, _, g, ..)| pk == k && *g == group));
        let headed = any(&local) || published.as_ref().is_some_and(any);
        assert_eq!(out.contains(&format!("\n  {title}\n")), headed, "{group}: {said}");
    }
}

/// Signed out, My Settings, the profile and a set (its tracks typed in with an artist or
/// without) can each be looked at, as they would be published, saying how to sign in, and as JSON
/// with no account or address; asked to publish them, it says to sign in, and nothing is written.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn signed_out_a_plan_is_only_shown(tc: TestCase) {
    let pds = MockPds::start();
    let fx = Env::from_template().with_directory(&pds.url);
    set_base(&fx, &draw_settings(&tc));
    fx.ok(&["--yes", "settings", "set", "lcd_brightness=2"]);
    fx.ok(&["--yes", "sets", "new", "--title", "Friday", "--date", "2026-10-03"]);
    let typed: Vec<&str> =
        tc.draw(gs::vecs(gs::sampled_from(&["Dubplate", "Someone - Dubplate", "Bootleg"][..])).min_size(1).max_size(3));
    for t in &typed {
        fx.ok(&["--yes", "sets", "edit", "Friday", "--insert", &format!("1:{t}")]);
    }
    let kinds: [(&[&str], &str); 3] = [
        (&["settings", "publish"], "My Settings"),
        (&["profile", "publish", "A", "--no-analysis"], "Your profile"),
        (&["sets", "publish", "Friday"], "This set"),
    ];
    for i in tc.draw(gs::permutations(vec![0, 1, 2])) {
        let (words, what) = kinds[i];
        let v = fx.json(&[words, &["--dry-run"][..]].concat());
        let out = fx.ok(&[words, &["--dry-run"][..]].concat());
        let said = format!("{words:?}\n{out}\n{v}");
        let what = if what == "My Settings" { "Your My Settings" } else { what };
        assert!(out.starts_with(&format!("{what}, as it would be published\n")), "{said}");
        assert!(out.contains("not signed in: `locallibrary account login <handle> --publish` signs in"), "{said}");
        assert!(v["plan"]["account"].is_null() && v["plan"]["uri"].is_null(), "{said}");
        if i == 2 {
            // Each typed-in track as it was typed: "Artist - Title", or the title alone.
            let tracks = v["plan"]["record"]["tracks"].as_array().unwrap();
            assert_eq!(tracks.len(), typed.len(), "{said}");
            for (n, t) in tracks.iter().enumerate() {
                let line = out.lines().find(|l| l.starts_with(&format!("  {:>3}. ", n + 1))).unwrap();
                let name = match t["artist"].as_str() {
                    Some(a) => format!("{a} - {}", t["title"].as_str().unwrap()),
                    None => t["title"].as_str().unwrap().to_string(),
                };
                assert!(typed.contains(&name.as_str()), "{name:?}: {said}");
                assert_eq!(line, format!("  {:>3}. {name}", n + 1), "{said}");
            }
        }
        let json = tc.draw(gs::booleans());
        let r = fx.run(&[if json { &["--yes", "--json"][..] } else { &["--yes"][..] }, words].concat());
        assert_eq!(r.code, 1, "{said}\n{}\n{}", r.out, r.err);
        let message = match json {
            true => serde_json::from_str::<Value>(&r.out).unwrap()["error"]["message"].as_str().unwrap().to_string(),
            false => r.err,
        };
        assert!(message.contains("not signed in to an Atmosphere account"), "{message}");
    }
    assert!(pds.calls("com.atproto.repo.putRecord").is_empty());
}

/// In a session's last two days (any time left in them), publishing My Settings or the profile
/// says to sign in again, by when; with `--yes` it publishes all the same, and in a terminal it
/// offers to sign in again first and, declined, asks to publish and does as answered.
#[cfg(unix)]
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn late_in_a_session_publishing_says_to_sign_in_again(tc: TestCase) {
    let pds = MockPds::start();
    let fx = Env::from_template().with_directory(&pds.url);
    let hours = tc.draw(gs::integers::<i64>().min_value(1).max_value(47));
    let by = (chrono::Utc::now() + chrono::Duration::hours(hours)).to_rfc3339();
    sign_in(&fx.store, &pds.url, &PUBLISHING, json!({ "sign_in_again_by": by }));
    let value = tc.draw(gs::sampled_from(&["quantize=on", "quantize=off", "jog_mode=cdj"][..]));
    fx.ok(&["--yes", "settings", "set", value]);
    let (words, what, rkey): (&[&str], &str, &str) = tc.draw(gs::sampled_from(
        &[
            (&["settings", "publish"][..], "your My Settings", "self"),
            (&["profile", "publish", "A", "--no-analysis"][..], "your profile", "self"),
        ][..],
    ));
    let collection = if what == "your profile" { "computer.localnetwork.dj.profile" } else { SETTINGS };
    // Asked in a terminal: not now, then whether to publish.
    let answer = tc.draw(gs::integers::<u8>().max_value(2));
    let r = match answer {
        0 => fx.run(&[&["--yes"][..], words].concat()),
        1 => fx.typed_ahead(words, "n\ny\n"),
        _ => fx.typed_ahead(words, "n\nn\n"),
    };
    let said = format!("{words:?} {hours}h left, answer {answer}: exited {}\n{}\n{}", r.code, r.out, r.err);
    assert!(r.out.contains("! @dj.example.com: sign in again by "), "{said}");
    assert_eq!(r.err.contains("Sign in again now? [y/N]"), answer > 0, "{said}");
    let published = answer < 2;
    assert_eq!(r.code, if published { 0 } else { 3 }, "{said}");
    assert_eq!(r.out.contains(&format!("published {what}")), published, "{said}");
    assert_eq!(pds.record(DID, collection, rkey).is_some(), published, "{said}");
}

/// `sets publish SET --upload` for a set of drawn tracks (timed or not, some not named with
/// `--hide`), with a stand-in ffmpeg on `PATH`: the printed plan says what the JSON's does (the
/// account, nothing uploaded yet, the recording, how it goes up, the chapters); unconfirmed nothing
/// goes; with `--yes`, printed or as JSON, the recording goes up and the set's record points at
/// it; and planned again, it replaces the recording uploaded.
#[cfg(unix)]
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn uploading_a_recording_publishes_the_set_pointing_at_it(tc: TestCase) {
    let pds = MockPds::start();
    pds.set_account(DID, "dj.example.com");
    let fx = Env::new().with_directory(&pds.url);
    sign_in(&fx.store, &pds.url, &PUBLISHING, json!({}));
    let recording = fx.outside("friday.wav");
    std::fs::write(&recording, loclib_testkit::audio::wav(8000, 16, 2.0)).unwrap();
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    let tracks: Vec<Value> = (0..n)
        .map(|i| {
            let mut t = json!({ "title": format!("Track {i}"), "artist": "Bicep", "origin": "added" });
            if i == 0 || tc.draw(gs::booleans()) {
                t["start_seconds"] = json!(i.min(1));
            }
            t
        })
        .collect();
    let (id, at) = ("3m4n5o6p7q8r9", "2026-10-03T23:00:00Z");
    let set = json!({
        "id": id, "title": "Friday", "date": "2026-10-03", "recording": { "path": recording },
        "tracks": tracks, "created_at": at, "updated_at": at,
    });
    std::fs::write(fx.store.join("sets.json"), json!({ "sets": [set] }).to_string()).unwrap();
    let bin = fx.outside("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let script = "#!/bin/sh\nfor last; do :; done\nprintf 'encoded audio' >\"$last\"\n\
                  printf 'total_size=13\\nprogress=end\\n'\n";
    std::fs::write(bin.join("ffmpeg"), script).unwrap();
    crate::support::set_mode(&bin.join("ffmpeg"), 0o755);
    let hide: Vec<usize> = tc.draw(gs::subsequences((1..=n).collect::<Vec<_>>()));
    let mut words: Vec<String> = ["sets", "publish", "Friday", "--upload"].map(String::from).to_vec();
    if !hide.is_empty() {
        words.extend(["--hide".to_string(), hide.iter().map(usize::to_string).collect::<Vec<_>>().join(",")]);
    }
    let run = |global: &[&str], more: &[&str]| {
        let out = fx.command().env("PATH", &bin).args(global).args(&words).args(more).output().unwrap();
        (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
    };

    let (code, planned, err) = run(&["--json"], &["--dry-run"]);
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(&planned).unwrap();
    let p = &v["plan"];
    let (code, out, err) = run(&[], &["--dry-run"]);
    let said = format!("{words:?} {tracks:?}\n{out}\n{err}\n{planned}");
    assert_eq!(code, 0, "{said}");
    let uri = format!("at://{DID}/computer.localnetwork.dj.recording/{id}");
    assert_eq!(p["uri"], uri, "{said}");
    assert!(out.starts_with("Upload this set's recording as @dj.example.com\n"), "{said}");
    assert!(out.contains(&format!("{uri} · not uploaded yet")), "{said}");
    assert!(out.contains(&format!("  Recording {} · WAV · ", recording.display())), "{said}");
    assert!(out.contains("  Goes up   as AAC at 128 kbit/s in an M4A file, about "), "{said}");
    let chapters = p["chapters"].as_array().unwrap();
    let plural = if chapters.len() == 1 { "" } else { "s" };
    let line = format!("  Chapters  {} track{plural}\n", chapters.len());
    assert_eq!(out.contains(&line), !chapters.is_empty(), "{said}");
    // A track not named is no chapter's title.
    for c in chapters {
        let title = c["title"].as_str().unwrap();
        assert!(hide.iter().all(|h| title != format!("Bicep - Track {}", h - 1)), "{title:?}: {said}");
    }
    assert!(out.contains("  then this set's record, pointing at it\n"), "{said}");

    // Unconfirmed: nothing goes.
    let json = tc.draw(gs::booleans());
    let (code, _, err) = run(if json { &["--json"] } else { &[] }, &[]);
    assert_eq!(code, 3, "{err}");
    assert!(pds.calls("com.atproto.repo.uploadBlob").is_empty(), "nothing goes unasked");

    let json = tc.draw(gs::booleans());
    let (code, out, err) = run(if json { &["--yes", "--json"] } else { &["--yes"] }, &[]);
    let said = format!("{said}\n{out}\n{err}");
    assert_eq!(code, 0, "{said}");
    let (recorded, cid) = pds.record(DID, "computer.localnetwork.dj.recording", id).unwrap();
    assert_eq!(recorded["durationSeconds"], 2, "{said}");
    let (set_record, _) = pds.record(DID, "computer.localnetwork.dj.set", id).unwrap();
    assert_eq!(set_record["recording"], json!({ "uri": uri, "cid": cid }), "{said}");
    match json {
        true => {
            let v: Value = serde_json::from_str(&out).unwrap();
            assert_eq!((&v["outcome"], &v["report"]["uri"]), (&json!("applied"), &json!(uri)), "{said}");
            assert_eq!(v["record"]["outcome"], "applied", "{said}");
        }
        false => {
            assert!(out.contains(&format!("✓ uploaded the recording\n  {uri}\n")), "{said}");
            assert!(out.contains("published this set"), "{said}");
        }
    }

    let (code, out, err) = run(&[], &["--dry-run"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains(&format!("{uri} · replaces the recording you uploaded ")), "{out}");
}

/// The settings a model of `settings import` comes to: those published (only those, replacing)
/// over the base.
fn imported(
    base: &IndexMap<&'static str, &'static str>,
    published: &IndexMap<&'static str, &'static str>,
    replace: bool,
) -> IndexMap<&'static str, &'static str> {
    let mut after = if replace { IndexMap::new() } else { base.clone() };
    after.extend(published.iter().map(|(k, v)| (*k, *v)));
    after
}

/// My Settings an account published (with settings this version doesn't know, a few or many),
/// imported by name into any settings profile, adding to its base or replacing it: the printed
/// plan, either way, has a line per change the JSON's has (or says there are none), what it'd
/// replace or add to, and a warning for each setting it leaves out up to 20, then how many more;
/// imported with `--yes`, the base is what the model says, and importing again changes nothing.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn importing_published_settings_comes_to_what_was_published(tc: TestCase) {
    let pds = MockPds::start();
    let fx = Env::new().with_directory(&pds.url);
    let published = draw_settings(&tc);
    // A few say enough: some at and past the 20 shown.
    let unknown = tc.draw(gs::sampled_from(vec![0usize, 3, 20, 21, 25]));
    pds.put(DID, SETTINGS, "self", settings_record(&published, unknown));
    let mut base = draw_settings(&tc);
    set_base(&fx, &base);
    let from = ["settings", "import", "--from-handle", DID];
    let check = |base: &IndexMap<&'static str, &'static str>, replace: bool| {
        let words = [&from[..], if replace { &["--replace", "--dry-run"][..] } else { &["--dry-run"][..] }].concat();
        let v = fx.json(&words);
        let out = fx.ok(&words);
        let said = format!("base {base:?}, published {published:?} + {unknown}, replace {replace}\n{out}\n{v}");
        let after = imported(base, &published, replace);
        let changes = v["plan"]["changes"].as_array().unwrap();
        // In the order of their keys.
        let mut keys: Vec<&str> = POOL.iter().map(|(k, ..)| *k).filter(|k| base.get(k) != after.get(k)).collect();
        keys.sort();
        let planned: Vec<&str> = changes.iter().map(|c| c["key"].as_str().unwrap()).collect();
        assert_eq!(planned, keys, "{said}");
        assert_eq!(v["outcome"], if keys.is_empty() { "nothing_to_do" } else { "dry_run" }, "{said}");
        assert!(out.starts_with("My Settings published by did:plc:testdj (2026-0"), "{said}");
        assert_eq!(out.contains("  the same as your settings profile\n"), keys.is_empty(), "{said}");
        assert_eq!(out.lines().filter(|l| l.contains(" → ")).count(), keys.len(), "{said}");
        let how = match replace {
            true => "Importing replaces your settings profile's base; per-drive overrides stay.",
            false => {
                "Importing adds these to your settings profile's base (--replace replaces it); per-drive overrides stay."
            }
        };
        assert!(out.contains(how), "{said}");
        let warnings = v["plan"]["warnings"].as_array().unwrap();
        let left_out = warnings
            .iter()
            .filter(|w| w.as_str().unwrap().starts_with("left out, this version doesn't know it: "))
            .count();
        assert_eq!(left_out, unknown.min(20), "{said}");
        let more = format!("left out {} more this version doesn't know", unknown.saturating_sub(20));
        assert_eq!(warnings.iter().any(|w| *w == more), unknown > 20, "{said}");
        let (count, n) = (published.len(), keys.len());
        let line = match replace {
            true => {
                format!("replace the settings profile's base with the {count} settings {DID} published ({n} change(s))")
            }
            false => format!("add the {count} settings {DID} published to the settings profile's base ({n} change(s))"),
        };
        assert!(v.to_string().contains(&line), "{line:?}: {said}");
    };
    for replace in [false, true] {
        check(&base, replace);
    }
    let replace = tc.draw(gs::booleans());
    let json = tc.draw(gs::booleans());
    let mut words = vec![if json { "--json" } else { "--quiet" }, "--yes"];
    words.extend(from);
    if replace {
        words.push("--replace");
    }
    let r = fx.run(&words);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    base = imported(&base, &published, replace);
    let profile = fx.json(&["settings", "profile"]);
    let want: serde_json::Map<String, Value> = base.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
    assert_eq!(profile["base"], Value::Object(want), "{profile}");
    // Imported, the same import again has nothing to do.
    check(&base, replace);
    let r = fx.run(&[&["--yes", "--json"][..], &words[2..]].concat());
    let v: Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!((r.code, &v["outcome"]), (0, &json!("nothing_to_do")), "{}", r.out);
}

/// Signed in, a set (titled, or only placed) published at a plugin's destination: listed, it's
/// its title and place, or the place alone; with no recording on this computer it can't be;
/// planned, it says its record will be offered; published with `--yes` (printed or as JSON) its
/// record goes out too, pointing at the link, or (the server refusing it) is said not to have
/// (exit status 5) with the set published there all the same; declined in a terminal, the record is
/// left for later. Deleting the set then names the record it has on the account, if any.
#[cfg(all(unix, debug_assertions))]
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn a_set_published_at_a_destination_publishes_its_record_too(tc: TestCase) {
    use crate::mock_pds::Writes;

    let pds = MockPds::start();
    let mut fx = Env::new().with_directory(&pds.url);
    fx.plugins(&[&crate::support::echo_plugin()]);
    sign_in(&fx.store, &pds.url, &PUBLISHING, json!({}));
    let recording = fx.outside("friday.wav");
    let (id, at) = ("3m4n5o6p7q8r9", "2026-10-03T23:00:00Z");
    let mut set = json!({
        "id": id, "date": "2026-10-03", "venue": "Warehouse", "recording": { "path": recording },
        "tracks": [{ "title": "Glue", "artist": "Bicep", "origin": "added" }],
        "created_at": at, "updated_at": at,
    });
    let title = tc.draw(gs::booleans());
    if title {
        set["title"] = json!("Friday");
    }
    std::fs::write(fx.store.join("sets.json"), json!({ "sets": [set] }).to_string()).unwrap();
    let listed = fx.ok(&["sets"]);
    let place = if title { "Friday · Warehouse" } else { "Warehouse" };
    assert!(listed.contains(&format!("  2026-10-03  {place} · 1 track\n")), "{listed}");

    let publish = ["sets", "publish", id, "--to", "echo:nowhere"];
    if tc.draw(gs::weighted_booleans(0.2)) {
        let r = fx.run(&[&["--yes"][..], &publish].concat());
        assert_eq!(r.code, 1, "{}\n{}", r.out, r.err);
        assert!(r.err.contains(&format!("{}, the set's recording", recording.display())), "{}", r.err);
        return;
    }
    std::fs::write(&recording, b"RIFF").unwrap();
    let out = fx.ok(&[&publish[..], &["--dry-run"]].concat());
    assert!(out.contains("then publishing its record as @dj.example.com is offered"), "{out}");
    let refused = tc.draw(gs::weighted_booleans(0.3));
    if refused {
        pds.set_writes(Writes::RefuseRecord);
    }
    let how = tc.draw(gs::integers::<u8>().max_value(2));
    let r = match how {
        0 => fx.run(&[&["--yes"][..], &publish].concat()),
        1 => fx.run(&[&["--yes", "--json"][..], &publish].concat()),
        _ => fx.typed_ahead(&publish, "y\nn\n"),
    };
    let said = format!("how {how}, refused {refused}: exited {}\n{}\n{}", r.code, r.out, r.err);
    let link = format!("https://sets.example/{id}");
    assert_eq!(fx.json(&["sets", "show", id])["link"], link, "published there either way: {said}");
    let record = pds.record(DID, "computer.localnetwork.dj.set", id).map(|(r, _)| r);
    let later = format!("`locallibrary sets publish {id}` publishes its record later");
    match (how, refused) {
        (2, _) => {
            assert_eq!(r.code, 0, "{said}");
            assert!(r.out.contains(&later) && record.is_none(), "{said}");
        }
        (_, true) => {
            assert_eq!(r.code, 5, "{said}");
            assert!(record.is_none(), "{said}");
            match how {
                0 => assert!(r.out.contains("! its record wasn't published: ") && r.out.contains(&later), "{said}"),
                _ => assert_eq!(serde_json::from_str::<Value>(&r.out).unwrap()["record"]["outcome"], "error", "{said}"),
            }
        }
        _ => {
            assert_eq!(r.code, 0, "{said}");
            assert_eq!(record.as_ref().unwrap()["links"][0]["uri"], link, "{said}");
        }
    }
    let uri = format!("at://{DID}/computer.localnetwork.dj.set/{id}");
    let out = fx.exits(3, &["sets", "delete", id]);
    let named = format!(", and its record on your Atmosphere account ({uri})");
    assert_eq!(out.contains(&named), record.is_some(), "{out}\n{said}");
}
