//! Audio files by name: the format each audio extension names, and which extensions DJ players
//! list.

/// An audio format, as a file's extension names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Mp3,
    /// `.aac`: AAC on its own.
    Aac,
    /// `.m4a` and `.mp4`: an MPEG-4 file, holding AAC or Apple Lossless.
    Mpeg4,
    /// `.alac`: Apple Lossless.
    Alac,
    Wav,
    Aiff,
    Flac,
    /// Ogg Vorbis.
    Ogg,
    Opus,
    Wma,
}

/// The extensions DJ players list, lower case, with the format each names: what the players'
/// manuals give under "Supported file formats".
pub const PLAYER_EXTENSIONS: [(&str, Format); 9] = [
    ("mp3", Format::Mp3),
    ("aac", Format::Aac),
    ("m4a", Format::Mpeg4),
    ("mp4", Format::Mpeg4),
    ("wav", Format::Wav),
    ("aif", Format::Aiff),
    ("aiff", Format::Aiff),
    ("flac", Format::Flac),
    ("fla", Format::Flac),
];

/// The other audio extensions, lower case: spellings players don't list, and formats they don't
/// play.
pub const OTHER_EXTENSIONS: [(&str, Format); 5] =
    [("wave", Format::Wav), ("alac", Format::Alac), ("ogg", Format::Ogg), ("opus", Format::Opus), ("wma", Format::Wma)];

impl Format {
    /// The format an extension (without its dot) names, in any letter case.
    pub fn from_extension(ext: &str) -> Option<Format> {
        find(PLAYER_EXTENSIONS.iter().chain(&OTHER_EXTENSIONS), ext)
    }

    /// The format a file name or path names by its last extension (`Contents/a.b/c.MP3`).
    pub fn from_name(name: &str) -> Option<Format> {
        name.rsplit_once('.').and_then(|(_, ext)| Format::from_extension(ext))
    }

    /// [`Format::from_name`] for files DJ players list: `None` for audio they skip.
    pub fn for_players(name: &str) -> Option<Format> {
        name.rsplit_once('.').and_then(|(_, ext)| find(&PLAYER_EXTENSIONS, ext))
    }
}

fn find<'a>(extensions: impl IntoIterator<Item = &'a (&'a str, Format)>, ext: &str) -> Option<Format> {
    extensions.into_iter().find(|(e, _)| e.eq_ignore_ascii_case(ext)).map(|&(_, f)| f)
}

#[cfg(test)]
mod tests;
