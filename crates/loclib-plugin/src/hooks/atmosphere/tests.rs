use super::*;

/// A manifest contributes `atmosphere` exactly when it asks to write a record type; reading the
/// library alone doesn't.
#[test]
fn writing_records_is_what_contributes_the_feature() {
    let head = "id = \"demo\"\nname = \"Demo\"\nversion = \"0.1.0\"\nprotocol = \"^0.2\"\n";
    let with = |caps: &str| Manifest::parse(&format!("{head}capabilities = [{caps}]\n")).unwrap();
    assert!((FEATURE.contributed)(&with("\"library:read\", \"atmosphere:write:fm.plyr.track\"")));
    assert!(!(FEATURE.contributed)(&with("\"library:read\"")));
}
