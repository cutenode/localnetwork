//! Plugins' params, options and settings as people type them (flags on the command line, fields
//! in a form): text made into the JSON a contribution's schema asks for, and values shown as text.

use loclib_plugin::{CliHints, Setting, SettingType, is_path};
use serde_json::{Map, Value};

/// The properties of a params (or options) JSON Schema, in its order.
pub fn properties(schema: Option<&Value>) -> impl Iterator<Item = (&String, &Value)> {
    schema.and_then(|s| s["properties"].as_object()).into_iter().flatten()
}

/// How a param is given as text: a switch (a boolean), a value of a type (`string`, `number`,
/// `integer`), several of them (an array of those), or `KEY=VALUE` pairs of one (an object of
/// those). A param of another type is given as JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Takes<'a> {
    Switch,
    One(&'a str),
    Many(&'a str),
    Pairs(&'a str),
}

pub fn takes(schema: &Value) -> Option<Takes<'_>> {
    fn simple(s: &Value) -> Option<&str> {
        s["type"].as_str().filter(|t| ["string", "number", "integer"].contains(t))
    }
    match schema["type"].as_str()? {
        "boolean" => Some(Takes::Switch),
        "array" => simple(&schema["items"]).map(Takes::Many),
        "object" => simple(&schema["additionalProperties"]).map(Takes::Pairs),
        _ => simple(schema).map(Takes::One),
    }
}

/// The flag a param is given with: its schema's (see [`CliHints`]), else `--min-bpm` for
/// `min_bpm`.
pub fn flag(param: &str, schema: &Value) -> String {
    CliHints::of(schema).ok().and_then(|h| h.flag).unwrap_or_else(|| param.replace('_', "-"))
}

/// Whether a param is given without a flag (see [`CliHints::positional`]).
pub fn positional(schema: &Value) -> bool {
    CliHints::of(schema).is_ok_and(|h| h.positional)
}

/// What a value of the type `kind` is, in words.
fn a(kind: &str) -> &'static str {
    match kind {
        "number" => "a number",
        "integer" => "a whole number",
        _ => "text",
    }
}

/// `raw` as a JSON value of the type `kind` (`string`, `number`, `integer`), if it is one. A
/// whole number may be written as a track id is shown, `#812`.
fn typed(kind: &str, raw: &str) -> Option<Value> {
    match kind {
        "number" => raw.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map(Value::Number),
        "integer" => raw.strip_prefix('#').unwrap_or(raw).parse::<i64>().ok().map(Value::from),
        _ => Some(Value::from(raw)),
    }
}

pub fn yes_or_no(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" => Some(true),
        "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// A path as given, made absolute: a plugin runs in a folder of its own.
pub fn absolute(raw: &str) -> Result<Value, String> {
    let path = std::path::absolute(raw).map_err(|e| format!("where {raw} is: {e}"))?;
    Ok(Value::from(path.display().to_string()))
}

/// The value of a param (or option) whose JSON Schema is `schema`, from what was typed for it,
/// in order (`raws`): the last of them for one value, each for several, each `KEY=VALUE` (keys
/// counted from 1 when the schema's hints say so) for pairs, and JSON for another type. Files
/// (`x-path`) are made absolute. `what` names it in errors (`--times`, `the level option`).
pub fn value(schema: &Value, raws: &[&str], what: &str) -> Result<Value, String> {
    let hints = CliHints::of(schema).unwrap_or_default();
    let last = raws.last().map(|r| r.trim()).unwrap_or_default();
    let one = |kind: &str, raw: &str| -> Result<Value, String> {
        let raw = raw.trim();
        match is_path(schema) && kind == "string" {
            true => absolute(raw),
            false => typed(kind, raw).ok_or_else(|| format!("{what} takes {}, not {raw:?}", a(kind))),
        }
    };
    match takes(schema) {
        Some(Takes::Switch) => {
            yes_or_no(last).map(Value::Bool).ok_or_else(|| format!("{what} takes true or false, not {last:?}"))
        }
        Some(Takes::One(kind)) => one(kind, last),
        Some(Takes::Many(kind)) => raws.iter().map(|raw| one(kind, raw)).collect::<Result<_, _>>().map(Value::Array),
        Some(Takes::Pairs(kind)) => {
            let mut pairs = Map::new();
            for pair in raws {
                let (key, raw) = pair.split_once('=').ok_or_else(|| format!("{what} takes KEY=VALUE, not {pair:?}"))?;
                let given = key.trim();
                let key = match hints.one_based {
                    true => given.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).map(|n| n.to_string()),
                    false => Some(given.to_string()),
                };
                let key = key.ok_or_else(|| format!("{what}'s keys count from 1: {given:?} isn't one"))?;
                pairs.insert(key, one(kind, raw)?);
            }
            Ok(Value::Object(pairs))
        }
        None => serde_json::from_str(last).map_err(|_| format!("{what} takes JSON, not {last:?}")),
    }
}

/// A setting's value as typed: a number or a yes/no for those, an absolute path for a file or
/// folder, else the text (the host checks it's one of an enum's choices).
pub fn setting_value(s: &Setting, raw: &str) -> Result<Value, String> {
    let wrong = |what: &str| format!("{} is {what}, not {raw:?}", s.key);
    match s.kind {
        SettingType::Number => typed("number", raw.trim()).ok_or_else(|| wrong("a number")),
        SettingType::Boolean => yes_or_no(raw).map(Value::Bool).ok_or_else(|| wrong("true or false")),
        SettingType::File | SettingType::Directory => absolute(raw),
        _ => Ok(Value::from(raw)),
    }
}

/// A value as text: text as it is, a list joined, nothing as `-`.
pub fn cell(v: &Value) -> String {
    match v {
        Value::Null => "-".into(),
        Value::String(s) => s.clone(),
        Value::Array(list) => list.iter().map(cell).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests;
