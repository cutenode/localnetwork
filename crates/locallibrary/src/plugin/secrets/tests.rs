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
    let spec = PluginSpec::first_party(MANIFEST, launch).unwrap();
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
    let entry = slot.entry(&store, "token").unwrap();
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
            let entry = slot.entry(&store, "token").unwrap();
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
