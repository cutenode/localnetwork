//! One settings file.
//!
//! Files are edited by patching single bytes in the existing file and recomputing the CRC, rather
//! than re-serialising a parsed struct. Bytes we don't understand (and fields newer firmware may
//! add) are therefore preserved exactly.
//!
//! File layout: `u32 0x60`, three 32-byte NUL-padded strings (brand, software, version),
//! `u32 len_data`, `len_data` bytes of settings, `u16` CRC-16/XMODEM, `u16 0`.

use serde::{Deserialize, Serialize};

use crate::{Choice, Error, Result, SCHEMA, SettingDef};

const HEADER_LEN: usize = 104;

/// Which of the four settings files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SettingsFile {
    /// `MYSETTING.DAT`: player settings.
    MySetting,
    /// `MYSETTING2.DAT`: newer player settings.
    MySetting2,
    /// `DJMMYSETTING.DAT`: mixer settings.
    DjmMySetting,
    /// `DEVSETTING.DAT`: waveform and key display.
    DevSetting,
}

impl SettingsFile {
    /// Every settings file.
    pub const ALL: [SettingsFile; 4] =
        [SettingsFile::MySetting, SettingsFile::MySetting2, SettingsFile::DjmMySetting, SettingsFile::DevSetting];

    /// The file's name, e.g. `MYSETTING.DAT`.
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

    pub(crate) fn data_len(self) -> usize {
        match self {
            SettingsFile::MySetting | SettingsFile::MySetting2 => 40,
            SettingsFile::DjmMySetting => 52,
            SettingsFile::DevSetting => 32,
        }
    }

    /// Default data section: fixed bytes of unknown meaning (as rekordbox writes them) with
    /// every known setting at its default value.
    pub(crate) fn default_data(self) -> Vec<u8> {
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

/// One settings file's bytes, read and edited in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsBlob {
    /// Which settings file it is.
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

    /// Reads `bytes` as a settings file of kind `file`. The checksum isn't checked here: see
    /// [`SettingsBlob::checksum_ok`].
    ///
    /// # Errors
    ///
    /// [`Error::Parse`] when the bytes are too short, or the header's string section length or
    /// data length isn't what that kind of file has.
    pub fn parse(file: SettingsFile, bytes: Vec<u8>) -> Result<Self> {
        let bad = |message: String| Error::Parse { file, message };
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

    /// The whole file, as it would be written.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn data(&self) -> &[u8] {
        &self.bytes[HEADER_LEN..HEADER_LEN + self.file.data_len()]
    }

    /// The software version string in the header (`0.001` on exports, `6.6.1`-ish locally).
    pub fn version(&self) -> String {
        let field = &self.bytes[68..100];
        let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        String::from_utf8_lossy(&field[..end]).into_owned()
    }

    /// The byte that holds `def`.
    ///
    /// # Panics
    ///
    /// `def` must be one of this file's settings. Debug builds panic when it's another file's,
    /// and any build panics when its offset is past the end of this file's data section.
    pub fn raw(&self, def: &SettingDef) -> u8 {
        debug_assert_eq!(def.file, self.file);
        self.data()[def.offset]
    }

    /// The value of `def`: one of its choices, or [`SettingValue::Unknown`] for a byte it doesn't
    /// have.
    ///
    /// # Panics
    ///
    /// As for [`SettingsBlob::raw`].
    pub fn get(&self, def: &SettingDef) -> SettingValue {
        let b = self.raw(def);
        match def.by_byte(b) {
            Some(c) => SettingValue::Known(c.id.to_string()),
            None => SettingValue::Unknown(b),
        }
    }

    /// Set a value; returns whether anything changed.
    ///
    /// # Panics
    ///
    /// `def` must be one of this file's settings. Debug builds panic when it's another file's,
    /// and any build panics when its offset is past the end of the file.
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

    /// The CRC-16/XMODEM checksum stored in the file.
    pub fn stored_checksum(&self) -> u16 {
        let o = HEADER_LEN + self.file.data_len();
        u16::from_le_bytes([self.bytes[o], self.bytes[o + 1]])
    }

    /// Whether the stored checksum matches the file's contents.
    pub fn checksum_ok(&self) -> bool {
        self.computed_checksum() == self.stored_checksum()
    }

    pub(crate) fn update_checksum(&mut self) {
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

/// A setting's value as read from a file. In JSON, a choice id as a string, or an unknown byte as
/// a number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum SettingValue {
    /// One of the setting's choices, by id.
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

#[cfg(test)]
mod tests;
