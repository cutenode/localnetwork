use super::*;
use hegel::TestCase;
use hegel::generators as gs;

fn hash(tc: &TestCase) -> Hash {
    Hash::of(&tc.draw(gs::binary().max_size(8)))
}

/// Every request reads back from the JSON it's sent as, tagged with its type, and fits in the
/// request limit; text that isn't one of the three is refused.
#[hegel::test(test_cases = 300)]
fn requests_read_back(tc: TestCase) {
    let request = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => Request::Hello { token: tc.draw(gs::text().max_size(64)) },
        1 => Request::Snapshot { id: hash(&tc) },
        _ => Request::Object { hash: hash(&tc) },
    };
    let bytes = serde_json::to_vec(&request).unwrap();
    assert!(bytes.len() <= REQUEST_LIMIT);
    let back: Request = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_vec(&back).unwrap(), bytes);
    let tag = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["type"].clone();
    let want = match &request {
        Request::Hello { .. } => "hello",
        Request::Snapshot { .. } => "snapshot",
        Request::Object { .. } => "object",
    };
    assert_eq!(tag, want);
    // Only a request of a known type with what it names reads as one.
    let h = hash(&tc).to_hex();
    let (junk, ok) = tc.draw(gs::sampled_from(vec![
        (r#"{"type":"hello","token":"x"}"#.to_string(), true),
        (r#"{"type":"hello"}"#.to_string(), false),
        (format!(r#"{{"type":"object","hash":"{h}"}}"#), true),
        (format!(r#"{{"type":"snapshot","id":"{h}"}}"#), true),
        (format!(r#"{{"type":"snapshot","hash":"{h}"}}"#), false),
        (format!(r#"{{"type":"object","hash":"{}"}}"#, &h[1..]), false),
        (format!(r#"{{"type":"files","hash":"{h}"}}"#), false),
        ("hello".to_string(), false),
    ]));
    assert_eq!(serde_json::from_str::<Request>(&junk).is_ok(), ok, "{junk}");
}

/// A catalog reads back from its JSON: the same store, libraries, snapshots and labels.
#[hegel::test(test_cases = 100)]
fn catalogs_read_back(tc: TestCase) {
    let at = chrono::DateTime::from_timestamp(tc.draw(gs::integers::<i64>().min_value(0).max_value(4_000_000_000)), 0)
        .unwrap();
    let n = tc.draw(gs::integers::<usize>().max_value(3));
    let libraries: Vec<LibraryRecord> = (0..n)
        .map(|i| LibraryRecord {
            id: Uuid::from_u128(tc.draw(gs::integers::<u128>())),
            name: format!("Library {i}"),
            head: tc.draw(gs::booleans()).then(|| hash(&tc)),
            created: at,
            updated: at,
        })
        .collect();
    let snapshots: Vec<CatalogSnapshot> = libraries
        .iter()
        .filter_map(|l| l.head.map(|id| CatalogSnapshot { id, library: l.id, parents: vec![hash(&tc)] }))
        .collect();
    let labels: Vec<SnapshotLabel> =
        snapshots.iter().map(|s| SnapshotLabel { name: "gig".into(), snapshot: s.id, created: at }).collect();
    let catalog = Catalog { store: Uuid::from_u128(tc.draw(gs::integers::<u128>())), libraries, snapshots, labels };
    let bytes = serde_json::to_vec(&catalog).unwrap();
    let back: Catalog = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(back.store, catalog.store);
    assert_eq!(serde_json::to_vec(&back).unwrap(), bytes);
    assert!(bytes.len() < JSON_LIMIT);
}
