use super::*;

/// `Serato Markers2` of serato-tags' `data/id3/hotcues-with-names` test file (MIT), written by
/// Serato DJ: white, three named hot cues, beat grid unlocked.
const HOTCUES_WITH_NAMES: &str = "AQFDT0xPUgAAAAAEAP///0NVRQAAAAAaAAAAAAAAAMwAAAAASGVsbG8sIFdvcmxkIQBDVUUA\n\
        AAAAFQABAANVWADMiAAAAMOkw7bDvMOfAENVRQAAAAATAAIAAOpkAAAAzAAAw6nDqMOqAEJQ\nTUxPQ0sAAAAAAQAA";

#[test]
fn markers_encode_like_serato() {
    let m = Markers {
        colour: [0xff, 0xff, 0xff],
        cues: vec![
            (0, 0, [0xcc, 0x00, 0x00], "Hello, World!".into()),
            (1, 0x0003_5558, [0xcc, 0x88, 0x00], "äöüß".into()),
            (2, 0xea64, [0x00, 0x00, 0xcc], "éèê".into()),
        ],
        ..Default::default()
    };
    let mut want = vec![1, 1];
    want.extend_from_slice(HOTCUES_WITH_NAMES.as_bytes());
    want.resize(470, 0);
    assert_eq!(m.encode(), want);
}

#[test]
fn rekordbox_cues_map_to_pads_and_loop_slots() {
    let cue = |hot_cue, start_ms, end_ms: Option<u32>, color| Cue {
        hot_cue,
        start_ms,
        end_ms,
        comment: String::new(),
        color,
        color_id: 0,
        loop_beats: None,
        active_loop: false,
    };
    let m = Markers::from_rekordbox(
        Some(TrackColor::Red),
        &[
            cue(None, 100, None, None),
            cue(None, 200, Some(400), None),
            cue(Some(0), 1000, None, Some([0xe6, 0x28, 0x28])),
            cue(Some(4), 2000, Some(2500), None),
            cue(Some(9), 3000, None, None),
            cue(Some(2), 4000, None, None),
        ],
    );
    assert_eq!(m.colour, [0xff, 0x99, 0x99]);
    assert_eq!(m.cues, [(0, 1000, [0xcc, 0x00, 0x00], String::new()), (2, 4000, PAD_COLOURS[2], String::new())]);
    assert_eq!(m.loops, [(0, 200, 400, String::new()), (4, 2000, 2500, String::new())]);
    assert_eq!(m.left_out, 2, "the memory cue and pad 10");
}

/// Serato has eight saved loop slots: memory loops past them are left out, and each track
/// colour is the pastel Serato's picker stores.
#[test]
fn loops_past_the_eighth_slot_are_left_out() {
    let memory_loop = |i: u32| Cue {
        hot_cue: None,
        start_ms: i * 1000,
        end_ms: Some(i * 1000 + 500),
        comment: String::new(),
        color: None,
        color_id: 0,
        loop_beats: None,
        active_loop: false,
    };
    let m = Markers::from_rekordbox(None, &(0..9).map(memory_loop).collect::<Vec<_>>());
    assert_eq!(m.loops.iter().map(|l| l.0).collect::<Vec<_>>(), (0..8).collect::<Vec<u8>>());
    assert_eq!((m.left_out, m.colour), (1, [0xff, 0xff, 0xff]));
    use TrackColor::*;
    let pastels = [Pink, Red, Orange, Yellow, Green, Aqua, Blue, Purple].map(|c| track_colour(Some(c)));
    assert!(pastels.iter().all(|[r, g, b]| r.min(g).min(b) == &0x99 && r.max(g).max(b) == &0xff), "{pastels:x?}");
    assert_eq!(pastels.iter().collect::<rustc_hash::FxHashSet<_>>().len(), 8);
}

/// Crate names are made safe for a file name, `%%` (Serato's folder separator) can't come
/// from a playlist's own name, long ones are cut on a character boundary, and same-named
/// crates are told apart.
#[test]
fn crate_names_are_safe_unique_and_short_enough() {
    let node = |id, parent_id, name: &str, is_folder| Playlist {
        id,
        parent_id,
        name: name.into(),
        is_folder,
        sort_order: id,
        track_ids: Vec::new(),
    };
    let long = "ü".repeat(150);
    let lib = Library {
        playlists: vec![
            node(1, None, "50%% off", true),
            node(2, Some(1), "Set", false),
            node(3, None, "set", false),
            node(4, None, "Set", false),
            node(5, None, &long, false),
        ],
        ..Default::default()
    };
    let names: Vec<(u32, String)> = crate_names(&lib).into_iter().map(|(p, n)| (p.id, n)).collect();
    assert_eq!(
        &names[..4],
        [(1, "50% off".to_string()), (2, "50% off%%Set".into()), (3, "set".into()), (4, "Set (2)".into())]
    );
    assert!(names[4].1.len() <= NAME_BYTES && long.starts_with(&names[4].1), "{}", names[4].1);
}

/// Cutting a long name never drops a level of nesting: a subcrate keeps its folder's whole
/// crate name, `%%` and the start of its own, however long or deep the folders above it.
#[test]
fn cut_names_keep_their_nesting() {
    let node = |id, parent_id, name: String, is_folder| Playlist {
        id,
        parent_id,
        name,
        is_folder,
        sort_order: id,
        track_ids: Vec::new(),
    };
    let mut playlists = vec![node(1, None, "f".repeat(198), true), node(2, Some(1), "a".repeat(60), false)];
    // Thirty folders of 40 bytes, one in the next, the last holding a playlist.
    playlists.extend((0..30).map(|i| node(10 + i, (i > 0).then_some(9 + i), "ü".repeat(20), true)));
    playlists.push(node(40, Some(39), "Set".into(), false));
    let lib = Library { playlists, ..Default::default() };
    let names: rustc_hash::FxHashMap<u32, String> = crate_names(&lib).into_iter().map(|(p, n)| (p.id, n)).collect();
    for p in &lib.playlists {
        let name = &names[&p.id];
        let own = match p.parent_id {
            Some(parent) => name.strip_prefix(&format!("{}%%", names[&parent])).expect("under its folder"),
            None => name,
        };
        assert!(!own.is_empty() && !own.contains("%%") && p.name.starts_with(own), "{name}");
        assert!(name.len() <= NAME_BYTES || own.chars().count() == 1, "{} bytes", name.len());
        assert!(format!("{name}.crate").len() <= 255, "{} bytes", name.len());
    }
    assert!(names[&2].starts_with(&format!("{}%%a", names[&1])), "{}", names[&2]);
}

#[test]
fn crates_and_their_paths() {
    let c = crate_file(&["Contents/A/x.mp3".into()]);
    assert_eq!(&c[..8], b"vrsn\0\0\0\x38");
    assert_eq!(&c[8..10], &[0, b'1']);
    let otrk = c.windows(4).position(|w| w == b"otrk").unwrap();
    assert_eq!(&c[otrk + 4..otrk + 8], &(8 + 32u32).to_be_bytes());
    assert_eq!(&c[otrk + 8..otrk + 12], b"ptrk");
    assert_eq!(&c[otrk + 16..], utf16be("Contents/A/x.mp3").as_slice());

    assert_eq!(
        volume_relative(Path::new("/Volumes/USB/Contents/x.mp3")),
        ("Contents/x.mp3".into(), PathBuf::from("/Volumes/USB"))
    );
    assert_eq!(volume_relative(Path::new(r"E:\Contents\x.mp3")).0, "Contents/x.mp3");
    assert_eq!(volume_relative(Path::new("/Users/me/Music/x.mp3")), ("Users/me/Music/x.mp3".into(), "/".into()));
    assert_eq!(cue_colour([0xe6, 0x28, 0x28]), Some([0xcc, 0x00, 0x00]));
    assert_eq!(cue_colour([0x1d, 0xbe, 0xbd]), Some([0x00, 0xcc, 0xcc]));
    assert_eq!(cue_colour([0xff, 0x37, 0x6f]), Some([0xcc, 0x00, 0x44]), "rekordbox's pink-red");
    assert_eq!(cue_colour([0x30, 0x5a, 0xff]), Some([0x00, 0x44, 0xcc]));
    assert_eq!(cue_colour([0x80, 0x80, 0x84]), None);
    for (i, c) in CUE_COLOURS.iter().enumerate() {
        assert_eq!(cue_colour(*c), Some(*c), "{i}");
    }
}
