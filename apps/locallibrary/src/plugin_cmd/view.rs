//! What a plugin's command answers, drawn as the CLI draws its own results.

use loclib_plugin::{Pair, View};
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use crate::output::{self, dim, heading};
use crate::plugins::params::cell;

/// A headless command's answer: text as it is, anything else as JSON, nothing when there's none.
pub(super) fn headless(v: &Value) {
    match v {
        Value::Null => {}
        Value::String(text) => say!("{text}"),
        other => say!("{}", serde_json::to_string_pretty(other).expect("JSON")),
    }
}

/// The widest a table's column is.
const MAX_COLUMN: usize = 48;

/// A table's lines, its columns' headings first: each column as wide as its widest cell (at most
/// [`MAX_COLUMN`]), two spaces apart. The CLI and the player draw them.
pub(crate) fn table(columns: &[String], rows: &[Vec<Value>]) -> Vec<String> {
    let cells: Vec<Vec<String>> = rows.iter().map(|r| r.iter().map(cell).collect()).collect();
    let widths: Vec<usize> = (0..columns.len())
        .map(|i| {
            let widest = cells.iter().filter_map(|r| r.get(i)).map(|c| c.width()).max().unwrap_or(0);
            widest.max(columns[i].width()).min(MAX_COLUMN)
        })
        .collect();
    let line = |row: &[String]| {
        let cells = row.iter().zip(&widths).map(|(c, w)| output::fit(c, *w)).collect::<Vec<_>>();
        cells.join("  ").trim_end().to_string()
    };
    std::iter::once(line(columns)).chain(cells.iter().map(|r| line(r))).collect()
}

/// Labelled values' lines, the labels aligned. The CLI and the player draw them.
pub(crate) fn pairs(pairs: &[Pair]) -> Vec<String> {
    let width = pairs.iter().map(|p| p.label.width()).max().unwrap_or(0);
    pairs.iter().map(|p| format!("{}  {}", output::fit(&p.label, width), cell(&p.value))).collect()
}

/// A view a plugin's command answered, drawn as the CLI's lists and tables are.
pub(super) fn view(v: &View) {
    let (View::List { title, .. } | View::Table { title, .. } | View::Kv { title, .. } | View::Text { title, .. }) = v;
    if let Some(title) = title {
        heading(title);
    }
    match v {
        View::List { items, .. } => {
            for i in items {
                match &i.detail {
                    Some(detail) => say!("{}  {}", i.label, dim(detail)),
                    None => say!("{}", i.label),
                }
            }
        }
        View::Table { columns, rows, .. } => {
            let lines = table(columns, rows);
            say!("{}", dim(&lines[0]));
            for line in &lines[1..] {
                say!("{line}");
            }
        }
        View::Kv { pairs: kv, .. } => {
            for line in pairs(kv) {
                say!("{line}");
            }
        }
        View::Text { text, .. } => say!("{text}"),
    }
}
