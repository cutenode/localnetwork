//! Properties of the equipment table's lookups, checked against equipment.json, the table as
//! it's published.

use std::sync::OnceLock;

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use loclib_equipment::{Codec, Exfat, FromPath, FsKind, MODELS, Model, Playback, UnknownModels, Unplayable};
use serde_json::Value;

const FS_KINDS: [FsKind; 8] = [
    FsKind::Fat12,
    FsKind::Fat16,
    FsKind::Fat32,
    FsKind::Exfat,
    FsKind::Hfsplus,
    FsKind::Apfs,
    FsKind::Ntfs,
    FsKind::Other,
];

fn table() -> &'static [Value] {
    static TABLE: OnceLock<Vec<Value>> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(include_str!("../equipment.json")).expect("equipment.json is JSON"))
}

#[hegel::test_helper]
fn model(tc: &TestCase) -> (usize, &'static Model) {
    let i = tc.draw(gs::integers::<usize>().max_value(MODELS.len() - 1));
    (i, &MODELS[i])
}

#[hegel::test_helper]
fn codec(tc: &TestCase) -> Codec {
    tc.draw(gs::sampled_from(Codec::ALL.to_vec()))
}

/// `name` as a person might type it: any case, `NXS` as `Nexus`, and spaces, dashes,
/// underscores or dots between any two characters.
#[hegel::test_helper]
fn respelled(tc: &TestCase, name: &str) -> String {
    let name = if tc.draw(gs::booleans()) { name.replace("NXS", "Nexus") } else { name.to_string() };
    let mut out = String::new();
    for ch in name.chars() {
        out.push_str(&tc.draw(gs::text().alphabet(" -_.").max_size(2)));
        out.push(if tc.draw(gs::booleans()) { ch.to_ascii_lowercase() } else { ch });
    }
    out
}

/// Every model is found by its name however it's spelled.
#[hegel::test(test_cases = 300)]
fn every_model_is_found_however_it_is_spelled(tc: TestCase) {
    let (_, m) = model(&tc);
    let spelled = respelled(&tc, m.name);
    assert_eq!(loclib_equipment::find(&spelled).map(|f| f.name), Some(m.name), "{spelled:?}");
}

/// Any text gets an answer, and the same one in any ASCII case or without what isn't an ASCII
/// letter or digit; whatever it finds is a model that its own name finds.
#[hegel::test(test_cases = 500)]
fn find_ignores_case_and_punctuation(tc: TestCase) {
    let text = tc.draw(hegel::one_of!(
        gs::text().max_size(16),
        gs::sampled_from(MODELS.iter().map(|m| m.name.to_string()).collect::<Vec<_>>())
            .flat_map(|name| { gs::text().max_size(3).map(move |extra| format!("{name}{extra}")) }),
    ));
    let found = loclib_equipment::find(&text).map(|m| m.name);
    assert_eq!(loclib_equipment::find(&text.to_ascii_lowercase()).map(|m| m.name), found);
    assert_eq!(loclib_equipment::find(&text.to_ascii_uppercase()).map(|m| m.name), found);
    let bare: String = text.chars().filter(char::is_ascii_alphanumeric).collect();
    assert_eq!(loclib_equipment::find(&bare).map(|m| m.name), found);
    if let Some(name) = found {
        assert_eq!(loclib_equipment::find(name).map(|m| m.name), Some(name));
    }
}

/// A name in a list to make canonical.
#[derive(Debug, Clone)]
enum Entry {
    Known(usize, String),
    Blank(String),
    /// Starts with `ZZ`, as no model does.
    Unknown(String),
}

/// Known names come back as the table spells them, each once, in the order first given; blank
/// ones are skipped; any unknown name fails the whole list, naming each unknown one (trimmed)
/// in order and listing every model.
#[hegel::test(test_cases = 300)]
fn canonical_names_each_model_once_or_every_unknown_name(tc: TestCase) {
    let mut entries = Vec::new();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(6)) {
        let pad = tc.draw(gs::sampled_from(vec!["", " ", "\t "]));
        entries.push(match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 => Entry::Blank(pad.to_string()),
            1 => Entry::Unknown(format!("{pad}ZZ{}{pad}", tc.draw(gs::text().alphabet("0123456789-X").max_size(4)))),
            _ => {
                let (i, m) = model(&tc);
                Entry::Known(i, format!("{pad}{}{pad}", respelled(&tc, m.name)))
            }
        });
    }
    let names: Vec<String> = entries
        .iter()
        .map(|e| match e {
            Entry::Known(_, s) | Entry::Blank(s) | Entry::Unknown(s) => s.clone(),
        })
        .collect();
    let unknown: Vec<String> = entries
        .iter()
        .filter_map(|e| match e {
            Entry::Unknown(s) => Some(s.trim().to_string()),
            _ => None,
        })
        .collect();
    match loclib_equipment::canonical(&names) {
        Ok(found) => {
            assert!(unknown.is_empty());
            let mut expected: Vec<String> = Vec::new();
            for e in &entries {
                if let Entry::Known(i, _) = e
                    && !expected.iter().any(|n| n == MODELS[*i].name)
                {
                    expected.push(MODELS[*i].name.to_string());
                }
            }
            assert_eq!(found, expected);
        }
        Err(e) => {
            assert_eq!(e, UnknownModels(unknown.clone()));
            let message = e.to_string();
            assert!(message.starts_with(&format!("not a known model: {} (known models: ", unknown.join(", "))));
            assert!(MODELS.iter().all(|m| message.contains(m.name)), "{message}");
        }
    }
}

/// Whether a model plays a file is what its published row lists: a format it doesn't list
/// doesn't play (as sure as the list is), and one it lists plays at a listed or unknown (0)
/// rate and, for lossless formats, at a listed or unknown depth, as sure as that format's entry.
/// An unknown rate or depth never makes a file unplayable, and a file wrong on both counts is
/// what the two single checks say.
#[hegel::test(test_cases = 500)]
fn plays_follows_the_published_table(tc: TestCase) {
    let (i, m) = model(&tc);
    let codec = codec(&tc);
    let rate = tc.draw(hegel::one_of!(
        gs::sampled_from(vec![0, 16_000, 22_050, 32_000, 44_100, 48_000, 88_200, 96_000, 192_000]),
        gs::integers::<u32>()
    ));
    let depth = tc.draw(hegel::one_of!(gs::sampled_from(vec![0, 8, 16, 24, 32]), gs::integers::<u16>()));
    let row = &table()[i];
    assert_eq!(row["name"], m.name);
    let name = codec.name().to_ascii_lowercase();
    let entry = row["audio"].as_array().unwrap().iter().find(|a| a["codec"] == name.as_str());
    assert_eq!(m.audio(codec).is_some(), entry.is_some());
    let expected = match entry {
        None => Playback { result: Err(Unplayable::Format), inferred: row["audio_inferred"].as_bool().unwrap() },
        Some(a) => {
            let listed = |field: &str, v: u64| a[field].as_array().unwrap().iter().any(|x| x.as_u64() == Some(v));
            let rate_ok = rate == 0 || listed("rates", rate.into());
            let depth_ok = matches!(codec, Codec::Mp3 | Codec::Aac) || depth == 0 || listed("depths", depth.into());
            let result = match (rate_ok, depth_ok) {
                (true, true) => Ok(()),
                (false, true) => Err(Unplayable::SampleRate),
                (true, false) => Err(Unplayable::BitDepth),
                (false, false) => Err(Unplayable::SampleRateAndBitDepth),
            };
            Playback { result, inferred: a["inferred"].as_bool().unwrap() }
        }
    };
    let plays = m.plays(codec, rate, depth);
    assert_eq!(plays, expected, "{} {codec:?} {rate} Hz {depth} bits", m.name);
    assert_eq!(codec.lossless(), !matches!(codec, Codec::Mp3 | Codec::Aac));

    if plays.result.is_ok() {
        assert!(m.plays(codec, 0, depth).result.is_ok() && m.plays(codec, rate, 0).result.is_ok());
    }
    let both_wrong = plays.result == Err(Unplayable::SampleRateAndBitDepth);
    let rate_wrong = m.plays(codec, rate, 0).result == Err(Unplayable::SampleRate);
    let depth_wrong = m.plays(codec, 0, depth).result == Err(Unplayable::BitDepth);
    assert_eq!(both_wrong, rate_wrong && depth_wrong);
}

/// Every model answers for every file system as its published row says: FAT32 always, FAT16
/// and HFS+ unless it's a mixer, exFAT as listed (unknown when it takes a firmware update),
/// nothing else. Its kind and databases read the same as the row too.
#[hegel::test(test_cases = 100)]
fn reads_follows_the_published_table(tc: TestCase) {
    let (i, m) = model(&tc);
    let row = &table()[i];
    let mixer = row["kind"] == "mixer";
    assert_eq!(m.is_mixer(), mixer);
    let reads_pdb = row["export_pdb"]["value"].as_bool().unwrap();
    let reads_onelibrary = row["onelibrary"]["value"].as_bool().unwrap();
    assert_eq!(m.onelibrary_only(), reads_onelibrary && !reads_pdb);
    let exfat = &row["exfat"]["value"];
    // Each file system for the drawn model: there are only eight.
    for fs in FS_KINDS {
        let expected = match fs {
            FsKind::Fat32 => Some(true),
            FsKind::Fat16 | FsKind::Hfsplus => Some(!mixer),
            FsKind::Exfat if exfat == "yes" => Some(true),
            FsKind::Exfat if exfat == "no" => Some(false),
            FsKind::Exfat => {
                assert!(matches!(m.exfat.value, Exfat::Firmware(v) if exfat["firmware"] == v), "{}: {exfat}", m.name);
                None
            }
            _ => Some(false),
        };
        assert_eq!(m.reads(fs), expected, "{} {fs:?}", m.name);
    }
    if mixer {
        assert!(m.audio.is_empty() && !reads_pdb && !reads_onelibrary, "{}", m.name);
    }
}

/// A file's format is its extension's, in any case, whatever comes before the last dot; a name
/// without an extension players list says nothing. Each format's own name as an extension
/// names it, but Apple Lossless, which comes in `.m4a`.
#[hegel::test(test_cases = 500)]
fn formats_follow_the_extension_in_any_case(tc: TestCase) {
    let known = [
        ("mp3", FromPath::Codec(Codec::Mp3)),
        ("aac", FromPath::Codec(Codec::Aac)),
        ("m4a", FromPath::AacOrAlac),
        ("mp4", FromPath::AacOrAlac),
        ("wav", FromPath::Codec(Codec::Wav)),
        ("aif", FromPath::Codec(Codec::Aiff)),
        ("aiff", FromPath::Codec(Codec::Aiff)),
        ("flac", FromPath::Codec(Codec::Flac)),
        ("fla", FromPath::Codec(Codec::Flac)),
    ];
    let stem = tc.draw(gs::text().max_size(10));
    let ext: String = tc.draw(hegel::one_of!(
        gs::sampled_from(known.iter().map(|(e, _)| e.to_string()).collect::<Vec<_>>()),
        gs::text().max_size(5).filter(|s| !s.contains('.')),
    ));
    let ext: String = ext.chars().map(|c| if tc.draw(gs::booleans()) { c.to_ascii_uppercase() } else { c }).collect();
    let expected = known.iter().find(|(e, _)| e.eq_ignore_ascii_case(&ext)).map_or(FromPath::Unknown, |(_, f)| *f);
    assert_eq!(Codec::from_path(&format!("{stem}.{ext}")), expected, "{stem:?} . {ext:?}");
    if !stem.contains('.') {
        assert_eq!(Codec::from_path(&stem), FromPath::Unknown);
    }
    let c = codec(&tc);
    let by_name = Codec::from_path(&format!("{stem}.{}", c.name()));
    assert_eq!(by_name, if c == Codec::Alac { FromPath::Unknown } else { FromPath::Codec(c) });
}

/// File systems and formats serialise as their names in lower case, and read back.
#[hegel::test(test_cases = 50)]
fn names_serialise_in_lower_case_and_read_back(tc: TestCase) {
    let fs = tc.draw(gs::sampled_from(FS_KINDS.to_vec()));
    let json = serde_json::to_value(fs).unwrap();
    assert_eq!(json, format!("{fs:?}").to_lowercase());
    assert_eq!(serde_json::from_value::<FsKind>(json).unwrap(), fs);
    let c = codec(&tc);
    let json = serde_json::to_value(c).unwrap();
    assert_eq!(json, c.name().to_lowercase());
    assert_eq!(serde_json::from_value::<Codec>(json).unwrap(), c);
}
