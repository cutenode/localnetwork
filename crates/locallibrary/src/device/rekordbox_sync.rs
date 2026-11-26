//! rekordbox's playlist sync files on a drive, which the `rekordbox_sync` fuzz target reads.

use schemars::JsonSchema;
use serde::Serialize;

/// rekordbox's automatic playlist sync to a drive (`playlists3.sync` / `playlists3Plus.sync`):
/// which playlists of its collection it keeps up to date there. Playlists locallibrary adds
/// aren't in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RekordboxSync {
    /// `device_library` (`export.pdb`) or `onelibrary`.
    pub library: &'static str,
    pub automatic: bool,
    /// rekordbox collection the playlists come from (OneLibrary's `masterDbId`).
    pub collection: Option<u32>,
    /// Playlists and folders rekordbox tracks.
    pub playlists: usize,
}

impl RekordboxSync {
    /// Parse a `playlists3*.sync` file (`<Sync DBID=".." AutomaticSync="1"><Playlists><NODE ../>`).
    pub fn parse(library: &'static str, xml: &str) -> Option<RekordboxSync> {
        let start = xml.find("<Sync ")?;
        let tag = &xml[start..start + xml[start..].find('>')?];
        let attr = |name: &str| {
            let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
            Some(&tag[at..at + tag[at..].find('"')?])
        };
        Some(RekordboxSync {
            library,
            automatic: attr("AutomaticSync") == Some("1"),
            // Written as a signed 32-bit number.
            collection: attr("DBID").and_then(|v| v.parse::<i64>().ok()).map(|v| v as u32),
            playlists: xml.matches("<NODE ").count(),
        })
    }
}
