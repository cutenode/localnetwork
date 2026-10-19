//! Reading tracks by id and their artwork files once art is set: the medium size players show
//! when there is one, the full image otherwise, always inside the drive.

use crate::common::{self, media, path};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use hegel::generators as gs;
use hegel::{Generator, HealthCheck, TestCase};
use locallibrary::Error;
use locallibrary::api::{
    self, AnalysisRequest, ArtworkRequest, LibraryCache, OneOrMany, SetArtwork, TracksGet, WaveformChoice,
    WaveformEncoding,
};
use locallibrary::device::dunce_canonicalize;
use locallibrary::paths;
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;

/// The demo export with art set on its tracks, the medium size removed from some: each track's
/// artwork file is its medium size when the drive has one, else the full image, a JPEG inside
/// the drive, one at a time or several at once. Tracks asked for by id come back in the order
/// asked, ids the library lacks listed apart, and the library read is never one the drive has
/// changed from since; the inline preview is the artwork file's bytes. Analysis comes with the
/// waveforms chosen (a track the library lacks has none; several at once, at most 100 and no
/// scrolling ones), and art is embedded in a file here but never one on an export, a folder or a
/// file that isn't there, which applying lists as failed.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn a_tracks_artwork_file_is_the_size_players_show(tc: TestCase) {
    let side = tc.draw(gs::integers::<u32>().min_value(16).max_value(64));
    let dir = tempfile::tempdir().unwrap();
    let volumes = common::isolate(dir.path(), None);
    let root = dunce_canonicalize(&common::demo_drive(&volumes, "Main"));
    let image = dir.path().join("cover.png");
    std::fs::write(&image, media::png(side, side, 7)).unwrap();
    let ids: Vec<u32> = common::library(&root).tracks.iter().map(|t| t.id).collect();
    let set =
        SetArtwork { drive: path(&root), image, tracks: TrackSelector { ids: ids.clone(), ..Default::default() } };
    api::artwork_set(&set, &Ctx::silent()).unwrap();
    let lib = common::library(&root);
    // Tracks given the same image share its files.
    let medium_of = |t: &locallibrary::Track| {
        paths::to_fs(
            &root,
            &locallibrary::artwork::medium_variant(&paths::key(t.artwork_path.as_ref().expect("art set"))),
        )
    };
    for t in &lib.tracks {
        let keep = tc.draw(gs::booleans());
        if !keep && medium_of(t).is_file() {
            std::fs::remove_file(medium_of(t)).unwrap();
        }
    }
    let medium: Vec<bool> = lib.tracks.iter().map(|t| medium_of(t).is_file()).collect();
    let cache = LibraryCache::new(1);
    let many = ArtworkRequest::Many { library: path(&root).into(), tracks: ids.clone() };
    let OneOrMany::Many(files) = api::artwork_path(&cache, &many).unwrap() else { panic!("one for many") };
    for ((t, f), has_medium) in lib.tracks.iter().zip(&files).zip(&medium) {
        let file = f.image.as_ref().expect("a file");
        let key = paths::key(t.artwork_path.as_ref().unwrap());
        let want = match has_medium {
            true => locallibrary::artwork::medium_variant(&key),
            false => key,
        };
        assert_eq!(file.path, dunce_canonicalize(&paths::to_fs(&root, &want)));
        assert!(file.path.starts_with(&root) && file.mime == "image/jpeg", "{file:?}");
        let one = ArtworkRequest::One { library: path(&root).into(), track: t.id };
        let OneOrMany::One(alone) = api::artwork_path(&cache, &one).unwrap() else { panic!("many for one") };
        assert_eq!(alone.map(|a| a.path), Some(file.path.clone()));
        // The preview a webview shows inline is that file's bytes.
        let OneOrMany::One(Some(inline)) = api::artwork_get_cached(&cache, &one).unwrap() else { panic!("no art") };
        assert_eq!(inline.mime, "image/jpeg");
        assert_eq!(BASE64_STANDARD.decode(&inline.base64).unwrap(), std::fs::read(&file.path).unwrap());
    }
    let unknown = ids.iter().max().unwrap() + 1;
    let asked: Vec<u32> = ids.iter().rev().copied().chain([unknown]).collect();
    let got = api::tracks_get(&cache, &TracksGet { library: path(&root).into(), ids: asked.clone() }).unwrap();
    assert_eq!(got.tracks.iter().map(|t| t.id).collect::<Vec<_>>(), asked[..ids.len()]);
    assert_eq!(got.missing, [unknown]);
    // Analysis with each waveform chosen.
    for waveform in [WaveformChoice::None, WaveformChoice::Preview, WaveformChoice::Detail, WaveformChoice::All] {
        let req =
            AnalysisRequest { library: path(&root).into(), track: ids[0], waveform, encoding: WaveformEncoding::Json };
        let view = api::track_analysis_cached(&cache, &req).unwrap();
        let all = api::track_analysis(&api::TrackRequest { drive: path(&root), track: ids[0] }).unwrap();
        let wants = |w: WaveformChoice| waveform == WaveformChoice::All || waveform == w;
        assert_eq!(view.preview.is_some(), wants(WaveformChoice::Preview) && all.preview.is_some(), "{waveform:?}");
        assert_eq!(view.detail.is_some(), wants(WaveformChoice::Detail) && all.detail.is_some(), "{waveform:?}");
    }
    // Several at once: in the order asked, with the ids the library lacks apart; refused past
    // the most at a time, or with scrolling waveforms.
    let several = |ids: Vec<u32>, waveform: WaveformChoice| {
        let req =
            api::TracksAnalysisRequest { library: path(&root).into(), ids, waveform, encoding: WaveformEncoding::Json };
        api::tracks_analysis(&cache, &req)
    };
    let analysed = several(asked.clone(), WaveformChoice::Preview).unwrap();
    assert_eq!(analysed.tracks.iter().map(|t| t.track).collect::<Vec<_>>(), asked[..ids.len()]);
    assert_eq!(analysed.missing, [unknown]);
    let over = api::MAX_ANALYSES + tc.draw(gs::integers::<usize>().min_value(1).max_value(50));
    let refused = several(vec![ids[0]; over], WaveformChoice::None).map(|a| a.tracks.len());
    let most = format!("at most {} tracks at a time, not {over}", api::MAX_ANALYSES);
    assert!(matches!(&refused, Err(Error::Invalid(m)) if *m == most), "{refused:?}");
    let scrolling = tc.draw(gs::sampled_from(vec![WaveformChoice::Detail, WaveformChoice::All]).print_as_debug());
    assert!(matches!(several(ids.clone(), scrolling), Err(Error::Invalid(_))));
    let analysis = api::track_analysis(&api::TrackRequest { drive: path(&root), track: unknown });
    assert!(matches!(&analysis, Err(Error::NotFound(m)) if *m == format!("track #{unknown}")), "{analysis:?}");
    // Art embedded in files here: never one on an export, a folder or a file that isn't there.
    let audio = paths::to_fs(&root, &paths::key(&lib.tracks[0].file_path));
    let folder = dir.path().join("folder");
    std::fs::create_dir(&folder).unwrap();
    let here = dir.path().join("here.wav");
    std::fs::write(&here, media::wav()).unwrap();
    let mut files = vec![audio, folder, dir.path().join("gone.wav")];
    let at = tc.draw(gs::integers::<usize>().max_value(files.len()));
    files.insert(at, here.clone());
    let plan =
        api::artwork_embed_plan(&api::EmbedArtwork { files: files.clone(), image: dir.path().join("cover.png") })
            .unwrap();
    assert_eq!(plan.files.iter().map(|f| &f.path).collect::<Vec<_>>(), [&here]);
    let why: Vec<&str> = plan.skipped.iter().map(|s| s.reason.as_str()).collect();
    assert_eq!(why[..2], ["file is on a rekordbox export; set the drive's artwork instead", "not a file"]);
    assert_eq!(why.len(), 3);
    // Applied, the one file here takes the art, and the ones skipped come back as failures, why
    // included, after the art went in.
    let store = locallibrary::Store::open_or_init(dir.path().join("store")).unwrap();
    let embedded = plan.apply(Some(&store), &Ctx::silent()).unwrap();
    assert_eq!(embedded.embedded, std::slice::from_ref(&here));
    let failed: Vec<(String, String)> = embedded.failed.iter().map(|f| (f.path.clone(), f.error.clone())).collect();
    let skipped: Vec<(String, String)> = plan.skipped.iter().map(|s| (s.path.clone(), s.reason.clone())).collect();
    assert_eq!(failed, skipped);
    assert_ne!(std::fs::read(&here).unwrap(), media::wav(), "the art went in");
    // Cached, but not past a change: a track exported since is found.
    let fresh = common::export_new_track(&root, "Artist", "Since");
    let got = api::tracks_get(&cache, &TracksGet { library: path(&root).into(), ids: vec![fresh] }).unwrap();
    assert_eq!((got.tracks.len(), got.missing.len()), (1, 0));
}
