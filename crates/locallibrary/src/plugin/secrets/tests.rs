use hegel::TestCase;
use hegel::generators::{self as gs, Generator};
use serde_json::{Value, json};

use super::*;
use crate::plugin::{Launch, Options, PluginSpec, Reads};
use crate::progress::Ctx;

struct NoReads;

impl Reads for NoReads {
    fn read(&self, _: &str, _: Value, _: &Ctx<'_>) -> Result<Value, crate::api::RpcFailure> {
        unreachable!("never started")
    }
}

const MANIFEST: &str = r#"
    id = "keeper"
    name = "Keeper"
    version = "0.1.0"
    protocol = "^0.2"

    [[contributes.settings]]
    key = "token"
    title = "Token"
    type = "secret"

    [[contributes.settings]]
    key = "name"
    title = "Name"
    type = "string"
    default = "me"
"#;

/// A host for a plugin with a secret setting, keeping secrets in `secrets`, and a store.
fn host(secrets: Storage) -> (Plugins, tempfile::TempDir, Store) {
    let launch = Launch { program: "nothing".into(), args: Vec::new(), env_remove: Vec::new() };
    let spec = PluginSpec::built_in(MANIFEST, launch).unwrap();
    let plugins = Plugins::new(vec![spec], std::sync::Arc::new(NoReads), Options { secrets, ..Options::default() });
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path()).unwrap();
    (plugins.unwrap(), dir, store)
}

/// A secret is kept apart from the settings, wherever it's kept: never in `settings.json` or
/// the settings hooks get, only said to be set; forgotten when reset.
#[test]
fn a_secret_is_kept_apart_and_only_said_to_be_set() {
    crate::keychain::tests::mock_keychain();
    for storage in [Storage::File, Storage::Keychain] {
        let (plugins, _dir, store) = host(storage);
        let settings_file = store.root().join("plugins/keeper/settings.json");
        assert_eq!(plugins.secrets("keeper", Some(&store)).unwrap(), IndexMap::from([("token".into(), false)]));
        assert!(!plugins.secrets("keeper", None).unwrap()["token"], "no store, no secret");
        let settings = plugins.set_setting("keeper", &store, "token", Some(json!("s3cr3t"))).unwrap();
        assert_eq!(settings, IndexMap::from([("name".into(), json!("me"))]), "{storage:?}");
        assert!(plugins.secrets("keeper", Some(&store)).unwrap()["token"]);
        assert_eq!(plugins.slot("keeper").unwrap().secret(&store, "token").unwrap().as_deref(), Some("s3cr3t"));
        assert!(!settings_file.exists() || !std::fs::read_to_string(&settings_file).unwrap().contains("s3cr3t"));
        // Apart from the plugin's own folder.
        let file = store.root().join("secrets/plugins/keeper.json");
        assert_eq!(file.exists(), storage == Storage::File, "{storage:?}");
        #[cfg(unix)]
        if storage == Storage::File {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&file), 0o600, "only you read it");
            let folders = [store.root().join("secrets"), file.parent().unwrap().to_path_buf()];
            assert_eq!(folders.map(|f| mode(&f)), [0o700; 2], "nor the folders it's in");
        }

        // Never said back, even when it's wrong.
        let e = plugins.set_setting("keeper", &store, "token", Some(json!(""))).unwrap_err();
        assert_eq!(e.to_string(), "token is a secret: some text");
        plugins.set_setting("keeper", &store, "name", Some(json!("you"))).unwrap();
        assert!(plugins.secrets("keeper", Some(&store)).unwrap()["token"], "other settings leave it");
        plugins.set_setting("keeper", &store, "token", None).unwrap();
        assert!(!plugins.secrets("keeper", Some(&store)).unwrap()["token"], "{storage:?}");
        assert!(!file.exists(), "a file holding none is removed");
        // Forgetting one that isn't set is nothing.
        plugins.set_setting("keeper", &store, "token", None).unwrap();
    }
}

/// Two stores on one computer keep a plugin's secrets apart in the keychain.
#[test]
fn each_store_has_its_own_keychain_item() {
    crate::keychain::tests::mock_keychain();
    let (one, _a, store_one) = host(Storage::Keychain);
    let (two, _b, store_two) = host(Storage::Keychain);
    one.set_setting("keeper", &store_one, "token", Some(json!("one"))).unwrap();
    assert!(!two.secrets("keeper", Some(&store_two)).unwrap()["token"]);
    two.set_setting("keeper", &store_two, "token", Some(json!("two"))).unwrap();
    assert_eq!(one.slot("keeper").unwrap().secret(&store_one, "token").unwrap().as_deref(), Some("one"));
}

/// A keychain that fails says so, and how to keep secrets without it.
#[test]
fn a_failing_keychain_says_how_to_do_without() {
    crate::keychain::tests::mock_keychain();
    let (plugins, _dir, store) = host(Storage::Keychain);
    let slot = plugins.slot("keeper").unwrap();
    let entry = entry(&slot.place(&store, "token")).unwrap();
    let cred: &keyring_core::mock::Cred = entry.as_any().downcast_ref().unwrap();
    cred.set_error(keyring::Error::NoStorageAccess(std::io::Error::other("locked").into()));
    let e = slot.secret(&store, "token").unwrap_err().to_string();
    assert!(e.starts_with("the system keychain failed: "), "{e}");
    assert!(e.ends_with("LOCALLIBRARY_SESSION_STORE=file to keep secrets in files only you can read"), "{e}");
}

/// A secrets file where the first builds with secrets kept it, in the plugin's own folder, is
/// moved out of it the first time it's read, and its secrets kept.
#[test]
fn a_secrets_file_in_the_plugins_folder_is_moved_out_of_it() {
    let (plugins, _dir, store) = host(Storage::File);
    let old = store.root().join("plugins/keeper/secrets.json");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(&old, r#"{ "token": "kept" }"#).unwrap();
    assert!(plugins.secrets("keeper", Some(&store)).unwrap()["token"]);
    assert!(!old.exists() && store.root().join("secrets/plugins/keeper.json").is_file());
    assert_eq!(plugins.slot("keeper").unwrap().secret(&store, "token").unwrap().as_deref(), Some("kept"));
}

/// Makes `file` unreadable or readable again. Whether it worked is for the caller to check: root
/// reads anything.
#[cfg(unix)]
fn set_mode(file: &std::path::Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// A secrets file that can't be read, in a folder that can be written, keeps its keys: the change
/// is refused with the read's error, where it used to replace the file with the one new key.
#[cfg(unix)]
#[test]
fn a_secrets_file_that_cant_be_read_keeps_its_other_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path()).unwrap();
    let file = store.root().join("secrets/plugins/keeper.json");
    let place = |key: &str| Place {
        store: &store,
        service: "locallibrary-plugin-keeper".into(),
        file: file.clone(),
        key: key.into(),
        moved_from: None,
    };
    set_secret(Storage::File, &place("a"), Some("1")).unwrap();
    set_secret(Storage::File, &place("b"), Some("2")).unwrap();
    let before = std::fs::read(&file).unwrap();
    set_mode(&file, 0o000);
    if std::fs::read(&file).is_ok() {
        return;
    }
    let refused = |e: Error| matches!(e, Error::Io { path, .. } if path == file);
    let (set, forgot) =
        (set_secret(Storage::File, &place("c"), Some("3")), set_secret(Storage::File, &place("a"), None));
    set_mode(&file, 0o600);
    assert!(refused(set.unwrap_err()));
    assert!(refused(forgot.unwrap_err()));
    assert_eq!(std::fs::read(&file).unwrap(), before);
}

/// A secrets file left in the plugin's folder that can't be moved out of it is still its secrets:
/// setting another one is refused, where it used to start a new file that hid the old one for good.
#[cfg(unix)]
#[test]
fn a_secrets_file_that_cant_be_moved_stays_the_plugins_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path()).unwrap();
    let old = store.root().join("plugins/keeper/secrets.json");
    let file = store.root().join("secrets/plugins/keeper.json");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(&old, r#"{"a":"1"}"#).unwrap();
    let place = Place {
        store: &store,
        service: "locallibrary-plugin-keeper".into(),
        file: file.clone(),
        key: "b".into(),
        moved_from: Some(old.clone()),
    };
    set_mode(old.parent().unwrap(), 0o555);
    let set = set_secret(Storage::File, &place, Some("2"));
    set_mode(old.parent().unwrap(), 0o755);
    // Root moves it anyway.
    if !old.exists() {
        return;
    }
    assert!(matches!(set.unwrap_err(), Error::Io { path, .. } if path == old));
    assert!(!file.exists());
}

/// A change to any of a plugin's files that can't be read right now is refused with the read's
/// error, and the file stays exactly as it was. Once it can be read, the change goes through and
/// nothing else in the file is lost.
#[cfg(unix)]
#[hegel::test(test_cases = 40)]
fn a_change_to_a_file_that_cant_be_read_loses_nothing(tc: TestCase) {
    type Change<'a> = Box<dyn Fn() -> crate::Result<()> + 'a>;
    let (plugins, _dir, store) = host(Storage::File);
    let slot = plugins.slot("keeper").unwrap();
    let text = || gs::from_regex("[a-z0-9]{1,6}").fullmatch(true);
    let (first, other) = (tc.draw(text()), tc.draw(text()));
    let value: Option<Value> = tc.draw(gs::optional(text())).map(Value::from);
    let (file, change): (std::path::PathBuf, Change<'_>) = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => {
            plugins.set_setting("keeper", &store, "token", Some(json!(first))).unwrap();
            set_secret(Storage::File, &slot.place(&store, "other"), Some(&other)).unwrap();
            let change = Box::new(|| plugins.set_setting("keeper", &store, "token", value.clone()).map(drop));
            (slot.place(&store, "token").file, change)
        }
        1 => {
            plugins.set_setting("keeper", &store, "name", Some(json!(first))).unwrap();
            let file = store.root().join("plugins/keeper/settings.json");
            let mut kept: IndexMap<String, Value> = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
            kept.insert("other".into(), json!(other));
            std::fs::write(&file, serde_json::to_vec(&kept).unwrap()).unwrap();
            (file, Box::new(|| plugins.set_setting("keeper", &store, "name", value.clone()).map(drop)))
        }
        _ => {
            crate::plugin::set_host_setting(&store, "changed", Some(json!(first))).unwrap();
            crate::plugin::set_host_setting(&store, "other", Some(json!(other))).unwrap();
            let change = Box::new(|| crate::plugin::set_host_setting(&store, "changed", value.clone()));
            (store.root().join("plugins/settings.json"), change)
        }
    };
    let before = std::fs::read(&file).unwrap();
    set_mode(&file, 0o000);
    // Root reads anything.
    if std::fs::read(&file).is_err() {
        let refused = change();
        set_mode(&file, 0o600);
        let refused = refused.unwrap_err();
        assert!(matches!(&refused, Error::Io { path, .. } if *path == file), "{refused}");
        assert_eq!(std::fs::read(&file).unwrap(), before);
    }
    set_mode(&file, 0o600);
    change().unwrap();
    let kept: IndexMap<String, Value> = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(kept["other"], json!(other));
    // Damaged, it has nothing usable in it, and the same change writes it anew.
    std::fs::write(&file, "not json").unwrap();
    change().unwrap();
    assert_ne!(std::fs::read(&file).ok().as_deref(), Some(&b"not json"[..]));
}

/// For any secrets set, forgotten and set wrong, kept in either place, with the keychain failing
/// now and then: the secret is said to be set exactly while the last change that went through set
/// it, and is what that set; a wrong one or a failing keychain changes nothing, and the keychain
/// failing says how to do without it; the secret is never among the settings, and the other
/// setting is as it was.
#[hegel::test(test_cases = 60)]
fn a_secret_is_what_the_last_change_made_it(tc: TestCase) {
    crate::keychain::tests::mock_keychain();
    let storage = tc.draw(gs::sampled_from(vec![Storage::File, Storage::Keychain]));
    let (plugins, _dir, store) = host(storage);
    let slot = plugins.slot("keeper").unwrap();
    let mut kept: Option<String> = None;
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(6)) {
        let value: Option<Value> = tc.draw(gs::optional(hegel::one_of!(
            gs::from_regex("[a-z0-9 ]{1,8}").fullmatch(true).map(Value::from),
            gs::just(json!("")),
            gs::just(json!(7)),
        )));
        let fails = storage == Storage::Keychain && tc.draw(gs::booleans());
        if fails {
            let entry = entry(&slot.place(&store, "token")).unwrap();
            let cred: &keyring_core::mock::Cred = entry.as_any().downcast_ref().unwrap();
            cred.set_error(keyring::Error::NoStorageAccess(std::io::Error::other("locked").into()));
        }
        let wrong = value.as_ref().is_some_and(|v| v.as_str().is_none_or(str::is_empty));
        let set = plugins.set_setting("keeper", &store, "token", value.clone());
        match (wrong, fails) {
            (true, _) => assert_eq!(set.unwrap_err().to_string(), "token is a secret: some text"),
            (false, true) => {
                let e = set.unwrap_err().to_string();
                assert!(e.ends_with("LOCALLIBRARY_SESSION_STORE=file to keep secrets in files only you can read"));
            }
            (false, false) => {
                assert_eq!(set.unwrap(), IndexMap::from([("name".into(), json!("me"))]));
                kept = value.and_then(|v| v.as_str().map(String::from));
            }
        }
        // A wrong one never reached the keychain: its error is still to come.
        if fails && wrong {
            assert!(slot.secret(&store, "token").is_err());
        }
        assert_eq!(plugins.secrets("keeper", Some(&store)).unwrap()["token"], kept.is_some());
        assert_eq!(slot.secret(&store, "token").unwrap(), kept);
        assert!(!plugins.settings("keeper", Some(&store)).unwrap().contains_key("token"));
    }
}

/// How a store's secrets can't be read or kept, as
/// [`prop_secrets_that_cant_be_read_or_kept_fail_and_stay`] draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Unkept {
    /// The secrets file isn't JSON.
    Damaged,
    /// The store's id can't be read (so its keychain item can't be named).
    NoStoreId,
    /// A file where the folder for secrets files goes.
    NoFolder,
    /// The first builds' secrets file, with no folder to move it to.
    OldNoFolder,
    /// The first builds' secrets file, with a folder it can't be moved into (on Unix).
    OldUnmovable,
}

/// For any way a store's secrets can't be read (a damaged file, a store whose id can't be read,
/// an old file that can't be moved where they're kept now), whether a secret is set can't be said
/// and the secret can't be read; for one where they can't be kept, setting one fails. A plugin
/// that isn't there has none. The old file stays where it was.
#[hegel::test(test_cases = 1)]
fn prop_secrets_that_cant_be_read_or_kept_fail_and_stay(tc: TestCase) {
    for unkept in [Unkept::Damaged, Unkept::NoStoreId, Unkept::NoFolder, Unkept::OldNoFolder, Unkept::OldUnmovable] {
        prop_secrets_that_cant_be_read_or_kept_fail_and_stay_case(&tc, unkept);
    }
}

/// Makes `dir`, if there is one, a folder the host can write in or can't (on Unix).
fn writable(dir: Option<&std::path::Path>, can: bool) {
    #[cfg(not(unix))]
    let _ = (dir, can);
    #[cfg(unix)]
    if let Some(dir) = dir {
        use std::os::unix::fs::PermissionsExt;
        let mode = if can { 0o700 } else { 0o500 };
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).unwrap();
    }
}

/// [`prop_secrets_that_cant_be_read_or_kept_fail_and_stay`] for one `unkept`.
#[hegel::test_helper]
fn prop_secrets_that_cant_be_read_or_kept_fail_and_stay_case(tc: &TestCase, unkept: Unkept) {
    crate::keychain::tests::mock_keychain();
    let storage = match unkept {
        Unkept::NoStoreId => Storage::Keychain,
        _ => Storage::File,
    };
    let (plugins, _dir, store) = host(storage);
    assert_eq!(plugins.secrets("nope", Some(&store)).unwrap_err().code(), "not_found");
    let slot = plugins.slot("keeper").unwrap();
    let secrets = store.root().join("secrets");
    let old = store.root().join("plugins/keeper/secrets.json");
    let secret: String = tc.draw(gs::from_regex("[a-z0-9]{1,8}").fullmatch(true));
    let blocked = (unkept == Unkept::OldUnmovable).then(|| secrets.join("plugins"));
    match unkept {
        Unkept::Damaged => {
            std::fs::create_dir_all(secrets.join("plugins")).unwrap();
            std::fs::write(secrets.join("plugins/keeper.json"), "damaged").unwrap();
        }
        Unkept::NoStoreId => std::fs::write(store.root().join("store.json"), "damaged").unwrap(),
        Unkept::NoFolder | Unkept::OldNoFolder => std::fs::write(&secrets, "").unwrap(),
        Unkept::OldUnmovable => std::fs::create_dir_all(secrets.join("plugins")).unwrap(),
    }
    if matches!(unkept, Unkept::OldNoFolder | Unkept::OldUnmovable) {
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, json!({ "token": secret }).to_string()).unwrap();
    }
    writable(blocked.as_deref(), false);
    let said = plugins.secrets("keeper", Some(&store));
    let read = slot.secret(&store, "token");
    let set = plugins.set_setting("keeper", &store, "token", Some(json!(secret)));
    writable(blocked.as_deref(), true);
    if unkept == Unkept::OldUnmovable && cfg!(not(unix)) {
        return;
    }
    match unkept {
        // One that can't be read is written anew.
        Unkept::Damaged => assert!(set.is_ok(), "{set:?}"),
        _ => assert!(set.is_err(), "{unkept:?}: {set:?}"),
    }
    if unkept != Unkept::NoFolder {
        assert!(said.is_err() && read.is_err(), "{unkept:?}");
    }
    if matches!(unkept, Unkept::OldNoFolder | Unkept::OldUnmovable) {
        assert!(old.is_file(), "the old file stays");
    }
}
