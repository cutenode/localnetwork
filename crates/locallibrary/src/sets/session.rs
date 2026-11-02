//! Sessions: every history list on the mounted drives and in the store, and imported logs, each
//! once, with the window it was recorded in.

use chrono::Utc;
use rustc_hash::FxHashMap;

use crate::device::Device;
use crate::plays::{self, DriveHistory, HistoryArchive, HistoryList};
use crate::progress::Ctx;
use crate::sets::{EntryOrigin, ImportedSession, Session, SetBook, SetEntry, TimedPlay, Window};
use crate::store::Store;
use crate::{Error, Result};

/// Every session, newest first, and what couldn't be read.
#[derive(Debug, Clone, Default, serde::Serialize, schemars::JsonSchema)]
pub struct Gathered {
    pub sessions: Vec<Session>,
    pub warnings: Vec<String>,
}

/// The sessions on `drives`, archived in `store` and imported into `book`, each once.
pub(crate) fn gather(store: Option<&Store>, drives: &[Device], book: &SetBook, ctx: &Ctx<'_>) -> Result<Gathered> {
    let archive = store.map(|s| plays::archive(s, None, ctx)).transpose()?;
    let drives: Vec<DriveHistory> = drives.iter().map(DriveHistory::read).collect();
    Ok(gather_from(archive.as_ref(), &drives, book))
}

/// The sessions in the history lists read from drives ([`DriveHistory::read`]), in `archive`
/// (the store's lists, read already: of one library, say) and imported into `book`, each once.
pub fn gather_from(archive: Option<&HistoryArchive>, drives: &[DriveHistory], book: &SetBook) -> Gathered {
    let mut out = Gathered::default();
    let mut index: FxHashMap<String, usize> = FxHashMap::default();
    for a in archive.iter().flat_map(|a| &a.lists) {
        let window = Window { after: a.recorded_after, before: Some(a.first_seen.created), exact: false };
        let id = a.list.fingerprint();
        index.insert(id.clone(), out.sessions.len());
        out.sessions.push(of_list(id, &a.list, a.first_seen.drive.clone(), window));
    }
    out.warnings.extend(archive.iter().flat_map(|a| a.warnings.iter().cloned()));
    let archived = out.sessions.len();
    let now = Utc::now();
    for history in drives {
        out.warnings.extend(history.warnings.iter().map(|w| format!("{}: {w}", history.drive)));
        // No snapshot holds a list only on the drive, so it was recorded since the last one.
        let pulled = archive.and_then(|a| a.pulled_before(history));
        for list in &history.lists {
            let id = list.fingerprint();
            let Some(&i) = index.get(&id) else {
                index.insert(id.clone(), out.sessions.len());
                let mut s = of_list(id, list, None, Window { after: pulled, before: Some(now), exact: false });
                s.drives.push(history.drive.clone());
                out.sessions.push(s);
                continue;
            };
            let s = &mut out.sessions[i];
            if !s.drives.contains(&history.drive) {
                s.drives.push(history.drive.clone());
            }
            // On several drives and in no snapshot: any of them might have recorded it.
            if i >= archived {
                s.window.after = s.window.after.zip(pulled).map(|(a, b)| a.min(b));
            }
        }
    }
    out.sessions.extend(book.imported.iter().map(of_log));
    for s in &mut out.sessions {
        s.sets = book.sets_with_session(&s.id).map(|set| set.id.clone()).collect();
        s.dismissed = book.dismissed.contains(&s.id);
    }
    newest_first(&mut out.sessions);
    out
}

/// In the order [`gather_from`] answers sessions: newest first, by when they were recorded.
pub(crate) fn newest_first(sessions: &mut [Session]) {
    // History lists count up (`HISTORY 002` is newer than `HISTORY 001`).
    sessions
        .sort_by(|a, b| (b.window.before, b.window.after, &b.name).cmp(&(a.window.before, a.window.after, &a.name)));
}

pub(crate) fn of_list(id: String, list: &HistoryList, first_seen_on: Option<String>, window: Window) -> Session {
    Session {
        tracks: list.tracks.iter().map(|t| SetEntry::from_track(t, EntryOrigin::Session, Some(&id))).collect(),
        id,
        name: list.name.clone(),
        path: list.path.clone(),
        source: list.database.to_string(),
        drives: Vec::new(),
        first_seen_on,
        window,
        missing: list.missing,
        sets: Vec::new(),
        dismissed: false,
    }
}

fn of_log(log: &ImportedSession) -> Session {
    let first = log.plays.iter().map(|p| p.started).min();
    let since_first = |p: &TimedPlay| first.and_then(|f| u32::try_from((p.started - f).num_seconds()).ok());
    Session {
        id: log.id.clone(),
        name: log.name.clone(),
        path: log.name.clone(),
        source: log.tool.clone(),
        drives: Vec::new(),
        first_seen_on: None,
        window: Window {
            after: first,
            before: log.plays.iter().map(|p| p.stopped.unwrap_or(p.started)).max(),
            exact: true,
        },
        tracks: log.plays.iter().map(|p| SetEntry { start_seconds: since_first(p), ..p.entry.clone() }).collect(),
        missing: 0,
        sets: Vec::new(),
        dismissed: false,
    }
}

/// A session by `query`: an id prefix of 4 or more characters, else a list's path or name when
/// only one session has it.
pub fn find<'a>(sessions: &'a [Session], query: &str) -> Result<&'a Session> {
    let q = query.trim();
    if q.is_empty() {
        return Err(Error::invalid("name a session: its id, or its list's name"));
    }
    let (id, path) = (q.to_ascii_lowercase(), q.trim_matches('/'));
    let matching =
        |pred: &dyn Fn(&Session) -> bool| -> Vec<&'a Session> { sessions.iter().filter(|s| pred(s)).collect() };
    let candidates = [
        matching(&|s| s.id == id),
        if id.len() >= 4 { matching(&|s| s.id.starts_with(&id)) } else { Vec::new() },
        matching(&|s| s.path == path),
        matching(&|s| s.path.eq_ignore_ascii_case(path)),
        matching(&|s| s.name.eq_ignore_ascii_case(path)),
    ];
    for found in candidates {
        match found[..] {
            [] => continue,
            [s] => return Ok(s),
            ref many => {
                let names: Vec<String> = many.iter().map(|s| describe(s)).collect();
                return Err(Error::invalid(format!("{q:?} could be any of: {}", names.join("; "))));
            }
        }
    }
    Err(Error::NotFound(format!("session {q:?} (`locallibrary plays` lists them)")))
}

/// `1a2b3c4d HISTORY 001 (Main, on 3 Oct 2026)`.
fn describe(s: &Session) -> String {
    let place = s.drives.first().or(s.first_seen_on.as_ref()).unwrap_or(&s.source);
    format!("{} {} ({place}, {})", s.short_id(), s.name, s.window.label())
}
