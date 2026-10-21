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
