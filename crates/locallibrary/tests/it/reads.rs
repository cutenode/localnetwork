//! Reading a library a piece at a time, and the cache that keeps a parsed library between
//! requests.

use crate::common;

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use locallibrary::api::{
    self, AnalysisRequest, ArtworkRequest, DriveRef, LibraryCache, LibraryRequest, OneOrMany, TracksGet, TracksQuery,
    WaveformChoice, WaveformEncoding,
};
use locallibrary::artwork::{self, Artwork};
use locallibrary::device::{Device, dunce_canonicalize};
use locallibrary::progress::Ctx;
use serde_json::{Value, json};

fn path(p: &Path) -> String {
    p.display().to_string()
}

fn query(root: &Path) -> TracksQuery {
    TracksQuery { drive: path(root), ..Default::default() }
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
    let whole = api::drive_library_cached(&cache, &LibraryRequest { drive: path(&root), offset: None, limit: None });
    let old = api::drive_library(&DriveRef { drive: path(&root) }).unwrap();
    assert_eq!(serde_json::to_value(whole.unwrap()).unwrap(), serde_json::to_value(old).unwrap());
    // A page of tracks; the rest of the library as it is.
    let req = LibraryRequest { drive: path(&root), offset: Some(2), limit: Some(3) };
    let page = api::drive_library_cached(&cache, &req).unwrap();
    let p = page.page.unwrap();
    assert_eq!((p.offset, p.count, p.total), (2, 3, lib.tracks.len()));
    assert_eq!(page.library.tracks, lib.tracks[2..5]);
    assert_eq!(page.library.playlists, lib.playlists);
    assert_eq!(page.summary, lib.summary());

    // The tree: every playlist once, with its entries counted.
    let tree = api::playlists_tree(&cache, &DriveRef { drive: path(&root) }).unwrap();
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
    let got = api::tracks_get(&cache, &TracksGet { drive: path(&root), ids }).unwrap();
    assert_eq!(got.tracks, [lib.tracks[3].clone(), lib.tracks[0].clone()]);
    assert_eq!(got.missing, [999_999]);
}

fn png() -> Vec<u8> {
    let img = image::RgbImage::from_fn(300, 300, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 40]));
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

#[test]
fn artwork_as_files_and_in_batches() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    let mut d = Device::open_export(&root).unwrap();
    artwork::set_on_drive(&mut d, &[1], &Artwork::from_bytes(&png()).unwrap(), &Ctx::silent()).unwrap();
    let cache = LibraryCache::new(1);
    let one = |track| ArtworkRequest::One { drive: path(&root), track };

    // A file inside the drive, the medium size players show.
    let OneOrMany::One(Some(file)) = api::artwork_path(&cache, &one(1)).unwrap() else { panic!("no artwork") };
    assert!(file.path.is_absolute() && file.path.starts_with(dunce_canonicalize(&root)), "{:?}", file.path);
    assert!(file.path.to_string_lossy().contains("_m."), "{:?}", file.path);
    assert_eq!(image::open(&file.path).unwrap().width(), 240);
    assert!(matches!(api::artwork_path(&cache, &one(2)).unwrap(), OneOrMany::One(None)));
    assert!(api::artwork_path(&cache, &one(999)).is_err());

    // Several at once, in the order asked; the old shape for one.
    let many = ArtworkRequest::Many { drive: path(&root), tracks: vec![2, 1] };
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
        std::fs::write(&outside, png()).unwrap();
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
    let req = |waveform, encoding| AnalysisRequest { drive: path(&root), track, waveform, encoding };

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
