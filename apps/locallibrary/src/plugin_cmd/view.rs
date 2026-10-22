//! What a plugin's command (or panel) answers, drawn as the CLI draws its own results: every kind
//! of view as lines of plain text (see loclib-plugin's README), which the player draws too.

use loclib_plugin::{
    Column, ColumnKind, FieldDisplay, FieldFormat, FieldValue, ListItem, Pair, Series, TreeItem, TypedColumn, View,
};
use rustc_hash::FxHashMap;
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

/// A track's name (`Artist - Title`) by its id, where the drive's library is at hand.
pub(crate) type Names<'a> = &'a dyn Fn(u32) -> Option<String>;

/// The names of the tracks on `drive`, read once a `track` cell asks for one: none without it.
pub(crate) fn names_on(drive: Option<&str>) -> impl Fn(u32) -> Option<String> {
    let library = std::cell::OnceCell::new();
    move |id| {
        let read = || locallibrary::api::resolve_drive(drive?).ok()?.library().ok();
        library.get_or_init(read).as_ref()?.track(id).map(|t| t.display_name())
    }
}

/// A line of a view as plain text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Drawn {
    /// A section's title.
    Heading(String),
    /// A list's or a tree's item, `depth` levels below the top.
    Item {
        label: String,
        detail: Option<String>,
        depth: usize,
    },
    /// A table's headings.
    Columns(String),
    Row(String),
    /// Text, maybe of several lines.
    Text(String),
    /// What a view showing nothing says.
    Missing,
}

/// A view's lines, but its title: each kind as plain text (a chart as a table of its points, an
/// image as what it shows, each section's title and then its view), and `nothing to show` for one
/// that shows nothing (under a table's headings, or a section's title). `names` names the tracks
/// of a `track` column.
pub(crate) fn lines(v: &View, names: Names<'_>) -> Vec<Drawn> {
    let mut lines = Vec::new();
    match v {
        View::Table { columns, rows, .. } => lines.extend(table_lines(columns, rows, names)),
        // As it is, its lines and all.
        View::Text { text, .. } => lines.push(Drawn::Text(text.clone())),
        // Each title, and what each shows, or that it shows nothing.
        View::Sections { items, .. } => {
            for s in items {
                lines.push(Drawn::Heading(s.title.clone()));
                lines.extend(self::lines(&s.view, names));
            }
        }
        _ if v.is_empty() => {}
        View::List { items, .. } => {
            let item = |i: &ListItem| Drawn::Item { label: i.label.clone(), detail: i.detail.clone(), depth: 0 };
            lines.extend(items.iter().map(item));
        }
        View::Kv { pairs: kv, .. } => lines.extend(pairs(kv).into_iter().map(Drawn::Text)),
        View::Tree { items, .. } => tree(items, 0, &mut lines),
        View::Chart { series, x_label, .. } => {
            let (columns, rows) = chart_table(series, x_label.as_deref());
            lines.extend(table_lines(&columns, &rows, names));
        }
        View::Image { alt, .. } => lines.push(Drawn::Text(format!("[image: {alt}]"))),
        View::Choose(_) => unreachable!("a question is asked, never drawn"),
    }
    if says_nothing(v) {
        lines.push(Drawn::Missing);
    }
    lines
}

/// Whether `v` shows nothing, and says so itself: sections showing nothing say so each.
fn says_nothing(v: &View) -> bool {
    v.is_empty() && !matches!(v, View::Sections { items, .. } if !items.is_empty())
}

/// `items` and those below them, `depth` levels down.
fn tree(items: &[TreeItem], depth: usize, lines: &mut Vec<Drawn>) {
    for i in items {
        lines.push(Drawn::Item { label: i.label.clone(), detail: i.detail.clone(), depth });
        tree(&i.children, depth + 1, lines);
    }
}

/// A chart's points as a table: x (named `x_label`, else `x`), then each series' y at it (`null`
/// where it has none), in x's order.
pub(crate) fn chart_table(series: &[Series], x_label: Option<&str>) -> (Vec<Column>, Vec<Vec<Value>>) {
    let number = |name: &str| Column::Typed(TypedColumn { name: name.into(), kind: ColumnKind::Number, unit: None });
    let columns = std::iter::once(number(x_label.unwrap_or("x"))).chain(series.iter().map(|s| number(&s.name)));
    // Each x's row (`-0`'s is `0`'s), each series' first y at it: one pass, as a plugin's chart
    // can have millions of points.
    let mut rows: FxHashMap<u64, Vec<Value>> = FxHashMap::default();
    for (i, s) in series.iter().enumerate() {
        for &(x, y) in &s.points {
            let x = x + 0.0;
            let row = rows.entry(x.to_bits()).or_insert_with(|| {
                std::iter::once(x.into()).chain(std::iter::repeat_n(Value::Null, series.len())).collect()
            });
            if row[i + 1].is_null() {
                row[i + 1] = y.into();
            }
        }
    }
    let mut rows: Vec<(u64, Vec<Value>)> = rows.into_iter().collect();
    rows.sort_unstable_by(|a, b| f64::from_bits(a.0).total_cmp(&f64::from_bits(b.0)));
    (columns.collect(), rows.into_iter().map(|(_, row)| row).collect())
}

/// A cell as its column says: a track by its name (`#id` without one), seconds as minutes and
/// seconds, a key in Camelot notation, a number with the column's unit; anything else, or a
/// cell that isn't what its column says, as it is.
pub(crate) fn typed(column: &Column, v: &Value, names: Names<'_>) -> String {
    let id = v.as_u64().and_then(|id| u32::try_from(id).ok());
    match (column.kind(), v, id) {
        (ColumnKind::Track, _, Some(id)) => names(id).unwrap_or_else(|| format!("#{id}")),
        (ColumnKind::Duration, Value::Number(n), _) => {
            let duration = FieldDisplay { format: Some(FieldFormat::Duration), ..FieldDisplay::default() };
            duration.show(&FieldValue::Number(n.as_f64().unwrap_or_default()))
        }
        (ColumnKind::Key, Value::String(k), _) => {
            locallibrary::harmony::Camelot::parse(k).map_or_else(|| k.clone(), |c| c.code())
        }
        (ColumnKind::Number, Value::Number(_), _) => match column.unit() {
            Some(unit) => format!("{} {unit}", cell(v)),
            None => cell(v),
        },
        _ => cell(v),
    }
}

/// A table's lines: [`table`]'s headings, then its rows.
fn table_lines(columns: &[Column], rows: &[Vec<Value>], names: Names<'_>) -> Vec<Drawn> {
    let mut lines = table(columns, rows, names).into_iter();
    let headings = lines.next().map(Drawn::Columns);
    headings.into_iter().chain(lines.map(Drawn::Row)).collect()
}

/// A table's lines, its columns' headings first: each cell as its column says ([`typed`]), each
/// column as wide as its widest cell (at most [`MAX_COLUMN`]), two spaces apart, numbers and
/// durations aligned right.
pub(crate) fn table(columns: &[Column], rows: &[Vec<Value>], names: Names<'_>) -> Vec<String> {
    let text = Column::Name(String::new());
    let column = |i: usize| columns.get(i).unwrap_or(&text);
    let cells: Vec<Vec<String>> =
        rows.iter().map(|r| r.iter().enumerate().map(|(i, v)| typed(column(i), v, names)).collect()).collect();
    let widths: Vec<usize> = (0..columns.len())
        .map(|i| {
            let widest = cells.iter().filter_map(|r| r.get(i)).map(|c| c.width()).max().unwrap_or(0);
            widest.max(columns[i].name().width()).min(MAX_COLUMN)
        })
        .collect();
    let right = |i: usize| matches!(column(i).kind(), ColumnKind::Number | ColumnKind::Duration);
    let line = |row: &[String]| {
        let fitted = row.iter().zip(&widths).enumerate().map(|(i, (c, w))| {
            let c = output::fit(c, *w);
            match right(i) {
                true => format!("{}{}", " ".repeat(w - c.trim_end().width()), c.trim_end()),
                false => c,
            }
        });
        fitted.collect::<Vec<_>>().join("  ").trim_end().to_string()
    };
    let headings: Vec<String> = columns.iter().map(|c| c.name().to_string()).collect();
    std::iter::once(line(&headings)).chain(cells.iter().map(|r| line(r))).collect()
}

/// Labelled values' lines, the labels aligned.
fn pairs(pairs: &[Pair]) -> Vec<String> {
    let width = pairs.iter().map(|p| p.label.width()).max().unwrap_or(0);
    pairs.iter().map(|p| format!("{}  {}", output::fit(&p.label, width), cell(&p.value))).collect()
}

/// Rows and items as a view command sends them in parts, drawn as they come: a row's cells two
/// spaces apart (the table's widths aren't known yet), an item's detail beside it.
pub(super) fn plain(rows: &[Vec<Value>], items: &[ListItem]) {
    for row in rows {
        say!("{}", row.iter().map(cell).collect::<Vec<_>>().join("  ").trim_end());
    }
    for i in items {
        match &i.detail {
            Some(detail) => say!("{}  {}", i.label, dim(detail)),
            None => say!("{}", i.label),
        }
    }
}

/// What's left of a view once its first rows and items (`drawn`) were drawn as they came.
pub(super) fn rest(v: &View, (rows, items): (usize, usize), names: Names<'_>) {
    match v {
        View::Table { rows: all, .. } => plain(all.get(rows..).unwrap_or_default(), &[]),
        View::List { items: all, .. } => plain(&[], all.get(items..).unwrap_or_default()),
        other => view(other, names),
    }
}

/// A view a plugin's command answered, drawn as the CLI's lists and tables are: its title, then
/// its lines, nothing for what shows nothing (as the CLI's own empty lists).
pub(crate) fn view(v: &View, names: Names<'_>) {
    if let Some(title) = v.title() {
        heading(title);
    }
    let mut lines = lines(v, names);
    if says_nothing(v) {
        lines.pop();
    }
    draw(&lines);
}

/// A view under `title`, a section of what the CLI shows (a panel under a track's details): the
/// title (and the view's, after it), then its lines, saying so when it shows nothing.
pub(crate) fn section(title: &str, v: &View, names: Names<'_>) {
    heading(&v.title().map_or_else(|| title.to_string(), |t| format!("{title} · {t}")));
    match says_nothing(v) {
        true => draw(&[Drawn::Missing]),
        false => draw(&lines(v, names)),
    }
}

/// Lines as the CLI draws them: section titles in bold; a table's headings, details and what's
/// missing dim; an item indented two spaces a level, its detail beside it.
fn draw(lines: &[Drawn]) {
    for line in lines {
        match line {
            Drawn::Heading(title) => heading(title),
            Drawn::Item { label, detail, depth } => {
                let indent = "  ".repeat(*depth);
                match detail {
                    Some(detail) => say!("{indent}{label}  {}", dim(detail)),
                    None => say!("{indent}{label}"),
                }
            }
            Drawn::Columns(headings) => say!("{}", dim(headings)),
            Drawn::Row(text) | Drawn::Text(text) => say!("{text}"),
            Drawn::Missing => say!("{}", dim("nothing to show")),
        }
    }
}

#[cfg(test)]
mod tests;
