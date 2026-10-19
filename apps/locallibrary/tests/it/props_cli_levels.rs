//! Property tests of the levels `check` reports: drive A's two tracks as synthetic audio at drawn
//! gains (quiet, ordinary, clipped), analysed by `doctor --analyze`, then checked. The tracks the
//! printed check lists are the ones its `--json` finds a problem with, each with its level.

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use loclib_mixid::test_util::{synth, wav_of};
use serde_json::Value;

use crate::props_cli_human::{TRACK_1, TRACK_2};
use crate::support::Env;

/// Gains for the test signal: far below the others, ordinary, and well past full scale (clipped).
const GAINS: [f32; 3] = [300.0, 12_000.0, 400_000.0];

/// A 16-bit WAV of two seconds of test signal `seed` at `gain`, cut off at full scale.
fn wav(seed: u32, gain: f32) -> Vec<u8> {
    let samples = synth(2.0, seed).into_iter().map(|s| ((s * gain) as i32).clamp(-32_768, 32_767));
    wav_of(11_025, 16, samples.collect::<Vec<_>>().into_iter())
}

/// Drive A's tracks at any gains, analysed: `check --verbose` lists, under its `loudness` item,
/// each track its JSON finds over 0 dBTP, clipped or far from the others' level, with its
/// integrated loudness, true peak and clipped runs as the JSON has them; without `--verbose` it
/// lists none and says `--verbose` would; and the item's level says whether any track was found.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn check_lists_the_tracks_whose_level_is_a_problem(tc: TestCase) {
    let fx = Env::from_template();
    let a = fx.volumes.join("A");
    for (track, seed) in [(TRACK_1, 7919), (TRACK_2, 31)] {
        let gain = tc.draw(gs::sampled_from(&GAINS[..]));
        std::fs::write(a.join(track), wav(seed, gain)).unwrap();
    }
    // Two seconds of audio where the drive says minutes: a problem doctor reports, after analysing.
    let doctor = fx.run(&["doctor", "A", "--analyze"]);
    assert!(doctor.out.contains("2 track(s) analysed"), "{}\n{}", doctor.out, doctor.err);

    // Rewriting the audio is a change the store hasn't seen, which may be a problem of its own.
    let json = fx.run(&["--json", "check", "A"]);
    let v: Value = serde_json::from_str(&json.out).unwrap_or_else(|e| panic!("{e}: {}\n{}", json.out, json.err));
    let verbose = fx.run(&["check", "A", "--verbose"]);
    let terse = fx.run(&["check", "A"]);
    let said = format!("{}\n{}\n{v}", verbose.out, verbose.err);
    assert_eq!((verbose.code, terse.code), (json.code, json.code), "{said}");
    let items = v["items"].as_array().unwrap_or_else(|| panic!("no items: {said}"));
    let item = items.iter().find(|i| i["check"] == "loudness");
    let item = item.unwrap_or_else(|| panic!("no loudness item: {said}"));
    let found: Vec<&Value> =
        v["loudness"].as_array().unwrap().iter().flat_map(|d| d["tracks"].as_array().unwrap()).collect();
    assert_eq!(item["level"] == "ok", found.is_empty(), "{said}");
    let listed: Vec<&str> = verbose.out.lines().filter(|l| l.starts_with(&" ".repeat(18))).collect();
    assert_eq!(listed.len(), found.len(), "{said}");
    for f in &found {
        let lufs = f["integrated_lufs"].as_f64().map_or("silent".to_string(), |l| format!("{l:.1} LUFS"));
        let line = format!(
            "{}{}  {lufs}, {:+.1} dBTP, {} clipped run(s)",
            " ".repeat(18),
            f["track"]["name"].as_str().unwrap(),
            f["true_peak_dbtp"].as_f64().unwrap(),
            f["clipped_runs"]
        );
        assert!(listed.contains(&line.as_str()), "{line:?}: {said}");
    }
    assert!(!terse.out.lines().any(|l| l.starts_with(&" ".repeat(18))), "{}", terse.out);
    assert_eq!(terse.out.contains("--verbose lists the tracks."), !found.is_empty(), "{}", terse.out);
}
