//! Packing tracks the players can't play as lossless copies they can.

use crate::common;

use std::fs;
use std::path::Path;

use locallibrary::anlz;
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions};
use locallibrary::pack::{self, Container, PackOptions, Transcode};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::{Error, Library, paths, pdb, probe};
use rekordbox_pdb::NewTrack;
use rekordcrate::pdb::{Header, PageType};

const HI_RES: &str = "/Contents/Artist/Album/Hi Res.wav";
const SLOW_MP3: &str = "/Contents/Artist/Album/Slow.mp3";

/// A drive whose playlist "Set" holds a 24-bit 96 kHz WAV, an MP3 at 22.05 kHz and two of the
/// fixture's MP3s, in both databases.
fn source(dir: &Path) -> std::path::PathBuf {
    let root = common::bigger_drive(dir, "Main");
    let track = |path: &str, rate: u32, depth: u16| NewTrack {
        analyze_path: format!("/PIONEER/USBANLZ/P0AA/{:08X}/ANLZ0000.DAT", rate),
        duration: 1,
        sample_rate: rate,
        sample_depth: depth,
        ..NewTrack::new(path.rsplit('/').next().unwrap(), path.to_string())
    };
    let hi = common::add_track(&root, track(HI_RES, 96_000, 24), &common::media::wav_of(96_000, 24, 2, 1.0));
    let slow = common::add_track(&root, track(SLOW_MP3, 22_050, 0), &common::media::mp3());
    common::add_onelibrary(&root);
    let set = TrackSelector { ids: vec![hi, slow, 1, 2], ..Default::default() };
    let edits = [Edit::SetTracks { playlist: "Set".into(), tracks: set, sort: None, limit: None, create: true }];
    let mut d = Device::open_export(&root).unwrap();
    let plan = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    root
}

fn options(players: &[&str]) -> PackOptions {
    PackOptions {
        playlists: vec!["Set".into()],
        transcode: Some(Transcode { players: players.iter().map(|p| p.to_string()).collect(), ..Default::default() }),
        ..Default::default()
    }
}

fn libraries(root: &Path) -> [Library; 2] {
    [pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap(), common::plus_library(root)]
}

/// A 24/96 WAV goes to a CDJ-2000NXS (WAV and AIFF at 48 kHz at most) as a 48 kHz AIFF that keeps
/// its row: both databases point at it with its format, size, rate and depth, its analysis files
/// record its path, and its master isn't packed. Packing again keeps the copy; a master replaced
/// at the same path is copied again.
#[test]
fn unplayable_lossless_tracks_are_packed_as_copies_the_players_play() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = source(dir.path());
    let club_root = dir.path().join("Club");
    fs::create_dir_all(&club_root).unwrap();
    let mut src = Device::open_export(&src_root).unwrap();
    src.adopt(None).unwrap();
    let mut club = Device::open(&club_root).unwrap();

    let plan = pack::plan(&src, &club, &options(&["CDJ-2000NXS"]), &Ctx::silent()).unwrap();
    let [t] = plan.transcodes.as_slice() else { panic!("{:?}", plan.transcodes) };
    assert_eq!((t.from.as_str(), t.to.as_str()), (&HI_RES[1..], "Contents/Artist/Album/Hi Res.aiff"));
    assert_eq!(t.recipe, "aiff-48000-24-tpdf-v1");
    assert_eq!(t.why, "CDJ-2000NXS doesn't play it at 96 kHz");
    assert!(!t.current);
    // A second of 24-bit stereo at 48 kHz, and the header.
    assert!((288_000..290_000).contains(&t.size), "{}", t.size);
    assert!(plan.needed_bytes >= plan.transcode_bytes + plan.copy_bytes);
    assert!(plan.copies.iter().all(|c| c.path != HI_RES[1..]), "the master isn't packed");
    assert!(plan.unplayable.is_empty(), "the NXS plays MPEG-2 MP3s: {:?}", plan.unplayable);
    let report = pack::apply(&plan, &src, &mut club, None, &Ctx::silent()).unwrap();
    assert_eq!(report.transcoded, 1);

    common::sqlcipher_reads(&club_root);
    let copy = club_root.join("Contents/Artist/Album/Hi Res.aiff");
    assert!(!club_root.join(&HI_RES[1..]).exists());
    let format = probe::format(&copy).unwrap();
    assert_eq!((format.sample_rate, format.bit_depth, format.channels), (48_000, 24, 2));
    // symphonia 0.5 counts SSND's offset and block size fields as one more frame.
    assert!(format.duration_secs.is_some_and(|d| (d - 1.0).abs() < 1e-4), "{format:?}");
    let size = fs::metadata(&copy).unwrap().len();
    for lib in libraries(&club_root) {
        let t = lib.tracks.iter().find(|t| t.title == "Hi Res.wav").unwrap();
        assert_eq!(t.file_path, "/Contents/Artist/Album/Hi Res.aiff");
        assert_eq!((t.file_size, t.sample_rate, t.sample_depth, t.bitrate), (size, 48_000, 24, 2304));
        assert!(lib.playlists.iter().any(|p| p.track_ids.contains(&t.id)), "it stays in its playlist");
        for ext in ["DAT", "EXT"] {
            let key = paths::key(&t.analyze_path).replace(".DAT", &format!(".{ext}"));
            let bytes = fs::read(paths::to_fs(&club_root, &key)).unwrap();
            assert_eq!(anlz::read_path(&bytes).unwrap().as_deref(), Some(t.file_path.as_str()));
        }
    }
    // rekordbox's format code for AIFF (12), as rekordcrate, an independent parser, reads it (its
    // `unknown6`, private: from its Debug output).
    let pdb_bytes = fs::read(club_root.join(paths::EXPORT_PDB)).unwrap();
    let mut reader = std::io::Cursor::new(&pdb_bytes);
    let header = <Header as binrw::BinRead>::read(&mut reader).unwrap();
    let table = header.tables.iter().find(|t| t.page_type == PageType::Tracks).unwrap();
    let pages = header.read_pages(&mut reader, binrw::Endian::NATIVE, (&table.first_page, &table.last_page)).unwrap();
    let row = pages
        .iter()
        .flat_map(|p| p.row_groups.iter())
        .flat_map(|g| g.present_rows())
        .map(|r| format!("{r:?}"))
        .find(|r| r.contains("Hi Res.aiff"))
        .unwrap();
    assert!(row.contains("unknown6: 12,"), "{row}");

    // Again: the copy is current, and kept.
    let mut club = Device::open(&club_root).unwrap();
    let again = pack::plan(&src, &club, &PackOptions::default(), &Ctx::silent()).unwrap();
    assert!(again.transcodes.iter().all(|t| t.current), "{:?}", again.transcodes);
    assert_eq!(again.transcode_bytes, 0);
    let made = fs::metadata(&copy).unwrap().modified().unwrap();
    assert_eq!(pack::apply(&again, &src, &mut club, None, &Ctx::silent()).unwrap().transcoded, 0);
    assert_eq!(fs::metadata(&copy).unwrap().modified().unwrap(), made);

    // A better master at the same path is copied again, though its name and place are the same.
    fs::write(src_root.join(&HI_RES[1..]), common::media::wav_of(96_000, 24, 2, 2.0)).unwrap();
    let src = Device::open_export(&src_root).unwrap();
    let upgraded = pack::plan(&src, &club, &PackOptions::default(), &Ctx::silent()).unwrap();
    assert!(upgraded.transcodes.iter().all(|t| !t.current), "{:?}", upgraded.transcodes);

    // A sync or push over the drive forgets the copies with the pack.
    pack::forget(&club).unwrap();
    assert!(!club.state_dir().join("transcodes.json").exists());
}

/// A lossy file a player can't play is left as it is unless decoding it is asked for, and never
/// encoded lossy again; FLAC asked for goes to AIFF on a player without FLAC.
#[test]
fn lossy_tracks_are_decoded_only_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = source(dir.path());
    let src = Device::open_export(&src_root).unwrap();
    fs::create_dir_all(dir.path().join("Club")).unwrap();
    let club = Device::open(dir.path().join("Club")).unwrap();

    let plan = pack::plan(&src, &club, &options(&["CDJ-3000"]), &Ctx::silent()).unwrap();
    assert!(plan.transcodes.is_empty(), "the CDJ-3000 plays 24/96 WAV: {:?}", plan.transcodes);
    let [u] = plan.unplayable.as_slice() else { panic!("{:?}", plan.unplayable) };
    assert_eq!((u.track.file_path.as_str(), u.why.as_str()), (SLOW_MP3, "CDJ-3000 doesn't play it at 22.05 kHz"));
    assert!(plan.warnings.iter().any(|w| w.starts_with("1 lossy track(s)")), "{:?}", plan.warnings);

    let mut decode = options(&["CDJ-3000", "CDJ-2000NXS"]);
    let t = decode.transcode.as_mut().unwrap();
    (t.decode_lossy, t.container) = (true, Container::Flac);
    let plan = pack::plan(&src, &club, &decode, &Ctx::silent()).unwrap();
    let recipes: Vec<(&str, &str)> = plan.transcodes.iter().map(|t| (t.to.as_str(), t.recipe.as_str())).collect();
    assert_eq!(
        recipes,
        [
            ("Contents/Artist/Album/Hi Res.aiff", "aiff-48000-24-tpdf-v1"),
            ("Contents/Artist/Album/Slow.aiff", "aiff-44100-16-tpdf-v1"),
        ],
        "the NXS plays no FLAC"
    );
}

/// Copies for no players are refused, as are players the table doesn't know.
#[test]
fn copies_need_known_players() {
    let dir = tempfile::tempdir().unwrap();
    let src = Device::open_export(source(dir.path())).unwrap();
    fs::create_dir_all(dir.path().join("Club")).unwrap();
    let club = Device::open(dir.path().join("Club")).unwrap();
    let err = pack::plan(&src, &club, &options(&[]), &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("name the players"), "{err}");
    let err = pack::plan(&src, &club, &options(&["CDJ-9999"]), &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err}");
}

fn club(dir: &Path) -> Device {
    fs::create_dir_all(dir.join("Club")).unwrap();
    Device::open(dir.join("Club")).unwrap()
}

/// A copy in the master's own format takes the master's path: `--container wav` on a 96 kHz
/// WAV makes a 48 kHz WAV where the master would have gone.
#[test]
fn a_copy_in_the_masters_format_takes_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let src = Device::open_export(source(dir.path())).unwrap();
    let mut club = club(dir.path());
    let mut opts = options(&["CDJ-2000NXS"]);
    opts.transcode.as_mut().unwrap().container = Container::Wav;
    let plan = pack::plan(&src, &club, &opts, &Ctx::silent()).unwrap();
    let [t] = plan.transcodes.as_slice() else { panic!("{:?} {:?}", plan.transcodes, plan.warnings) };
    assert_eq!((t.to.as_str(), t.recipe.as_str()), (&HI_RES[1..], "wav-48000-24-tpdf-v1"));
    assert!(!plan.warnings.iter().any(|w| w.contains("another track's file")), "{:?}", plan.warnings);
    pack::apply(&plan, &src, &mut club, None, &Ctx::silent()).unwrap();
    let format = probe::format(&dir.path().join("Club").join(&HI_RES[1..])).unwrap();
    assert_eq!(format.sample_rate, 48_000);
}

/// Packing without copies leaves no record of copies; a record that can't be read counts as
/// none, with a warning, rather than stopping every later pack.
#[test]
fn copy_records_are_kept_only_when_there_are_copies() {
    let dir = tempfile::tempdir().unwrap();
    let mut src = Device::open_export(source(dir.path())).unwrap();
    src.adopt(None).unwrap();
    let mut club = club(dir.path());
    let plain = PackOptions { playlists: vec!["Set".into()], ..Default::default() };
    let plan = pack::plan(&src, &club, &plain, &Ctx::silent()).unwrap();
    pack::apply(&plan, &src, &mut club, None, &Ctx::silent()).unwrap();
    let record = club.state_dir().join("transcodes.json");
    assert!(!record.exists());

    fs::write(&record, b"{not json").unwrap();
    let plan = pack::plan(&src, &club, &options(&["CDJ-2000NXS"]), &Ctx::silent()).unwrap();
    assert_eq!(plan.transcodes.len(), 1);
    assert!(plan.warnings.iter().any(|w| w.contains("transcodes.json")), "{:?}", plan.warnings);

    // Made and current: packing again copies none of its analysis files.
    pack::apply(&plan, &src, &mut club, None, &Ctx::silent()).unwrap();
    let again = pack::plan(&src, &club, &PackOptions::default(), &Ctx::silent()).unwrap();
    assert!(again.transcodes[0].current);
    assert!(again.copies.iter().all(|c| !c.path.starts_with("PIONEER/USBANLZ/P0AA/00017700")), "{:?}", again.copies);

    // Packed again without copies: their record goes.
    let plan = pack::plan(&src, &club, &plain, &Ctx::silent()).unwrap();
    pack::apply(&plan, &src, &mut club, None, &Ctx::silent()).unwrap();
    assert!(!record.exists());
}

/// A copy that fails keeps the record of the copies made before it, so they aren't made again.
#[test]
fn copies_made_before_a_failure_are_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let root = source(dir.path());
    // A second master, after the first, that won't decode.
    let broken = "/Contents/Artist/Album/Broken.wav";
    let track = NewTrack {
        analyze_path: "/PIONEER/USBANLZ/P0AB/00000001/ANLZ0000.DAT".into(),
        duration: 1,
        sample_rate: 96_000,
        sample_depth: 24,
        ..NewTrack::new("Broken", broken)
    };
    let id = common::add_track(&root, track, b"RIFF this isn't a WAV");
    common::add_onelibrary(&root);
    let tracks = TrackSelector { ids: vec![id], ..Default::default() };
    let add = Edit::AddTracks { playlist: "Set".into(), tracks, position: None, sort: None, allow_repeats: false };
    common::apply(&root, &[add]);
    let src = Device::open_export(&root).unwrap();
    let mut club = club(dir.path());
    let plan = pack::plan(&src, &club, &options(&["CDJ-2000NXS"]), &Ctx::silent()).unwrap();
    assert_eq!(plan.transcodes.len(), 2, "{:?}", plan.transcodes);
    assert!(pack::apply(&plan, &src, &mut club, None, &Ctx::silent()).is_err());
    let record = fs::read_to_string(club.state_dir().join("transcodes.json")).unwrap();
    assert!(record.contains("Hi Res.aiff") && !record.contains("Broken.aiff"), "{record}");
}

/// A copy too long for AIFF (4 GiB at most) is refused when it's planned, not after hours of
/// writing it: a FLAC master saying it's 6 hours of 96 kHz audio.
#[test]
fn a_copy_too_long_for_aiff_is_refused_when_planned() {
    let dir = tempfile::tempdir().unwrap();
    let root = source(dir.path());
    // The master as FLAC, its header claiming 6 hours.
    let flac = root.join("Contents/Artist/Album/Hi Res.flac");
    let wav = root.join(&HI_RES[1..]);
    let recipe = loclib_transcode::Recipe {
        container: Container::Flac,
        sample_rate: 96_000,
        bit_depth: 24,
        dither: loclib_transcode::Dither::None,
    };
    loclib_transcode::transcode(&wav, &recipe, &flac, &Ctx::silent()).unwrap();
    let mut bytes = fs::read(&flac).unwrap();
    // STREAMINFO's 36-bit sample count is the low bits of the 8 bytes from offset 18.
    let mut word = u64::from_be_bytes(bytes[18..26].try_into().unwrap());
    word = (word & !((1 << 36) - 1)) | (96_000 * 6 * 3600);
    bytes[18..26].copy_from_slice(&word.to_be_bytes());
    fs::write(&flac, bytes).unwrap();
    fs::remove_file(&wav).unwrap();
    let new_path = "/Contents/Artist/Album/Hi Res.flac";
    let set_path = Edit::SetPath {
        track: TrackSelector::paths([HI_RES.to_string()]),
        path: new_path.into(),
        file: None,
        shift_ms: 0,
    };
    common::apply(&root, &[set_path]);
    let src = Device::open_export(&root).unwrap();
    let plan = pack::plan(&src, &club(dir.path()), &options(&["CDJ-2000NXS"]), &Ctx::silent()).unwrap();
    assert!(plan.transcodes.is_empty(), "{:?}", plan.transcodes);
    let [u] = plan.unplayable.as_slice() else { panic!("{:?}", plan.unplayable) };
    assert_eq!(u.why, "it's too long for AIFF, which holds 4 GiB at most");
}
