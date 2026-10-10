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
mod tests {
    use super::*;
    use crate::Error;
    use crate::format::players::{EXFAT, NO_EXFAT};

    /// The table agrees with the exFAT lists `disks` and `format` show.
    #[test]
    fn agrees_with_the_exfat_lists() {
        let parse = |entry: &str| -> Vec<(String, Option<String>)> {
            let (names, note) = match entry.split_once(" (") {
                Some((n, rest)) => {
                    (n, rest.trim_end_matches(')').strip_prefix("firmware ").map(|v| v.trim_end_matches('+')))
                }
                None => (entry, None),
            };
            names.split('/').map(|n| (n.to_string(), note.map(str::to_string))).collect()
        };
        for (name, firmware) in EXFAT.iter().flat_map(|e| parse(e)) {
            let m = find(&name).unwrap_or_else(|| panic!("{name} isn't in the table"));
            match (firmware, m.exfat.value) {
                (Some(v), Exfat::Firmware(f)) => assert_eq!(f, v, "{name}"),
                (None, Exfat::Yes) => {}
                (listed, table) => panic!("{name}: the list says {listed:?}, the table {table:?}"),
            }
        }
        for (name, _) in NO_EXFAT.iter().flat_map(|e| parse(e)) {
            let m = find(&name).unwrap_or_else(|| panic!("{name} isn't in the table"));
            assert_eq!(m.exfat.value, Exfat::No, "{name}");
        }
        let listed = EXFAT.len() + NO_EXFAT.iter().map(|e| parse(e).len()).sum::<usize>();
        assert_eq!(listed, MODELS.len(), "every model is on one of the lists");
    }

    #[test]
    fn remembered_models() {
        let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let e = Error::from(canonical(&names(&["CDJ-3000", "XYZ-9"])).unwrap_err());
        assert_eq!(e.code(), "invalid");
        let e = e.to_string();
        assert!(
            e.starts_with("not a model locallibrary knows: XYZ-9 (it knows CDJ-3000X, ") && e.contains("OPUS-QUAD"),
            "{e}"
        );
        let lib = Uuid::new_v4();
        let r = Remembered { default: names(&["CDJ-3000"]), libraries: [(lib, names(&["OPUS-QUAD"]))].into() };
        assert_eq!(r.for_library(Some(lib)), (&names(&["OPUS-QUAD"])[..], true));
        assert_eq!(r.for_library(Some(Uuid::new_v4())), (&names(&["CDJ-3000"])[..], false));
        assert_eq!(r.for_library(None), (&names(&["CDJ-3000"])[..], false));
    }
}
