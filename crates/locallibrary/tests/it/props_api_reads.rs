//! Properties of the service layer's reads (`api::reads`, `api::cache` and the read-only calls
//! beside them): each answer agrees with the drive's library read directly, pages and trees
//! account for every track and playlist once, and the cache of parsed libraries keeps the drives
//! read last until their databases change.

use crate::common::{self, path};
use crate::drawn;

use rustc_hash::FxHashMap;
use std::path::PathBuf;
use std::sync::Arc;

use base64::prelude::*;
use hegel::generators as gs;
use hegel::{Generator, HealthCheck, TestCase};
use locallibrary::anlz::{WaveColumn, WaveLayer, Waveform, WaveformStyle};
use locallibrary::api::{
    self, AnalysisRequest, ArtworkRequest, DriveRef, LibraryCache, LibraryRef, LibraryRequest, ListCues,
    MatchesRequest, OneOrMany, PackedWaveform, PlaylistNode, TrackDetailsRequest, TrackRequest, TracksGet,
    WaveformChoice, WaveformData, WaveformEncoding,
};
use locallibrary::device::{Device, dunce_canonicalize};
use locallibrary::query::TrackSelector;
use locallibrary::{Error, Library, Playlist, harmony, paths};

/// A drive to read, in a folder of its own.
struct Drive {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Drive {
    fn spec(&self) -> String {
        path(&self.root)
    }

    fn library(&self) -> Library {
        common::library(&self.root)
    }
}

/// [`drawn::varied`], in a folder of its own whose `volumes` folder (empty) is the only place
/// drives are looked for.
#[hegel::test_helper]
fn drive(tc: &TestCase) -> Drive {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let root = drawn::varied(tc, dir.path(), "Main");
    Drive { _dir: dir, root }
}

/// Every node of a tree, depth first, with its parent's path.
fn walk<'a>(nodes: &'a [PlaylistNode], parent: &str, out: &mut Vec<(&'a PlaylistNode, String)>) {
    for n in nodes {
        out.push((n, parent.to_string()));
        walk(&n.children, &n.path, out);
    }
}

/// A tree of `lists` is every list once, each under its parent in display order, its path its
/// parent's and its name, its entries and their length counted from the library's tracks.
fn assert_tree(tree: &[PlaylistNode], lists: &[Playlist], lib: &Library) {
    let mut seen = Vec::new();
    walk(tree, "", &mut seen);
    assert_eq!(seen.len(), lists.len(), "every list once");
    let durations: FxHashMap<u32, u32> = lib.tracks.iter().map(|t| (t.id, t.duration_secs)).collect();
    for (node, parent) in &seen {
        let p = lists.iter().find(|p| p.id == node.id).expect("a list of the library");
        let expected = if parent.is_empty() { p.name.clone() } else { format!("{parent}/{}", p.name) };
        assert_eq!((&node.name, &node.path, node.folder), (&p.name, &expected, p.is_folder));
        assert_eq!(node.tracks, p.track_ids.len());
        let secs: u64 = p.track_ids.iter().filter_map(|t| durations.get(t)).map(|d| u64::from(*d)).sum();
        assert_eq!(node.duration_secs, secs);
        let mut kids: Vec<&Playlist> = lists.iter().filter(|c| c.parent_id == Some(p.id)).collect();
        kids.sort_by_key(|c| (c.sort_order, c.id));
        let expected: Vec<u32> = if p.is_folder { kids.iter().map(|c| c.id).collect() } else { Vec::new() };
        assert_eq!(node.children.iter().map(|c| c.id).collect::<Vec<_>>(), expected, "{}'s contents", node.path);
    }
}

/// The library in pages, the playlist tree, tracks by id, track details, harmonic matches,
/// analysis views and artwork, read through the cache, agree with the library read directly.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn reads_agree_with_the_library(tc: TestCase) {
    let drive = drive(&tc);
    let lib = drive.library();
    let cache = LibraryCache::new(1);
    let n = lib.tracks.len();

    // Unpaged, as `drive.library`; paged, a slice of the tracks with the rest as it is.
    let whole =
        api::drive_library_cached(&cache, &LibraryRequest { library: drive.spec().into(), offset: None, limit: None })
            .unwrap();
    assert!(whole.page.is_none());
    assert_eq!(whole.library, lib);
    assert_eq!(
        serde_json::to_value(&whole).unwrap(),
        serde_json::to_value(api::drive_library(&DriveRef { drive: drive.spec() }).unwrap()).unwrap()
    );
    // A few pages, each from the library kept in the cache.
    for _ in 0..4 {
        let offset = tc.draw(gs::optional(gs::integers::<usize>().max_value(n + 2)));
        let limit = tc.draw(gs::optional(gs::integers::<usize>().max_value(n + 2)));
        let req = LibraryRequest { library: drive.spec().into(), offset, limit };
        let page = api::drive_library_cached(&cache, &req).unwrap();
        let start = offset.unwrap_or(0).min(n);
        let end = limit.map_or(n, |l| (start + l).min(n));
        assert_eq!(page.library.tracks, lib.tracks[start..end]);
        assert_eq!(
            (&page.library.playlists, &page.library.history, &page.summary),
            (&lib.playlists, &lib.history, &lib.summary())
        );
        match (offset, limit, page.page) {
            (None, None, None) => {}
            (_, _, Some(p)) => assert_eq!((p.offset, p.count, p.total), (offset.unwrap_or(0), end - start, n)),
            (o, l, p) => panic!("{o:?} {l:?} paged as {p:?}"),
        }
    }

    let tree = api::playlists_tree(&cache, &LibraryRef { drive: drive.spec(), combined: false }).unwrap();
    assert_tree(&tree.playlists, &lib.playlists, &lib);
    assert_tree(&tree.history, &lib.history, &lib);

    // By id, in the order asked; ids the library doesn't have, said.
    let known: Vec<u32> = lib.tracks.iter().map(|t| t.id).collect();
    let ids = tc.draw(
        gs::vecs(hegel::one_of!(gs::sampled_from(known.clone()), gs::integers::<u32>().max_value(40))).max_size(5),
    );
    let got = api::tracks_get(&cache, &TracksGet { library: drive.spec().into(), ids: ids.clone() }).unwrap();
    let expected: Vec<_> = ids.iter().filter_map(|id| lib.track(*id).cloned()).collect();
    let missing: Vec<u32> = ids.iter().copied().filter(|id| lib.track(*id).is_none()).collect();
    assert_eq!((got.tracks, got.missing), (expected, missing));

    // Details, in the order selected.
    let selected = tc.draw(gs::vecs(gs::sampled_from(known.clone())).min_size(1).max_size(3).unique(true));
    let req = TrackDetailsRequest {
        library: drive.spec().into(),
        tracks: TrackSelector { ids: selected.clone(), ..Default::default() },
    };
    let details = api::track_details_cached(&cache, &req).unwrap();
    assert_eq!(
        serde_json::to_value(&details).unwrap(),
        serde_json::to_value(api::track_details(&req).unwrap()).unwrap()
    );
    assert_eq!(details.iter().map(|d| d.track.id).collect::<Vec<_>>(), selected);
    let d = Device::open(&drive.root).unwrap();
    for det in &details {
        let t = &det.track;
        let mut playlists: Vec<String> = lib
            .playlists
            .iter()
            .filter(|p| !p.is_folder && p.track_ids.contains(&t.id))
            .map(|p| lib.playlist_path(p.id))
            .collect();
        playlists.sort();
        assert_eq!(det.playlists, playlists, "{}'s playlists", t.display_name());
        let history: Vec<String> = lib
            .history
            .iter()
            .filter(|h| !h.is_folder && h.track_ids.contains(&t.id))
            .map(|h| h.name.clone())
            .collect();
        assert_eq!(det.history, history);
        assert_eq!(det.audio_present, paths::to_fs(&drive.root, &paths::key(&t.file_path)).is_file());
        let copies: Vec<u32> = lib
            .tracks
            .iter()
            .filter(|o| {
                o.id != t.id
                    && o.fingerprint_key() == t.fingerprint_key()
                    && !(t.artist.trim().is_empty() && t.title.trim().is_empty())
            })
            .map(|o| o.id)
            .collect();
        assert_eq!(det.copies.iter().map(|c| c.id).collect::<Vec<_>>(), copies);
        let a = api::track_analysis(&TrackRequest { drive: drive.spec(), track: t.id }).unwrap();
        let hot = a.cues.iter().filter(|c| c.hot_cue.is_some()).count();
        assert_eq!(
            (det.analysis.beats, det.analysis.bars, det.analysis.hot_cues),
            (a.beats.len(), a.beats.len().div_ceil(4), hot)
        );
        assert_eq!((det.analysis.memory_cues, det.analysis.waveform), (a.cues.len() - hot, a.detail.is_some()));
        if let Some(r) = &det.analysis.tempo_range {
            assert!(
                r.low <= r.high && a.beats.iter().filter(|b| b.bpm > 0.0).all(|b| b.bpm >= r.low && b.bpm <= r.high)
            );
        }
        assert_eq!(
            det.analysis.analysed,
            d.checked_path(&paths::key(&t.analyze_path)).is_ok_and(|p| p.is_file()) && !t.analyze_path.is_empty()
        );

        // Matches are harmony's suggestions for the track.
        let limit = tc.draw(gs::optional(gs::integers::<usize>().max_value(5)));
        let matches =
            api::harmonic_matches_cached(&cache, &MatchesRequest { library: drive.spec().into(), track: t.id, limit })
                .unwrap();
        let suggested = harmony::suggestions(&lib, t, limit.unwrap_or(40));
        assert_eq!(
            matches.iter().map(|m| (m.track.id, m.key.clone(), m.relation)).collect::<Vec<_>>(),
            suggested.iter().map(|s| (s.track.id, s.key.code(), s.relation.describe())).collect::<Vec<_>>()
        );

        // Analysis as asked for: beats and cues always, each waveform when chosen.
        let waveform = tc.draw(
            gs::sampled_from(vec![
                WaveformChoice::None,
                WaveformChoice::Preview,
                WaveformChoice::Detail,
                WaveformChoice::All,
            ])
            .print_as_debug(),
        );
        for encoding in [WaveformEncoding::Json, WaveformEncoding::Base64] {
            let view = api::track_analysis_cached(
                &cache,
                &AnalysisRequest { library: drive.spec().into(), track: t.id, waveform, encoding },
            )
            .unwrap();
            assert_eq!(serde_json::to_value(&view.beats).unwrap(), serde_json::to_value(&a.beats).unwrap());
            assert_eq!(serde_json::to_value(&view.cues).unwrap(), serde_json::to_value(&a.cues).unwrap());
            let wants = |w: WaveformChoice| waveform == WaveformChoice::All || waveform == w;
            for (got, had, chosen) in [
                (&view.preview, &a.preview, wants(WaveformChoice::Preview)),
                (&view.detail, &a.detail, wants(WaveformChoice::Detail)),
            ] {
                match (got, had.as_ref().filter(|_| chosen)) {
                    (None, None) => {}
                    (Some(WaveformData::Json(w)), Some(h)) => assert!(encoding == WaveformEncoding::Json && w == h),
                    (Some(WaveformData::Packed(p)), Some(h)) => {
                        assert!(encoding == WaveformEncoding::Base64 && *p == PackedWaveform::pack(h))
                    }
                    (g, h) => panic!("{waveform:?}/{encoding:?}: sent {}, had {}", g.is_some(), h.is_some()),
                }
            }
        }
    }
    let unknown = 1 + known.iter().max().unwrap();
    assert!(matches!(
        api::harmonic_matches_cached(
            &cache,
            &MatchesRequest { library: drive.spec().into(), track: unknown, limit: None }
        ),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        api::track_analysis_cached(
            &cache,
            &AnalysisRequest {
                library: drive.spec().into(),
                track: unknown,
                waveform: WaveformChoice::All,
                encoding: WaveformEncoding::Json
            }
        ),
        Err(Error::NotFound(_))
    ));

    // Artwork: a batch answers what each track alone does, in the order asked.
    let asked = tc.draw(gs::vecs(gs::sampled_from(known.clone())).max_size(4));
    let many = ArtworkRequest::Many { library: drive.spec().into(), tracks: asked.clone() };
    let OneOrMany::Many(files) = api::artwork_path(&cache, &many).unwrap() else { panic!("one for many") };
    let OneOrMany::Many(images) = api::artwork_get_cached(&cache, &many).unwrap() else { panic!("one for many") };
    assert_eq!(files.iter().map(|f| f.track).collect::<Vec<_>>(), asked);
    assert_eq!(images.iter().map(|f| f.track).collect::<Vec<_>>(), asked);
    let root = dunce_canonicalize(&drive.root);
    for (file, image) in files.iter().zip(&images) {
        let t = lib.track(file.track).unwrap();
        let one = ArtworkRequest::One { library: drive.spec().into(), track: file.track };
        let OneOrMany::One(alone) = api::artwork_path(&cache, &one).unwrap() else { panic!("many for one") };
        assert_eq!(alone.as_ref().map(|f| &f.path), file.image.as_ref().map(|f| &f.path));
        if let Some(f) = &file.image {
            assert!(t.artwork_path.is_some() && f.path.starts_with(&root) && f.path.is_file(), "{:?}", f.path);
            let bytes = image.image.as_ref().map(|i| BASE64_STANDARD.decode(&i.base64).unwrap());
            assert!(bytes.is_some_and(|b| !b.is_empty()), "a file but no image for #{}", t.id);
        } else {
            assert!(t.artwork_path.is_none() || image.image.is_none());
        }
    }
    assert!(api::artwork_path(&cache, &ArtworkRequest::One { library: drive.spec().into(), track: unknown }).is_err());
}

/// The cached reads of My Tags, cues and play history agree with the uncached ones, and with
/// what the drive's history lists hold.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn cached_and_uncached_lists_agree(tc: TestCase) {
    let drive = drive(&tc);
    let lib = drive.library();
    let cache = LibraryCache::new(2);
    let r = DriveRef { drive: drive.spec() };
    let tags = api::tags_list_cached(&cache, &r).unwrap();
    assert_eq!(serde_json::to_value(&*tags).unwrap(), serde_json::to_value(api::tags_list(&r).unwrap()).unwrap());
    assert!(Arc::ptr_eq(&tags, &api::tags_list_cached(&cache, &r).unwrap()), "read once");
    let by_track = api::tags_by_track(&r).unwrap();
    assert_eq!(by_track, locallibrary::tags::by_track(&Device::open(&drive.root).unwrap()).unwrap());

    let ids = tc
        .draw(gs::vecs(gs::sampled_from(lib.tracks.iter().map(|t| t.id).collect::<Vec<_>>())).min_size(1).max_size(3));
    let req = ListCues { library: drive.spec().into(), tracks: TrackSelector { ids, ..Default::default() } };
    let cached = api::cues_list_cached(&cache, &req).unwrap();
    assert_eq!(serde_json::to_value(&cached).unwrap(), serde_json::to_value(api::cues_list(&req).unwrap()).unwrap());

    let history = api::drive_history(&r).unwrap();
    let lists: Vec<&Playlist> = lib.history.iter().filter(|h| !h.is_folder).collect();
    assert_eq!(history.lists.len(), lists.len());
    for (got, list) in history.lists.iter().zip(&lists) {
        assert_eq!(got.name, list.name);
        let tracks: Vec<u32> = list.track_ids.iter().copied().filter(|id| lib.track(*id).is_some()).collect();
        assert_eq!(got.tracks.iter().map(|t| t.id).collect::<Vec<_>>(), tracks);
    }
}

/// Packing a waveform keeps one height byte and three colour bytes a column, each height the
/// column's clamped to 0-1 in 255ths, and a front layer only when some column has one.
#[hegel::test(test_cases = 200)]
fn packed_waveforms_hold_every_column(tc: TestCase) {
    let layer = |tc: &TestCase| WaveLayer {
        height: tc.draw(hegel::one_of!(gs::floats::<f32>().min_value(-0.5).max_value(1.5), gs::just(0.0f32))),
        color: [tc.draw(gs::integers::<u8>()), tc.draw(gs::integers::<u8>()), tc.draw(gs::integers::<u8>())],
    };
    let n = tc.draw(gs::integers::<usize>().max_value(40));
    let columns: Vec<WaveColumn> = (0..n).map(|_| WaveColumn { back: layer(&tc), front: layer(&tc) }).collect();
    let style = tc.draw(gs::sampled_from(vec![WaveformStyle::Blue, WaveformStyle::Rgb]).print_as_debug());
    let w = Waveform { style, columns_per_sec: tc.draw(gs::optional(gs::just(150.0))), columns };
    let p = PackedWaveform::pack(&w);
    assert_eq!((p.encoding, p.style, p.columns_per_sec, p.columns), ("base64", w.style, w.columns_per_sec, n));
    let unpack = |l: &api::PackedLayer| {
        (BASE64_STANDARD.decode(&l.heights).unwrap(), BASE64_STANDARD.decode(&l.colors).unwrap())
    };
    let check = |l: &api::PackedLayer, pick: fn(&WaveColumn) -> WaveLayer| {
        let (heights, colors) = unpack(l);
        assert_eq!((heights.len(), colors.len()), (n, 3 * n));
        for (i, c) in w.columns.iter().enumerate() {
            let want = pick(c);
            let h = f32::from(heights[i]) / 255.0;
            assert!(
                (h - want.height.clamp(0.0, 1.0)).abs() <= 0.5 / 255.0 + 1e-6,
                "column {i}: {h} for {}",
                want.height
            );
            assert_eq!(colors[3 * i..3 * i + 3], want.color);
        }
    };
    check(&p.back, |c| c.back);
    assert_eq!(p.front.is_some(), w.columns.iter().any(|c| c.front.height > 0.0));
    if let Some(front) = &p.front {
        check(front, |c| c.front);
    }
}

// ---------------------------------------------------------------------------------------------
// The cache of parsed libraries
// ---------------------------------------------------------------------------------------------

struct CacheMachine {
    _dir: tempfile::TempDir,
    cache: LibraryCache,
    capacity: usize,
    drives: Vec<PathBuf>,
    /// What the model keeps, least recently used first: each drive's library as last read, and
    /// whether its databases changed since (kept, taking its place, until it's next read).
    kept: Vec<(PathBuf, Arc<locallibrary::api::CachedLibrary>, bool)>,
    /// Tracks exported to each drive since it was made.
    exported: Vec<usize>,
}

impl CacheMachine {
    fn root(&self, i: usize) -> PathBuf {
        dunce_canonicalize(&self.drives[i])
    }
}

#[hegel::state_machine]
impl CacheMachine {
    /// A read answers the library kept from before when its databases haven't changed since,
    /// else reads it again; either way it's the drive's library now, and the drive is the most
    /// recently used.
    #[rule]
    fn get(&mut self, tc: TestCase) {
        let i = tc.draw(gs::integers::<usize>().max_value(self.drives.len() - 1));
        let root = self.root(i);
        let got = match tc.draw(gs::booleans()) {
            true => self.cache.get(&Device::open(&root).unwrap()).unwrap(),
            false => self.cache.open(&path(&root)).unwrap().1,
        };
        if let Some(k) = self.kept.iter().position(|(r, _, _)| *r == root) {
            let (_, before, stale) = self.kept.remove(k);
            assert_eq!(Arc::ptr_eq(&got, &before), !stale, "{} read again: {}", root.display(), !stale);
        }
        assert_eq!(*got.library, Device::open(&root).unwrap().library().unwrap());
        assert_eq!(got.root, root);
        if self.capacity > 0 {
            self.kept.push((root, got, false));
            let excess = self.kept.len().saturating_sub(self.capacity);
            self.kept.drain(..excess);
        }
    }

    /// rekordbox exports another track: the next read sees it, without being told.
    #[rule]
    fn export(&mut self, tc: TestCase) {
        let i = tc.draw(gs::integers::<usize>().max_value(self.drives.len() - 1));
        self.exported[i] += 1;
        common::export_new_track(&self.drives[i], "Artist", &format!("Fresh {}", self.exported[i]));
        let root = self.root(i);
        for k in self.kept.iter_mut().filter(|(r, _, _)| *r == root) {
            k.2 = true;
        }
    }

    #[rule]
    fn invalidate(&mut self, tc: TestCase) {
        let i = tc.draw(gs::integers::<usize>().max_value(self.drives.len() - 1));
        let root = self.root(i);
        self.cache.invalidate(&root);
        self.kept.retain(|(r, _, _)| *r != root);
    }

    #[rule]
    fn clear(&mut self, _: TestCase) {
        self.cache.clear();
        self.kept.clear();
    }

    /// The cache keeps the drives the model does, in its order, never more than it may.
    #[invariant]
    fn keeps_what_the_model_does(&mut self, _: TestCase) {
        assert_eq!(self.cache.roots(), self.kept.iter().map(|(r, _, _)| r.clone()).collect::<Vec<_>>());
    }
}

/// Any sequence of reads, exports, invalidations and clears leaves the cache keeping the
/// libraries a least-recently-used list of its capacity would, and every read answers the
/// drive's library as it is now.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn the_cache_is_a_least_recently_used_list(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let drives: Vec<PathBuf> = ["A", "B", "C"].iter().map(|n| common::demo_drive(dir.path(), n)).collect();
    let capacity = tc.draw(gs::integers::<usize>().max_value(3));
    let machine = CacheMachine {
        _dir: dir,
        cache: LibraryCache::new(capacity),
        capacity,
        exported: vec![0; drives.len()],
        drives,
        kept: Vec::new(),
    };
    hegel::stateful::machine(machine).steps(12).run(tc);
}
