use super::*;

fn heard(key: &str) -> Estimate {
    Estimate { bpm: 128.0, bpm_confidence: 1.0, key: Camelot::parse(key).unwrap(), key_confidence: 1.0 }
}

#[test]
fn keys_the_analysis_questions_are_less_sure() {
    assert_eq!(key_confidence("Am", &heard("8A")), None);
    assert_eq!(key_confidence("8A", &heard("8B")), Some(0.5));
    assert_eq!(key_confidence("8A", &heard("11B")), Some(0.5));
    assert_eq!(key_confidence("8A", &heard("9A")), Some(0.5));
    assert_eq!(key_confidence("8A", &heard("2B")), Some(0.25));
    assert_eq!(key_confidence("", &heard("8A")), None);
}

#[test]
fn the_median_of_an_even_count_is_between_the_middle_two() {
    let level = |lufs| Loudness {
        integrated_lufs: lufs,
        range_lu: 0.0,
        sample_peak: 0.5,
        true_peak: 0.5,
        clipped_runs: 0,
        clipped_samples: 0,
    };
    let levels = [level(Some(-8.0)), level(Some(-10.0)), level(None), level(Some(-6.0)), level(Some(-12.0))];
    assert_eq!(median_lufs(levels.iter()), Some(-9.0));
    assert_eq!(median_lufs(levels[..2].iter().chain(&levels[3..4])), Some(-8.0));
    assert_eq!(median_lufs(levels[2..3].iter()), None);
}
