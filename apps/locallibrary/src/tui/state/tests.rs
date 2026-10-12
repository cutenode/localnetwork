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
        sort: tc.draw(gs::booleans()).then(|| SortKey { field: SortField::Bpm, descending: tc.draw(gs::booleans()) }),
        search_all: tc.draw(gs::booleans()),
        volume: tc.draw(gs::booleans()).then(|| tc.draw(gs::integers::<u8>().max_value(150)) as f32 / 100.0),
        vis: maybe(word()),
        details: tc.draw(gs::booleans()),
        repeat: maybe(word()),
        quantize: tc.draw(gs::booleans()).then(|| tc.draw(gs::booleans())),
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
