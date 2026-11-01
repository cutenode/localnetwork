use locallibrary::query::SortField;

use super::super::browse::Category;
use super::*;

#[test]
fn round_trips_beside_the_store_until_there_is_one() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let saved = Saved {
        root: Some("/Volumes/KEYCHAIN".into()),
        node: Some(SavedNode::Group { category: Category::Key, key: "8A".into() }),
        view: Some("Mixes".into()),
        sort: Some(SortKey { field: SortField::Bpm, descending: true }),
        volume: Some(0.6),
        vis: Some("deck".into()),
        details: true,
        shuffle: true,
        repeat: Some("all".into()),
        queue: Some(SavedQueue {
            root: "/Volumes/KEYCHAIN".into(),
            tracks: vec!["/Contents/a.mp3".into()],
            at_ms: 61_000,
            ..Default::default()
        }),
        ..Default::default()
    };
    save(&store, &saved);
    assert!(!store.exists(), "the store's folder isn't made by the player");
    assert!(dir.path().join("store-player").join(FILE).is_file());
    assert_eq!(load(&store), saved);

    // Once there's a store, the state lives in it (the copy beside it is only read until then).
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(store.join("store.json"), "{}").unwrap();
    assert_eq!(load(&store), saved);
    let newer = Saved { volume: Some(0.2), ..saved.clone() };
    save(&store, &newer);
    assert_eq!(load(&store), newer);
    assert!(store.join(FILE).is_file());
    std::fs::write(store.join(FILE), "not json").unwrap();
    assert_eq!(load(&store), Saved::default());

    // Never on the drive that was open.
    let drive = dir.path().join("drive");
    let on_drive = drive.join("store");
    save(&on_drive, &Saved { root: Some(drive.clone()), ..Default::default() });
    assert!(!drive.exists());
    // Nor when the store's path reaches the drive through a link (as macOS's /var is
    // /private/var): the drive's root comes resolved.
    #[cfg(unix)]
    {
        std::fs::create_dir_all(&drive).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&drive, &link).unwrap();
        save(&link.join("store"), &Saved { root: Some(drive.canonicalize().unwrap()), ..Default::default() });
        assert_eq!(std::fs::read_dir(&drive).unwrap().count(), 0);
    }
}

/// Whatever's saved reads back the same, beside the store before there is one and in it
/// after; and nothing is saved inside the drive that was open.
#[hegel::test(test_cases = 40)]
fn what_is_saved_reads_back(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    if tc.draw(gs::booleans()) {
        locallibrary::Store::open_or_init(&store).unwrap();
    }
    let word = || tc.draw(gs::text().alphabet("ab/ é").max_size(6));
    let maybe = |s: String| tc.draw(gs::booleans()).then_some(s);
    let saved = Saved {
        root: maybe(word()).map(|w| dir.path().join(format!("drives/d{w}"))),
        view: maybe(word()),
        sort: tc.draw(gs::booleans()).then(|| SortKey {
            field: match tc.draw(gs::booleans()) {
                true => SortField::Bpm,
                false => SortField::Field(locallibrary::query::FieldName::new("sound.bpm")),
            },
            descending: tc.draw(gs::booleans()),
        }),
        field_columns: (0..tc.draw(gs::integers::<usize>().max_value(2))).map(|i| format!("sound.f{i}")).collect(),
        search_all: tc.draw(gs::booleans()),
        volume: tc.draw(gs::optional(gs::integers::<u8>().max_value(150))).map(|v| v as f32 / 100.0),
        vis: maybe(word()),
        details: tc.draw(gs::booleans()),
        repeat: maybe(word()),
        quantize: tc.draw(gs::optional(gs::booleans())),
        output: maybe(word()),
        mixes: (0..tc.draw(gs::integers::<usize>().max_value(3))).map(|_| word().into()).collect(),
        queue: tc.draw(gs::booleans()).then(|| SavedQueue {
            root: word().into(),
            tracks: (0..tc.draw(gs::integers::<usize>().max_value(3))).map(|_| word()).collect(),
            pos: tc.draw(gs::integers::<usize>().max_value(3)),
            at_ms: tc.draw(gs::integers::<u64>()),
            ..Default::default()
        }),
        ..Default::default()
    };
    save(&store, &saved);
    assert_eq!(load(&store), saved);
    // Saved from a store inside the drive that was open: not written there.
    let drive = dir.path().join("drive");
    save(&drive.join("store"), &Saved { root: Some(drive.clone()), ..saved });
    assert!(!drive.exists());
}

/// A store at the top of the disk has no folder beside it to keep the player's state in: nothing
/// is saved there, and the next run starts fresh.
#[hegel::test(test_cases = 20)]
fn a_store_with_nothing_beside_it_keeps_nothing(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let root = std::path::Path::new("/");
    let saved = Saved {
        volume: Some(tc.draw(gs::sampled_from(&[0.2f32, 1.0][..]))),
        shuffle: tc.draw(gs::booleans()),
        ..Default::default()
    };
    save(root, &saved);
    assert!(!root.join(FILE).exists());
    assert_eq!(load(root), Saved::default());
}

/// `saved` with only its preferences, the part `settings export` takes. Every field of [`Saved`]
/// is named here, as a preference or as where the player was, so a new one doesn't build until
/// it's one or the other (a preference also goes in `locallibrary::settings::transfer::PlayerPrefs`,
/// or the test below fails).
fn preferences_of(saved: &Saved) -> Saved {
    let Saved {
        // Where it was: never exported.
        root: _,
        node: _,
        view: _,
        volume: _,
        details: _,
        crate_panel: _,
        output: _,
        mixes: _,
        mix_folders: _,
        crate_list: _,
        queue: _,
        // The user's preferences.
        sort,
        search_all,
        vis,
        ratings,
        tags_column,
        field_columns,
        theme,
        shuffle,
        repeat,
        quantize,
        single,
        auto_cue,
    } = saved.clone();
    Saved {
        sort,
        search_all,
        vis,
        ratings,
        tags_column,
        field_columns,
        theme,
        shuffle,
        repeat,
        quantize,
        single,
        auto_cue,
        ..Default::default()
    }
}

/// The player's preferences go out with `settings export` and come back in another store's
/// player; where it was (the drive, the list, the queue, its panels, the volume, the output)
/// never goes out.
#[hegel::test(test_cases = 30)]
fn its_preferences_go_to_another_store_and_where_it_was_stays(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use locallibrary::api;
    let dir = tempfile::tempdir().unwrap();
    let (here, there) = (dir.path().join("here"), dir.path().join("there"));
    let store = locallibrary::Store::open_or_init(&here).unwrap();
    let maybe = |s: &str| tc.draw(gs::booleans()).then(|| s.to_string());
    let on = || tc.draw(gs::booleans());
    let saved = Saved {
        root: Some(dir.path().join("drives/USB")),
        node: Some(SavedNode::Group { category: Category::Key, key: "8A".into() }),
        view: Some("Mixes".into()),
        sort: on().then_some(SortKey { field: SortField::Bpm, descending: on() }),
        search_all: on(),
        volume: Some(0.4),
        vis: maybe("deck"),
        details: true,
        crate_panel: true,
        ratings: on(),
        tags_column: on(),
        field_columns: (0..tc.draw(gs::integers::<usize>().max_value(2))).map(|i| format!("sound.f{i}")).collect(),
        theme: maybe("themes.night"),
        shuffle: on(),
        repeat: maybe("one"),
        quantize: tc.draw(gs::optional(gs::booleans())),
        output: Some("Speakers".into()),
        single: on(),
        auto_cue: on(),
        mixes: vec!["/m/a.wav".into()],
        mix_folders: vec!["/m".into()],
        crate_list: vec![SavedCrateTrack {
            root: "/Volumes/USB".into(),
            drive: "USB".into(),
            track: Default::default(),
        }],
        queue: Some(SavedQueue { root: "/Volumes/USB".into(), tracks: vec!["/a.mp3".into()], ..Default::default() }),
    };
    save(&here, &saved);
    let doc = api::settings_export(&store).unwrap();
    // Every field where it was is set, so each is one `preferences_of` leaves out.
    let (all, kept) = (serde_json::to_value(&saved).unwrap(), serde_json::to_value(preferences_of(&saved)).unwrap());
    let session: Vec<&String> =
        all.as_object().unwrap().keys().filter(|k| all[k.as_str()] != kept[k.as_str()]).collect();
    assert_eq!(session.len(), 11, "{session:?}");
    assert!(session.iter().all(|k| !doc.player.contains_key(k.as_str())), "{:?}", doc.player);
    let file = dir.path().join("settings.json");
    std::fs::write(&file, serde_json::to_vec(&doc).unwrap()).unwrap();
    let other = locallibrary::Store::open_or_init(&there).unwrap();
    api::settings_import_file(&other, &api::ImportSettingsFile { file, replace: false }).unwrap();
    assert_eq!(load(&there), preferences_of(&saved));
}
