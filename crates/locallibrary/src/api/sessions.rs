//! Sessions: what players logged, gathered from drives, the store and imported logs.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{known_drives, resolve_drive};
use crate::Result;
use crate::device::Device;
use crate::plays;
use crate::progress::Ctx;
use crate::sets::session::{self, Gathered};
use crate::sets::suggest::{self, Candidate};
use crate::sets::{ImportedSession, Session, SetBook, import};
use crate::store::Store;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SessionsRequest {
    /// Only this drive's sessions and those of its library in the store. Default: every mounted
    /// rekordbox drive's and the whole store's.
    #[serde(default)]
    pub drive: Option<String>,
    /// Only the sessions in no set that weren't dismissed (`sessions.dismiss`).
    #[serde(default)]
    pub unassigned: bool,
}

/// Every session, newest first, each once: the history lists on drives, every list the store's
/// snapshots hold, and the logs imported from other tools, with the window each was recorded in
/// and the sets built from it.
pub fn sessions(store: Option<&Store>, req: &SessionsRequest, ctx: &Ctx<'_>) -> Result<Gathered> {
    let book = store.map(SetBook::load).transpose()?.unwrap_or_default();
    let (mut got, _) = gather(store, req.drive.as_deref(), &book, ctx)?;
    if req.unassigned {
        got.sessions.retain(|s| s.sets.is_empty() && !s.dismissed);
    }
    Ok(got)
}

/// The sessions of `drive` and its library's in the store, else of every mounted rekordbox drive
/// and the whole store; and the drives read.
pub(super) fn gather(
    store: Option<&Store>,
    drive: Option<&str>,
    book: &SetBook,
    ctx: &Ctx<'_>,
) -> Result<(Gathered, Vec<Device>)> {
    let Some(spec) = drive else {
        let drives: Vec<Device> = known_drives(false).into_iter().filter(Device::has_export).collect();
        return Ok((session::gather(store, &drives, book, ctx)?, drives));
    };
    let drives = vec![resolve_drive(spec)?];
    let library = drives[0].state().and_then(|s| s.library_id);
    let archive = store.map(|s| plays::archive(s, library, ctx)).transpose()?;
    Ok((session::gather_from(archive.as_ref(), &drives, book), drives))
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct DismissSessions {
    /// Ids (any unique prefix of 4 or more characters) or list names.
    pub sessions: Vec<String>,
    /// Take the dismissal back.
    #[serde(default)]
    pub undo: bool,
}

/// Say sessions weren't a set, so `sets.suggest` leaves them out (or, with `undo`, that they
/// might be). Answers their ids.
pub fn sessions_dismiss(store: &Store, req: &DismissSessions, ctx: &Ctx<'_>) -> Result<Vec<String>> {
    let (got, _) = gather(Some(store), None, &SetBook::load(store)?, ctx)?;
    let ids =
        req.sessions.iter().map(|q| Ok(session::find(&got.sessions, q)?.id.clone())).collect::<Result<Vec<_>>>()?;
    // Read again: finding the sessions takes a while, and the book may have changed since.
    let mut book = SetBook::load(store)?;
    for id in &ids {
        if req.undo {
            book.dismissed.remove(id);
        } else {
            book.dismissed.insert(id.clone());
        }
    }
    book.save(store)?;
    Ok(ids)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ImportSessionLog {
    /// A Beat Link Trigger Playlist Writer CSV.
    pub file: PathBuf,
    /// The drive whose library the log's tracks are looked up in. Default: the only mounted
    /// rekordbox drive (with none mounted, the log is kept as it is).
    #[serde(default)]
    pub drive: Option<String>,
    /// Default: the file's name.
    #[serde(default)]
    pub name: Option<String>,
}

/// Keep a play log from another tool in the store as a session. The same log imported again
/// replaces its session.
pub fn sessions_import(store: &Store, req: &ImportSessionLog) -> Result<ImportedSession> {
    let library = super::sets::drive_library(req.drive.as_deref())?;
    let log = import::beat_link_trigger(&req.file, req.name.as_deref(), library.as_ref())?;
    let mut book = SetBook::load(store)?;
    book.imported.retain(|s| s.id != log.id);
    book.imported.push(log.clone());
    book.save(store)?;
    Ok(log)
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SuggestSets {
    /// Only this drive's sessions and those of its library in the store. Default: every mounted
    /// rekordbox drive's and the whole store's.
    #[serde(default)]
    pub drive: Option<String>,
}

/// The sets the sessions suggest, and the sessions they're made of.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Suggestions {
    /// Newest first.
    pub candidates: Vec<Candidate>,
    /// The candidates' sessions, newest first.
    pub sessions: Vec<Session>,
}

/// The sets the sessions suggest, newest first: the sessions in no set that weren't dismissed,
/// grouped by night where their windows are narrow enough to tell, with the evidence that each
/// was a performance.
pub fn sets_suggest(store: &Store, req: &SuggestSets, ctx: &Ctx<'_>) -> Result<Suggestions> {
    let (mut got, drives) = gather(Some(store), req.drive.as_deref(), &SetBook::load(store)?, ctx)?;
    let candidates = suggest::suggest(store, &got.sessions, &drives)?;
    got.sessions.retain(|s| candidates.iter().any(|c| c.sessions.contains(&s.id)));
    Ok(Suggestions { candidates, sessions: got.sessions })
}
