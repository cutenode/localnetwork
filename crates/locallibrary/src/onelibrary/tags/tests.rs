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

/// Id 0 is the parent every category has, so a row with it would be taken for the parent of
/// them all: neither added nor removed, and every category stays.
#[test]
fn id_0_is_refused() {
    let mut one = OneLibrary::create().unwrap();
    one.add_my_tag(1, 0, "Genre", true).unwrap();
    one.add_my_tag(2, 1, "Deep", false).unwrap();
    for category in [true, false] {
        assert_eq!(one.add_my_tag(0, if category { 0 } else { 1 }, "Zero", category).unwrap_err().code(), "invalid");
    }
    // A database made elsewhere may hold one already.
    one.conn
        .execute(
            "INSERT INTO myTag (myTag_id, sequenceNo, name, attribute, myTag_id_parent) VALUES (0, 1, 'Zero', 1, 0)",
            [],
        )
        .unwrap();
    assert_eq!(one.remove_my_tag(0).unwrap_err().code(), "invalid");
    assert_eq!(names(&one, 0), [("Genre".into(), 0), ("Zero".into(), 1)]);
    assert_eq!(names(&one, 1), [("Deep".into(), 0)]);
}

/// Whatever My Tags and links a database holds, adding or removing id 0 is refused as invalid and
/// leaves every row and link as it was; any other free id is added under a category.
#[hegel::test(test_cases = 50)]
fn id_0_is_refused_whatever_the_tags(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut one = OneLibrary::create().unwrap();
    let categories = tc.draw(gs::integers::<i64>().min_value(1).max_value(3));
    for c in 1..=categories {
        one.add_my_tag(c, 0, &format!("Category {c}"), true).unwrap();
        for t in 0..tc.draw(gs::integers::<i64>().max_value(3)) {
            let id = c * 100 + t;
            one.add_my_tag(id, c, &format!("Tag {id}"), false).unwrap();
            one.link_my_tag(id, &tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(5)).max_size(3)))
                .unwrap();
        }
    }
    let before = (one.my_tag_rows().unwrap(), one.my_tag_links().unwrap());
    let category = tc.draw(gs::booleans());
    let parent = if category { 0 } else { tc.draw(gs::integers::<i64>().min_value(1).max_value(categories)) };
    let name = tc.draw(gs::text().max_size(8));
    let e = one.add_my_tag(0, parent, &name, category).unwrap_err();
    assert_eq!(e.code(), "invalid", "{e}");
    assert!(e.to_string().contains("id 0 is reserved"), "{e}");
    assert_eq!(one.remove_my_tag(0).unwrap_err().code(), "invalid");
    assert_eq!((one.my_tag_rows().unwrap(), one.my_tag_links().unwrap()), before);
    one.add_my_tag(1000, parent, &name, category).unwrap();
    assert!(one.my_tag_rows().unwrap().iter().any(|r| r.id == 1000 && r.parent == parent && r.name == name));
}
