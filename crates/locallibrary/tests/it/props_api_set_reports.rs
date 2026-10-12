//! Sets built from kept reports of recordings: the order their tracks play in, what's refused
//! with a recording, attaching another report, and the playlist an untitled set makes.

use crate::common;
use crate::sets::report;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api::{self, EditSet, NewSet, SetFields, SetPlaylist, TrackPlaysQuery};
use locallibrary::mix::report as kept;
use locallibrary::progress::Ctx;
use locallibrary::sets::{EntryOrigin, Set};
use locallibrary::{Device, Error, Store, Track};

/// Plays of distinct tracks, at whole seconds apart enough to be told apart.
fn plays(tc: &TestCase, tracks: &[Track]) -> Vec<(usize, f64)> {
    let picks =
        tc.draw(gs::vecs(gs::integers::<usize>().max_value(tracks.len() - 1)).min_size(1).max_size(4).unique(true));
    picks.into_iter().map(|i| (i, f64::from(tc.draw(gs::integers::<u16>().max_value(3000))) * 2.0 + 0.5)).collect()
}

/// What a set from these plays holds: each track by name, from its start, in the order played.
fn in_order(tracks: &[Track], plays: &[(usize, f64)]) -> Vec<(String, Option<u32>)> {
    let mut p = plays.to_vec();
    p.sort_by(|a, b| a.1.total_cmp(&b.1));
    p.iter().map(|&(i, s)| (tracks[i].display_name(), Some(s as u32))).collect()
}

fn held(set: &Set) -> Vec<(String, Option<u32>)> {
    set.tracks.iter().map(|e| (e.name(), e.start_seconds)).collect()
}

/// A kept report of a recording playing drawn tracks of the twelve-track export at drawn
/// starts: a set from it holds them in the order played, each from its start, heard in the
/// recording. A report and a recording together are refused, as is a recording that isn't there
/// or isn't one mix reads. Attaching another report rebuilds the tracklist from it, unless
/// changes by position come with it, which are refused. An untitled set's playlist is named for
/// its day. Asking where a track was played needs a track named.
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn sets_from_kept_reports_play_in_recording_order(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let ctx = Ctx::silent();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let volumes = common::isolate(dir.path(), Some(store.root()));
    let drive = common::bigger_drive(&volumes, "Main");
    let tracks = Device::open(&drive).unwrap().library().unwrap().tracks;
    let first = plays(&tc, &tracks);
    let second = plays(&tc, &tracks);
    let title = tc.draw(gs::optional(gs::sampled_from(vec!["Friday", "Warehouse 4/5"])));
    // Reports are kept by their recording: two recordings, two reports.
    let save = |p: &[(usize, f64)], recording: &str| {
        let p: Vec<(&Track, f64, f64)> = p.iter().map(|&(i, s)| (&tracks[i], s, 0.9)).collect();
        let mut r = report(&p, None);
        r.mix.path = format!("/mixes/{recording}.wav").into();
        kept::save(&store, &r).unwrap()
    };
    let (a, b) = (save(&first, "first"), save(&second, "second"));
    assert_ne!(a, b);

    let fields = SetFields { title: title.map(String::from), date: Some("2026-10-03".into()), ..Default::default() };
    let wav = dir.path().join("set.wav");
    let notes = dir.path().join("notes.txt");
    std::fs::write(&notes, "not a recording").unwrap();
    let refused = |req: NewSet| api::set_new(&store, &req, &ctx).map(|c| c.set.id).unwrap_err();
    let both = NewSet { report: Some(a.clone()), recording: Some(wav.clone()), ..Default::default() };
    assert!(matches!(refused(both), Error::Invalid(m) if m == "give a recording or a report, not both"));
    assert!(matches!(refused(NewSet { recording: Some(wav.clone()), ..Default::default() }), Error::NotFound(_)));
    let e = refused(NewSet { recording: Some(notes), ..Default::default() });
    assert!(e.to_string().contains("isn't a recording mix reads"), "{e}");

    let made = api::set_new(&store, &NewSet { fields, report: Some(a.clone()), ..Default::default() }, &ctx).unwrap();
    let set = made.set;
    assert_eq!(held(&set), in_order(&tracks, &first));
    assert!(set.tracks.iter().all(|e| e.origin == EntryOrigin::Recording));
    assert_eq!(set.recording.as_ref().and_then(|r| r.report.as_deref()), Some(a.as_str()));

    let with_changes = EditSet { set: set.id.clone(), report: Some(b.clone()), drop: vec![1], ..Default::default() };
    let e = api::set_edit(&store, &with_changes, &ctx).map(|c| c.set.id).unwrap_err();
    assert!(e.to_string().contains("attach the recording first, then change its tracklist"), "{e}");
    let attached =
        api::set_edit(&store, &EditSet { set: set.id.clone(), report: Some(b.clone()), ..Default::default() }, &ctx)
            .unwrap();
    assert_eq!(held(&attached.set), in_order(&tracks, &second));
    assert_eq!(attached.set.recording.as_ref().and_then(|r| r.report.as_deref()), Some(b.as_str()));

    let req = SetPlaylist { set: set.id.clone(), drive: drive.display().to_string(), name: None };
    api::set_playlist_apply(&store, &req, &ctx).unwrap();
    let lib = common::library(&drive);
    let name = match title {
        Some(t) => format!("Sets/2026-10-03 {}", t.replace('/', "-")),
        None => "Sets/2026-10-03".to_string(),
    };
    let list = lib.find_playlist(&name).unwrap_or_else(|| panic!("{name} on the drive"));
    let mut want: Vec<(usize, f64)> = second.clone();
    want.sort_by(|x, y| x.1.total_cmp(&y.1));
    assert_eq!(list.track_ids, want.iter().map(|&(i, _)| tracks[i].id).collect::<Vec<_>>());

    let e = api::track_plays(&store, &TrackPlaysQuery { track: "  ".into() }).unwrap_err();
    assert!(matches!(e, Error::Invalid(_)), "{e}");
}
