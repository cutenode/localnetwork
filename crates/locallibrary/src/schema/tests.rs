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

/// A schema drawn with the shape it was built to have, its definitions in `defs`: an object, a
/// null, anything else, a choice of schemas (objects only when every choice is one), or a
/// reference (to what's defined, or to nothing).
#[hegel::test_helper]
fn drawn(
    tc: &hegel::TestCase,
    defs: &mut serde_json::Map<String, serde_json::Value>,
    depth: u8,
) -> (serde_json::Value, Shape) {
    use hegel::generators as gs;
    use serde_json::json;
    let leaf = depth >= 3 || tc.draw(gs::booleans());
    if leaf {
        return match tc.draw(gs::integers::<u8>().max_value(5)) {
            0 => (json!({ "type": "object" }), Shape::Object),
            1 => (json!({ "type": "null" }), Shape::Null),
            2 => (json!({ "type": tc.draw(gs::sampled_from(vec!["string", "integer", "array"])) }), Shape::Other),
            3 => (json!({ "properties": { "a": {} } }), Shape::Object),
            4 => (json!({ "$ref": "#/$defs/Missing" }), Shape::Other),
            _ => (json!({}), Shape::Other),
        };
    }
    match tc.draw(gs::booleans()) {
        true => {
            let key = tc.draw(gs::sampled_from(vec!["oneOf", "anyOf", "allOf"]));
            let kids: Vec<(serde_json::Value, Shape)> =
                (0..tc.draw(gs::integers::<usize>().max_value(3))).map(|_| drawn(tc, defs, depth + 1)).collect();
            let objects = !kids.is_empty() && kids.iter().all(|(_, s)| *s == Shape::Object);
            let list: Vec<serde_json::Value> = kids.into_iter().map(|(v, _)| v).collect();
            (json!({ key: list }), if objects { Shape::Object } else { Shape::Other })
        }
        false => {
            let (def, shape) = drawn(tc, defs, depth + 1);
            let name = format!("D{}", defs.len());
            defs.insert(name.clone(), def);
            (json!({ "$ref": format!("#/$defs/{name}") }), shape)
        }
    }
}

/// A schema's shape is the one it was built with, references followed; one behind more than 16
/// references in a row is taken for a loop.
#[hegel::test(test_cases = 300)]
fn a_schemas_shape_is_what_it_was_built_as(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut g = schemars::SchemaGenerator::default();
    let mut defs = serde_json::Map::new();
    let (schema, want) = drawn(&tc, &mut defs, 0);
    // Behind a chain of references, as long as drawn.
    let chain = tc.draw(gs::integers::<usize>().max_value(20));
    let mut top = schema;
    for n in 0..chain {
        let name = format!("Chain{n}");
        defs.insert(name.clone(), top);
        top = serde_json::json!({ "$ref": format!("#/$defs/{name}") });
    }
    g.definitions_mut().extend(defs);
    let got = shape(&g, &schemars::Schema::try_from(top).unwrap());
    // The drawn schema nests up to three references of its own below the chain.
    if chain <= 13 {
        assert_eq!(got, want);
    }
    if chain > 16 {
        assert_eq!(got, Shape::Other, "{chain} references deep");
    }
}
