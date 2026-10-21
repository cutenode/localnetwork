//! What a view command (or a panel) answers: a result each host (CLI, TUI, desktop app) draws its
//! own way, or a question for the user first. Plugins never ship UI code. Every kind has a plain
//! text form for a host that can't draw it, and for the command line: see the crate's README.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Choice;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum View {
    /// Lines, each with an optional second line.
    List {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        items: Vec<ListItem>,
    },
    /// Rows of cells (strings, numbers, booleans or `null`) under column headings: each a name,
    /// or (feature `views`) a name with what its cells are ([`Column`]).
    Table {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        columns: Vec<Column>,
        rows: Vec<Vec<Value>>,
    },
    /// Labelled values, in order.
    Kv {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        pairs: Vec<Pair>,
    },
    /// Plain text.
    Text {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        text: String,
    },
    /// A question for the user (feature `choose`): the host asks it, then runs the command again
    /// with what was chosen (see [`Choice`]). It's never drawn.
    Choose(Choice),
    /// Items with items below them (feature `views`).
    Tree {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        items: Vec<TreeItem>,
    },
    /// Points, each series a line or bars (feature `views`).
    Chart {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        style: ChartStyle,
        series: Vec<Series>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x_label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        y_label: Option<String>,
    },
    /// A picture (features `views` and `blobs`): a file in the request's scratch folder, which
    /// the host reads (see [`crate::Blob`]).
    Image {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Its file's name in the scratch folder.
        blob: String,
        /// What it shows, in a few words: what a host that can't draw it says instead.
        alt: String,
        /// The picture as a `data:` URL, filled in by the host once it has read the blob and
        /// found an image there: whatever a plugin sends here is replaced.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<String>,
    },
    /// Views one after another, each under a title of its own (feature `views`).
    Sections {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        items: Vec<Section>,
    },
}

impl View {
    /// What it says it is (a question, what it asks).
    pub fn title(&self) -> Option<&str> {
        match self {
            View::List { title, .. }
            | View::Table { title, .. }
            | View::Kv { title, .. }
            | View::Text { title, .. }
            | View::Tree { title, .. }
            | View::Chart { title, .. }
            | View::Image { title, .. }
            | View::Sections { title, .. } => title.as_deref(),
            View::Choose(c) => Some(&c.title),
        }
    }

    /// Whether it's (or has, in a section) what only a host with the `views` feature draws: a
    /// tree, a chart, an image, sections, or a table with a typed column.
    pub fn needs_views(&self) -> bool {
        match self {
            View::Table { columns, .. } => columns.iter().any(|c| matches!(c, Column::Typed(_))),
            View::Tree { .. } | View::Chart { .. } | View::Image { .. } | View::Sections { .. } => true,
            View::List { .. } | View::Kv { .. } | View::Text { .. } | View::Choose(_) => false,
        }
    }

    /// Whether it is, or has in a section, what a section can't be: a question.
    pub fn asks(&self) -> bool {
        match self {
            View::Choose(_) => true,
            View::Sections { items, .. } => items.iter().any(|s| s.view.asks()),
            _ => false,
        }
    }

    /// Every image in it, a section's too, in order: for a host to read their blobs.
    pub fn images_mut(&mut self) -> Vec<&mut View> {
        match self {
            View::Image { .. } => vec![self],
            View::Sections { items, .. } => items.iter_mut().flat_map(|s| s.view.images_mut()).collect(),
            _ => Vec::new(),
        }
    }

    /// Whether it shows nothing: no items, rows, pairs, points or text (a picture always shows
    /// something), nor a section that does.
    pub fn is_empty(&self) -> bool {
        match self {
            View::List { items, .. } => items.is_empty(),
            View::Table { rows, .. } => rows.is_empty(),
            View::Kv { pairs, .. } => pairs.is_empty(),
            View::Text { text, .. } => text.trim().is_empty(),
            View::Tree { items, .. } => items.is_empty(),
            View::Chart { series, .. } => series.iter().all(|s| s.points.is_empty()),
            View::Image { .. } | View::Choose(_) => false,
            View::Sections { items, .. } => items.iter().all(|s| s.view.is_empty()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ListItem {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Pair {
    pub label: String,
    pub value: Value,
}

/// A table's column: its heading alone (text), or (feature `views`) a heading and what its cells
/// are.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Column {
    Name(String),
    Typed(TypedColumn),
}

impl Column {
    pub fn name(&self) -> &str {
        match self {
            Column::Name(name) => name,
            Column::Typed(c) => &c.name,
        }
    }

    pub fn kind(&self) -> ColumnKind {
        match self {
            Column::Name(_) => ColumnKind::Text,
            Column::Typed(c) => c.kind,
        }
    }

    /// What its numbers are in (`BPM`), said after each.
    pub fn unit(&self) -> Option<&str> {
        match self {
            Column::Name(_) => None,
            Column::Typed(c) => c.unit.as_deref(),
        }
    }
}

impl From<&str> for Column {
    fn from(name: &str) -> Column {
        Column::Name(name.into())
    }
}

/// A column and what its cells are.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TypedColumn {
    pub name: String,
    #[serde(default)]
    pub kind: ColumnKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// What a column's cells are, for a host to show them as it shows its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ColumnKind {
    /// As they are.
    #[default]
    Text,
    /// A track, by id: shown by its artist and title where the host has the drive's library.
    Track,
    /// Seconds: shown as minutes and seconds (`3:07`).
    Duration,
    /// A key (`8A`, `Am`): shown in Camelot notation where it's one.
    Key,
    /// Numbers, with the column's unit after each.
    Number,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TreeItem {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TreeItem>,
}

/// How a chart draws its series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChartStyle {
    Line,
    Bar,
}

/// A chart's series: its name and its points, `[x, y]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Series {
    pub name: String,
    pub points: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Section {
    pub title: String,
    pub view: View,
}

#[cfg(test)]
mod tests;
