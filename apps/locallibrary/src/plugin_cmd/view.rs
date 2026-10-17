//! What a plugin's command answers, drawn as the CLI draws its own results.

use loclib_plugin::View;
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
            say!("{}", dim(line(columns)));
            for row in &cells {
                say!("{}", line(row));
            }
        }
        View::Kv { pairs, .. } => {
            let width = pairs.iter().map(|p| p.label.width()).max().unwrap_or(0);
            for p in pairs {
                say!("{}  {}", output::fit(&p.label, width), cell(&p.value));
            }
        }
        View::Text { text, .. } => say!("{text}"),
    }
}
