use hegel::TestCase;
use hegel::generators as gs;
use serde_json::json;

use super::*;

/// A view as JSON, and as it reads back.
fn round_trip(v: &View) -> Value {
    let json = serde_json::to_value(v).unwrap();
    assert_eq!(&serde_json::from_value::<View>(json.clone()).unwrap(), v, "{json}");
    json
}

#[test]
fn a_tables_columns_are_names_or_typed() {
    let v: View = serde_json::from_value(json!({
        "kind": "table",
        "columns": ["Title", { "name": "Length", "kind": "duration" }, { "name": "Tempo", "kind": "number", "unit": "BPM" }, { "name": "Note" }],
        "rows": [["A", 187, 124.5, "x"]],
    }))
    .unwrap();
    let View::Table { columns, .. } = &v else { panic!("a table") };
    let kinds: Vec<(&str, ColumnKind, Option<&str>)> = columns.iter().map(|c| (c.name(), c.kind(), c.unit())).collect();
    assert_eq!(
        kinds,
        [
            ("Title", ColumnKind::Text, None),
            ("Length", ColumnKind::Duration, None),
            ("Tempo", ColumnKind::Number, Some("BPM")),
            ("Note", ColumnKind::Text, None),
        ]
    );
    // Each as it was sent: a name stays a name.
    assert_eq!(round_trip(&v)["columns"][0], json!("Title"));
    assert!(v.needs_views(), "typed columns");
    let plain = View::Table { title: None, columns: vec!["Title".into()], rows: Vec::new() };
    assert!(!plain.needs_views());
    let wrong = json!({ "kind": "table", "columns": [{ "name": "Key", "kind": "colour" }], "rows": [] });
    assert!(serde_json::from_value::<View>(wrong).is_err());
}

#[test]
fn the_new_kinds_read_as_the_protocol_says() {
    let tree = json!({ "kind": "tree", "items": [{ "label": "Folder", "children": [{ "label": "Set", "detail": "12 tracks" }] }] });
    let chart = json!({ "kind": "chart", "style": "line", "series": [{ "name": "energy", "points": [[0.0, 1.0], [30.0, 4.5]] }], "x_label": "s" });
    let image = json!({ "kind": "image", "blob": "cover.png", "alt": "The cover" });
    let sections = json!({ "kind": "sections", "title": "About", "items": [{ "title": "Shape", "view": tree }, { "title": "Cover", "view": image }] });
    for v in [&tree, &chart, &image, &sections] {
        let view: View = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(&round_trip(&view), v);
        assert!(view.needs_views() && !view.is_empty() && !view.asks());
    }
    let mut v: View = serde_json::from_value(sections).unwrap();
    assert_eq!(v.title(), Some("About"));
    let images = v.images_mut();
    assert_eq!(images.len(), 1);
    let View::Image { blob, .. } = &*images[0] else { panic!("an image") };
    assert_eq!(blob, "cover.png");
}

#[test]
fn a_view_is_empty_with_nothing_to_show() {
    let empty = [
        json!({ "kind": "list", "items": [] }),
        json!({ "kind": "text", "text": " \n" }),
        json!({ "kind": "chart", "style": "bar", "series": [{ "name": "a", "points": [] }] }),
        json!({ "kind": "sections", "items": [{ "title": "A", "view": { "kind": "tree", "items": [] } }] }),
    ];
    for v in empty {
        assert!(serde_json::from_value::<View>(v.clone()).unwrap().is_empty(), "{v}");
    }
    let asking = json!({ "kind": "sections", "items": [{ "title": "A", "view": { "kind": "choose", "title": "Which?", "param": "p", "items": [] } }] });
    assert!(serde_json::from_value::<View>(asking).unwrap().asks());
}

// Properties.

/// A view of any kind but a question, sections nesting `depth` more levels at most.
#[hegel::test_helper]
fn view(tc: &TestCase, depth: u8) -> View {
    let title = tc.draw(gs::optional(gs::text().max_size(6)));
    let kinds = if depth == 0 { 7 } else { 8 };
    match tc.draw(gs::integers::<u8>().max_value(kinds - 1)) {
        0 => {
            let items = tc.draw(gs::vecs(gs::text().max_size(4)).max_size(3));
            View::List { title, items: items.into_iter().map(|label| ListItem { label, detail: None }).collect() }
        }
        1 => {
            let n = tc.draw(gs::integers::<usize>().max_value(3));
            let columns = (0..n)
                .map(|i| match tc.draw(gs::booleans()) {
                    true => Column::Name(format!("c{i}")),
                    false => Column::Typed(TypedColumn {
                        name: format!("c{i}"),
                        kind: tc.draw(gs::sampled_from(vec![
                            ColumnKind::Text,
                            ColumnKind::Track,
                            ColumnKind::Duration,
                            ColumnKind::Key,
                            ColumnKind::Number,
                        ])),
                        unit: tc.draw(gs::optional(gs::text().max_size(3))),
                    }),
                })
                .collect();
            let rows = tc.draw(gs::vecs(gs::vecs(gs::integers::<i32>()).min_size(n).max_size(n)).max_size(3));
            View::Table {
                title,
                columns,
                rows: rows.into_iter().map(|r| r.into_iter().map(Value::from).collect()).collect(),
            }
        }
        2 => View::Kv { title, pairs: vec![Pair { label: tc.draw(gs::text().max_size(4)), value: json!(1) }] },
        3 => View::Text { title, text: tc.draw(gs::text().max_size(8)) },
        4 => {
            let leaf =
                |tc: &TestCase| TreeItem { label: tc.draw(gs::text().max_size(4)), detail: None, children: Vec::new() };
            let children = (0..tc.draw(gs::integers::<usize>().max_value(2))).map(|_| leaf(tc)).collect();
            View::Tree { title, items: vec![TreeItem { children, ..leaf(tc) }] }
        }
        5 => {
            let point = gs::tuples!(
                gs::floats::<f64>().min_value(-1e6).max_value(1e6),
                gs::floats::<f64>().min_value(-1e6).max_value(1e6)
            );
            let series = vec![Series { name: "s".into(), points: tc.draw(gs::vecs(point).max_size(4)) }];
            let style = tc.draw(gs::sampled_from(vec![ChartStyle::Line, ChartStyle::Bar]));
            View::Chart { title, style, series, x_label: None, y_label: tc.draw(gs::optional(gs::text().max_size(3))) }
        }
        6 => View::Image { title, blob: "a.png".into(), alt: tc.draw(gs::text().max_size(6)), data: None },
        _ => {
            let n = tc.draw(gs::integers::<usize>().max_value(3));
            let items = (0..n).map(|i| Section { title: format!("s{i}"), view: view(tc, depth - 1) }).collect();
            View::Sections { title, items }
        }
    }
}

/// Every view reads back from its JSON as it was, fits the view's JSON Schema, and needs the
/// `views` feature exactly when it's (or holds) one of the kinds that came with it or a typed
/// column.
#[hegel::test(test_cases = 200)]
fn views_read_back_and_say_what_they_need(tc: TestCase) {
    let v = view(&tc, 2);
    let json = round_trip(&v);
    let schema = serde_json::to_value(schemars::schema_for!(View)).unwrap();
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&json), "{json}");
    let typed = |columns: &[Column]| columns.iter().any(|c| matches!(c, Column::Typed(_)));
    let needs = match &v {
        View::Table { columns, .. } => typed(columns),
        View::List { .. } | View::Kv { .. } | View::Text { .. } => false,
        _ => true,
    };
    assert_eq!(v.needs_views(), needs);
    let images = json.to_string().matches("\"kind\":\"image\"").count();
    assert_eq!(v.clone().images_mut().len(), images);
}

/// A question, as a view: what it asks, of items labelled after their values.
#[hegel::test_helper]
fn question(tc: &TestCase) -> View {
    let title = tc.draw(gs::text().max_size(6));
    let values: Vec<i32> = tc.draw(gs::vecs(gs::integers::<i32>()).max_size(2));
    let items = values.into_iter().map(|v| crate::ChoiceItem { label: v.to_string(), detail: None, value: v.into() });
    View::Choose(Choice { title, param: "p".into(), multi: false, items: items.collect(), more: Vec::new() })
}

/// Whether the view `json` shows nothing, read from its JSON: no items, rows, pairs, points or
/// text, nor a section that shows something; a picture or a question is never empty.
fn shows_nothing(json: &Value) -> bool {
    let none = |key: &str| json[key].as_array().is_none_or(Vec::is_empty);
    match json["kind"].as_str().unwrap() {
        "list" | "tree" => none("items"),
        "table" => none("rows"),
        "kv" => none("pairs"),
        "text" => json["text"].as_str().unwrap().trim().is_empty(),
        "chart" => json["series"].as_array().unwrap().iter().all(|s| s["points"].as_array().unwrap().is_empty()),
        "sections" => json["items"].as_array().unwrap().iter().all(|s| shows_nothing(&s["view"])),
        _ => false,
    }
}

/// Any view (a question now and then, alone or in a section) says the title its JSON has, asks
/// exactly when its JSON holds a question, and is empty exactly when its JSON shows nothing; a
/// table's columns say their names, kinds and units as their JSON does (a name alone is text).
#[hegel::test(test_cases = 200)]
fn prop_a_view_says_its_title_what_it_asks_and_whether_it_shows_anything(tc: TestCase) {
    let mut v = match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => question(&tc),
        _ => view(&tc, 2),
    };
    if let View::Sections { items, .. } = &mut v
        && !items.is_empty()
        && tc.draw(gs::booleans())
    {
        let i = tc.draw(gs::integers::<usize>().max_value(items.len() - 1));
        items[i].view = question(&tc);
    }
    let json = serde_json::to_value(&v).unwrap();
    assert_eq!(v.title(), json["title"].as_str());
    assert_eq!(v.asks(), json.to_string().contains("\"kind\":\"choose\""), "{json}");
    assert_eq!(v.is_empty(), shows_nothing(&json), "{json}");
    if let View::Choose(_) = v {
        assert!(!v.needs_views());
    }
    let View::Table { columns, .. } = &v else { return };
    for (column, json) in columns.iter().zip(json["columns"].as_array().unwrap()) {
        let typed = json.as_object();
        let name = typed.map_or(json, |c| &c["name"]);
        assert_eq!(column.name(), name.as_str().unwrap());
        let kind = typed.map_or(json!("text"), |c| c["kind"].clone());
        assert_eq!(serde_json::to_value(column.kind()).unwrap(), kind);
        assert_eq!(column.unit(), typed.and_then(|c| c.get("unit")?.as_str()));
        assert_eq!(Column::from(column.name()), Column::Name(column.name().into()));
    }
}
