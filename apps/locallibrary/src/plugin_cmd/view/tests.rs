use hegel::TestCase;
use hegel::generators as gs;
use loclib_plugin::{ChartStyle, Section};
use serde_json::json;

use super::*;

fn none(_: u32) -> Option<String> {
    None
}

fn view(json: Value) -> View {
    serde_json::from_value(json).unwrap()
}

#[test]
fn typed_cells_show_as_their_columns_say() {
    let v = view(json!({
        "kind": "table",
        "columns": ["Name", { "name": "Track", "kind": "track" }, { "name": "Time", "kind": "duration" },
            { "name": "Key", "kind": "key" }, { "name": "Tempo", "kind": "number", "unit": "BPM" }],
        "rows": [["a", 7, 187, "Am", 124.5], ["b", 8, 3723, "8A", 90], ["c", "x", "soon", 5, "fast"]],
    }));
    let names = |id: u32| (id == 7).then(|| "Burial - Archangel".to_string());
    assert_eq!(
        lines(&v, &names),
        [
            Drawn::Columns("Name  Track                  Time  Key      Tempo".into()),
            Drawn::Row("a     Burial - Archangel     3:07  8A   124.5 BPM".into()),
            Drawn::Row("b     #8                  1:02:03  8A      90 BPM".into()),
            Drawn::Row("c     x                      soon  5         fast".into()),
        ]
    );
}

#[test]
fn every_kind_has_its_plain_text() {
    let tree = json!({ "kind": "tree", "items": [{ "label": "Club", "detail": "2 lists", "children": [{ "label": "Warm up" }] }] });
    assert_eq!(
        lines(&view(tree.clone()), &none),
        [
            Drawn::Item { label: "Club".into(), detail: Some("2 lists".into()), depth: 0 },
            Drawn::Item { label: "Warm up".into(), detail: None, depth: 1 },
        ]
    );
    let chart = json!({ "kind": "chart", "style": "bar", "x_label": "min", "series": [
        { "name": "energy", "points": [[0.0, 1.0], [2.0, 3.0]] }, { "name": "tempo", "points": [[2.0, 124.0]] }] });
    assert_eq!(
        lines(&view(chart), &none),
        [
            Drawn::Columns("min  energy  tempo".into()),
            Drawn::Row("0.0     1.0      -".into()),
            Drawn::Row("2.0     3.0  124.0".into()),
        ]
    );
    let image = json!({ "kind": "image", "blob": "a.png", "alt": "the cover" });
    assert_eq!(lines(&view(image.clone()), &none), [Drawn::Text("[image: the cover]".into())]);
    let sections = json!({ "kind": "sections", "items": [{ "title": "Shape", "view": tree },
        { "title": "Nothing", "view": { "kind": "list", "items": [] } }] });
    let drawn = lines(&view(sections), &none);
    assert_eq!(drawn[0], Drawn::Heading("Shape".into()));
    assert_eq!(&drawn[3..], [Drawn::Heading("Nothing".into()), Drawn::Missing]);
    let empty = json!({ "kind": "table", "columns": ["Title"], "rows": [] });
    assert_eq!(lines(&view(empty), &none), [Drawn::Columns("Title".into()), Drawn::Missing]);
    assert_eq!(lines(&view(json!({ "kind": "sections", "items": [] })), &none), [Drawn::Missing]);
}

/// A chart's table has a row for each x any series has, in order, and each series' y there in
/// its column (none where it has none).
#[hegel::test(test_cases = 200)]
fn a_charts_table_holds_every_point(tc: TestCase) {
    let n = tc.draw(gs::integers::<usize>().max_value(3));
    let series: Vec<Series> = (0..n)
        .map(|i| {
            let point = gs::tuples!(gs::integers::<i16>(), gs::integers::<i16>());
            let mut points: Vec<(i16, i16)> = tc.draw(gs::vecs(point).max_size(5));
            points.sort_by_key(|p| p.0);
            points.dedup_by_key(|p| p.0);
            Series { name: format!("s{i}"), points: points.into_iter().map(|(x, y)| (x.into(), y.into())).collect() }
        })
        .collect();
    let (columns, rows) = chart_table(&series, None);
    assert_eq!(columns.len(), series.len() + 1);
    let xs: Vec<f64> = rows.iter().map(|r| r[0].as_f64().unwrap()).collect();
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
    for (i, s) in series.iter().enumerate() {
        for row in &rows {
            let x = row[0].as_f64().unwrap();
            let y = s.points.iter().find(|p| p.0 == x).map(|p| p.1);
            assert_eq!(row[i + 1].as_f64(), y, "{} at {x}", s.name);
        }
        assert!(s.points.iter().all(|p| xs.contains(&p.0)));
    }
    let v = View::Chart { title: None, style: ChartStyle::Line, series, x_label: None, y_label: None };
    let shown = lines(&v, &none);
    assert_eq!(shown.len(), if v.is_empty() { 1 } else { rows.len() + 1 });
    let sections = View::Sections { title: None, items: vec![Section { title: "a".into(), view: v }] };
    assert_eq!(lines(&sections, &none)[1..], shown[..]);
}

/// A tree of at most `depth` more levels, labelled `i0`, `i1`... in the order drawn, and each
/// item's label and depth in that order.
#[hegel::test_helper]
fn tree_items(tc: &TestCase, depth: usize, drawn: &mut Vec<(String, usize)>, at: usize) -> Vec<TreeItem> {
    (0..tc.draw(gs::integers::<usize>().max_value(3)))
        .map(|_| {
            let label = format!("i{}", drawn.len());
            drawn.push((label.clone(), at));
            let detail = tc.draw(gs::booleans()).then(|| format!("{label} detail"));
            let children = if depth == 0 { Vec::new() } else { tree_items(tc, depth - 1, drawn, at + 1) };
            TreeItem { label, detail, children }
        })
        .collect()
}

/// Any view of the kinds a plugin with `views` can answer is drawn whole: a tree's items in order,
/// each its depth down; a picture as what it shows; sections each as its title and then what its
/// view draws on its own; and what shows nothing says so.
#[hegel::test(test_cases = 200)]
fn any_view_is_drawn_whole(tc: TestCase) {
    let mut drawn = Vec::new();
    let items = tree_items(&tc, tc.draw(gs::integers::<usize>().max_value(3)), &mut drawn, 0);
    let tree = View::Tree { title: None, items };
    let items: Vec<Drawn> =
        drawn.iter().map(|(label, depth)| Drawn::Item { label: label.clone(), detail: None, depth: *depth }).collect();
    let shown: Vec<Drawn> = lines(&tree, &none)
        .into_iter()
        .map(|d| match d {
            Drawn::Item { label, detail, depth } => {
                assert!(detail.is_none_or(|d| d == format!("{label} detail")));
                Drawn::Item { label, detail: None, depth }
            }
            other => other,
        })
        .collect();
    match items.is_empty() {
        true => assert_eq!(shown, [Drawn::Missing]),
        false => assert_eq!(shown, items),
    }
    let alt = tc.draw(gs::text().max_size(8));
    let image = View::Image { title: None, blob: "a.png".into(), alt: alt.clone(), data: None };
    assert_eq!(lines(&image, &none), [Drawn::Text(format!("[image: {alt}]"))]);
    let kinds = [tree, image, View::List { title: None, items: Vec::new() }];
    let n = tc.draw(gs::integers::<usize>().max_value(4));
    let items: Vec<Section> =
        (0..n).map(|i| Section { title: format!("s{i}"), view: tc.draw(gs::sampled_from(kinds.to_vec())) }).collect();
    let each: Vec<Drawn> = items
        .iter()
        .flat_map(|s| std::iter::once(Drawn::Heading(s.title.clone())).chain(lines(&s.view, &none)))
        .collect();
    let sections = View::Sections { title: None, items };
    match n {
        0 => assert_eq!(lines(&sections, &none), [Drawn::Missing]),
        _ => assert_eq!(lines(&sections, &none), each),
    }
}

/// Any cell shows as its column says: a track by its name (`#id` without one), whole seconds as
/// minutes (and hours) and seconds, a key in Camelot notation, a number with its column's unit,
/// and a cell that isn't what its column says as it is.
#[hegel::test(test_cases = 300)]
fn any_cell_shows_as_its_column_says(tc: TestCase) {
    let names = |id: u32| id.is_multiple_of(2).then(|| format!("track {id}"));
    let unit = tc.draw(gs::optional(gs::sampled_from(vec!["BPM", "dB"])));
    let typed_as = |kind| Column::Typed(TypedColumn { name: "c".into(), kind, unit: unit.map(String::from) });
    let (column, v, shown) = match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => {
            let id = tc.draw(gs::integers::<u32>());
            (typed_as(ColumnKind::Track), json!(id), names(id).unwrap_or_else(|| format!("#{id}")))
        }
        1 => {
            let s = tc.draw(gs::integers::<u64>().max_value(100_000));
            let shown = match s / 3600 {
                0 => format!("{}:{:02}", s / 60, s % 60),
                h => format!("{h}:{:02}:{:02}", s / 60 % 60, s % 60),
            };
            (typed_as(ColumnKind::Duration), json!(s), shown)
        }
        2 => {
            let keys = vec![("Am", "8A"), ("C", "8B"), ("Fm", "4A"), ("G", "9B"), ("8A", "8A"), ("12B", "12B")];
            let (key, code) = tc.draw(gs::sampled_from(keys));
            (typed_as(ColumnKind::Key), json!(key), code.to_string())
        }
        3 => {
            let n = tc.draw(gs::integers::<i32>());
            let shown = unit.map_or_else(|| n.to_string(), |u| format!("{n} {u}"));
            (typed_as(ColumnKind::Number), json!(n), shown)
        }
        // Not what its column says, or a column of text.
        _ => {
            let kind = tc.draw(gs::sampled_from(vec![ColumnKind::Duration, ColumnKind::Number, ColumnKind::Text]));
            let text = tc.draw(gs::text().alphabet("abc ").max_size(6));
            (typed_as(kind), json!(text), text)
        }
    };
    assert_eq!(typed(&column, &v, &names), shown, "{column:?} {v}");
    let table = View::Table { title: None, columns: vec![column], rows: vec![vec![v]] };
    let Drawn::Row(row) = &lines(&table, &names)[1] else { panic!("a row") };
    assert_eq!(row.trim(), crate::output::fit(&shown, MAX_COLUMN).trim());
}
