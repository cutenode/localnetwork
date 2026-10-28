use super::*;

#[test]
fn publishing_needs_settings_and_importing_needs_someone() {
    let dir = tempfile::tempdir().unwrap();
    let err = settings_publish_plan(dir.path()).unwrap_err();
    assert!(err.to_string().contains("settings profile is empty"), "{err}");
    let err = settings_import_published_plan(dir.path(), &ImportPublishedSettings::default()).unwrap_err();
    assert_eq!(err.code(), "account");
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none(), "nothing written");

    // Signed out, a plan shows the record without an account to write it to.
    let store = Store::open_or_init(dir.path()).unwrap();
    let mut profile = store.settings_profile().unwrap();
    profile.set(None, "quantize", "off").unwrap();
    store.save_settings_profile(&profile).unwrap();
    let plan = settings_publish_plan(dir.path()).unwrap();
    assert!(plan.account.is_none() && plan.uri.is_none() && plan.current.is_none());
    assert_eq!(plan.record["player"]["quantize"], "off");
    let err = settings_publish_apply(dir.path()).unwrap_err();
    assert!(err.to_string().contains("not signed in"), "{err}");
}
