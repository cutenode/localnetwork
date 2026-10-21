//! `track show`'s panels: what each plugin's panel shows about each track, under its details.

use std::path::PathBuf;

use anyhow::Result;
use indexmap::IndexMap;
use locallibrary::Error;
use locallibrary::progress::Ctx;
use loclib_plugin::View;
use serde::Serialize;

use super::view::{self, Names};
use crate::output::warn;
use crate::plugins;

/// A panel's view of a track, or why there's none.
#[derive(Debug, Clone, Serialize)]
pub struct Rendered {
    pub plugin: String,
    pub panel: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<View>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Every panel's view of each of `tracks` on `drive`, by track id (none without a panel): a panel
/// that fails says why, and the others show all the same.
pub fn panels(
    drive: &str,
    tracks: &[u32],
    store_path: &Option<PathBuf>,
    ctx: &Ctx<'_>,
) -> Result<IndexMap<u32, Vec<Rendered>>> {
    // Plugins that were refused have none.
    let Ok(plugins) = plugins::host() else { return Ok(IndexMap::new()) };
    let all = plugins.panels();
    if all.is_empty() {
        return Ok(IndexMap::new());
    }
    let store = crate::diff_cmd::existing_store(store_path)?;
    let mut shown = IndexMap::new();
    for &track in tracks {
        let mut each = Vec::new();
        for (plugin, p) in &all {
            let rendered = plugins.render_panel(plugin, &p.name, drive, track, store.as_ref(), ctx);
            let (view, error) = match rendered {
                Ok(v) => (Some(v), None),
                Err(Error::Cancelled) => return Err(Error::Cancelled.into()),
                Err(e) => (None, Some(e.to_string())),
            };
            each.push(Rendered {
                plugin: plugin.to_string(),
                panel: p.name.clone(),
                title: p.title.clone(),
                view,
                error,
            });
        }
        shown.insert(track, each);
    }
    Ok(shown)
}

/// A track's panels as sections under its details: each titled (with its view's title too), and
/// what it shows, or a warning saying why it shows nothing.
pub fn show_panels(rendered: &[Rendered], names: Names<'_>) {
    for r in rendered {
        say!();
        match (&r.view, &r.error) {
            (Some(v), _) => view::section(&r.title, v, names),
            (None, error) => warn(format!("{}: {}", r.title, error.as_deref().unwrap_or_default())),
        }
    }
}
