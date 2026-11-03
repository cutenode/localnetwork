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
