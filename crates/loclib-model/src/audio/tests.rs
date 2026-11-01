use super::*;

use hegel::TestCase;
use hegel::generators as gs;

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
fn every_extension_names_its_format_in_any_case(tc: TestCase) {
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
