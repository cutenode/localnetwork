use crate::common;

use std::io::Cursor;

use locallibrary::artwork::{self, Artwork};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::xml::{self, XmlOptions};
use locallibrary::{anlz, paths};

fn png(w: u32, h: u32, shade: u8) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, shade]));
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn dims(path: &std::path::Path) -> (u32, u32) {
    let img = image::open(path).unwrap();
    (img.width(), img.height())
}

#[test]
fn set_artwork_on_drive() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    let lib = d.library().unwrap();
    assert_eq!(artwork::missing(&d, &lib).len(), 2);

    let art = Artwork::from_bytes(&png(500, 400, 10)).unwrap();
    let r = artwork::set_on_drive(&mut d, &[1, 2], &art, &Ctx::silent()).unwrap();
    assert_eq!((r.tracks_updated, r.artwork_rows_added), (2, 1));
    let lib = d.library().unwrap();
    assert!(artwork::missing(&d, &lib).is_empty());
    let t = &lib.tracks[0];
    assert_eq!(t.artwork_id, lib.tracks[1].artwork_id);
    let key = paths::key(t.artwork_path.as_ref().unwrap());
    assert_eq!(dims(&d.path(&key)), (80, 80));
    assert_eq!(dims(&d.path(&artwork::medium_variant(&key))), (240, 240));

    // Changing just one track's art gives it its own row; the other keeps the shared one.
    let art2 = Artwork::from_bytes(&png(300, 300, 200)).unwrap();
    let r = artwork::set_on_drive(&mut d, &[2], &art2, &Ctx::silent()).unwrap();
    assert_eq!(r.artwork_rows_added, 1);
    let lib = d.library().unwrap();
    assert_ne!(lib.tracks[0].artwork_id, lib.tracks[1].artwork_id);

    // Changing it again replaces the files in place.
    let r = artwork::set_on_drive(&mut d, &[2], &art, &Ctx::silent()).unwrap();
    assert_eq!(r.artwork_rows_added, 0);
    let preview = artwork::read_from_drive(&d, &d.library().unwrap(), 2).unwrap().unwrap();
    assert!(image::load_from_memory(&preview).is_ok());
    assert!(artwork::set_on_drive(&mut d, &[99], &art, &Ctx::silent()).is_err());
}

#[test]
fn auto_artwork_from_folder_images() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open_export(common::bigger_drive(dir.path(), "A")).unwrap();
    let lib = d.library().unwrap();
    let album_dir = d.path("Contents/[a]pendics.shuffle/Framed And Defamed");
    std::fs::write(album_dir.join("cover.png"), png(64, 64, 50)).unwrap();

    let plan = artwork::plan_auto(&d, &lib);
    assert_eq!(plan.found.len(), 4);
    assert_eq!(plan.not_found.len(), 8);
    let r = artwork::apply_auto(&mut d, &plan, &Ctx::silent()).unwrap();
    // One image -> one shared artwork row for the whole album.
    assert_eq!((r.tracks_updated, r.artwork_rows_added), (4, 1));
    assert_eq!(artwork::missing(&d, &d.library().unwrap()).len(), 8);
}

/// Minimal valid 16-bit mono PCM WAV.
fn wav() -> Vec<u8> {
    let samples = 4410u32;
    let data_len = samples * 2;
    let mut w = Vec::new();
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&44100u32.to_le_bytes());
    w.extend_from_slice(&88200u32.to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    w.extend(std::iter::repeat_n(0u8, data_len as usize));
    w
}

/// Regression: `art set` made its backup and interrupted marker and wrote the new images before
/// editing the databases in memory, so one that failed there (here export.pdb has no artwork
/// table, so its editor can't add a row) left them behind. Now the drive is left as it was.
#[test]
fn setting_artwork_that_fails_leaves_the_drive_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    common::add_onelibrary(&root);
    // The table directory: the number of tables at 8, then 16 bytes each from 0x1c, type first.
    let path = root.join(paths::EXPORT_PDB);
    let mut bytes = std::fs::read(&path).unwrap();
    let tables = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let table = (0..tables).map(|i| 0x1c + i * 16).find(|&o| bytes[o..o + 4] == [13, 0, 0, 0]).unwrap();
    bytes[table] = 0xff;
    std::fs::write(&path, bytes).unwrap();
    let before = common::Before::take(&root);
    let mut d = Device::open_export(&root).unwrap();
    let art = Artwork::from_bytes(&png(300, 300, 40)).unwrap();
    artwork::set_on_drive(&mut d, &[1, 2], &art, &Ctx::silent()).unwrap_err();
    before.untouched(&root).unwrap();
}

#[test]
fn embed_into_audio_file_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("track.wav");
    std::fs::write(&f, wav()).unwrap();
    assert!(artwork::read_embedded(&f).unwrap().is_none());
    artwork::embed_in_file(&f, &Artwork::from_bytes(&png(1000, 1000, 0)).unwrap()).unwrap();
    let pic = artwork::read_embedded(&f).unwrap().expect("embedded");
    let img = image::load_from_memory(&pic).unwrap();
    assert_eq!((img.width(), img.height()), (artwork::EMBED_SIZE, artwork::EMBED_SIZE));
    // An audio file works as an artwork source too.
    assert!(Artwork::from_path(&f).is_ok());
}

#[test]
fn embedded_art_can_be_taken_back_out() {
    let dir = tempfile::tempdir().unwrap();
    let store = locallibrary::Store::open_or_init(dir.path().join("store")).unwrap();
    let image = dir.path().join("cover.png");
    std::fs::write(&image, png(400, 400, 9)).unwrap();
    let (a, b) = (dir.path().join("a.wav"), dir.path().join("b.wav"));
    for f in [&a, &b] {
        std::fs::write(f, wav()).unwrap();
    }
    let req = locallibrary::api::EmbedArtwork { files: vec![a.clone(), b.clone()], image };
    let plan = locallibrary::api::artwork_embed_plan(&req).unwrap();
    assert_eq!(plan.files.len(), 2);
    assert!(plan.destructive && plan.backup_bytes > 0);
    let report = plan.apply(Some(&store), &Ctx::silent()).unwrap();
    assert_eq!(report.embedded.len(), 2);
    assert!(artwork::read_embedded(&a).unwrap().is_some());

    // b changes again afterwards: it's left as it is, a goes back to what it was.
    std::fs::write(&b, [wav(), vec![0; 64]].concat()).unwrap();
    let undo = locallibrary::api::UnembedRequest { backup: report.backup.clone() };
    let plan = locallibrary::api::artwork_unembed_plan(&store, &undo).unwrap();
    assert_eq!((plan.restored.len(), plan.skipped.len()), (1, 1));
    let r = plan.apply(Some(&store), &Ctx::silent()).unwrap();
    assert_eq!(r.restored, std::slice::from_ref(&a));
    assert_eq!(std::fs::read(&a).unwrap(), wav());
    assert!(artwork::read_embedded(&a).unwrap().is_none());
}

/// Regression: `art embed` ignored Ctrl-C, and listed what it changed only once every file was
/// done, so a run stopped part way couldn't be taken back.
#[test]
fn an_embed_stopped_part_way_can_be_taken_back() {
    struct StopAfterFirst(locallibrary::CancelToken);
    impl locallibrary::Progress for StopAfterFirst {
        fn on_event(&self, e: &locallibrary::Event) {
            if matches!(e, locallibrary::Event::Item { .. }) {
                self.0.cancel();
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let store = locallibrary::Store::open_or_init(dir.path().join("store")).unwrap();
    let image = dir.path().join("cover.png");
    std::fs::write(&image, png(400, 400, 9)).unwrap();
    let (a, b) = (dir.path().join("a.wav"), dir.path().join("b.wav"));
    for f in [&a, &b] {
        std::fs::write(f, wav()).unwrap();
    }
    let req = locallibrary::api::EmbedArtwork { files: vec![a.clone(), b.clone()], image };
    let plan = locallibrary::api::artwork_embed_plan(&req).unwrap();
    let stop = StopAfterFirst(locallibrary::CancelToken::new());
    let err = plan.apply(Some(&store), &Ctx::new(&stop, stop.0.clone())).unwrap_err();
    assert!(matches!(err, locallibrary::Error::Cancelled), "{err}");
    assert!(artwork::read_embedded(&a).unwrap().is_some());
    assert_eq!(std::fs::read(&b).unwrap(), wav(), "stopped before b");
    let undo = locallibrary::api::UnembedRequest { backup: None };
    let r =
        locallibrary::api::artwork_unembed_plan(&store, &undo).unwrap().apply(Some(&store), &Ctx::silent()).unwrap();
    assert_eq!(r.restored, std::slice::from_ref(&a));
    assert_eq!(std::fs::read(&a).unwrap(), wav());
}

/// Regression: a file given by a relative path (or a symlink) inside a rekordbox drive wasn't
/// seen to be on it, so `art embed` rewrote the drive's audio outside its write routine.
#[cfg(unix)]
#[test]
fn files_on_a_drive_are_never_embedded_into_however_named() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let lib = Device::open_export(&root).unwrap().library().unwrap();
    let on_drive = paths::to_fs(&root, &paths::key(&lib.tracks[0].file_path));
    let link = dir.path().join("link.mp3");
    std::os::unix::fs::symlink(&on_drive, &link).unwrap();
    let image = dir.path().join("cover.png");
    std::fs::write(&image, png(400, 400, 9)).unwrap();
    let plan =
        locallibrary::api::artwork_embed_plan(&locallibrary::api::EmbedArtwork { files: vec![link], image }).unwrap();
    assert!(plan.files.is_empty() && plan.skipped.len() == 1, "{:?}", plan.skipped);
}

#[test]
fn xml_export_is_valid_rekordbox_xml() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open_export(common::demo_drive(dir.path(), "USB")).unwrap();
    let (xml, report) = xml::export(&d, &XmlOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(report.tracks, 2);
    assert_eq!(report.tracks_with_beatgrid, 2);
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
        xml::export(&b, &XmlOptions { location_root: Some("/Music".into()), ..Default::default() }, &Ctx::silent())
            .unwrap();
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
