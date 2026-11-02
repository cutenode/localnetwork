//! Everything set in a store to and from one file (`settings export`, `settings import FILE`):
//! an export fits the checked-in schema, importing it into an empty store gives the same file back,
//! importing adds without touching what's set (unless replacing), plugins that aren't here keep
//! their settings, and of what the player remembers only its preferences ever go out.

use std::fs;
use std::path::{Path, PathBuf};

use hegel::TestCase;
use hegel::generators as gs;
use indexmap::IndexMap;
use locallibrary::api::{self, ImportSettingsFile};
use locallibrary::equipment::Remembered;
use locallibrary::profile::{Precision, ProfilePrefs, Section};
use locallibrary::query::{SortField, SortKey};
use locallibrary::settings::transfer::{Action, PlayerPrefs, SettingsExport};
use locallibrary::settings::{DriveOverrides, SCHEMA, SettingsProfile};
use locallibrary::store::Store;
use serde_json::{Map, Value, json};
use uuid::Uuid;

/// What's set in a store, written as the app writes each of its files.
#[derive(Debug, Default)]
struct Contents {
    my_settings: SettingsProfile,
    players: Remembered,
    prefs: Option<ProfilePrefs>,
    /// What the player remembers (`tui.json`), its preferences and where it was.
    player: Option<Map<String, Value>>,
    host: Map<String, Value>,
    plugins: Map<String, Value>,
}

impl Contents {
    fn write(&self, store: &Store) {
        store.save_settings_profile(&self.my_settings).unwrap();
        store.save_player_models(&self.players).unwrap();
        if let Some(prefs) = &self.prefs {
            prefs.save(store).unwrap();
        }
        if let Some(player) = &self.player {
            fs::write(store.root().join("tui.json"), serde_json::to_vec(player).unwrap()).unwrap();
        }
        let plugins = store.root().join("plugins");
        fs::create_dir_all(&plugins).unwrap();
        if !self.host.is_empty() {
            fs::write(plugins.join("settings.json"), serde_json::to_vec(&self.host).unwrap()).unwrap();
        }
        for (id, settings) in &self.plugins {
            fs::create_dir_all(plugins.join(id)).unwrap();
            fs::write(plugins.join(id).join("settings.json"), serde_json::to_vec(settings).unwrap()).unwrap();
        }
    }
}

#[hegel::test_helper]
fn scalar(tc: &TestCase) -> Value {
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => tc.draw(gs::booleans()).into(),
        1 => tc.draw(gs::integers::<i32>()).into(),
        _ => tc.draw(gs::text().max_size(6)).into(),
    }
}

#[hegel::test_helper]
fn names(tc: &TestCase) -> Vec<String> {
    tc.draw(gs::vecs(gs::sampled_from(vec!["A - One", "B - Two", "C", "CDJ-3000", "XDJ-AZ", "é"])).max_size(3))
        .into_iter()
        .map(String::from)
        .collect()
}

#[hegel::test_helper]
fn my_settings_values(tc: &TestCase, max: usize) -> IndexMap<String, String> {
    let mut out = IndexMap::new();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(max)) {
        let def = tc.draw(gs::sampled_from(SCHEMA.iter().collect::<Vec<_>>()));
        out.insert(def.key.to_string(), tc.draw(gs::sampled_from(def.choices)).id.to_string());
    }
    out.sort_unstable_keys();
    out
}

/// Something set in each part of a store, or nothing, from a few keys and drive ids so two drawn
/// stores share some of them.
#[hegel::test_helper]
fn contents(tc: &TestCase) -> Contents {
    let ids = [Uuid::from_u128(1), Uuid::from_u128(2)];
    let mut c = Contents {
        my_settings: SettingsProfile { base: my_settings_values(tc, 4), ..Default::default() },
        ..Default::default()
    };
    for id in tc.draw(gs::vecs(gs::sampled_from(&ids[..])).max_size(2)) {
        let values = my_settings_values(tc, 2);
        c.my_settings
            .drives
            .insert(id, DriveOverrides { name: tc.draw(gs::sampled_from(vec!["SD", "USB"])).into(), values });
    }
    c.players.default = names(tc);
    for id in tc.draw(gs::vecs(gs::sampled_from(&ids[..])).max_size(2)) {
        c.players.libraries.insert(id, names(tc));
    }
    if tc.draw(gs::booleans()) {
        let sections = tc.draw(gs::optional(gs::vecs(gs::sampled_from(&Section::ALL[..])).max_size(3)));
        c.prefs = Some(ProfilePrefs {
            sections: sections.map(|s| s.into_iter().collect()),
            precision: tc.draw(gs::sampled_from(vec![Precision::Rounded, Precision::Exact])),
            pinned: names(tc),
            hidden: names(tc),
            hidden_artists: names(tc),
            genre_aliases: names(tc).into_iter().map(|n| (n.to_lowercase(), n)).collect(),
            headline: tc.draw(gs::optional(gs::sampled_from(vec!["warm", "peak"]))).map(String::from),
        });
    }
    if tc.draw(gs::booleans()) {
        let maybe = |tc: &TestCase| tc.draw(gs::optional(gs::booleans()));
        let prefs = PlayerPrefs {
            theme: tc.draw(gs::optional(gs::sampled_from(vec!["themes.night", "themes.day"]))).map(String::from),
            sort: tc
                .draw(gs::optional(gs::sampled_from(vec![SortField::Bpm, SortField::Title])))
                .map(|field| SortKey { field, descending: tc.draw(gs::booleans()) }),
            search_all: maybe(tc),
            vis: tc.draw(gs::optional(gs::sampled_from(vec!["spectrum", "deck"]))).map(String::from),
            ratings: maybe(tc),
            tags_column: maybe(tc),
            field_columns: tc
                .draw(gs::optional(gs::vecs(gs::sampled_from(vec!["sound.bpm", "sound.key"])).max_size(2)))
                .map(|c| c.into_iter().map(String::from).collect()),
            shuffle: maybe(tc),
            repeat: tc.draw(gs::optional(gs::sampled_from(vec!["off", "all", "one"]))).map(String::from),
            quantize: maybe(tc),
            single: maybe(tc),
            auto_cue: maybe(tc),
        };
        let Value::Object(mut player) = serde_json::to_value(prefs).unwrap() else { unreachable!() };
        for (key, value) in SESSION.iter() {
            if tc.draw(gs::booleans()) {
                player.insert(key.to_string(), value.clone());
            }
        }
        c.player = Some(player);
    }
    for key in tc.draw(gs::vecs(gs::sampled_from(vec!["checks.sound.bpm.on", "providers.auto_pick"])).max_size(2)) {
        c.host.insert(key.into(), scalar(tc));
    }
    for id in tc.draw(gs::vecs(gs::sampled_from(vec!["sound", "coverart", "not-installed"])).max_size(3)) {
        let mut settings = Map::new();
        for key in tc.draw(gs::vecs(gs::sampled_from(vec!["greeting", "keys.analyze", "services.watch"])).max_size(3)) {
            settings.insert(key.into(), scalar(tc));
        }
        c.plugins.insert(id.into(), settings.into());
    }
    c
}

/// Where the player was, as `tui.json` keeps it: never exported.
static SESSION: std::sync::LazyLock<Vec<(&str, Value)>> = std::sync::LazyLock::new(|| {
    vec![
        ("root", json!("/Volumes/USB")),
        ("node", json!({ "Playlist": { "path": "Set" } })),
        ("view", json!("Library")),
        ("volume", json!(0.5)),
        ("details", json!(true)),
        ("crate_panel", json!(true)),
        ("output", json!("Speakers")),
        ("mixes", json!(["/m/a.wav"])),
        ("queue", json!({ "root": "/Volumes/USB", "drive": "USB", "tracks": ["/a.mp3"], "pos": 0, "at_ms": 5 })),
    ]
});

fn store_in(dir: &Path, name: &str) -> Store {
    Store::open_or_init(dir.join(name)).unwrap()
}

/// `store`'s export, written to a file as `settings export` writes it.
fn exported(store: &Store, dir: &Path) -> PathBuf {
    let path = dir.join(format!("{}.json", Uuid::new_v4()));
    fs::write(&path, serde_json::to_vec_pretty(&api::settings_export(store).unwrap()).unwrap()).unwrap();
    path
}

fn import(store: &Store, file: &Path, replace: bool) -> SettingsExport {
    api::settings_import_file(store, &ImportSettingsFile { file: file.to_path_buf(), replace }).unwrap()
}

fn json_of(doc: &SettingsExport) -> Value {
    serde_json::to_value(doc).unwrap()
}

/// Every value in `before` is in `after` as it was; a list may have names added after its own.
fn holds(before: &Value, after: &Value, at: &str) {
    match (before, after) {
        (Value::Object(b), Value::Object(a)) => {
            for (k, v) in b {
                let got = a.get(k).unwrap_or_else(|| panic!("{at}.{k} is gone: {after}"));
                holds(v, got, &format!("{at}.{k}"));
            }
        }
        (Value::Array(b), Value::Array(a)) => assert!(a.starts_with(b), "{at}: {before} became {after}"),
        _ => assert_eq!(before, after, "{at}"),
    }
}

#[hegel::test(test_cases = 60)]
fn an_export_fits_its_schema(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path(), "store");
    contents(&tc).write(&store);
    let schema = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/locallibrary/docs/settings-export.schema.json");
    let schema: Value = serde_json::from_slice(&fs::read(schema).unwrap()).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let export = json_of(&api::settings_export(&store).unwrap());
    for (key, _) in SESSION.iter() {
        assert!(export["player"].get(key).is_none(), "{key} went out: {export}");
    }
    let errors: Vec<String> = validator.iter_errors(&export).map(|e| format!("{}: {e}", e.instance_path())).collect();
    assert!(errors.is_empty(), "{errors:?}\n{export:#}");
}

/// Exported, imported into an empty store and exported from there: the same file.
#[hegel::test(test_cases = 60)]
fn an_export_comes_back_from_an_empty_store(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (here, there) = (store_in(dir.path(), "here"), store_in(dir.path(), "there"));
    contents(&tc).write(&here);
    let file = exported(&here, dir.path());
    let replace = tc.draw(gs::booleans());
    let after = import(&there, &file, replace);
    assert_eq!(json_of(&after), json_of(&api::settings_export(&here).unwrap()));
    assert_eq!(fs::read(exported(&there, dir.path())).unwrap(), fs::read(&file).unwrap());
}

/// Importing adds the file's values without changing any set here; replacing makes each of the
/// file's values the file's, and still keeps what the file doesn't have.
#[hegel::test(test_cases = 100)]
fn importing_adds_and_replacing_overwrites(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (from, into) = (store_in(dir.path(), "from"), store_in(dir.path(), "into"));
    contents(&tc).write(&from);
    contents(&tc).write(&into);
    let file = exported(&from, dir.path());
    let (theirs, ours) =
        (json_of(&api::settings_export(&from).unwrap()), json_of(&api::settings_export(&into).unwrap()));
    let replace = tc.draw(gs::booleans());
    let plan = api::settings_import_file_plan(&into, &ImportSettingsFile { file: file.clone(), replace }).unwrap();
    let after = json_of(&import(&into, &file, replace));
    let refused = if replace { Action::Keep } else { Action::Replace };
    assert!(plan.plan.changes.iter().all(|c| c.action != refused), "{:?}", plan.plan.changes);
    if !replace {
        holds(&ours, &after, "");
        return;
    }
    // Every value the file has is the file's (a list too); what only this store had stays.
    fn replaced(theirs: &Value, after: &Value, ours: Option<&Value>, at: &str) {
        match (theirs, after) {
            (Value::Object(t), Value::Object(a)) => {
                for (k, v) in t {
                    replaced(v, &a[k], ours.and_then(|o| o.get(k)), &format!("{at}.{k}"));
                }
                for (k, v) in ours.and_then(Value::as_object).into_iter().flatten().filter(|(k, _)| !t.contains_key(*k))
                {
                    assert_eq!(a.get(k), Some(v), "{at}.{k} was only here");
                }
            }
            _ => assert_eq!(theirs, after, "{at}"),
        }
    }
    replaced(&theirs, &after, Some(&ours), "");
}

/// A plugin that isn't here keeps its settings (its key bindings among them) for when it is; a
/// value that differs is kept and listed, and `replace` takes the file's.
#[test]
fn plugins_not_here_keep_their_settings_and_a_difference_is_listed() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path(), "store");
    store
        .save_settings_profile(&SettingsProfile {
            base: [("quantize".into(), "on".into())].into(),
            ..Default::default()
        })
        .unwrap();
    let file = dir.path().join("settings.json");
    let doc = json!({
        "version": 1,
        "my_settings": { "base": { "quantize": "off", "jog_mode": "cdj" } },
        "plugins": { "not-installed": { "keys.analyze": "a", "services.watch": true } }
    });
    fs::write(&file, doc.to_string()).unwrap();
    let req = ImportSettingsFile { file: file.clone(), replace: false };
    let plan = api::settings_import_file_plan(&store, &req).unwrap();
    let kept: Vec<_> = plan.plan.changes.iter().filter(|c| c.action == Action::Keep).collect();
    assert_eq!(kept.len(), 1, "{:?}", plan.plan.changes);
    assert_eq!(
        (kept[0].path.join("."), &kept[0].current, &kept[0].imported),
        ("my_settings.base.quantize".into(), &Some(json!("on")), &json!("off"))
    );
    assert!(!plan.noop && !plan.destructive && plan.warnings.len() == 1, "{:?}", plan.warnings);

    let after = api::settings_import_file(&store, &req).unwrap();
    assert_eq!(after.my_settings["base"], json!({ "jog_mode": "cdj", "quantize": "on" }));
    assert_eq!(after.plugins["not-installed"], json!({ "keys.analyze": "a", "services.watch": true }));
    let kept_file = store.root().join("plugins/not-installed/settings.json");
    assert_eq!(serde_json::from_slice::<Value>(&fs::read(kept_file).unwrap()).unwrap()["keys.analyze"], "a");

    let again = api::settings_import_file_plan(&store, &req).unwrap();
    assert!(again.noop, "{:?}", again.plan.changes);
    let after = api::settings_import_file(&store, &ImportSettingsFile { file, replace: true }).unwrap();
    assert_eq!(after.my_settings["base"]["quantize"], "off");
}

/// A file from a newer locallibrary, or with a plugin id that can't name a folder or a setting
/// that doesn't exist, is refused before anything is planned.
#[test]
fn a_file_that_cant_be_imported_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path(), "store");
    let file = dir.path().join("settings.json");
    for (doc, says) in [
        (json!({ "version": 2 }), "newer"),
        (json!({ "version": 1, "plugins": { "../up": {} } }), "plugin"),
        (json!({ "version": 1, "plugins": { "sound": "on" } }), "plugin"),
        (json!({ "version": 1, "my_settings": { "base": { "no_such_setting": "on" } } }), "my_settings"),
        (json!({ "my_settings": {} }), "version"),
    ] {
        fs::write(&file, doc.to_string()).unwrap();
        let err = api::settings_import_file_plan(&store, &ImportSettingsFile { file: file.clone(), replace: false })
            .unwrap_err()
            .to_string();
        assert!(err.contains(says), "{doc}: {err}");
    }
    assert!(!store.root().join("plugins").exists() && !store.root().join("settings.json").exists());
}

/// A plan is refused once what it would write changed, and imports nothing.
#[test]
fn a_plan_is_stale_once_the_store_changes() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path(), "store");
    let file = dir.path().join("settings.json");
    fs::write(&file, json!({ "version": 1, "my_settings": { "base": { "quantize": "off" } } }).to_string()).unwrap();
    let plan = api::settings_import_file_plan(&store, &ImportSettingsFile { file, replace: false }).unwrap();
    store
        .save_settings_profile(&SettingsProfile { base: [("sync".into(), "on".into())].into(), ..Default::default() })
        .unwrap();
    let err = plan.apply(Some(&store), &locallibrary::progress::Ctx::silent()).unwrap_err();
    assert!(matches!(err, locallibrary::Error::StalePlan(_)), "{err}");
    assert!(!store.settings_profile().unwrap().base.contains_key("quantize"));
}

/// Importing the player's preferences keeps where it was: the queue and the drive stay, and a
/// player that kept its state beside the store (there was none yet) finds it in the store.
#[test]
fn importing_the_players_preferences_keeps_where_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path(), "store");
    let beside = dir.path().join("store-player");
    fs::create_dir_all(&beside).unwrap();
    let kept = json!({ "root": "/Volumes/USB", "theme": "themes.day", "queue": { "root": "/Volumes/USB", "drive": "USB", "tracks": ["/a.mp3"], "pos": 0, "at_ms": 5 } });
    fs::write(beside.join("tui.json"), kept.to_string()).unwrap();
    let file = dir.path().join("settings.json");
    let doc =
        json!({ "version": 1, "player": { "theme": "themes.night", "ratings": true, "queue": { "tracks": [] } } });
    fs::write(&file, doc.to_string()).unwrap();
    let after = import(&store, &file, false);
    assert_eq!(
        Value::Object(after.player),
        json!({ "theme": "themes.day", "ratings": true }),
        "the file's queue never comes in"
    );
    let state: Value = serde_json::from_slice(&fs::read(store.root().join("tui.json")).unwrap()).unwrap();
    assert_eq!(state["queue"], kept["queue"]);
    assert_eq!(
        (&state["root"], &state["theme"], &state["ratings"]),
        (&json!("/Volumes/USB"), &json!("themes.day"), &json!(true))
    );
    let after = import(&store, &file, true);
    assert_eq!(after.player["theme"], "themes.night");
}
