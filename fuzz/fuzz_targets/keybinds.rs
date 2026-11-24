//! Someone else's keybinds record, as the player imports it: the record read back, then each key it
//! kept spelled again and read again. The input as text is a key spelled any way at all.

#![no_main]

use libfuzzer_sys::fuzz_target;
use locallibrary::keybinds::{Key, Keybinds};

/// A key read from `spelled` reads the same again from how it's written, and writes the same.
fn round_trips(spelled: &str) {
    let Ok(key) = Key::parse(spelled) else { return };
    let written = key.to_string();
    assert_eq!(Key::parse(&written), Ok(key), "{spelled:?} read as {key:?}, written {written:?}");
    assert_eq!(Key::parse(&written).map(|k| k.to_string()).as_deref(), Ok(written.as_str()));
}

fuzz_target!(|data: &[u8]| {
    if let Ok(record) = serde_json::from_slice(data)
        && let Ok((binds, _skipped)) = Keybinds::from_record(&record)
    {
        for key in binds.keys.values().flatten() {
            // What a record keeps is spelled as `Key` writes it, so it reads back as itself.
            assert_eq!(Key::parse(key).map(|k| k.to_string()).as_deref(), Ok(key.as_str()), "kept {key:?}");
        }
    }
    if let Ok(text) = std::str::from_utf8(data) {
        round_trips(text);
    }
});
