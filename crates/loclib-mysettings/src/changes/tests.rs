use super::*;

/// [`changes`] over files kept in memory.
fn plan(
    files: &IndexMap<SettingsFile, SettingsBlob>,
    desired: &[(&str, &str)],
    create_missing: bool,
) -> (ApplyReport, NewFiles) {
    let desired = desired.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    changes(|f| Ok::<_, Error>(files.get(&f).cloned()), &desired, create_missing).unwrap()
}

#[test]
fn changes_create_update_and_repair() {
    let mut files = IndexMap::new();
    let (report, new) = plan(&files, &[("quantize", "off")], false);
    assert_eq!(report.files_created, vec![SettingsFile::MySetting]);
    assert_eq!(report.changes[0].from, None);
    for (f, bytes) in new {
        files.insert(f, SettingsBlob::parse(f, bytes).unwrap());
    }
    let mut snap = SettingsSnapshot::default();
    snap.insert(&files[&SettingsFile::MySetting]);
    assert_eq!(snap.values["quantize"], SettingValue::Known("off".into()));
    assert!(snap.problems.is_empty() && snap.undecoded.is_empty());

    // Idempotent.
    let (report, new) = plan(&files, &[("quantize", "off")], false);
    assert!(report.changes.is_empty() && new.is_empty());
    // create_missing writes the remaining files with defaults.
    assert_eq!(plan(&files, &[], true).0.files_created.len(), 3);

    // A stale checksum is repaired, and reported.
    let mut bytes = files[&SettingsFile::MySetting].as_bytes().to_vec();
    let crc = bytes.len() - 4;
    bytes[crc] ^= 0xff;
    files.insert(SettingsFile::MySetting, SettingsBlob::parse(SettingsFile::MySetting, bytes).unwrap());
    let mut snap = SettingsSnapshot::default();
    snap.insert(&files[&SettingsFile::MySetting]);
    assert_eq!(snap.problems, ["MYSETTING.DAT: checksum mismatch"]);
    let (report, new) = plan(&files, &[], false);
    assert!(report.changes.is_empty() && report.files_written == [SettingsFile::MySetting]);
    assert!(SettingsBlob::parse(SettingsFile::MySetting, new[0].1.clone()).unwrap().checksum_ok());

    // An unknown key fails before any file is read.
    let err = changes(|_| -> Result<_, Error> { unreachable!() }, &[("nope".into(), "on".into())].into(), false);
    assert_eq!(err.unwrap_err().to_string(), "unknown setting \"nope\"");
}
