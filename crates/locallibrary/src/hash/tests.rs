use super::*;

/// The JSON schema says what a hash serialises to, and logs show its short form.
#[test]
fn the_schema_matches_the_serialised_form() {
    let h = Hash::of(b"audio");
    let json = serde_json::to_value(h).unwrap();
    let hex = json.as_str().unwrap();
    assert!(hex.len() == 64 && hex.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')), "{hex}");
    let schema = schemars::schema_for!(Hash);
    assert_eq!(schema.get("pattern").and_then(|p| p.as_str()), Some("^[0-9a-f]{64}$"));
    assert_eq!(format!("{h:?}"), format!("Hash({})", &hex[..12]));
    assert!(serde_json::from_str::<Hash>("\"not a hash\"").is_err());
}

/// A hash reads back from its hex in either case, its short form starts it, and it shows as
/// `Hash(short)` when debugged.
#[hegel::test(test_cases = 100)]
fn a_hash_reads_back_from_its_hex(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let bytes: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>()).max_size(64));
    let h = Hash::of(&bytes);
    let hex = h.to_hex();
    assert_eq!(hex.len(), 64);
    assert!(hex.starts_with(&h.short()));
    let upper = tc.draw(gs::booleans());
    let text = if upper { hex.to_uppercase() } else { hex.clone() };
    assert_eq!(text.parse::<Hash>().ok(), Some(h));
    assert_eq!(format!("{h:?}"), format!("Hash({})", h.short()));
    assert_eq!(h.to_string(), hex);
}
