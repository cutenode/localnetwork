use std::path::Path;

use serde_json::json;

use super::*;

#[test]
fn flags_are_named_by_params_or_their_hints() {
    assert_eq!(flag("min_bpm", &json!({ "type": "number" })), "min-bpm");
    assert_eq!(flag("playlist", &json!({ "type": "string", "x-cli": { "flag": "from" } })), "from");
}

#[test]
fn settings_take_their_types() {
    let setting =
        |kind: SettingType| Setting { key: "k".into(), title: "K".into(), kind, default: None, choices: Vec::new() };
    assert_eq!(setting_value(&setting(SettingType::Number), "1.5").unwrap(), json!(1.5));
    assert_eq!(setting_value(&setting(SettingType::Boolean), "off").unwrap(), json!(false));
    assert_eq!(setting_value(&setting(SettingType::String), "hi").unwrap(), json!("hi"));
    let file = setting_value(&setting(SettingType::File), "x.txt").unwrap();
    assert!(Path::new(file.as_str().unwrap()).is_absolute());
    assert_eq!(setting_value(&setting(SettingType::Number), "lots").unwrap_err(), "k is a number, not \"lots\"");
    assert_eq!(
        setting_value(&setting(SettingType::Boolean), "maybe").unwrap_err(),
        "k is true or false, not \"maybe\""
    );
}

#[test]
fn cells_read_as_text() {
    assert_eq!(cell(&json!(null)), "-");
    assert_eq!(cell(&json!("a")), "a");
    assert_eq!(cell(&json!(["a", 2])), "a, 2");
    assert_eq!(cell(&json!({ "a": 1 })), "{\"a\":1}");
}
