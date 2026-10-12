//! Questions across sets: when and where a track was played, and which sets a listing shows.

use chrono::NaiveDate;
use schemars::JsonSchema;
use serde::Serialize;

use super::{Set, SetBook, newest};
use crate::model::normalize;
use crate::paths;

/// A track's place in a set.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Appearance {
    /// The set's id.
    pub set: String,
    pub date: NaiveDate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub venue: Option<String>,
    /// From 1.
    pub position: usize,
    /// Seconds from the start of the set, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_seconds: Option<u32>,
    /// The track as the set has it: `Artist - Title`.
    pub track: String,
}

/// Every time a track matching `query` was played in `book`'s sets, newest first: by file when
/// the query is a drive path (`/Contents/...`), else by `Artist - Title` containing it (ignoring
/// case and spacing).
pub(crate) fn played(book: &SetBook, query: &str) -> Vec<Appearance> {
    let query = query.trim();
    let file = query.starts_with('/').then(|| paths::key(query).to_lowercase());
    let needle = normalize(query);
    let matches = |e: &super::SetEntry| match &file {
        Some(f) => e.file_key().as_ref() == Some(f),
        None => normalize(&e.name()).contains(&needle),
    };
    let mut sets: Vec<&Set> = book.sets.iter().collect();
    sets.sort_by(|a, b| Set::newest_first(a, b));
    sets.into_iter()
        .flat_map(|s| {
            s.tracks.iter().enumerate().filter(|(_, e)| matches(e)).map(|(i, e)| Appearance {
                set: s.id.clone(),
                date: s.date,
                title: s.title.clone(),
                venue: s.venue.clone(),
                position: i + 1,
                start_seconds: e.start_seconds,
                track: e.name(),
            })
        })
        .collect()
}

/// Which sets a listing shows.
#[derive(Debug, Clone, Default)]
pub(crate) struct SetFilter {
    /// Part of the venue's name, ignoring case.
    pub venue: Option<String>,
    /// The kind, ignoring case.
    pub kind: Option<String>,
    /// Played on or after.
    pub since: Option<NaiveDate>,
    /// Played on or before.
    pub until: Option<NaiveDate>,
}

impl SetFilter {
    pub fn matches(&self, s: &Set) -> bool {
        let has = |field: &Option<String>, wanted: &Option<String>, whole: bool| match wanted {
            None => true,
            Some(w) => field.as_deref().map(normalize).is_some_and(|f| match whole {
                true => f == normalize(w),
                false => f.contains(&normalize(w)),
            }),
        };
        has(&s.venue, &self.venue, false)
            && has(&s.kind, &self.kind, true)
            && self.since.is_none_or(|d| s.date >= d)
            && self.until.is_none_or(|d| s.date <= d)
    }

    /// The newest of `book`'s sets it shows.
    pub fn latest<'a>(&self, book: &'a SetBook) -> Option<&'a Set> {
        newest(book.sets.iter().enumerate().filter(|(_, s)| self.matches(s))).map(|i| &book.sets[i])
    }
}

#[cfg(test)]
pub(crate) mod tests;
