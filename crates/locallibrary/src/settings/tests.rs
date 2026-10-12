use super::*;

#[test]
fn damaged_files_are_refused_by_name() {
    // Read from a file, the error names the file.
    let file = SettingsFile::MySetting;
    let good = SettingsBlob::default_for(file).as_bytes().to_vec();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(file.file_name());
    std::fs::write(&path, &good[..10]).unwrap();
    let err = read_blob(file, &path).unwrap_err();
    assert!(matches!(&err, Error::Parse { path: p, .. } if *p == path), "{err}");

    // Parsed from bytes, it names the kind of file.
    let err = Error::from(SettingsBlob::parse(file, good[..10].to_vec()).unwrap_err());
    assert!(matches!(&err, Error::Parse { path, .. } if path.as_os_str() == "MYSETTING.DAT"), "{err}");
    assert_eq!(err.to_string(), "could not parse MYSETTING.DAT: too short (10 bytes)");
}

#[test]
fn invalid_settings_say_what_would_resolve_them() {
    let unknown = validate("nope", "on").unwrap_err();
    assert_eq!((unknown.code(), unknown.to_string().as_str()), ("invalid", "unknown setting \"nope\""));
    let detail = unknown.detail();
    assert_eq!(detail.reason, Some(crate::error::Reason::UnknownSetting));
    assert_eq!(detail.fields["key"], "nope");
    assert_eq!(detail.actions[0].method.as_deref(), Some("settings.schema"));

    let value = validate("quantize", "maybe").unwrap_err();
    assert_eq!(value.to_string(), "invalid value \"maybe\" for quantize; expected one of: off, on");
    assert_eq!((value.code(), value.reason()), ("invalid", None));
}

#[test]
fn profile_overrides() {
    let mut p = SettingsProfile::default();
    let id = Uuid::new_v4();
    p.set(None, "quantize", "on").unwrap();
    p.set(None, "language", "english").unwrap();
    p.set(Some((id, "Club drive")), "language", "japanese").unwrap();
    assert_eq!(p.effective(None)["language"], "english");
    assert_eq!(p.effective(Some(id))["language"], "japanese");
    assert_eq!(p.effective(Some(id))["quantize"], "on");
    assert!(p.unset(Some(id), "language").unwrap());
    assert!(p.drives.is_empty());
}

#[test]
fn apply_creates_and_updates() {
    let dir = tempfile::tempdir().unwrap();
    let loc = DriveSettings(dir.path());
    let mut want = IndexMap::new();
    want.insert("quantize".to_string(), "off".to_string());
    let r = apply(&loc, &want, false, false).unwrap();
    assert_eq!(r.files_created, vec![SettingsFile::MySetting]);
    assert_eq!(read_all(&loc).values["quantize"], SettingValue::Known("off".into()));
    // Idempotent.
    let r = apply(&loc, &want, false, false).unwrap();
    assert!(r.files_written.is_empty() && r.changes.is_empty());
    // create_missing writes the remaining files with defaults.
    let r = apply(&loc, &want, true, false).unwrap();
    assert_eq!(r.files_created.len(), 3);
}
