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

use hegel::TestCase;
use hegel::generators as gs;

/// What an analyzer kept for a track, any role holding any kind of value.
#[hegel::test_helper]
fn some_value(tc: &TestCase) -> FieldValue {
    match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => FieldValue::Number(tc.draw(gs::sampled_from(vec![-14.0, -9.5, 64.0, 126.0, 128.0, 256.0]))),
        1 => FieldValue::Integer(tc.draw(gs::sampled_from(vec![-8i64, 3, 128]))),
        2 => FieldValue::String(tc.draw(gs::sampled_from(vec!["8A", "9B", "2B", "not a key"])).to_string()),
        3 => FieldValue::Null,
        _ => FieldValue::Boolean(true),
    }
}

/// Findings come only from what was heard: every track heard is analysed; a tempo heard as a
/// number that doesn't agree with the stored one, and a key heard as a key code the stored key
/// doesn't match, are reported; a track heard in none of the level roles has no level, and the
/// median is that of the loudness numbers heard.
#[hegel::test(test_cases = 200)]
fn prop_findings_come_only_from_what_was_heard(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>().min_value(1).max_value(5));
    let mut library = Library::default();
    let mut heard: FxHashMap<u32, RoleValues> = FxHashMap::default();
    for id in 1..=n {
        let mut t = loclib_model::test_util::track(id, "A", &format!("T{id}"), &format!("/Contents/{id}.mp3"));
        t.bpm = tc.draw(gs::sampled_from(vec![0.0, 128.0, 140.0]));
        t.key = tc.draw(gs::sampled_from(vec!["8A", "Am", "", "9B"])).to_string();
        library.tracks.push(t);
        if tc.draw(gs::booleans()) {
            let roles: Vec<Role> = tc.draw(gs::vecs(gs::sampled_from(ROLES.to_vec())).max_size(4));
            heard.insert(id, roles.into_iter().map(|r| (r, some_value(&tc))).collect());
        }
    }
    let r = findings(&library, &heard);
    assert_eq!(r.analyzed, heard.len());
    let as_number = |v: Option<&FieldValue>| match v {
        Some(FieldValue::Number(x)) => Some(*x),
        Some(FieldValue::Integer(x)) => Some(*x as f64),
        _ => None,
    };
    let mut tempo = Vec::new();
    let mut key = Vec::new();
    let mut lufs = Vec::new();
    for t in &library.tracks {
        let Some(h) = heard.get(&t.id) else { continue };
        if let Some(bpm) = as_number(h.get(&Role::Bpm))
            && TempoRelation::between(t.bpm, bpm).is_some_and(|r| r != TempoRelation::Agrees)
        {
            tempo.push(t.id);
        }
        let heard_in = match h.get(&Role::Key) {
            Some(FieldValue::String(code)) => Camelot::parse(code),
            _ => None,
        };
        if let (Some(stored), Some(k)) = (Camelot::parse(&t.key), heard_in)
            && KeyOutcome::of(stored, Some(k)) != KeyOutcome::Correct
        {
            key.push(t.id);
        }
        if LEVEL.iter().any(|r| h.contains_key(r)) {
            lufs.extend(as_number(h.get(&Role::Loudness)));
        }
    }
    assert_eq!(r.tempo.iter().map(|f| f.track.id).collect::<Vec<_>>(), tempo);
    assert_eq!(r.key.iter().map(|f| f.track.id).collect::<Vec<_>>(), key);
    lufs.sort_by(f64::total_cmp);
    let median = match lufs.len() {
        0 => None,
        n if n % 2 == 1 => Some(lufs[n / 2]),
        n => Some((lufs[n / 2 - 1] + lufs[n / 2]) / 2.0),
    };
    assert_eq!(r.median_lufs, median);
}
