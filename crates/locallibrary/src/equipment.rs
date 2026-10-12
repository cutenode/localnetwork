//! What each player, all-in-one system and mixer reads ([`loclib_equipment`], whose docs say
//! where each value comes from), and the models a DJ plays on, remembered in the store.
//!
//! The table's exFAT support is what [`crate::format::players::EXFAT`] and
//! [`crate::format::players::NO_EXFAT`] list, and [`crate::format::players`] has why MBR's 2 TiB
//! is the largest drive players use.

use indexmap::IndexMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use loclib_equipment::*;

/// The models a DJ plays on, kept in the store (`players.json`) so `check` uses them when no
/// `--players` are given: a list per library, and a default for drives of any other library.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Remembered {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default: Vec<String>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub libraries: IndexMap<Uuid, Vec<String>>,
}

impl Remembered {
    /// The models for a drive of `library`, and whether they're that library's own (rather than
    /// the default).
    pub fn for_library(&self, library: Option<Uuid>) -> (&[String], bool) {
        match library.and_then(|id| self.libraries.get(&id)) {
            Some(models) => (models, true),
            None => (&self.default, false),
        }
    }
}

#[cfg(test)]
mod tests;
