use super::*;

use hegel::TestCase;
use hegel::generators as gs;

/// What's set is kept in the store and read back as it was; nothing set reads as the default
/// limit, and a limit too big to count in bytes is as good as none.
#[hegel::test(test_cases = 50)]
fn the_index_limit_is_kept_in_the_store(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path()).unwrap();
    assert_eq!(MixPrefs::load(&store).unwrap().index_limit(), DEFAULT_INDEX_LIMIT_MIB << 20);
    let prefs = MixPrefs { index_limit_mib: tc.draw(gs::optional(gs::integers::<u64>())) };
    prefs.save(&store).unwrap();
    let kept = MixPrefs::load(&store).unwrap();
    assert_eq!(kept, prefs);
    let limit = prefs.index_limit_mib.unwrap_or(DEFAULT_INDEX_LIMIT_MIB);
    let bytes = if limit <= u64::MAX >> 20 { limit << 20 } else { u64::MAX };
    assert_eq!(kept.index_limit(), bytes);
}

/// Settings that don't read as settings are an error naming the file, never the defaults: a
/// limit someone set mustn't silently become another.
#[hegel::test(test_cases = 50)]
fn prop_settings_that_dont_read_are_an_error(tc: TestCase) {
    use hegel::Generator;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path()).unwrap();
    let file = store.root().join(PREFS_FILE);
    let junk = tc.draw(hegel::one_of!(
        gs::text().max_size(20).map(|t| format!("{{{t}")),
        gs::just("\"512\"".to_string()),
        gs::integers::<i64>().map(|n| format!("{{\"index_limit_mib\": {n}.5}}")),
    ));
    std::fs::write(&file, &junk).unwrap();
    let e = MixPrefs::load(&store).unwrap_err();
    assert!(matches!(&e, crate::Error::Parse { path, .. } if *path == file), "{junk:?}: {e}");
}
