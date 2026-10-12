//! Every known setting: the file and data byte it lives in, and the values it takes.

use serde::Serialize;

use crate::{Error, Result, SettingsFile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema), schemars(rename = "SettingCategory"))]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Player,
    Display,
    Mixer,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Choice {
    pub id: &'static str,
    pub label: &'static str,
    #[serde(skip)]
    pub byte: u8,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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

    pub(crate) fn by_byte(&self, b: u8) -> Option<&'static Choice> {
        self.choices.iter().find(|c| c.byte == b)
    }

    pub(crate) fn default_choice(&self) -> &'static Choice {
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
    let def = find(key).ok_or_else(|| Error::UnknownSetting(key.to_string()))?;
    let choice = def.choice(value).ok_or_else(|| Error::InvalidValue {
        key: def.key,
        value: value.to_string(),
        expected: def.choices.iter().map(|c| c.id).collect(),
    })?;
    Ok((def, choice))
}

#[cfg(test)]
mod tests;
