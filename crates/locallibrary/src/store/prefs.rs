//! What's set for the store on this computer, kept in the store's [`PREFS_FILE`]. A store copy
//! doesn't take it (see [`crate::offsite`]): it's about this computer's disks.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::store::Store;
use crate::{Result, fsutil};

/// The file in the store that [`StorePrefs`] are kept in.
pub const PREFS_FILE: &str = "store-prefs.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct StorePrefs {
    /// Whether `pull` makes a track rekordbox's collection holds on the store's volume a
    /// copy-on-write copy of the collection's file rather than copying the drive's. It takes no
    /// space of its own, but the store is then no second physical copy of the track, and a first
    /// pull reads every file of the collection that's the size of one on the drive. Off unless
    /// turned on.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub copy_on_write: bool,
}

impl StorePrefs {
    pub fn load(store: &Store) -> Result<StorePrefs> {
        Ok(fsutil::read_json_opt(&store.root().join(PREFS_FILE))?.unwrap_or_default())
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        fsutil::write_json_atomic(&store.root().join(PREFS_FILE), self)
    }
}
