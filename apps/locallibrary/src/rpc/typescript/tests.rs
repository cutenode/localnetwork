use serde_json::json;

use super::*;

#[test]
fn schemas_become_types() {
    let t = |s: Value| ty(&s, 0);
    assert_eq!(t(json!({ "type": ["string", "null"] })), "string | null");
    assert_eq!(t(json!({ "$ref": "#/$defs/Hash" })), "Hash");
    assert_eq!(
        t(
            json!({ "$ref": "#/$defs/A", "type": "object", "properties": { "event": { "const": "a.b", "type": "string" } }, "required": ["event"] })
        ),
        "A & {\n  event: \"a.b\";\n}"
    );
    assert_eq!(t(json!({ "enum": ["a", "b"] })), "\"a\" | \"b\"");
    assert_eq!(
        t(json!({ "type": "array", "items": { "anyOf": [{ "type": "integer" }, { "type": "null" }] } })),
        "(number | null)[]"
    );
    assert_eq!(
        t(
            json!({ "type": "array", "prefixItems": [{ "type": "string", "title": "path" }, { "type": "string", "title": "error" }] })
        ),
        "[path: string, error: string]"
    );
    assert_eq!(
        t(
            json!({ "type": "object", "properties": { "a-b": { "type": "boolean", "description": "Yes." }, "c": { "type": "integer" } }, "required": ["c"] })
        ),
        "{\n  /** Yes. */\n  \"a-b\"?: boolean;\n  c: number;\n}"
    );
    assert_eq!(
        t(json!({ "type": "object", "additionalProperties": { "type": "integer" } })),
        "{ [key: string]: number }"
    );
    assert_eq!(
        t(
            json!({ "allOf": [{ "$ref": "#/$defs/A" }, { "anyOf": [{ "$ref": "#/$defs/B" }, { "$ref": "#/$defs/C" }] }] })
        ),
        "A & (B | C)"
    );
    assert_eq!(t(json!(true)), "unknown");
    assert_eq!(
        t(json!({ "oneOf": [{ "type": "object", "properties": { "kind": { "const": "x" } }, "required": ["kind"] }] })),
        "{\n  kind: \"x\";\n}"
    );
}

/// Schemas outside what schemars writes for the API still give TypeScript that compiles:
/// whatever isn't understood is `unknown`, nothing impossible is `never`.
#[test]
fn odd_schemas_still_make_valid_types() {
    let t = |s: Value| ty(&s, 0);
    assert_eq!(t(json!(false)), "never");
    assert_eq!(t(json!(3)), "unknown");
    assert_eq!(t(json!({ "enum": [] })), "never");
    assert_eq!(t(json!({ "type": "decimal" })), "unknown");
    assert_eq!(t(json!({ "type": 7 })), "never");
    assert_eq!(t(json!({ "anyOf": [{ "type": "string" }, true] })), "unknown");
    assert_eq!(t(json!({ "properties": {} })), "{ [key: string]: unknown }");
    assert_eq!(t(json!({ "type": "object", "additionalProperties": false })), "Record<string, never>");
    assert_eq!(
        t(
            json!({ "properties": { "a": { "type": "string" } }, "required": ["a"], "additionalProperties": { "type": "integer" } })
        ),
        "{\n  a: string;\n} & { [key: string]: number }"
    );
    assert_eq!(t(json!({ "type": "array", "prefixItems": [{ "type": "integer" }] })), "[number]");
    // A `|` inside a string literal doesn't make a type a union; one between types does.
    assert_eq!(tight("\"a\\\"|b\"".into()), "\"a\\\"|b\"");
    assert_eq!(tight("/* a | b */ string".into()), "/* a | b */ string");
    assert_eq!(tight("string | null".into()), "(string | null)");
    assert_eq!(ident("1st-thing"), "_1st_thing");
    assert_eq!(key("a-b"), "\"a-b\"");
}

#[test]
fn descriptions_become_doc_comments() {
    let mut out = String::new();
    doc(&mut out, &json!({ "description": "First.\n\nSecond, about */ comments." }), "  ");
    assert_eq!(out, "  /**\n   * First.\n   *\n   * Second, about *\\/ comments.\n   */\n");
    let mut out = String::new();
    doc(&mut out, &json!({ "description": "  " }), "");
    assert!(out.is_empty());
}

/// The error's `data` gets a type of its own, unless it's a definition already.
#[test]
fn the_error_data_is_declared_once() {
    let contract = json!({ "$defs": {}, "methods": [], "error": { "type": "object", "properties": { "kind": { "type": "string" } } } });
    assert!(declarations(&contract).contains("export type ErrorData = {\n  kind?: string;\n};\n"));
    let defined = json!({ "$defs": { "ErrorData": { "type": "object" } }, "error": { "$ref": "#/$defs/ErrorData" } });
    assert_eq!(declarations(&defined).matches("export type ErrorData").count(), 1);
}

/// Names a schema might give a property or a definition: identifiers and not.
const NAMES: &[&str] = &["a", "kind", "a b", "1st", "$x", "with-dash", "é", "_ok", "quote\"d"];

/// A schema of the kinds schemars writes, nested up to `depth` deep, with names, constants
/// and descriptions that need quoting or escaping.
#[hegel::test_helper]
fn schema(tc: &hegel::TestCase, depth: usize) -> Value {
    use hegel::generators as gs;
    let name = |tc: &hegel::TestCase| NAMES[tc.draw(gs::integers::<usize>().max_value(NAMES.len() - 1))];
    let kind = tc.draw(gs::integers::<u8>().max_value(if depth == 0 { 6 } else { 12 }));
    let mut s = match kind {
        0 => json!({ "type": tc.draw(gs::sampled_from(&["string", "integer", "number", "boolean", "null"][..])) }),
        1 => json!({ "type": ["string", "null"] }),
        2 => json!({ "const": name(tc) }),
        3 => json!({ "enum": [name(tc), 1, null] }),
        4 => json!({ "$ref": format!("#/$defs/{}", name(tc)) }),
        5 => json!(tc.draw(gs::booleans())),
        6 => json!({}),
        7 | 8 => {
            let mut props = Map::new();
            let mut required = Vec::new();
            for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
                let n = name(tc);
                props.insert(n.into(), schema(tc, depth - 1));
                if tc.draw(gs::booleans()) {
                    required.push(n);
                }
            }
            let mut o = json!({ "type": "object", "properties": props, "required": required });
            if tc.draw(gs::booleans()) {
                o["additionalProperties"] = schema(tc, depth - 1);
            }
            o
        }
        9 => json!({ "type": "array", "items": schema(tc, depth - 1) }),
        10 => {
            json!({ "type": "array", "prefixItems": [schema(tc, depth - 1), { "type": "string", "title": name(tc) }] })
        }
        11 => {
            let key = tc.draw(gs::sampled_from(&["anyOf", "oneOf", "allOf"][..]));
            let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
            json!({ key: (0..n).map(|_| schema(tc, depth - 1)).collect::<Vec<_>>() })
        }
        _ => json!({ "type": "object", "additionalProperties": schema(tc, depth - 1) }),
    };
    if s.is_object() && tc.draw(gs::weighted_booleans(0.3)) {
        s["description"] = tc.draw(gs::sampled_from(&["One line.", "Two\n\nlines, a */ in one", " "][..])).into();
    }
    s
}

/// Whether `t`'s brackets close in order, outside string literals and comments.
fn balanced(t: &str) -> bool {
    let mut open = Vec::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => drop(chars.next()),
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                let mut last = ' ';
                for c in chars.by_ref() {
                    if last == '*' && c == '/' {
                        break;
                    }
                    last = c;
                }
            }
            '(' | '[' | '{' | '<' => open.push(c),
            ')' | ']' | '}' | '>' => {
                let want = match c {
                    ')' => '(',
                    ']' => '[',
                    '}' => '{',
                    _ => '<',
                };
                if open.pop() != Some(want) {
                    return false;
                }
            }
            _ => {}
        }
    }
    open.is_empty()
}

/// Any schema becomes a type whose brackets close, with each of an object's properties
/// declared under its name (quoted when it isn't an identifier), optional unless required;
/// identifiers made of names are identifiers.
#[hegel::test(test_cases = 400)]
fn any_schema_becomes_a_well_formed_type(tc: hegel::TestCase) {
    let s = schema(&tc, 3);
    let t = ty(&s, 0);
    assert!(balanced(&t), "{t}\nfrom {s}");
    assert!(!t.is_empty());
    let tight_t = tight(t.clone());
    assert!(tight_t == t || tight_t == format!("({t})"), "{tight_t}");
    if s["type"] == "object"
        && let Some(props) = s["properties"].as_object()
    {
        let required: Vec<&Value> = s["required"].as_array().into_iter().flatten().collect();
        for name in props.keys() {
            let optional = if required.contains(&&Value::String(name.clone())) { "" } else { "?" };
            let declared = format!("  {}{optional}: ", key(name));
            assert!(t.contains(&declared), "{declared:?} isn't in\n{t}");
        }
    }
    for name in NAMES {
        let id = ident(name);
        assert!(id.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_'), "{id}");
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "{id}");
        let k = key(name);
        let back: String = if k.starts_with('"') { serde_json::from_str(&k).unwrap() } else { k.clone() };
        assert_eq!(back, *name, "{k}");
    }
}

/// Declarations for any of the sidecar's methods declare each of them, and each old name,
/// once among the methods, and every definition once as a type.
#[hegel::test(test_cases = 40)]
fn declarations_name_each_method_once(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let full = super::super::contract::contract();
    let all = full["methods"].as_array().unwrap();
    let indices: Vec<usize> = tc.draw(gs::subsequences((0..all.len()).collect::<Vec<_>>()).max_size(8));
    let picked: Vec<Value> = indices.into_iter().map(|i| all[i].clone()).collect();
    // Only the definitions they use, as `rpc.describe` gives them: all of them take long.
    let mut keep = std::collections::BTreeSet::new();
    let mut todo = vec![Value::Array(picked.clone()), full["notifications"].clone(), full["error"].clone()];
    while let Some(v) = todo.pop() {
        match v {
            Value::Object(map) => {
                if let Some(name) = map.get("$ref").and_then(Value::as_str).and_then(|r| r.strip_prefix("#/$defs/"))
                    && keep.insert(name.to_string())
                {
                    todo.push(full["$defs"][name].clone());
                }
                todo.extend(map.into_iter().map(|(_, v)| v));
            }
            Value::Array(list) => todo.extend(list),
            _ => {}
        }
    }
    let mut contract = full.clone();
    contract["methods"] = Value::Array(picked.clone());
    contract["$defs"] = full["$defs"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(k, _)| keep.contains(*k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let d = declarations(&contract);
    assert!(balanced(&d));
    let methods = d.split("export interface Methods {").nth(1).unwrap().split("\n}\n").next().unwrap();
    for m in &picked {
        let names = std::iter::once(&m["name"]).chain(m["aliases"].as_array().into_iter().flatten());
        for name in names.filter_map(Value::as_str) {
            let entry = format!("\n  {}: {{ params: ", key(name));
            assert_eq!(methods.matches(&entry).count(), 1, "{name}");
        }
    }
    let entries = methods.matches(": { params: ").count();
    let expected: usize = picked.iter().map(|m| 1 + m["aliases"].as_array().map_or(0, Vec::len)).sum();
    assert_eq!(entries, expected);
    for name in contract["$defs"].as_object().unwrap().keys() {
        let declared = format!("export type {} =", ident(name));
        assert_eq!(d.lines().filter(|l| l.starts_with(&declared)).count(), 1, "{name}");
    }
}
