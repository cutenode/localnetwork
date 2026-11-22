use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::{
    FT_BOOL, FT_CHAR_SIZE, FT_DOUBLE, FT_END_TAG, FT_FLOAT, FT_LONG_LONG, FT_NAME, FT_SIZE_T, FT_START_TAG, FT_STRING,
};

/// Binary XML written token by token, in any of the character sizes Audacity uses: each name
/// goes into the dictionary the first time it's used.
pub struct Xml {
    pub(crate) char_size: u8,
    pub(crate) dict: Vec<u8>,
    pub(crate) doc: Vec<u8>,
    pub(crate) names: Vec<String>,
}

impl Xml {
    /// An empty document whose strings take `char_size` bytes a character (1, 2 or 4).
    pub fn new(char_size: u8) -> Xml {
        Xml { char_size, dict: vec![FT_CHAR_SIZE, char_size], doc: Vec::new(), names: Vec::new() }
    }

    fn text(&self, s: &str) -> Vec<u8> {
        match self.char_size {
            4 => s.chars().flat_map(|c| (c as u32).to_le_bytes()).collect(),
            2 => s.encode_utf16().flat_map(u16::to_le_bytes).collect(),
            _ => s.as_bytes().to_vec(),
        }
    }

    fn id(&mut self, name: &str) -> [u8; 2] {
        let id = match self.names.iter().position(|n| n == name) {
            Some(i) => i as u16,
            None => {
                let id = self.names.len() as u16;
                let text = self.text(name);
                self.dict.push(FT_NAME);
                self.dict.extend(id.to_le_bytes());
                self.dict.extend((text.len() as u16).to_le_bytes());
                self.dict.extend(text);
                self.names.push(name.into());
                id
            }
        };
        id.to_le_bytes()
    }

    /// A token naming `name`, then `value`'s bytes.
    fn token(&mut self, op: u8, name: &str, value: &[u8]) -> &mut Xml {
        let id = self.id(name);
        self.doc.push(op);
        self.doc.extend(id);
        self.doc.extend(value);
        self
    }

    /// Opens element `name`.
    pub fn start(&mut self, name: &str) -> &mut Xml {
        self.token(FT_START_TAG, name, &[])
    }

    /// Closes element `name`.
    pub fn end(&mut self, name: &str) -> &mut Xml {
        self.token(FT_END_TAG, name, &[])
    }

    /// Attribute `name` as the string `v`.
    pub fn string(&mut self, name: &str, v: &str) -> &mut Xml {
        let text = self.text(v);
        self.token(FT_STRING, name, &[&(text.len() as i32).to_le_bytes()[..], &text].concat())
    }

    /// `name` as a whole number, in whichever of the integer tokens `op` is.
    pub(crate) fn int(&mut self, op: u8, name: &str, v: i64) -> &mut Xml {
        let bytes = match op {
            FT_BOOL => vec![v as u8],
            FT_LONG_LONG => v.to_le_bytes().to_vec(),
            FT_SIZE_T => (v as u32).to_le_bytes().to_vec(),
            _ => (v as i32).to_le_bytes().to_vec(),
        };
        self.token(op, name, &bytes)
    }

    /// Attribute `name` as a 32-bit float.
    pub fn float(&mut self, name: &str, v: f32) -> &mut Xml {
        self.token(FT_FLOAT, name, &[&v.to_le_bytes()[..], &7i32.to_le_bytes()].concat())
    }

    /// Attribute `name` as a 64-bit float.
    pub fn double(&mut self, name: &str, v: f64) -> &mut Xml {
        self.token(FT_DOUBLE, name, &[&v.to_le_bytes()[..], &15i32.to_le_bytes()].concat())
    }

    /// A clip at `offset` seconds of `samples` samples in `blocks`, each `(first sample, block
    /// id)`. It leaves the `waveclip` element open, for attributes after it.
    pub fn clip(&mut self, offset: f64, samples: i64, blocks: &[(i64, i64)]) -> &mut Xml {
        self.start("waveclip").double("offset", offset).start("sequence");
        self.int(FT_LONG_LONG, "numsamples", samples);
        for &(start, id) in blocks {
            self.start("waveblock").int(FT_LONG_LONG, "start", start).int(FT_LONG_LONG, "blockid", id);
            self.end("waveblock");
        }
        self.end("sequence")
    }
}

/// A project file in `dir`, in Audacity's tables, holding `xml` (in `table`, `project` or
/// `autosave`) and the sample blocks, each `(id, format, samples)`. Files are numbered by how
/// many `dir` already holds.
///
/// # Panics
///
/// When `dir` can't be listed or the file can't be written.
pub fn project(dir: &Path, table: &str, xml: Option<&Xml>, blocks: &[(i64, i64, Vec<u8>)]) -> PathBuf {
    let path = dir.join(format!("{}.aup3", std::fs::read_dir(dir).unwrap().count()));
    let db = scratch_db(&path);
    db.execute_batch(
        "CREATE TABLE project(id INTEGER PRIMARY KEY, dict BLOB, doc BLOB);
             CREATE TABLE autosave(id INTEGER PRIMARY KEY, dict BLOB, doc BLOB);
             CREATE TABLE sampleblocks(blockid INTEGER PRIMARY KEY AUTOINCREMENT, sampleformat INTEGER,
               summin REAL, summax REAL, sumrms REAL, summary256 BLOB, summary64k BLOB, samples BLOB);",
    )
    .unwrap();
    if let Some(x) = xml {
        db.execute(&format!("INSERT INTO {table} VALUES (1, ?1, ?2)"), (&x.dict, &x.doc)).unwrap();
    }
    for (id, format, samples) in blocks {
        db.execute(
            "INSERT INTO sampleblocks (blockid, sampleformat, samples) VALUES (?1, ?2, ?3)",
            (id, format, samples),
        )
        .unwrap();
    }
    path
}

/// Samples as Audacity stores 32-bit float ones (sample format `0x0004_000F`).
pub fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// A SQLite database at `path` that writes fast: a test's file needn't survive a crash.
pub(crate) fn scratch_db(path: &Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch("PRAGMA journal_mode = MEMORY; PRAGMA synchronous = OFF;").unwrap();
    conn
}
