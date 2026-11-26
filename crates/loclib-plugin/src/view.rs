//! What a view command (or a panel) answers: a result each host (CLI, TUI, desktop app) draws its
//! own way, or a question for the user first. Plugins never ship UI code. Every kind has a plain
//! text form for a host that can't draw it, and for the command line: see the crate's README.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Choice;

/// What a view command or a panel answers, by `kind`: what to show, or a question for the user
/// first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum View {
    /// Lines, each with an optional second line.
    List {
        /// Its heading, if it has one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// The lines, in order.
        items: Vec<ListItem>,
    },
    /// Rows of cells (strings, numbers, booleans or `null`) under column headings: each a name,
    /// or (feature `views`) a name with what its cells are ([`Column`]).
    Table {
        /// Its heading, if it has one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// The columns, in order.
        columns: Vec<Column>,
        /// The rows, each a cell a column, in the columns' order.
        rows: Vec<Vec<Value>>,
    },
    /// Labelled values, in order.
    Kv {
        /// Its heading, if it has one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// The labelled values.
        pairs: Vec<Pair>,
    },
    /// Plain text.
    Text {
        /// Its heading, if it has one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// The text, shown as it is.
        text: String,
    },
    /// A question for the user (feature `choose`): the host asks it, then runs the command again
    /// with what was chosen (see [`Choice`]). It's never drawn.
    Choose(Choice),
    /// Items with items below them (feature `views`).
    Tree {
        /// Its heading, if it has one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// The items at the top, each with the items below it.
        items: Vec<TreeItem>,
    },
    /// Points, each series a line or bars (feature `views`).
    Chart {
        /// Its heading, if it has one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Whether its series are lines or bars.
        style: ChartStyle,
        /// Its series, each named.
        series: Vec<Series>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// What the x axis measures.
        x_label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// What the y axis measures.
        y_label: Option<String>,
    },
    /// A picture (features `views` and `blobs`): a file in the request's scratch folder, which
    /// the host reads (see [`crate::Blob`]).
    Image {
        /// Its heading, if it has one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Its file's name in the scratch folder.
        blob: String,
        /// What it shows, in a few words: what a host that can't draw it says instead.
        alt: String,
        /// The picture as a `data:` URL, filled in by the host once it has read the blob and
        /// found an image there (locallibrary's: a PNG, shrunk to a preview): whatever a plugin
        /// sends here is replaced.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<String>,
    },
    /// Views one after another, each under a title of its own (feature `views`).
    Sections {
        /// Its heading, if it has one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// The sections, in order. None of them is a question.
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

/// A line of a list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ListItem {
    /// The line itself.
    pub label: String,
    /// More about it, shown beside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// A labelled value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Pair {
    /// What the value is.
    pub label: String,
    /// The value, shown beside its label.
    pub value: Value,
}

/// A table's column: its heading alone (text), or (feature `views`) a heading and what its cells
/// are.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Column {
    /// Its heading alone: its cells are text.
    Name(String),
    /// Its heading and what its cells are (feature `views`).
    Typed(TypedColumn),
}

impl Column {
    /// Its heading.
    pub fn name(&self) -> &str {
        match self {
            Column::Name(name) => name,
            Column::Typed(c) => &c.name,
        }
    }

    /// What its cells are: text, for a heading alone.
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
    /// Its heading.
    pub name: String,
    /// What its cells are: `text` unless it says.
    #[serde(default)]
    pub kind: ColumnKind,
    /// What a `number` column's numbers are in (`BPM`), said after each.
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

/// An item of a tree, with the items below it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TreeItem {
    /// The item itself.
    pub label: String,
    /// More about it, shown beside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The items below it, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TreeItem>,
}

/// How a chart draws its series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChartStyle {
    /// Each series as a line through its points.
    Line,
    /// Each series as bars, one for each point.
    Bar,
}

/// A chart's series: its name and its points, `[x, y]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Series {
    /// What it is, as the chart's legend names it.
    pub name: String,
    /// Its points, `[x, y]`, in order.
    pub points: Vec<(f64, f64)>,
}

/// A part of a `sections` view: a view under a title of its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Section {
    /// Its title, shown above its view.
    pub title: String,
    /// What it shows: any view but a question.
    pub view: View,
}

#[cfg(test)]
mod tests;
