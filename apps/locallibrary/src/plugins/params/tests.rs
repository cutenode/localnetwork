use std::path::Path;

use hegel::TestCase;
use hegel::generators as gs;
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

/// Text a person might type for a value: numbers (whole, with a point, as a track id), yes and
/// no in any case and with spaces, JSON, `KEY=VALUE`, paths and words.
#[hegel::test_helper]
fn typed_text(tc: &TestCase) -> String {
    match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => tc.draw(gs::integers::<i64>().min_value(-1000).max_value(1000)).to_string(),
        1 => format!("{:?}", tc.draw(gs::floats::<f64>().min_value(-1e3).max_value(1e3))),
        2 => format!("#{}", tc.draw(gs::integers::<u32>().max_value(999))),
        3 => tc.draw(gs::sampled_from(vec![" yes", "No ", "ON", "off", "true", "FALSE", "maybe", ""])).to_string(),
        _ => {
            let words = vec!["k=1", "a=b=c", "[1, 2]", "{\"a\": 1}", "\"x\"", "null", "list.txt", "Peak Time"];
            tc.draw(gs::sampled_from(words)).to_string()
        }
    }
}

/// `raw` as a value of `kind` (`string`, `number`, `integer`), as a model has it.
fn modelled(kind: &str, raw: &str) -> Option<Value> {
    match kind {
        "integer" => raw.strip_prefix('#').unwrap_or(raw).parse::<i64>().ok().map(Value::from),
        "number" => match raw.parse::<i64>() {
            Ok(n) => Some(json!(n)),
            Err(_) => raw.parse::<f64>().ok().filter(|f| f.is_finite()).map(|f| json!(f)),
        },
        _ => Some(json!(raw)),
    }
}

/// Whether `raw` says yes or no, as a model has it.
fn modelled_switch(raw: &str) -> Option<bool> {
    match raw.trim().to_lowercase().as_str() {
        "true" | "yes" | "on" => Some(true),
        "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Whatever is typed for a setting of any kind, it's taken as that kind says (a number, yes or
/// no, an absolute path, else the text as it is), or refused naming the setting, what it takes
/// and what was typed.
#[hegel::test(test_cases = 1000)]
fn any_setting_typed_is_taken_as_its_kind_says(tc: TestCase) {
    let kinds = vec![
        SettingType::String,
        SettingType::Number,
        SettingType::Boolean,
        SettingType::Enum,
        SettingType::File,
        SettingType::Directory,
        SettingType::Secret,
    ];
    let kind = tc.draw(gs::sampled_from(kinds));
    let s = Setting { key: "k".into(), title: "K".into(), kind, default: None, choices: Vec::new() };
    // A yes or a no, mostly, for a switch.
    let raw = match kind == SettingType::Boolean && tc.draw(gs::booleans()) {
        true => tc.draw(gs::sampled_from(vec!["yes", "no", "on", "off", "true", "false", " Off ", "YES"])).to_string(),
        false => typed_text(&tc),
    };
    let got = setting_value(&s, &raw);
    let (want, takes) = match kind {
        SettingType::Number => (modelled("number", raw.trim()), "a number"),
        SettingType::Boolean => (modelled_switch(&raw).map(Value::Bool), "true or false"),
        SettingType::File | SettingType::Directory => {
            // Nothing typed names no path.
            if raw.is_empty() {
                assert!(got.unwrap_err().starts_with("where  is: "));
                return;
            }
            let path = got.unwrap();
            let path = path.as_str().unwrap();
            // Windows drops a name's trailing spaces and dots: "No " is the file "No" there, and
            // that's the one a plugin would open with it.
            let name = if cfg!(windows) { raw.trim_end_matches([' ', '.']) } else { raw.as_str() };
            let named = Path::new(path).file_name().and_then(|n| n.to_str());
            assert!(Path::new(path).is_absolute() && named == Some(name), "{raw:?}: {path}");
            return;
        }
        _ => (Some(json!(raw)), ""),
    };
    match want {
        Some(v) => assert_eq!(got, Ok(v), "{kind:?} {raw:?}"),
        None => assert_eq!(got, Err(format!("k is {takes}, not {raw:?}")), "{kind:?}"),
    }
}

/// Whatever is typed, once or more, for a param of any schema: a switch is the last typed, yes or
/// no; one value the last typed, of its type; several each of them; pairs each `KEY=VALUE`, the
/// last of a key winning; and any other type the last typed, as JSON. What can't be is refused,
/// naming the param, what it takes and what was typed.
#[hegel::test(test_cases = 1000)]
fn any_param_typed_is_taken_as_its_schema_says(tc: TestCase) {
    let kind = tc.draw(gs::sampled_from(vec!["string", "number", "integer"]));
    let (schema, shape) = match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => (json!({ "type": "boolean" }), "switch"),
        1 => (json!({ "type": kind }), "one"),
        2 => (json!({ "type": "array", "items": { "type": kind } }), "many"),
        3 => (json!({ "type": "object", "additionalProperties": { "type": kind } }), "pairs"),
        _ => (json!({ "type": tc.draw(gs::sampled_from(vec!["object", "array"])) }), "json"),
    };
    let typed: Vec<String> =
        (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3))).map(|_| typed_text(&tc)).collect();
    let raws: Vec<&str> = typed.iter().map(String::as_str).collect();
    let last = raws.last().unwrap().trim();
    let takes = match kind {
        "number" => "a number",
        "integer" => "a whole number",
        _ => "text",
    };
    let one = |raw: &str| modelled(kind, raw.trim()).ok_or_else(|| format!("--x takes {takes}, not {:?}", raw.trim()));
    let want: Result<Value, String> = match shape {
        "switch" => {
            modelled_switch(last).map(Value::Bool).ok_or_else(|| format!("--x takes true or false, not {last:?}"))
        }
        "one" => one(last),
        "many" => raws.iter().map(|r| one(r)).collect::<Result<Vec<_>, _>>().map(Value::Array),
        "pairs" => raws
            .iter()
            .map(|pair| {
                let (key, raw) = pair.split_once('=').ok_or_else(|| format!("--x takes KEY=VALUE, not {pair:?}"))?;
                Ok((key.trim().to_string(), one(raw)?))
            })
            .collect::<Result<Map<_, _>, String>>()
            .map(Value::Object),
        _ => serde_json::from_str(last).map_err(|_| format!("--x takes JSON, not {last:?}")),
    };
    assert_eq!(value(&schema, &raws, "--x"), want, "{schema} {raws:?}");
}
