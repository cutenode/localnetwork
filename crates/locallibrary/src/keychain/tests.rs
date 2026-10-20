/// Keychain items go to an in-memory keychain in this test binary, never the computer's.
pub(crate) fn mock_keychain() {
    static MOCK: std::sync::Once = std::sync::Once::new();
    MOCK.call_once(|| {
        // keyring sets the system's keychain as the default the first time it's used.
        let _ = keyring::Entry::store_status();
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
    });
}
