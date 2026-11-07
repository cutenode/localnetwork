use std::fs;
use std::path::Path;

use filetime::FileTime;

use super::*;
use crate::model::Library;

fn analyzer(plugin: &str, analyzer: &str) -> Analyzer {
    Analyzer { plugin: plugin.into(), analyzer: analyzer.into(), version: "1".into() }
}

fn field(plugin: &str, a: &str, field: &str) -> RoleField {
    RoleField { analyzer: analyzer(plugin, a), field: field.into() }
}

/// Two analyzers with a key, in that order, one of them with a BPM too, offered on this thread.
fn offer() {
    let offered: Offered = [
        (Role::Key, vec![field("sound", "tkl", "key"), field("other", "keys", "camelot")]),
        (Role::Bpm, vec![field("other", "keys", "tempo")]),
    ]
    .into_iter()
    .collect();
    crate::testing::roles_on_this_thread(Some(offered));
}

fn store(dir: &Path) -> Store {
    Store::open_or_init(dir.join("store")).unwrap()
}

/// A role is read from the analyzer chosen for it, else the first offered; a choice of one no
/// longer offered is the default again. A role no analyzer has is read from none.
#[test]
fn a_role_is_read_from_the_analyzer_chosen_else_the_first() {
    offer();
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let name = |s: Option<&Store>, r| chosen(s, r).map(|f| f.name());
    assert_eq!(name(None, Role::Key).as_deref(), Some("sound.tkl"));
    assert_eq!(name(Some(&store), Role::Bpm).as_deref(), Some("other.keys"), "the only one");
    assert_eq!(name(Some(&store), Role::Loudness), None);

    let all = choose(&store, Role::Key, Some("other.keys")).unwrap();
    assert_eq!(chosen(Some(&store), Role::Key), Some(field("other", "keys", "camelot")));
    let key = &all[0];
    assert_eq!(
        (key.role, key.chosen.as_deref(), key.set, key.offered.clone()),
        (Role::Key, Some("other.keys"), true, vec!["sound.tkl".to_string(), "other.keys".into()])
    );
    assert_eq!(all.iter().map(|c| c.role).collect::<Vec<_>>(), Role::ALL, "every role");
    let none = all.iter().find(|c| c.role == Role::Identity).unwrap();
    assert_eq!((none.chosen.as_ref(), none.set, none.offered.len()), (None, false, 0));
    let kept: Value = serde_json::from_slice(&fs::read(store.root().join("plugins/settings.json")).unwrap()).unwrap();
    assert_eq!(kept, serde_json::json!({ "analysis.roles.key": "other.keys" }));

    // Turned off: read from none, until chosen or put back.
    let off = turn_off(&store, Role::Key).unwrap().remove(0);
    assert_eq!((off.chosen, off.set, off.off), (None, false, true));
    assert_eq!(chosen(Some(&store), Role::Key), None);
    assert_eq!(name(Some(&store), Role::Bpm).as_deref(), Some("other.keys"), "only that role");

    // Put back, or no longer offered: the default.
    choose(&store, Role::Key, None).unwrap();
    assert_eq!(name(Some(&store), Role::Key).as_deref(), Some("sound.tkl"));
    choose(&store, Role::Key, Some("other.keys")).unwrap();
    crate::testing::roles_on_this_thread(Some([(Role::Key, vec![field("sound", "tkl", "key")])].into_iter().collect()));
    let key = choices(Some(&store)).remove(0);
    assert_eq!((key.chosen.as_deref(), key.set, key.off), (Some("sound.tkl"), false, false));
    crate::testing::roles_on_this_thread(None);
}

/// Only an analyzer that has the role can be chosen for it, and damaged settings are
/// the defaults, written anew by a choice.
#[test]
fn only_an_analyzer_with_the_role_is_chosen() {
    offer();
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let e = choose(&store, Role::Bpm, Some("sound.tkl")).unwrap_err();
    assert_eq!(e.to_string(), "sound.tkl doesn't work out bpm: other.keys does");
    let e = choose(&store, Role::Loudness, Some("sound.tkl")).unwrap_err();
    assert_eq!(e.to_string(), "no analyzer works out loudness");
    fs::create_dir_all(store.root().join("plugins")).unwrap();
    fs::write(store.root().join("plugins/settings.json"), "not json").unwrap();
    assert_eq!(chosen(Some(&store), Role::Key).unwrap().name(), "sound.tkl");
    choose(&store, Role::Key, Some("other.keys")).unwrap();
    assert_eq!(chosen(Some(&store), Role::Key).unwrap().name(), "other.keys");
    crate::testing::roles_on_this_thread(None);
}

/// Each track's values, by role, from the analyzers chosen: one file read per analyzer, a null
/// kept, a role or track with nothing left out.
#[test]
fn values_are_read_by_role() {
    offer();
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let drive = dir.path().join("USB");
    fs::create_dir_all(drive.join("Contents")).unwrap();
    let audio = |name: &str| {
        let path = drive.join("Contents").join(name);
        fs::write(&path, name).unwrap();
        filetime::set_file_mtime(&path, FileTime::from_unix_time(1_790_000_000, 0)).unwrap();
        path
    };
    let fields = FieldStore::in_store(&store);
    let kept = |pairs: &[(&str, FieldValue)]| pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect();
    let (a, b) = (audio("a.mp3"), audio("b.mp3"));
    audio("c.mp3");
    fields.put(&a, &analyzer("sound", "tkl"), &kept(&[("key", FieldValue::String("8A".into()))])).unwrap();
    fields.put(&a, &analyzer("other", "keys"), &kept(&[("tempo", FieldValue::Number(128.0))])).unwrap();
    fields.put(&b, &analyzer("other", "keys"), &kept(&[("tempo", FieldValue::Null)])).unwrap();
    let track = |id, file: &str| loclib_model::test_util::track(id, "A", "T", file);
    let library = Library {
        tracks: vec![track(1, "/Contents/a.mp3"), track(2, "/Contents/b.mp3"), track(3, "/Contents/c.mp3")],
        ..Default::default()
    };
    let tracks: Vec<&Track> = library.tracks.iter().collect();
    let device = Device::open(&drive).unwrap();
    let heard = by_role(&store, &[Role::Key, Role::Bpm, Role::Loudness], &device, &tracks);
    let one: RoleValues =
        [(Role::Key, FieldValue::String("8A".into())), (Role::Bpm, FieldValue::Number(128.0))].into_iter().collect();
    assert_eq!(heard[&1], one);
    assert_eq!(heard[&2], [(Role::Bpm, FieldValue::Null)].into_iter().collect::<RoleValues>());
    assert!(!heard.contains_key(&3), "nothing kept for it");
    assert!(by_role(&store, &[Role::Loudness], &device, &tracks).is_empty(), "no analyzer has it");
    crate::testing::roles_on_this_thread(None);
}

/// For any sequence of choices (an analyzer, the default again, or none at all) for any roles,
/// each role is read from what the last choice for it says: the analyzer chosen if it has the
/// role, else the first offered, none once turned off; choosing one without the role is refused
/// and changes nothing. Each track's values are then those its file has from the fields so
/// chosen, and a track off the drive has none.
#[hegel::test(test_cases = 30)]
fn prop_each_role_is_read_from_what_its_last_choice_says(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let offered: Offered = [
        (Role::Key, vec![field("sound", "tkl", "key"), field("other", "keys", "camelot")]),
        (Role::Bpm, vec![field("other", "keys", "tempo")]),
        (Role::Loudness, vec![field("sound", "loud", "lufs")]),
    ]
    .into_iter()
    .collect();
    crate::testing::roles_on_this_thread(Some(offered.clone()));
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    // The model: a role's last choice (`None`: the default, `Some(None)`: turned off).
    let mut model: IndexMap<Role, Option<Option<String>>> = IndexMap::new();
    let roles = [Role::Key, Role::Bpm, Role::Loudness, Role::Identity];
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(6)) {
        let role = tc.draw(gs::sampled_from(roles.to_vec()));
        let names: Vec<String> = offered.get(&role).into_iter().flatten().map(RoleField::name).collect();
        match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => {
                turn_off(&store, role).unwrap();
                model.insert(role, Some(None));
            }
            1 => {
                choose(&store, role, None).unwrap();
                model.insert(role, None);
            }
            _ => {
                let name = tc.draw(gs::sampled_from(vec!["sound.tkl", "other.keys", "sound.loud", "nope.x"]));
                match choose(&store, role, Some(name)) {
                    Ok(_) => {
                        assert!(names.iter().any(|n| n == name));
                        model.insert(role, Some(Some(name.to_string())));
                    }
                    Err(_) => assert!(!names.iter().any(|n| n == name), "{name} for {role:?}"),
                }
            }
        }
    }
    let want = |role: Role| -> Option<RoleField> {
        let all = offered.get(&role)?;
        match model.get(&role).cloned().flatten() {
            Some(None) => None,
            Some(Some(name)) => all.iter().find(|f| f.name() == name).cloned(),
            None => all.first().cloned(),
        }
    };
    for role in roles {
        assert_eq!(chosen(Some(&store), role), want(role), "{role:?}");
    }

    let drive = dir.path().join("USB");
    fs::create_dir_all(drive.join("Contents")).unwrap();
    let a = drive.join("Contents/a.mp3");
    fs::write(&a, "a").unwrap();
    filetime::set_file_mtime(&a, FileTime::from_unix_time(1_790_000_000, 0)).unwrap();
    let kept: [(&str, &str, &str, FieldValue); 3] = [
        ("sound", "tkl", "key", FieldValue::String("8A".into())),
        ("other", "keys", "camelot", FieldValue::String("9A".into())),
        ("other", "keys", "tempo", FieldValue::Number(128.0)),
    ];
    let fields = FieldStore::in_store(&store);
    for (plugin, a_name) in [("sound", "tkl"), ("other", "keys")] {
        let values = kept.iter().filter(|k| (k.0, k.1) == (plugin, a_name)).map(|k| (k.2.to_string(), k.3.clone()));
        fields.put(&a, &analyzer(plugin, a_name), &values.collect()).unwrap();
    }
    let track = |id, file: &str| loclib_model::test_util::track(id, "A", "T", file);
    let library = Library { tracks: vec![track(1, "/Contents/a.mp3"), track(2, "/../a.mp3")], ..Default::default() };
    let tracks: Vec<&Track> = library.tracks.iter().collect();
    let heard = by_role(&store, &roles, &Device::open(&drive).unwrap(), &tracks);
    let expected: RoleValues = roles
        .into_iter()
        .filter_map(|r| {
            let f = want(r)?;
            let k = kept.iter().find(|k| (k.0, k.1, k.2) == (&*f.analyzer.plugin, &*f.analyzer.analyzer, &*f.field))?;
            Some((r, k.3.clone()))
        })
        .collect();
    assert_eq!(heard.get(&1).cloned().unwrap_or_default(), expected);
    assert!(!heard.contains_key(&2), "off the drive");
    crate::testing::roles_on_this_thread(None);
}
