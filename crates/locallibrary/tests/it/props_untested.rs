//! What the operating system a thread answers for changes: the warnings about what hasn't been
//! tested on it.

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::format;

/// On a thread answering for Linux or Windows, anything planned warns that it hasn't been tested
/// there, naming it and the system; for macOS (or any other), and for this Mac itself once the
/// thread answers for itself again, nothing does. Only the last answer counts.
#[hegel::test(test_cases = 100)]
fn only_linux_and_windows_warn_about_untested_steps(tc: TestCase) {
    let what = tc.draw(gs::sampled_from(vec!["ejecting", "formatting", "the file-system check", "a b/c"]));
    let systems: Vec<Option<&'static str>> =
        tc.draw(gs::vecs(gs::sampled_from(vec![Some("linux"), Some("windows"), Some("macos"), Some("freebsd"), None])));
    for &os in &systems {
        locallibrary::testing::os_on_this_thread(os);
    }
    let os = systems.last().copied().flatten().unwrap_or(std::env::consts::OS);
    let named = match os {
        "linux" => Some("Linux"),
        "windows" => Some("Windows"),
        _ => None,
    };
    let warning = format::untested_here(what);
    locallibrary::testing::os_on_this_thread(None);
    assert_eq!(warning, named.map(|n| format!("{what} hasn't been tested on {n} yet; check the result")));
}
