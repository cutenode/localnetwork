//! What each Pioneer DJ / AlphaTheta player, all-in-one system and mixer reads from a USB drive:
//! which database, which file systems, and which audio formats at which sample rates and bit
//! depths.
//!
//! Sources (checked October 2026; each model's entry names its own):
//!
//! * the model's instruction manual: the "USB devices" and "Supported file formats" (older
//!   manuals: "Playable music file formats") sections, read from AlphaTheta's download server
//!   (`downloads.support.alphatheta.com/manuals/...`) unless the entry says otherwise;
//! * AlphaTheta, "Important Notice for Customers Using USB Devices with Our DJ Equipment"
//!   (18 March 2026), for which models read OneLibrary and which read Device Library
//!   (`export.pdb`), and the models' manuals ("This unit only supports OneLibrary");
//! * the rekordbox FAQ "Which DJ equipment supports exFAT" for the firmware a model needs to read
//!   exFAT (the same list as [`crate::format::players::EXFAT`]).
//!
//! What every manual read says alike, and so isn't repeated per model: FAT16, FAT32 and HFS+ are
//! read, NTFS isn't; of several partitions only the first is used (or, most add, the one holding
//! a rekordbox library); folders are browsed 8 levels deep, showing up to 10,000
//! entries per folder, and the number of folders and files is "unlimited". None gives a limit on
//! the number of tracks. The GUID partition table isn't supported (AlphaTheta Help Center), which
//! leaves MBR's 2 TiB as the largest usable drive (see [`crate::format::players`]).
//!
//! MP3 and AAC are listed at 16 bit: that's what the decoder puts out, not something a file
//! has, so only lossless formats' bit depths are checked.
//!
//! A value no manual confirmed is marked `inferred`, so a check built on it says "probably".

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::format::FsKind;
use crate::{Error, Result};

/// An audio format players list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    Mp3,
    Aac,
    Wav,
    Aiff,
    Flac,
    Alac,
}

impl Codec {
    pub const ALL: [Codec; 6] = [Codec::Mp3, Codec::Aac, Codec::Wav, Codec::Aiff, Codec::Flac, Codec::Alac];

    pub fn name(self) -> &'static str {
        match self {
            Codec::Mp3 => "MP3",
            Codec::Aac => "AAC",
            Codec::Wav => "WAV",
            Codec::Aiff => "AIFF",
            Codec::Flac => "FLAC",
            Codec::Alac => "ALAC",
        }
    }

    /// Lossless formats, whose bit depth matters.
    pub fn lossless(self) -> bool {
        !matches!(self, Codec::Mp3 | Codec::Aac)
    }

    /// The format a file's extension names (`.m4a` and `.mp4` hold AAC or Apple Lossless).
    pub fn from_path(path: &str) -> FromPath {
        let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
        match ext.as_deref() {
            Some("mp3") => FromPath::Codec(Codec::Mp3),
            Some("aac") => FromPath::Codec(Codec::Aac),
            Some("m4a" | "mp4") => FromPath::AacOrAlac,
            Some("wav") => FromPath::Codec(Codec::Wav),
            Some("aif" | "aiff") => FromPath::Codec(Codec::Aiff),
            Some("flac" | "fla") => FromPath::Codec(Codec::Flac),
            _ => FromPath::Unknown,
        }
    }
}

/// What a file name says about its format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FromPath {
    Codec(Codec),
    /// An MPEG-4 file: AAC or Apple Lossless.
    AacOrAlac,
    /// No format players list.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A multi player (CDJ, XDJ-1000...).
    Player,
    /// Players and a mixer in one unit (XDJ-XZ, OPUS-QUAD...).
    AllInOne,
    /// A mixer that reads USB drives for settings and firmware only.
    Mixer,
}

/// A value, and whether it's inferred rather than documented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(rename = "Spec_for_{T}")]
pub struct Spec<T> {
    pub value: T,
    pub inferred: bool,
}

const fn known<T>(value: T) -> Spec<T> {
    Spec { value, inferred: false }
}

/// Whether a model reads exFAT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Exfat {
    No,
    Yes,
    /// With this firmware version or later.
    Firmware(&'static str),
}

/// One audio format a model plays, at these sample rates (Hz) and bit depths.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub struct Audio {
    pub codec: Codec,
    pub rates: &'static [u32],
    pub depths: &'static [u16],
    pub inferred: bool,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub struct Model {
    /// As AlphaTheta writes it: `CDJ-2000NXS2`, `OPUS-QUAD`.
    pub name: &'static str,
    pub kind: Kind,
    /// Reads Device Library (`export.pdb`).
    pub export_pdb: Spec<bool>,
    /// Reads OneLibrary (`exportLibrary.db`, called Device Library Plus before October 2025).
    pub onelibrary: Spec<bool>,
    pub exfat: Spec<Exfat>,
    /// The formats it plays. A format missing from the list doesn't play.
    pub audio: &'static [Audio],
    /// The list of formats is itself inferred: a format missing from it probably doesn't play.
    pub audio_inferred: bool,
    /// What else is worth knowing about it.
    pub notes: &'static [&'static str],
    /// Where the values come from.
    pub source: &'static str,
}

/// How a model gets on with a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Unplayable {
    /// It doesn't play the format at all.
    Format,
    SampleRate,
    BitDepth,
    SampleRateAndBitDepth,
}

/// Whether a model plays a file, and whether that rests on an inferred value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Playback {
    pub result: Result<(), Unplayable>,
    pub inferred: bool,
}

impl Model {
    pub fn is_mixer(&self) -> bool {
        self.kind == Kind::Mixer
    }

    /// Reads only OneLibrary.
    pub fn onelibrary_only(&self) -> bool {
        self.onelibrary.value && !self.export_pdb.value
    }

    pub fn audio(&self, codec: Codec) -> Option<&'static Audio> {
        self.audio.iter().find(|a| a.codec == codec)
    }

    /// Whether it plays `codec` at `rate` Hz and `depth` bits. A rate or depth of 0 means
    /// unknown and isn't held against the file; depths only count for lossless formats.
    pub fn plays(&self, codec: Codec, rate: u32, depth: u16) -> Playback {
        let Some(a) = self.audio(codec) else {
            return Playback { result: Err(Unplayable::Format), inferred: self.audio_inferred };
        };
        let rate_ok = rate == 0 || a.rates.contains(&rate);
        let depth_ok = !codec.lossless() || depth == 0 || a.depths.contains(&depth);
        let result = match (rate_ok, depth_ok) {
            (true, true) => Ok(()),
            (false, true) => Err(Unplayable::SampleRate),
            (true, false) => Err(Unplayable::BitDepth),
            (false, false) => Err(Unplayable::SampleRateAndBitDepth),
        };
        Playback { result, inferred: a.inferred }
    }

    /// Whether it reads a file system: `None` when it's an exFAT model that needs a firmware
    /// update (the version is in [`Model::exfat`]).
    pub fn reads(&self, fs: FsKind) -> Option<bool> {
        match fs {
            FsKind::Fat32 => Some(true),
            // Mixers take settings and firmware from FAT32 only.
            FsKind::Fat16 | FsKind::Hfsplus => Some(!self.is_mixer()),
            FsKind::Exfat => match self.exfat.value {
                Exfat::No => Some(false),
                Exfat::Yes => Some(true),
                Exfat::Firmware(_) => None,
            },
            FsKind::Fat12 | FsKind::Ntfs | FsKind::Apfs | FsKind::Other => Some(false),
        }
    }
}

const MPEG_1_AND_2: &[u32] = &[16_000, 22_050, 24_000, 32_000, 44_100, 48_000];
const R32_48: &[u32] = &[32_000, 44_100, 48_000];
const R44_48: &[u32] = &[44_100, 48_000];
const R44_96: &[u32] = &[44_100, 48_000, 88_200, 96_000];
const BITS_16: &[u16] = &[16];
const BITS_16_24: &[u16] = &[16, 24];

const fn lossy(codec: Codec, rates: &'static [u32], inferred: bool) -> Audio {
    Audio { codec, rates, depths: BITS_16, inferred }
}

const fn lossless(codec: Codec, rates: &'static [u32], inferred: bool) -> Audio {
    Audio { codec, rates, depths: BITS_16_24, inferred }
}

/// Every format at 44.1/48 kHz for MP3 (MPEG-1 Layer 3 only) and AAC, and 44.1 to 96 kHz for
/// the lossless ones: the CDJ-3000's generation.
const MODERN_96K: &[Audio] = &[
    lossy(Codec::Mp3, R44_48, false),
    lossy(Codec::Aac, R44_48, false),
    lossless(Codec::Wav, R44_96, false),
    lossless(Codec::Aiff, R44_96, false),
    lossless(Codec::Alac, R44_96, false),
    lossless(Codec::Flac, R44_96, false),
];

/// The same, with the lossless formats at 44.1/48 kHz only.
const MODERN_48K: &[Audio] = &[
    lossy(Codec::Mp3, R44_48, false),
    lossy(Codec::Aac, R44_48, false),
    lossless(Codec::Wav, R44_48, false),
    lossless(Codec::Aiff, R44_48, false),
    lossless(Codec::Alac, R44_48, false),
    lossless(Codec::Flac, R44_48, false),
];

/// MP3 (MPEG-1 and MPEG-2 Layer 3) and AAC from 16 to 48 kHz, every lossless format at 44.1 to
/// 96 kHz: the CDJ-2000NXS2.
const NXS2: &[Audio] = &[
    lossy(Codec::Mp3, MPEG_1_AND_2, false),
    lossy(Codec::Aac, MPEG_1_AND_2, false),
    lossless(Codec::Wav, R44_96, false),
    lossless(Codec::Aiff, R44_96, false),
    lossless(Codec::Alac, R44_96, false),
    lossless(Codec::Flac, R44_96, false),
];

/// MP3 and AAC from 16 to 48 kHz, WAV and AIFF at 44.1/48 kHz: the CDJ-2000NXS's generation.
const NXS: &[Audio] = &[
    lossy(Codec::Mp3, MPEG_1_AND_2, false),
    lossy(Codec::Aac, MPEG_1_AND_2, false),
    lossless(Codec::Wav, R44_48, false),
    lossless(Codec::Aiff, R44_48, false),
];

/// The same, not checked against the model's own manual.
const NXS_INFERRED: &[Audio] = &[
    lossy(Codec::Mp3, MPEG_1_AND_2, true),
    lossy(Codec::Aac, MPEG_1_AND_2, true),
    lossless(Codec::Wav, R44_48, true),
    lossless(Codec::Aiff, R44_48, true),
];

const PDB: Spec<bool> = known(true);
const NO_PDB: Spec<bool> = known(false);
const ONELIBRARY: Spec<bool> = known(true);
const NO_ONELIBRARY: Spec<bool> = known(false);

/// Every model locallibrary knows, newest first within each kind.
pub const MODELS: &[Model] = &[
    Model {
        name: "CDJ-3000X",
        kind: Kind::Player,
        export_pdb: NO_PDB,
        onelibrary: ONELIBRARY,
        exfat: known(Exfat::Yes),
        audio: MODERN_96K,
        audio_inferred: false,
        notes: &["exFAT drives can also update its firmware"],
        source: "CDJ-3000X Instruction Manual DRI1956B pp. 9, 11-12 \
                 (downloads.support.alphatheta.com/manuals/dj-players/CDJ-3000X/CDJ-3000X_DRI1956B_manual.pdf)",
    },
    Model {
        name: "CDJ-1500X",
        kind: Kind::Player,
        export_pdb: NO_PDB,
        onelibrary: ONELIBRARY,
        exfat: known(Exfat::Yes),
        audio: MODERN_48K,
        audio_inferred: false,
        notes: &[],
        source: "CDJ-1500X Instruction Manual DRI2044A pp. 10, 12-13 \
                 (downloads.support.alphatheta.com/manuals/dj-players/CDJ-1500X/CDJ-1500X_DRI2044A_manual.pdf)",
    },
    Model {
        name: "CDJ-3000",
        kind: Kind::Player,
        export_pdb: PDB,
        onelibrary: NO_ONELIBRARY,
        exfat: known(Exfat::Firmware("1.20")),
        audio: MODERN_96K,
        audio_inferred: false,
        notes: &[
            "firmware 3.30 read OneLibrary, in preference to export.pdb when a drive has both, but was withdrawn in \
             November 2025; 3.22 (January 2026) reads export.pdb only",
        ],
        source: "CDJ-3000 Instruction Manual DRI1586A pp. 11, 13 \
                 (downloads.support.alphatheta.com/manuals/dj-players/CDJ-3000/CDJ-3000_DRI1586A_manual.pdf); \
                 AlphaTheta's USB notice (18 March 2026); Pioneer DJ news, firmware 3.22 (15 January 2026)",
    },
    Model {
        name: "CDJ-2000NXS2",
        kind: Kind::Player,
        export_pdb: PDB,
        onelibrary: NO_ONELIBRARY,
        exfat: known(Exfat::No),
        audio: NXS2,
        audio_inferred: false,
        notes: &[],
        source: "CDJ-2000NXS2 Operating Instructions DRI1290A pp. 6-7 \
                 (downloads.support.alphatheta.com/manuals/dj-players/CDJ-2000NXS2/CDJ-2000NXS2_DRI1290A_manual.pdf)",
    },
    Model {
        name: "CDJ-2000NXS",
        kind: Kind::Player,
        export_pdb: PDB,
        onelibrary: NO_ONELIBRARY,
        exfat: known(Exfat::No),
        audio: NXS,
        audio_inferred: false,
        notes: &[],
        source: "CDJ-2000NXS Operating Instructions DRI1052A pp. 6-7 \
                 (downloads.support.alphatheta.com/manuals/dj-players/CDJ-2000NXS/CDJ-2000NXS_DRI1052_manual.pdf)",
    },
    Model {
        name: "CDJ-900NXS",
        kind: Kind::Player,
        export_pdb: PDB,
        onelibrary: NO_ONELIBRARY,
        exfat: known(Exfat::No),
        audio: NXS,
        audio_inferred: false,
        notes: &[],
        source: "CDJ-900NXS Operating Instructions DRI1168A (pioneerdj.com/support)",
    },
    Model {
        name: "CDJ-2000",
        kind: Kind::Player,
        export_pdb: PDB,
        onelibrary: NO_ONELIBRARY,
        exfat: known(Exfat::No),
        audio: NXS_INFERRED,
        audio_inferred: true,
        notes: &[],
        source: "inferred from the CDJ-900 of the same generation (its Operating Instructions DRB1486A p. 8); \
                 the CDJ-2000's own manual (DRB1475) wasn't read",
    },
    Model {
        name: "CDJ-900",
        kind: Kind::Player,
        export_pdb: PDB,
        onelibrary: NO_ONELIBRARY,
        exfat: known(Exfat::No),
        audio: NXS,
        audio_inferred: false,
        notes: &[],
        source: "CDJ-900 Operating Instructions DRB1486A pp. 7-8",
    },
    Model {
        name: "XDJ-1000MK2",
        kind: Kind::Player,
        export_pdb: PDB,
        onelibrary: NO_ONELIBRARY,
        exfat: known(Exfat::No),
        audio: &[
            lossy(Codec::Mp3, MPEG_1_AND_2, false),
            lossy(Codec::Aac, MPEG_1_AND_2, false),
            lossless(Codec::Wav, R44_48, false),
            lossless(Codec::Aiff, R44_48, false),
            lossless(Codec::Alac, R44_48, false),
            lossless(Codec::Flac, R44_48, false),
        ],
        audio_inferred: false,
        notes: &[],
        source: "XDJ-1000MK2 Operating Instructions DRI1396B p. 6 \
                 (downloads.support.alphatheta.com/manuals/dj-players/XDJ-1000MK2/XDJ-1000MK2_DRI1396B_manual.pdf)",
    },
    Model {
        name: "XDJ-700",
        kind: Kind::Player,
        export_pdb: PDB,
        onelibrary: NO_ONELIBRARY,
        exfat: known(Exfat::No),
        audio: NXS,
        audio_inferred: false,
        notes: &[],
        source: "XDJ-700 Operating Instructions DRI1271B p. 5 \
                 (downloads.support.alphatheta.com/manuals/dj-players/XDJ-700/XDJ-700_DRI1271B_manual.pdf)",
    },
    Model {
        name: "OPUS-QUAD",
        kind: Kind::AllInOne,
        export_pdb: NO_PDB,
        onelibrary: ONELIBRARY,
        exfat: known(Exfat::Yes),
        audio: MODERN_96K,
        audio_inferred: false,
        notes: &[],
        source: "OPUS-QUAD Instruction Manual DRI1795D pp. 9-10, 12 \
                 (assets.pioneerdjhub.com/OPUS_QUAD_DRI1795D_manual_EN.pdf); AlphaTheta's USB notice (18 March 2026)",
    },
    Model {
        name: "OMNIS-DUO",
        kind: Kind::AllInOne,
        export_pdb: NO_PDB,
        onelibrary: ONELIBRARY,
        exfat: known(Exfat::Yes),
        audio: MODERN_48K,
        audio_inferred: false,
        notes: &[],
        source: "OMNIS-DUO Instruction Manual DRI1882A pp. 13-14, 17 \
                 (downloads.support.alphatheta.com/manuals/OMNIS_DUO_DRI1882A_manual.pdf)",
    },
    Model {
        name: "XDJ-AZ",
        kind: Kind::AllInOne,
        export_pdb: NO_PDB,
        onelibrary: ONELIBRARY,
        exfat: known(Exfat::Yes),
        audio: MODERN_96K,
        audio_inferred: false,
        notes: &[],
        source: "XDJ-AZ Instruction Manual DRI1936C pp. 10-11, 13 \
                 (downloads.support.alphatheta.com/manuals/all-in-one-dj-systems/XDJ-AZ/XDJ-AZ_DRI1936C_manual_EN.pdf)",
    },
    Model {
        name: "XDJ-AN",
        kind: Kind::AllInOne,
        export_pdb: NO_PDB,
        onelibrary: ONELIBRARY,
        exfat: known(Exfat::Yes),
        audio: MODERN_48K,
        audio_inferred: false,
        notes: &[],
        source: "XDJ-AN Instruction Manual DRI2023A pp. 8-9, 11 (AlphaTheta Help Center, July 2026)",
    },
    Model {
        name: "XDJ-XZ",
        kind: Kind::AllInOne,
        export_pdb: PDB,
        onelibrary: NO_ONELIBRARY,
        exfat: known(Exfat::Firmware("1.23")),
        audio: &[
            lossy(Codec::Mp3, R32_48, false),
            lossy(Codec::Aac, R32_48, false),
            lossless(Codec::Wav, R44_48, false),
            lossless(Codec::Aiff, R44_48, false),
            lossless(Codec::Flac, R44_48, false),
        ],
        audio_inferred: false,
        notes: &["the 2019 manual lists no Apple Lossless (ALAC)"],
        source: "XDJ-XZ Operating Instructions DRI1625B pp. 6-8 \
                 (downloads.support.alphatheta.com/manuals/all-in-one-dj-systems/XDJ-XZ/XDJ-XZ_DRI1625B_manual.pdf)",
    },
    Model {
        name: "XDJ-RX3",
        kind: Kind::AllInOne,
        export_pdb: PDB,
        onelibrary: NO_ONELIBRARY,
        exfat: known(Exfat::Firmware("1.11")),
        audio: &[
            lossy(Codec::Mp3, R32_48, false),
            // The table's AAC rates share a cell with MP3's; taken to match, as on the XDJ-XZ.
            lossy(Codec::Aac, R32_48, true),
            lossless(Codec::Wav, R44_48, false),
            lossless(Codec::Aiff, R44_48, false),
            lossless(Codec::Flac, R44_48, false),
        ],
        audio_inferred: false,
        notes: &["the manual lists no Apple Lossless (ALAC)"],
        source: "XDJ-RX3 Instruction Manual DRI1702C pp. 9-10 \
                 (downloads.support.alphatheta.com/manuals/all-in-one-dj-systems/XDJ-RX3/XDJ-RX3_DRI1702C_manual.pdf)",
    },
    Model {
        name: "XDJ-RX2",
        kind: Kind::AllInOne,
        export_pdb: PDB,
        onelibrary: NO_ONELIBRARY,
        exfat: known(Exfat::No),
        audio: NXS_INFERRED,
        audio_inferred: true,
        notes: &[],
        source: "inferred: manua.ls's summary of its manual lists MP3, AAC, WAV and AIFF; rates as on the XDJ-700",
    },
    Model {
        name: "XDJ-RR",
        kind: Kind::AllInOne,
        export_pdb: PDB,
        onelibrary: NO_ONELIBRARY,
        exfat: known(Exfat::No),
        audio: NXS_INFERRED,
        audio_inferred: true,
        notes: &[],
        source: "inferred: AlphaTheta's USB notice lists it as a Device Library unit; formats as on the XDJ-RX2",
    },
    Model {
        name: "DJM-V10",
        kind: Kind::Mixer,
        export_pdb: NO_PDB,
        onelibrary: NO_ONELIBRARY,
        exfat: known(Exfat::No),
        audio: &[],
        audio_inferred: false,
        notes: &["reads My Settings and firmware updates from a FAT32 drive on MBR"],
        source: "AlphaTheta Help Center (see format::players)",
    },
    Model {
        name: "DJM-A9",
        kind: Kind::Mixer,
        export_pdb: NO_PDB,
        onelibrary: NO_ONELIBRARY,
        exfat: known(Exfat::No),
        audio: &[],
        audio_inferred: false,
        notes: &["reads My Settings and firmware updates from a FAT32 drive on MBR (one update file at a time)"],
        source: "AlphaTheta Help Center (see format::players)",
    },
];

/// A model name reduced to what tells models apart: `cdj-2000 nexus2` and `CDJ2000NXS2` are both
/// `CDJ2000NXS2`.
fn normalise(name: &str) -> String {
    let s: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect();
    s.replace("NEXUS", "NXS")
}

/// The model `name` names, however it's spelled (`cdj-2000nxs2`, `CDJ 2000 NEXUS2`).
pub fn find(name: &str) -> Option<&'static Model> {
    let n = normalise(name);
    MODELS.iter().find(|m| normalise(m.name) == n)
}

/// `names` as the table spells them. Names it doesn't know are an error that lists the ones it does.
pub fn canonical(names: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    let mut unknown = Vec::new();
    for n in names.iter().map(|n| n.trim()).filter(|n| !n.is_empty()) {
        match find(n) {
            Some(m) if !out.iter().any(|o| o == m.name) => out.push(m.name.to_string()),
            Some(_) => {}
            None => unknown.push(n.to_string()),
        }
    }
    if !unknown.is_empty() {
        let known: Vec<&str> = MODELS.iter().map(|m| m.name).collect();
        return Err(Error::invalid(format!(
            "not a model locallibrary knows: {} (it knows {})",
            unknown.join(", "),
            known.join(", ")
        )));
    }
    Ok(out)
}

/// The models a DJ plays on, kept in the store (`players.json`) so `check` uses them when no
/// `--players` are given: a list per library, and a default for drives of any other library.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Remembered {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub libraries: BTreeMap<Uuid, Vec<String>>,
}

impl Remembered {
    /// The models for a drive of `library`, and whether they're that library's own (rather than
    /// the default).
    pub fn for_library(&self, library: Option<Uuid>) -> (&[String], bool) {
        match library.and_then(|id| self.libraries.get(&id)) {
            Some(models) => (models, true),
            None => (&self.default, false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::players::{EXFAT, NO_EXFAT};

    #[test]
    fn names_are_found_however_they_are_spelled() {
        assert_eq!(find("cdj-2000nxs2").unwrap().name, "CDJ-2000NXS2");
        assert_eq!(find("CDJ 2000 NEXUS2").unwrap().name, "CDJ-2000NXS2");
        assert_eq!(find("opus quad").unwrap().name, "OPUS-QUAD");
        assert_eq!(find("CDJ-2000").unwrap().name, "CDJ-2000");
        assert!(find("CDJ-2000NXS3").is_none());
        let mut seen = std::collections::HashSet::new();
        assert!(MODELS.iter().all(|m| seen.insert(normalise(m.name))), "no two models normalise alike");
    }

    /// The table agrees with the exFAT lists `disks` and `format` show.
    #[test]
    fn agrees_with_the_exfat_lists() {
        let parse = |entry: &str| -> Vec<(String, Option<String>)> {
            let (names, note) = match entry.split_once(" (") {
                Some((n, rest)) => {
                    (n, rest.trim_end_matches(')').strip_prefix("firmware ").map(|v| v.trim_end_matches('+')))
                }
                None => (entry, None),
            };
            names.split('/').map(|n| (n.to_string(), note.map(str::to_string))).collect()
        };
        for (name, firmware) in EXFAT.iter().flat_map(|e| parse(e)) {
            let m = find(&name).unwrap_or_else(|| panic!("{name} isn't in the table"));
            match (firmware, m.exfat.value) {
                (Some(v), Exfat::Firmware(f)) => assert_eq!(f, v, "{name}"),
                (None, Exfat::Yes) => {}
                (listed, table) => panic!("{name}: the list says {listed:?}, the table {table:?}"),
            }
        }
        for (name, _) in NO_EXFAT.iter().flat_map(|e| parse(e)) {
            let m = find(&name).unwrap_or_else(|| panic!("{name} isn't in the table"));
            assert_eq!(m.exfat.value, Exfat::No, "{name}");
        }
        let listed = EXFAT.len() + NO_EXFAT.iter().map(|e| parse(e).len()).sum::<usize>();
        assert_eq!(listed, MODELS.len(), "every model is on one of the lists");
    }

    #[test]
    fn every_player_reads_a_database_and_plays_mp3() {
        for m in MODELS.iter().filter(|m| !m.is_mixer()) {
            assert!(m.export_pdb.value || m.onelibrary.value, "{}", m.name);
            assert!(m.audio(Codec::Mp3).is_some(), "{}", m.name);
            let mut codecs: Vec<Codec> = m.audio.iter().map(|a| a.codec).collect();
            codecs.dedup();
            assert_eq!(codecs.len(), m.audio.len(), "{} lists a format twice", m.name);
        }
    }

    #[test]
    fn what_plays_where() {
        let nxs2 = find("CDJ-2000NXS2").unwrap();
        assert_eq!(nxs2.plays(Codec::Flac, 96_000, 24).result, Ok(()));
        assert_eq!(nxs2.plays(Codec::Flac, 192_000, 24).result, Err(Unplayable::SampleRate));
        assert_eq!(nxs2.plays(Codec::Wav, 44_100, 32).result, Err(Unplayable::BitDepth));
        assert_eq!(nxs2.plays(Codec::Wav, 192_000, 32).result, Err(Unplayable::SampleRateAndBitDepth));
        // MP3s at 22.05 kHz are MPEG-2 Layer 3, which the NXS2 lists and the CDJ-3000 doesn't.
        assert_eq!(nxs2.plays(Codec::Mp3, 22_050, 16).result, Ok(()));
        let cdj3000 = find("CDJ-3000").unwrap();
        assert_eq!(cdj3000.plays(Codec::Mp3, 22_050, 16).result, Err(Unplayable::SampleRate));
        // Lossy formats' depths aren't held against them, and unknown values aren't either.
        assert_eq!(cdj3000.plays(Codec::Mp3, 44_100, 32).result, Ok(()));
        assert_eq!(cdj3000.plays(Codec::Flac, 0, 0).result, Ok(()));
        let nxs = find("CDJ-2000NXS").unwrap();
        assert_eq!(nxs.plays(Codec::Flac, 44_100, 16), Playback { result: Err(Unplayable::Format), inferred: false });
        assert_eq!(nxs.plays(Codec::Wav, 96_000, 24).result, Err(Unplayable::SampleRate));
        let omnis = find("OMNIS-DUO").unwrap();
        assert_eq!(omnis.plays(Codec::Flac, 96_000, 24).result, Err(Unplayable::SampleRate));
        let rx2 = find("XDJ-RX2").unwrap();
        assert_eq!(rx2.plays(Codec::Flac, 44_100, 16), Playback { result: Err(Unplayable::Format), inferred: true });
        assert!(find("XDJ-RX3").unwrap().plays(Codec::Aac, 32_000, 16).inferred);
    }

    #[test]
    fn file_systems() {
        let reads = |name: &str, fs| find(name).unwrap().reads(fs);
        assert_eq!(reads("CDJ-2000NXS2", FsKind::Exfat), Some(false));
        assert_eq!(reads("CDJ-3000", FsKind::Exfat), None);
        assert_eq!(reads("CDJ-3000X", FsKind::Exfat), Some(true));
        assert_eq!(reads("CDJ-3000X", FsKind::Ntfs), Some(false));
        assert_eq!(reads("DJM-V10", FsKind::Hfsplus), Some(false));
        assert_eq!(reads("XDJ-700", FsKind::Hfsplus), Some(true));
        assert_eq!(reads("DJM-V10", FsKind::Fat16), Some(false), "settings from FAT32 only");
        assert_eq!(reads("DJM-A9", FsKind::Fat32), Some(true));
        assert_eq!(reads("CDJ-2000NXS2", FsKind::Fat16), Some(true));
    }

    #[test]
    fn remembered_models() {
        let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            canonical(&names(&["cdj-3000x", "CDJ 2000 NEXUS2", "CDJ-3000X"])).unwrap(),
            ["CDJ-3000X", "CDJ-2000NXS2"]
        );
        let e = canonical(&names(&["CDJ-3000", "XYZ-9"])).unwrap_err().to_string();
        assert!(e.contains("XYZ-9") && e.contains("OPUS-QUAD"), "{e}");
        let lib = Uuid::new_v4();
        let r = Remembered { default: names(&["CDJ-3000"]), libraries: [(lib, names(&["OPUS-QUAD"]))].into() };
        assert_eq!(r.for_library(Some(lib)), (&names(&["OPUS-QUAD"])[..], true));
        assert_eq!(r.for_library(Some(Uuid::new_v4())), (&names(&["CDJ-3000"])[..], false));
        assert_eq!(r.for_library(None), (&names(&["CDJ-3000"])[..], false));
    }

    #[test]
    fn formats_from_file_names() {
        assert_eq!(Codec::from_path("/Contents/a/b.MP3"), FromPath::Codec(Codec::Mp3));
        assert_eq!(Codec::from_path("/Contents/a/b.m4a"), FromPath::AacOrAlac);
        assert_eq!(Codec::from_path("/Contents/a/b.fla"), FromPath::Codec(Codec::Flac));
        assert_eq!(Codec::from_path("/Contents/a/b.ogg"), FromPath::Unknown);
        assert_eq!(Codec::from_path("/Contents/a/b"), FromPath::Unknown);
    }
}
