//! What a view command answers: a result each host (CLI, TUI, desktop app) draws its own way, or
//! a question for the user first. Plugins never ship UI code.

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
    /// Rows of cells (strings, numbers, booleans or `null`) under column headings.
    Table {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        columns: Vec<String>,
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
}

impl View {
    /// What it says it is (a question, what it asks).
    pub fn title(&self) -> Option<&str> {
        match self {
            View::List { title, .. }
            | View::Table { title, .. }
            | View::Kv { title, .. }
            | View::Text { title, .. } => title.as_deref(),
            View::Choose(c) => Some(&c.title),
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
