//! `export.pdb` and `exportExt.pdb` (one page layout): the vendored readers, the My Tags editor's
//! parse, and the library model locallibrary builds from a database. Called directly, without
//! the `catch_unwind` locallibrary wraps them in, so a panic here is a crash and not a parse
//! error.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rekordbox_pdb::{Database, ExtDatabase, ExtEditor};

fuzz_target!(|data: &[u8]| {
    if let Ok(db) = Database::from_bytes(data.to_vec()) {
        // Playlist trees from the file can be cyclic or dangling: `untangle` has to cope.
        locallibrary::pdb::library_from_database(&db);
    }
    if let Ok(db) = ExtDatabase::from_bytes(data.to_vec()) {
        for category in db.categories() {
            db.tags_in(category.id);
        }
    }
    if let Ok(editor) = ExtEditor::from_bytes(data.to_vec()) {
        for (id, _) in editor.categories() {
            editor.tags(id);
        }
    }
});
