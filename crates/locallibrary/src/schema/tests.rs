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
