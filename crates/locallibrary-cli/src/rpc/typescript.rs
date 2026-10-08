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
    if let Some(r) = obj.get("$ref").and_then(Value::as_str) {
        return ident(r.rsplit('/').next().unwrap_or(r));
    }
    if let Some(c) = obj.get("const") {
        return c.to_string();
    }
    if let Some(values) = obj.get("enum").and_then(Value::as_array) {
        return union(values.iter().map(Value::to_string).collect());
    }
    let mut parts = Vec::new();
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
}
