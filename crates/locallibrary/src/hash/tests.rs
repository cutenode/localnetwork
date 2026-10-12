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
