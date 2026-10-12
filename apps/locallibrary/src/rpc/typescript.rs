//! TypeScript declarations from the [contract](super::contract): a type for every definition, and
//! `Methods`, mapping each method's name (aliases included) to its `params` and `result`.
//!
//! It covers the JSON Schema that schemars writes for the API's types, not JSON Schema in
//! general: `$ref`, `const`, `enum`, `type` (one or several), objects with `properties`,
//! `required` and `additionalProperties`, arrays with `items` or `prefixItems` (tuples, labelled
//! by each element's `title`), `allOf` (an intersection), `anyOf` and `oneOf` (a union).
//! Anything else is `unknown`. The output depends only on the contract, so it's the same on
//! every run.

use serde_json::{Map, Value};

/// The `.d.ts` file for `contract`.
pub fn declarations(contract: &Value) -> String {
    let mut out = String::new();
    out.push_str(
        "// The locallibrary RPC sidecar's types, made from schema.json by `locallibrary rpc --emit-schema`.\n\
         // Don't edit: regenerate it.\n\n",
    );
    if let Some(protocol) = contract["protocol"].as_u64() {
        out.push_str(&format!("/** The protocol these types describe (`initialize` answers it). */\nexport const PROTOCOL = {protocol};\n\n"));
    }
    let empty = Map::new();
    let defs = contract["$defs"].as_object().unwrap_or(&empty);
    for (name, schema) in defs {
        doc(&mut out, schema, "");
        out.push_str(&format!("export type {} = {};\n\n", ident(name), ty(schema, 0)));
    }

    out.push_str(
        "/** Every method, by name (aliases included): its parameters and result. */\nexport interface Methods {\n",
    );
    for m in contract["methods"].as_array().into_iter().flatten() {
        let params = ty(&m["params"], 2);
        let result = ty(&m["result"], 2);
        let mut lines = Vec::new();
        lines.push(format!("{} method", m["kind"].as_str().unwrap_or("")));
        if let Some(pair) = m["pair"].as_str() {
            lines.push(format!("paired with `{pair}`"));
        }
        if m["long_running"] == true {
            lines.push("long running: sends `progress`, can be cancelled".into());
        }
        if m["destructive"] == true {
            lines.push("can delete or overwrite something".into());
        }
        let names = std::iter::once(&m["name"]).chain(m["aliases"].as_array().into_iter().flatten());
        for (i, name) in names.filter_map(Value::as_str).enumerate() {
            let mut lines = lines.clone();
            if i > 0 {
                lines.push(format!("an older name for `{}`", m["name"].as_str().unwrap_or("")));
            }
            if let Some(instead) = m["deprecated"].as_str() {
                lines.push(format!("@deprecated use `{instead}`"));
            }
            out.push_str(&format!("  /** {} */\n", lines.join("; ")));
            out.push_str(&format!("  {}: {{ params: {params}; result: {result} }};\n", key(name)));
        }
    }
    out.push_str("}\n\n");
    out.push_str(
        "export type MethodName = keyof Methods;\n\
         export type Params<M extends MethodName> = Methods[M][\"params\"];\n\
         export type Result<M extends MethodName> = Methods[M][\"result\"];\n\n",
    );
    out.push_str("/** Notifications the sidecar sends, by method. */\nexport interface Notifications {\n");
    for (name, schema) in contract["notifications"].as_object().unwrap_or(&empty) {
        out.push_str(&format!("  {}: {};\n", key(name), ty(schema, 2)));
    }
    out.push_str("}\n");
    // An error response's `error.data`, unless it's a definition of that name already.
    if contract.get("error").is_some() && contract["error"]["$ref"] != "#/$defs/ErrorData" {
        out.push_str(&format!(
            "\n/** An error response's `error.data`. */\nexport type ErrorData = {};\n",
            ty(&contract["error"], 0)
        ));
    }
    // A type laid out over several lines starts on the next one: no space before it.
    out.lines().map(str::trim_end).collect::<Vec<_>>().join("\n") + "\n"
}

/// A definition's name as a TypeScript identifier.
fn ident(name: &str) -> String {
    let mut s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    s
}

/// A property name, quoted unless it's an identifier.
fn key(name: &str) -> String {
    let plain = name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    if plain { name.to_string() } else { Value::String(name.into()).to_string() }
}

/// A JSDoc comment from the schema's description.
fn doc(out: &mut String, schema: &Value, pad: &str) {
    let Some(text) = schema.get("description").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) else {
        return;
    };
    let text = text.replace("*/", "*\\/");
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() == 1 {
        out.push_str(&format!("{pad}/** {} */\n", lines[0].trim()));
    } else {
        out.push_str(&format!("{pad}/**\n"));
        for l in lines {
            let l = l.trim_end();
            if l.is_empty() {
                out.push_str(&format!("{pad} *\n"));
            } else {
                out.push_str(&format!("{pad} * {l}\n"));
            }
        }
        out.push_str(&format!("{pad} */\n"));
    }
}

/// Wrapped in parentheses when it's a union or an intersection, for use inside another type.
fn tight(t: String) -> String {
    let mut depth = 0i32;
    let mut loose = false;
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // String literals and comments say nothing about the type's shape.
            '"' => {
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => {
                            chars.next();
                        }
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
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth -= 1,
            '|' | '&' if depth == 0 => loose = true,
            _ => {}
        }
    }
    if loose { format!("({t})") } else { t }
}

/// A union whose members are described: one member a line, each under its comment.
fn described_union(list: &[Value], indent: usize) -> String {
    let pad = " ".repeat(indent + 2);
    let mut s = String::new();
    for m in list {
        s.push('\n');
        doc(&mut s, m, &pad);
        s.push_str(&format!("{pad}| {}", tight(ty(m, indent + 2))));
    }
    s
}

fn union(mut parts: Vec<String>) -> String {
    let mut seen = Vec::new();
    parts.retain(|p| {
        let new = !seen.contains(p);
        seen.push(p.clone());
        new
    });
    if parts.iter().any(|p| p == "unknown") {
        return "unknown".into();
    }
    match parts.len() {
        0 => "never".into(),
        1 => parts.remove(0),
        _ => parts.into_iter().map(tight).collect::<Vec<_>>().join(" | "),
    }
}

/// The TypeScript type for `schema`, laid out for `indent` spaces.
fn ty(schema: &Value, indent: usize) -> String {
    let obj = match schema {
        Value::Bool(true) => return "unknown".into(),
        Value::Bool(false) => return "never".into(),
        Value::Object(o) => o,
        _ => return "unknown".into(),
    };
    let mut parts = Vec::new();
    if let Some(r) = obj.get("$ref").and_then(Value::as_str) {
        let name = ident(r.rsplit('/').next().unwrap_or(r));
        // A tagged variant of a flattened struct: the struct, plus its tag beside the `$ref`.
        if !obj.contains_key("properties") {
            return name;
        }
        parts.push(name);
    }
    if let Some(c) = obj.get("const") {
        return c.to_string();
    }
    if let Some(values) = obj.get("enum").and_then(Value::as_array) {
        return union(values.iter().map(Value::to_string).collect());
    }
    if let Some(types) = obj.get("type") {
        let types: Vec<&str> = match types {
            Value::String(t) => vec![t.as_str()],
            Value::Array(list) => list.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        parts.push(union(types.into_iter().map(|t| of_type(t, obj, indent)).collect()));
    } else if obj.contains_key("properties") || obj.contains_key("additionalProperties") {
        parts.push(object(obj, indent));
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(list) = obj.get(key).and_then(Value::as_array) {
            let described = list.len() > 1 && list.iter().any(|s| s.get("description").is_some());
            parts.push(if described {
                described_union(list, indent)
            } else {
                union(list.iter().map(|s| ty(s, indent)).collect())
            });
        }
    }
    if let Some(list) = obj.get("allOf").and_then(Value::as_array) {
        parts.extend(list.iter().map(|s| ty(s, indent)));
    }
    parts.retain(|p| p != "unknown");
    match parts.len() {
        0 => "unknown".into(),
        1 => parts.remove(0),
        _ => parts.into_iter().map(tight).collect::<Vec<_>>().join(" & "),
    }
}

fn of_type(t: &str, obj: &Map<String, Value>, indent: usize) -> String {
    match t {
        "string" => "string".into(),
        "integer" | "number" => "number".into(),
        "boolean" => "boolean".into(),
        "null" => "null".into(),
        "array" => {
            if let Some(items) = obj.get("prefixItems").and_then(Value::as_array) {
                let elems: Vec<String> = items
                    .iter()
                    .map(|i| match i.get("title").and_then(Value::as_str) {
                        Some(title) => format!("{}: {}", ident(title), ty(i, indent)),
                        None => ty(i, indent),
                    })
                    .collect();
                format!("[{}]", elems.join(", "))
            } else {
                format!("{}[]", tight(obj.get("items").map_or("unknown".into(), |i| ty(i, indent))))
            }
        }
        "object" => object(obj, indent),
        _ => "unknown".into(),
    }
}

fn object(obj: &Map<String, Value>, indent: usize) -> String {
    let props = obj.get("properties").and_then(Value::as_object);
    let extra = obj.get("additionalProperties").filter(|a| **a != Value::Bool(false));
    let required: Vec<&str> =
        obj.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    let map = |a: &Value| format!("{{ [key: string]: {} }}", ty(a, indent));
    let Some(props) = props.filter(|p| !p.is_empty()) else {
        return match extra {
            Some(a) => map(a),
            None if obj.get("additionalProperties") == Some(&Value::Bool(false)) => "Record<string, never>".into(),
            None => "{ [key: string]: unknown }".into(),
        };
    };
    let pad = " ".repeat(indent + 2);
    let mut s = String::from("{\n");
    for (name, p) in props {
        doc(&mut s, p, &pad);
        let optional = if required.contains(&name.as_str()) { "" } else { "?" };
        s.push_str(&format!("{pad}{}{optional}: {};\n", key(name), ty(p, indent + 2)));
    }
    s.push_str(&" ".repeat(indent));
    s.push('}');
    match extra {
        Some(a) => format!("{s} & {}", map(a)),
        None => s,
    }
}

#[cfg(test)]
mod tests {
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
            t(
                json!({ "oneOf": [{ "type": "object", "properties": { "kind": { "const": "x" } }, "required": ["kind"] }] })
            ),
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
        let defined =
            json!({ "$defs": { "ErrorData": { "type": "object" } }, "error": { "$ref": "#/$defs/ErrorData" } });
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
}
