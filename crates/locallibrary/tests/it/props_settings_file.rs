//! Everything set in a store to and from one file (`settings export`, `settings import FILE`):
//! an export fits the checked-in schema, importing it into an empty store gives the same file back,
//! importing adds without touching what's set (unless replacing), plugins that aren't here keep
//! their settings, of what the player remembers only its preferences ever go out, and the player's
//! keys go action by action. `keybinds.schema.json` is the file's schema, made from [`Keybinds`].

use std::fs;
use std::path::{Path, PathBuf};

use hegel::TestCase;
use hegel::generators as gs;
use indexmap::IndexMap;
use locallibrary::api::{self, ImportSettingsFile};
use locallibrary::equipment::Remembered;
use locallibrary::keybinds::{self, Keybinds};
use locallibrary::mix::prefs::MixPrefs;
use locallibrary::profile::{Precision, ProfilePrefs, Section};
use locallibrary::query::{SortField, SortKey};
use locallibrary::settings::transfer::{Action, PlayerPrefs, SettingsExport};
use locallibrary::settings::{DriveOverrides, SCHEMA, SettingsProfile};
use locallibrary::store::Store;
use locallibrary::store::prefs::StorePrefs;
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
    mix: Option<MixPrefs>,
    store: Option<StorePrefs>,
    host: Map<String, Value>,
    plugins: Map<String, Value>,
    keybinds: Option<Keybinds>,
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
        if let Some(mix) = &self.mix {
            mix.save(store).unwrap();
        }
        if let Some(prefs) = &self.store {
            prefs.save(store).unwrap();
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
        if let Some(binds) = &self.keybinds {
            keybinds::save(store.root(), binds).unwrap();
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
            color: tc.draw(gs::optional(gs::sampled_from(vec!["truecolor", "256", "16"]))).map(String::from),
            icons: tc.draw(gs::optional(gs::sampled_from(vec!["nerd", "unicode", "ascii"]))).map(String::from),
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
            fill: maybe(tc),
            notation: tc.draw(gs::optional(gs::sampled_from(vec!["camelot", "open-key"]))).map(String::from),
            key_palette: tc.draw(gs::optional(gs::sampled_from(vec!["theme", "tritanopia"]))).map(String::from),
            bands: tc.draw(gs::optional(gs::sampled_from(vec!["blue", "3band"]))).map(String::from),
            end_warning: tc.draw(gs::optional(gs::sampled_from(vec![0, 30]))),
            art: maybe(tc),
            art_halfblocks: maybe(tc),
            hints: tc.draw(gs::optional(gs::sampled_from(vec!["full", "off"]))).map(String::from),
            help_hint: maybe(tc),
            recent_commands: tc
                .draw(gs::optional(gs::vecs(gs::sampled_from(vec!["quit", "sync"])).max_size(2)))
                .map(|c| c.into_iter().map(String::from).collect()),
        };
        let Value::Object(mut player) = serde_json::to_value(prefs).unwrap() else { unreachable!() };
        for (key, value) in SESSION.iter() {
            if tc.draw(gs::booleans()) {
                player.insert(key.to_string(), value.clone());
            }
        }
        c.player = Some(player);
    }
    if tc.draw(gs::booleans()) {
        let limit = gs::sampled_from(vec![0, 64, 256, 1024]);
        c.mix = Some(MixPrefs { index_limit_mib: tc.draw(gs::optional(limit)) });
    }
    if tc.draw(gs::booleans()) {
        c.store = Some(StorePrefs { copy_on_write: tc.draw(gs::booleans()) });
    }
    for key in tc.draw(gs::vecs(gs::sampled_from(vec!["checks.sound.bpm.on", "providers.auto_pick"])).max_size(2)) {
        c.host.insert(key.into(), scalar(tc));
    }
    for id in tc.draw(gs::vecs(gs::sampled_from(vec!["sound", "coverart", "not-installed"])).max_size(3)) {
        let mut settings = Map::new();
        for key in tc.draw(gs::vecs(gs::sampled_from(vec!["greeting", "language", "services.watch"])).max_size(3)) {
            settings.insert(key.into(), scalar(tc));
        }
        c.plugins.insert(id.into(), settings.into());
    }
    if tc.draw(gs::booleans()) {
        let mut binds = Keybinds {
            from: tc.draw(gs::optional(gs::sampled_from(vec!["did:plc:abc", "did:plc:xyz"]))).map(String::from),
            updated_at: tc.draw(gs::optional(gs::sampled_from(vec!["2026-11-20T18:00:00.000Z"]))).map(String::from),
            ..Default::default()
        };
        let ids = vec!["playback.next", "player.jobs", "plugin.sound.analyze", "visuals.sparkles"];
        for id in tc.draw(gs::vecs(gs::sampled_from(ids)).max_size(3)) {
            let keys = tc.draw(gs::vecs(gs::sampled_from(vec!["]", "n", "f6", "ctrl-k", "not a key"])).max_size(2));
            binds.keys.insert(id.into(), keys.into_iter().map(String::from).collect());
        }
        c.keybinds = Some(binds);
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
    let c = contents(&tc);
    c.write(&store);
    let schema = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/locallibrary/docs/settings-export.schema.json");
    let schema: Value = serde_json::from_slice(&fs::read(schema).unwrap()).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let export = json_of(&api::settings_export(&store).unwrap());
    for (key, _) in SESSION.iter() {
        assert!(export["player"].get(key).is_none(), "{key} went out: {export}");
    }
    assert_eq!(export["mix"], serde_json::to_value(c.mix.unwrap_or_default()).unwrap(), "{export}");
    assert_eq!(export["store"], serde_json::to_value(c.store.unwrap_or_default()).unwrap(), "{export}");
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
/// file's values the file's, and still keeps what the file doesn't have. A plan applied without
/// the store is refused, importing nothing.
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
    let err = plan.apply(None, &locallibrary::progress::Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("importing settings needs the store"), "{err}");
    assert_eq!(json_of(&api::settings_export(&into).unwrap()), ours, "nothing imported without the store");
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

/// A plugin that isn't here keeps its settings for when it is; a value that differs is kept and
/// listed, and `replace` takes the file's.
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
        "plugins": { "not-installed": { "language": "a", "services.watch": true } }
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
    assert_eq!(after.plugins["not-installed"], json!({ "language": "a", "services.watch": true }));
    let kept_file = store.root().join("plugins/not-installed/settings.json");
    assert_eq!(serde_json::from_slice::<Value>(&fs::read(kept_file).unwrap()).unwrap()["language"], "a");

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

/// Importing into a store with a section it can't read right now is refused, and the file stays as
/// it was: read as empty, importing would write just the file's values over it.
#[cfg(unix)]
#[hegel::test(test_cases = 5)]
fn importing_over_a_file_that_cant_be_read_leaves_it_alone(tc: TestCase) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path(), "store");
    let doc = dir.path().join("settings.json");
    let value = tc.draw(gs::integers::<u8>());
    let set = json!({ "version": 1, "player": { "ratings": true }, "plugin_host": { "new": value } });
    fs::write(&doc, set.to_string()).unwrap();
    let plugins = store.root().join("plugins");
    fs::create_dir_all(&plugins).unwrap();
    let (player, host) = (store.root().join("tui.json"), plugins.join("settings.json"));
    fs::write(&player, json!({ "root": "/Volumes/USB" }).to_string()).unwrap();
    fs::write(&host, json!({ "kept": 2 }).to_string()).unwrap();
    for file in [&player, &host] {
        let before = fs::read(file).unwrap();
        let mode = |bits: u32| fs::set_permissions(file, fs::Permissions::from_mode(bits)).unwrap();
        mode(0o000);
        // Root reads anything.
        if fs::read(file).is_ok() {
            return mode(0o644);
        }
        let imported = api::settings_import_file(&store, &ImportSettingsFile { file: doc.clone(), replace: false });
        mode(0o644);
        assert!(matches!(imported, Err(locallibrary::Error::Io { path, .. }) if path == *file));
        assert_eq!(fs::read(file).unwrap(), before, "{}", file.display());
    }
}

/// Importing into a store whose plugins can't be listed right now is refused: their settings read
/// as none, importing would write just the file's values over a plugin's.
#[cfg(unix)]
#[hegel::test(test_cases = 5)]
fn importing_when_the_plugins_cant_be_listed_leaves_theirs_alone(tc: TestCase) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path(), "store");
    let doc = dir.path().join("settings.json");
    let value = tc.draw(gs::integers::<u8>());
    fs::write(&doc, json!({ "version": 1, "plugins": { "echo": { "new": value } } }).to_string()).unwrap();
    let plugins = store.root().join("plugins");
    let kept = plugins.join("echo/settings.json");
    fs::create_dir_all(kept.parent().unwrap()).unwrap();
    fs::write(&kept, json!({ "kept": 2 }).to_string()).unwrap();
    let before = fs::read(&kept).unwrap();
    let mode = |bits: u32| fs::set_permissions(&plugins, fs::Permissions::from_mode(bits)).unwrap();
    mode(0o311);
    // Root lists anything.
    if fs::read_dir(&plugins).is_ok() {
        return mode(0o755);
    }
    let imported = api::settings_import_file(&store, &ImportSettingsFile { file: doc.clone(), replace: false });
    let exported = api::settings_export(&store);
    mode(0o755);
    assert!(matches!(imported, Err(locallibrary::Error::Io { path, .. }) if path == plugins));
    assert!(matches!(exported, Err(locallibrary::Error::Io { path, .. }) if path == plugins));
    assert_eq!(fs::read(&kept).unwrap(), before);
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

/// A file whose version isn't this one's, or with a section that isn't what that section holds
/// (a value of the wrong kind, a setting that doesn't exist, in the base or a drive's
/// overrides, a plugin's entry that isn't one), is refused naming the version or the section,
/// before anything is planned or written.
#[hegel::test(test_cases = 10)]
fn prop_a_file_with_a_section_it_cant_read_is_refused(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path(), "store");
    let drive = Uuid::from_u128(1).to_string();
    let drive = drive.as_str();
    let replace = tc.draw(gs::booleans());
    for case in 0..12u8 {
        let (doc, says) = match case {
            0 => (json!({ "version": 0 }), "not one locallibrary wrote".to_string()),
            1 => {
                let v = tc.draw(gs::integers::<u32>().min_value(2));
                (json!({ "version": v }), format!("its version is {v}: a newer"))
            }
            2 => (json!({ "version": 1, "my_settings": { "base": 5 } }), "my_settings".into()),
            3 => {
                (json!({ "version": 1, "my_settings": { "base": { "no_such_setting": "on" } } }), "my_settings".into())
            }
            4 => (
                json!({ "version": 1, "my_settings": { "drives": { drive: { "name": "SD", "values": { "quantize": "sideways" } } } } }),
                "my_settings".into(),
            ),
            5 => (json!({ "version": 1, "players": { "default": 5 } }), "players".into()),
            6 => (json!({ "version": 1, "profile": { "precision": [] } }), "profile".into()),
            7 => (json!({ "version": 1, "player": { "theme": 5 } }), "player".into()),
            8 => (json!({ "version": 1, "mix": { "index_limit_mib": "lots" } }), "mix".into()),
            9 => (json!({ "version": 1, "store": { "copy_on_write": "yes" } }), "store".into()),
            10 => (json!({ "version": 1, "keybinds": { "keys": { "playback.next": "]" } } }), "keybinds".into()),
            _ => {
                let id = tc.draw(gs::sampled_from(vec!["../up", "a b", ""]));
                (json!({ "version": 1, "plugins": { id: {} } }), "plugins".into())
            }
        };
        let file = dir.path().join("settings.json");
        fs::write(&file, doc.to_string()).unwrap();
        let req = ImportSettingsFile { file: file.clone(), replace };
        let err = api::settings_import_file_plan(&store, &req).unwrap_err().to_string();
        assert!(err.contains(&says) && err.contains(&file.display().to_string()), "{doc}: {err}");
        let err = locallibrary::settings::transfer::read(&file).unwrap_err();
        assert_eq!(err.code(), "parse", "{err}");
    }
    assert_eq!(
        fs::read_dir(store.root()).unwrap().count(),
        fs::read_dir(store_in(dir.path(), "fresh").root()).unwrap().count()
    );
}

/// A file the store keeps settings in that can't be read stops an export, and an import, which
/// compares the file with what's set here: nothing's made of what was set.
#[hegel::test(test_cases = 20)]
fn prop_a_store_file_that_cant_be_read_stops_export_and_import(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path(), "store");
    let damaged = tc.draw(gs::sampled_from(vec![
        "settings.json",
        "players.json",
        locallibrary::profile::PREFS_FILE,
        locallibrary::mix::prefs::PREFS_FILE,
        locallibrary::store::prefs::PREFS_FILE,
        "plugins/sound/settings.json",
        "keybinds.json",
    ]));
    let path = store.root().join(damaged);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, tc.draw(gs::sampled_from(vec![&b"not json"[..], b"{", b"[1, 2"]))).unwrap();
    assert!(api::settings_export(&store).is_err());
    let file = dir.path().join("import.json");
    fs::write(&file, json!({ "version": 1, "mix": { "index_limit_mib": 64 } }).to_string()).unwrap();
    let doc = locallibrary::settings::transfer::read(&file).unwrap();
    assert!(locallibrary::settings::transfer::merge(&store, &doc, tc.draw(gs::booleans())).is_err());
    assert!(api::settings_import_file_plan(&store, &ImportSettingsFile { file, replace: false }).is_err());
}

/// An import whose file can't be written where the store keeps it fails, saying so, and the
/// other sections' files are as they were.
#[hegel::test(test_cases = 10)]
fn prop_an_import_that_cant_be_written_fails(tc: TestCase) {
    use locallibrary::settings::transfer;
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path(), "store");
    let (section, kept_in) = tc.draw(gs::sampled_from(vec![
        (json!({ "my_settings": { "base": { "quantize": "off" } } }), "settings.json"),
        (json!({ "players": { "default": ["CDJ-3000"] } }), "players.json"),
        (json!({ "mix": { "index_limit_mib": 64 } }), locallibrary::mix::prefs::PREFS_FILE),
        (json!({ "keybinds": { "keys": { "playback.next": ["]"] } } }), "keybinds.json"),
    ]));
    let mut doc = section;
    doc["version"] = json!(1);
    let file = dir.path().join("import.json");
    fs::write(&file, doc.to_string()).unwrap();
    let merged = transfer::merge(&store, &transfer::read(&file).unwrap(), tc.draw(gs::booleans())).unwrap();
    assert!(!merged.changes.is_empty());
    fs::create_dir_all(store.root().join(kept_in).join("in the way")).unwrap();
    assert!(merged.write(&store).is_err());
    assert!(store.root().join(kept_in).is_dir());
}

/// The player's keys go out as `keybinds.json` keeps them, but for when they last changed here.
/// Importing them goes action by action: an action's keys are one value, its list never merged
/// with the one here, and an empty one (no key) is kept, not left out. What's imported is a change
/// made now.
#[test]
fn settings_export_carries_the_keybinds() {
    let dir = tempfile::tempdir().unwrap();
    let (from, into) = (store_in(dir.path(), "from"), store_in(dir.path(), "into"));
    let binds = |pairs: &[(&str, &[&str])], from: Option<&str>| {
        let mut out = Keybinds { from: from.map(String::from), ..Default::default() };
        out.updated_at = Some("2026-11-01T00:00:00.000Z".into());
        for (id, keys) in pairs {
            out.keys.insert(id.to_string(), keys.iter().map(|k| k.to_string()).collect());
        }
        out
    };
    let theirs =
        binds(&[("player.jobs", &[]), ("playback.next", &["]"]), ("visuals.sparkles", &["f6"])], Some("did:plc:abc"));
    keybinds::save(from.root(), &theirs).unwrap();
    keybinds::save(into.root(), &binds(&[("playback.next", &["n", "m"])], None)).unwrap();
    let file = exported(&from, dir.path());
    let export: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(
        export["keybinds"],
        json!({ "keys": { "playback.next": ["]"], "player.jobs": [], "visuals.sparkles": ["f6"] }, "from": "did:plc:abc" })
    );

    let plan =
        api::settings_import_file_plan(&into, &ImportSettingsFile { file: file.clone(), replace: false }).unwrap();
    let changes: Vec<(String, Action)> = plan.plan.changes.iter().map(|c| (c.path.join("."), c.action)).collect();
    assert_eq!(
        changes,
        [
            ("keybinds.keys.playback.next".to_string(), Action::Keep),
            ("keybinds.keys.player.jobs".into(), Action::Add),
            ("keybinds.keys.visuals.sparkles".into(), Action::Add),
            ("keybinds.from".into(), Action::Add),
        ]
    );
    let before = chrono::Utc::now();
    import(&into, &file, false);
    let kept = keybinds::load(into.root()).unwrap();
    assert_eq!(kept.keys["playback.next"], ["n", "m"], "the one here stays whole");
    assert_eq!(kept.keys["player.jobs"], Vec::<String>::new());
    assert_eq!(kept.from.as_deref(), Some("did:plc:abc"));
    let stamped = chrono::DateTime::parse_from_rfc3339(kept.updated_at.as_deref().unwrap()).unwrap();
    assert!(stamped >= before - chrono::Duration::seconds(1), "{stamped}");
    import(&into, &file, true);
    assert_eq!(keybinds::load(into.root()).unwrap().keys["playback.next"], ["]"]);
}

/// `keybinds.schema.json` beside the player's docs is what `Keybinds` makes. When the file
/// changes, regenerate it: run this test with `LOCALLIBRARY_BLESS=1`. Files the player writes fit
/// it, and one with a key list that isn't one doesn't.
#[test]
fn the_keybinds_schema_is_the_file() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/locallibrary/docs/keybinds.schema.json");
    let mut schema = schemars::generate::SchemaSettings::draft2020_12()
        .for_deserialize()
        .into_generator()
        .into_root_schema_for::<Keybinds>();
    schema.insert("$id".into(), "keybinds.schema.json".into());
    schema.insert("title".into(), "locallibrary keybinds".into());
    schema.insert(
        "description".into(),
        "keybinds.json: the keys set in locallibrary's player, beside tui.json. Only the actions whose keys \
         differ from the player's defaults, by id. An id the player doesn't know is kept and ignored, and a \
         key it can't read is skipped when the file is applied."
            .into(),
    );
    let mut made = serde_json::to_string_pretty(&schema).unwrap();
    made.push('\n');
    let kept = fs::read_to_string(&path).unwrap_or_default();
    if made != kept && std::env::var_os("LOCALLIBRARY_BLESS").is_some_and(|v| v == "1") {
        fs::write(&path, &made).unwrap();
    } else {
        assert!(
            made == kept,
            "apps/locallibrary/docs/keybinds.schema.json is out of date: Keybinds changed. Regenerate it with \
             `LOCALLIBRARY_BLESS=1 cargo nextest run -p locallibrary-core -E 'test(the_keybinds_schema_is_the_file)'`"
        );
    }
    let validator = jsonschema::validator_for(&serde_json::from_str(&made).unwrap()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut binds = Keybinds { from: Some("did:plc:abc".into()), ..Default::default() };
    binds.keys.insert("deck.hot-cue".into(), vec!["&".into(), "é".into()]);
    binds.keys.insert("player.jobs".into(), vec![]);
    let store = store_in(dir.path(), "store");
    keybinds::save(store.root(), &binds).unwrap();
    let written: Value = serde_json::from_slice(&fs::read(store.root().join("keybinds.json")).unwrap()).unwrap();
    assert!(validator.is_valid(&written), "{written}");
    assert!(!validator.is_valid(&json!({ "keys": { "player.jobs": "j" } })));
}

/// With no store, `settings export` (and the sidecar's) prints [`SettingsExport::default`]: the
/// file an empty store exports, no keys set in it.
#[test]
fn the_empty_export_is_an_empty_stores() {
    let dir = tempfile::tempdir().unwrap();
    let empty = json_of(&SettingsExport::default());
    assert_eq!(empty["keybinds"], json!({}), "{empty}");
    assert_eq!(empty, json_of(&api::settings_export(&store_in(dir.path(), "store")).unwrap()));
}
