use super::*;

fn heard(key: &str) -> Camelot {
    Camelot::parse(key).unwrap()
}

#[test]
fn keys_the_analysis_questions_are_less_sure() {
    assert_eq!(key_confidence("Am", heard("8A")), None);
    assert_eq!(key_confidence("8A", heard("8B")), Some(0.5));
    assert_eq!(key_confidence("8A", heard("11B")), Some(0.5));
    assert_eq!(key_confidence("8A", heard("9A")), Some(0.5));
    assert_eq!(key_confidence("8A", heard("2B")), Some(0.25));
    assert_eq!(key_confidence("", heard("8A")), None);
}

#[test]
fn the_median_of_an_even_count_is_between_the_middle_two() {
    let level = |lufs| Level { integrated_lufs: lufs, true_peak_dbtp: -6.0, clipped_runs: 0 };
    let levels = [level(Some(-8.0)), level(Some(-10.0)), level(None), level(Some(-6.0)), level(Some(-12.0))];
    assert_eq!(median_lufs(levels.iter()), Some(-9.0));
    assert_eq!(median_lufs(levels[..2].iter().chain(&levels[3..4])), Some(-8.0));
    assert_eq!(median_lufs(levels[2..3].iter()), None);
}

/// A level is made of whichever of its roles were worked out: a true peak left out (or null, for
/// silence) is minus infinity, clipped runs left out none. With none of them there's no level.
#[test]
fn a_level_is_read_from_its_roles() {
    let heard = |pairs: &[(Role, FieldValue)]| pairs.iter().cloned().collect::<RoleValues>();
    let silent = heard(&[(Role::Loudness, FieldValue::Null), (Role::TruePeak, FieldValue::Null)]);
    assert_eq!(
        Level::of(&silent),
        Some(Level { integrated_lufs: None, true_peak_dbtp: f64::NEG_INFINITY, clipped_runs: 0 })
    );
    let hot = heard(&[(Role::TruePeak, FieldValue::Number(1.5)), (Role::ClippedRuns, FieldValue::Integer(4))]);
    assert_eq!(Level::of(&hot), Some(Level { integrated_lufs: None, true_peak_dbtp: 1.5, clipped_runs: 4 }));
    assert_eq!(Level::of(&heard(&[(Role::Key, FieldValue::String("8A".into()))])), None);
}
