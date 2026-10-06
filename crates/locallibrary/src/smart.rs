//! Rule-based ("smart") playlists: a filter, sort order and limit kept in the store and written to
//! drives as ordinary playlists, which is what players can show. rekordbox's own Intelligent
//! Playlists reach a drive the same way, as fixed lists from export time.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::edit::Edit;
use crate::query::{self, Filter, TrackSelector};
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SmartPlaylist {
    /// Where the playlist goes on a drive, e.g. `Smart/Peak time`.
    pub path: String,
    /// See [`crate::query`].
    pub filter: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// Drives it applies to; every drive when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drives: Vec<Uuid>,
}

impl SmartPlaylist {
    pub fn validate(&self) -> Result<()> {
        if self.path.split('/').all(|p| p.trim().is_empty()) {
            return Err(Error::invalid("give the playlist a path, e.g. Smart/Peak time"));
        }
        // A blank filter would select every track, which applying the rule refuses.
        if self.filter.trim().is_empty() {
            return Err(Error::invalid("give the rule a filter, e.g. \"bpm:126-132 rating:>=4\""));
        }
        Filter::parse(&self.filter)?;
        if let Some(s) = &self.sort {
            query::parse_sort(s)?;
        }
        Ok(())
    }

    /// The path as edits split it: names trimmed, empty ones left out (`Smart / Peak` is
    /// `Smart/Peak`). Rules are told apart by it, exactly: `Foo` and `FoO` are two rules, which
    /// fill two playlists.
    fn key(&self) -> String {
        key(&self.path)
    }

    pub fn applies_to(&self, drive: Option<Uuid>) -> bool {
        self.drives.is_empty() || drive.is_some_and(|d| self.drives.contains(&d))
    }

    /// The edit that brings a drive's copy up to date.
    pub fn edit(&self) -> Edit {
        Edit::SetTracks {
            playlist: self.path.clone(),
            tracks: TrackSelector { filter: Some(self.filter.clone()), ..Default::default() },
            sort: self.sort.clone(),
            limit: self.limit,
            create: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SmartPlaylists {
    #[serde(default)]
    pub playlists: Vec<SmartPlaylist>,
}

fn key(path: &str) -> String {
    path.split('/').map(str::trim).filter(|n| !n.is_empty()).collect::<Vec<_>>().join("/")
}

impl SmartPlaylists {
    /// The rule for exactly `path`.
    pub fn get(&self, path: &str) -> Option<&SmartPlaylist> {
        self.playlists.iter().find(|p| p.key() == key(path))
    }

    /// Add a rule, or replace the one for exactly its path.
    pub fn set(&mut self, rule: SmartPlaylist) -> Result<()> {
        rule.validate()?;
        match self.playlists.iter_mut().find(|p| p.key() == rule.key()) {
            Some(p) => *p = rule,
            None => self.playlists.push(rule),
        }
        Ok(())
    }

    /// Remove the rule for exactly `path`, else the only one for it ignoring case.
    pub fn remove(&mut self, path: &str) -> Result<SmartPlaylist> {
        let wanted = key(path);
        let exact = self.playlists.iter().position(|p| p.key() == wanted);
        let loose: Vec<usize> =
            (0..self.playlists.len()).filter(|i| self.playlists[*i].key().eq_ignore_ascii_case(&wanted)).collect();
        let i = match (exact, loose.as_slice()) {
            (Some(i), _) | (None, &[i]) => i,
            (None, []) => return Err(Error::NotFound(format!("smart playlist {path:?}"))),
            (None, _) => {
                let paths: Vec<&str> = loose.iter().map(|i| self.playlists[*i].path.as_str()).collect();
                return Err(Error::invalid(format!(
                    "{path:?} matches {} smart playlists ({}); give its exact path",
                    loose.len(),
                    paths.join(", ")
                )));
            }
        };
        Ok(self.playlists.remove(i))
    }

    /// The edits that bring a drive's smart playlists up to date.
    pub fn edits_for(&self, drive: Option<Uuid>) -> Vec<Edit> {
        self.playlists.iter().filter(|p| p.applies_to(drive)).map(SmartPlaylist::edit).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_are_validated_and_replaced_by_path() {
        let mut s = SmartPlaylists::default();
        let rule = |path: &str, filter: &str| SmartPlaylist {
            path: path.into(),
            filter: filter.into(),
            sort: Some("-rating".into()),
            limit: None,
            drives: vec![],
        };
        s.set(rule("Smart/Peak", "bpm:>=128")).unwrap();
        s.set(rule("Smart / Peak ", "bpm:>=130")).unwrap();
        assert_eq!(s.playlists.len(), 1);
        assert_eq!(s.playlists[0].filter, "bpm:>=130");
        assert!(s.set(rule("Smart/Bad", "key:H")).is_err());
        assert!(s.set(rule("", "bpm:1")).is_err());
        assert!(s.set(rule("Smart/All", "")).is_err() && s.set(rule("Smart/All", "  ")).is_err());
        let only = Uuid::new_v4();
        s.set(SmartPlaylist { drives: vec![only], ..rule("Tokyo", "genre:techno") }).unwrap();
        assert_eq!(s.edits_for(None).len(), 1);
        assert_eq!(s.edits_for(Some(only)).len(), 2);
        s.remove("SMART/PEAK").unwrap();
        assert!(s.remove("nope").is_err());
    }

    /// Regression: rules were keyed by path ignoring case, so a rule for `FoO` replaced the one
    /// for `Foo` (and filled its playlist).
    #[test]
    fn rules_whose_paths_differ_only_in_case_are_two_rules() {
        let mut s = SmartPlaylists::default();
        let rule = |path: &str| SmartPlaylist {
            path: path.into(),
            filter: "rating:5".into(),
            sort: None,
            limit: None,
            drives: vec![],
        };
        s.set(rule("Smart/Foo")).unwrap();
        s.set(rule("Smart/FoO")).unwrap();
        assert_eq!(s.playlists.len(), 2);
        assert!(s.remove("smart/foo").is_err(), "ambiguous");
        assert_eq!(s.remove("Smart/FoO").unwrap().path, "Smart/FoO");
        assert_eq!(s.remove("smart/foo").unwrap().path, "Smart/Foo");
    }
}
