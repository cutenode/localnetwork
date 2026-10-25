//! Reading a library a piece at a time, and the cache that keeps a parsed library between
//! requests.

use crate::common::{self, path};

use std::path::Path;
use std::sync::Arc;

use locallibrary::api::{
    self, AnalysisRequest, ArtworkRequest, DriveRef, LibraryCache, LibraryRef, LibraryRequest, MatchesRequest,
    OneOrMany, PluginFieldsRequest, TracksAnalysisRequest, TracksGet, TracksQuery, WaveformChoice, WaveformEncoding,
};
use locallibrary::device::{Device, dunce_canonicalize};
use locallibrary::fields::{Analyzer, FieldStore, FieldValue, Fields};
use locallibrary::store::Store;
use locallibrary::{Error, Track, paths};
use serde_json::{Value, json};

fn query(root: &Path) -> TracksQuery {
    TracksQuery { library: path(root).into(), ..Default::default() }
}

#[test]
fn a_library_is_kept_until_its_databases_change() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b, c) = (
        common::demo_drive(dir.path(), "A"),
        common::bigger_drive(dir.path(), "B"),
        common::demo_drive(dir.path(), "C"),
    );
    let cache = LibraryCache::new(2);
    let get = |root: &Path| cache.get(&Device::open(root).unwrap()).unwrap();
    let first = get(&a);
    assert!(Arc::ptr_eq(&first, &get(&a)), "read once");
    assert_eq!(api::tracks_query_cached(&cache, &query(&a)).unwrap().total, 2);

    // rekordbox exports another track: read again, without being told.
    common::export_new_track(&a, "Artist", "Fresh");
    let again = get(&a);
    assert!(!Arc::ptr_eq(&first, &again));
    assert_eq!(again.library.tracks.len(), 3);
    assert_eq!(api::tracks_query_cached(&cache, &query(&a)).unwrap().total, 3);

    // Told: read again too.
    cache.invalidate(&again.root);
    assert!(!Arc::ptr_eq(&again, &get(&a)));

    // The drives read last are kept, the least recently used one goes.
    get(&b);
    get(&a);
    get(&c);
    assert_eq!(cache.roots(), [dunce_canonicalize(&a), dunce_canonicalize(&c)]);
    cache.clear();
    assert!(cache.roots().is_empty());

    // Keeping nothing still answers.
    let none = LibraryCache::none();
    assert!(!Arc::ptr_eq(&none.get(&Device::open(&a).unwrap()).unwrap(), &get(&a)));
    assert!(none.roots().is_empty());
}

#[test]
fn the_library_in_pages_trees_and_by_id() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let cache = LibraryCache::new(1);
    let lib = Device::open(&root).unwrap().library().unwrap();
    assert!(lib.tracks.len() > 5 && !lib.playlists.is_empty());

    // Unpaged: as before.
    let whole =
        api::drive_library_cached(&cache, &LibraryRequest { library: path(&root).into(), offset: None, limit: None });
    let old = api::drive_library(&DriveRef { drive: path(&root) }).unwrap();
    assert_eq!(serde_json::to_value(whole.unwrap()).unwrap(), serde_json::to_value(old).unwrap());
    // A page of tracks; the rest of the library as it is.
    let req = LibraryRequest { library: path(&root).into(), offset: Some(2), limit: Some(3) };
    let page = api::drive_library_cached(&cache, &req).unwrap();
    let p = page.page.unwrap();
    assert_eq!((p.offset, p.count, p.total), (2, 3, lib.tracks.len()));
    assert_eq!(page.library.tracks, lib.tracks[2..5]);
    assert_eq!(page.library.playlists, lib.playlists);
    assert_eq!(page.summary, lib.summary());

    // The tree: every playlist once, with its entries counted.
    let tree = api::playlists_tree(&cache, &LibraryRef { drive: path(&root), combined: false }).unwrap();
    fn walk(nodes: &[api::PlaylistNode], out: &mut Vec<(u32, String, usize)>) {
        for n in nodes {
            out.push((n.id, n.path.clone(), n.tracks));
            walk(&n.children, out);
        }
    }
    let mut seen = Vec::new();
    walk(&tree.playlists, &mut seen);
    assert_eq!(seen.len(), lib.playlists.len());
    for p in &lib.playlists {
        let found = seen.iter().find(|(id, _, _)| *id == p.id).unwrap();
        assert_eq!((found.1.as_str(), found.2), (lib.playlist_path(p.id).as_str(), p.track_ids.len()));
    }
    let json = serde_json::to_value(&tree).unwrap();
    assert!(json["playlists"][0]["duration_secs"].is_u64(), "{json}");

    // Tracks by id, in the order asked, and the ones that aren't there.
    let ids = vec![lib.tracks[3].id, 999_999, lib.tracks[0].id];
    let got = api::tracks_get(&cache, &TracksGet { library: path(&root).into(), ids }).unwrap();
    assert_eq!(got.tracks, [lib.tracks[3].clone(), lib.tracks[0].clone()]);
    assert_eq!(got.missing, [999_999]);
}

#[test]
fn artwork_as_files_and_in_batches() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    common::media::set_art(&root, &[1]);
    let cache = LibraryCache::new(1);
    let one = |track| ArtworkRequest::One { library: path(&root).into(), track };

    // A file inside the drive, the medium size players show.
    let OneOrMany::One(Some(file)) = api::artwork_path(&cache, &one(1)).unwrap() else { panic!("no artwork") };
    assert!(file.path.is_absolute() && file.path.starts_with(dunce_canonicalize(&root)), "{:?}", file.path);
    assert!(file.path.to_string_lossy().contains("_m."), "{:?}", file.path);
    assert_eq!(image::open(&file.path).unwrap().width(), 240);
    assert!(matches!(api::artwork_path(&cache, &one(2)).unwrap(), OneOrMany::One(None)));
    assert!(api::artwork_path(&cache, &one(999)).is_err());

    // Several at once, in the order asked; the old shape for one.
    let many = ArtworkRequest::Many { library: path(&root).into(), tracks: vec![2, 1] };
    let got = serde_json::to_value(api::artwork_get_cached(&cache, &many).unwrap()).unwrap();
    assert_eq!(got[0], json!({ "track": 2, "image": null }));
    assert_eq!((&got[1]["track"], &got[1]["image"]["mime"]), (&json!(1), &json!("image/jpeg")));
    let old = api::artwork_get(&api::GetArtwork { drive: path(&root), track: 1 }).unwrap();
    let new = api::artwork_get_cached(&cache, &one(1)).unwrap();
    assert_eq!(serde_json::to_value(new).unwrap(), serde_json::to_value(old).unwrap());
    let parsed: ArtworkRequest = serde_json::from_value(json!({ "drive": "A", "tracks": [1, 2] })).unwrap();
    assert!(matches!(parsed, ArtworkRequest::Many { ref tracks, .. } if tracks == &[1, 2]));

    // A file that leads out of the drive through a symlink is never given.
    #[cfg(unix)]
    {
        let outside = dir.path().join("elsewhere.jpg");
        std::fs::write(&outside, common::media::png(300, 300, 40)).unwrap();
        let OneOrMany::One(Some(file)) = api::artwork_path(&cache, &one(1)).unwrap() else { unreachable!() };
        std::fs::remove_file(&file.path).unwrap();
        std::os::unix::fs::symlink(&outside, &file.path).unwrap();
        let after = api::artwork_path(&cache, &one(1)).unwrap();
        let OneOrMany::One(found) = after else { unreachable!() };
        assert!(found.as_ref().is_none_or(|f| f.path.starts_with(dunce_canonicalize(&root))), "{found:?}");
    }
}

fn decode(s: &str) -> Vec<u8> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|c| *c != b'=') {
        acc = acc << 6 | T.iter().position(|t| *t == c).unwrap() as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

#[test]
fn analysis_with_the_waveforms_asked_for() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    let cache = LibraryCache::new(1);
    let track = Device::open(&root).unwrap().library().unwrap().tracks[0].id;
    let req = |waveform, encoding| AnalysisRequest { library: path(&root).into(), track, waveform, encoding };

    // The defaults give what `track.analysis` always has.
    let old = api::track_analysis(&api::TrackRequest { drive: path(&root), track }).unwrap();
    let all = api::track_analysis_cached(&cache, &req(WaveformChoice::All, WaveformEncoding::Json)).unwrap();
    let old_json = serde_json::to_value(&old).unwrap();
    assert_eq!(serde_json::to_value(&all).unwrap(), old_json);
    let parsed: AnalysisRequest = serde_json::from_value(json!({ "drive": "A", "track": 1 })).unwrap();
    assert_eq!((parsed.waveform, parsed.encoding), (WaveformChoice::All, WaveformEncoding::Json));

    // None: the grid, cues and phrases only.
    let none = serde_json::to_value(
        api::track_analysis_cached(&cache, &req(WaveformChoice::None, WaveformEncoding::Json)).unwrap(),
    )
    .unwrap();
    assert!(none["preview"].is_null() && none["detail"].is_null());
    assert_eq!((&none["beats"], &none["structure"]), (&old_json["beats"], &old_json["structure"]));

    // The scrolling waveform, packed: a few bytes a column, the same heights and colours.
    let packed = api::track_analysis_cached(&cache, &req(WaveformChoice::Detail, WaveformEncoding::Base64)).unwrap();
    let packed = serde_json::to_value(&packed).unwrap();
    assert!(packed["preview"].is_null());
    let d: &Value = &packed["detail"];
    let detail = old.detail.as_ref().unwrap();
    assert_eq!((&d["encoding"], d["columns"].as_u64()), (&json!("base64"), Some(detail.columns.len() as u64)));
    assert_eq!(d["columns_per_sec"], json!(150.0));
    assert!(d["front"].is_null(), "scrolling waveforms have no front layer");
    let heights = decode(d["back"]["heights"].as_str().unwrap());
    let colors = decode(d["back"]["colors"].as_str().unwrap());
    assert_eq!((heights.len(), colors.len()), (detail.columns.len(), 3 * detail.columns.len()));
    for (i, c) in detail.columns.iter().enumerate().step_by(97) {
        assert_eq!(heights[i], (c.back.height * 255.0).round() as u8);
        assert_eq!(colors[3 * i..3 * i + 3], c.back.color);
    }
    let size = |v: &Value| serde_json::to_vec(v).unwrap().len();
    assert!(size(d) * 10 < size(&old_json["detail"]), "{} vs {} bytes", size(d), size(&old_json["detail"]));

    // The overview has a front layer.
    let preview = api::track_analysis_cached(&cache, &req(WaveformChoice::Preview, WaveformEncoding::Base64)).unwrap();
    let preview = serde_json::to_value(&preview).unwrap();
    assert!(preview["detail"].is_null());
    assert_eq!(decode(preview["preview"]["front"]["heights"].as_str().unwrap()).len(), 1200);
}

/// A drive's My Tags are read once and kept, both as listed and by track for `tag:` filters,
/// and cues are listed for the tracks chosen.
#[test]
fn my_tags_are_read_once_and_cues_listed_from_the_cache() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let cache = LibraryCache::new(1);
    let drive = DriveRef { drive: path(&root) };

    let first = api::tags_list_cached(&cache, &drive).unwrap();
    assert_eq!(first.categories.len(), 4);
    assert!(Arc::ptr_eq(&first, &api::tags_list_cached(&cache, &drive).unwrap()), "kept, not read again");
    let tagged =
        api::TracksQuery { library: path(&root).into(), filter: Some("tag:Techno".into()), ..Default::default() };
    for _ in 0..2 {
        assert_eq!(api::tracks_query_cached(&cache, &tagged).unwrap().total, 0, "no track has the tag");
    }

    let first_track = api::TrackSelector { ids: vec![1], ..Default::default() };
    let cues =
        api::cues_list_cached(&cache, &api::ListCues { library: path(&root).into(), tracks: first_track }).unwrap();
    assert_eq!(cues.iter().map(|c| c.track.id).collect::<Vec<_>>(), [1]);
}

/// Tracks are queried in a playlist's order or sorted, and matched by key and tempo; a track or
/// playlist the drive doesn't have is refused.
#[test]
fn tracks_are_queried_and_matched() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Club");
    let lib = Device::open(&root).unwrap().library().unwrap();
    let playlist = lib.playlists.iter().find(|p| !p.is_folder).unwrap();
    let query = |playlist: Option<&str>, sort: Option<&str>| api::TracksQuery {
        library: path(&root).into(),
        playlist: playlist.map(Into::into),
        sort: sort.map(Into::into),
        ..Default::default()
    };

    let page = api::tracks_query(&query(Some(&playlist.name), None)).unwrap();
    assert_eq!(page.tracks.iter().map(|t| t.id).collect::<Vec<_>>(), playlist.track_ids);
    let sorted = api::tracks_query(&query(Some(&playlist.name), Some("-title"))).unwrap();
    let mut titles: Vec<String> = page.tracks.iter().map(|t| t.title.clone()).collect();
    titles.sort_by(|a, b| b.cmp(a));
    assert_eq!(sorted.tracks.iter().map(|t| t.title.clone()).collect::<Vec<_>>(), titles);
    assert!(matches!(api::tracks_query(&query(Some("Nope"), None)), Err(Error::NotFound(_))));

    let matches =
        |track| api::harmonic_matches(&api::MatchesRequest { library: path(&root).into(), track, limit: Some(3) });
    assert!(matches!(matches(999), Err(Error::NotFound(m)) if m == "track #999"));
    let found = matches(lib.tracks[0].id).unwrap();
    assert!(found.len() <= 3);
    assert!(found.iter().all(|m| m.track.id != lib.tracks[0].id && !m.key.is_empty()), "{found:?}");
}

/// A drive whose OneLibrary has a track `export.pdb` lacks, in a playlist only OneLibrary has,
/// as a CDJ-3000X or a OneLibrary-only export leaves it. Returns OneLibrary's id for the track.
fn drive_with_a_onelibrary_only_track(dir: &Path) -> (std::path::PathBuf, u32) {
    let root = common::demo_drive(dir, "A");
    let mut lib = Device::open(&root).unwrap().library().unwrap();
    let only = Track {
        id: lib.tracks.iter().map(|t| t.id).max().unwrap() + 1,
        title: "Only Plus".into(),
        file_path: "/Contents/Booth/Only Plus.mp3".into(),
        ..lib.tracks[0].clone()
    };
    lib.tracks.push(only);
    common::populate_library(&root, &lib);
    let mut one = common::onelibrary_for(&lib);
    let id = one.library().unwrap().tracks.iter().find(|t| t.title == "Only Plus").unwrap().id;
    let booth = one.create_playlist("Booth", 0, false).unwrap();
    one.add_to_playlist(booth, id).unwrap();
    std::fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    (root, id)
}

fn whole(root: &Path) -> LibraryRef {
    LibraryRef { drive: path(root), combined: true }
}

/// OneLibrary's own tracks keep OneLibrary's id with the top bit set, the same on every read and
/// after rekordbox exports again, so it never names an `export.pdb` track; reads without
/// `combined` and edits refuse it, saying why.
#[test]
fn onelibrary_only_tracks_have_ids_of_their_own() {
    let dir = tempfile::tempdir().unwrap();
    let (root, plus_id) = drive_with_a_onelibrary_only_track(dir.path());
    let read = || locallibrary::convert::combined_library(&Device::open(&root).unwrap()).unwrap();
    let id = read().tracks.iter().find(|t| t.title == "Only Plus").unwrap().id;
    assert_eq!(id, plus_id | locallibrary::convert::ONELIBRARY_ONLY);
    assert!(locallibrary::convert::is_onelibrary_only(id) && !locallibrary::convert::is_onelibrary_only(plus_id));
    let fresh = common::export_new_track(&root, "Artist", "Fresh");
    let again = read();
    assert_eq!(again.tracks.iter().find(|t| t.title == "Only Plus").unwrap().id, id);
    assert_eq!(again.tracks.iter().find(|t| t.title == "Fresh").unwrap().id, fresh);

    let cache = LibraryCache::new(1);
    let players = MatchesRequest { library: path(&root).into(), track: id, limit: None };
    let Err(Error::NotFound(m)) = api::harmonic_matches_cached(&cache, &players) else { panic!("found") };
    assert_eq!(
        m,
        format!("track #{id} is one only OneLibrary has: reads see it with `combined`, and edits can't change it")
    );
    let edit: api::EditRequest = serde_json::from_value(json!({
        "drive": path(&root),
        "edits": [{ "op": "edit_tracks", "tracks": { "ids": [id] }, "fields": { "title": "Renamed" } }],
    }))
    .unwrap();
    let refused = api::edit_plan(&edit, &locallibrary::progress::Ctx::silent());
    assert!(matches!(refused, Err(Error::NotFound(ref m)) if m.contains("only OneLibrary has")), "{refused:?}");
}

/// With `combined`, every read that takes a track id sees the whole library exports are made
/// from, OneLibrary's own tracks and playlists included; without it, the library players see,
/// as before. A drive with one database reads the same either way.
#[test]
fn the_whole_library_has_what_only_onelibrary_holds() {
    let dir = tempfile::tempdir().unwrap();
    let (root, _) = drive_with_a_onelibrary_only_track(dir.path());
    let d = Device::open(&root).unwrap();
    let (players, whole_lib) = (d.library().unwrap(), locallibrary::convert::combined_library(&d).unwrap());
    let only = whole_lib.tracks.iter().find(|t| t.title == "Only Plus").unwrap().clone();
    assert!(players.track(only.id).is_none() && whole_lib.tracks.len() == players.tracks.len() + 1);
    let cache = LibraryCache::new(1);
    let library = |combined| LibraryRef { drive: path(&root), combined };

    let all = |combined| {
        let req = LibraryRequest { library: library(combined), offset: None, limit: None };
        api::drive_library_cached(&cache, &req).unwrap().library
    };
    assert_eq!(all(false), players);
    assert_eq!(all(true), whole_lib);
    let (d, cached, _) = cache.read(&whole(&root)).unwrap();
    assert!(Arc::ptr_eq(&cached.view(&d, true).unwrap(), &cached.view(&d, true).unwrap()), "read once, then kept");
    let req = LibraryRequest { library: whole(&root), offset: Some(1), limit: Some(5) };
    let page = api::drive_library_cached(&cache, &req).unwrap();
    assert_eq!(
        (page.library.tracks, page.page.unwrap().total),
        (whole_lib.tracks[1..].to_vec(), whole_lib.tracks.len())
    );

    let total = |combined| {
        let req = TracksQuery { library: library(combined), ..Default::default() };
        api::tracks_query_cached(&cache, &req).unwrap().total
    };
    assert_eq!((total(false), total(true)), (players.tracks.len(), whole_lib.tracks.len()));
    let names = |combined| {
        let tree = api::playlists_tree(&cache, &library(combined)).unwrap();
        tree.playlists.into_iter().map(|p| p.name).collect::<Vec<_>>()
    };
    assert!(!names(false).contains(&"Booth".into()) && names(true).contains(&"Booth".into()));
    let get = |combined| api::tracks_get(&cache, &TracksGet { library: library(combined), ids: vec![only.id] });
    assert_eq!((get(false).unwrap().missing, get(true).unwrap().tracks), (vec![only.id], vec![only.clone()]));

    // Every other read by id takes it too.
    let analysis = |library, track| {
        let (waveform, encoding) = (WaveformChoice::None, WaveformEncoding::Json);
        api::track_analysis_cached(&cache, &AnalysisRequest { library, track, waveform, encoding })
    };
    assert!(matches!(analysis(library(false), only.id), Err(Error::NotFound(_))));
    // Its analysis files are the first track's.
    let (got, first) =
        (analysis(whole(&root), only.id).unwrap(), analysis(library(false), players.tracks[0].id).unwrap());
    assert_eq!(serde_json::to_value(got).unwrap(), serde_json::to_value(first).unwrap());
    let art = ArtworkRequest::One { library: whole(&root), track: only.id };
    assert!(matches!(api::artwork_get_cached(&cache, &art).unwrap(), OneOrMany::One(_)));
    assert!(matches!(api::artwork_path(&cache, &art).unwrap(), OneOrMany::One(_)));
    let selected = api::TrackSelector { ids: vec![only.id], ..Default::default() };
    let shown = api::TrackDetailsRequest { library: whole(&root), tracks: selected.clone() };
    let shown = api::track_details_cached(&cache, &shown).unwrap();
    assert_eq!((shown[0].track.id, shown[0].playlists.clone()), (only.id, vec!["Booth".to_string()]));
    let matches = MatchesRequest { library: whole(&root), track: only.id, limit: None };
    assert!(api::harmonic_matches_cached(&cache, &matches).is_ok());
    let cues = api::cues_list_cached(&cache, &api::ListCues { library: whole(&root), tracks: selected }).unwrap();
    assert_eq!(cues[0].track.id, only.id);

    let plain = common::demo_drive(dir.path(), "B");
    let req =
        |combined| LibraryRequest { library: LibraryRef { drive: path(&plain), combined }, offset: None, limit: None };
    let (a, b) = (api::drive_library_cached(&cache, &req(false)), api::drive_library_cached(&cache, &req(true)));
    assert_eq!(a.unwrap().library, b.unwrap().library);
}

/// Several tracks' analyses at once are each what asking for it alone gives, without waveforms
/// unless asked, with the ids the library lacks listed; more than the cap, and scrolling
/// waveforms, are refused.
#[test]
fn several_tracks_analysis_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    let cache = LibraryCache::new(1);
    let ids: Vec<u32> = Device::open(&root).unwrap().library().unwrap().tracks.iter().map(|t| t.id).collect();
    let asked: Vec<u32> = ids.iter().rev().copied().chain([999_999]).collect();
    let parsed: TracksAnalysisRequest = serde_json::from_value(json!({ "drive": path(&root), "ids": asked })).unwrap();
    assert_eq!((parsed.waveform, parsed.encoding), (WaveformChoice::None, WaveformEncoding::Json));

    for (waveform, encoding) in
        [(WaveformChoice::None, WaveformEncoding::Json), (WaveformChoice::Preview, WaveformEncoding::Base64)]
    {
        let req = TracksAnalysisRequest { waveform, encoding, ..parsed.clone() };
        let got = api::tracks_analysis(&cache, &req).unwrap();
        assert_eq!(got.missing, [999_999]);
        assert_eq!(got.tracks.iter().map(|t| t.track).collect::<Vec<_>>(), asked[..ids.len()]);
        for t in &got.tracks {
            let one = AnalysisRequest { library: path(&root).into(), track: t.track, waveform, encoding };
            let alone = api::track_analysis_cached(&cache, &one).unwrap();
            assert_eq!(serde_json::to_value(&t.analysis).unwrap(), serde_json::to_value(alone).unwrap());
        }
    }
    for waveform in [WaveformChoice::Detail, WaveformChoice::All] {
        let refused = api::tracks_analysis(&cache, &TracksAnalysisRequest { waveform, ..parsed.clone() });
        assert!(matches!(refused, Err(Error::Invalid(m)) if m.contains("one track at a time")));
    }

    let many = |n| TracksAnalysisRequest { ids: vec![ids[0]; n], ..parsed.clone() };
    assert_eq!(api::tracks_analysis(&cache, &many(api::MAX_ANALYSES)).unwrap().tracks.len(), api::MAX_ANALYSES);
    let refused = api::tracks_analysis(&cache, &many(api::MAX_ANALYSES + 1));
    assert!(matches!(refused, Err(Error::Invalid(m)) if m == "at most 100 tracks at a time, not 101"));
}

/// The fields plugins' analyzers kept for a drive's tracks, for the tracks and plugin asked,
/// with the ids the library lacks listed; no fields without a store.
#[test]
fn plugin_fields_are_read_for_a_drives_tracks() {
    let dir = tempfile::tempdir().unwrap();
    let (root, _) = drive_with_a_onelibrary_only_track(dir.path());
    let d = Device::open(&root).unwrap();
    let whole_lib = locallibrary::convert::combined_library(&d).unwrap();
    let audio = |t: &Track| d.checked_path(&paths::key(&t.file_path)).unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let cache = LibraryCache::new(1);
    let req = |tracks: Option<Vec<u32>>, plugin: Option<&str>, combined| PluginFieldsRequest {
        library: LibraryRef { drive: path(&root), combined },
        tracks,
        plugin: plugin.map(Into::into),
    };
    let analyzer = |plugin: &str, analyzer: &str| Analyzer {
        plugin: plugin.into(),
        version: "0.1.0".into(),
        analyzer: analyzer.into(),
    };
    let installed = [analyzer("analysis", "tempo"), analyzer("analysis", "key"), analyzer("keys", "key")];
    let read =
        |store, req| serde_json::to_value(api::track_fields_of(&cache, store, &req, &installed).unwrap()).unwrap();
    let none = json!({ "tracks": [], "missing": [] });
    assert_eq!(read(Some(&store), req(None, None, false)), none, "nothing kept yet");

    let fields = FieldStore::in_store(&store);
    let bpm: Fields = [("bpm".to_string(), FieldValue::Number(128.0))].into_iter().collect();
    let key: Fields = [("key".to_string(), FieldValue::String("8A".into()))].into_iter().collect();
    let (first, only) = (&whole_lib.tracks[0], whole_lib.tracks.iter().find(|t| t.title == "Only Plus").unwrap());
    fields.put(&audio(first), &analyzer("analysis", "tempo"), &bpm).unwrap();
    fields.put(&audio(first), &analyzer("analysis", "key"), &key).unwrap();
    fields.put(&audio(first), &analyzer("keys", "key"), &key).unwrap();
    fields.put(&audio(only), &analyzer("analysis", "tempo"), &bpm).unwrap();

    let both = json!({ "analysis": { "key": { "key": "8A" }, "tempo": { "bpm": 128.0 } }, "keys": { "key": { "key": "8A" } } });
    let kept = json!({ "tracks": [{ "track": first.id, "plugins": both }], "missing": [] });
    assert_eq!(read(Some(&store), req(None, None, false)), kept);
    assert_eq!(read(None, req(Some(vec![999]), None, false)), json!({ "tracks": [], "missing": [999] }));
    let keys = json!({ "tracks": [{ "track": first.id, "plugins": { "keys": { "key": { "key": "8A" } } } }], "missing": [999] });
    assert_eq!(read(Some(&store), req(Some(vec![999, first.id]), Some("keys"), false)), keys);
    assert_eq!(read(Some(&store), req(Some(vec![whole_lib.tracks[1].id]), None, false)), none);
    let combined = read(Some(&store), req(Some(vec![only.id]), Some("analysis"), true));
    let tempo = json!({ "analysis": { "tempo": { "bpm": 128.0 } } });
    assert_eq!(combined, json!({ "tracks": [{ "track": only.id, "plugins": tempo }], "missing": [] }));
}

/// Exports for other software never carry an id with the reserved bit: OneLibrary's own tracks
/// are numbered on from `export.pdb`'s, and the XML's playlists name them by those numbers.
#[test]
fn exports_number_onelibrary_only_tracks_in_range() {
    let dir = tempfile::tempdir().unwrap();
    let (root, _) = drive_with_a_onelibrary_only_track(dir.path());
    let d = Device::open(&root).unwrap();
    let players = d.library().unwrap();
    let exported = locallibrary::convert::export_library(&d).unwrap();
    let only = exported.tracks.iter().find(|t| t.title == "Only Plus").unwrap();
    assert_eq!(only.id, players.tracks.iter().map(|t| t.id).max().unwrap() + 1);
    let booth = exported.playlists.iter().find(|p| p.name == "Booth").unwrap();
    assert_eq!(booth.track_ids, [only.id]);

    let opts = common::XmlOptions { skip_analysis: true, ..Default::default() };
    let (xml, _) = common::rekordbox_xml(&d, &opts).unwrap();
    let numbers = |attr: &str| -> Vec<u64> {
        xml.split(attr).skip(1).map(|rest| rest.split('"').next().unwrap().parse().unwrap()).collect()
    };
    let (ids, keys) = (numbers("TrackID=\""), numbers("Key=\""));
    assert_eq!(ids.len(), exported.tracks.len());
    assert!(ids.iter().all(|id| *id < 1 << 31), "{ids:?}");
    assert!(keys.iter().all(|k| ids.contains(k)) && keys.contains(&u64::from(only.id)), "{keys:?}");
}
