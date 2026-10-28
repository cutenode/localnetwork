//! Changing a set: its details, and its tracklist.
//!
//! Tracklist positions start at 1 and always name the tracklist as it was before the edit, so
//! the changes of one edit don't shift each other: dropping track 1 and moving track 5 to 2 puts
//! the track that was fifth where the track that was second is (now first). A track moved up
//! lands just before the track that was at `to`, one moved down just after it, so a move on its
//! own leaves the track at `to`. An inserted track lands just before the track that was at `at`
//! (one past the last appends). Tracks landing in one place keep the order they were given in,
//! inserted ones first.

use chrono::NaiveDate;
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::Deserialize;

use super::{EntryOrigin, Partner, Set, SetEntry};
use crate::model::Library;
use crate::query::{QueryContext, TrackSelector};
use crate::{Error, Result};

/// A set's details. Left out, a field stays as it is; empty (`""`), it's cleared.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SetFields {
    #[serde(default)]
    pub title: Option<String>,
    /// The day it was played, `2026-10-03`.
    #[serde(default)]
    pub date: Option<String>,
    /// `club`, `festival`, `radio`, `livestream`, `mix`, or another word.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub venue: Option<String>,
    #[serde(default)]
    pub city: Option<String>,
    /// ISO 3166 code, `GB`.
    #[serde(default)]
    pub country: Option<String>,
    /// The `at://` URI of the event's `community.lexicon.calendar.event` record.
    #[serde(default)]
    pub event: Option<String>,
    /// Who they played with: names, or `did:...` for an Atmosphere account. Replaces the list;
    /// empty clears it.
    #[serde(default)]
    pub with: Option<Vec<String>>,
    /// Where people can listen to it (`https://...`).
    #[serde(default)]
    pub link: Option<String>,
    /// Private notes, never published.
    #[serde(default)]
    pub notes: Option<String>,
}

impl SetFields {
    /// The day given, if one is.
    pub fn date(&self) -> Result<Option<NaiveDate>> {
        let Some(d) = self.date.as_deref().map(str::trim) else { return Ok(None) };
        if d.is_empty() {
            return Err(Error::invalid("a set needs its day: give one like 2026-10-03"));
        }
        NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| Error::invalid(format!("{d:?} isn't a day: give one like 2026-10-03")))
    }

    /// Write the fields given into `set`, once they're all valid.
    pub fn apply(&self, set: &mut Set) -> Result<()> {
        let date = self.date()?;
        let text = |v: &Option<String>| v.as_deref().map(|s| Some(s.trim().to_string()).filter(|s| !s.is_empty()));
        let country = text(&self.country).map(|c| c.map(|c| c.to_ascii_uppercase()));
        if let Some(Some(c)) = &country
            && !(c.len() == 2 && c.bytes().all(|b| b.is_ascii_alphabetic()))
        {
            return Err(Error::invalid(format!("{c:?} isn't a country code: give two letters, like GB")));
        }
        let event = text(&self.event);
        if let Some(Some(e)) = &event
            && !e.starts_with("at://")
        {
            return Err(Error::invalid(format!("{e:?} isn't an event: give the at:// URI of its record")));
        }
        let link = text(&self.link);
        if let Some(Some(l)) = &link
            && !(l.starts_with("https://") || l.starts_with("http://"))
        {
            return Err(Error::invalid(format!("{l:?} isn't a link: give it with https://")));
        }
        if let Some(d) = date {
            set.date = d;
        }
        // The name the plugin gave the link it published names that link alone.
        if link.is_some() {
            set.link_name = None;
        }
        let fields = [
            (&mut set.title, text(&self.title)),
            (&mut set.kind, text(&self.kind)),
            (&mut set.venue, text(&self.venue)),
            (&mut set.city, text(&self.city)),
            (&mut set.country, country),
            (&mut set.event, event),
            (&mut set.link, link),
            (&mut set.notes, text(&self.notes)),
        ];
        for (field, value) in fields {
            if let Some(v) = value {
                *field = v;
            }
        }
        if let Some(with) = &self.with {
            set.with = partners(with);
        }
        Ok(())
    }
}

/// Who a set was played with: a `did:...` is an Atmosphere account (named by the DID until a
/// handle is looked up), anything else a name. Handles stay names: resolving one takes the
/// network.
fn partners(with: &[String]) -> Vec<Partner> {
    with.iter()
        .map(|w| w.trim())
        .filter(|w| !w.is_empty())
        .map(|w| Partner { name: w.to_string(), did: w.starts_with("did:").then(|| w.to_string()) })
        .collect()
}

/// Changes to a tracklist, by position (see the module docs for how they combine).
#[derive(Debug, Clone, Default)]
pub(crate) struct Changes {
    pub drop: Vec<usize>,
    /// `(from, to)`.
    pub moves: Vec<(usize, usize)>,
    /// `(at, text)`: a search for a library track, or `Artist - Title`.
    pub insert: Vec<(usize, String)>,
    /// Published without naming the track.
    pub unnamed: Vec<usize>,
    pub named: Vec<usize>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.drop.is_empty()
            && self.moves.is_empty()
            && self.insert.is_empty()
            && self.unnamed.is_empty()
            && self.named.is_empty()
    }

    /// Refuse positions a tracklist of `len` tracks doesn't have, and changes that contradict
    /// each other.
    pub fn check(&self, len: usize) -> Result<()> {
        let track = |p: usize| match (p, len) {
            (1.., _) if p <= len => Ok(()),
            (_, 0) => Err(Error::invalid(format!("there's no track {p}: the set has no tracks"))),
            _ => Err(Error::invalid(format!("there's no track {p}: the set has {len}"))),
        };
        let moved: Vec<usize> = self.moves.iter().map(|m| m.0).collect();
        for &p in self.drop.iter().chain(&moved).chain(&self.unnamed).chain(&self.named) {
            track(p)?;
        }
        for &(_, to) in &self.moves {
            track(to)?;
        }
        if let Some((at, _)) = self.insert.iter().find(|(at, _)| *at == 0 || *at > len + 1) {
            return Err(Error::invalid(format!(
                "can't insert at {at}: positions go from 1 to {} (one past the last track appends)",
                len + 1
            )));
        }
        let mut seen = FxHashSet::default();
        if let Some(p) = moved.iter().find(|p| !seen.insert(**p)) {
            return Err(Error::invalid(format!("track {p} is moved twice")));
        }
        if let Some(p) = self.drop.iter().find(|p| moved.contains(p)) {
            return Err(Error::invalid(format!("track {p} can't be dropped and moved")));
        }
        if let Some(p) = self.unnamed.iter().find(|p| self.named.contains(p)) {
            return Err(Error::invalid(format!("track {p} can't be named and unnamed")));
        }
        Ok(())
    }

    /// `tracks` changed, once every change is checked. `find` turns an insert's text into the
    /// entry that goes in.
    pub fn apply(&self, tracks: &[SetEntry], mut find: impl FnMut(&str) -> Result<SetEntry>) -> Result<Vec<SetEntry>> {
        self.check(tracks.len())?;
        let n = tracks.len();
        // What lands just before and just after each original position (`before[n + 1]` appends).
        let mut before: Vec<Vec<SetEntry>> = vec![Vec::new(); n + 2];
        let mut after: Vec<Vec<SetEntry>> = vec![Vec::new(); n + 1];
        for (at, text) in &self.insert {
            before[*at].push(find(text)?);
        }
        let mut kept: Vec<Option<SetEntry>> = tracks.iter().cloned().map(Some).collect();
        for (positions, unnamed) in [(&self.unnamed, true), (&self.named, false)] {
            for p in positions {
                if let Some(e) = kept[p - 1].as_mut() {
                    e.unnamed = unnamed;
                }
            }
        }
        for &(from, to) in self.moves.iter().filter(|(from, to)| from != to) {
            let Some(e) = kept[from - 1].take() else { continue };
            match to < from {
                true => before[to].push(e),
                false => after[to].push(e),
            }
        }
        for p in &self.drop {
            kept[p - 1] = None;
        }
        let mut out = Vec::with_capacity(n + self.insert.len());
        for p in 1..=n {
            out.append(&mut before[p]);
            out.extend(kept[p - 1].take());
            out.append(&mut after[p]);
        }
        out.append(&mut before[n + 1]);
        Ok(out)
    }
}

/// The entry to insert for `text`: the library's track when exactly one matches it as a search
/// (`Artist - Title`, part of it, or a file path), else the text as typed (`Artist - Title`).
/// With a note saying which.
pub(crate) fn lookup(text: &str, library: Option<&Library>) -> Result<(SetEntry, String)> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::invalid("say what to insert: a track to look for, or Artist - Title"));
    }
    let Some(lib) = library else {
        return Ok((SetEntry::added(text), format!("{text:?} went in as typed: no drive to look it up on")));
    };
    let selector = TrackSelector { query: Some(text.to_string()), ..Default::default() };
    let ids = selector.select_ordered(&QueryContext::new(lib), true)?;
    Ok(match lib.tracks_by_id(&ids).as_slice() {
        [t] => (
            SetEntry::from_track(t, EntryOrigin::Added, None),
            format!("{} went in from the library", t.display_name()),
        ),
        [] => (SetEntry::added(text), format!("{text:?} went in as typed: no track matches it")),
        many => (SetEntry::added(text), format!("{text:?} went in as typed: {} tracks match it", many.len())),
    })
}

#[cfg(test)]
mod tests;
