//! Describing the API's JSON: what front ends need to check requests and read results without
//! reading the Rust.
//!
//! Every request and result type in [`crate::api`] implements [`schemars::JsonSchema`]; the RPC
//! sidecar puts them together into its contract (`rpc.describe`, `locallibrary rpc
//! --emit-schema`). This module has the pieces the derive doesn't cover: results that were tuples
//! keep their JSON (an array) but name each element (`labelled_tuple!`, [`Failure`], [`Skip`]).

pub(crate) use loclib_model::labelled_tuple;

labelled_tuple! {
    /// Something an operation couldn't do, and the error: `[path, error]`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Failure {
        /// The file (or folder, or key below a drive's root).
        pub path: String,
        pub error: String,
    }
}

labelled_tuple! {
    /// Something an operation left alone, and why: `[path, reason]`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Skip {
        /// The file (or key below a drive's root).
        pub path: String,
        pub reason: String,
    }
}

impl Failure {
    pub fn new(path: impl Into<String>, error: impl ToString) -> Failure {
        Failure { path: path.into(), error: error.to_string() }
    }
}

impl Skip {
    pub fn new(path: impl Into<String>, reason: impl Into<String>) -> Skip {
        Skip { path: path.into(), reason: reason.into() }
    }
}

/// What a schema's JSON is, as far as merging it into an object goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// An object (or a choice of objects).
    Object,
    /// `null` (`()`).
    Null,
    Other,
}

/// What `schema` describes, following `$ref`s into `generator`'s definitions.
pub fn shape(generator: &schemars::SchemaGenerator, schema: &schemars::Schema) -> Shape {
    fn of(defs: &serde_json::Map<String, serde_json::Value>, v: &serde_json::Value, depth: u8) -> Shape {
        if depth > 16 {
            return Shape::Other;
        }
        if let Some(r) = v.get("$ref").and_then(|r| r.as_str()) {
            let name = r.rsplit('/').next().unwrap_or(r);
            return defs.get(name).map_or(Shape::Other, |d| of(defs, d, depth + 1));
        }
        match v.get("type").and_then(|t| t.as_str()) {
            Some("object") => return Shape::Object,
            Some("null") => return Shape::Null,
            Some(_) => return Shape::Other,
            None => {}
        }
        for key in ["oneOf", "anyOf", "allOf"] {
            if let Some(list) = v.get(key).and_then(|l| l.as_array()) {
                let objects = !list.is_empty() && list.iter().all(|s| of(defs, s, depth + 1) == Shape::Object);
                return if objects { Shape::Object } else { Shape::Other };
            }
        }
        if v.get("properties").is_some() { Shape::Object } else { Shape::Other }
    }
    of(generator.definitions(), schema.as_value(), 0)
}

#[cfg(test)]
mod tests;
