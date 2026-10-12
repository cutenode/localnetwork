use super::*;
use loclib_model::test_util::track;

fn lib() -> Library {
    let mut l = Library {
        tracks: (1..=5).map(|i| track(i, "A", &format!("T{i}"), &format!("/Contents/{i}.mp3"))).collect(),
        ..Default::default()
    };
    l.playlists = vec![
        Playlist { id: 1, parent_id: None, name: "Sets".into(), is_folder: true, sort_order: 0, track_ids: vec![] },
        Playlist {
            id: 2,
            parent_id: Some(1),
            name: "Fri".into(),
            is_folder: false,
            sort_order: 0,
            track_ids: vec![1, 2, 3],
        },
        Playlist { id: 3, parent_id: None, name: "Loose".into(), is_folder: false, sort_order: 1, track_ids: vec![4] },
    ];
    l
}

/// A drive that's an empty folder of its own (the system's temp folder, used before, can
/// hold anything a drive might).
fn empty_drive() -> (tempfile::TempDir, Device) {
    let dir = tempfile::tempdir().unwrap();
    let device = Device::open(dir.path()).unwrap();
    (dir, device)
}

fn run(edit: Edit) -> Result<Library> {
    let (_dir, device) = empty_drive();
    let extra = DriveContext::default();
    let r = Resolver::new(&device, &extra, false, Ctx::silent());
    let mut model = ModelDb { lib: lib(), pdb: None };
    let (ops, _, _) = r.resolve(&edit, &model.lib)?;
    for op in &ops {
        apply_op(&mut model, op, true, "test", &mut Vec::new())?;
    }
    Ok(model.lib)
}

fn entries(l: &Library, p: &str) -> Vec<u32> {
    l.find_playlist(p).unwrap().track_ids.clone()
}

#[test]
fn playlist_edits_on_the_model() {
    let l = run(Edit::CreatePlaylist { path: "Shows/2026/Berlin".into(), folder: false }).unwrap();
    assert_eq!(l.playlist_path(l.find_playlist("Shows/2026/Berlin").unwrap().id), "Shows/2026/Berlin");
    assert!(l.find_playlist("Shows/2026").unwrap().is_folder);

    let l = run(Edit::MovePlaylist { playlist: "Loose".into(), into: Some("Sets".into()), position: Some(1) }).unwrap();
    assert_eq!(l.children(Some(1)).iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Loose", "Fri"]);
    assert!(run(Edit::MovePlaylist { playlist: "Sets".into(), into: Some("Sets".into()), position: None }).is_err());

    let l = run(Edit::DeletePlaylist { playlist: "Sets".into(), with_tracks: false }).unwrap();
    assert_eq!(l.playlists.len(), 1);
    assert_eq!(l.tracks.len(), 5);
    // Tracks 2 and 3 are only in Sets/Fri; track 1 is too, but a history list has it.
    let mut with_history = lib();
    with_history.history = vec![Playlist {
        id: 9,
        parent_id: None,
        name: "HISTORY 001".into(),
        is_folder: false,
        sort_order: 0,
        track_ids: vec![1],
    }];
    let (_dir, device) = empty_drive();
    let extra = DriveContext::default();
    let r = Resolver::new(&device, &extra, false, Ctx::silent());
    let (ops, _, _) =
        r.resolve(&Edit::DeletePlaylist { playlist: "Sets".into(), with_tracks: true }, &with_history).unwrap();
    assert!(ops.contains(&Op::DeleteTracks { tracks: vec!["contents/2.mp3".into(), "contents/3.mp3".into()] }));

    let add = |ids: Vec<u32>, position| Edit::AddTracks {
        playlist: "Sets/Fri".into(),
        tracks: TrackSelector { ids, ..Default::default() },
        position,
        sort: None,
        allow_repeats: false,
    };
    assert_eq!(entries(&run(add(vec![5, 1], Some(2))).unwrap(), "Fri"), [1, 5, 2, 3]);
    assert_eq!(entries(&run(add(vec![4], None)).unwrap(), "Fri"), [1, 2, 3, 4]);

    let l = run(Edit::MoveTracks { playlist: "Fri".into(), positions: vec![3], to: 1 }).unwrap();
    assert_eq!(entries(&l, "Fri"), [3, 1, 2]);
    let l = run(Edit::RemoveTracks { playlist: "Fri".into(), tracks: None, positions: vec![2] }).unwrap();
    assert_eq!(entries(&l, "Fri"), [1, 3]);
    assert!(run(Edit::RemoveTracks { playlist: "Fri".into(), tracks: None, positions: vec![9] }).is_err());

    let l = run(Edit::SetTracks {
        playlist: "Rules/Unplayed".into(),
        tracks: TrackSelector { filter: Some("is:unplayed".into()), ..Default::default() },
        sort: Some("-title".into()),
        limit: Some(2),
        create: true,
    })
    .unwrap();
    assert_eq!(entries(&l, "Rules/Unplayed"), [5, 4]);

    let l = run(Edit::RenamePlaylist { playlist: "Fri".into(), name: "Friday".into() }).unwrap();
    assert!(l.find_playlist("Sets/Friday").is_some());
    assert!(run(Edit::RenamePlaylist { playlist: "Loose".into(), name: "Sets".into() }).is_err());
}

/// Regression: edits couldn't reach a playlist shown after a folder of its name (they found
/// the folder and said so). Edits that need a playlist find it, and the others take the
/// playlist unless its path ends in `/`.
#[test]
fn a_playlist_shown_after_a_folder_of_its_name_is_edited() {
    let (_dir, device) = empty_drive();
    let extra = DriveContext::default();
    let r = Resolver::new(&device, &extra, false, Ctx::silent());
    let mut start = lib();
    start.playlists.push(Playlist {
        id: 4,
        parent_id: None,
        name: "Sets".into(),
        is_folder: false,
        sort_order: 2,
        track_ids: vec![5],
    });
    let run = |edits: &[Edit]| -> Result<Library> {
        let mut model = ModelDb { lib: start.clone(), pdb: None };
        for edit in edits {
            let (ops, _, _) = r.resolve(edit, &model.lib)?;
            for op in &ops {
                apply_op(&mut model, op, true, "test", &mut Vec::new())?;
            }
        }
        Ok(model.lib)
    };
    let add = Edit::AddTracks {
        playlist: "Sets".into(),
        tracks: TrackSelector { ids: vec![1], ..Default::default() },
        position: None,
        sort: None,
        allow_repeats: false,
    };
    let l = run(&[add]).unwrap();
    assert_eq!(l.playlist(4).unwrap().track_ids, [5, 1]);
    let rename = |spec: &str| Edit::RenamePlaylist { playlist: spec.into(), name: "Old sets".into() };
    let l = run(&[rename("Sets")]).unwrap();
    assert_eq!((l.playlist(1).unwrap().name.as_str(), l.playlist(4).unwrap().name.as_str()), ("Sets", "Old sets"));
    let l = run(&[rename("Sets/")]).unwrap();
    assert_eq!((l.playlist(1).unwrap().name.as_str(), l.playlist(4).unwrap().name.as_str()), ("Old sets", "Sets"));
    // Into the folder, not the playlist; deleting the playlist leaves the folder.
    let into = Edit::MovePlaylist { playlist: "Loose".into(), into: Some("Sets".into()), position: None };
    assert_eq!(run(&[into]).unwrap().playlist(3).unwrap().parent_id, Some(1));
    let l = run(&[Edit::DeletePlaylist { playlist: "Sets".into(), with_tracks: false }]).unwrap();
    assert!(l.playlist(1).is_some() && l.playlist(4).is_none());
    // The folder's playlist is still found through it.
    let fill = Edit::SetEntries { playlist: "Sets/Fri".into(), paths: vec!["/Contents/4.mp3".into()], create: true };
    assert_eq!(run(&[fill]).unwrap().playlist(2).unwrap().track_ids, [4]);
}

/// Regression: a fill that makes its playlist took a playlist of that name in another folder
/// when the path was missing, and made another case variant when the path matched several
/// ignoring case.
#[test]
fn a_fill_that_makes_its_playlist_takes_exactly_its_path() {
    let fill = |path: &str| Edit::SetTracks {
        playlist: path.into(),
        tracks: TrackSelector { ids: vec![5], ..Default::default() },
        sort: None,
        limit: None,
        create: true,
    };
    let l = run(fill("Fri")).unwrap();
    assert_eq!(l.playlist(2).unwrap().track_ids, [1, 2, 3], "Sets/Fri is left alone");
    let top = l.children(None).into_iter().find(|p| p.name == "Fri").expect("a top-level Fri");
    assert_eq!(top.track_ids, [5]);

    let (_dir, device) = empty_drive();
    let extra = DriveContext::default();
    let r = Resolver::new(&device, &extra, false, Ctx::silent());
    let mut twins = lib();
    for (id, name) in [(10, "GIGS"), (12, "Gigs")] {
        twins.playlists.push(Playlist {
            id,
            parent_id: None,
            name: name.into(),
            is_folder: true,
            sort_order: id,
            track_ids: vec![],
        });
        twins.playlists.push(Playlist {
            id: id + 1,
            parent_id: Some(id),
            name: "Warmup".into(),
            is_folder: false,
            sort_order: 0,
            track_ids: vec![1],
        });
    }
    // Never a folder, nor a playlist whose path differs in case.
    twins.playlists.push(Playlist {
        id: 20,
        parent_id: None,
        name: "Sets".into(),
        is_folder: false,
        sort_order: 20,
        track_ids: vec![],
    });
    let found = |q| find_exact(&twins, q).unwrap().map(|p| p.id);
    assert_eq!(
        (found("Sets"), found("Sets/"), found("Loose"), found("LOOSE"), found("Fri")),
        (Some(20), Some(20), Some(3), None, None)
    );
    let err = r.resolve(&fill("gigs/warmup"), &twins).unwrap_err().to_string();
    assert!(err.contains("\"GIGS/Warmup\" and \"Gigs/Warmup\""), "{err}");
    let import = Edit::SetEntries { playlist: "gigs/warmup".into(), paths: vec![], create: true };
    assert!(r.resolve(&import, &twins).is_err());
    let (ops, _, _) = r.resolve(&fill("Gigs/Warmup"), &twins).unwrap();
    assert_eq!(
        ops,
        [Op::SetEntries {
            node: vec!["Gigs".into(), "Warmup".into()],
            tracks: vec!["contents/5.mp3".into()],
            before: None
        }]
    );
}

/// Regression: reordering a playlist in the second of two same-named folders moved it into
/// the first, which its folder's path names.
#[test]
fn reordering_in_a_later_same_named_folder_keeps_it_there() {
    let mut model = ModelDb { lib: lib(), pdb: None };
    let node = |id, parent_id, name: &str, is_folder, sort_order| Playlist {
        id,
        parent_id,
        name: name.into(),
        is_folder,
        sort_order,
        track_ids: vec![],
    };
    model.lib.playlists.extend([
        node(10, None, "Gigs", true, 5),
        node(11, Some(10), "Warmup", false, 0),
        node(12, None, "Gigs", true, 6),
        node(13, Some(12), "Set", false, 0),
        node(14, Some(12), "Closing", false, 1),
    ]);
    let (_dir, device) = empty_drive();
    let extra = DriveContext::default();
    let r = Resolver::new(&device, &extra, false, Ctx::silent());
    let edit = Edit::MovePlaylist { playlist: "Gigs/Closing".into(), into: Some("Gigs".into()), position: Some(1) };
    let (ops, _, _) = r.resolve(&edit, &model.lib).unwrap();
    for op in &ops {
        apply_op(&mut model, op, true, "test", &mut Vec::new()).unwrap();
    }
    let names = |id| model.lib.children(Some(id)).iter().map(|p| p.name.clone()).collect::<Vec<_>>();
    assert_eq!((names(10), names(12)), (vec!["Warmup".to_string()], vec!["Closing".to_string(), "Set".to_string()]));
}

/// Regression: verification keyed playlists by path, so it missed one of two same-named
/// siblings, or a playlist next to a folder of its name, reading back wrong.
#[test]
fn verification_sees_every_playlist_sharing_a_path() {
    let node = |id, is_folder, track_ids| Playlist {
        id,
        parent_id: None,
        name: "Set".into(),
        is_folder,
        sort_order: id,
        track_ids,
    };
    let check = |expected: Vec<Playlist>, actual: Vec<Playlist>| {
        let lib = |playlists| Library { tracks: lib().tracks, playlists, ..Default::default() };
        verify(&lib(expected), &lib(actual))
    };
    let twins = vec![node(1, false, vec![1]), node(2, false, vec![2])];
    assert_eq!(check(twins.clone(), twins.clone()), Ok(()));
    // The first reads back with the second's entries.
    let err = check(twins, vec![node(1, false, vec![2]), node(2, false, vec![2])]);
    assert!(err.is_err_and(|e| e.starts_with("playlist Set:")));
    let next_to_folder = vec![node(1, false, vec![1]), node(2, true, vec![])];
    let err = check(next_to_folder.clone(), vec![node(1, false, vec![3]), node(2, true, vec![])]);
    assert!(err.is_err_and(|e| e.starts_with("playlist Set:")));
    // The playlist reads back after the folder.
    let err = check(next_to_folder, vec![node(3, false, vec![1]), node(2, true, vec![])]);
    assert_eq!(err, Err("playlist order differs".into()));
}

/// My Tag and cue edits go into the journal as JSON and come back the same.
#[test]
fn my_tag_and_cue_edits_are_recorded_and_read_back() {
    use crate::cue_edit::{CueChange, CuePosition, CueRef, HotCue, SetCue};
    use crate::tags::TagChange;
    let a = vec!["/Contents/A/a.mp3".to_string()];
    let set = SetCue {
        hot_cue: Some(HotCue(1)),
        at: CuePosition::Beat(33),
        snap: false,
        loop_beats: Some(0.5),
        name: Some("Drop".into()),
        color: None,
    };
    let edits = vec![
        Edit::MyTag(TagChange::Add { tag: "Genre/Deep".into(), tracks: a.clone() }),
        Edit::MyTag(TagChange::Rename { tag: "Genre".into(), to: "Style".into() }),
        Edit::Cues { tracks: a.clone(), change: CueChange::Set(set) },
        Edit::Cues { tracks: a.clone(), change: CueChange::Delete(CueRef::All) },
        Edit::Cues { tracks: a, change: CueChange::Copy { from: "/Contents/B/b.mp3".into() } },
    ];
    let json = serde_json::to_string(&edits).unwrap();
    assert!(json.starts_with(r#"[{"op":"my_tag","action":"add","tag":"Genre/Deep""#), "{json}");
    assert!(json.contains(r#""change":{"set":{"hot_cue":"B","at":{"beat":33},"loop_beats":0.5"#), "{json}");
    assert_eq!(serde_json::from_str::<Vec<Edit>>(&json).unwrap(), edits);
}

#[test]
fn track_edits_on_the_model() {
    let l = run(Edit::EditTracks {
        tracks: TrackSelector { ids: vec![2], ..Default::default() },
        fields: TrackFields {
            rating: Some(4),
            color: Some("aqua".into()),
            genre: Some("Techno".into()),
            ..Default::default()
        },
    })
    .unwrap();
    let t = l.track(2).unwrap();
    assert_eq!((t.rating, t.color, t.genre.as_str()), (4, Some(TrackColor::Aqua), "Techno"));
    assert!(
        run(Edit::EditTracks {
            tracks: TrackSelector { ids: vec![2], ..Default::default() },
            fields: TrackFields::default()
        })
        .is_err()
    );
    assert!(
        run(Edit::EditTracks {
            tracks: TrackSelector { ids: vec![2], ..Default::default() },
            fields: TrackFields { rating: Some(6), ..Default::default() }
        })
        .is_err()
    );

    let l = run(Edit::ReplaceTrack {
        duplicate: TrackSelector { ids: vec![2], ..Default::default() },
        keep: TrackSelector { ids: vec![5], ..Default::default() },
        keep_duplicate: false,
    })
    .unwrap();
    assert_eq!(entries(&l, "Fri"), [1, 5, 3]);
    assert!(l.track(2).is_none());
}
