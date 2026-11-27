use std::path::PathBuf;

use super::*;

/// A write that came to `written` edits, with a warning, a backup and the counter it moved.
fn write(written: usize) -> CollectionWrite {
    CollectionWrite {
        written,
        waiting: 0,
        why: None,
        count: Some((7, 8)),
        backup: Some(PathBuf::from("/store/collection/backups/master.db")),
        warnings: vec!["a track was gone".into()],
    }
}

/// What a write or an undo of the collection came to is said however it came out: these are
/// what the CLI's own runs can't reach, a write rekordbox's collection takes (CI has no rekordbox
/// whose agent says Cloud Library Sync is off).
#[test]
fn what_a_write_came_to_is_said_however_it_came_out() {
    written(&write(2));
    let waiting = CollectionWrite { waiting: 1, why: Some("rekordbox is open".into()), ..Default::default() };
    written(&waiting);
    undone(&CollectionUndoReport { dropped: false, write: write(1) });
    undone(&CollectionUndoReport { dropped: true, write: CollectionWrite::default() });
}
