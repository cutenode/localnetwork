use super::*;

/// A plugin being checked is given nothing: the host refuses whatever it calls back for.
#[test]
fn a_checked_plugin_is_given_nothing() {
    assert!(options().isolated);
}
