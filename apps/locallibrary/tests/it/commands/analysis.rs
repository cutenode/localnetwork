//! `doctor --analyze`, the check's loudness item and `analysis bench`, with analyses put in the
//! store's cache beforehand (as an earlier run would have left them), so nothing is decoded.

use locallibrary::harmony::Camelot;
use locallibrary::sound::{self, Analysis, Estimate, Loudness, VERSION};
use locallibrary::{Device, Store, paths};

use crate::support::{Env, line, path};

fn analysis(bpm: f32, key: Camelot, lufs: f64, clipped_runs: u64) -> Analysis {
    Analysis {
        version: VERSION,
        duration_secs: 120.0,
        estimate: Some(Estimate { bpm, bpm_confidence: 0.9, key, key_confidence: 0.8 }),
        loudness: Some(Loudness {
            integrated_lufs: Some(lufs),
            range_lu: 4.0,
            sample_peak: 1.0,
            true_peak: if clipped_runs > 0 { 1.2 } else { 0.9 },
            clipped_runs,
            clipped_samples: clipped_runs * 10,
        }),
    }
}

/// The demo drive with its first track heard at double its tempo in its parallel key, and its
/// second clipped (or with `agreeing`, both heard as stored, at one level).
fn analysed(env: &Env, agreeing: bool) -> std::path::PathBuf {
    let root = env.demo("A");
    let store = Store::open_or_init(&env.store).unwrap();
    let cache = sound::Cache::in_store(&store);
    let lib = Device::open(&root).unwrap().library().unwrap();
    for (i, t) in lib.tracks.iter().enumerate() {
        let key = Camelot::parse(&t.key).unwrap();
        let heard = match (agreeing, i) {
            (true, _) => analysis(t.bpm as f32, key, -8.0, 0),
            (false, 0) => analysis(t.bpm as f32 * 2.0, key.parallel(), -8.0, 0),
            (false, _) => analysis(t.bpm as f32, key, -8.0, 4),
        };
        let file = paths::to_fs(&root, &paths::key(&t.file_path));
        cache.put(&sound::Cache::key(&file).unwrap(), &heard).unwrap();
    }
    root
}

#[test]
fn doctor_lists_what_the_analysis_disagrees_with() {
    let env = Env::new();
    let root = analysed(&env, false);
    let out = env.ok(&["doctor", path(&root), "--only", "analysis"]);
    assert!(out.contains("Analysis (2 track(s) analysed)"), "{out}");
    assert!(line(&out, "Tempo (1)").contains("stored, then heard"));
    assert!(out.contains("(half"), "{out}");
    assert!(out.contains("(parallel)"), "{out}");
    assert!(line(&out, "Level (1").contains("median is -8.0 LUFS"), "{out}");
    assert!(out.contains("over 0 dBTP, clipped"), "{out}");
    assert!(out.contains("x2 and /2"), "{out}");

    let v = env.json(&["doctor", path(&root), "--analyze", "--only", "unfiled"]);
    assert_eq!(v["analysis"]["tempo"][0]["relation"], "half", "{v}");
    assert_eq!(v["analysis"]["key"][0]["relation"], "parallel");
    assert_eq!(v["analysis"]["loudness"][0]["problems"], serde_json::json!(["over_zero", "clipped"]));

    // The check before a set warns about the clipped track, and lists it with --verbose.
    let out = env.exits(0, &["check", path(&root), "--verbose"]);
    assert!(line(&out, "loudness").contains("1 track(s) are over 0 dBTP, clipped"), "{out}");
    assert!(out.contains("+1.6 dBTP, 4 clipped run(s)"), "{out}");
}

#[test]
fn bench_scores_rekordbox_and_the_analysis() {
    let env = Env::new();
    let root = analysed(&env, false);
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
    let out = env.ok(&args);
    assert!(line(&out, "rekordbox").contains("weighted 100.0%, correct 100.0%"), "{out}");
    assert!(line(&out, "stratum-dsp").contains("weighted 60.0%, correct 50.0%"), "{out}");
    assert!(out.contains("parallel 50.0%"), "{out}");
    assert!(out.contains("Accuracy1 50.0%, Accuracy2 100.0%"), "{out}");
    assert!(out.contains("double 50.0%"), "{out}");
    assert!(out.contains("1 annotated track(s) had no audio to score"), "{out}");

    let v = env.json(&args);
    assert_eq!(v["keys"]["rekordbox"]["tracks"], 2, "{v}");
    assert_eq!(v["accuracy2"]["stratum-dsp"], 100.0);
    assert_eq!(v["tempos"]["stratum-dsp"]["counts"]["double"], 1);

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
