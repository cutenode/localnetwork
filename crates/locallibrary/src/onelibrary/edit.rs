//! Editing OneLibrary's playlists, histories and track details in place (see [`crate::edit`]).

use rusqlite::{OptionalExtension, params};

use super::{CueRow, Lookup, OneLibrary, id32, int};
use crate::edit::{AudioFile, TrackFields};
use crate::{Error, Result};

/// The playlist-shaped trees edits can change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tree {
    Playlist,
    History,
}

impl Tree {
    fn table(self) -> &'static str {
        match self {
            Tree::Playlist => "playlist",
            Tree::History => "history",
        }
    }
}

impl OneLibrary {
    pub fn rename_node(&mut self, tree: Tree, id: u32, name: &str) -> Result<()> {
        let t = tree.table();
        let n = self.conn.execute(&format!("UPDATE {t} SET name = ?1 WHERE {t}_id = ?2"), params![name, id])?;
        if n != 1 {
            return Err(Error::NotFound(format!("{t} #{id} in exportLibrary.db")));
        }
        Ok(())
    }

    /// Put a node under `parent` (0 for the top level) at sequence number `sequence`.
    pub fn place_node(&mut self, tree: Tree, id: u32, parent: u32, sequence: u32) -> Result<()> {
        let t = tree.table();
        let n = self.conn.execute(
            &format!("UPDATE {t} SET {t}_id_parent = ?1, sequenceNo = ?2 WHERE {t}_id = ?3"),
            params![parent, sequence, id],
        )?;
        if n != 1 {
            return Err(Error::NotFound(format!("{t} #{id} in exportLibrary.db")));
        }
        Ok(())
    }

    /// Remove nodes with their entries (children of a removed folder must be passed too).
    pub fn remove_nodes(&mut self, tree: Tree, ids: &[u32]) -> Result<()> {
        let t = tree.table();
        let tx = self.conn.transaction()?;
        for id in ids {
            tx.execute(&format!("DELETE FROM {t}_content WHERE {t}_id = ?1"), [id])?;
            if tx.execute(&format!("DELETE FROM {t} WHERE {t}_id = ?1"), [id])? != 1 {
                return Err(Error::NotFound(format!("{t} #{id} in exportLibrary.db")));
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Replace a playlist's contents with `tracks`, in order, numbered from 1.
    pub fn set_playlist_entries(&mut self, playlist: u32, tracks: &[u32]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM playlist_content WHERE playlist_id = ?1", [playlist])?;
        {
            let mut insert =
                tx.prepare("INSERT INTO playlist_content (playlist_id, content_id, sequenceNo) VALUES (?1, ?2, ?3)")?;
            for (n, t) in tracks.iter().enumerate() {
                insert.execute(params![playlist, t, n as i64 + 1])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Point playlist and history entries for track `from` at `to`, and give `to` the My Tags
    /// `from` has. Returns how many entries changed.
    pub fn repoint_track(&mut self, from: u32, to: u32) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut n = 0;
        for t in ["playlist_content", "history_content"] {
            n += tx.execute(&format!("UPDATE {t} SET content_id = ?1 WHERE content_id = ?2"), params![to, from])?;
        }
        tx.execute(
            "INSERT INTO myTag_content (myTag_id, content_id)
             SELECT myTag_id, ?1 FROM myTag_content WHERE content_id = ?2
               AND myTag_id NOT IN (SELECT myTag_id FROM myTag_content WHERE content_id = ?1)",
            params![to, from],
        )?;
        tx.commit()?;
        Ok(n)
    }

    /// Point track `id` at the audio file at `path` (a device path), as when its file was moved;
    /// with `file`, another encoding (its format, size, rate, depth and bit rate), whose cues are
    /// `shift_ms` later.
    pub fn set_track_path(&mut self, id: u32, path: &str, file: Option<&AudioFile>, shift_ms: i32) -> Result<()> {
        let name = path.rsplit('/').next().unwrap_or(path);
        let n = self
            .conn
            .execute("UPDATE content SET path = ?1, fileName = ?2 WHERE content_id = ?3", params![path, name, id])?;
        if n != 1 {
            return Err(Error::NotFound(format!("track #{id} in exportLibrary.db")));
        }
        self.conn
            .execute("UPDATE content SET fileType = ?1 WHERE content_id = ?2", params![super::file_type(path), id])?;
        if let Some(f) = file {
            self.conn.execute(
                "UPDATE content SET fileSize = ?1, samplingRate = ?2, bitDepth = ?3, bitrate = ?4 WHERE content_id = ?5",
                params![f.size as i64, f.sample_rate, f.bit_depth, f.bitrate, id],
            )?;
            self.clear_cue_seeks(id)?;
        }
        if shift_ms != 0 {
            let shift = |usec: i64| (usec + i64::from(shift_ms) * 1000).max(0);
            let rows: Vec<CueRow> = self
                .cue_rows(id)?
                .into_iter()
                .map(|r| CueRow { in_usec: shift(r.in_usec), out_usec: r.out_usec.map(shift), ..r })
                .collect();
            self.replace_cues(id, &rows)?;
        }
        Ok(())
    }

    /// Change tracks' details. Artists, albums, genres, labels and keys are found or created by
    /// name (an empty name clears the field); rows the tracks used before and nothing uses any
    /// more are removed.
    pub fn set_track_fields(&mut self, ids: &[u32], f: &TrackFields) -> Result<()> {
        let color = f.color_value()?;
        let mut candidates: Vec<(Lookup, i64)> = Vec::new();
        for &id in ids {
            let old: Option<[i64; 9]> = self
                .conn
                .query_row(
                    "SELECT artist_id_artist, artist_id_remixer, artist_id_originalArtist, artist_id_composer,
                            album_id, genre_id, label_id, key_id, coalesce((SELECT artist_id FROM album al
                                WHERE al.album_id = content.album_id), 0)
                     FROM content WHERE content_id = ?1",
                    [id],
                    |r| {
                        let mut out = [0i64; 9];
                        for (i, v) in out.iter_mut().enumerate() {
                            *v = int(r.get_ref(i)?);
                        }
                        Ok(out)
                    },
                )
                .optional()?;
            let Some(old) = old else {
                return Err(Error::NotFound(format!("track #{id} in exportLibrary.db")));
            };
            let set = |column: &str, value: rusqlite::types::Value| -> Result<()> {
                self.conn
                    .execute(&format!("UPDATE content SET {column} = ?1 WHERE content_id = ?2"), params![value, id])?;
                Ok(())
            };
            let text = |s: &str| rusqlite::types::Value::Text(s.to_string());
            let opt = |v: Option<i64>| v.map_or(rusqlite::types::Value::Null, rusqlite::types::Value::Integer);
            if let Some(v) = &f.title {
                set("title", text(v))?;
                set("titleForSearch", rusqlite::types::Value::Null)?;
            }
            if let Some(v) = &f.comment {
                set("djComment", text(v))?;
            }
            if let Some(v) = &f.mix_name {
                set("subtitle", text(v))?;
            }
            let artist = |name: &str| -> Result<Option<i64>> {
                if name.trim().is_empty() { Ok(None) } else { self.named(Lookup::Artist, name).map(Some) }
            };
            for (field, column, old_id) in [
                (&f.artist, "artist_id_artist", old[0]),
                (&f.remixer, "artist_id_remixer", old[1]),
                (&f.original_artist, "artist_id_originalArtist", old[2]),
                (&f.composer, "artist_id_composer", old[3]),
            ] {
                if let Some(name) = field {
                    set(column, opt(artist(name)?))?;
                    candidates.push((Lookup::Artist, old_id));
                }
            }
            if let Some(name) = &f.album {
                let album_artist = (old[8] > 0).then_some(old[8]);
                let album = if name.trim().is_empty() { None } else { Some(self.album(name, album_artist)?) };
                set("album_id", opt(album))?;
                candidates.push((Lookup::Album, old[4]));
                candidates.push((Lookup::Artist, old[8]));
            }
            for (field, table, column, old_id) in [
                (&f.genre, Lookup::Genre, "genre_id", old[5]),
                (&f.label, Lookup::Label, "label_id", old[6]),
                (&f.key, Lookup::Key, "key_id", old[7]),
            ] {
                if let Some(name) = field {
                    let new = if name.trim().is_empty() { None } else { Some(self.named(table, name)?) };
                    // rekordbox writes 0, not NULL, for "no key".
                    let value =
                        if table == Lookup::Key { rusqlite::types::Value::Integer(new.unwrap_or(0)) } else { opt(new) };
                    set(column, value)?;
                    candidates.push((table, old_id));
                }
            }
            if let Some(v) = f.rating {
                set("rating", rusqlite::types::Value::Integer(v.into()))?;
            }
            if let Some(c) = color {
                set("color_id", rusqlite::types::Value::Integer(c.map_or(0, |c| c.id().into())))?;
            }
            if let Some(v) = f.year {
                set("releaseYear", rusqlite::types::Value::Integer(v.into()))?;
            }
            if let Some(v) = f.play_count {
                set("djPlayCount", rusqlite::types::Value::Integer(v.into()))?;
            }
            if let Some(v) = f.track_number {
                set("trackNo", rusqlite::types::Value::Integer(v.into()))?;
            }
            if let Some(v) = f.disc_number {
                set("discNo", rusqlite::types::Value::Integer(v.into()))?;
            }
        }
        self.prune_lookups(&candidates)
    }

    /// Remove the candidate lookup rows nothing references any more. Albums go first, so an
    /// album's artist (a candidate with it) that only that album used goes too.
    fn prune_lookups(&mut self, candidates: &[(Lookup, i64)]) -> Result<()> {
        let mut candidates: Vec<(Lookup, i64)> = candidates.iter().copied().filter(|(_, id)| *id > 0).collect();
        candidates.sort_by_key(|(t, id)| (*t != Lookup::Album, *id));
        candidates.dedup();
        for (table, id) in candidates {
            let used_sql = match table {
                Lookup::Artist => {
                    "SELECT 1 FROM content WHERE ?1 IN (artist_id_artist, artist_id_remixer, artist_id_originalArtist,
                         artist_id_composer, artist_id_lyricist) UNION SELECT 1 FROM album WHERE artist_id = ?1"
                }
                Lookup::Album => "SELECT 1 FROM content WHERE album_id = ?1",
                Lookup::Genre => "SELECT 1 FROM content WHERE genre_id = ?1",
                Lookup::Label => "SELECT 1 FROM content WHERE label_id = ?1",
                Lookup::Key => "SELECT 1 FROM content WHERE key_id = ?1",
                Lookup::Image => {
                    "SELECT 1 FROM content WHERE image_id = ?1 UNION SELECT 1 FROM album WHERE image_id = ?1
                     UNION SELECT 1 FROM playlist WHERE image_id = ?1 UNION SELECT 1 FROM hotCueBankList WHERE image_id = ?1"
                }
            };
            let used: Option<i64> = self.conn.query_row(used_sql, [id], |r| r.get(0)).optional()?;
            if used.is_some() {
                continue;
            }
            self.conn.execute(&format!("DELETE FROM {} WHERE {} = ?1", table.table(), table.id()), [id])?;
        }
        Ok(())
    }

    /// The id of a `playlist` or `history` node by parent and name (for tests and checks).
    pub fn node_id(&self, tree: Tree, parent: u32, name: &str) -> Result<Option<u32>> {
        let t = tree.table();
        let id: Option<i64> = self
            .conn
            .query_row(
                &format!("SELECT {t}_id FROM {t} WHERE coalesce({t}_id_parent, 0) = ?1 AND name = ?2"),
                params![parent, name],
                |r| r.get(0),
            )
            .optional()?;
        id.map(|id| id32(id, t)).transpose()
    }
}

#[cfg(test)]
mod tests;
