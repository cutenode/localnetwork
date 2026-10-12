use super::*;

#[test]
fn schema_is_consistent() {
    let mut keys = rustc_hash::FxHashSet::default();
    for def in SCHEMA {
        assert!(keys.insert(def.key), "duplicate key {}", def.key);
        assert!(def.offset < def.file.data_len(), "{} offset out of range", def.key);
        let _ = def.default_choice();
        let mut bytes: Vec<u8> = def.choices.iter().map(|c| c.byte).collect();
        bytes.sort_unstable();
        bytes.dedup();
        assert_eq!(bytes.len(), def.choices.len(), "{} has duplicate bytes", def.key);
    }
    // No two settings share a byte.
    let mut slots = rustc_hash::FxHashSet::default();
    for def in SCHEMA {
        assert!(slots.insert((def.file, def.offset)), "{} overlaps another setting", def.key);
    }
}

#[test]
fn lenient_lookup() {
    assert_eq!(validate("auto-cue-level", "-60dB").unwrap().1.id, "-60db");
    assert_eq!(validate("crossfader_curve", "Slow Cut").unwrap().1.id, "slow-cut");
    assert_eq!(validate("language", "Deutsch").unwrap().1.id, "german");
    assert!(validate("quantize", "maybe").is_err());
    assert!(validate("nope", "on").is_err());
}
