//! `doctor --analyze`, the check's loudness item, `analyze --apply`, `plugin roles` and
//! `analysis bench`. Most tracks' tempos, keys and levels are put in the store beforehand, as the
//! sound plugin's analyzer would have kept them, so nothing is decoded for them.

use std::f64::consts::PI;

use locallibrary::fields::{Analyzer, FieldStore, FieldValue, Fields};
use locallibrary::harmony::Camelot;
use locallibrary::{Device, Store, paths};
use loclib_mixid::test_util::wav_of;
use loclib_plugin::Manifest;

use serde_json::{Value, json};

use crate::support::{Env, Rpc, line, path};

/// The sound plugin's analyzer, at the version whose values are read.
fn sound() -> Analyzer {
    let m = Manifest::parse(locallibrary_plugin_sound::MANIFEST).unwrap();
    let a = &m.contributes.analyzers[0];
    Analyzer { plugin: m.id.clone(), analyzer: a.name.clone(), version: a.version.clone() }
}

/// What the sound plugin would answer for a track heard at `bpm` in `key`, at `lufs`, clipped
/// `clipped_runs` times (and then over 0 dBTP).
fn analysis(bpm: f64, key: Camelot, lufs: f64, clipped_runs: i64) -> Fields {
    let fields = [
        ("bpm", FieldValue::Number(bpm)),
        ("bpm_confidence", FieldValue::Number(0.9)),
        ("key", FieldValue::String(key.code())),
        ("key_confidence", FieldValue::Number(0.8)),
        ("integrated_lufs", FieldValue::Number(lufs)),
        ("true_peak_dbtp", FieldValue::Number(if clipped_runs > 0 { 1.58 } else { -0.92 })),
        ("clipped_runs", FieldValue::Integer(clipped_runs)),
    ];
    fields.into_iter().map(|(name, v)| (name.to_string(), v)).collect()
}

/// The demo drive with its first track heard at double its tempo in its parallel key, and its
/// second clipped (or with `agreeing`, both heard as stored, at one level).
fn analysed(env: &Env, agreeing: bool) -> std::path::PathBuf {
    let root = env.demo("A");
    let store = FieldStore::in_store(&Store::open_or_init(&env.store).unwrap());
    let lib = Device::open(&root).unwrap().library().unwrap();
    for (i, t) in lib.tracks.iter().enumerate() {
        let key = Camelot::parse(&t.key).unwrap();
        let heard = match (agreeing, i) {
            (true, _) => analysis(t.bpm, key, -8.0, 0),
            (false, 0) => analysis(t.bpm * 2.0, key.parallel(), -8.0, 0),
            (false, _) => analysis(t.bpm, key, -8.0, 4),
        };
        store.put(&paths::to_fs(&root, &paths::key(&t.file_path)), &sound(), &heard).unwrap();
    }
    root
}

#[test]
fn doctor_lists_what_the_analysis_disagrees_with() {
    let env = Env::new();
    let root = analysed(&env, false);
    let out = env.ok(&["doctor", path(&root), "--only", "analysis"]);
    assert!(out.contains("Analysis (2 track(s) analysed)"), "nothing decoded: {out}");
    assert!(line(&out, "Tempo (1)").contains("stored, then heard"));
    assert!(out.contains("(half"), "{out}");
    assert!(out.contains("(parallel)"), "{out}");
    assert!(line(&out, "Level (1").contains("median is -8.0 LUFS"), "{out}");
    assert!(out.contains("over 0 dBTP, clipped"), "{out}");
    assert!(out.contains("x2 and /2"), "{out}");

    let v = env.json(&["doctor", path(&root), "--analyze", "--only", "unfiled"]);
    assert_eq!(v["analysis"]["tempo"][0]["relation"], "half", "{v}");
    assert!((v["analysis"]["tempo"][0]["confidence"].as_f64().unwrap() - 0.9).abs() < 1e-6, "{v}");
    assert_eq!(v["analysis"]["key"][0]["relation"], "parallel");
    assert_eq!(v["analysis"]["loudness"][0]["problems"], json!(["over_zero", "clipped"]));

    // The check before a set warns about the clipped track, and lists it with --verbose.
    let out = env.exits(0, &["check", path(&root), "--verbose"]);
    assert!(line(&out, "loudness").contains("1 track(s) are over 0 dBTP, clipped"), "{out}");
    assert!(out.contains("+1.6 dBTP, 4 clipped run(s)"), "{out}");
}

/// A 100 Hz sine driven to twice full scale for `secs`: clipped flat on every half cycle.
fn clipped_wav(secs: f64) -> Vec<u8> {
    let n = (44_100.0 * secs) as usize;
    let pcm: Vec<i32> = (0..n)
        .map(|i| ((2.0 * (2.0 * PI * 100.0 * i as f64 / 44_100.0).sin()).clamp(-1.0, 1.0) * 32_767.0) as i32)
        .collect();
    wav_of(44_100, 16, pcm.into_iter())
}

/// Doctor's analysis runs the sound plugin first on the tracks it hasn't analysed (the demo's
/// placeholder audio doesn't decode, and says why), once: then the store has what it heard.
#[test]
fn doctor_analyses_with_the_sound_plugin_first() {
    let env = Env::new();
    let root = env.demo("A");
    let lib = Device::open(&root).unwrap().library().unwrap();
    std::fs::write(paths::to_fs(&root, &paths::key(&lib.tracks[0].file_path)), clipped_wav(3.0)).unwrap();
    let r = env.run(&["doctor", path(&root), "--only", "analysis"]);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    assert!(r.out.contains("Analysis (1 track(s) analysed, 1 just now)"), "{}", r.out);
    assert!(r.out.contains("clipped"), "{}", r.out);
    let failed = line(&r.out, "couldn't be analysed");
    assert!(failed.contains("1 track(s) couldn't be analysed") && failed.contains(&lib.tracks[1].title), "{failed}");
    let again = env.ok(&["doctor", path(&root), "--only", "analysis"]);
    assert!(again.contains("Analysis (1 track(s) analysed)"), "kept: {again}");
}

/// What the analyzer heard is written to the tracks as an edit, asked about first: only where a
/// track has none unless `--overwrite`, and planned without changing anything with `--dry-run`.
#[test]
fn analyze_writes_keys_and_bpms_to_the_tracks() {
    let env = Env::new();
    let root = analysed(&env, false);
    let before = Device::open(&root).unwrap().library().unwrap().tracks;
    let out = env.ok(&["analyze", path(&root), "--apply", "key,bpm"]);
    assert!(out.contains("tempo-key-loudness (sound): 0 tracks analyzed, 2 already"), "{out}");
    assert!(out.contains("nothing to change"), "the tracks have keys and BPMs: {out}");
    let unsure = env.ok(&["analyze", path(&root), "--apply", "key", "--overwrite", "--min-confidence", "0.95"]);
    assert!(unsure.contains("nothing to change") && unsure.contains("less sure of them than 0.95"), "{unsure}");
    let planned = env.json(&["analyze", path(&root), "--apply", "key,bpm", "--overwrite", "--dry-run"]);
    assert_eq!(planned["plan"]["operation"], "plugins.apply_fields", "{planned}");
    assert_eq!(planned["plan"]["summary"][0], "write key and bpm from Tempo, key and loudness to 1 track(s)");
    let out = env.ok(&["--yes", "analyze", path(&root), "--apply", "key,bpm", "--overwrite"]);
    assert!(out.contains("write key and bpm from Tempo, key and loudness to 1 track(s)"), "{out}");
    let after = Device::open(&root).unwrap().library().unwrap().tracks;
    let key = Camelot::parse(&before[0].key).unwrap().parallel().note();
    assert_eq!((&after[0].key, after[0].bpm), (&key, before[0].bpm * 2.0));
    assert_eq!((&after[1].key, after[1].bpm), (&before[1].key, before[1].bpm));

    let err = env.fails(2, &["analyze", path(&root), "--overwrite"]);
    assert!(err.contains("--apply"), "{err}");
    let err = env.fails(2, &["analyze", path(&root), "--apply", "loudness"]);
    assert!(err.contains("[possible values: key, bpm]"), "{err}");
    let err = env.fails(2, &["analyze", path(&root), "--plugin", "sound", "--analyzer", "nope", "--apply", "key"]);
    assert!(err.contains("no plugin's analyzer here works out key"), "{err}");
}

/// `plugin roles` lists which analyzer each role is read from, and chooses one that has it.
#[test]
fn plugin_roles_shows_and_chooses_where_each_role_is_read_from() {
    let env = Env::new();
    let out = env.ok(&["plugin", "roles"]);
    assert!(line(&out, "loudness").contains("sound.tempo-key-loudness  default"), "{out}");
    assert!(line(&out, "identity").contains("no analyzer works it out"), "{out}");
    let v = env.json(&["plugin", "roles", "key"]);
    let sound = "sound.tempo-key-loudness";
    assert_eq!(v, json!([{ "role": "key", "chosen": sound, "set": false, "offered": [sound] }]));
    let out = env.ok(&["--yes", "plugin", "roles", "key", sound]);
    assert!(line(&out, "key  ").ends_with("chosen"), "{out}");
    let out = env.ok(&["--yes", "plugin", "roles", "key", "--reset"]);
    assert!(line(&out, "key  ").ends_with("default"), "{out}");
    let err = env.fails(1, &["--yes", "plugin", "roles", "key", "sound.nope"]);
    assert!(err.contains("sound.nope doesn't work out key: sound.tempo-key-loudness does"), "{err}");
    let err = env.fails(2, &["plugin", "roles", "tempo"]);
    assert!(err.contains("\"tempo\" isn't a role"), "{err}");
}

#[test]
fn bench_scores_rekordbox_and_the_analysis() {
    let env = Env::new();
    let root = env.demo("A");
    let lib = Device::open(&root).unwrap().library().unwrap();
    // Annotations named after the drive's audio files, agreeing with what rekordbox stored.
    let (keys, tempos) = (env.outside("keys"), env.outside("tempos"));
    std::fs::create_dir_all(&keys).unwrap();
    std::fs::create_dir_all(&tempos).unwrap();
    for t in &lib.tracks {
        let name = t.file_path.rsplit('/').next().unwrap();
        let id = name.split('.').next().unwrap();
        std::fs::write(keys.join(format!("{id}.key")), Camelot::parse(&t.key).unwrap().note()).unwrap();
        std::fs::write(tempos.join(format!("{id}.bpm")), t.bpm.to_string()).unwrap();
    }
    // One more annotated track, which isn't on the drive.
    std::fs::write(keys.join("9999.LOFI.key"), "D major").unwrap();
    let args = ["analysis", "bench", path(&root), "--keys", path(&keys), "--tempos", path(&tempos)];
    let r = env.run(&args);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    let out = r.out;
    assert!(line(&out, "rekordbox").contains("weighted 100.0%, correct 100.0%"), "{out}");
    // The demo's placeholder audio doesn't decode: the analysis has no key for either track.
    assert!(line(&out, "stratum-dsp").contains("weighted 0.0%, correct 0.0%  no key 100.0%"), "{out}");
    assert!(out.contains("Accuracy1 100.0%, Accuracy2 100.0%"), "{out}");
    assert!(out.contains("1 annotated track(s) had no audio to score"), "{out}");
    assert!(line(&out, "couldn't be analysed").contains("2 file(s)"), "{out}");

    let v = env.json(&args);
    assert_eq!(v["keys"]["rekordbox"]["tracks"], 2, "{v}");
    assert_eq!(v["keys"]["stratum-dsp"]["counts"]["missing"], 2);
    assert_eq!(v["failed"].as_array().unwrap().len(), 2);

    let err = env.fails(2, &["analysis", "bench", path(&root)]);
    assert!(err.contains("--keys"), "{err}");
}

/// When the analysis agrees with everything, doctor and the check say so.
#[test]
fn agreement_is_said_once_each() {
    let env = Env::new();
    let root = analysed(&env, true);
    let out = env.ok(&["doctor", path(&root), "--only", "analysis"]);
    assert!(out.contains("every stored tempo agrees with the analysis"), "{out}");
    assert!(out.contains("every stored key agrees with the analysis"), "{out}");
    assert!(out.contains("no track is over 0 dBTP, clipped or far from the library's level"), "{out}");
    let out = env.ok(&["check", path(&root)]);
    assert!(line(&out, "loudness").contains("none of the 2 analysed track(s)"), "{out}");
}

/// The sidecar chooses which analyzer a role is read from, writes what one heard to the tracks
/// through a plan of its own (applied only as one), and runs doctor's analysis as the command line
/// does.
#[test]
fn the_sidecar_chooses_roles_and_writes_what_was_heard() {
    let env = Env::new();
    let root = analysed(&env, false);
    let before = Device::open(&root).unwrap().library().unwrap().tracks;
    let mut s = Rpc::start(env.command());
    let result = |r: Value| r.get("result").cloned().unwrap_or_else(|| panic!("{r}"));
    let roles = result(s.call(1, "plugins.roles", Value::Null));
    assert_eq!(
        roles[0],
        json!({ "role": "key", "chosen": "sound.tempo-key-loudness", "set": false, "offered": ["sound.tempo-key-loudness"] })
    );
    let wrong = s.call(2, "plugins.roles.set", json!({ "role": "key", "analyzer": "sound.nope" }));
    assert_eq!(wrong["error"]["data"]["kind"], "invalid", "{wrong}");
    let set = result(s.call(3, "plugins.roles.set", json!({ "role": "key", "analyzer": "sound.tempo-key-loudness" })));
    assert_eq!(set[0]["set"], true);
    assert_eq!(result(s.call(4, "plugins.roles.set", json!({ "role": "key" })))[0]["set"], false);

    let apply = json!({ "plugin": "sound", "analyzer": "tempo-key-loudness", "drive": path(&root), "fields": ["key", "bpm"], "overwrite": true });
    let planned = result(s.call(5, "plugins.apply_fields.plan", apply));
    assert_eq!(planned["summary"][0], "write key and bpm from Tempo, key and loudness to 1 track(s)", "{planned}");
    let wrong = s.call(6, "plugins.op.apply", json!({ "plan_id": planned["plan_id"] }));
    assert!(
        wrong["error"]["message"].as_str().unwrap().contains("is a plugins.apply_fields plan, not plugins.op"),
        "{wrong}"
    );
    result(s.call(7, "plugins.apply_fields.apply", json!({ "plan_id": planned["plan_id"] })));
    let after = Device::open(&root).unwrap().library().unwrap().tracks;
    assert_eq!(after[0].bpm, before[0].bpm * 2.0);

    let doctor = result(s.call(8, "doctor", json!({ "drive": path(&root), "only": ["analysis"] })));
    assert_eq!(
        (doctor["analysis"]["analyzed"].clone(), doctor["analysis"]["decoded"].clone()),
        (json!(2), json!(0)),
        "{doctor}"
    );
}

/// Without the sound plugin (here left out, clashing with another called `sound`), what needs a
/// track's tempo, key or level says there's nothing to work them out, and the rest works.
#[cfg(debug_assertions)]
#[test]
fn without_the_sound_plugin_its_analysis_says_so() {
    let mut env = Env::new();
    env.plugins(&[&crate::support::echo_plugin().replace("id = \"echo\"", "id = \"sound\"")]);
    let root = env.demo("A");
    let r = env.run(&["doctor", path(&root), "--only", "analysis"]);
    assert_eq!(r.code, 1, "{}\n{}", r.out, r.err);
    assert!(r.err.contains("the sound plugin is left out"), "{}", r.err);
    assert!(r.err.contains("error: no plugin works out bpm"), "{}", r.err);
    let out = env.ok(&["check", path(&root)]);
    assert!(!out.contains("loudness"), "nothing measured: {out}");
    assert!(env.ok(&["plugin", "roles", "key"]).contains("no analyzer works it out"));
    let err = env.fails(2, &["analyze", path(&root), "--apply", "key"]);
    assert!(err.contains("no plugin's analyzer here works out key"), "{err}");
}

/// With two analyzers working out a key, `--apply key` runs the one chosen for it: the sound
/// plugin's at first, the other once `plugin roles` chooses it.
#[cfg(debug_assertions)]
#[test]
fn analyze_applies_from_the_analyzer_chosen_for_the_role() {
    let mut env = Env::new();
    let heard = "\n[[contributes.analyzers]]\nname = \"heard\"\ntitle = \"Heard\"\nversion = \"1\"\n\
                 fields = { tone = \"string\" }\nroles = { key = \"tone\" }\n";
    env.plugins(&[&(crate::support::echo_plugin() + heard)]);
    let root = analysed(&env, false);
    let out = env.ok(&["analyze", path(&root), "--apply", "key", "--dry-run"]);
    assert!(out.contains("tempo-key-loudness (sound)") && !out.contains("heard (echo)"), "{out}");
    env.ok(&["--yes", "plugin", "roles", "key", "echo.heard"]);
    // What the test plugin's `heard` answers: what's in the audio file.
    for t in Device::open(&root).unwrap().library().unwrap().tracks {
        std::fs::write(paths::to_fs(&root, &paths::key(&t.file_path)), r#"{"tone": "8A"}"#).unwrap();
    }
    let out = env.ok(&["analyze", path(&root), "--apply", "key", "--dry-run"]);
    assert!(out.contains("heard (echo): 2 tracks analyzed") && !out.contains("(sound)"), "{out}");
}
