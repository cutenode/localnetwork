use crate::SettingsFile;

/// What can go wrong reading a settings file or validating a value.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// No setting has that key.
    #[error("unknown setting {0:?}")]
    UnknownSetting(String),

    /// The value isn't one of the setting's choices.
    #[error("invalid value {value:?} for {key}; expected one of: {}", expected.join(", "))]
    InvalidValue { key: &'static str, value: String, expected: Vec<&'static str> },

    /// The bytes aren't a settings file of that kind (too short, or a header or length that
    /// doesn't match).
    #[error("could not parse {}: {message}", file.file_name())]
    Parse { file: SettingsFile, message: String },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
