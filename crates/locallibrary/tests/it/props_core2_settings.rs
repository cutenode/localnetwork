//! My Settings in a plain folder of `.DAT` files (as rekordbox keeps its own) are written and read
//! exactly as on a drive, and a damaged one is a problem naming its file.

use std::fs;

use hegel::TestCase;
use hegel::generators as gs;
use indexmap::IndexMap;
use locallibrary::settings::{self, DriveSettings, FlatSettings, SCHEMA, SettingsFile, SettingsLocation};

#[hegel::test(test_cases = 10)]
fn settings_in_a_plain_folder_are_a_drives(tc: TestCase) {
    let mut desired = IndexMap::new();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        let def = tc.draw(gs::sampled_from(SCHEMA.iter().collect::<Vec<_>>()));
        let choice = tc.draw(gs::sampled_from(def.choices));
        desired.insert(def.key.to_string(), choice.id.to_string());
    }
    let dir = tempfile::tempdir().unwrap();
    let (folder, drive) = (dir.path().join("rekordbox6"), dir.path().join("USB"));
    fs::create_dir_all(&folder).unwrap();
    let (flat, usb) = (FlatSettings(&folder), DriveSettings(&drive));
    settings::apply(&flat, &desired, true, false).unwrap();
    settings::apply(&usb, &desired, true, false).unwrap();
    for f in SettingsFile::ALL {
        assert_eq!(flat.settings_path(f), folder.join(f.file_name()));
        assert_eq!(fs::read(flat.settings_path(f)).unwrap(), fs::read(usb.settings_path(f)).unwrap(), "{f:?}");
    }
    let read = settings::read_all(&flat);
    assert_eq!(read.values, settings::read_all(&usb).values);
    for (key, id) in &desired {
        assert_eq!(read.values[key.as_str()], settings::SettingValue::Known(id.clone()), "{key}");
    }

    let damaged = tc.draw(gs::sampled_from(&SettingsFile::ALL));
    fs::write(flat.settings_path(damaged), b"not a settings file").unwrap();
    let read = settings::read_all(&flat);
    assert_eq!(read.problems.len(), 1, "{:?}", read.problems);
    assert!(read.problems[0].contains(damaged.file_name()), "{:?}", read.problems);
}
