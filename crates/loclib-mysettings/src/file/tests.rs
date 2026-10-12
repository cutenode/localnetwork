use super::*;
use crate::validate;

#[test]
fn damaged_files_are_refused_by_name() {
    let file = SettingsFile::MySetting;
    let good = SettingsBlob::default_for(file).as_bytes().to_vec();
    let refused = |bytes: Vec<u8>| SettingsBlob::parse(file, bytes).unwrap_err().to_string();
    assert!(refused(good[..10].to_vec()).ends_with("too short (10 bytes)"));
    let mut section = good.clone();
    section[0] = 0x61;
    assert!(refused(section).ends_with("unexpected string section length"));
    let mut longer = good.clone();
    longer.push(0);
    let message = refused(longer);
    assert!(message.contains("unexpected data length") && message.contains(file.file_name()), "{message}");
}

#[test]
fn values_the_schema_lacks_are_shown_as_bytes() {
    assert_eq!(SettingValue::Unknown(0x7f).to_string(), "unknown(0x7f)");
    assert_eq!(SettingValue::Known("on".into()).to_string(), "on");
}

#[test]
fn crc_reference_value() {
    // CRC-16/XMODEM check value from the CRC catalogue.
    assert_eq!(crc16_xmodem(b"123456789"), 0x31c3);
}

#[test]
fn defaults_roundtrip_and_validate() {
    for file in SettingsFile::ALL {
        let blob = SettingsBlob::default_for(file);
        assert!(blob.checksum_ok());
        let parsed = SettingsBlob::parse(file, blob.as_bytes().to_vec()).unwrap();
        for def in SCHEMA.iter().filter(|d| d.file == file) {
            assert_eq!(parsed.get(def), SettingValue::Known(def.default.into()));
        }
    }
}

#[test]
fn set_patches_one_byte() {
    let mut blob = SettingsBlob::default_for(SettingsFile::MySetting);
    let before = blob.as_bytes().to_vec();
    let (def, choice) = validate("quantize", "OFF").unwrap();
    assert!(blob.set(def, choice));
    assert!(!blob.set(def, choice));
    assert!(blob.checksum_ok());
    let diff = before.iter().zip(blob.as_bytes()).filter(|(a, b)| a != b).count();
    assert!((1..=3).contains(&diff), "value byte + up to two checksum bytes, got {diff}");
}
