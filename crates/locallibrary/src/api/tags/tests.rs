use super::*;

#[test]
fn requests_are_flat_json() {
    let req: TagsEdit = serde_json::from_value(serde_json::json!({
        "drive": "Main", "action": "add", "tag": "Genre/Deep House", "query": "Loopmasters", "ids": [3]
    }))
    .unwrap();
    match req.action {
        TagAction::Add { tag, tracks } => {
            assert_eq!(tag, "Genre/Deep House");
            assert_eq!(tracks.query.as_deref(), Some("Loopmasters"));
            assert_eq!(tracks.ids, [3]);
        }
        other => panic!("{other:?}"),
    }
    let req: TagsEdit = serde_json::from_value(
        serde_json::json!({ "drive": "Main", "action": "rename", "tag": "Genre", "to": "Style" }),
    )
    .unwrap();
    assert!(matches!(req.action, TagAction::Rename { .. }));
}
