//! My Tags in OneLibrary: `myTag`, shaped like `playlist` (categories have attribute 1 and parent
//! 0, tags are the children of a category), and `myTag_content` (tag, track) links. See
//! [`crate::tags`] for how they're matched with `exportExt.pdb`'s.

use rusqlite::{OptionalExtension, params};

use super::{OneLibrary, int, text};
use crate::error::Reason;
use crate::{Error, Result};

/// A `myTag` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MyTagRow {
    pub id: i64,
    /// The category a tag belongs to; 0 for a category.
    pub parent: i64,
    pub name: String,
    pub is_category: bool,
    pub sequence: i64,
}

impl OneLibrary {
    /// Every `myTag` row, in display order within each parent (`sequenceNo`, then id).
    pub fn my_tag_rows(&self) -> Result<Vec<MyTagRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT myTag_id, myTag_id_parent, name, attribute, sequenceNo FROM myTag
             ORDER BY myTag_id_parent, sequenceNo, myTag_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(MyTagRow {
                id: int(r.get_ref(0)?),
                parent: int(r.get_ref(1)?),
                name: text(r.get_ref(2)?),
                is_category: int(r.get_ref(3)?) == 1,
                sequence: int(r.get_ref(4)?),
            })
        })?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }

    /// Every `myTag_content` row as (tag id, track id).
    pub fn my_tag_links(&self) -> Result<Vec<(i64, i64)>> {
        let mut stmt = self.conn.prepare("SELECT myTag_id, content_id FROM myTag_content ORDER BY rowid")?;
        let rows = stmt.query_map([], |r| Ok((int(r.get_ref(0)?), int(r.get_ref(1)?))))?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }

    fn my_tag(&self, id: i64) -> Result<MyTagRow> {
        self.conn
            .query_row(
                "SELECT myTag_id_parent, name, attribute, sequenceNo FROM myTag WHERE myTag_id = ?1",
                [id],
                |r| {
                    Ok(MyTagRow {
                        id,
                        parent: int(r.get_ref(0)?),
                        name: text(r.get_ref(1)?),
                        is_category: int(r.get_ref(2)?) == 1,
                        sequence: int(r.get_ref(3)?),
                    })
                },
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("My Tag #{id} in exportLibrary.db")))
    }

    /// Add a category (`parent` 0) or a tag after its siblings, with the given id. A first
    /// sibling is numbered like the rest of its kind (from 0 if there are none).
    pub fn add_my_tag(&mut self, id: i64, parent: i64, name: &str, is_category: bool) -> Result<()> {
        let taken: bool =
            self.conn.query_row("SELECT count(*) > 0 FROM myTag WHERE myTag_id = ?1", [id], |r| r.get(0))?;
        if taken {
            return Err(Error::conflict(Reason::Exists, format!("My Tag id {id} is already used in exportLibrary.db")));
        }
        if !is_category {
            let parent = self.my_tag(parent)?;
            if !parent.is_category {
                return Err(Error::invalid(format!("My Tag {:?} isn't a category", parent.name)));
            }
        }
        let sequence: i64 = self.conn.query_row(
            "SELECT coalesce(
                 (SELECT max(sequenceNo) + 1 FROM myTag WHERE myTag_id_parent = ?1),
                 (SELECT min(sequenceNo) FROM myTag WHERE attribute = ?2),
                 0)",
            params![parent, i64::from(is_category)],
            |r| r.get(0),
        )?;
        self.conn.execute(
            "INSERT INTO myTag (myTag_id, sequenceNo, name, attribute, myTag_id_parent) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, sequence, name, i64::from(is_category), parent],
        )?;
        Ok(())
    }

    pub fn rename_my_tag(&mut self, id: i64, name: &str) -> Result<()> {
        if self.conn.execute("UPDATE myTag SET name = ?1 WHERE myTag_id = ?2", params![name, id])? != 1 {
            return Err(Error::NotFound(format!("My Tag #{id} in exportLibrary.db")));
        }
        Ok(())
    }

    /// Remove a tag with its track links, or a category with its tags and their links. Later
    /// siblings move up, so `sequenceNo` stays without gaps.
    pub fn remove_my_tag(&mut self, id: i64) -> Result<()> {
        let row = self.my_tag(id)?;
        let tx = self.conn.transaction()?;
        if row.is_category {
            tx.execute(
                "DELETE FROM myTag_content WHERE myTag_id IN (SELECT myTag_id FROM myTag WHERE myTag_id_parent = ?1)",
                [id],
            )?;
            tx.execute("DELETE FROM myTag WHERE myTag_id_parent = ?1", [id])?;
        }
        tx.execute("DELETE FROM myTag_content WHERE myTag_id = ?1", [id])?;
        tx.execute("DELETE FROM myTag WHERE myTag_id = ?1", [id])?;
        tx.execute(
            "UPDATE myTag SET sequenceNo = sequenceNo - 1 WHERE myTag_id_parent = ?1 AND sequenceNo > ?2",
            params![row.parent, row.sequence],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Give tracks a tag; returns how many didn't have it yet.
    pub fn link_my_tag(&mut self, tag: i64, tracks: &[u32]) -> Result<usize> {
        if self.my_tag(tag)?.is_category {
            return Err(Error::invalid(format!("My Tag #{tag} is a category, not a tag")));
        }
        let tx = self.conn.transaction()?;
        let mut added = 0;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO myTag_content (myTag_id, content_id) SELECT ?1, ?2
                 WHERE NOT EXISTS (SELECT 1 FROM myTag_content WHERE myTag_id = ?1 AND content_id = ?2)",
            )?;
            for t in tracks {
                added += stmt.execute(params![tag, t])?;
            }
        }
        tx.commit()?;
        Ok(added)
    }

    /// Take every tag off tracks; returns how many links went.
    pub fn unlink_my_tags_of(&mut self, tracks: &[u32]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut removed = 0;
        {
            let mut stmt = tx.prepare("DELETE FROM myTag_content WHERE content_id = ?1")?;
            for t in tracks {
                removed += stmt.execute([t])?;
            }
        }
        tx.commit()?;
        Ok(removed)
    }

    /// Take a tag off tracks; returns how many had it.
    pub fn unlink_my_tag(&mut self, tag: i64, tracks: &[u32]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut removed = 0;
        {
            let mut stmt = tx.prepare("DELETE FROM myTag_content WHERE myTag_id = ?1 AND content_id = ?2")?;
            for t in tracks {
                removed += usize::from(stmt.execute(params![tag, t])? > 0);
            }
        }
        tx.commit()?;
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(one: &OneLibrary, parent: i64) -> Vec<(String, i64)> {
        one.my_tag_rows().unwrap().into_iter().filter(|r| r.parent == parent).map(|r| (r.name, r.sequence)).collect()
    }

    #[test]
    fn categories_tags_and_links() {
        let mut one = OneLibrary::create().unwrap();
        one.add_my_tag(1, 0, "Genre", true).unwrap();
        one.add_my_tag(2, 0, "Situation", true).unwrap();
        one.add_my_tag(100, 1, "House", false).unwrap();
        one.add_my_tag(101, 1, "Techno", false).unwrap();
        one.add_my_tag(102, 1, "Disco", false).unwrap();
        one.add_my_tag(200, 2, "Opening", false).unwrap();
        assert_eq!(names(&one, 0), [("Genre".into(), 0), ("Situation".into(), 1)]);
        assert!(one.add_my_tag(101, 2, "Again", false).is_err(), "ids are unique");
        assert!(one.add_my_tag(300, 100, "Under a tag", false).is_err());
        assert!(one.link_my_tag(1, &[5]).is_err(), "categories aren't tags");

        assert_eq!(one.link_my_tag(101, &[5, 6]).unwrap(), 2);
        assert_eq!(one.link_my_tag(101, &[6, 7]).unwrap(), 1);
        one.link_my_tag(200, &[5]).unwrap();
        assert_eq!(one.unlink_my_tag(101, &[7, 8]).unwrap(), 1);
        assert_eq!(one.my_tag_links().unwrap(), [(101, 5), (101, 6), (200, 5)]);

        one.remove_my_tag(100).unwrap();
        assert_eq!(names(&one, 1), [("Techno".into(), 0), ("Disco".into(), 1)]);
        one.rename_my_tag(102, "Nu Disco").unwrap();
        assert_eq!(names(&one, 1)[1].0, "Nu Disco");
        one.remove_my_tag(1).unwrap();
        assert_eq!(names(&one, 0), [("Situation".into(), 0)]);
        assert!(names(&one, 1).is_empty());
        assert_eq!(one.my_tag_links().unwrap(), [(200, 5)]);
        // A new first tag is numbered like the others.
        one.add_my_tag(3, 0, "Genre", true).unwrap();
        one.add_my_tag(103, 3, "Electro", false).unwrap();
        assert_eq!(names(&one, 3), [("Electro".into(), 0)]);
    }
}
