use super::*;

fn names(one: &OneLibrary, parent: i64) -> Vec<(String, i64)> {
    one.my_tag_rows().unwrap().into_iter().filter(|r| r.parent == parent).map(|r| (r.name, r.sequence)).collect()
}

#[test]
fn categories_tags_and_links() {
    let mut one = OneLibrary::create().unwrap();
    one.add_my_tag(1, 0, "Genre", true).unwrap();
    one.add_my_tag(2, 0, "Situation", true).unwrap();
    one.add_my_tag(100, 1, "House", false).unwrap();
    one.add_my_tag(101, 1, "Techno", false).unwrap();
    one.add_my_tag(102, 1, "Disco", false).unwrap();
    one.add_my_tag(200, 2, "Opening", false).unwrap();
    assert_eq!(names(&one, 0), [("Genre".into(), 0), ("Situation".into(), 1)]);
    assert!(one.add_my_tag(101, 2, "Again", false).is_err(), "ids are unique");
    assert!(one.add_my_tag(300, 100, "Under a tag", false).is_err());
    assert!(one.link_my_tag(1, &[5]).is_err(), "categories aren't tags");

    assert_eq!(one.link_my_tag(101, &[5, 6]).unwrap(), 2);
    assert_eq!(one.link_my_tag(101, &[6, 7]).unwrap(), 1);
    one.link_my_tag(200, &[5]).unwrap();
    assert_eq!(one.unlink_my_tag(101, &[7, 8]).unwrap(), 1);
    assert_eq!(one.my_tag_links().unwrap(), [(101, 5), (101, 6), (200, 5)]);

    one.remove_my_tag(100).unwrap();
    assert_eq!(names(&one, 1), [("Techno".into(), 0), ("Disco".into(), 1)]);
    one.rename_my_tag(102, "Nu Disco").unwrap();
    assert_eq!(names(&one, 1)[1].0, "Nu Disco");
    one.remove_my_tag(1).unwrap();
    assert_eq!(names(&one, 0), [("Situation".into(), 0)]);
    assert!(names(&one, 1).is_empty());
    assert_eq!(one.my_tag_links().unwrap(), [(200, 5)]);
    // A new first tag is numbered like the others.
    one.add_my_tag(3, 0, "Genre", true).unwrap();
    one.add_my_tag(103, 3, "Electro", false).unwrap();
    assert_eq!(names(&one, 3), [("Electro".into(), 0)]);
}
