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

/// My Settings that can't be read or written stop an apply, and leave the files as they were:
/// a settings file that can't be read or is damaged, a folder that can't be written; on a drive
/// also one another process has locked, or whose settings file is a link out of it (which only
/// the drive's apply looks for).
#[cfg(unix)]
#[hegel::test(test_cases = 20)]
fn prop_settings_that_cant_be_read_or_written_stop_an_apply(tc: TestCase) {
    use locallibrary::device::Device;
    use std::os::unix::fs::{PermissionsExt, symlink};
    let def = tc.draw(gs::sampled_from(SCHEMA.iter().filter(|d| d.choices.len() > 1).collect::<Vec<_>>()));
    let choice = tc.draw(gs::sampled_from(def.choices.iter().filter(|c| c.id != def.default).collect::<Vec<_>>()));
    let desired: IndexMap<String, String> = [(def.key.to_string(), choice.id.to_string())].into();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("USB");
    let usb = DriveSettings(&root);
    settings::apply(&usb, &IndexMap::new(), true, false).unwrap();
    let path = usb.settings_path(def.file);
    let folder = path.parent().unwrap().to_path_buf();
    let read_all = || SettingsFile::ALL.map(|f| fs::read(usb.settings_path(f)).ok());
    let fault = tc.draw(gs::integers::<u8>().max_value(4));
    let mut held = None;
    match fault {
        0 => fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap(),
        1 => fs::write(&path, b"not a settings file").unwrap(),
        2 => fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap(),
        3 => held = Some(Device::open(&root).unwrap().lock().unwrap()),
        _ => {
            let outside = dir.path().join("outside.DAT");
            fs::rename(&path, &outside).unwrap();
            symlink(&outside, &path).unwrap();
        }
    }
    let before = (fault != 0).then(read_all);
    let mut d = Device::open(&root).unwrap();
    let plain = (fault != 4).then(|| settings::apply(&usb, &desired, false, false));
    let on_drive = settings::apply_to_drive(&mut d, &desired, false, &locallibrary::progress::Ctx::silent());
    drop(held);
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
    if fault == 0 {
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    }
    match fault {
        3 => {
            assert!(matches!(on_drive, Err(locallibrary::Error::Locked(_))), "{on_drive:?}");
            assert!(plain.unwrap().is_ok_and(|r| !r.files_written.is_empty()));
            return;
        }
        4 => assert!(matches!(on_drive, Err(locallibrary::Error::Invalid(_))), "{on_drive:?}"),
        _ => {
            assert!(plain.unwrap().is_err());
            assert!(on_drive.is_err(), "{on_drive:?}");
        }
    }
    if let Some(before) = before {
        assert_eq!(read_all(), before);
    }
}
