//! The sidecar's contract as data: every method with JSON Schemas for its parameters and result,
//! the notifications it sends and its errors' `data`, from the [registry](super::methods). It's
//! what `rpc.describe` answers and what `locallibrary rpc --emit-schema DIR` writes (with
//! TypeScript types made from it, see [`super::typescript`]).

use std::path::Path;
use std::sync::OnceLock;

use locallibrary::api;
use schemars::SchemaGenerator;
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use super::methods::{Method, Registry, registry};
use super::{PROTOCOL, typescript};

/// What the files `--emit-schema` writes are called.
pub const SCHEMA_FILE: &str = "schema.json";
pub const TYPES_FILE: &str = "locallibrary.d.ts";

/// Where a schema shared by parameters and results, but read differently by each (a field with
/// a default is optional in a request and always there in a result), is kept for results:
/// `<Name>Output`.
const OUTPUT_SUFFIX: &str = "Output";

/// The whole contract (without the sidecar's version, so it changes only when the contract
/// does).
pub fn contract() -> &'static Value {
    static CONTRACT: OnceLock<Value> = OnceLock::new();
    CONTRACT.get_or_init(|| build(registry(), registry().methods().collect()))
}

/// What `rpc.describe` answers: the contract with the version, or one method of it (with the
/// definitions it uses).
pub fn describe(method: Option<&str>) -> Option<Value> {
    let mut v = match method {
        None => contract().clone(),
        Some(name) => {
            let m = registry().get(name).filter(|m| !m.hidden)?;
            // Cut from the whole contract, so its definitions have the names they have there:
            // built on its own, a result's type would lose the `Output` a parameter's of the same
            // name gives it, and name another definition than the contract's.
            let mut v = contract().clone();
            v["methods"].as_array_mut().expect("a list").retain(|d| d["name"] == m.name);
            prune_definitions(&mut v);
            v
        }
    };
    v["version"] = env!("CARGO_PKG_VERSION").into();
    Some(v)
}

fn build(registry: &Registry, methods: Vec<&Method>) -> Value {
    // Parameters are read as requests (defaults make fields optional), results as they're
    // written (every field is there unless it's skipped).
    let mut params = SchemaSettings::draft2020_12().for_deserialize().into_generator();
    let mut results = SchemaSettings::draft2020_12().for_serialize().into_generator();
    let mut described = Vec::new();
    for m in methods {
        let mut d = Map::new();
        d.insert("name".into(), m.name.into());
        d.insert("aliases".into(), json!(m.aliases));
        d.insert("kind".into(), json!(m.kind));
        if let Some(pair) = m.pair {
            d.insert("pair".into(), pair.into());
        }
        if let Some(op) = m.operation {
            d.insert("operation".into(), op.into());
        }
        d.insert("long_running".into(), m.long_running.into());
        d.insert("destructive".into(), m.destructive.into());
        if let Some(instead) = m.deprecated {
            d.insert("deprecated".into(), instead.into());
        }
        d.insert("params".into(), m.params_schema(&mut params).to_value());
        d.insert("result".into(), m.result_schema(&mut results, registry).to_value());
        described.push(Value::Object(d));
    }
    let mut notifications = json!({
        "progress": results.subschema_for::<api::ProgressParams>().to_value(),
        "event": results.subschema_for::<api::Notice>().to_value(),
    });
    #[cfg(feature = "peers")]
    {
        notifications["serve"] = results.subschema_for::<api::ServeParams>().to_value();
    }
    let mut contract = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "locallibrary RPC",
        "description": "Methods of `locallibrary rpc` (JSON-RPC 2.0, one message per line): each with its \
            parameters' and result's schema. Names are `noun.verb`; a change is `<op>.plan` (answers a \
            plan with a `plan_id`) and `<op>.apply` (applies it). `aliases` still work.",
        "protocol": PROTOCOL,
        "methods": described,
        "notifications": notifications,
        "error": results.subschema_for::<api::ErrorData>().to_value(),
    });
    merge_definitions(&mut contract, &mut params, &mut results);
    contract
}

/// Puts both generators' definitions under `$defs`. One read differently as a parameter and as
/// a result keeps its name for parameters, and results refer to `<Name>Output`.
fn merge_definitions(contract: &mut Value, params: &mut SchemaGenerator, results: &mut SchemaGenerator) {
    let mut defs = params.take_definitions(true);
    let mut out = results.take_definitions(true);
    let renamed: Vec<String> =
        out.iter().filter(|(name, s)| defs.get(*name).is_some_and(|p| p != *s)).map(|(name, _)| name.clone()).collect();
    if !renamed.is_empty() {
        let rename = |v: &mut Value| rename_refs(v, &renamed);
        for s in out.values_mut() {
            rename(s);
        }
        for m in contract["methods"].as_array_mut().into_iter().flatten() {
            rename(&mut m["result"]);
        }
        rename(&mut contract["notifications"]);
        rename(&mut contract["error"]);
        for name in &renamed {
            if let Some(s) = out.remove(name) {
                out.insert(format!("{name}{OUTPUT_SUFFIX}"), s);
            }
        }
    }
    for (name, s) in out {
        defs.entry(name).or_insert(s);
    }
    contract["$defs"] = Value::Object(defs);
}

fn rename_refs(v: &mut Value, renamed: &[String]) {
    match v {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get_mut("$ref")
                && let Some(name) = r.strip_prefix("#/$defs/")
                && renamed.iter().any(|n| n == name)
            {
                *r = format!("{r}{OUTPUT_SUFFIX}");
            }
            for v in map.values_mut() {
                rename_refs(v, renamed);
            }
        }
        Value::Array(list) => list.iter_mut().for_each(|v| rename_refs(v, renamed)),
        _ => {}
    }
}

/// Drops the definitions nothing refers to (for one method's description).
fn prune_definitions(contract: &mut Value) {
    let Some(Value::Object(defs)) = contract.get_mut("$defs").map(Value::take) else { return };
    let mut keep = std::collections::BTreeSet::new();
    let mut todo = Vec::new();
    refs(&contract["methods"], &mut todo);
    refs(&contract["notifications"], &mut todo);
    refs(&contract["error"], &mut todo);
    while let Some(name) = todo.pop() {
        if keep.insert(name.clone())
            && let Some(d) = defs.get(&name)
        {
            refs(d, &mut todo);
        }
    }
    contract["$defs"] = Value::Object(defs.into_iter().filter(|(name, _)| keep.contains(name)).collect());
}

fn refs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            if let Some(name) = map.get("$ref").and_then(Value::as_str).and_then(|r| r.strip_prefix("#/$defs/")) {
                out.push(name.to_string());
            }
            map.values().for_each(|v| refs(v, out));
        }
        Value::Array(list) => list.iter().for_each(|v| refs(v, out)),
        _ => {}
    }
}

/// The contract's files: `schema.json` and the TypeScript types made from it.
pub fn files() -> Vec<(&'static str, String)> {
    let mut schema = serde_json::to_string_pretty(contract()).expect("serialisable");
    schema.push('\n');
    vec![(SCHEMA_FILE, schema), (TYPES_FILE, typescript::declarations(contract()))]
}

/// `locallibrary rpc --emit-schema DIR`.
pub fn emit(dir: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut written = Vec::new();
    for (name, text) in files() {
        let path = dir.join(name);
        std::fs::write(&path, text)?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests;
