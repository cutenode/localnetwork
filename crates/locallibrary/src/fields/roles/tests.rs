use std::fs;
use std::path::Path;

use filetime::FileTime;

use super::*;
use crate::model::Library;
use crate::plugin::AnalysisPart;

fn analyzer(plugin: &str, analyzer: &str) -> Analyzer {
    Analyzer { plugin: plugin.into(), analyzer: analyzer.into(), version: "1".into() }
}

fn field(plugin: &str, a: &str, field: &str) -> RoleField {
    RoleField { analyzer: analyzer(plugin, a), field: field.into() }
}

/// Two analyzers with a key and a waveform, in that order, one of them with a BPM too, offered on
/// this thread.
fn offer() {
    let offered: Offered = [
        (RoleOrPart::Role(Role::Key), vec![field("sound", "tkl", "key"), field("other", "keys", "camelot")]),
        (RoleOrPart::Role(Role::Bpm), vec![field("other", "keys", "tempo")]),
        (
            RoleOrPart::Part(AnalysisPart::Waveform),
            vec![field("sound", "tkl", "wave"), field("other", "keys", "bands")],
        ),
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
        (RoleOrPart::Role(Role::Key), Some("other.keys"), true, vec!["sound.tkl".to_string(), "other.keys".into()])
    );
    assert_eq!(all.iter().map(|c| c.role).collect::<Vec<_>>(), RoleOrPart::all().collect::<Vec<_>>(), "every role");
    let none = all.iter().find(|c| c.role == RoleOrPart::Role(Role::Identity)).unwrap();
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
    let key = [(RoleOrPart::Role(Role::Key), vec![field("sound", "tkl", "key")])];
    crate::testing::roles_on_this_thread(Some(key.into_iter().collect()));
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
        (RoleOrPart::Role(Role::Key), vec![field("sound", "tkl", "key"), field("other", "keys", "camelot")]),
        (RoleOrPart::Role(Role::Bpm), vec![field("other", "keys", "tempo")]),
        (RoleOrPart::Role(Role::Loudness), vec![field("sound", "loud", "lufs")]),
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
        let names: Vec<String> =
            offered.get(&RoleOrPart::Role(role)).into_iter().flatten().map(RoleField::name).collect();
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
        let all = offered.get(&RoleOrPart::Role(role))?;
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

/// A part of the analysis is chosen, shown, turned off and put back as a role is, under its own
/// name, and read from the analyzer chosen for it alone; one without it is refused.
#[test]
fn a_part_of_the_analysis_is_chosen_as_a_role_is() {
    offer();
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let wave =
        |all: Vec<RoleChoice>| all.into_iter().find(|c| c.role == RoleOrPart::Part(AnalysisPart::Waveform)).unwrap();
    let default = wave(choices(Some(&store)));
    assert_eq!((default.chosen.as_deref(), default.set, default.off), (Some("sound.tkl"), false, false));
    assert_eq!(parts_chosen(Some(&store)), PartsChosen::new(), "none chosen: any analyzer's");

    let chosen = wave(choose(&store, AnalysisPart::Waveform, Some("other.keys")).unwrap());
    assert_eq!((chosen.chosen.as_deref(), chosen.set, chosen.offered.len()), (Some("other.keys"), true, 2));
    assert_eq!(
        parts_chosen(Some(&store)),
        PartsChosen::from([(AnalysisPart::Waveform, Some(analyzer("other", "keys")))])
    );
    let kept: Value = serde_json::from_slice(&fs::read(store.root().join("plugins/settings.json")).unwrap()).unwrap();
    assert_eq!(kept, serde_json::json!({ "analysis.roles.waveform": "other.keys" }));

    let e = choose(&store, AnalysisPart::Waveform, Some("nope.x")).unwrap_err();
    assert_eq!(e.to_string(), "nope.x doesn't work out waveform: sound.tkl, other.keys does");
    let e = choose(&store, AnalysisPart::Grid, Some("sound.tkl")).unwrap_err();
    assert_eq!(e.to_string(), "no analyzer works out grid");
    let waveform = parts_chosen(Some(&store)).shift_remove(&AnalysisPart::Waveform);
    assert_eq!(waveform, Some(Some(analyzer("other", "keys"))), "a refusal changes nothing");

    let off = wave(turn_off(&store, AnalysisPart::Waveform).unwrap());
    assert_eq!((off.chosen, off.set, off.off), (None, false, true));
    assert_eq!(parts_chosen(Some(&store)), PartsChosen::from([(AnalysisPart::Waveform, None)]));
    choose(&store, AnalysisPart::Waveform, None).unwrap();
    assert_eq!(parts_chosen(Some(&store)), PartsChosen::new());
    crate::testing::roles_on_this_thread(None);
}

/// A role or part is named as the settings, `plugin roles` and the sidecar name it, and nothing
/// else is one.
#[test]
fn roles_and_parts_are_named_alike() {
    assert_eq!("waveform".parse::<RoleOrPart>(), Ok(RoleOrPart::Part(AnalysisPart::Waveform)));
    assert_eq!("key".parse::<RoleOrPart>(), Ok(RoleOrPart::Role(Role::Key)));
    let e = "wave".parse::<RoleOrPart>().unwrap_err();
    assert!(
        e.starts_with("\"wave\" isn't a role: one of key, bpm,") && e.ends_with(", grid, phrases, waveform"),
        "{e}"
    );
    for r in RoleOrPart::all() {
        assert_eq!(serde_json::to_value(r).unwrap(), Value::from(r.name()));
        assert_eq!(serde_json::from_value::<RoleOrPart>(Value::from(r.name())).unwrap(), r);
    }
    assert_eq!(RoleOrPart::all().count(), Role::ALL.len() + AnalysisPart::ALL.len());
}

/// For any sequence of choices (an analyzer, the default again, or none at all) for any part of
/// the analysis, it's read from the analyzer its last choice named if that has it, from none once
/// turned off, else from any; `plugin roles` shows the same, its default the first offered; and
/// choosing one without it is refused and changes nothing.
#[hegel::test(test_cases = 30)]
fn prop_each_part_is_read_from_what_its_last_choice_says(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let offered: Offered = [
        (RoleOrPart::Part(AnalysisPart::Grid), vec![field("sound", "tkl", "grid"), field("other", "keys", "beats")]),
        (RoleOrPart::Part(AnalysisPart::Waveform), vec![field("other", "keys", "wave")]),
    ]
    .into_iter()
    .collect();
    crate::testing::roles_on_this_thread(Some(offered.clone()));
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let names = |part: AnalysisPart| -> Vec<String> {
        offered.get(&RoleOrPart::Part(part)).into_iter().flatten().map(RoleField::name).collect()
    };
    // The model: a part's last choice (`None`: the default, `Some(None)`: turned off).
    let mut model: IndexMap<AnalysisPart, Option<Option<String>>> = IndexMap::new();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(6)) {
        let part = tc.draw(gs::sampled_from(AnalysisPart::ALL.to_vec()));
        match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => {
                turn_off(&store, part).unwrap();
                model.insert(part, Some(None));
            }
            1 => {
                choose(&store, part, None).unwrap();
                model.insert(part, None);
            }
            _ => {
                let name = tc.draw(gs::sampled_from(vec!["sound.tkl", "other.keys", "nope.x"]));
                match choose(&store, part, Some(name)) {
                    Ok(_) => {
                        assert!(names(part).iter().any(|n| n == name));
                        model.insert(part, Some(Some(name.to_string())));
                    }
                    Err(_) => assert!(!names(part).iter().any(|n| n == name), "{name} for {part:?}"),
                }
            }
        }
    }
    let by_name = |name: &str| offered.values().flatten().find(|f| f.name() == name).map(|f| f.analyzer.clone());
    let expected: PartsChosen = model
        .iter()
        .filter_map(|(part, last)| Some((*part, last.as_ref()?.as_deref().map(|n| by_name(n).unwrap()))))
        .collect();
    // In any order.
    assert_eq!(parts_chosen(Some(&store)), expected);
    for c in choices(Some(&store)) {
        let RoleOrPart::Part(part) = c.role else { continue };
        let last = model.get(&part).cloned().flatten();
        let want = match &last {
            Some(None) => None,
            Some(Some(name)) => Some(name.clone()),
            None => names(part).first().cloned(),
        };
        assert_eq!((c.chosen, c.off, c.offered), (want, last == Some(None), names(part)), "{part:?}");
    }
    crate::testing::roles_on_this_thread(None);
}
