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
mod tests {
    use super::*;

    #[test]
    fn a_labelled_tuple_keeps_its_json_and_names_its_elements() {
        let f = Failure::new("Contents/a.mp3", "unreadable");
        let json = serde_json::to_value(&f).unwrap();
        assert_eq!(json, serde_json::json!(["Contents/a.mp3", "unreadable"]));
        assert_eq!(serde_json::from_value::<Failure>(json).unwrap(), f);

        let schema = schemars::SchemaGenerator::default().into_root_schema_for::<Failure>();
        let items = schema.get("prefixItems").and_then(|i| i.as_array()).unwrap();
        let titles: Vec<&str> = items.iter().map(|i| i["title"].as_str().unwrap()).collect();
        assert_eq!(titles, ["path", "error"]);
        assert_eq!(items[0]["description"], "The file (or folder, or key below a drive's root).");
        assert_eq!(schema.get("maxItems").unwrap(), 2);
    }

    #[test]
    fn shapes_follow_references_and_choices() {
        #[derive(schemars::JsonSchema)]
        #[allow(dead_code)]
        struct Fields {
            a: u32,
        }
        let mut g = schemars::SchemaGenerator::default();
        let fields = g.subschema_for::<Fields>();
        assert!(fields.get("$ref").is_some(), "{fields:?}");
        g.definitions_mut().insert("Loop".into(), serde_json::json!({ "$ref": "#/$defs/Loop" }));
        let of = |v: serde_json::Value| shape(&g, &schemars::Schema::try_from(v).unwrap());

        assert_eq!(shape(&g, &fields), Shape::Object);
        assert_eq!(of(serde_json::json!({ "type": "null" })), Shape::Null);
        assert_eq!(of(serde_json::json!({ "type": "string" })), Shape::Other);
        assert_eq!(of(serde_json::json!({ "properties": { "a": {} } })), Shape::Object);
        assert_eq!(of(serde_json::json!({ "oneOf": [fields.clone(), { "type": "object" }] })), Shape::Object);
        assert_eq!(of(serde_json::json!({ "anyOf": [fields, { "type": "string" }] })), Shape::Other);
        assert_eq!(of(serde_json::json!({ "allOf": [] })), Shape::Other);
        assert_eq!(of(serde_json::json!({ "$ref": "#/$defs/Missing" })), Shape::Other);
        // A reference to itself ends, unresolved.
        assert_eq!(of(serde_json::json!({ "$ref": "#/$defs/Loop" })), Shape::Other);
        assert_eq!(of(serde_json::json!({})), Shape::Other);
    }
}
