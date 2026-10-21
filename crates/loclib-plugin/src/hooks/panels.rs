//! Feature `panels`: what a plugin shows about a track, under its details: a view each host draws
//! its own way, asked for as the user comes to the track.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::manifest::{invalid, shown, unique_names};
use crate::sdk::declared;
use crate::{Context, Contributions, Feature, Hook, Manifest, ManifestError, RpcError, View, Where};

/// Host to plugin (feature `panels`): [`RenderPanel`] -> [`View`].
pub const RENDER_PANEL: &str = "panel/render";

pub(crate) const FEATURE: Feature =
    Feature { name: "panels", methods: &[RENDER_PANEL], sends: &[], contributed: |m| !m.contributes.panels.is_empty() };

/// `panel/render` (feature `panels`): the panel's view of the track in `context.tracks` (the one
/// shown, by id) on `context.drive`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RenderPanel {
    /// One of the manifest's panels.
    pub panel: String,
    #[serde(default)]
    pub context: Context,
}

/// A [`View`] of the track, never a question ([`View::Choose`]): a panel asks nothing.
impl Hook for RenderPanel {
    const METHOD: &'static str = RENDER_PANEL;
    type Answer = View;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        declared("panel", &self.panel, c.panels.iter().map(|x| &x.name))
    }
}

/// Shown under a track's details, as `panel/render` answers for the track. Hosts ask for it as
/// the user comes to a track, so it answers quickly, reading only what it needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Panel {
    pub name: String,
    pub title: String,
    /// What it's shown with: a track, unless it says.
    #[serde(default = "a_track", rename = "where")]
    pub offered: Where,
}

fn a_track() -> Where {
    Where::Track
}

/// `[[contributes.panels]]`: names and titles, each shown with a track.
pub(crate) fn validate(m: &Manifest) -> Result<(), ManifestError> {
    let (at, c) = ("contributes.panels", &m.contributes);
    unique_names(at, "name", c.panels.iter().map(|x| x.name.as_str()).enumerate())?;
    for (i, x) in c.panels.iter().enumerate() {
        shown(format!("{at}[{i}].title"), &x.title)?;
        if x.offered != Where::Track {
            return Err(invalid(format!("{at}[{i}].where"), "a panel is shown with a track: where = \"track\""));
        }
    }
    Ok(())
}
