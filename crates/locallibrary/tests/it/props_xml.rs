//! What the rekordbox XML export says of each track: the kind its file's extension names, the
//! colour and rating rekordbox gives it, and the year (or its release date's); and which
//! playlists it holds when only some are asked for.

use crate::common;

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::device::Device;
use locallibrary::paths;
use locallibrary::progress::Ctx;
use locallibrary::xml::{self, XmlOptions};
use rekordbox_pdb::{NewTrack, PdbEditor};

/// Extensions a track's file can have, and the kind rekordbox's XML calls each (case aside).
const KINDS: &[(&str, &str)] = &[
    ("mp3", "MP3 File"),
    ("m4a", "M4A File"),
    ("mp4", "M4A File"),
    ("aac", "M4A File"),
    ("wav", "WAV File"),
    ("aif", "AIFF File"),
    ("aiff", "AIFF File"),
    ("flac", "FLAC File"),
    // What it doesn't know it calls MP3, as rekordbox does.
    ("ogg", "MP3 File"),
    ("opus", "MP3 File"),
];

/// rekordbox's colours by id (1 pink to 8 purple), as its XML spells them.
const COLOURS: [&str; 8] =
    ["0xFF007F", "0xFF0000", "0xFFA500", "0xFFFF00", "0x00FF00", "0x25FDE9", "0x0000FF", "0x660099"];

/// The attribute `name` of the `TRACK` element of track `id` in `xml`.
fn attribute(xml: &str, id: u32, name: &str) -> Option<String> {
    let element = xml.lines().find(|l| l.trim_start().starts_with(&format!("<TRACK TrackID=\"{id}\" ")))?;
    let at = element.find(&format!(" {name}=\""))? + name.len() + 3;
    Some(element[at..at + element[at..].find('"')?].to_string())
}

/// Every track's kind follows its extension in any case, its colour is rekordbox's for its
/// colour id (none without one), its rating is the 0 to 255 scale rekordbox's XML uses (five
/// stars at most), and its year is the release date's when it has none of its own. Asking for
/// only a playlist the drive has exports just it; one it hasn't is refused naming it.
#[hegel::test(test_cases = 30)]
fn xml_tracks_say_what_rekordbox_says_of_them(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    // The twelve-track export, with drawn tracks added.
    let root = common::bigger_drive(dir.path(), "USB");
    let mut want = Vec::new();
    for n in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        let (ext, kind) = tc.draw(gs::sampled_from(KINDS));
        let ext = if tc.draw(gs::booleans()) { ext.to_uppercase() } else { ext.to_string() };
        let color = tc.draw(gs::integers::<u8>().max_value(8));
        let rating = tc.draw(gs::integers::<u8>().max_value(7));
        let year = tc.draw(gs::sampled_from(vec![0u16, 1999, 2026]));
        let released = tc.draw(gs::sampled_from(vec!["", "2011-04-01", "19xx"]));
        let path = format!("/Contents/Drawn/Track {n}.{ext}");
        let track = NewTrack {
            color_id: color,
            rating,
            year,
            release_date: released.into(),
            ..NewTrack::new(format!("Drawn {n}"), path.clone())
        };
        let id = common::add_track(&root, track, &common::placeholder_audio(&path));
        let year = match year {
            0 => released.get(..4).and_then(|y| y.parse().ok()).unwrap_or(0),
            y => y,
        };
        let colour = (color > 0).then(|| COLOURS[usize::from(color - 1)].to_string());
        want.push((id, kind, colour, [0, 51, 102, 153, 204, 255][usize::from(rating.min(5))], year));
    }
    let d = Device::open_export(&root).unwrap();
    let (text, report) = xml::export(&d, &XmlOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(report.tracks, 12 + want.len());
    for (id, kind, colour, rating, year) in want {
        assert_eq!(attribute(&text, id, "Kind").as_deref(), Some(kind), "track {id}");
        assert_eq!(attribute(&text, id, "Colour"), colour, "track {id}");
        assert_eq!(attribute(&text, id, "Rating"), Some(rating.to_string()), "track {id}");
        assert_eq!(attribute(&text, id, "Year"), Some(year.to_string()), "track {id}");
    }

    let names = ["Peak", "Closing", "B2B"];
    let made = tc.draw(gs::sampled_from(names.to_vec()));
    let pdb = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    ed.create_playlist(made, 0, false).unwrap();
    ed.save(&pdb).unwrap();
    let asked = tc.draw(gs::sampled_from(names.to_vec()));
    let d = Device::open_export(&root).unwrap();
    let only = XmlOptions { only: vec![asked.to_string()], ..Default::default() };
    match xml::export(&d, &only, &Ctx::silent()) {
        Ok((text, report)) => {
            assert_eq!(asked, made);
            assert_eq!((report.playlists, report.tracks), (1, 0));
            assert!(text.contains(&format!("Name=\"{made}\"")), "{text}");
        }
        Err(e) => {
            assert_ne!(asked, made);
            assert_eq!(e.code(), "not_found", "{e}");
            assert!(e.to_string().contains(asked), "{e}");
        }
    }
}
