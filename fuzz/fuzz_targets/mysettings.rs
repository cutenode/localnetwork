//! The players' settings files (`MYSETTING.DAT`, `MYSETTING2.DAT`, `DJMMYSETTING.DAT`,
//! `DEVSETTING.DAT`): parsing each kind, reading every setting, and planning an edit.

#![no_main]

use indexmap::IndexMap;
use libfuzzer_sys::fuzz_target;
use loclib_mysettings::{Error, SCHEMA, SettingsBlob, SettingsFile, SettingsSnapshot, changes};

fuzz_target!(|data: &[u8]| {
    // Every setting at its last choice, so most files have something to change.
    let desired: IndexMap<String, String> =
        SCHEMA.iter().filter_map(|def| Some((def.key.to_string(), def.choices.last()?.id.to_string()))).collect();
    for file in SettingsFile::ALL {
        let Ok(blob) = SettingsBlob::parse(file, data.to_vec()) else { continue };
        blob.version();
        SettingsSnapshot::default().insert(&blob);
        let existing = |f: SettingsFile| Ok::<_, Error>((f == file).then(|| blob.clone()));
        let _ = changes(existing, &desired, false);
    }
});
