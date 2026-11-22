use super::*;

use hegel::generators as gs;
use hegel::{Generator, TestCase};

#[test]
fn formats_are_told_by_the_last_extension() {
    assert_eq!(Format::from_name("Contents/a.b/c.MP3"), Some(Format::Mp3));
    assert_eq!(Format::from_name("i.txt.wav"), Some(Format::Wav));
    assert_eq!(Format::from_name("h.mp3.txt"), None);
    assert_eq!(Format::from_name("Contents/a.mp3/c"), None);
    assert_eq!(Format::from_name("mp3"), None);
    assert_eq!(Format::from_name("j.mp3 "), None);
    assert_eq!(Format::from_extension(".mp3"), None);
}

#[test]
fn players_list_only_the_extensions_their_manuals_give() {
    assert_eq!(Format::for_players("/Contents/a.fla"), Some(Format::Flac));
    assert_eq!(Format::for_players("/Contents/a.M4A"), Some(Format::Mpeg4));
    for name in ["a.wave", "a.alac", "a.ogg", "a.opus", "a.wma", "a.txt", "a"] {
        assert_eq!(Format::for_players(name), None, "{name}");
    }
    assert_eq!(Format::from_name("a.wave"), Some(Format::Wav));
}

/// Every extension is in one table, lower case, and names its format in any letter case, on
/// its own or ending a path; players list a file by it exactly when it's in their table.
#[hegel::test(test_cases = 200)]
fn prop_every_extension_names_its_format_in_any_case(tc: TestCase) {
    let all: Vec<(&str, Format)> = PLAYER_EXTENSIONS.into_iter().chain(OTHER_EXTENSIONS).collect();
    let (ext, format) = tc.draw(gs::sampled_from(all.clone()));
    assert_eq!(ext, ext.to_ascii_lowercase());
    assert_eq!(all.iter().filter(|(e, _)| *e == ext).count(), 1, "{ext}");
    let cased: String = ext.chars().map(|c| if tc.draw(gs::booleans()) { c.to_ascii_uppercase() } else { c }).collect();
    let name = format!("{}/set.{cased}", tc.draw(gs::sampled_from(vec!["", "/Contents/a.flac", "C:\\x.y"])));
    assert_eq!(Format::from_extension(&cased), Some(format));
    assert_eq!(Format::from_name(&name), Some(format), "{name}");
    let listed = PLAYER_EXTENSIONS.iter().any(|(e, _)| *e == ext);
    assert_eq!(Format::for_players(&name), listed.then_some(format), "{name}");
}

/// A file's format code comes from its last extension in any case, and is 0 for an extension
/// rekordbox doesn't list or a name without one.
#[hegel::test(test_cases = 300)]
fn prop_file_type_codes_follow_the_last_extension(tc: TestCase) {
    let codes =
        [("mp3", 1), ("m4a", 4), ("mp4", 4), ("aac", 4), ("flac", 5), ("wav", 0x0b), ("aiff", 0x0c), ("aif", 0x0c)];
    let (ext, code) = tc.draw(gs::sampled_from(codes.to_vec()));
    let upper: Vec<bool> = tc.draw(gs::vecs(gs::booleans()).min_size(ext.len()).max_size(ext.len()));
    let spelled: String = ext.chars().zip(upper).map(|(c, u)| if u { c.to_ascii_uppercase() } else { c }).collect();
    let stem = tc.draw(gs::text().max_size(12));
    assert_eq!(file_type_code(&format!("{stem}.{spelled}")), code);
    assert_eq!(file_type_code(&format!("{stem}.{spelled}.part")), 0);
    let bare = tc.draw(gs::text().max_size(12).filter(|s: &String| !s.contains('.')));
    assert_eq!(file_type_code(&bare), 0);
}
