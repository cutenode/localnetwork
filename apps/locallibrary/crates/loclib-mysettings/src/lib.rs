#![doc = include_str!("../README.md")]

mod changes;
mod error;
mod file;
mod schema;

pub use changes::{ApplyReport, NewFiles, SettingChange, SettingsSnapshot, UndecodedByte, changes};
pub use error::{Error, Result};
pub use file::{SettingValue, SettingsBlob, SettingsFile, crc16_xmodem};
pub use schema::{Category, Choice, SCHEMA, SettingDef, find, validate};
