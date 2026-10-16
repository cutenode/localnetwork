//! What a view command answers: a result each host (CLI, TUI, desktop app) draws its own way.
//! Plugins never ship UI code.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
