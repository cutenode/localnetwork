//! What a check finds about a library: one shape for locallibrary's checks (the check before a set,
//! doctor) and plugins' alike, so a front end shows them all one way.

use serde::{Deserialize, Serialize};

/// How much a finding matters, least first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Worth knowing, nothing to fix (what was checked and found fine, too).
    Info,
    /// Worth a look: it may be a problem, or a problem for some players.
    Warning,
    /// Something to fix before it's used.
    Problem,
}

/// One thing a check found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Finding {
    pub level: Level,
    /// The check that found it (`files`, a plugin's `<plugin>.<check>`).
    #[serde(default)]
    pub check: String,
    /// What it found, in a line.
    pub message: String,
    /// The tracks it's about, by id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracks: Vec<u32>,
    /// The playlist it's about, by path (`Folder/Name`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playlist: Option<String>,
    /// The files it's about, relative to the drive's root (`Contents/a.mp3`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

impl Finding {
    /// About the library as a whole: no track, playlist or file named.
    pub fn new(level: Level, check: impl Into<String>, message: impl Into<String>) -> Finding {
        Finding {
            level,
            check: check.into(),
            message: message.into(),
            tracks: Vec::new(),
            playlist: None,
            paths: Vec::new(),
        }
    }
}
