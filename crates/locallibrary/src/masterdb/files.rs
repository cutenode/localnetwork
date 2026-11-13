//! rekordbox's files on this computer, as a backup of them needs them (a plugin's
//! `collection:files` reads, see [`crate::plugin`]): where they are, whether rekordbox is running
//! and which version it is, a copy of `master.db` made as rekordbox's own backup makes one, and
//! what its tracks' files are. As everywhere in [`super`], `master.db` and its WAL are copied
//! before anything reads them, and nothing is written to rekordbox's folders.

use std::ffi::OsStr;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use loclib_plugin::{CollectionCopy, CollectionLocations, CollectionStatus, CollectionTrack, CollectionTracks};
use rusqlite::types::Value;
use rustc_hash::FxHashMap;

use super::{
    Db, agent_option, agent_options_path, default_path, in_cloud, int, open_copy, settings_path, share_dir, text,
};
use crate::error::IoContext;
use crate::onelibrary::cipher::{self, Codec};
use crate::{Error, Result};

/// Where rekordbox's files are: `master.db` where [`default_path`] finds it ([`super::PATH_ENV`]
/// too), the folder it's in and the `share` folder there, and rekordbox's settings folder.
pub fn locations() -> CollectionLocations {
    let master_db = default_path();
    CollectionLocations {
        data: master_db.as_deref().and_then(Path::parent).map(Path::to_path_buf),
        master_db_exists: master_db.as_deref().is_some_and(Path::is_file),
        share: master_db.as_deref().map(share_dir),
        settings: settings_path().and_then(|p| p.parent().map(Path::to_path_buf)),
        master_db,
    }
}

/// Whether rekordbox is running now, and its version: its app's, else the one its agent last
/// recorded in its `options.json`.
pub fn status() -> CollectionStatus {
    let app = running();
    let (program, options) = (app.clone().flatten(), agent_options_path());
    let version = version(program.as_deref(), Path::new("/Applications"), options.as_deref());
    CollectionStatus { running: app.is_some(), version }
}

/// rekordbox's process, when it's running, with where its program is when that can be read.
fn running() -> Option<Option<PathBuf>> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut system = System::new();
    let exe = ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet);
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, exe);
    let found = system.processes().values().find(|p| is_rekordbox(p.name()))?;
    Some(found.exe().map(Path::to_path_buf))
}

/// Whether a process named `name` is rekordbox itself (`rekordbox`, `rekordbox.exe`), not its
/// agent (`rekordboxAgent`) or a helper.
fn is_rekordbox(name: &OsStr) -> bool {
    let name = name.to_string_lossy().to_lowercase();
    name == "rekordbox" || name == "rekordbox.exe"
}

/// rekordbox's version: its app's (see [`app_version`]), else the one its agent last recorded in
/// its `options.json` at `options` (`app_ver`, `7.2.14`), as on Windows, whose apps keep theirs in
/// their programs.
fn version(program: Option<&Path>, applications: &Path, options: Option<&Path>) -> Option<String> {
    app_version(program, applications).or_else(|| agent_option(options?, "app_ver"))
}

/// The version of the app whose program is `program` (its `CFBundleShortVersionString`,
/// `7.2.14.0323`), else of the newest rekordbox app in `applications` (`rekordbox 7/`).
#[cfg(target_os = "macos")]
fn app_version(program: Option<&Path>, applications: &Path) -> Option<String> {
    // The program is `rekordbox.app/Contents/MacOS/rekordbox`.
    let contents = program.and_then(|p| p.ancestors().nth(2));
    contents.and_then(|c| bundle_version(&c.join("Info.plist"))).or_else(|| {
        let folders = std::fs::read_dir(applications).ok()?.flatten();
        let rekordbox = folders.filter(|e| e.file_name().to_string_lossy().to_lowercase().starts_with("rekordbox"));
        let versions = rekordbox.filter_map(|e| bundle_version(&e.path().join("rekordbox.app/Contents/Info.plist")));
        versions.max_by_key(|v| v.split('.').map(|n| n.trim().parse::<u64>().unwrap_or(0)).collect::<Vec<_>>())
    })
}

#[cfg(not(target_os = "macos"))]
fn app_version(_: Option<&Path>, _: &Path) -> Option<String> {
    None
}

/// The version an app's `Info.plist` says it is.
#[cfg(target_os = "macos")]
fn bundle_version(info: &Path) -> Option<String> {
    let info = plist::Value::from_file(info).ok()?;
    info.as_dictionary()?.get("CFBundleShortVersionString")?.as_string().map(str::to_string)
}

/// The `agentRegistry` row rekordbox's cloud agent signs in with, as an SQL condition: its
/// `agentCredentials` holds an id, a token and a secret. Of the 30 rows a rekordbox 7.2.14
/// library's registry had, it was the only one holding them: the others hold paths, counts,
/// times, a flag, notification state and service logs.
const CREDENTIALS: &str = "registry_id = 'agentCredentials'";

/// Writes a copy of the `master.db` at `path` to `into` (a new file) as rekordbox's own backup
/// writes one: what the database and its WAL hold as of one moment, every
/// table rebuilt into a new file by SQLite's `VACUUM INTO` (as SQLCipher's `sqlcipher_export`
/// rebuilds one), so no free page keeps a row left out, then encrypted under `key` with the
/// original's salt, which SQLCipher keeps too when it exports under the same key. A plain database
/// stays plain. The cloud agent's sign-in row (`agentCredentials`) is left out first, unless
/// `with_credentials` (rekordbox's own backup keeps it). `into` is removed again when this fails.
pub fn export(path: &Path, key: &str, with_credentials: bool, into: &Path) -> Result<CollectionCopy> {
    let dir = tempfile::tempdir().map_err(|e| Error::Io { path: std::env::temp_dir(), source: e })?;
    let (db, salt) = open_copy(path, key, dir.path(), true)?;
    let credentials_removed = if with_credentials { 0 } else { db.remove_credentials()? };
    let (db_id, db_version) = (db.property("DBID")?, db.property("DBVersion")?);
    let rebuilt = dir.path().join("export.db");
    let to = rebuilt.to_str().ok_or_else(|| Error::invalid("the temporary folder's path isn't UTF-8"))?;
    db.conn.execute("VACUUM INTO ?1", [to]).map_err(|e| db.sql(e))?;
    drop(db);
    let mut out = File::create_new(into).at(into)?;
    let codec = salt.map(|salt| Codec::with_key(key.as_bytes(), salt));
    let written = write_pages(&rebuilt, codec.as_ref(), &mut out, into);
    if written.is_err() {
        drop(out);
        let _ = std::fs::remove_file(into);
    }
    Ok(CollectionCopy { bytes: written?, db_id, db_version, credentials_removed })
}

/// Copies the database at `plain` to `out` (the file `into`), each page encrypted with `codec`
/// when there is one, answering how many bytes it wrote.
fn write_pages(plain: &Path, codec: Option<&Codec>, out: &mut File, into: &Path) -> Result<u64> {
    let mut from = File::open(plain).at(plain)?;
    let Some(codec) = codec else { return std::io::copy(&mut from, out).at(into) };
    let len = from.metadata().at(plain)?.len();
    let size = cipher::PAGE_SIZE as u64;
    if !len.is_multiple_of(size) {
        return Err(Error::parse(plain, format!("{len} bytes is not a whole number of {size}-byte pages")));
    }
    let mut page = vec![0u8; cipher::PAGE_SIZE];
    for pgno in 1..=(len / size) as u32 {
        from.read_exact(&mut page).at(plain)?;
        if pgno == 1 {
            // Its reserved bytes hold each page's IV and HMAC: VACUUM INTO keeps them.
            cipher::check_plain_header(&page).map_err(|e| Error::parse(plain, e))?;
        }
        out.write_all(&codec.encrypt_page(pgno, &page)?).at(into)?;
    }
    out.flush().at(into)?;
    Ok(len)
}

impl Db<'_> {
    /// Removes the cloud agent's sign-in rows from `agentRegistry` ([`CREDENTIALS`]), answering
    /// how many there were. A database without the table has none.
    fn remove_credentials(&self) -> Result<u64> {
        let columns = self.columns("agentRegistry")?;
        if columns.is_empty() {
            return Ok(0);
        }
        if !columns.contains("registry_id") {
            let why = "its agentRegistry has no registry_id, which tells the sign-in rows apart";
            return Err(Error::parse(self.path, why));
        }
        let removed = self.conn.execute(&format!("DELETE FROM agentRegistry WHERE {CREDENTIALS}"), []);
        Ok(removed.map_err(|e| self.sql(e))? as u64)
    }
}

/// The track rows (`djmdContent`) of the `master.db` at `path`, deleted ones too: `limit` of them
/// from `offset` on, in the order rekordbox stored them, and how many there are. Each has its
/// artist's and album's names as `djmdArtist` and `djmdAlbum` hold them, which rekordbox's own
/// backup names its music folders after.
pub fn tracks(path: &Path, key: &str, offset: u64, limit: u64) -> Result<CollectionTracks> {
    let dir = tempfile::tempdir().map_err(|e| Error::Io { path: std::env::temp_dir(), source: e })?;
    let (db, _) = open_copy(path, key, dir.path(), false)?;
    let total: i64 = db.conn.query_row("SELECT count(*) FROM djmdContent", [], |r| r.get(0)).map_err(|e| db.sql(e))?;
    let wanted = [
        "ID",
        "UUID",
        "FolderPath",
        "OrgFolderPath",
        "AnalysisDataPath",
        "ImagePath",
        "rb_local_deleted",
        "ArtistID",
        "AlbumID",
    ];
    // SQLite's numbers are signed.
    let (limit, offset) = (limit.min(i64::MAX as u64), offset.min(i64::MAX as u64));
    let rows = db.rows("djmdContent", &wanted, true, &format!(" LIMIT {limit} OFFSET {offset}"))?;
    let (artists, albums) = (db.names("djmdArtist", "Name")?, db.names("djmdAlbum", "Name")?);
    let given = |v: &Value| Some(text(v)).filter(|t| !t.is_empty());
    let named = |names: &FxHashMap<String, String>, id: &Value| names.get(&text(id)).filter(|n| !n.is_empty()).cloned();
    let track = |r: &Vec<Value>| CollectionTrack {
        id: text(&r[0]),
        uuid: given(&r[1]),
        path: audio(text(&r[2]), given(&r[3])),
        analysis_path: given(&r[4]),
        image_path: given(&r[5]),
        deleted: int(&r[6]) != 0,
        artist: named(&artists, &r[7]),
        album: named(&albums, &r[8]),
    };
    let tracks = rows.iter().map(track).collect();
    Ok(CollectionTracks { db_id: db.property("DBID")?, total: total as usize, tracks })
}

/// A track's audio file on this computer: `org` (`OrgFolderPath`) for one Cloud Library Sync
/// keeps, whose `folder` (`FolderPath`) is its path in the service's folder
/// (`/contents_<DBID>/...`), else `folder`.
fn audio(folder: String, org: Option<String>) -> String {
    match org {
        Some(org) if in_cloud(&folder) => org,
        _ => folder,
    }
}

#[cfg(test)]
pub(crate) mod tests;
