//! JSON Schema in the property tests: values drawn in the shape a schema describes, and where a
//! value breaks one, by the `jsonschema` crate's validator (formats checked too).
//!
//! Shared by the library's and the CLI's tests (`#[path]`), so it only uses what both have.

#![allow(dead_code)]

use hegel::TestCase;
use hegel::generators as gs;
use jsonschema::Validator;
use serde_json::{Map, Value};

/// How deep a drawn value's arrays and objects go: the schemas are recursive.
const DEPTH: usize = 3;

/// A string for the property named `key` (`""` at the top), whose schema is the one given.
pub type Strings<'a> = &'a dyn Fn(&TestCase, &str, &Map<String, Value>) -> String;

/// The schema behind a `$ref` into `root`'s definitions.
pub fn resolved<'s>(root: &'s Value, schema: &'s Value) -> &'s Value {
    match schema.get("$ref").and_then(Value::as_str).and_then(|r| r.strip_prefix("#/$defs/")) {
        Some(name) => resolved(root, &root["$defs"][name]),
        None => schema,
    }
}

/// A value of the shape `schema` (its `$ref`s into `root`'s definitions) describes, for the
/// property `key`, `depth` arrays and objects down, mostly: now and then an object's required field
/// is left out, or a number is below the minimum a schema without one would allow, so some values
/// aren't what it describes.
#[hegel::test_helper]
pub fn draw(tc: &TestCase, root: &Value, schema: &Value, key: &str, depth: usize, strings: Strings) -> Value {
    // `true` (and anything that isn't an object) takes any value.
    let Some(s) = resolved(root, schema).as_object() else { return Value::Null };
    let pick = |n: usize| tc.draw(gs::integers::<usize>().max_value(n - 1));
    if let Some(c) = s.get("const") {
        return c.clone();
    }
    if let Some(Value::Array(choices)) = s.get("enum") {
        return choices[pick(choices.len())].clone();
    }
    for keyword in ["anyOf", "oneOf"] {
        if let Some(Value::Array(options)) = s.get(keyword) {
            return draw(tc, root, &options[pick(options.len())], key, depth, strings);
        }
    }
    // A flattened struct: its parts' fields, and then its own.
    let mut out = Map::new();
    for part in s.get("allOf").and_then(Value::as_array).into_iter().flatten() {
        if let Value::Object(m) = draw(tc, root, part, key, depth, strings) {
            out.extend(m);
        }
    }
    let types: Vec<&str> = match s.get("type") {
        Some(Value::String(t)) => vec![t.as_str()],
        Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).collect(),
        _ => vec!["object"],
    };
    let some: Vec<&str> = types.iter().copied().filter(|t| *t != "null").collect();
    let t = match some.is_empty() || (types.contains(&"null") && tc.draw(gs::weighted_booleans(0.3))) {
        true => "null",
        false => some[pick(some.len())],
    };
    let min = s.get("minimum").and_then(Value::as_i64).unwrap_or(-2);
    match t {
        "null" => Value::Null,
        "boolean" => tc.draw(gs::booleans()).into(),
        "integer" => tc.draw(gs::integers::<i64>().min_value(min).max_value(min.max(0) + 5)).into(),
        "number" => tc.draw(gs::floats::<f64>().min_value(-1.0).max_value(200.0)).into(),
        "string" => strings(tc, key, s).into(),
        "array" if depth < DEPTH => {
            let items = s.get("items").unwrap_or(&Value::Bool(true));
            (0..pick(3)).map(|_| draw(tc, root, items, key, depth + 1, strings)).collect()
        }
        "array" => Value::Array(Vec::new()),
        _ if depth >= DEPTH => Value::Object(out),
        _ => {
            let required: Vec<&str> =
                s.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
            for (k, sub) in s.get("properties").and_then(Value::as_object).into_iter().flatten() {
                let keep = if required.contains(&k.as_str()) { 0.95 } else { 0.5 };
                if tc.draw(gs::weighted_booleans(keep)) {
                    out.insert(k.clone(), draw(tc, root, sub, k, depth + 1, strings));
                }
            }
            if let Some(extra) = s.get("additionalProperties").filter(|a| a.is_object())
                && tc.draw(gs::booleans())
            {
                let k = strings(tc, "keys", &Map::new());
                out.insert(k, draw(tc, root, extra, "values", depth + 1, strings));
            }
            Value::Object(out)
        }
    }
}

/// How the tests build validators: formats are checked too.
pub fn options<'i>() -> jsonschema::ValidationOptions<'i> {
    jsonschema::options().should_validate_formats(true)
}

/// Where `value` breaks what `validator` checks, a line a place, or `None` where it doesn't.
pub fn violations(validator: &Validator, value: &Value) -> Option<String> {
    let errors: Vec<String> = validator.iter_errors(value).map(|e| format!("{}: {e}", e.instance_path())).collect();
    (!errors.is_empty()).then(|| errors.join("\n"))
}
