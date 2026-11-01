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
    assert!(!out.contains("--fix-tempo"), "the library's tempos favour the stored one: {out}");

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

/// `doctor --fix-tempo` plans doubling the beat grid and BPM of the track heard at twice its
/// tempo (the library's other track is at that tempo, so it isn't favoured as stored), saying
/// the extended grid isn't rescaled; `--dry-run` changes nothing, and `undo` takes the fix back.
#[test]
fn doctor_doubles_a_tempo_heard_at_double() {
    let env = Env::new();
    let root = env.demo("A");
    let lib = Device::open(&root).unwrap().library().unwrap();
    let (t, other) = (&lib.tracks[0], &lib.tracks[1]);
    let double = (2.0 * t.bpm).to_string();
    env.ok(&["--yes", "track", "set", path(&root), "--track", &other.id.to_string(), "--bpm", &double]);
    let store = FieldStore::in_store(&Store::open_or_init(&env.store).unwrap());
    for x in [t, other] {
        let heard = analysis(2.0 * t.bpm, Camelot::parse(&x.key).unwrap(), -8.0, 0);
        store.put(&paths::to_fs(&root, &paths::key(&x.file_path)), &sound(), &heard).unwrap();
    }
    let bpm = || Device::open(&root).unwrap().library().unwrap().tracks[0].bpm;

    let out = env.ok(&["doctor", path(&root), "--only", "analysis"]);
    assert!(out.contains("--fix-tempo` doubles or halves the beat grids"), "{out}");
    let dry = env.ok(&["doctor", path(&root), "--fix-tempo", "--dry-run"]);
    assert!(dry.contains(&format!("set the beat grid of {}", t.file_path)), "{dry}");
    assert!(dry.contains("(PQT2) isn't decoded, so it isn't rescaled"), "{dry}");
    assert_eq!(bpm(), t.bpm);
    env.ok(&["--yes", "doctor", path(&root), "--fix-tempo"]);
    assert!((bpm() - 2.0 * t.bpm).abs() < 0.02, "{} from {}", bpm(), t.bpm);
    assert!(env.ok(&["doctor", path(&root), "--fix-tempo"]).contains("nothing to change"));
    env.ok(&["--yes", "undo", path(&root)]);
    assert_eq!(bpm(), t.bpm);
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
    assert!(err.contains("[possible values: key, bpm, grid, phrases, waveform]"), "{err}");
    let err = env.fails(2, &["analyze", path(&root), "--plugin", "sound", "--analyzer", "nope", "--apply", "key"]);
    assert!(err.contains("no plugin's analyzer here works out key"), "{err}");
}

/// `plugin roles` lists which analyzer each role is read from, and chooses one that has it.
#[test]
fn plugin_roles_shows_and_chooses_where_each_role_is_read_from() {
    let env = Env::new();
    let out = env.ok(&["plugin", "roles"]);
    assert!(line(&out, "loudness").contains("sound.tempo-key-loudness  default"), "{out}");
    assert!(line(&out, "identity").contains("musicbrainz.ids  default"), "{out}");
    let v = env.json(&["plugin", "roles", "key"]);
    let sound = "sound.tempo-key-loudness";
    assert_eq!(v, json!([{ "role": "key", "chosen": sound, "set": false, "off": false, "offered": [sound] }]));
    let out = env.ok(&["--yes", "plugin", "roles", "key", sound]);
    assert!(line(&out, "key  ").ends_with("chosen"), "{out}");
    let out = env.ok(&["--yes", "plugin", "roles", "key", "--reset"]);
    assert!(line(&out, "key  ").ends_with("default"), "{out}");
    let err = env.fails(1, &["--yes", "plugin", "roles", "key", "sound.nope"]);
    assert!(err.contains("sound.nope doesn't work out key: sound.tempo-key-loudness does"), "{err}");
    let err = env.fails(2, &["plugin", "roles", "tempo"]);
    assert!(err.contains("\"tempo\" isn't a role"), "{err}");

    // A role read from none, for good: `sets publish` looks up no MusicBrainz IDs.
    let out = env.ok(&["--yes", "plugin", "roles", "recording", "--none"]);
    assert!(line(&out, "recording  ").ends_with("none, as chosen"), "{out}");
    let v = env.json(&["plugin", "roles", "recording"]);
    assert_eq!((&v[0]["chosen"], &v[0]["off"]), (&Value::Null, &json!(true)), "{v}");
    let out = env.ok(&["--yes", "plugin", "roles", "recording", "--reset"]);
    assert!(line(&out, "recording  ").contains("musicbrainz.ids  default"), "{out}");
}

/// `analyze` alone leaves out the analyzers of plugins that go online, and says how to run them:
/// naming the plugin does.
#[cfg(debug_assertions)]
#[test]
fn analyze_alone_stays_on_this_computer() {
    let mut env = Env::new();
    let online = crate::support::echo_plugin().replacen(
        "capabilities = [\"library:read\"",
        "capabilities = [\"net:example.org\", \"library:read\"",
        1,
    );
    env.plugins(&[&online]);
    let root = analysed(&env, true);
    let r = env.run(&["analyze", path(&root)]);
    assert_eq!(r.code, 0, "{}", r.err);
    let said = "left out what goes online (musicbrainz, echo): name it to run it, as in --plugin musicbrainz";
    assert!(r.err.contains(said), "{}", r.err);
    assert!(r.out.contains("(sound)") && !r.out.contains("(echo)"), "{}", r.out);
    let out = env.ok(&["analyze", path(&root), "--plugin", "echo"]);
    assert!(out.contains("size (echo): 2 tracks analyzed"), "{out}");
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
        json!({
            "role": "key", "chosen": "sound.tempo-key-loudness", "set": false, "off": false,
            "offered": ["sound.tempo-key-loudness"],
        })
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

/// `--apply grid,phrases,waveform` writes what an analyzer worked out besides its columns into the
/// tracks' analysis files, planned as any edit is, and the sidecar plans it too; a key or BPM goes
/// in a run of its own.
#[cfg(debug_assertions)]
#[test]
fn analyze_writes_a_grid_into_the_analysis_files() {
    let mut env = Env::new();
    let heard = "\n[[contributes.analyzers]]\nname = \"heard\"\ntitle = \"Heard\"\nversion = \"1\"\n\
                 fields = { grid = \"beats\", tone = \"string\" }\nroles = { key = \"tone\" }\n";
    env.plugins(&[&(crate::support::echo_plugin() + heard)]);
    let root = env.demo("A");
    let grid: Vec<Value> =
        (0..16).map(|i| json!({ "beat_number": i % 4 + 1, "bpm": 125.0, "time_ms": 30 + i * 480 })).collect();
    // What the test plugin's `heard` answers: what's in the audio file.
    for t in Device::open(&root).unwrap().library().unwrap().tracks {
        std::fs::write(paths::to_fs(&root, &paths::key(&t.file_path)), json!({ "grid": grid }).to_string()).unwrap();
    }
    let err = env.fails(2, &["analyze", path(&root), "--apply", "key,grid"]);
    assert!(err.contains("write a key and BPM in a run of their own"), "{err}");
    let err = env.fails(2, &["analyze", path(&root), "--apply", "grid", "--min-confidence", "0.5"]);
    assert!(err.contains("--min-confidence is for a key and BPM"), "{err}");
    let out = env.ok(&["analyze", path(&root), "--apply", "grid", "--dry-run"]);
    assert!(out.contains("heard (echo): 2 tracks analyzed"), "{out}");
    assert!(out.contains("2 of what's asked the tracks have already"), "{out}");
    let out = env.ok(&["--yes", "analyze", path(&root), "--apply", "grid", "--overwrite"]);
    assert!(out.contains("write the grid from Heard to 2 track(s)"), "{out}");
    let d = Device::open(&root).unwrap();
    for t in d.library().unwrap().tracks {
        let beats = locallibrary::anlz::load(&d, &t).beats;
        assert_eq!((beats.len(), beats[1].time_ms, t.bpm), (16, 510, 125.0));
    }

    let mut s = Rpc::start(env.command());
    let apply =
        json!({ "plugin": "echo", "analyzer": "heard", "drive": path(&root), "parts": ["grid"], "overwrite": true });
    let planned = s.call(1, "plugins.apply_analysis.plan", apply);
    assert_eq!(planned["result"]["noop"], true, "as it is now: {planned}");
    let nothing = s.call(
        2,
        "plugins.apply_analysis.plan",
        json!({ "plugin": "echo", "analyzer": "heard", "drive": path(&root), "parts": [] }),
    );
    assert_eq!(nothing["error"]["data"]["kind"], "invalid", "{nothing}");
}

/// For any of grid, phrases and waveform asked of the test plugin's `heard` (one of two analyzers
/// working out a grid), overwriting or not, planned or applied, in text or `--json`, with a track
/// whose audio fails it or none: `analyze --apply` runs it and writes what it worked out where
/// it can, saying what it left out (a grid only over the tracks' with --overwrite, phrases on the
/// tracks' grid, no waveform over rekordbox's colour one), and changes nothing on a dry run. A
/// key or BPM asked with them, --min-confidence, an analyzer not named or one that doesn't work
/// it out is refused before anything runs, and every track failing is an error. A key or BPM
/// asked alone is written from the analyzer chosen for it (the sound plugin's, of it and
/// `heard`), as `plugins.apply_fields` plans it.
#[cfg(debug_assertions)]
#[hegel::test(test_cases = 40, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn analyze_apply_writes_the_parts_asked_where_it_can(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use locallibrary::plugin::AnalysisPart;
    let mut env = Env::new();
    let analyzers = "\n[[contributes.analyzers]]\nname = \"heard\"\ntitle = \"Heard\"\nversion = \"1\"\n\
                     fields = { grid = \"beats\", phrases = \"phrases\", wave = \"waveform\", tone = \"string\" }\n\
                     roles = { key = \"tone\" }\n\n\
                     [[contributes.analyzers]]\nname = \"beat\"\ntitle = \"Beat\"\nversion = \"1\"\n\
                     fields = { beats = \"beats\" }\n";
    env.plugins(&[&(crate::support::echo_plugin() + analyzers)]);
    let (overwrite, dry_run, as_json) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    let flags = |mut args: Vec<&'static str>| {
        args.extend(overwrite.then_some("--overwrite"));
        args.extend(dry_run.then_some("--dry-run"));
        if as_json {
            args.insert(0, "--json");
        }
        args
    };
    if tc.draw(gs::weighted_booleans(0.25)) {
        // The sound plugin heard the first track at double its tempo in its parallel key.
        let root = analysed(&env, false);
        let before = Device::open(&root).unwrap().library().unwrap().tracks;
        let bpm = tc.draw(gs::booleans());
        let key = !bpm || tc.draw(gs::booleans());
        let roles: Vec<&str> = [(key, "key"), (bpm, "bpm")].into_iter().filter(|r| r.0).map(|r| r.1).collect();
        let listed = roles.join(",");
        let mut args = flags(vec!["--yes", "analyze"]);
        args.extend([path(&root), "--apply", &listed]);
        let r = env.run(&args);
        assert_eq!(r.code, 0, "{args:?}\n{}\n{}", r.out, r.err);
        let written = usize::from(overwrite);
        let summary = format!("write {} from Tempo, key and loudness to {written} track(s)", roles.join(" and "));
        if as_json {
            let v: Value = serde_json::from_str(&r.out).unwrap();
            assert_eq!(v["plan"]["operation"], "plugins.apply_fields", "{v}");
            assert_eq!(v["plan"]["summary"][0], summary.as_str(), "{v}");
        } else {
            assert!(r.out.contains("tempo-key-loudness (sound): 0 tracks analyzed, 2 already"), "{}", r.out);
            assert!(r.out.contains(&summary), "{}", r.out);
        }
        let after = Device::open(&root).unwrap().library().unwrap().tracks;
        let applied = written > 0 && !dry_run;
        let heard_key = Camelot::parse(&before[0].key).unwrap().parallel().note();
        assert_eq!(after[0].key, if applied && key { heard_key } else { before[0].key.clone() });
        assert_eq!(after[0].bpm, if applied && bpm { before[0].bpm * 2.0 } else { before[0].bpm });
        assert_eq!((&after[1].key, after[1].bpm), (&before[1].key, before[1].bpm));
        return;
    }
    let root = env.demo("A");
    let device = Device::open(&root).unwrap();
    let tracks = device.library().unwrap().tracks;
    let before: Vec<_> = tracks.iter().map(|t| locallibrary::anlz::load(&device, t).beats).collect();
    let grid: Vec<Value> =
        (0..16).map(|i| json!({ "beat_number": i % 4 + 1, "bpm": 125.0, "time_ms": 30 + i * 480 })).collect();
    let phrases = json!({ "mood": "mid", "phrases": [{ "kind": "intro", "start_beat": 1, "end_beat": 17 }] });
    let failing = match tc.draw(gs::weighted_booleans(0.3)) {
        true => tc.draw(gs::integers::<usize>().min_value(1).max_value(tracks.len())),
        false => 0,
    };
    // What the test plugin's `heard` answers: what's in the audio file, the first `failing`
    // tracks' not JSON.
    for (i, t) in tracks.iter().enumerate() {
        let answer = json!({
            "grid": grid,
            "phrases": phrases,
            "wave": { "blob": "w.bands", "columns_per_sec": 150.0 },
            "blobs": { "w.bands": vec![9u8; 3 * 300] },
        });
        let audio = if i < failing { "not JSON".to_string() } else { answer.to_string() };
        std::fs::write(paths::to_fs(&root, &paths::key(&t.file_path)), audio).unwrap();
    }
    let at = path(&root);
    let heard = ["--plugin", "echo", "--analyzer", "heard"];
    let refusal = tc.draw(gs::weighted_booleans(0.3)).then(|| tc.draw(gs::integers::<u8>().max_value(3)));
    let refused: Option<(Vec<&str>, &str)> = match refusal {
        None => None,
        Some(0) => Some((vec!["--apply", "bpm,phrases"], "write a key and BPM in a run of their own")),
        Some(1) => Some((vec!["--apply", "grid", "--min-confidence", "0.5"], "--min-confidence is for a key and BPM")),
        Some(2) => {
            Some((vec!["--apply", "grid"], "several analyzers work out grid: say which with --plugin and --analyzer"))
        }
        Some(_) => {
            Some((vec!["--analyzer", "beat", "--apply", "waveform"], "no plugin's analyzer here works out waveform"))
        }
    };
    if let Some((args, why)) = refused {
        let err = env.fails(2, &[&["analyze", at][..], &args].concat());
        assert!(err.contains(why), "{err}");
        assert!(!err.contains("analyzed"), "nothing ran: {err}");
        return;
    }
    let mut parts: Vec<AnalysisPart> =
        [(AnalysisPart::Grid, 0.7), (AnalysisPart::Phrases, 0.5), (AnalysisPart::Waveform, 0.3)]
            .into_iter()
            .filter(|(_, p)| tc.draw(gs::weighted_booleans(*p)))
            .map(|(part, _)| part)
            .collect();
    if parts.is_empty() {
        parts.push(AnalysisPart::Grid);
    }
    let asked: Vec<&str> = parts.iter().map(|p| p.name()).collect();
    let listed = asked.join(",");
    let mut args = flags(vec!["--yes", "analyze"]);
    args.extend([at, "--apply", &listed]);
    args.extend(heard);
    let r = env.run(&args);
    let analysed = tracks.len() - failing;
    if analysed == 0 {
        assert_eq!(r.code, 1, "{}", r.err);
        let said = if as_json { &r.out } else { &r.err };
        assert!(said.contains(&format!("every track failed ({failing})")), "{said}");
        return;
    }
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    let wants = |p| parts.contains(&p);
    // The demo's tracks have a grid, phrases and rekordbox's colour waveform: only a grid with
    // --overwrite is written (with the phrases on it), a waveform leaving the track out.
    let written = match overwrite && wants(AnalysisPart::Grid) && !wants(AnalysisPart::Waveform) {
        true => analysed,
        false => 0,
    };
    let what: Vec<&str> = AnalysisPart::ALL.into_iter().filter(|p| wants(*p)).map(AnalysisPart::name).collect();
    let summary = format!("write the {} from Heard to {written} track(s)", what.join(" and "));
    let mut warnings = Vec::new();
    if failing > 0 {
        warnings.push(format!("{failing} track(s) aren't analysed by Heard yet"));
    }
    if !overwrite {
        warnings.push(format!("{} of what's asked the tracks have already", analysed * parts.len()));
    }
    if overwrite && wants(AnalysisPart::Phrases) && !wants(AnalysisPart::Grid) {
        warnings.push(format!("{analysed} track(s)' phrases left out"));
    }
    if overwrite && wants(AnalysisPart::Waveform) {
        warnings.push(format!("{analysed} track(s) left out, e.g. "));
    }
    if as_json {
        let v: Value = serde_json::from_str(&r.out).unwrap();
        assert_eq!(v["plan"]["operation"], "plugins.apply_analysis", "{v}");
        assert_eq!(v["plan"]["summary"][0], summary.as_str(), "{v}");
        assert_eq!(v["applied"], json!(written > 0 && !dry_run), "{v}");
        let said = v["plan"]["warnings"].as_array().unwrap();
        for w in &warnings {
            assert!(said.iter().any(|s| s.as_str().unwrap().starts_with(w.as_str())), "{w:?} in {said:?}");
        }
    } else {
        let ran = format!("heard (echo): {analysed} track{} analyzed", if analysed == 1 { "" } else { "s" });
        assert!(r.out.contains(&ran) && r.out.contains(&summary), "{}", r.out);
        assert_eq!(r.out.contains("nothing to change"), written == 0, "{}", r.out);
        for w in &warnings {
            assert!(r.out.contains(w.as_str()) || r.err.contains(w.as_str()), "{w:?} in\n{}\n{}", r.out, r.err);
        }
    }
    let device = Device::open(&root).unwrap();
    let applied = written > 0 && !dry_run;
    for (i, t) in device.library().unwrap().tracks.iter().enumerate() {
        let beats = locallibrary::anlz::load(&device, t).beats;
        match applied && i >= failing {
            true => assert_eq!((beats.len(), beats[1].time_ms, t.bpm), (16, 510, 125.0)),
            false => assert_eq!(beats, before[i]),
        }
    }
}

/// What `plugin roles` prints for each role in `listed` (its `--json`): the analyzer it's read
/// from and whether that's chosen or the default (with the others that have it), or that it's
/// read from none.
fn roles_text(listed: &Value) -> String {
    let all = listed.as_array().unwrap();
    let name = |c: &Value| c["role"].as_str().unwrap().to_string();
    let width = all.iter().map(|c| name(c).len()).max().unwrap_or(0);
    let mut out = String::new();
    for c in all {
        let chosen = c["chosen"].as_str();
        let others: Vec<&str> =
            c["offered"].as_array().unwrap().iter().filter_map(Value::as_str).filter(|o| Some(*o) != chosen).collect();
        let how = match (c["set"] == true, others.is_empty()) {
            (true, _) => "chosen".to_string(),
            (false, true) => "default".to_string(),
            (false, false) => format!("default; also {}", others.join(", ")),
        };
        let role = format!("{:width$}", name(c));
        out += &match chosen {
            Some(a) => format!("{role}  {a}  {how}\n"),
            None if c["off"] == true => format!("{role}  none, as chosen\n"),
            None => format!("{role}  no analyzer works it out\n"),
        };
    }
    out
}

/// Any steps of `plugin roles` (a role read from an analyzer that has it, from one that doesn't,
/// from none, or back from its default; confirmed or not; shown in text or `--json`): each role
/// reads back as last chosen, its default until then and again once put back, and an analyzer
/// that doesn't have it is refused, naming those that do. The text says what the JSON does.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn plugin_roles_read_back_as_last_chosen(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let env = Env::new();
    let all = env.json(&["plugin", "roles"]);
    assert_eq!(env.ok(&["plugin", "roles"]), roles_text(&all));
    let roles: Vec<(String, Vec<String>, Value)> = all
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let offered = c["offered"].as_array().unwrap().iter().map(|o| o.as_str().unwrap().to_string()).collect();
            (c["role"].as_str().unwrap().to_string(), offered, c["chosen"].clone())
        })
        .collect();
    // Each role's state: `None` its default, `Some(None)` read from none, `Some(Some(a))` from `a`.
    let mut kept: Vec<Option<Option<String>>> = vec![None; roles.len()];
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        let i = tc.draw(gs::integers::<usize>().max_value(roles.len() - 1));
        let (role, offered, default) = &roles[i];
        let step = tc.draw(gs::sampled_from(vec!["choose", "choose", "wrong", "none", "reset"]));
        let yes = tc.draw(gs::weighted_booleans(0.8));
        let analyzer = match step {
            "choose" if !offered.is_empty() => tc.draw(gs::sampled_from(offered.clone())),
            _ => "sound.nope".to_string(),
        };
        let mut args = vec!["plugin", "roles", role.as_str()];
        match step {
            "none" => args.push("--none"),
            "reset" => args.push("--reset"),
            _ => args.push(&analyzer),
        }
        if yes {
            args.insert(0, "--yes");
        }
        let r = env.run(&args);
        let said = format!("{args:?}: exit {}\n{}{}", r.code, r.out, r.err);
        if analyzer == "sound.nope" && step != "none" && step != "reset" {
            let why = match offered.is_empty() {
                true => format!("no analyzer works out {role}"),
                false => format!("sound.nope doesn't work out {role}: {} does", offered.join(", ")),
            };
            assert!(r.code == if yes { 1 } else { 3 } && (!yes || r.err.contains(&why)), "{why}: {said}");
        } else if !yes {
            assert_eq!(r.code, 3, "{said}");
        } else {
            assert_eq!(r.code, 0, "{said}");
            kept[i] = match step {
                "none" => Some(None),
                "reset" => None,
                _ => Some(Some(analyzer.clone())),
            };
        }
        let shown = env.json(&["plugin", "roles", role]);
        // Changed, it shows the role as it's read now.
        assert!(r.code != 0 || r.out.ends_with(&roles_text(&shown)), "{said}\n{shown}");
        let c = &shown[0];
        match &kept[i] {
            None => assert!(c["chosen"] == *default && c["set"] == false && c["off"] == false, "{said}\n{shown}"),
            Some(None) => assert!(c["chosen"].is_null() && c["off"] == true, "{said}\n{shown}"),
            Some(Some(a)) => assert!(c["chosen"] == a.as_str() && c["set"] == true && c["off"] == false, "{shown}"),
        }
        assert_eq!(env.ok(&["plugin", "roles", role]), roles_text(&shown), "{said}");
    }
}
