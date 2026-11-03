//! What's set for identifying mixes, kept in the store's [`PREFS_FILE`].

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::store::Store;
use crate::{Result, fsutil};

/// The file in the store that [`MixPrefs`] are kept in.
pub const PREFS_FILE: &str = "mix.json";

/// The largest fingerprint index kept for a drive (MiB) unless [`MixPrefs::index_limit_mib`]
/// says otherwise: about 3,000 six-minute tracks at the most landmarks a second.
pub const DEFAULT_INDEX_LIMIT_MIB: u64 = 512;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct MixPrefs {
    /// The largest fingerprint index kept for a drive between runs, in MiB: a bigger one is built
    /// again each run instead. 0 keeps none. Unset, [`DEFAULT_INDEX_LIMIT_MIB`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_limit_mib: Option<u64>,
}

impl MixPrefs {
    pub fn load(store: &Store) -> Result<MixPrefs> {
        Ok(fsutil::read_json_opt(&store.root().join(PREFS_FILE))?.unwrap_or_default())
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        fsutil::write_json_atomic(&store.root().join(PREFS_FILE), self)
    }

    /// The largest index kept, in bytes.
    pub fn index_limit(&self) -> u64 {
        self.index_limit_mib.unwrap_or(DEFAULT_INDEX_LIMIT_MIB).saturating_mul(1 << 20)
    }
}

#[cfg(test)]
mod tests;
