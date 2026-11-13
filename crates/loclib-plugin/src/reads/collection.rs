//! The host's reads of rekordbox's own files on this computer, typed: what a plugin with
//! `collection:files` needs to back them up. [`Host::collection`] calls them.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{Host, RpcError};

impl Host {
    /// The reads of rekordbox's files on this computer, which `collection:files` grants.
    pub fn collection(&self) -> CollectionReads<'_> {
        CollectionReads { host: self }
    }
}

/// rekordbox's files on this computer, read through the host ([`Host::collection`]). The host
/// only ever reads rekordbox's folders: what it writes goes in the request's scratch folder.
#[derive(Clone, Copy)]
pub struct CollectionReads<'h> {
    host: &'h Host,
}

impl CollectionReads<'_> {
    /// `collection.locations`: where rekordbox's files are, as absolute paths.
    pub fn locations(&self) -> Result<CollectionLocations, RpcError> {
        self.host.call("collection.locations", &json!({}))
    }

    /// `collection.status`: whether rekordbox is running, and which version is installed.
    pub fn status(&self) -> Result<CollectionStatus, RpcError> {
        self.host.call("collection.status", &json!({}))
    }

    /// `collection.copy`: a copy of `master.db` as of one moment, still encrypted, written to
    /// `into` (a new file in the request's scratch folder, absolute). The copy leaves out the rows
    /// rekordbox's cloud agent signs in with, unless `with_credentials` (as rekordbox's own
    /// backup keeps them).
    pub fn copy(&self, into: &Path, with_credentials: bool) -> Result<CollectionCopy, RpcError> {
        let params = json!({ "into": into, "with_credentials": with_credentials });
        self.host.call("collection.copy", &params)
    }

    /// `collection.tracks`: every track row of `master.db`, deleted ones too, a page at a time (of
    /// the size the host chooses) until one comes empty or the total is read. `db` is a copy in
    /// the request's scratch folder (one it copied, or a backup's `master.db`) to read instead of
    /// rekordbox's own, which can change between pages while rekordbox runs.
    pub fn tracks(&self, db: Option<&Path>) -> Result<CollectionTracks, RpcError> {
        let mut all = CollectionTracks::default();
        loop {
            let mut params = json!({ "offset": all.tracks.len() });
            if let Some(db) = db {
                params["db"] = json!(db);
            }
            let mut page: CollectionTracks = self.host.call("collection.tracks", &params)?;
            let done = page.tracks.is_empty() || all.tracks.len() + page.tracks.len() >= page.total;
            all.tracks.append(&mut page.tracks);
            all.db_id = page.db_id;
            if done {
                all.total = all.tracks.len();
                return Ok(all);
            }
        }
    }
}

/// `collection.locations`'s answer: where rekordbox keeps its files on this computer, as
/// absolute paths. Each is `None` where rekordbox doesn't run (anywhere but macOS and Windows).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CollectionLocations {
    /// The folder `master.db` is in, with the playlist files and the databases beside it
    /// (`~/Library/Pioneer/rekordbox`).
    pub data: Option<PathBuf>,
    pub master_db: Option<PathBuf>,
    /// Whether `master_db` is there.
    pub master_db_exists: bool,
    /// The folder a track's `analysis_path` and `image_path` are in (`share` in `data`).
    pub share: Option<PathBuf>,
    /// rekordbox's settings folder (`~/Library/Application Support/Pioneer/rekordbox6`, which
    /// rekordbox 7 uses too).
    pub settings: Option<PathBuf>,
}

/// `collection.status`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CollectionStatus {
    /// Whether rekordbox is running now: it may be writing `master.db`.
    pub running: bool,
    /// rekordbox's version: on macOS the running app's, else the newest installed (`7.2.14.0323`);
    /// else the one rekordbox's agent last recorded (`7.2.14`).
    pub version: Option<String>,
}

/// `collection.copy`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CollectionCopy {
    /// The copy's size.
    pub bytes: u64,
    /// `djmdProperty.DBID`, as stored: which library this is.
    pub db_id: Option<String>,
    /// `djmdProperty.DBVersion`.
    pub db_version: Option<String>,
    /// How many sign-in rows the copy left out: every one, unless it was asked `with_credentials`.
    pub credentials_removed: u64,
}

/// `collection.tracks`'s answer: a page of `master.db`'s tracks, in the order rekordbox stored
/// them, and how many it has. [`CollectionReads::tracks`] answers every page as one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CollectionTracks {
    /// `djmdProperty.DBID`, as stored.
    pub db_id: Option<String>,
    /// Track rows in all, deleted ones too.
    pub total: usize,
    pub tracks: Vec<CollectionTrack>,
}

/// A track row of `master.db` (`djmdContent`), as a backup lists it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CollectionTrack {
    /// rekordbox's id, as stored (`"84935471"`).
    pub id: String,
    pub uuid: Option<String>,
    /// The audio file on this computer: `OrgFolderPath` for a track Cloud Library Sync keeps (whose
    /// `FolderPath` is `/contents_<DBID>/...`), else `FolderPath`.
    pub path: String,
    /// Its analysis file in the `share` folder, as stored (`/PIONEER/USBANLZ/...`).
    pub analysis_path: Option<String>,
    /// Its artwork in the `share` folder, as stored (`/PIONEER/Artwork/...`).
    pub image_path: Option<String>,
    /// rekordbox marked it deleted (`rb_local_deleted`): its files aren't backed up.
    pub deleted: bool,
    /// Its artist's name as rekordbox stores it (`djmdArtist`), which rekordbox's backup names
    /// its music folders after; `None` without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    /// Its album's name as rekordbox stores it (`djmdAlbum`); `None` without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
}
