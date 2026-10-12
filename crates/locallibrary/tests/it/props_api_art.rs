//! Reading tracks by id and their artwork files once art is set: the medium size players show
//! when there is one, the full image otherwise, always inside the drive.

use crate::common::{self, media, path};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
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
/// changed from since. Analysis comes with the waveforms chosen, and art isn't embedded in a file
/// on an export, a folder or a file that isn't there.
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
    let many = ArtworkRequest::Many { drive: path(&root), tracks: ids.clone() };
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
        let one = ArtworkRequest::One { drive: path(&root), track: t.id };
        let OneOrMany::One(alone) = api::artwork_path(&cache, &one).unwrap() else { panic!("many for one") };
        assert_eq!(alone.map(|a| a.path), Some(file.path.clone()));
    }
    let unknown = ids.iter().max().unwrap() + 1;
    let asked: Vec<u32> = ids.iter().rev().copied().chain([unknown]).collect();
    let got = api::tracks_get(&cache, &TracksGet { drive: path(&root), ids: asked.clone() }).unwrap();
    assert_eq!(got.tracks.iter().map(|t| t.id).collect::<Vec<_>>(), asked[..ids.len()]);
    assert_eq!(got.missing, [unknown]);
    // Analysis with each waveform chosen.
    for waveform in [WaveformChoice::None, WaveformChoice::Preview, WaveformChoice::Detail, WaveformChoice::All] {
        let req = AnalysisRequest { drive: path(&root), track: ids[0], waveform, encoding: WaveformEncoding::Json };
        let view = api::track_analysis_cached(&cache, &req).unwrap();
        let all = api::track_analysis(&api::TrackRequest { drive: path(&root), track: ids[0] }).unwrap();
        let wants = |w: WaveformChoice| waveform == WaveformChoice::All || waveform == w;
        assert_eq!(view.preview.is_some(), wants(WaveformChoice::Preview) && all.preview.is_some(), "{waveform:?}");
        assert_eq!(view.detail.is_some(), wants(WaveformChoice::Detail) && all.detail.is_some(), "{waveform:?}");
    }
    // Art embedded in files here: never one on an export, a folder or a file that isn't there.
    let audio = paths::to_fs(&root, &paths::key(&lib.tracks[0].file_path));
    let folder = dir.path().join("folder");
    std::fs::create_dir(&folder).unwrap();
    let files = vec![audio, folder, dir.path().join("gone.wav")];
    let plan =
        api::artwork_embed_plan(&api::EmbedArtwork { files: files.clone(), image: dir.path().join("cover.png") })
            .unwrap();
    assert!(plan.files.is_empty() && plan.noop);
    let why: Vec<&str> = plan.skipped.iter().map(|s| s.reason.as_str()).collect();
    assert_eq!(why[..2], ["file is on a rekordbox export; set the drive's artwork instead", "not a file"]);
    assert_eq!(why.len(), 3);
    // Cached, but not past a change: a track exported since is found.
    let fresh = common::export_new_track(&root, "Artist", "Since");
    let got = api::tracks_get(&cache, &TracksGet { drive: path(&root), ids: vec![fresh] }).unwrap();
    assert_eq!((got.tracks.len(), got.missing.len()), (1, 0));
}
