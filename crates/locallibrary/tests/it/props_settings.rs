//! Property tests for the players' "My Settings" files (`*SETTING.DAT`): the CRC, single-setting
//! patches against rekordcrate's parser, lenient key/value lookup, `apply` and profile drift.

use crate::common;

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;
use std::sync::OnceLock;

use binrw::{BinRead, BinWrite};
use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::settings::{
    self, DriveSettings, SCHEMA, SettingDef, SettingValue, SettingsBlob, SettingsFile, SettingsLocation,
    SettingsProfile,
};
use uuid::Uuid;

const HEADER_LEN: usize = 104;

fn data_len(file: SettingsFile) -> usize {
    match file {
        SettingsFile::MySetting | SettingsFile::MySetting2 => 40,
        SettingsFile::DjmMySetting => 52,
        SettingsFile::DevSetting => 32,
    }
}

/// Every real file of each kind: rekordcrate's per-setting fixtures and the demo export's.
fn real_files(file: SettingsFile) -> &'static [Vec<u8>] {
    static FILES: OnceLock<BTreeMap<SettingsFile, Vec<Vec<u8>>>> = OnceLock::new();
    &FILES.get_or_init(|| {
        let mut out: BTreeMap<SettingsFile, Vec<Vec<u8>>> = BTreeMap::new();
        for f in SettingsFile::ALL {
            let dir = common::fixtures()
                .join("rekordcrate/settings")
                .join(f.file_name().trim_end_matches(".DAT").to_lowercase());
            let mut cases: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
            cases.sort();
            let files = out.entry(f).or_default();
            files.extend(cases.iter().map(|c| std::fs::read(c.join(f.file_name())).unwrap()));
            files.push(std::fs::read(common::fixtures().join("rekordcrate/demo_tracks").join(f.key())).unwrap());
        }
        out
    })[&file]
}

#[hegel::test_helper]
fn settings_file(tc: &TestCase) -> SettingsFile {
    let i = tc.draw(gs::integers::<usize>().max_value(3));
    SettingsFile::ALL[i]
}

/// A file to edit: the defaults, or one rekordbox wrote.
#[hegel::test_helper]
fn base_blob(tc: &TestCase, file: SettingsFile) -> SettingsBlob {
    let files = real_files(file);
    let which = tc.draw(gs::integers::<usize>().max_value(files.len()));
    match files.get(which) {
        Some(bytes) => SettingsBlob::parse(file, bytes.clone()).unwrap(),
        None => SettingsBlob::default_for(file),
    }
}

/// A setting of `file` (or of any file) and one of its choices.
#[hegel::test_helper]
fn setting(tc: &TestCase, file: Option<SettingsFile>) -> (&'static SettingDef, usize) {
    let defs: Vec<&'static SettingDef> = SCHEMA.iter().filter(|d| file.is_none_or(|f| d.file == f)).collect();
    let d = tc.draw(gs::integers::<usize>().max_value(defs.len() - 1));
    let c = tc.draw(gs::integers::<usize>().max_value(defs[d].choices.len() - 1));
    (defs[d], c)
}

/// `s` as a person might type it: any case, `-`, `_` (and, with `spaces`, a space) between
/// words, and blanks around it.
#[hegel::test_helper]
fn spelled(tc: &TestCase, s: &str, spaces: bool) -> String {
    let case = tc.draw(gs::integers::<u8>().max_value(2));
    let cased = match case {
        0 => s.to_string(),
        1 => s.to_uppercase(),
        _ => s.to_lowercase(),
    };
    let separators: &[char] = if spaces { &['-', '_', ' '] } else { &['-', '_'] };
    let mut out = String::new();
    for (i, ch) in cased.chars().enumerate() {
        if i > 0 && matches!(ch, '-' | '_' | ' ') {
            let sep = tc.draw(gs::integers::<usize>().max_value(separators.len() - 1));
            out.push(separators[sep]);
        } else {
            out.push(ch);
        }
    }
    let pad = tc.draw(gs::sampled_from(vec!["", " ", "\t", "  "]));
    format!("{pad}{out}{pad}")
}

/// CRC-16/XMODEM appended big-endian leaves a zero remainder, and every single-bit error changes
/// the CRC.
#[hegel::test]
fn crc_residue_is_zero_and_single_bit_errors_are_caught(tc: TestCase) {
    let data = tc.draw(gs::binary().max_size(300));
    let crc = settings::crc16_xmodem(&data);
    let mut framed = data.clone();
    framed.extend_from_slice(&crc.to_be_bytes());
    assert_eq!(settings::crc16_xmodem(&framed), 0);
    if !data.is_empty() {
        let bit = tc.draw(gs::integers::<usize>().max_value(data.len() * 8 - 1));
        let mut flipped = data.clone();
        flipped[bit / 8] ^= 1 << (bit % 8);
        assert_ne!(settings::crc16_xmodem(&flipped), crc);
    }
}

/// Setting values one after another: each reads back, the checksum stays valid, a change
/// touches at most the value byte and the two checksum bytes, `set` reports whether anything
/// changed, and in the end every byte but the values set and the checksum is as it was.
#[hegel::test]
fn set_patches_only_the_value_and_checksum(tc: TestCase) {
    let file = settings_file(&tc);
    let mut blob = base_blob(&tc, file);
    let original = blob.clone();
    let steps = tc.draw(gs::integers::<usize>().min_value(1).max_value(8));
    let mut touched = vec![HEADER_LEN + data_len(file), HEADER_LEN + data_len(file) + 1];
    for _ in 0..steps {
        let (def, c) = setting(&tc, Some(file));
        let choice = &def.choices[c];
        let before = blob.as_bytes().to_vec();
        let changed = blob.set(def, choice);
        let after = blob.as_bytes();
        assert_eq!(changed, before != after);
        assert_eq!(blob.get(def), SettingValue::Known(choice.id.into()));
        assert!(blob.checksum_ok());
        let diff = before.iter().zip(after).filter(|(a, b)| a != b).count();
        assert!(diff <= 3, "{diff} bytes changed setting {} to {}", def.key, choice.id);
        touched.push(HEADER_LEN + def.offset);
    }
    for (i, (a, b)) in original.as_bytes().iter().zip(blob.as_bytes()).enumerate() {
        assert!(a == b || touched.contains(&i), "byte {i} changed");
    }
    for def in SCHEMA.iter().filter(|d| d.file == file && !touched.contains(&(HEADER_LEN + d.offset))) {
        assert_eq!(blob.get(def), original.get(def), "{}", def.key);
    }
}

/// rekordcrate, an independent parser, reads every file with any choices set, and writing its
/// parse back (with the CRC it computes) gives the same bytes: its model of every value and its
/// checksum agree with ours.
#[hegel::test]
fn rekordcrate_reads_and_rewrites_files_identically(tc: TestCase) {
    use rekordcrate::setting::Setting;
    let file = settings_file(&tc);
    let mut blob = base_blob(&tc, file);
    let steps = tc.draw(gs::integers::<usize>().max_value(8));
    for _ in 0..steps {
        let (def, c) = setting(&tc, Some(file));
        blob.set(def, &def.choices[c]);
    }
    let parsed = Setting::read(&mut Cursor::new(blob.as_bytes())).unwrap_or_else(|e| panic!("rekordcrate: {e}"));
    let mut out = Cursor::new(Vec::new());
    parsed.write_options(&mut out, binrw::Endian::Little, (false,)).unwrap();
    assert_eq!(out.into_inner(), blob.as_bytes());
}

/// Every choice is found by its id or its label however it's spelled (case, `-`/`_`/space,
/// blanks around it), and every setting by its key the same way.
#[hegel::test(test_cases = 300)]
fn lookup_is_lenient_about_spelling(tc: TestCase) {
    let (def, c) = setting(&tc, None);
    let choice = &def.choices[c];
    let use_label = tc.draw(gs::booleans());
    let value = spelled(&tc, if use_label { choice.label } else { choice.id }, true);
    let key = spelled(&tc, def.key, true);
    let (found, chosen) = settings::validate(&key, &value).unwrap_or_else(|e| panic!("{key:?} = {value:?}: {e}"));
    assert_eq!((found.key, chosen.id), (def.key, choice.id), "{key:?} = {value:?}");
}

/// Shrunk counterexamples of `lookup_is_lenient_about_spelling`: labels with non-ASCII capitals
/// typed in another case, and keys typed with spaces like their labels.
#[test]
fn lookup_regressions() {
    assert_eq!(settings::validate("language", "FRANÇAIS").unwrap().1.id, "french");
    assert_eq!(settings::validate("language", "čeština").unwrap().1.id, "czech");
    assert_eq!(settings::validate("language", "ΕΛΛΗΝΙΚΆ").unwrap().1.id, "greek");
    assert_eq!(settings::validate("auto cue", "off").unwrap().0.key, "auto_cue");
}

/// A settings file on a drive, as a test sets it up.
#[derive(Debug, Clone)]
enum Seed {
    Missing,
    Defaults,
    Real(usize),
    /// A real file with its checksum broken.
    Stale(usize),
    /// A real file with a byte no known choice has at one setting's offset.
    UnknownValue(usize, usize),
}

#[hegel::test_helper]
fn seed(tc: &TestCase, file: SettingsFile) -> Seed {
    let real = real_files(file).len() - 1;
    match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => Seed::Missing,
        1 => Seed::Defaults,
        2 => Seed::Real(tc.draw(gs::integers::<usize>().max_value(real))),
        3 => Seed::Stale(tc.draw(gs::integers::<usize>().max_value(real))),
        _ => {
            let defs = SCHEMA.iter().filter(|d| d.file == file).count();
            Seed::UnknownValue(
                tc.draw(gs::integers::<usize>().max_value(real)),
                tc.draw(gs::integers::<usize>().max_value(defs - 1)),
            )
        }
    }
}

fn plant(loc: &DriveSettings, file: SettingsFile, seed: &Seed) {
    let real = |i: usize| real_files(file)[i].clone();
    let bytes = match *seed {
        Seed::Missing => return,
        Seed::Defaults => SettingsBlob::default_for(file).as_bytes().to_vec(),
        Seed::Real(i) => real(i),
        Seed::Stale(i) => {
            let mut b = real(i);
            b[HEADER_LEN + data_len(file)] ^= 0x5a;
            b
        }
        Seed::UnknownValue(i, d) => {
            let mut b = real(i);
            let def = SCHEMA.iter().filter(|d| d.file == file).nth(d).unwrap();
            b[HEADER_LEN + def.offset] = 0x7f;
            b
        }
    };
    let path = loc.settings_path(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn files_on(root: &Path) -> Vec<(SettingsFile, Option<Vec<u8>>)> {
    let loc = DriveSettings(root);
    SettingsFile::ALL.iter().map(|&f| (f, std::fs::read(loc.settings_path(f)).ok())).collect()
}

/// `apply` makes a drive's settings what was asked (by canonical key, by id or label), keeps
/// every other value and unknown byte, repairs checksums, creates only the files it needs (or
/// all with `create_missing`), writes nothing on a dry run but reports the same, and a second
/// run finds nothing to do.
#[hegel::test(test_cases = 100)]
fn apply_reaches_the_wanted_values_and_is_idempotent(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let loc = DriveSettings(dir.path());
    let seeds: Vec<Seed> = SettingsFile::ALL.iter().map(|&f| seed(&tc, f)).collect();
    for (f, s) in SettingsFile::ALL.iter().zip(&seeds) {
        plant(&loc, *f, s);
    }
    let mut desired = BTreeMap::new();
    let wanted = tc.draw(gs::integers::<usize>().max_value(6));
    for _ in 0..wanted {
        let (def, c) = setting(&tc, None);
        let by_label = tc.draw(gs::booleans());
        let choice = &def.choices[c];
        desired.insert(def.key.to_string(), if by_label { choice.label } else { choice.id }.to_string());
    }
    let create_missing = tc.draw(gs::booleans());
    let before = settings::read_all(&loc);
    let files_before = files_on(dir.path());

    let dry = settings::apply(&loc, &desired, create_missing, true).unwrap();
    assert_eq!(files_on(dir.path()), files_before, "a dry run wrote something");
    let report = settings::apply(&loc, &desired, create_missing, false).unwrap();
    assert_eq!(format!("{:?}", dry.changes), format!("{:?}", report.changes));
    assert_eq!((&dry.files_written, &dry.files_created), (&report.files_written, &report.files_created));

    let after = settings::read_all(&loc);
    assert!(after.problems.is_empty(), "{:?}", after.problems);
    for def in SCHEMA {
        let present = after.values.contains_key(def.key);
        let was_missing = before.missing_files.contains(&def.file);
        let wanted = desired.get(def.key).map(|v| settings::validate(def.key, v).unwrap().1.id);
        assert_eq!(
            present,
            !was_missing || create_missing || SCHEMA.iter().any(|d| d.file == def.file && desired.contains_key(d.key))
        );
        if !present {
            continue;
        }
        let expect = match wanted {
            Some(id) => SettingValue::Known(id.into()),
            None if was_missing => SettingValue::Known(def.default.into()),
            None => before.values[def.key].clone(),
        };
        assert_eq!(after.values[def.key], expect, "{}", def.key);
    }
    let undecoded = |s: &settings::SettingsSnapshot| format!("{:?}", s.undecoded);
    assert_eq!(undecoded(&after), undecoded(&before));

    let files = files_on(dir.path());
    let again = settings::apply(&loc, &desired, create_missing, false).unwrap();
    assert!(again.changes.is_empty() && again.files_written.is_empty(), "{again:?}");
    assert_eq!(files_on(dir.path()), files);
}

/// A profile built with `set`/`unset` (keys and values spelled any way) and applied to a drive
/// leaves no drift for that drive, before and after a JSON round trip of the profile; and a
/// profile imported from a drive has no drift against it.
#[hegel::test(test_cases = 100)]
fn no_drift_after_applying_a_profile(tc: TestCase) {
    let drives = [Uuid::from_u128(1), Uuid::from_u128(2)];
    let mut profile = SettingsProfile::default();
    let edits = tc.draw(gs::integers::<usize>().max_value(12));
    for _ in 0..edits {
        let (def, c) = setting(&tc, None);
        let target = tc.draw(gs::integers::<usize>().max_value(2));
        let drive = drives.get(target).copied();
        let key = spelled(&tc, def.key, false);
        let unset = tc.draw(gs::weighted_booleans(0.2));
        if unset {
            profile.unset(drive, &key).unwrap();
        } else {
            let use_label = tc.draw(gs::booleans());
            let value = spelled(&tc, if use_label { def.choices[c].label } else { def.choices[c].id }, true);
            profile.set(drive.map(|d| (d, "drive")), &key, &value).unwrap();
        }
    }
    let drive = drives[tc.draw(gs::integers::<usize>().max_value(1))];
    let dir = tempfile::tempdir().unwrap();
    let loc = DriveSettings(dir.path());
    for f in SettingsFile::ALL {
        let s = seed(&tc, f);
        plant(&loc, f, &s);
    }
    settings::apply(&loc, &profile.effective(Some(drive)), false, false).unwrap();
    let snap = settings::read_all(&loc);
    assert!(
        settings::drift(&profile, Some(drive), &snap).is_empty(),
        "{:?}",
        settings::drift(&profile, Some(drive), &snap)
    );
    let json = serde_json::to_string(&profile).unwrap();
    let back: SettingsProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(back.effective(Some(drive)), profile.effective(Some(drive)));
    assert!(settings::drift(&back, Some(drive), &snap).is_empty());

    let mut imported = SettingsProfile::default();
    imported.import_base(&snap);
    assert!(settings::drift(&imported, None, &snap).is_empty());
}

/// Any bytes of the right length parse, and reading, checking and setting values on them never
/// panics; other lengths are refused. Damaged files on a drive are reported, not fatal.
#[hegel::test]
fn arbitrary_bytes_never_panic(tc: TestCase) {
    let file = settings_file(&tc);
    let exact = tc.draw(gs::booleans());
    let bytes = if exact {
        let mut b =
            tc.draw(gs::binary().min_size(HEADER_LEN + data_len(file) + 4).max_size(HEADER_LEN + data_len(file) + 4));
        b[..4].copy_from_slice(&0x60u32.to_le_bytes());
        b[100..104].copy_from_slice(&(data_len(file) as u32).to_le_bytes());
        b
    } else {
        tc.draw(gs::binary().max_size(200))
    };
    match SettingsBlob::parse(file, bytes.clone()) {
        Ok(mut blob) => {
            let _ = (blob.checksum_ok(), blob.stored_checksum(), blob.version());
            for def in SCHEMA.iter().filter(|d| d.file == file) {
                let _ = blob.get(def);
                // A change recomputes the checksum (an unchanged value leaves a stale one).
                if blob.set(def, &def.choices[0]) {
                    assert!(blob.checksum_ok());
                }
            }
        }
        Err(_) => assert!(!exact),
    }
    let dir = tempfile::tempdir().unwrap();
    let loc = DriveSettings(dir.path());
    let path = loc.settings_path(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let snap = settings::read_all(&loc);
    let sound = SettingsBlob::parse(file, bytes).is_ok_and(|b| b.checksum_ok());
    assert_eq!(snap.problems.is_empty(), sound, "{:?}", snap.problems);
}
