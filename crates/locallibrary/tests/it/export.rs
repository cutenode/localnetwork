use crate::common::{self, XmlOptions};

use std::fs;
use std::path::{Path, PathBuf};

use locallibrary::api::{self, ExportPlaylist};
use locallibrary::device::Device;
use locallibrary::export::{Format, PathStyle};
use locallibrary::progress::Ctx;
use locallibrary::{Error, anlz, paths};
use rekordbox_pdb::PdbEditor;

const A1: &str = "Contents/[a]pendics.shuffle/Framed And Defamed/01 A1 - It's In The Eyes.mp3";

/// The bigger fixture with `Shows/2026/Warmup` (tracks 1, 2) and `Shows/2026/Peak` (3, 1).
fn drive(dir: &Path) -> PathBuf {
    let root = common::bigger_drive(dir, "Main");
    let pdb = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    let shows = ed.create_playlist("Shows", 0, true).unwrap();
    let year = ed.create_playlist("2026", shows, true).unwrap();
    for (name, tracks) in [("Warmup", [1, 2]), ("Peak", [3, 1])] {
        let p = ed.create_playlist(name, year, false).unwrap();
        for t in tracks {
            ed.add_to_playlist(p, t, None).unwrap();
        }
    }
    ed.save(&pdb).unwrap();
    common::add_history(&root, "HISTORY 001", &[4, 5]);
    root
}

fn export(root: &Path, output: PathBuf, f: impl FnOnce(&mut ExportPlaylist)) -> api::ExportReport {
    let mut req = ExportPlaylist {
        drive: root.display().to_string(),
        playlist: None,
        history: None,
        archived: false,
        format: None,
        output,
        paths: PathStyle::Relative,
        split: false,
        replace: false,
    };
    f(&mut req);
    api::export_playlist(None, &req, &Ctx::silent()).unwrap()
}

#[test]
fn playlists_as_m3u8_with_each_path_style() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive(dir.path());
    let out = dir.path().join("lists/warmup.m3u8");
    let r = export(&root, out.clone(), |r| r.playlist = Some("Shows/2026/Warmup".into()));
    assert_eq!(r.format, Format::M3u8);
    assert_eq!((r.files.len(), r.files[0].tracks, r.files[0].name.as_str()), (1, 2, "Shows/2026/Warmup"));
    let text = fs::read_to_string(&out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[..3],
        ["#EXTM3U", "#PLAYLIST:Shows/2026/Warmup", "#EXTINF:380,[a]pendics.shuffle - A1 - It's In The Eyes"]
    );
    assert_eq!(lines[3], format!("../Main/{A1}"));
    assert!(out.parent().unwrap().join(lines[3]).is_file(), "relative paths resolve");
    assert_eq!(lines.len(), 6);

    let drive_paths = dir.path().join("drive.m3u");
    export(&root, drive_paths.clone(), |r| {
        r.playlist = Some("Warmup".into());
        r.paths = PathStyle::Drive;
    });
    assert!(fs::read_to_string(&drive_paths).unwrap().contains(&format!("\n/{A1}\n")));

    let absolute = dir.path().join("abs.m3u8");
    export(&root, absolute.clone(), |r| {
        r.playlist = Some("Warmup".into());
        r.paths = PathStyle::Absolute;
    });
    let abs = paths::to_fs(Device::open(&root).unwrap().root(), A1);
    assert!(fs::read_to_string(&absolute).unwrap().contains(&abs.display().to_string()));
}

#[test]
fn folders_libraries_and_history_as_csv_and_text() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive(dir.path());

    // A folder as one list: every track once.
    let txt = dir.path().join("shows.txt");
    export(&root, txt.clone(), |r| r.playlist = Some("Shows".into()));
    assert_eq!(
        fs::read_to_string(&txt).unwrap(),
        "01. [a]pendics.shuffle - A1 - It's In The Eyes\n02. [a]pendics.shuffle - A2 - In Charge\n\
         03. [a]pendics.shuffle - B1 - About To Be\n"
    );

    // ...or one file per playlist, in folders.
    let split = dir.path().join("split");
    let r = export(&root, split.clone(), |r| {
        r.playlist = Some("Shows".into());
        r.split = true;
        r.format = Some(Format::Csv);
    });
    assert_eq!(r.files.len(), 2);
    let peak = fs::read_to_string(split.join("2026/Peak.csv")).unwrap();
    assert!(peak.starts_with(
        "#,Title,Artist,Album,Genre,Label,BPM,Key,Rating,Duration,Year,Play Count,Date Added,Comment,Path\r\n"
    ));
    assert!(peak.contains("\r\n1,B1 - About To Be,[a]pendics.shuffle,Framed And Defamed,"));
    assert!(split.join("2026/Warmup.csv").is_file());

    // The whole library.
    let all = dir.path().join("all.csv");
    let r = export(&root, all.clone(), |_| {});
    assert_eq!(r.files[0].tracks, 12);
    assert_eq!(fs::read_to_string(&all).unwrap().lines().count(), 13);

    // A history list.
    let h = dir.path().join("history.txt");
    let r = export(&root, h.clone(), |r| r.history = Some("history 001".into()));
    assert_eq!(r.files[0].name, "HISTORY 001");
    assert_eq!(fs::read_to_string(&h).unwrap().lines().count(), 2);

    let req = ExportPlaylist {
        drive: root.display().to_string(),
        playlist: Some("Nope".into()),
        history: None,
        archived: false,
        format: None,
        output: dir.path().join("x.m3u8"),
        paths: PathStyle::Relative,
        split: false,
        replace: false,
    };
    assert!(matches!(api::export_playlist(None, &req, &Ctx::silent()), Err(Error::NotFound(_))));
}

/// Regression: exports replaced a file already there without a word.
#[test]
fn files_already_there_are_replaced_only_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive(dir.path());
    let mine = dir.path().join("friday.m3u8");
    fs::write(&mine, "mine").unwrap();
    let req = |replace| ExportPlaylist {
        drive: root.display().to_string(),
        playlist: Some("Warmup".into()),
        history: None,
        archived: false,
        format: None,
        output: mine.clone(),
        paths: PathStyle::Relative,
        split: false,
        replace,
    };
    let e = api::export_playlist(None, &req(false), &Ctx::silent()).unwrap_err();
    assert_eq!((e.code(), e.reason()), ("conflict", Some(locallibrary::error::Reason::Exists)), "{e}");
    assert_eq!(fs::read_to_string(&mine).unwrap(), "mine");
    api::export_playlist(None, &req(true), &Ctx::silent()).unwrap();
    assert!(fs::read_to_string(&mine).unwrap().starts_with("#EXTM3U"));
}

#[test]
fn xml_of_some_playlists_only() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive(dir.path());
    let d = Device::open_export(&root).unwrap();
    let opts = |only: &[&str]| XmlOptions { only: only.iter().map(|s| s.to_string()).collect(), ..Default::default() };

    let (text, report) = common::rekordbox_xml(&d, &opts(&["Shows/2026/Peak"])).unwrap();
    assert_eq!((report.tracks, report.playlists), (2, 1));
    assert_eq!(text.matches("<TRACK TrackID=").count(), 2);
    // The folders above it stay, holding only it.
    assert!(text.contains("<NODE Type=\"0\" Name=\"Shows\" Count=\"1\">"));
    assert!(text.contains("<NODE Type=\"0\" Name=\"2026\" Count=\"1\">"));
    assert!(text.contains("Name=\"Peak\"") && !text.contains("Name=\"Warmup\"") && !text.contains("Name=\"aab\""));

    let (_, report) = common::rekordbox_xml(&d, &opts(&["Shows", "aab"])).unwrap();
    assert_eq!((report.tracks, report.playlists), (12, 3));
    assert_eq!(common::rekordbox_xml(&d, &opts(&["Nope"])).unwrap_err(), "Nope");
}

#[test]
fn xml_export_is_valid_rekordbox_xml() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open_export(common::demo_drive(dir.path(), "USB")).unwrap();
    let (xml, report) = common::rekordbox_xml(&d, &XmlOptions::default()).unwrap();
    assert_eq!(report.tracks, 2);
    assert_eq!(report.beat_grids, 2);
    assert!(xml.contains("<TEMPO Inizio="));
    assert!(xml.contains("Tonality=\"Fm\""));
    assert!(xml.contains("Location=\"file://localhost/"));
    assert_well_formed(&xml);

    // Real cue data from the fixture's analysis files ends up in the XML.
    let lib = d.library().unwrap();
    let ext = std::fs::read(d.path(&paths::key(&lib.tracks[0].analyze_path.replace(".DAT", ".EXT")))).unwrap();
    let cues = anlz::cues(&ext).unwrap();
    assert_eq!(xml.matches("<POSITION_MARK").count(), report.cues);
    assert!(report.cues >= cues.len());

    let b = Device::open_export(common::bigger_drive(dir.path(), "B")).unwrap();
    let (xml, report) =
        common::rekordbox_xml(&b, &XmlOptions { location_root: Some("/Music".into()), ..Default::default() }).unwrap();
    assert_eq!(report.playlists, 1);
    assert!(xml.contains("<NODE Name=\"aab\" Type=\"1\" KeyType=\"0\" Entries=\"12\">"));
    assert!(xml.contains("file://localhost/Music/Contents/%5Ba%5Dpendics.shuffle/"));
    assert_well_formed(&xml);
    // rekordcrate's model of the format accepts it. (Its model can't represent empty playlist
    // folders, so this check needs a library with playlists.)
    let _doc: rekordcrate::xml::Document = quick_xml::de::from_str(&xml).expect("parses as DJ_PLAYLISTS");
}

fn assert_well_formed(xml: &str) {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut depth = 0i32;
    loop {
        match reader.read_event().expect("well-formed XML") {
            quick_xml::events::Event::Start(_) => depth += 1,
            quick_xml::events::Event::End(_) => depth -= 1,
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    assert_eq!(depth, 0);
}
