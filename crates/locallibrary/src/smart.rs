//! Rule-based ("smart") playlists: a filter, sort order and limit kept in the store and written to
//! drives as ordinary playlists, which is what players can show. rekordbox's own Intelligent
//! Playlists reach a drive the same way, as fixed lists from export time.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::edit::Edit;
use crate::fields::Declared;
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
    /// The libraries it's limited to, by a library's id or one of its drives' (any clone of that
    /// drive's library takes it, as clones are kept identical); every drive when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drives: Vec<Uuid>,
}

impl SmartPlaylist {
    /// Refuse a rule that can't be applied: one without a path or a filter, or whose filter or
    /// sort can't be read, or names a plugin's field none of the analyzers installed declares
    /// (`declared` are those they do) or a value its field can't hold.
    pub fn validate(&self, declared: &Declared) -> Result<()> {
        if self.path.split('/').all(|p| p.trim().is_empty()) {
            return Err(Error::invalid("give the playlist a path, e.g. Smart/Peak time"));
        }
        // A blank filter would select every track, which applying the rule refuses.
        if self.filter.trim().is_empty() {
            return Err(Error::invalid("give the rule a filter, e.g. \"bpm:126-132 rating:>=4\""));
        }
        Filter::parse(&self.filter)?.check_fields(declared)?;
        if let Some(s) = &self.sort {
            query::parse_sort_with(s, Some(declared))?;
        }
        Ok(())
    }

    /// The path as edits split it: names trimmed, empty ones left out (`Smart / Peak` is
    /// `Smart/Peak`). Rules are told apart by it, exactly: `Foo` and `FoO` are two rules, which
    /// fill two playlists.
    fn key(&self) -> String {
        key(&self.path)
    }

    /// Whether it applies to a drive that answers to `ids`: its own, its library's and its
    /// library's other drives'.
    pub fn applies_to(&self, ids: &[Uuid]) -> bool {
        self.drives.is_empty() || ids.iter().any(|id| self.drives.contains(id))
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

    /// Add a rule, or replace the one for exactly its path, once it's valid (see
    /// [`SmartPlaylist::validate`]).
    pub fn set(&mut self, rule: SmartPlaylist, declared: &Declared) -> Result<()> {
        rule.validate(declared)?;
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

    /// The edits that bring a drive's smart playlists up to date (see [`SmartPlaylist::applies_to`]).
    pub fn edits_for(&self, ids: &[Uuid]) -> Vec<Edit> {
        self.playlists.iter().filter(|p| p.applies_to(ids)).map(SmartPlaylist::edit).collect()
    }
}

#[cfg(test)]
mod tests;
