//! rekordbox "My Settings" on drives: `MYSETTING.DAT`, `MYSETTING2.DAT`, `DJMMYSETTING.DAT` and
//! `DEVSETTING.DAT` in `PIONEER/`.
//!
//! Players load these when the drive is inserted (quantize, auto cue, jog mode, language, mixer
//! curves, waveform colours...). rekordbox writes them on export; locallibrary lets you define
//! them once in a [`SettingsProfile`] and apply it to every drive, with optional per-drive
//! overrides.
//!
//! Files are edited by patching single bytes in the existing file and recomputing the CRC, rather
//! than re-serialising a parsed struct. Bytes we don't understand (and fields newer firmware may
//! add) are therefore preserved exactly. Layout and values follow the reverse-engineering work in
//! [rekordcrate](https://github.com/Holzhaus/rekordcrate) and are verified against its fixtures.
//!
//! File layout: `u32 0x60`, three 32-byte NUL-padded strings (brand, software, version),
//! `u32 len_data`, `len_data` bytes of settings, `u16` CRC-16/XMODEM, `u16 0`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::IoContext;
use crate::error::{Action, Reason};
use crate::fsutil;
use crate::paths;
use crate::{Error, Result};

const HEADER_LEN: usize = 104;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SettingsFile {
    MySetting,
    MySetting2,
    DjmMySetting,
    DevSetting,
}

impl SettingsFile {
    pub const ALL: [SettingsFile; 4] =
        [SettingsFile::MySetting, SettingsFile::MySetting2, SettingsFile::DjmMySetting, SettingsFile::DevSetting];

    pub fn file_name(self) -> &'static str {
        match self {
            SettingsFile::MySetting => "MYSETTING.DAT",
            SettingsFile::MySetting2 => "MYSETTING2.DAT",
            SettingsFile::DjmMySetting => "DJMMYSETTING.DAT",
            SettingsFile::DevSetting => "DEVSETTING.DAT",
        }
    }

    /// Drive-relative key, e.g. `PIONEER/MYSETTING.DAT`.
    pub fn key(self) -> &'static str {
        match self {
            SettingsFile::MySetting => "PIONEER/MYSETTING.DAT",
            SettingsFile::MySetting2 => "PIONEER/MYSETTING2.DAT",
            SettingsFile::DjmMySetting => "PIONEER/DJMMYSETTING.DAT",
            SettingsFile::DevSetting => "PIONEER/DEVSETTING.DAT",
        }
    }

    fn brand(self) -> &'static str {
        match self {
            SettingsFile::MySetting | SettingsFile::MySetting2 => "PIONEER",
            SettingsFile::DjmMySetting => "PioneerDJ",
            SettingsFile::DevSetting => "PIONEER DJ",
        }
    }

    fn data_len(self) -> usize {
        match self {
            SettingsFile::MySetting | SettingsFile::MySetting2 => 40,
            SettingsFile::DjmMySetting => 52,
            SettingsFile::DevSetting => 32,
        }
    }

    /// Default data section: fixed bytes of unknown meaning (as rekordbox writes them) with
    /// every known setting at its default value.
    fn default_data(self) -> Vec<u8> {
        let mut d = vec![0u8; self.data_len()];
        match self {
            SettingsFile::MySetting => {
                d[..8].copy_from_slice(&[0x78, 0x56, 0x34, 0x12, 0x02, 0x00, 0x00, 0x00]);
                d[13] = 0x01;
                d[17..20].copy_from_slice(&[0x01, 0x01, 0x01]);
            }
            SettingsFile::MySetting2 => d[11] = 0x81,
            SettingsFile::DjmMySetting => {
                d[..12].copy_from_slice(&[0x78, 0x56, 0x34, 0x12, 0x01, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00]);
            }
            SettingsFile::DevSetting => {
                d[..9].copy_from_slice(&[0x78, 0x56, 0x34, 0x12, 0x01, 0x00, 0x00, 0x00, 0x01]);
                d[11] = 0x01;
            }
        }
        for def in SCHEMA.iter().filter(|s| s.file == self) {
            d[def.offset] = def.default_choice().byte;
        }
        d
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(rename = "SettingCategory")]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Player,
    Display,
    Mixer,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub struct Choice {
    pub id: &'static str,
    pub label: &'static str,
    #[serde(skip)]
    pub byte: u8,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SettingDef {
    pub key: &'static str,
    pub label: &'static str,
    pub category: Category,
    pub file: SettingsFile,
    #[serde(skip)]
    pub offset: usize,
    pub choices: &'static [Choice],
    pub default: &'static str,
}

impl SettingDef {
    pub fn choice(&self, id: &str) -> Option<&'static Choice> {
        let want = normalize_id(id);
        self.choices
            .iter()
            .find(|c| normalize_id(c.id) == want)
            .or_else(|| self.choices.iter().find(|c| normalize_id(c.label) == want))
    }

    fn by_byte(&self, b: u8) -> Option<&'static Choice> {
        self.choices.iter().find(|c| c.byte == b)
    }

    fn default_choice(&self) -> &'static Choice {
        self.choice(self.default).expect("schema default is a valid choice")
    }
}

fn normalize_id(s: &str) -> String {
    // Full Unicode lowercasing: labels such as "Français" or "Ελληνικά" have non-ASCII capitals.
    s.trim().to_lowercase().replace(['_', ' '], "-")
}

macro_rules! choices {
    ($($id:literal $label:literal = $byte:literal),+ $(,)?) => {
        &[$(Choice { id: $id, label: $label, byte: $byte }),+]
    };
}

const OFF_ON: &[Choice] = choices!["off" "Off" = 0x80, "on" "On" = 0x81];
const BRIGHTNESS_1_5: &[Choice] =
    choices!["1" "1" = 0x81, "2" "2" = 0x82, "3" "3" = 0x83, "4" "4" = 0x84, "5" "5" = 0x85];
const OFF_DARK_BRIGHT: &[Choice] = choices!["off" "Off" = 0x80, "dark" "1 (Dark)" = 0x81, "bright" "2 (Bright)" = 0x82];
const UNLOCK_LOCK: &[Choice] = choices!["unlock" "Unlock" = 0x80, "lock" "Lock" = 0x81];

use Category::{Display, Mixer, Player};
use SettingsFile::{DevSetting, DjmMySetting, MySetting, MySetting2};

macro_rules! setting {
    ($key:literal, $label:literal, $cat:expr, $file:expr, $off:literal, $choices:expr, $default:literal) => {
        SettingDef {
            key: $key,
            label: $label,
            category: $cat,
            file: $file,
            offset: $off,
            choices: $choices,
            default: $default,
        }
    };
}

/// Every known setting. Offsets are relative to the start of the data section.
pub static SCHEMA: &[SettingDef] = &[
    // MYSETTING.DAT: player
    setting!("on_air_display", "On Air Display", Player, MySetting, 8, OFF_ON, "on"),
    setting!("lcd_brightness", "LCD Brightness", Display, MySetting, 9, BRIGHTNESS_1_5, "3"),
    setting!("quantize", "Quantize", Player, MySetting, 10, OFF_ON, "on"),
    setting!(
        "auto_cue_level",
        "Auto Cue Level",
        Player,
        MySetting,
        11,
        choices![
            "-36db" "-36dB" = 0x80, "-42db" "-42dB" = 0x81, "-48db" "-48dB" = 0x82, "-54db" "-54dB" = 0x83,
            "-60db" "-60dB" = 0x84, "-66db" "-66dB" = 0x85, "-72db" "-72dB" = 0x86, "-78db" "-78dB" = 0x87,
            "memory" "Memory" = 0x88,
        ],
        "memory"
    ),
    setting!(
        "language",
        "Language",
        Display,
        MySetting,
        12,
        choices![
            "english" "English" = 0x81, "french" "Français" = 0x82, "german" "Deutsch" = 0x83,
            "italian" "Italiano" = 0x84, "dutch" "Nederlands" = 0x85, "spanish" "Español" = 0x86,
            "russian" "Русский" = 0x87, "korean" "한국어" = 0x88, "chinese-simplified" "简体中文" = 0x89,
            "chinese-traditional" "繁體中文" = 0x8a, "japanese" "日本語" = 0x8b, "portuguese" "Português" = 0x8c,
            "swedish" "Svenska" = 0x8d, "czech" "Čeština" = 0x8e, "hungarian" "Magyar" = 0x8f,
            "danish" "Dansk" = 0x90, "greek" "Ελληνικά" = 0x91, "turkish" "Türkçe" = 0x92,
        ],
        "english"
    ),
    setting!("jog_ring_brightness", "Jog Ring Brightness", Display, MySetting, 14, OFF_DARK_BRIGHT, "bright"),
    setting!("jog_ring_indicator", "Jog Ring Indicator", Display, MySetting, 15, OFF_ON, "on"),
    setting!("slip_flashing", "Slip Flashing", Display, MySetting, 16, OFF_ON, "on"),
    setting!("disc_slot_illumination", "Disc Slot Illumination", Display, MySetting, 20, OFF_DARK_BRIGHT, "bright"),
    setting!("eject_lock", "Eject/Load Lock", Player, MySetting, 21, UNLOCK_LOCK, "unlock"),
    setting!("sync", "Sync", Player, MySetting, 22, OFF_ON, "off"),
    setting!(
        "play_mode",
        "Play Mode / Auto Play Mode",
        Player,
        MySetting,
        23,
        choices!["continue" "Continue / On" = 0x80, "single" "Single / Off" = 0x81],
        "single"
    ),
    setting!(
        "quantize_beat_value",
        "Quantize Beat Value",
        Player,
        MySetting,
        24,
        choices![
            "1" "1 Beat" = 0x80, "1/2" "1/2 Beat" = 0x81, "1/4" "1/4 Beat" = 0x82, "1/8" "1/8 Beat" = 0x83,
        ],
        "1"
    ),
    setting!(
        "hot_cue_auto_load",
        "Hot Cue Auto Load",
        Player,
        MySetting,
        25,
        choices![
            "off" "Off" = 0x80, "on" "On" = 0x81, "rekordbox" "rekordbox SETTING" = 0x82,
        ],
        "on"
    ),
    setting!("hot_cue_color", "Hot Cue Color", Display, MySetting, 26, OFF_ON, "off"),
    setting!("needle_lock", "Needle Lock", Player, MySetting, 29, UNLOCK_LOCK, "lock"),
    setting!(
        "time_mode",
        "Time Mode",
        Display,
        MySetting,
        32,
        choices!["elapsed" "Elapsed" = 0x80, "remain" "Remain" = 0x81],
        "remain"
    ),
    setting!(
        "jog_mode",
        "Jog Mode",
        Player,
        MySetting,
        33,
        choices!["cdj" "CDJ" = 0x80, "vinyl" "Vinyl" = 0x81],
        "vinyl"
    ),
    setting!("auto_cue", "Auto Cue", Player, MySetting, 34, OFF_ON, "on"),
    setting!("master_tempo", "Master Tempo", Player, MySetting, 35, OFF_ON, "off"),
    setting!(
        "tempo_range",
        "Tempo Range",
        Player,
        MySetting,
        36,
        choices![
            "6" "±6%" = 0x80, "10" "±10%" = 0x81, "16" "±16%" = 0x82, "wide" "Wide" = 0x83,
        ],
        "10"
    ),
    setting!(
        "phase_meter",
        "Phase Meter",
        Display,
        MySetting,
        37,
        choices!["type1" "Type 1" = 0x80, "type2" "Type 2" = 0x81],
        "type1"
    ),
    // MYSETTING2.DAT: newer player features
    setting!(
        "vinyl_speed_adjust",
        "Vinyl Speed Adjust",
        Player,
        MySetting2,
        0,
        choices![
            "touch-release" "Touch & Release" = 0x80, "touch" "Touch" = 0x81, "release" "Release" = 0x82,
        ],
        "touch"
    ),
    setting!(
        "jog_display_mode",
        "Jog Display Mode",
        Display,
        MySetting2,
        1,
        choices![
            "auto" "Auto" = 0x80, "info" "Info" = 0x81, "simple" "Simple" = 0x82, "artwork" "Artwork" = 0x83,
        ],
        "auto"
    ),
    setting!(
        "pad_button_brightness",
        "Pad/Button Brightness",
        Display,
        MySetting2,
        2,
        choices!["1" "1" = 0x81, "2" "2" = 0x82, "3" "3" = 0x83, "4" "4" = 0x84],
        "3"
    ),
    setting!("jog_lcd_brightness", "Jog LCD Brightness", Display, MySetting2, 3, BRIGHTNESS_1_5, "3"),
    setting!(
        "waveform_divisions",
        "Waveform Divisions",
        Display,
        MySetting2,
        4,
        choices!["time-scale" "Time Scale" = 0x80, "phrase" "Phrase" = 0x81],
        "phrase"
    ),
    setting!(
        "waveform_display",
        "Waveform / Phase Meter",
        Display,
        MySetting2,
        10,
        choices!["waveform" "Waveform" = 0x80, "phase-meter" "Phase Meter" = 0x81],
        "waveform"
    ),
    setting!(
        "beat_jump_beat_value",
        "Beat Jump Beat Value",
        Player,
        MySetting2,
        12,
        choices![
            "1/2" "1/2 Beat" = 0x80, "1" "1 Beat" = 0x81, "2" "2 Beat" = 0x82, "4" "4 Beat" = 0x83,
            "8" "8 Beat" = 0x84, "16" "16 Beat" = 0x85, "32" "32 Beat" = 0x86, "64" "64 Beat" = 0x87,
        ],
        "16"
    ),
    // DJMMYSETTING.DAT: mixer
    setting!(
        "channel_fader_curve",
        "Channel Fader Curve",
        Mixer,
        DjmMySetting,
        12,
        choices![
            "steep-top" "Steep Top" = 0x80, "linear" "Linear" = 0x81, "steep-bottom" "Steep Bottom" = 0x82,
        ],
        "linear"
    ),
    setting!(
        "crossfader_curve",
        "Crossfader Curve",
        Mixer,
        DjmMySetting,
        13,
        choices![
            "constant-power" "Constant Power" = 0x80, "slow-cut" "Slow Cut" = 0x81, "fast-cut" "Fast Cut" = 0x82,
        ],
        "fast-cut"
    ),
    setting!(
        "headphones_pre_eq",
        "Headphones Pre EQ",
        Mixer,
        DjmMySetting,
        14,
        choices!["post-eq" "Post EQ" = 0x80, "pre-eq" "Pre EQ" = 0x81],
        "post-eq"
    ),
    setting!(
        "headphones_mono_split",
        "Headphones Mono Split",
        Mixer,
        DjmMySetting,
        15,
        choices!["stereo" "Stereo" = 0x80, "mono-split" "Mono Split" = 0x81],
        "stereo"
    ),
    setting!("beat_fx_quantize", "Beat FX Quantize", Mixer, DjmMySetting, 16, OFF_ON, "on"),
    setting!("mic_low_cut", "Mic Low Cut", Mixer, DjmMySetting, 17, OFF_ON, "on"),
    setting!(
        "talk_over_mode",
        "Talk Over Mode",
        Mixer,
        DjmMySetting,
        18,
        choices!["advanced" "Advanced" = 0x80, "normal" "Normal" = 0x81],
        "advanced"
    ),
    setting!(
        "talk_over_level",
        "Talk Over Level",
        Mixer,
        DjmMySetting,
        19,
        choices![
            "-24db" "-24dB" = 0x80, "-18db" "-18dB" = 0x81, "-12db" "-12dB" = 0x82, "-6db" "-6dB" = 0x83,
        ],
        "-18db"
    ),
    setting!(
        "midi_channel",
        "MIDI Channel",
        Mixer,
        DjmMySetting,
        20,
        choices![
            "1" "1" = 0x80, "2" "2" = 0x81, "3" "3" = 0x82, "4" "4" = 0x83, "5" "5" = 0x84, "6" "6" = 0x85,
            "7" "7" = 0x86, "8" "8" = 0x87, "9" "9" = 0x88, "10" "10" = 0x89, "11" "11" = 0x8a, "12" "12" = 0x8b,
            "13" "13" = 0x8c, "14" "14" = 0x8d, "15" "15" = 0x8e, "16" "16" = 0x8f,
        ],
        "1"
    ),
    setting!(
        "midi_button_type",
        "MIDI Button Type",
        Mixer,
        DjmMySetting,
        21,
        choices!["toggle" "Toggle" = 0x80, "trigger" "Trigger" = 0x81],
        "toggle"
    ),
    setting!(
        "mixer_display_brightness",
        "Mixer Display Brightness",
        Mixer,
        DjmMySetting,
        22,
        choices![
            "white" "White" = 0x80, "1" "1" = 0x81, "2" "2" = 0x82, "3" "3" = 0x83, "4" "4" = 0x84, "5" "5" = 0x85,
        ],
        "5"
    ),
    setting!(
        "mixer_indicator_brightness",
        "Mixer Indicator Brightness",
        Mixer,
        DjmMySetting,
        23,
        choices!["1" "1" = 0x80, "2" "2" = 0x81, "3" "3" = 0x82],
        "3"
    ),
    setting!(
        "channel_fader_curve_long",
        "Channel Fader Curve (Long Fader)",
        Mixer,
        DjmMySetting,
        24,
        choices![
            "exponential" "Exponential" = 0x80, "smooth" "Smooth" = 0x81, "linear" "Linear" = 0x82,
        ],
        "exponential"
    ),
    // DEVSETTING.DAT: waveform/key display
    setting!(
        "overview_waveform_type",
        "Overview Waveform Type",
        Display,
        DevSetting,
        9,
        choices!["half" "Half Waveform" = 0x01, "full" "Full Waveform" = 0x02],
        "half"
    ),
    setting!(
        "waveform_color",
        "Waveform Color",
        Display,
        DevSetting,
        10,
        choices!["blue" "Blue" = 0x01, "rgb" "RGB" = 0x03, "3band" "3Band" = 0x04],
        "blue"
    ),
    setting!(
        "key_display_format",
        "Key Display Format",
        Display,
        DevSetting,
        12,
        choices!["classic" "Classic" = 0x01, "alphanumeric" "Alphanumeric" = 0x02],
        "classic"
    ),
    setting!(
        "waveform_current_position",
        "Waveform Current Position",
        Display,
        DevSetting,
        13,
        choices!["center" "Center" = 0x01, "left" "Left" = 0x02],
        "center"
    ),
];

/// Look up a setting by key (`auto-cue`, `auto_cue` and `auto cue` all work, as for values).
pub fn find(key: &str) -> Option<&'static SettingDef> {
    let want = key.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    SCHEMA.iter().find(|d| d.key == want)
}

/// Validate a key/value pair, returning canonical ids.
pub fn validate(key: &str, value: &str) -> Result<(&'static SettingDef, &'static Choice)> {
    let def = find(key).ok_or_else(|| {
        Error::invalid(format!("unknown setting {key:?}"))
            .because(Reason::UnknownSetting)
            .with("key", key)
            .action(Action::call("settings.schema", serde_json::json!({}), "List the settings"))
    })?;
    let choice = def.choice(value).ok_or_else(|| {
        let ids: Vec<&str> = def.choices.iter().map(|c| c.id).collect();
        Error::invalid(format!("invalid value {value:?} for {}; expected one of: {}", def.key, ids.join(", ")))
    })?;
    Ok((def, choice))
}

// ---------------------------------------------------------------------------------------------
// One settings file
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsBlob {
    pub file: SettingsFile,
    bytes: Vec<u8>,
}

impl SettingsBlob {
    /// A file with all defaults, as rekordbox would write it.
    pub fn default_for(file: SettingsFile) -> Self {
        let data = file.default_data();
        let mut bytes = Vec::with_capacity(HEADER_LEN + data.len() + 4);
        bytes.extend_from_slice(&0x60u32.to_le_bytes());
        for s in [file.brand(), "rekordbox", "0.001"] {
            let mut field = [0u8; 32];
            field[..s.len()].copy_from_slice(s.as_bytes());
            bytes.extend_from_slice(&field);
        }
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&data);
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        let mut blob = SettingsBlob { file, bytes };
        blob.update_checksum();
        blob
    }

    pub fn parse(file: SettingsFile, bytes: Vec<u8>) -> Result<Self> {
        let name = file.file_name();
        let bad = |msg: String| Error::parse(name, msg);
        if bytes.len() < HEADER_LEN + 4 {
            return Err(bad(format!("too short ({} bytes)", bytes.len())));
        }
        let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        if u32_at(0) != 0x60 {
            return Err(bad("unexpected string section length".into()));
        }
        let len = u32_at(100) as usize;
        if len != file.data_len() || bytes.len() != HEADER_LEN + len + 4 {
            return Err(bad(format!("unexpected data length {len} (file is {} bytes)", bytes.len())));
        }
        Ok(SettingsBlob { file, bytes })
    }

    pub fn read(file: SettingsFile, path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).at(path)?;
        Self::parse(file, bytes).map_err(|e| match e {
            Error::Parse { message, .. } => Error::parse(path, message),
            e => e,
        })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    fn data(&self) -> &[u8] {
        &self.bytes[HEADER_LEN..HEADER_LEN + self.file.data_len()]
    }

    /// The software version string in the header (`0.001` on exports, `6.6.1`-ish locally).
    pub fn version(&self) -> String {
        let field = &self.bytes[68..100];
        let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        String::from_utf8_lossy(&field[..end]).into_owned()
    }

    pub fn raw(&self, def: &SettingDef) -> u8 {
        debug_assert_eq!(def.file, self.file);
        self.data()[def.offset]
    }

    pub fn get(&self, def: &SettingDef) -> SettingValue {
        let b = self.raw(def);
        match def.by_byte(b) {
            Some(c) => SettingValue::Known(c.id.to_string()),
            None => SettingValue::Unknown(b),
        }
    }

    /// Set a value; returns whether anything changed.
    pub fn set(&mut self, def: &SettingDef, choice: &Choice) -> bool {
        debug_assert_eq!(def.file, self.file);
        let i = HEADER_LEN + def.offset;
        if self.bytes[i] == choice.byte {
            return false;
        }
        self.bytes[i] = choice.byte;
        self.update_checksum();
        true
    }

    fn computed_checksum(&self) -> u16 {
        let end = HEADER_LEN + self.file.data_len();
        let start = if self.file == SettingsFile::DjmMySetting { 0 } else { HEADER_LEN };
        crc16_xmodem(&self.bytes[start..end])
    }

    pub fn stored_checksum(&self) -> u16 {
        let o = HEADER_LEN + self.file.data_len();
        u16::from_le_bytes([self.bytes[o], self.bytes[o + 1]])
    }

    pub fn checksum_ok(&self) -> bool {
        self.computed_checksum() == self.stored_checksum()
    }

    fn update_checksum(&mut self) {
        let crc = self.computed_checksum();
        let o = HEADER_LEN + self.file.data_len();
        self.bytes[o..o + 2].copy_from_slice(&crc.to_le_bytes());
    }
}

/// CRC-16/XMODEM: poly 0x1021, init 0, no reflection, no final xor.
pub fn crc16_xmodem(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SettingValue {
    Known(String),
    /// A byte value not in the schema (newer firmware?). Preserved untouched.
    Unknown(u8),
}

impl std::fmt::Display for SettingValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingValue::Known(s) => f.write_str(s),
            SettingValue::Unknown(b) => write!(f, "unknown(0x{b:02x})"),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// A set of settings files (a drive's PIONEER folder, or rekordbox's local settings folder)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SettingsSnapshot {
    pub values: BTreeMap<String, SettingValue>,
    pub missing_files: Vec<SettingsFile>,
    pub problems: Vec<String>,
    /// Bytes outside every known setting that differ from what rekordbox 6 writes: settings newer
    /// rekordbox versions added (rekordbox 7.2 writes six). Preserved by `apply`, but not managed
    /// by profiles until their meaning is known.
    pub undecoded: Vec<UndecodedByte>,
}

/// A data byte of a settings file that no known setting covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct UndecodedByte {
    pub file: SettingsFile,
    /// Offset in the file's data section (after the 104-byte header).
    pub offset: usize,
    pub value: u8,
    /// What rekordbox 6 writes there.
    pub expected: u8,
}

/// Where each settings file lives for a given source.
pub trait SettingsLocation {
    fn settings_path(&self, file: SettingsFile) -> PathBuf;
}

/// Drive root: files live in `PIONEER/`.
pub struct DriveSettings<'a>(pub &'a Path);

impl SettingsLocation for DriveSettings<'_> {
    fn settings_path(&self, file: SettingsFile) -> PathBuf {
        paths::to_fs(self.0, file.key())
    }
}

/// A plain directory containing the `.DAT` files (e.g. rekordbox's local settings folder).
pub struct FlatSettings<'a>(pub &'a Path);

impl SettingsLocation for FlatSettings<'_> {
    fn settings_path(&self, file: SettingsFile) -> PathBuf {
        self.0.join(file.file_name())
    }
}

/// rekordbox's own settings folder on this computer (holds the "My Settings" it exports).
pub fn rekordbox_settings_dir() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    #[cfg(target_os = "macos")]
    let dir = base.home_dir().join("Library/Application Support/Pioneer/rekordbox6");
    #[cfg(windows)]
    let dir = base.config_dir().join("Pioneer").join("rekordbox6");
    #[cfg(all(unix, not(target_os = "macos")))]
    let dir = base.config_dir().join("Pioneer").join("rekordbox6");
    dir.is_dir().then_some(dir)
}

pub fn read_all(loc: &impl SettingsLocation) -> SettingsSnapshot {
    let mut snap = SettingsSnapshot {
        values: BTreeMap::new(),
        missing_files: Vec::new(),
        problems: Vec::new(),
        undecoded: Vec::new(),
    };
    for file in SettingsFile::ALL {
        let path = loc.settings_path(file);
        if !path.is_file() {
            snap.missing_files.push(file);
            continue;
        }
        match SettingsBlob::read(file, &path) {
            Ok(blob) => {
                if !blob.checksum_ok() {
                    snap.problems.push(format!("{}: checksum mismatch", file.file_name()));
                }
                for def in SCHEMA.iter().filter(|d| d.file == file) {
                    snap.values.insert(def.key.to_string(), blob.get(def));
                }
                let known: Vec<usize> = SCHEMA.iter().filter(|d| d.file == file).map(|d| d.offset).collect();
                let defaults = file.default_data();
                for (offset, (&value, &expected)) in blob.data().iter().zip(&defaults).enumerate() {
                    if value != expected && !known.contains(&offset) {
                        snap.undecoded.push(UndecodedByte { file, offset, value, expected });
                    }
                }
            }
            Err(e) => snap.problems.push(e.to_string()),
        }
    }
    snap
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SettingChange {
    pub key: String,
    pub file: SettingsFile,
    /// `None` when the file did not exist yet.
    pub from: Option<SettingValue>,
    pub to: String,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct ApplyReport {
    pub changes: Vec<SettingChange>,
    pub files_written: Vec<SettingsFile>,
    pub files_created: Vec<SettingsFile>,
    pub dry_run: bool,
}

/// Make the settings at `loc` match `desired` (canonical key -> value id). Keys not in
/// `desired` keep their current value. Missing files are created only when `desired` touches
/// them, or when `create_missing` is set.
pub fn apply(
    loc: &impl SettingsLocation,
    desired: &BTreeMap<String, String>,
    create_missing: bool,
    dry_run: bool,
) -> Result<ApplyReport> {
    let (mut report, files) = changes(loc, desired, create_missing)?;
    report.dry_run = dry_run;
    if !dry_run {
        for (file, bytes) in files {
            fsutil::write_atomic(&loc.settings_path(file), &bytes)?;
        }
    }
    Ok(report)
}

/// [`apply`] to a drive: the files are written through the drive's write routine, backed up
/// first so `restore` undoes it.
pub fn apply_to_drive(
    device: &mut crate::device::Device,
    desired: &BTreeMap<String, String>,
    create_missing: bool,
    ctx: &crate::progress::Ctx<'_>,
) -> Result<ApplyReport> {
    let _lock = crate::commit::lock(device, None)?;
    for f in SettingsFile::ALL {
        device.checked_path(f.key())?; // PIONEER/ must not be a symlink out of the drive
    }
    let (report, files) = changes(&DriveSettings(device.root()), desired, create_missing)?;
    if files.is_empty() {
        return Ok(report);
    }
    let files = files.into_iter().map(|(f, bytes)| crate::commit::NewFile::bytes(f.key(), bytes)).collect();
    let changes = crate::commit::Changes { files, ..Default::default() };
    let mut opts = crate::commit::Options::new(crate::device::LineageOp::Edit, "write My Settings")
        .backup("before writing My Settings");
    opts.lineage = false;
    crate::commit::commit(device, changes, opts, ctx)?;
    Ok(report)
}

/// Settings files' new bytes.
pub type NewFiles = Vec<(SettingsFile, Vec<u8>)>;

/// What [`apply`] would change, and the files' new bytes.
pub fn changes(
    loc: &impl SettingsLocation,
    desired: &BTreeMap<String, String>,
    create_missing: bool,
) -> Result<(ApplyReport, NewFiles)> {
    let mut report = ApplyReport::default();
    let mut files = Vec::new();
    for file in SettingsFile::ALL {
        let wanted: Vec<(&SettingDef, &Choice)> = desired
            .iter()
            .map(|(k, v)| validate(k, v))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|(d, _)| d.file == file)
            .collect();
        let path = loc.settings_path(file);
        let existing = if path.is_file() { Some(SettingsBlob::read(file, &path)?) } else { None };
        if existing.is_none() && wanted.is_empty() && !create_missing {
            continue;
        }
        let created = existing.is_none();
        let mut blob = existing.clone().unwrap_or_else(|| SettingsBlob::default_for(file));
        for (def, choice) in wanted {
            let before = existing.as_ref().map(|b| b.get(def));
            if blob.set(def, choice) || created {
                report.changes.push(SettingChange { key: def.key.into(), file, from: before, to: choice.id.into() });
            }
        }
        // Repair a stale checksum even if no value changed.
        let repaired = !created && existing.as_ref().is_some_and(|b| !b.checksum_ok());
        if repaired {
            blob.update_checksum();
        }
        if created || existing.as_ref() != Some(&blob) {
            files.push((file, blob.as_bytes().to_vec()));
            if created {
                report.files_created.push(file);
            }
            report.files_written.push(file);
        }
    }
    Ok((report, files))
}

// ---------------------------------------------------------------------------------------------
// Profile: settings defined once, applied to many drives
// ---------------------------------------------------------------------------------------------

/// One setting in the settings profile's base, before and after importing.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct BaseChange {
    pub key: String,
    /// `None`: not set.
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SettingsProfile {
    /// Applied to every drive.
    #[serde(default)]
    pub base: BTreeMap<String, String>,
    /// Per-drive deviations, keyed by drive id.
    #[serde(default)]
    pub drives: BTreeMap<Uuid, DriveOverrides>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct DriveOverrides {
    /// Drive name when the override was last edited (for display only).
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub values: BTreeMap<String, String>,
}

impl SettingsProfile {
    /// Base values with the drive's overrides on top.
    pub fn effective(&self, drive: Option<Uuid>) -> BTreeMap<String, String> {
        let mut out = self.base.clone();
        if let Some(o) = drive.and_then(|d| self.drives.get(&d)) {
            out.extend(o.values.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        out
    }

    /// Set a value in the base profile, or as an override for `drive`. Values are validated
    /// and stored in canonical form.
    pub fn set(&mut self, drive: Option<(Uuid, &str)>, key: &str, value: &str) -> Result<()> {
        let (def, choice) = validate(key, value)?;
        let (k, v) = (def.key.to_string(), choice.id.to_string());
        match drive {
            None => {
                self.base.insert(k, v);
            }
            Some((id, name)) => {
                let o = self.drives.entry(id).or_default();
                o.name = name.to_string();
                o.values.insert(k, v);
            }
        }
        Ok(())
    }

    pub fn unset(&mut self, drive: Option<Uuid>, key: &str) -> Result<bool> {
        let def = find(key).ok_or_else(|| Error::invalid(format!("unknown setting {key:?}")))?;
        Ok(match drive {
            None => self.base.remove(def.key).is_some(),
            Some(id) => {
                let removed = self.drives.get_mut(&id).is_some_and(|o| o.values.remove(def.key).is_some());
                self.drives.retain(|_, o| !o.values.is_empty());
                removed
            }
        })
    }

    /// Set every known value read from a source (drive or rekordbox) in the base, keeping the
    /// rest. Returns how many were set.
    pub fn import_base(&mut self, snapshot: &SettingsSnapshot) -> usize {
        let mut n = 0;
        for (k, v) in &snapshot.values {
            if let SettingValue::Known(id) = v {
                self.base.insert(k.clone(), id.clone());
                n += 1;
            }
        }
        n
    }
}

/// How a drive's current settings deviate from what the profile wants.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Drift {
    pub key: String,
    pub expected: String,
    pub actual: Option<SettingValue>,
    /// Whether the expectation comes from a per-drive override.
    pub overridden: bool,
}

pub fn drift(profile: &SettingsProfile, drive: Option<Uuid>, current: &SettingsSnapshot) -> Vec<Drift> {
    let overrides = drive.and_then(|d| profile.drives.get(&d));
    profile
        .effective(drive)
        .into_iter()
        .filter_map(|(key, expected)| {
            let actual = current.values.get(&key).cloned();
            let matches = matches!(&actual, Some(SettingValue::Known(v)) if *v == expected);
            (!matches).then(|| Drift {
                overridden: overrides.is_some_and(|o| o.values.contains_key(&key)),
                key,
                expected,
                actual,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_consistent() {
        let mut keys = std::collections::HashSet::new();
        for def in SCHEMA {
            assert!(keys.insert(def.key), "duplicate key {}", def.key);
            assert!(def.offset < def.file.data_len(), "{} offset out of range", def.key);
            let _ = def.default_choice();
            let mut bytes: Vec<u8> = def.choices.iter().map(|c| c.byte).collect();
            bytes.sort_unstable();
            bytes.dedup();
            assert_eq!(bytes.len(), def.choices.len(), "{} has duplicate bytes", def.key);
        }
        // No two settings share a byte.
        let mut slots = std::collections::HashSet::new();
        for def in SCHEMA {
            assert!(slots.insert((def.file, def.offset)), "{} overlaps another setting", def.key);
        }
    }

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

        // Read from a file, the error names the file.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(file.file_name());
        std::fs::write(&path, &good[..10]).unwrap();
        let err = SettingsBlob::read(file, &path).unwrap_err();
        assert!(matches!(&err, Error::Parse { path: p, .. } if *p == path), "{err}");
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

    #[test]
    fn lenient_lookup() {
        assert_eq!(validate("auto-cue-level", "-60dB").unwrap().1.id, "-60db");
        assert_eq!(validate("crossfader_curve", "Slow Cut").unwrap().1.id, "slow-cut");
        assert_eq!(validate("language", "Deutsch").unwrap().1.id, "german");
        assert!(validate("quantize", "maybe").is_err());
        assert!(validate("nope", "on").is_err());
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
        let mut want = BTreeMap::new();
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
}
