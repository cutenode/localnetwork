//! Lenient accessors for the JSON that `lsblk`, `plutil` and PowerShell produce (numbers and
//! booleans sometimes arrive as strings, single-element arrays as bare objects).

use serde_json::Value;

/// `v[key]` as text: a string trimmed (`None` when that leaves nothing, or a lone NUL), or a
/// number written out.
pub fn str(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty() && s != "\0"),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// `v[key]` as a count: a number, or a string holding one.
pub fn u64(v: &Value, key: &str) -> Option<u64> {
    match v.get(key)? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// `v[key]` as a flag: `true`, `1`, or a string saying either. Anything else, missing
/// included, is `false`.
pub fn bool(v: &Value, key: &str) -> bool {
    match v.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => matches!(s.trim(), "1" | "true" | "True"),
        Some(Value::Number(n)) => n.as_u64() == Some(1),
        _ => false,
    }
}

/// `v`'s items: an array's, an object's on its own, or none.
pub fn array(v: Option<&Value>) -> Vec<&Value> {
    match v {
        Some(Value::Array(a)) => a.iter().collect(),
        // Windows PowerShell 5.1 sometimes wraps arrays as {"value": [...], "Count": n}.
        Some(Value::Object(o)) if o.get("value").is_some_and(Value::is_array) => array(o.get("value")),
        Some(o @ Value::Object(_)) => vec![o],
        _ => Vec::new(),
    }
}
