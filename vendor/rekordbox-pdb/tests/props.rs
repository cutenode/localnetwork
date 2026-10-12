//! Properties of the editor's track rows, read back through the reader's named accessors.

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use rekordbox_pdb::pdb::TRACK_STRING_NAMES;
use rekordbox_pdb::{Database, NewTrack, PdbEditor, PdbError, file_type_code};

const ONE_SONG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/one-song-export.pdb");

/// A file's format code comes from its last extension in any case, and is 0 for an extension
/// rekordbox doesn't list or a name without one.
#[hegel::test(test_cases = 300)]
fn file_type_codes_follow_the_last_extension(tc: TestCase) {
    let codes = [("mp3", 1), ("m4a", 4), ("mp4", 4), ("aac", 4), ("flac", 5), ("wav", 0x0b), ("aiff", 0x0c), ("aif", 0x0c)];
    let (ext, code) = tc.draw(gs::sampled_from(codes.to_vec()));
    let upper: Vec<bool> = tc.draw(gs::vecs(gs::booleans()).min_size(ext.len()).max_size(ext.len()));
    let spelled: String =
        ext.chars().zip(upper).map(|(c, u)| if u { c.to_ascii_uppercase() } else { c }).collect();
    let stem = tc.draw(gs::text().max_size(12));
    assert_eq!(file_type_code(&format!("{stem}.{spelled}")), code);
    assert_eq!(file_type_code(&format!("{stem}.{spelled}.part")), 0);
    let bare = tc.draw(gs::text().max_size(12).filter(|s: &String| !s.contains('.')));
    assert_eq!(file_type_code(&bare), 0);
}

/// A track added with any text in its string fields reads back through the named accessors
/// (and the same strings by position), and setting one string by name changes only that one.
#[hegel::test(test_cases = 100)]
fn track_strings_read_back_by_name(tc: TestCase) {
    let text = || gs::text().max_size(20).exclude_characters("\0");
    let t = NewTrack {
        title: tc.draw(text()),
        file_path: format!("/Contents/{}.mp3", tc.draw(gs::text().alphabet("abc").max_size(8))),
        comment: tc.draw(text()),
        mix_name: tc.draw(text()),
        release_date: tc.draw(text()),
        analyze_path: tc.draw(text()),
        date_added: tc.draw(text()),
        ..Default::default()
    };
    let mut ed = PdbEditor::from_bytes(std::fs::read(ONE_SONG).unwrap());
    let id = ed.add_track(&t).unwrap();
    let field = tc.draw(gs::sampled_from(vec!["message", "analyze_date", "comment", "mix_name"]));
    let value = tc.draw(text());
    ed.set_track_strings(id, &[(field, &value)]).unwrap();

    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    let row = db.tracks.iter().find(|r| r.id == id).unwrap();
    let want = |name: &str, default: &str| if name == field { value.clone() } else { default.to_string() };
    let named = [
        ("title", row.title(), t.title.clone()),
        ("comment", row.comment(), t.comment.clone()),
        ("mix_name", row.mix_name(), t.mix_name.clone()),
        ("release_date", row.release_date(), t.release_date.clone()),
        ("analyze_path", row.analyze_path(), t.analyze_path.clone()),
        ("date_added", row.date_added(), t.date_added.clone()),
        ("file_path", row.file_path(), t.file_path.clone()),
        ("filename", row.filename(), t.file_path.rsplit('/').next().unwrap().to_string()),
        ("message", row.message(), String::new()),
        // Analysed when added, as far as the row says.
        ("analyze_date", row.analyze_date(), t.date_added.clone()),
    ];
    for (name, got, written) in named {
        assert_eq!(got, want(name, &written), "{name}");
        let at = TRACK_STRING_NAMES.iter().position(|n| *n == name).unwrap();
        assert_eq!(row.strings[at], got, "{name} is string {at}");
    }
    assert!(matches!(ed.set_track_strings(u32::MAX, &[(field, "x")]), Err(PdbError::TrackNotFound(_))));
}

/// Every refusal says what was refused, naming the value it was given.
#[hegel::test(test_cases = 100)]
fn errors_name_what_they_refused(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>());
    let name = tc.draw(gs::text().max_size(10));
    let errors = [
        (PdbError::UnknownString { kind: n as u8, offset: n as usize }, format!("{:#04x}", n as u8)),
        (PdbError::UnknownField(name.clone()), format!("{name:?}")),
        (PdbError::TrackNotFound(n), n.to_string()),
        (PdbError::PlaylistNotFound(n), n.to_string()),
        (PdbError::RowNotFound(n as usize), format!("{:#x}", n as usize)),
        (PdbError::RowTooLarge { alloc: n as usize, max: 7 }, n.to_string()),
        (PdbError::ValueOutOfRange { field: name.clone(), value: u64::from(n), size: 2 }, format!("{name}={n}")),
        (PdbError::TagNotFound(n), n.to_string()),
        (PdbError::IdInUse(n), n.to_string()),
        (PdbError::Malformed(name.clone()), name.clone()),
        (PdbError::Io(std::io::Error::other(name.clone())), name.clone()),
    ];
    for (e, named) in errors {
        let shown = e.to_string();
        assert!(shown.contains(&named), "{shown:?} doesn't name {named:?}");
        let _: &dyn std::error::Error = &e;
    }
    for e in [PdbError::TooManySlots, PdbError::StringTooLong] {
        assert!(!e.to_string().is_empty());
    }
    let io: PdbError = std::io::Error::other(name.clone()).into();
    assert!(matches!(io, PdbError::Io(_)));
}
