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
    run_on(lib(), edit)
}

/// [`run`] on the library `start`.
fn run_on(start: Library, edit: Edit) -> Result<Library> {
    let (_dir, device) = empty_drive();
    let extra = DriveContext::default();
    let r = Resolver::new(&device, &extra, false, Ctx::silent());
    let mut model = ModelDb { lib: start, pdb: None };
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

/// How often each key is in `keys`.
fn counts(keys: &[String]) -> FxHashMap<&str, usize> {
    let mut out = FxHashMap::default();
    for k in keys {
        *out.entry(k.as_str()).or_default() += 1;
    }
    out
}

/// An edit planned on `before` and turned into `after`, made in a database whose own entries are
/// `current`: made on `before` itself it gives `after`; otherwise each key loses the occurrences
/// the edit removed (as many as `current` has) and gains the ones it added, and keys the edit
/// never saw stay in `current`'s order.
#[hegel::test(test_cases = 300)]
fn an_edit_rebased_onto_other_entries_makes_its_own_changes(tc: hegel::TestCase) {
    use hegel::Generator;
    use hegel::generators as gs;
    let keys = || gs::vecs(gs::sampled_from(vec!["a", "b", "c", "d", "e"]).map(String::from)).max_size(6);
    let before: Vec<String> = tc.draw(keys());
    let after: Vec<String> = tc.draw(keys());
    let current: Vec<String> = match tc.draw(gs::booleans()) {
        true => before.clone(),
        false => tc.draw(keys()),
    };
    let got: Vec<String> = rebase_entries(&current, &before, &after).into_iter().map(String::from).collect();
    if current == before {
        assert_eq!(got, after);
    }
    let (b, a, c, g) = (counts(&before), counts(&after), counts(&current), counts(&got));
    for k in ["a", "b", "c", "d", "e"] {
        let n = |m: &FxHashMap<&str, usize>| m.get(k).copied().unwrap_or(0);
        let removed = n(&b).saturating_sub(n(&a)).min(n(&c));
        let added = n(&a).saturating_sub(n(&b));
        assert_eq!(n(&g), n(&c) - removed + added, "{k}: {current:?} rebased from {before:?} to {after:?} is {got:?}");
    }
    let unseen = |l: &[String]| -> Vec<String> {
        l.iter().filter(|k| !b.contains_key(k.as_str()) && !a.contains_key(k.as_str())).cloned().collect()
    };
    assert_eq!(unseen(&got), unseen(&current));
}

/// [`lib`] with a sixth track, another copy of the first recording, drawn entries in its two
/// playlists, and drawn history lists (two may share a name).
#[hegel::test_helper]
fn drawn_lib(tc: &hegel::TestCase) -> Library {
    use hegel::generators as gs;
    let mut l = lib();
    l.tracks.push(track(6, "A", "T1", "/Contents/copy.mp3"));
    let entries = || tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(6)).max_size(6));
    for p in l.playlists.iter_mut().filter(|p| !p.is_folder) {
        p.track_ids = entries();
    }
    let names = ["HISTORY 001", "HISTORY 002", "HISTORY 001"];
    for (i, name) in names.into_iter().take(tc.draw(gs::integers::<usize>().max_value(3))).enumerate() {
        let id = 30 + u32::try_from(i).unwrap();
        l.history.push(Playlist {
            id,
            parent_id: None,
            name: name.into(),
            is_folder: false,
            sort_order: id,
            track_ids: entries(),
        });
    }
    l
}

/// An edit for [`drawn_lib`], naming playlists that may be missing, folders, or other case.
#[hegel::test_helper]
fn draw_model_edit(tc: &hegel::TestCase) -> Edit {
    use hegel::generators as gs;
    // Mostly playlists that are there; at times a folder, another case, or one that's missing.
    let spec = || {
        let specs = vec!["Sets/Fri", "Sets/Fri", "Loose", "Loose", "Fri", "Sets", "Sets/", "LOOSE", "New/Deep", "Nope"];
        tc.draw(gs::sampled_from(specs)).to_string()
    };
    // Tracks that are there (id 7 isn't, at times).
    let ids = || {
        let most = if tc.draw(gs::weighted_booleans(0.1)) { 7 } else { 6 };
        let ids = tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(most)).min_size(1).max_size(3));
        TrackSelector { ids, ..Default::default() }
    };
    let position = || tc.draw(gs::optional(gs::integers::<usize>().max_value(7)));
    let positions = || tc.draw(gs::vecs(gs::integers::<usize>().min_value(1).max_value(4)).max_size(2));
    let sort = || {
        tc.draw(gs::optional(gs::sampled_from(vec!["title", "-bpm", "artist,-title", "rating", "nonsense"])))
            .map(String::from)
    };
    match tc.draw(gs::integers::<u8>().max_value(16)) {
        0 => Edit::CreatePlaylist { path: spec(), folder: tc.draw(gs::booleans()) },
        1 => Edit::RenamePlaylist {
            playlist: spec(),
            name: tc.draw(gs::sampled_from(vec!["Fri", "Loose", "Sat", "sets"])).into(),
        },
        2 => Edit::MovePlaylist {
            playlist: spec(),
            into: tc.draw(gs::optional(gs::sampled_from(vec!["Sets", "Loose", "", "Nope"]))).map(String::from),
            position: position(),
        },
        3 => Edit::DeletePlaylist { playlist: spec(), with_tracks: tc.draw(gs::booleans()) },
        4 => Edit::AddTracks {
            playlist: spec(),
            tracks: ids(),
            position: position(),
            sort: sort(),
            allow_repeats: tc.draw(gs::booleans()),
        },
        5 => Edit::RemoveTracks { playlist: spec(), tracks: tc.draw(gs::booleans()).then(ids), positions: positions() },
        6 => Edit::MoveTracks {
            playlist: spec(),
            positions: positions(),
            to: tc.draw(gs::integers::<usize>().max_value(7)),
        },
        7 => Edit::SetTracks {
            playlist: spec(),
            tracks: ids(),
            sort: sort(),
            limit: position(),
            create: tc.draw(gs::booleans()),
        },
        8 => Edit::SetEntries {
            playlist: spec(),
            paths: (1..=tc.draw(gs::integers::<u32>().max_value(3))).map(|i| format!("/Contents/{i}.mp3")).collect(),
            create: tc.draw(gs::booleans()),
        },
        9 => Edit::SortPlaylist { playlist: spec(), by: sort().unwrap_or_default() },
        10 => Edit::Dedupe { playlist: spec(), recordings: tc.draw(gs::booleans()) },
        11 => {
            // At times no field (refused), or an album of only spaces (which clears its artist too).
            let fields = TrackFields {
                rating: tc.draw(gs::optional(gs::integers::<u8>().max_value(6))),
                album: tc.draw(gs::optional(gs::sampled_from(vec!["", " ", "B"]))).map(String::from),
                ..Default::default()
            };
            Edit::EditTracks { tracks: ids(), fields }
        }
        12 => Edit::DeleteTracks { tracks: ids(), keep_files: tc.draw(gs::booleans()) },
        13 => Edit::ReplaceTrack { duplicate: ids(), keep: ids(), keep_duplicate: tc.draw(gs::booleans()) },
        14 => Edit::DeleteHistory {
            names: tc
                .draw(gs::vecs(gs::sampled_from(vec!["HISTORY 001", "history 002", "HISTORY 009"])).max_size(2))
                .into_iter()
                .map(String::from)
                .collect(),
            all: tc.draw(gs::booleans()),
        },
        _ => Edit::PlanSet {
            playlist: spec(),
            start: tc.draw(gs::booleans()).then(ids),
            into: tc.draw(gs::optional(gs::sampled_from(vec!["Planned", "Sets/Fri"]))).map(String::from),
        },
    }
}

/// `edits` resolved and applied one after another on `start`, as a drive's database would be.
fn resolve_all(start: &Library, edits: &[Edit], lenient: bool) -> Result<(Library, Vec<Edit>)> {
    let (_dir, device) = empty_drive();
    let extra = DriveContext::default();
    let r = Resolver::new(&device, &extra, lenient, Ctx::silent());
    let mut model = ModelDb { lib: start.clone(), pdb: None };
    let mut recorded = Vec::new();
    for edit in edits {
        let (ops, _, rec) = r.resolve(edit, &model.lib)?;
        for op in &ops {
            apply_op(&mut model, op, true, "test", &mut Vec::new())?;
        }
        recorded.extend(rec);
    }
    Ok((model.lib, recorded))
}

/// What the journal keeps of an edit redoes it: its recorded form, made on the library the edit
/// was made on, gives the same library, and so does replaying that leniently.
#[hegel::test(test_cases = 400)]
fn an_edits_recorded_form_redoes_it(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let start = drawn_lib(&tc);
    let edits: Vec<Edit> =
        (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3))).map(|_| draw_model_edit(&tc)).collect();
    let Ok((after, recorded)) = resolve_all(&start, &edits, false) else { return };
    tc.note(&format!("recorded: {recorded:?}"));
    for lenient in [false, true] {
        let (replayed, again) = resolve_all(&start, &recorded, lenient).unwrap();
        assert_eq!(replayed, after, "replaying {recorded:?} (lenient: {lenient})");
        assert_eq!(again, recorded, "a recorded edit records itself");
    }
}

/// Regression: an edit that changed nothing (adding tracks a playlist has, removing ones it
/// lacks, deleting every history list of a drive with none) was recorded with no tracks or lists,
/// which a replay refused.
#[test]
fn edits_that_change_nothing_record_nothing() {
    let mut start = lib();
    start.playlists[1].track_ids = vec![1, 2];
    let ids = |ids: Vec<u32>| TrackSelector { ids, ..Default::default() };
    for edit in [
        Edit::AddTracks {
            playlist: "Fri".into(),
            tracks: ids(vec![2]),
            position: None,
            sort: None,
            allow_repeats: false,
        },
        Edit::RemoveTracks { playlist: "Fri".into(), tracks: Some(ids(vec![5])), positions: vec![] },
        Edit::DeleteHistory { names: vec![], all: true },
    ] {
        let (after, recorded) = resolve_all(&start, std::slice::from_ref(&edit), false).unwrap();
        assert_eq!((after, recorded), (start.clone(), vec![]), "{edit:?}");
    }
}

/// Verification passes a database that reads back as its model and names the first kind of
/// difference otherwise: a track's details, an extra track, a playlist's entries, an extra
/// playlist, the order of a folder's children, a history list's entries, or an extra list.
#[hegel::test(test_cases = 200)]
fn verification_names_the_first_difference_it_finds(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let expected = drawn_lib(&tc);
    let mut actual = expected.clone();
    let kind = tc.draw(gs::integers::<u8>().max_value(7));
    let want = match kind {
        0 => None,
        1 => {
            let last = actual.tracks.len() - 1;
            actual.tracks[tc.draw(gs::integers::<usize>().max_value(last))].title.push_str(" (edit)");
            Some("track ")
        }
        2 => {
            actual.tracks.push(track(9, "B", "Extra", "/Contents/extra.mp3"));
            Some("6 tracks expected, 7 read back")
        }
        3 => {
            actual.playlists[1].track_ids.push(9);
            Some("playlist Sets/Fri:")
        }
        4 => {
            let id = 40;
            actual.playlists.push(Playlist {
                id,
                parent_id: Some(1),
                name: "Sat".into(),
                is_folder: false,
                sort_order: 9,
                track_ids: vec![],
            });
            Some("3 playlists expected, 4 read back")
        }
        5 => {
            (actual.playlists[0].sort_order, actual.playlists[2].sort_order) = (1, 0);
            Some("playlist order differs")
        }
        6 if !expected.history.is_empty() => {
            actual.history[0].track_ids.push(5);
            Some("history list ")
        }
        _ => {
            actual.history.push(Playlist {
                id: 50,
                parent_id: None,
                name: "HISTORY 009".into(),
                is_folder: false,
                sort_order: 50,
                track_ids: vec![1],
            });
            Some("history lists expected")
        }
    };
    match (verify(&expected, &actual), want) {
        (Ok(()), None) => {}
        (Err(e), Some(w)) => assert!(e.starts_with(w) || e.contains(w), "{e:?} for {w:?}"),
        (got, want) => panic!("{got:?} where {want:?} was due"),
    }
}

/// Playlists nest 64 levels at most: making one, or moving a folder with everything below it,
/// is refused exactly when it would go deeper. A move never gives a playlist a sibling of its
/// exact name.
#[hegel::test(test_cases = 40)]
fn playlists_nest_no_deeper_than_a_drive_reads_and_moves_make_no_namesakes(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let chain = |prefix: &str, n: usize| (0..n).map(|i| format!("{prefix}{i}")).collect::<Vec<_>>().join("/");
    let depth = tc.draw(gs::integers::<usize>().min_value(50).max_value(MAX_DEPTH + 2));
    let made = resolve_all(&lib(), &[Edit::CreatePlaylist { path: chain("L", depth), folder: false }], false);
    assert_eq!(made.is_ok(), depth <= MAX_DEPTH, "a playlist {depth} levels deep");
    if let Err(e) = made {
        assert!(e.to_string().contains("levels deep"), "{e}");
    }

    // A folder `height` levels tall (it and what's below it) moved into one `at` levels down.
    let (at, height) = (
        tc.draw(gs::integers::<usize>().min_value(1).max_value(60)),
        tc.draw(gs::integers::<usize>().min_value(1).max_value(10)),
    );
    let edits = [
        Edit::CreatePlaylist { path: chain("A", at), folder: true },
        Edit::CreatePlaylist { path: format!("Tall/{}", chain("T", height - 1)), folder: true },
        Edit::MovePlaylist { playlist: "Tall/".into(), into: Some(chain("A", at)), position: None },
    ];
    let moved = resolve_all(&lib(), &edits, false);
    assert_eq!(moved.is_ok(), at + height <= MAX_DEPTH, "{height} levels into one {at} down: {moved:?}");

    // Into Sets, which holds Fri, from the top level.
    let name = tc.draw(gs::sampled_from(vec!["Fri", "fri", "Sat"]));
    let mut start = lib();
    start.playlists.push(Playlist {
        id: 8,
        parent_id: None,
        name: name.into(),
        is_folder: false,
        sort_order: 5,
        track_ids: vec![],
    });
    let op = Op::Place { node: vec![name.into()], folder: false, parent: vec!["Sets".into()], position: Some(0) };
    let moved = apply_op(&mut ModelDb { lib: start, pdb: None }, &op, true, "test", &mut Vec::new());
    match name {
        "Fri" => assert!(moved.is_err_and(|e| e.to_string().contains("already in")), "a namesake next to Fri"),
        _ => assert!(moved.is_ok(), "{name} goes next to Fri"),
    }
}

/// A fill that makes its playlist when it's missing takes the playlist at exactly its path; when
/// none is there and several paths match ignoring case, which was meant can't be told and it's
/// refused naming them; otherwise it makes the playlist at exactly its path.
#[hegel::test(test_cases = 100)]
fn a_fill_takes_exactly_its_path_or_says_which_ones_it_could_mean(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut start = lib();
    let folders: Vec<&str> = tc.draw(gs::subsequences(&["GIGS", "Gigs", "gIGS"]));
    for (i, name) in folders.iter().enumerate() {
        let id = 10 + 2 * u32::try_from(i).unwrap();
        start.playlists.push(Playlist {
            id,
            parent_id: None,
            name: (*name).into(),
            is_folder: true,
            sort_order: id,
            track_ids: vec![],
        });
        start.playlists.push(Playlist {
            id: id + 1,
            parent_id: Some(id),
            name: "Warmup".into(),
            is_folder: false,
            sort_order: 0,
            track_ids: vec![1],
        });
    }
    let query = tc.draw(gs::sampled_from(vec!["gigs", "Gigs", "GIGS"]));
    let fill = Edit::SetTracks {
        playlist: format!("{query}/Warmup"),
        tracks: TrackSelector { ids: vec![5], ..Default::default() },
        sort: None,
        limit: None,
        create: true,
    };
    let got = resolve_all(&start, &[fill], false);
    let exact = folders.contains(&query);
    match got {
        Ok((l, _)) => {
            assert!(exact || folders.len() < 2, "{query} matched {folders:?} ignoring case");
            let p = l.playlists.iter().find(|p| p.track_ids == [5]).expect("a filled playlist");
            assert_eq!(l.playlist_path(p.id), format!("{query}/Warmup"));
            assert_eq!(l.playlists.len(), start.playlists.len() + if exact { 0 } else { 2 });
        }
        Err(e) => {
            assert!(!exact && folders.len() >= 2, "{e}");
            for f in &folders {
                assert!(e.to_string().contains(&format!("\"{f}/Warmup\"")), "{e}");
            }
        }
    }
}

/// Deleting history lists recorded by name and entries takes, for each, the first list of that
/// name and those entries not taken already. Strictly each must be there; leniently (a replay)
/// the ones that are go, unless none is.
#[hegel::test(test_cases = 150)]
fn recorded_history_lists_are_deleted_by_name_and_entries(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let start = drawn_lib(&tc);
    let files = |ids: &[u32]| -> Vec<String> {
        ids.iter().map(|id| start.track(*id).map_or(String::new(), |t| t.file_path.clone())).collect()
    };
    let wanted: Vec<HistoryList> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)))
        .map(|_| match start.history.is_empty() || tc.draw(gs::weighted_booleans(0.3)) {
            true => HistoryList { name: "HISTORY 009".into(), tracks: files(&[1]) },
            false => {
                let h = &tc.draw(gs::sampled_from(&start.history));
                HistoryList { name: h.name.clone(), tracks: files(&h.track_ids) }
            }
        })
        .collect();
    // The model: each wanted list takes the first matching one not taken.
    let mut taken: Vec<u32> = Vec::new();
    let mut missing = 0;
    for w in &wanted {
        match start
            .history
            .iter()
            .find(|h| h.name == w.name && files(&h.track_ids) == w.tracks && !taken.contains(&h.id))
        {
            Some(h) => taken.push(h.id),
            None => missing += 1,
        }
    }
    let edit = Edit::DeleteHistoryLists { lists: wanted.clone() };
    for lenient in [false, true] {
        let got = resolve_all(&start, std::slice::from_ref(&edit), lenient);
        let ok = if lenient { !taken.is_empty() } else { missing == 0 };
        assert_eq!(got.is_ok(), ok, "{wanted:?} (lenient: {lenient}): {got:?}");
        if let Ok((l, _)) = got {
            let left: Vec<u32> = l.history.iter().map(|h| h.id).collect();
            let want: Vec<u32> = start.history.iter().map(|h| h.id).filter(|id| !taken.contains(id)).collect();
            assert_eq!(left, want);
        }
    }
}

/// A track is pointed only at an audio file on the drive that no other track has, and only when
/// no other track shares its own file (the databases' edits find it by its file).
#[hegel::test(test_cases = 100)]
#[hegel::explicit_test_case(shared = false, id = 1u32, path = "/contents/NEW.mp3")]
fn a_track_is_pointed_only_at_an_audio_file_of_its_own(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (dir, device) = empty_drive();
    std::fs::create_dir_all(dir.path().join("Contents")).unwrap();
    std::fs::write(dir.path().join("Contents/new.mp3"), b"audio").unwrap();
    std::fs::write(dir.path().join("Contents/._new.mp3"), b"resource fork").unwrap();
    std::fs::write(dir.path().join("Contents/2.mp3"), b"audio").unwrap();
    let mut start = lib();
    let shared = tc.draw(gs::booleans());
    if shared {
        start.tracks.push(track(6, "A", "Again", "/Contents/1.mp3"));
    }
    let id = tc.draw(gs::integers::<u32>().min_value(1).max_value(2));
    let path = tc.draw(gs::sampled_from(vec![
        "/Contents/new.mp3",
        "/contents/NEW.mp3",
        "/Contents/missing.mp3",
        "/Contents/._new.mp3",
        "/Contents/2.mp3",
        "/PIONEER/rekordbox/export.pdb",
        "/../new.mp3",
    ]));
    let extra = DriveContext::default();
    let r = Resolver::new(&device, &extra, false, Ctx::silent());
    let edit = Edit::SetPath {
        track: TrackSelector { ids: vec![id], ..Default::default() },
        path: path.into(),
        file: None,
        shift_ms: 0,
    };
    let got = r.resolve(&edit, &start);
    let on_drive = path.eq_ignore_ascii_case("/Contents/new.mp3");
    let others = path == "/Contents/2.mp3" && id != 2;
    let own = path == "/Contents/2.mp3" && id == 2;
    match got {
        Ok((ops, _, _)) => {
            assert!((on_drive || own) && !(shared && id == 1), "pointed {id} at {path}");
            // At the file as the drive spells it, whatever case it was given in.
            let want = if on_drive { "/Contents/new.mp3" } else { path };
            assert!(matches!(ops.as_slice(), [Op::SetPath { path: p, .. }] if p == want), "{ops:?}");
        }
        Err(e) => {
            let why = e.to_string();
            match () {
                _ if !on_drive && !others && !own => assert!(why.contains("on the drive"), "{why}"),
                _ if others => assert!(why.contains("is the audio file of"), "{why}"),
                _ => assert!(shared && id == 1 && why.contains("several tracks"), "{why}"),
            }
        }
    }
}

/// A track edit is refused exactly when it changes nothing, gives a rating past 5, or names a
/// colour or key that isn't one; one that's taken gives every selected track what it sets.
#[hegel::test(test_cases = 200)]
#[hegel::explicit_test_case(rating = Some(6u8), color = None::<&str>, key = None::<&str>)]
#[hegel::explicit_test_case(rating = Some(5u8), color = Some("plaid"), key = Some("8A"))]
#[hegel::explicit_test_case(rating = None::<u8>, color = Some("red"), key = Some("zz"))]
fn track_edits_take_only_ratings_colours_and_keys_there_are(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let rating = tc.draw(gs::optional(gs::integers::<u8>()));
    let color = tc.draw(gs::optional(gs::sampled_from(vec!["red", "none", "3", "plaid", "9"])));
    let key = tc.draw(gs::optional(gs::sampled_from(vec!["8A", "Am", " ", "zz", "13B"])));
    let fields =
        TrackFields { rating, color: color.map(String::from), key: key.map(String::from), ..Default::default() };
    let ids = vec![1, 3];
    let got = run(Edit::EditTracks { tracks: TrackSelector { ids: ids.clone(), ..Default::default() }, fields });
    let valid = (rating.is_some() || color.is_some() || key.is_some())
        && rating.is_none_or(|r| r <= 5)
        && color.is_none_or(|c| matches!(c, "red" | "none" | "3"))
        && key.is_none_or(|k| matches!(k, "8A" | "Am" | " "));
    assert_eq!(got.is_ok(), valid, "{rating:?} {color:?} {key:?}: {got:?}");
    if let Ok(l) = got {
        for t in l.tracks.iter().filter(|t| ids.contains(&t.id)) {
            assert_eq!(t.rating, rating.unwrap_or(0));
        }
    }
}

/// An operation naming what a database lacks (a playlist, folder, track or history list) is
/// refused on the database edits are planned on, and on the other skipped, said and leaving it as
/// it was. Entries can't be set on a folder on either.
#[hegel::test(test_cases = 60)]
fn ops_naming_what_a_database_lacks_are_refused_or_skipped(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let name = || vec![tc.draw(gs::text().alphabet("xyz").min_size(1).max_size(4))];
    let track = || format!("contents/{}.mp3", tc.draw(gs::text().alphabet("xyz").min_size(1).max_size(4)));
    let folder = tc.draw(gs::booleans());
    let fields = Box::new(TrackFields { rating: Some(3), ..Default::default() });
    let lacking = vec![
        Op::Create { parent: name(), name: "New".into(), folder },
        Op::Create { parent: vec!["Sets".into(), "Fri".into()], name: "New".into(), folder },
        Op::Rename { node: name(), folder, name: "New".into() },
        Op::Place { node: name(), folder, parent: Vec::new(), position: None },
        Op::Place { node: vec!["Loose".into()], folder: false, parent: name(), position: None },
        Op::Delete { node: name(), folder },
        Op::SetEntries { node: name(), tracks: Vec::new(), before: None },
        Op::SetEntries { node: vec!["Loose".into()], tracks: vec![track()], before: None },
        Op::SetFields { tracks: vec![track()], fields },
        Op::DeleteTracks { tracks: vec![track()] },
        Op::Repoint { from: track(), to: "contents/1.mp3".into() },
        Op::SetPath { track: track(), path: "/Contents/9.mp3".into(), file: None, shift_ms: 0 },
        Op::DeleteHistory { lists: vec![(name()[0].clone(), 0)] },
    ];
    // Entries for the playlist the last of them sets: what's there of the tracks it names.
    let loose_after = |l: &Library| l.find_playlist("Loose").unwrap().track_ids.clone();
    for op in &lacking {
        let mut strict = ModelDb { lib: lib(), pdb: None };
        let err = apply_op(&mut strict, op, true, "export.pdb", &mut Vec::new()).unwrap_err();
        assert!(matches!(err, Error::NotFound(_)), "{op:?}: {err}");
        let mut other = ModelDb { lib: lib(), pdb: None };
        let mut warnings = Vec::new();
        apply_op(&mut other, op, false, "exportLibrary.db", &mut warnings).unwrap();
        assert_eq!(warnings.len(), 1, "{op:?}: {warnings:?}");
        assert!(warnings[0].starts_with("exportLibrary.db: ") && warnings[0].ends_with("; skipped there"));
        match op {
            Op::SetEntries { node, .. } if node[0] == "Loose" => assert!(loose_after(&other.lib).is_empty()),
            _ => assert_eq!(other.lib, lib(), "{op:?}"),
        }
    }
    for strict in [true, false] {
        let mut db = ModelDb { lib: lib(), pdb: None };
        let op = Op::SetEntries { node: vec!["Sets".into()], tracks: Vec::new(), before: None };
        let err = apply_op(&mut db, &op, strict, "export.pdb", &mut Vec::new()).unwrap_err();
        assert!(err.to_string().contains("is a folder"), "{err}");
    }
}

/// An edit that names a playlist by a path of no names (only slashes and spaces) is refused for
/// it, whether it fills, makes or orders into one, as is an edit that adds at position 0 or
/// points several tracks at one file; the library is left as it was.
#[hegel::test(test_cases = 30)]
fn edits_naming_no_playlist_position_or_track_are_refused(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let path = tc.draw(gs::text().alphabet(" /").max_size(4));
    let ids = || TrackSelector { ids: vec![1, 2], ..Default::default() };
    let several = TrackSelector {
        ids: tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(5)).min_size(2).max_size(4).unique(true)),
        ..Default::default()
    };
    let cases = [
        (Edit::CreatePlaylist { path: path.clone(), folder: tc.draw(gs::booleans()) }, "give a playlist name"),
        (
            Edit::SetTracks { playlist: path.clone(), tracks: ids(), sort: None, limit: None, create: true },
            "give a playlist name",
        ),
        (
            Edit::SetEntries { playlist: path.clone(), paths: vec!["/Contents/1.mp3".into()], create: true },
            "give a playlist name",
        ),
        (Edit::PlanSet { playlist: "Sets/Fri".into(), start: None, into: Some(path.clone()) }, "give a playlist name"),
        (
            Edit::AddTracks {
                playlist: "Loose".into(),
                tracks: ids(),
                position: Some(0),
                sort: None,
                allow_repeats: true,
            },
            "positions start at 1",
        ),
        (
            Edit::SetPath { track: several, path: "/Contents/new.mp3".into(), file: None, shift_ms: 0 },
            "select exactly one track",
        ),
        (Edit::MovePlaylist { playlist: "Loose".into(), into: None, position: Some(0) }, "positions start at 1"),
    ];
    for (edit, why) in cases {
        let err = run(edit.clone()).unwrap_err();
        assert!(err.to_string().contains(why), "{edit:?}: {err}");
    }
}

/// Removing tracks from a playlist, by track and by position, takes exactly the entries named
/// (every entry of a track named, one of a repeated track at a position) and keeps the rest in
/// order; removing none records nothing, and what's recorded removes the same again.
#[hegel::test(test_cases = 200)]
fn removing_tracks_takes_exactly_the_entries_named(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let start = drawn_lib(&tc);
    let old = entries(&start, "Sets/Fri");
    let tracks = tc
        .draw(gs::optional(gs::vecs(gs::integers::<u32>().min_value(1).max_value(6)).min_size(1).max_size(2)))
        .map(|ids| TrackSelector { ids, ..Default::default() });
    let positions: Vec<usize> = match old.len() {
        0 => Vec::new(),
        n => tc.draw(gs::vecs(gs::integers::<usize>().min_value(1).max_value(n)).max_size(2)),
    };
    let edit = Edit::RemoveTracks { playlist: "Sets/Fri".into(), tracks: tracks.clone(), positions: positions.clone() };
    let got = resolve_all(&start, std::slice::from_ref(&edit), false);
    if tracks.is_none() && positions.is_empty() {
        assert!(got.is_err());
        return;
    }
    let (after, recorded) = got.unwrap();
    let named: Vec<u32> = tracks.map(|t| t.ids).unwrap_or_default();
    let want: Vec<u32> = old
        .iter()
        .enumerate()
        .filter(|(i, t)| !positions.contains(&(i + 1)) && !named.contains(t))
        .map(|(_, t)| *t)
        .collect();
    assert_eq!(entries(&after, "Sets/Fri"), want);
    assert_eq!(recorded.is_empty(), want == old, "{recorded:?}");
    let (again, _) = resolve_all(&start, &recorded, false).unwrap();
    assert_eq!(entries(&again, "Sets/Fri"), want);
}

/// Whether `s` is an ISRC, told by each character's place: two letters, three letters or
/// digits, seven digits.
fn isrc_model(s: &str) -> bool {
    s.chars().count() == 12
        && s.chars().enumerate().all(|(i, c)| match i {
            0 | 1 => c.is_ascii_uppercase(),
            2..=4 => c.is_ascii_uppercase() || c.is_ascii_digit(),
            _ => c.is_ascii_digit(),
        })
}

/// A track edit setting a BPM, an ISRC, a release date or an album artist is refused, saying
/// which, exactly when the BPM is outside 1 to 999.99, the ISRC or date (given) isn't one written
/// out in full, or an album artist would be left on a track without an album; one that's taken
/// gives every selected track the BPM to hundredths, the ISRC and date as given, and the album
/// artist only with an album.
#[hegel::test(test_cases = 300)]
#[hegel::explicit_test_case(bpm = Some(999.994f64), isrc = Some("GB1A30000059".to_string()), date = Some("2019-05-01".to_string()), album = None::<String>, album_artist = None::<String>, ids = vec![1u32, 2])]
#[hegel::explicit_test_case(bpm = None::<f64>, isrc = None::<String>, date = Some("2019-02-29".to_string()), album = None::<String>, album_artist = None::<String>, ids = vec![1u32, 2])]
#[hegel::explicit_test_case(bpm = None::<f64>, isrc = None::<String>, date = Some("0000-01-1".to_string()), album = None::<String>, album_artist = None::<String>, ids = vec![1u32])]
#[hegel::explicit_test_case(bpm = None::<f64>, isrc = Some("GBDUW000005".to_string()), date = None::<String>, album = None::<String>, album_artist = None::<String>, ids = vec![1u32, 2])]
#[hegel::explicit_test_case(bpm = None::<f64>, isrc = None::<String>, date = None::<String>, album = Some(" ".to_string()), album_artist = Some("Bicep".to_string()), ids = vec![2u32])]
fn track_edits_take_only_tempos_codes_dates_and_album_artists_there_are(tc: hegel::TestCase) {
    use hegel::Generator;
    use hegel::generators as gs;
    let bpm = tc.draw(gs::optional(hegel::one_of!(
        gs::sampled_from(vec![0.0, 0.999, 1.0, 999.99, 999.994, 999.996, 1000.0, f64::NAN, -1.0]),
        gs::floats::<f64>().min_value(-10.0).max_value(1100.0)
    )));
    let isrc = tc.draw(gs::optional(hegel::one_of!(
        gs::sampled_from(vec!["", "GBDUW0000059", "GB1A30000059", "gbduw0000059", "GBDUW000005", "GBDUW00000590"])
            .map(String::from),
        gs::from_regex("[A-Z0-9a]{2}[A-Z0-9-]{3}[0-9A]{6,8}").fullmatch(true),
        gs::text().max_size(13)
    )));
    let date = tc.draw(gs::optional(hegel::one_of!(
        gs::sampled_from(vec!["", "2019-05-01", "2019-5-1", "2019-02-29", "2020-02-29", "20190501", "2019-13-01"])
            .map(String::from),
        gs::from_regex("[0-9]{2,5}-[0-9]{1,3}-[0-9]{1,3}").fullmatch(true),
        gs::text().max_size(11)
    )));
    let album = tc.draw(gs::optional(gs::sampled_from(vec!["", " ", "Isles"]).map(String::from)));
    let album_artist = tc.draw(gs::optional(gs::sampled_from(vec!["", " ", "Bicep"]).map(String::from)));
    let ids: Vec<u32> =
        tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(5)).min_size(1).max_size(3).unique(true));
    // Tracks 2 and 4 have an album already.
    let mut start = lib();
    for t in start.tracks.iter_mut().filter(|t| t.id % 2 == 0) {
        t.album = "Rise".into();
        t.album_artist = "Four Tet".into();
    }
    let fields = TrackFields {
        bpm,
        isrc: isrc.clone(),
        release_date: date.clone(),
        album: album.clone(),
        album_artist: album_artist.clone(),
        ..Default::default()
    };
    let edit = Edit::EditTracks { tracks: TrackSelector { ids: ids.clone(), ..Default::default() }, fields };
    let got = run_on(start.clone(), edit);

    let full_date =
        |d: &str| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok_and(|x| x.format("%Y-%m-%d").to_string() == d);
    let blank = |s: &Option<String>| s.as_deref().is_none_or(|s| s.trim().is_empty());
    let albumless = ids.iter().any(|id| match &album {
        Some(a) => a.trim().is_empty(),
        None => id % 2 == 1,
    });
    let refusal = if bpm.is_none() && isrc.is_none() && date.is_none() && album.is_none() && album_artist.is_none() {
        Some("give at least one field")
    } else if bpm.is_some_and(|b| !(1.0..=999.99).contains(&b)) {
        Some("BPM goes from 1 to 999.99")
    } else if isrc.as_deref().is_some_and(|i| !i.is_empty() && !isrc_model(i)) {
        Some("isn't an ISRC")
    } else if date.as_deref().is_some_and(|d| !d.is_empty() && !full_date(d)) {
        Some("isn't a release date (YYYY-MM-DD)")
    } else if !blank(&album_artist) && albumless {
        Some("has no album to keep an album artist with")
    } else {
        None
    };
    let after = match (refusal, got) {
        (Some(why), Err(e)) => return assert!(e.to_string().contains(why), "{why}: {e}"),
        (None, Ok(after)) => after,
        (why, got) => panic!("expected {why:?}: {got:?}"),
    };
    for (t, was) in after.tracks.iter().zip(&start.tracks) {
        if !ids.contains(&t.id) {
            assert_eq!(t, was);
            continue;
        }
        assert_eq!(t.bpm, bpm.map_or(was.bpm, |b| (b * 100.0).round() / 100.0));
        assert_eq!(t.isrc, isrc.clone().unwrap_or_else(|| was.isrc.clone()));
        assert_eq!(t.release_date, date.clone().unwrap_or_else(|| was.release_date.clone()));
        let name = |v: &Option<String>, was: &str| match v {
            Some(v) if v.trim().is_empty() => String::new(),
            Some(v) => v.clone(),
            None => was.to_string(),
        };
        assert_eq!(t.album, name(&album, &was.album));
        let artist =
            if t.album.is_empty() && album.is_some() { String::new() } else { name(&album_artist, &was.album_artist) };
        assert_eq!(t.album_artist, artist);
    }
}
