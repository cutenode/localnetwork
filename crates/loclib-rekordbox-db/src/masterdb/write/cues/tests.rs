use super::*;

/// An id that's taken is passed over for the next one drawn.
#[test]
fn a_cue_id_thats_taken_is_drawn_again() {
    let mut drawn = ["7", "8"].into_iter().map(|s| Ok(s.to_string()));
    let id = first_free(|| drawn.next().unwrap(), |id| Ok(id == "7")).unwrap();
    assert_eq!(id, "8");
}

/// Reals are written as JSON numbers, and a time that isn't rekordbox's (another tool's, with its
/// zone's name) stays as it is.
#[test]
fn a_list_holds_reals_and_odd_times_as_they_are() {
    let odd = "2025-04-12 19:11:29.274 -05:00 (Central Daylight Time)";
    let row = vec![Value::Real(1.5), Value::Text(odd.into()), Value::Real(f64::NAN)];
    let json = cue_list_json(&["BeatLoopSize", "created_at", "CueMicrosec"], &[row]);
    assert_eq!(json, format!(r#"[{{"BeatLoopSize":1.5,"created_at":"{odd}","CueMicrosec":null}}]"#));
}

/// A stored value goes to JSON and back; a blob, or JSON no column holds, is refused.
#[test]
fn stored_values_go_to_json_and_back() {
    for v in [Value::Null, Value::Integer(-4), Value::Real(2.25), Value::Text("ü".into())] {
        assert_eq!(from_json(&to_json(&v).unwrap()).unwrap(), v);
    }
    assert_eq!(to_json(&Value::Real(f64::INFINITY)).unwrap(), serde_json::Value::Null);
    assert!(matches!(to_json(&Value::Blob(vec![1])), Err(Error::Invalid(_))));
    assert_eq!(from_json(&serde_json::json!(u64::MAX)).unwrap(), Value::Real(u64::MAX as f64));
    for odd in [serde_json::json!(true), serde_json::json!([1]), serde_json::json!({"a": 1})] {
        assert!(matches!(from_json(&odd), Err(Error::Invalid(_))), "{odd}");
    }
}

/// Tables without `rb_local_deleted` are read whole; a list is the one its rows make, or it
/// disagrees; a track with two lists disagrees; a database without `contentCue` has nothing to
/// disagree.
#[test]
fn mismatches_in_tables_of_other_shapes() {
    let conn = Connection::open_in_memory().unwrap();
    assert_eq!(mismatches(&conn, true, None).unwrap(), Vec::<String>::new());
    conn.execute_batch(
        "CREATE TABLE djmdContent (ID TEXT);
         CREATE TABLE djmdCue (ID TEXT, ContentID TEXT, InMsec INTEGER, OutMsec INTEGER, Kind INTEGER);
         CREATE TABLE contentCue (ContentID TEXT, Cues TEXT, rb_cue_count INTEGER);
         INSERT INTO djmdContent VALUES ('1'), ('2'), ('3'), ('4');
         INSERT INTO djmdCue VALUES ('a', '1', 10, -1, 0), ('b', '2', 20, -1, 0), ('c', '4', 30, -1, 0);
         INSERT INTO contentCue VALUES ('1', '[{\"ID\":\"a\",\"ContentID\":\"1\",\"InMsec\":10,\"OutMsec\":-1,\"Kind\":0}]', 1),
             ('2', '[]', 0), ('2', '[]', 0),
             ('4', '[{\"ID\":\"c\",\"InMsec\":30,\"OutMsec\":-1,\"Kind\":0}]', 1);",
    )
    .unwrap();
    assert_eq!(mismatches(&conn, false, None).unwrap(), ["2", "4"]);
    let only: FxHashSet<String> = ["1".to_string()].into_iter().collect();
    assert_eq!(mismatches(&conn, false, Some(&only)).unwrap(), Vec::<String>::new());
}
