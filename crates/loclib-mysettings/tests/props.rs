//! Properties of parsing, editing and planning changes to My Settings files, over bytes in
//! memory. locallibrary's `props_settings` checks the same files against rekordcrate's parser and
//! real drives.

use std::cell::Cell;

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use indexmap::IndexMap;
use loclib_mysettings::{
    Error, SCHEMA, SettingDef, SettingValue, SettingsBlob, SettingsFile, SettingsSnapshot, changes, crc16_xmodem, find,
    validate,
};

const HEADER_LEN: usize = 104;

/// The length of a file's data section, from the file rekordbox would write.
fn data_len(file: SettingsFile) -> usize {
    SettingsBlob::default_for(file).as_bytes().len() - HEADER_LEN - 4
}

fn defs(file: SettingsFile) -> impl Iterator<Item = &'static SettingDef> {
    SCHEMA.iter().filter(move |d| d.file == file)
}

#[hegel::test_helper]
fn settings_file(tc: &TestCase) -> SettingsFile {
    tc.draw(gs::sampled_from(&SettingsFile::ALL))
}

/// A setting of `file` and one of its choices' indexes.
#[hegel::test_helper]
fn setting(tc: &TestCase, file: SettingsFile) -> (&'static SettingDef, usize) {
    let defs: Vec<_> = defs(file).collect();
    let d = tc.draw(gs::sampled_from(&defs));
    (d, tc.draw(gs::integers::<usize>().max_value(d.choices.len() - 1)))
}

/// rekordbox's defaults for `file` with a few settings changed.
#[hegel::test_helper]
fn edited(tc: &TestCase, file: SettingsFile) -> SettingsBlob {
    let mut blob = SettingsBlob::default_for(file);
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
        let (def, c) = setting(tc, file);
        blob.set(def, &def.choices[c]);
    }
    blob
}

/// Every kind of file is rekordbox's defaults as it writes them: the right length, a valid
/// checksum, version `0.001`, every setting at its default and nothing undecoded; parsing its
/// bytes gives it back, and it lives in `PIONEER/` under its own name.
#[hegel::test(test_cases = 20)]
fn default_files_hold_every_default(tc: TestCase) {
    let file = settings_file(&tc);
    let blob = SettingsBlob::default_for(file);
    assert_eq!(blob.as_bytes().len(), HEADER_LEN + data_len(file) + 4);
    assert!(blob.checksum_ok());
    assert_eq!(blob.version(), "0.001");
    assert_eq!(SettingsBlob::parse(file, blob.as_bytes().to_vec()).unwrap(), blob);
    assert_eq!(file.key(), format!("PIONEER/{}", file.file_name()));
    let mut snap = SettingsSnapshot::default();
    snap.insert(&blob);
    assert!(snap.problems.is_empty() && snap.undecoded.is_empty() && snap.missing_files.is_empty());
    let keys: Vec<&str> = snap.values.keys().map(String::as_str).collect();
    assert_eq!(keys, defs(file).map(|d| d.key).collect::<Vec<_>>());
    for def in defs(file) {
        assert_eq!(snap.values[def.key], SettingValue::Known(def.default.into()));
    }
}

/// Bytes parse exactly when they're as long as the file's kind, start with the string section's
/// length and give the right data length; then they're kept as they are. Anything else (random
/// bytes, a file cut short or run on, another kind's file) is a parse error naming the file.
#[hegel::test(test_cases = 500)]
fn parse_accepts_exactly_the_right_shape(tc: TestCase) {
    let file = settings_file(&tc);
    let mut bytes = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => tc.draw(gs::binary().max_size(200)),
        _ => {
            let source = settings_file(&tc);
            edited(&tc, source).as_bytes().to_vec()
        }
    };
    if tc.draw(gs::booleans()) {
        let len = tc.draw(gs::integers::<usize>().max_value(bytes.len() + 8));
        bytes.resize(len, tc.draw(gs::integers::<u8>()));
    }
    if bytes.len() > 4 && tc.draw(gs::weighted_booleans(0.2)) {
        let i = tc.draw(gs::sampled_from(vec![0usize, 100]));
        if i + 4 <= bytes.len() {
            bytes[i] ^= tc.draw(gs::integers::<u8>().min_value(1));
        }
    }
    let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    let len = HEADER_LEN + data_len(file) + 4;
    let shaped = bytes.len() == len && u32_at(0) == 0x60 && u32_at(100) as usize == data_len(file);
    match SettingsBlob::parse(file, bytes.clone()) {
        Ok(blob) => {
            assert!(shaped);
            assert_eq!((blob.file, blob.as_bytes()), (file, bytes.as_slice()));
        }
        Err(e) => {
            assert!(!shaped);
            let message = e.to_string();
            assert!(matches!(e, Error::Parse { file: f, .. } if f == file));
            assert!(message.starts_with(&format!("could not parse {}: ", file.file_name())), "{message}");
            if bytes.len() < HEADER_LEN + 4 {
                assert!(message.ends_with(&format!("too short ({} bytes)", bytes.len())), "{message}");
            }
        }
    }
}

/// The checksum covers a file's data section (and, in DJMMYSETTING.DAT, the header too), so
/// changing any byte it covers, or the checksum itself, is a checksum problem and nothing else
/// is. Planning no changes repairs exactly the checksum and keeps every other byte; a file with
/// a sound checksum isn't written.
#[hegel::test(test_cases = 300)]
fn checksum_covers_the_data_and_for_djm_the_header(tc: TestCase) {
    let file = settings_file(&tc);
    let data_end = HEADER_LEN + data_len(file);
    let mut bytes = edited(&tc, file).as_bytes().to_vec();
    // Any byte but the string section's and data's lengths, which parsing checks.
    let at = tc.draw(
        gs::integers::<usize>().max_value(bytes.len() - 1).filter(|i| !(0..4).contains(i) && !(100..104).contains(i)),
    );
    bytes[at] ^= tc.draw(gs::integers::<u8>().min_value(1));
    let blob = SettingsBlob::parse(file, bytes.clone()).unwrap();
    let covered_from = if file == SettingsFile::DjmMySetting { 0 } else { HEADER_LEN };
    let checked = (covered_from..data_end + 2).contains(&at);
    assert_eq!(blob.checksum_ok(), !checked, "byte {at}");
    assert_eq!(blob.stored_checksum(), u16::from_le_bytes([bytes[data_end], bytes[data_end + 1]]));

    let mut snap = SettingsSnapshot::default();
    snap.insert(&blob);
    let problems: Vec<String> = if checked { vec![format!("{}: checksum mismatch", file.file_name())] } else { vec![] };
    assert_eq!(snap.problems, problems);

    let existing = |f: SettingsFile| Ok::<_, Error>((f == file).then(|| blob.clone()));
    let (report, written) = changes(existing, &IndexMap::new(), false).unwrap();
    assert!(report.changes.is_empty() && report.files_created.is_empty());
    if checked {
        assert_eq!(report.files_written, [file]);
        let repaired = SettingsBlob::parse(file, written[0].1.clone()).unwrap();
        assert!(repaired.checksum_ok());
        let differing: Vec<usize> = (0..bytes.len()).filter(|&i| bytes[i] != repaired.as_bytes()[i]).collect();
        assert!(differing.iter().all(|i| (data_end..data_end + 2).contains(i)), "{differing:?}");
    } else {
        assert!(report.files_written.is_empty() && written.is_empty());
    }
}

/// A setting's byte reads as the choice that has it, or as the raw byte (shown in hex) when none
/// does; setting a choice reads back as it, with a sound checksum whenever something changed.
#[hegel::test(test_cases = 300)]
fn values_read_back_as_choices_or_raw_bytes(tc: TestCase) {
    let file = settings_file(&tc);
    let (def, c) = setting(&tc, file);
    let byte = tc.draw(hegel::one_of!(
        gs::sampled_from(def.choices.iter().map(|c| c.byte).collect::<Vec<_>>()),
        gs::integers::<u8>()
    ));
    let mut bytes = SettingsBlob::default_for(file).as_bytes().to_vec();
    bytes[HEADER_LEN + def.offset] = byte;
    let mut blob = SettingsBlob::parse(file, bytes).unwrap();
    assert_eq!(blob.raw(def), byte);
    let value = blob.get(def);
    match def.choices.iter().find(|c| c.byte == byte) {
        Some(choice) => {
            assert_eq!(value, SettingValue::Known(choice.id.into()));
            assert_eq!(value.to_string(), choice.id);
        }
        None => {
            assert_eq!(value, SettingValue::Unknown(byte));
            let hex = value.to_string().strip_prefix("unknown(0x").and_then(|s| s.strip_suffix(')')).map(String::from);
            assert_eq!(hex.and_then(|h| u8::from_str_radix(&h, 16).ok()), Some(byte), "{value}");
        }
    }
    let choice = &def.choices[c];
    let changed = blob.set(def, choice);
    assert_eq!(changed, byte != choice.byte);
    assert_eq!(blob.get(def), SettingValue::Known(choice.id.into()));
    assert_eq!(blob.raw(def), choice.byte);
    if changed {
        assert!(blob.checksum_ok());
    }
}

/// A settings file before planning, as a test sets it up.
#[derive(Debug, Clone)]
enum Seed {
    Missing,
    Edited,
    /// With its checksum broken.
    Stale,
    /// With a byte no choice has at one setting's offset.
    UnknownValue(usize),
    /// With a byte off its default outside every known setting, as newer rekordbox writes.
    Undecoded(usize, u8),
}

#[hegel::test_helper]
fn seeded(tc: &TestCase, file: SettingsFile) -> Option<SettingsBlob> {
    let seed = match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => Seed::Missing,
        1 => Seed::Edited,
        2 => Seed::Stale,
        3 => Seed::UnknownValue(tc.draw(gs::integers::<usize>().max_value(defs(file).count() - 1))),
        _ => {
            let free: Vec<usize> = (0..data_len(file)).filter(|o| defs(file).all(|d| d.offset != *o)).collect();
            Seed::Undecoded(tc.draw(gs::sampled_from(&free)), tc.draw(gs::integers()))
        }
    };
    let mut bytes = match seed {
        Seed::Missing => return None,
        _ => edited(tc, file).as_bytes().to_vec(),
    };
    match seed {
        Seed::Stale => bytes[HEADER_LEN + data_len(file)] ^= 0x5a,
        Seed::UnknownValue(d) => bytes[HEADER_LEN + defs(file).nth(d).unwrap().offset] = 0x7f,
        Seed::Undecoded(offset, value) => bytes[HEADER_LEN + offset] = value,
        Seed::Missing | Seed::Edited => {}
    }
    Some(SettingsBlob::parse(file, bytes).unwrap())
}

fn snapshot(files: &IndexMap<SettingsFile, SettingsBlob>) -> SettingsSnapshot {
    let mut snap = SettingsSnapshot::default();
    files.values().for_each(|b| snap.insert(b));
    snap
}

/// Planning makes every wanted setting (by id or label) what was asked and keeps every other
/// value and undecoded byte; it creates the missing files it needs (all of them with
/// `create_missing`) from the defaults, reports a change for each wanted value that differs or
/// lands in a new file, writes files with sound checksums, and planning again finds nothing.
#[hegel::test(test_cases = 200)]
fn planned_changes_reach_the_wanted_values_and_settle(tc: TestCase) {
    let mut files: IndexMap<SettingsFile, SettingsBlob> =
        SettingsFile::ALL.iter().filter_map(|&f| seeded(&tc, f).map(|b| (f, b))).collect();
    let mut desired = IndexMap::new();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(6)) {
        let file = settings_file(&tc);
        let (def, c) = setting(&tc, file);
        let choice = &def.choices[c];
        let by_label = tc.draw(gs::booleans());
        desired.insert(def.key.to_string(), if by_label { choice.label } else { choice.id }.to_string());
    }
    let wanted = |def: &SettingDef| desired.get(def.key).map(|v| validate(def.key, v).unwrap().1);
    let create_missing = tc.draw(gs::booleans());
    let before = files.clone();
    let reads = Cell::new(0);
    let existing = |f: SettingsFile| {
        reads.set(reads.get() + 1);
        Ok::<_, Error>(before.get(&f).cloned())
    };
    let (report, written) = changes(existing, &desired, create_missing).unwrap();
    assert_eq!(reads.get(), SettingsFile::ALL.len());
    assert!(!report.dry_run);

    let touched = |f: SettingsFile| defs(f).any(|d| desired.contains_key(d.key));
    let created: Vec<SettingsFile> =
        SettingsFile::ALL.into_iter().filter(|f| !before.contains_key(f) && (create_missing || touched(*f))).collect();
    assert_eq!(report.files_created, created);
    let written_files: Vec<SettingsFile> = written.iter().map(|(f, _)| *f).collect();
    assert_eq!(report.files_written, written_files);
    for (f, bytes) in written {
        let blob = SettingsBlob::parse(f, bytes).unwrap();
        assert!(blob.checksum_ok(), "{f:?}");
        files.insert(f, blob);
    }
    let mut expected_changes = Vec::new();
    for f in SettingsFile::ALL {
        let old = before.get(&f);
        for def in defs(f) {
            let Some(blob) = files.get(&f) else {
                assert!(!created.contains(&f));
                continue;
            };
            let now = blob.get(def);
            match wanted(def) {
                Some(choice) => {
                    assert_eq!(now, SettingValue::Known(choice.id.into()), "{}", def.key);
                    let from = old.map(|b| b.get(def));
                    if from.as_ref() != Some(&now) {
                        expected_changes.push((def.key, from, choice.id));
                    }
                }
                None => {
                    let kept = old.map_or(SettingValue::Known(def.default.into()), |b| b.get(def));
                    assert_eq!(now, kept, "{}", def.key);
                }
            }
        }
    }
    // Changes come file by file, each in the order the settings were asked for.
    expected_changes.sort_by_key(|(key, ..)| {
        let def = find(key).unwrap();
        (SettingsFile::ALL.iter().position(|f| *f == def.file), desired.get_index_of(*key))
    });
    let got: Vec<_> = report.changes.iter().map(|c| (c.key.as_str(), c.from.clone(), c.to.as_str())).collect();
    assert_eq!(got, expected_changes);
    assert!(report.changes.iter().all(|c| find(&c.key).unwrap().file == c.file));
    let undecoded = |snap: SettingsSnapshot| {
        snap.undecoded.into_iter().filter(|u| before.contains_key(&u.file)).collect::<Vec<_>>()
    };
    assert_eq!(undecoded(snapshot(&files)), undecoded(snapshot(&before)));
    assert!(snapshot(&files).problems.is_empty());

    let (again, written) = changes(|f| Ok::<_, Error>(files.get(&f).cloned()), &desired, create_missing).unwrap();
    assert!(again.changes.is_empty() && again.files_written.is_empty() && written.is_empty(), "{again:?}");
}

/// A key no setting has, or a value none of a setting's choices is, fails validation naming
/// what was given (and, for a value, every choice), and fails planning before any file is read.
#[hegel::test(test_cases = 300)]
fn bad_keys_and_values_are_refused_before_anything_is_read(tc: TestCase) {
    let file = settings_file(&tc);
    let (def, c) = setting(&tc, file);
    let key = tc.draw(hegel::one_of!(gs::just(def.key.to_string()), gs::text().max_size(10)));
    let value = tc.draw(hegel::one_of!(
        gs::just(def.choices[c].id.to_string()),
        gs::just(def.choices[c].label.to_string()),
        gs::text().max_size(8)
    ));
    let result = validate(&key, &value);
    match (find(&key), &result) {
        (None, Err(Error::UnknownSetting(k))) => {
            assert_eq!(k, &key);
            assert_eq!(result.as_ref().unwrap_err().to_string(), format!("unknown setting {key:?}"));
        }
        (Some(found), Err(Error::InvalidValue { key: k, value: v, expected })) => {
            assert_eq!((*k, v), (found.key, &value));
            assert_eq!(expected, &found.choices.iter().map(|c| c.id).collect::<Vec<_>>());
            assert!(found.choice(&value).is_none());
            let message = result.as_ref().unwrap_err().to_string();
            assert!(message.ends_with(&format!("expected one of: {}", expected.join(", "))), "{message}");
        }
        (Some(found), Ok((d, choice))) => {
            assert_eq!(d.key, found.key);
            assert_eq!(found.choice(&value).map(|c| c.id), Some(choice.id));
        }
        (found, result) => panic!("{key:?} = {value:?}: {:?} gave {result:?}", found.map(|d| d.key)),
    }
    if key == def.key && (value == def.choices[c].id || value == def.choices[c].label) {
        assert_eq!(result.as_ref().map(|(_, choice)| choice.id).ok(), Some(def.choices[c].id));
    }

    let mut desired: IndexMap<String, String> = [("quantize".to_string(), "off".to_string())].into();
    desired.insert(key.clone(), value.clone());
    let read = Cell::new(false);
    let planned = changes(
        |_| {
            read.set(true);
            Ok::<_, Error>(None)
        },
        &desired,
        false,
    );
    // `quantize` may have been overwritten by the drawn pair; either way planning stands or
    // falls with validation.
    let all_valid = desired.iter().all(|(k, v)| validate(k, v).is_ok());
    assert_eq!(planned.is_ok(), all_valid);
    assert_eq!(read.get(), all_valid);
}

/// CRC-16/XMODEM starts from zero with no final xor, so it's linear: the CRC of two equally
/// long inputs xored together is their CRCs xored.
#[hegel::test(test_cases = 200)]
fn crc_is_linear(tc: TestCase) {
    let a = tc.draw(gs::binary().max_size(120));
    let b = tc.draw(gs::binary().min_size(a.len()).max_size(a.len()));
    let both: Vec<u8> = a.iter().zip(&b).map(|(x, y)| x ^ y).collect();
    assert_eq!(crc16_xmodem(&both), crc16_xmodem(&a) ^ crc16_xmodem(&b));
}
