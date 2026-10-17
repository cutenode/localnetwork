//! A track's analysis as players show it: the beat grid, cues, waveforms and phrases, decoded from
//! wherever they're stored (locallibrary reads them from rekordbox's ANLZ files), and rekordbox's
//! hot cue palette.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Beat {
    /// Position in the bar, 1-4.
    pub beat_number: u16,
    pub bpm: f64,
    pub time_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Cue {
    /// `None` for memory cues; `Some(0)` = hot cue A, `Some(1)` = B, ...
    pub hot_cue: Option<u8>,
    pub start_ms: u32,
    /// End of the loop, for loops.
    pub end_ms: Option<u32>,
    pub comment: String,
    pub color: Option<[u8; 3]>,
    /// A memory cue's or loop's colour: a row of rekordbox's colour table (1 pink ... 8 purple,
    /// as [`crate::model::TrackColor`]), 0 for none. Only the extended list has it.
    pub color_id: u8,
    /// A loop's length in beats as rekordbox stored it, numerator and denominator (4/1, 1/2),
    /// when it was set on the grid. Only the extended list has it.
    #[cfg_attr(feature = "schemars", schemars(with = "Option<LoopBeats>"))]
    pub loop_beats: Option<(u16, u16)>,
}

crate::labelled_tuple! {
    /// A loop's length in beats as a fraction, `[numerator, denominator]`: `[4, 1]` is 4 beats,
    /// `[1, 2]` half a beat. Only describes `loop_beats` in the schema; the code uses the pair.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct LoopBeats {
        pub numerator: u16,
        pub denominator: u16,
    }
}

/// One drawn layer of a waveform column.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct WaveLayer {
    /// 0.0-1.0 of the waveform's full height.
    pub height: f32,
    pub color: [u8; 3],
}

/// One column of a waveform: a back layer and a (shorter, brighter) front layer drawn over it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct WaveColumn {
    pub back: WaveLayer,
    pub front: WaveLayer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WaveformStyle {
    /// Monochrome blue, older players (`PWAV`, `PWV3`).
    Blue,
    /// Colour by frequency content, nexus 2 and later (`PWV4`, `PWV5`).
    Rgb,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Waveform {
    pub style: WaveformStyle,
    /// Columns per second of audio for scrolling (detail) waveforms; `None` for previews, whose
    /// columns span the whole track.
    pub columns_per_sec: Option<f64>,
    pub columns: Vec<WaveColumn>,
}

/// rekordbox's overall classification of a track, which decides how phrases are named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Mood {
    High,
    Mid,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PhraseKind {
    Intro,
    /// Build-ups, in high-mood tracks.
    Up,
    /// Breakdowns, in high-mood tracks.
    Down,
    Verse(u8),
    Bridge,
    Chorus,
    Outro,
    Unknown(u16),
}

impl PhraseKind {
    pub fn name(self) -> String {
        match self {
            PhraseKind::Intro => "Intro".into(),
            PhraseKind::Up => "Up".into(),
            PhraseKind::Down => "Down".into(),
            PhraseKind::Verse(n) => format!("Verse {n}"),
            PhraseKind::Bridge => "Bridge".into(),
            PhraseKind::Chorus => "Chorus".into(),
            PhraseKind::Outro => "Outro".into(),
            PhraseKind::Unknown(k) => format!("Phrase {k}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Phrase {
    pub kind: PhraseKind,
    /// 1-based beat numbers into the beat grid; the phrase runs up to (not including) `end_beat`.
    pub start_beat: u16,
    pub end_beat: u16,
    /// Beat where a fill-in leads into the next phrase.
    pub fill_beat: Option<u16>,
}

/// rekordbox's phrase analysis (`PSSI`, in `.EXT`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SongStructure {
    pub mood: Mood,
    pub phrases: Vec<Phrase>,
}

/// Everything a player shows about a track, from its analysis files.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TrackAnalysis {
    pub beats: Vec<Beat>,
    pub cues: Vec<Cue>,
    pub preview: Option<Waveform>,
    pub detail: Option<Waveform>,
    pub structure: Option<SongStructure>,
}

/// The colour rekordbox 7 stores (with colour code 0) for a hot cue left at its default green.
pub const DEFAULT_HOT_CUE_RGB: [u8; 3] = [0x1a, 0xff, 0x00];

/// rekordbox's hot cue palette: the colour of code `i + 1` (as beat-link and rekordcrate list
/// them). The RGB a file stores next to the code is close to, but not always the same as, this.
pub const HOT_CUE_PALETTE: [[u8; 3]; 62] = [
    [0x30, 0x5a, 0xff],
    [0x50, 0x73, 0xff],
    [0x50, 0x8c, 0xff],
    [0x50, 0xa0, 0xff],
    [0x50, 0xb4, 0xff],
    [0x50, 0xb0, 0xf2],
    [0x50, 0xae, 0xe8],
    [0x45, 0xac, 0xdb],
    [0x00, 0xe0, 0xff],
    [0x19, 0xda, 0xf0],
    [0x32, 0xd2, 0xe6],
    [0x21, 0xb4, 0xb9],
    [0x20, 0xaa, 0xa0],
    [0x1f, 0xa3, 0x92],
    [0x19, 0xa0, 0x8c],
    [0x14, 0xa5, 0x84],
    [0x14, 0xaa, 0x7d],
    [0x10, 0xb1, 0x76],
    [0x30, 0xd2, 0x6e],
    [0x37, 0xde, 0x5a],
    [0x3c, 0xeb, 0x50],
    [0x28, 0xe2, 0x14],
    [0x7d, 0xc1, 0x3d],
    [0x8c, 0xc8, 0x32],
    [0x9b, 0xd7, 0x23],
    [0xa5, 0xe1, 0x16],
    [0xa5, 0xdc, 0x0a],
    [0xaa, 0xd2, 0x08],
    [0xb4, 0xc8, 0x05],
    [0xb4, 0xbe, 0x04],
    [0xba, 0xb4, 0x04],
    [0xc3, 0xaf, 0x04],
    [0xe1, 0xaa, 0x00],
    [0xff, 0xa0, 0x00],
    [0xff, 0x96, 0x00],
    [0xff, 0x8c, 0x00],
    [0xff, 0x75, 0x00],
    [0xe0, 0x64, 0x1b],
    [0xe0, 0x46, 0x1e],
    [0xe0, 0x30, 0x1e],
    [0xe0, 0x28, 0x23],
    [0xe6, 0x28, 0x28],
    [0xff, 0x37, 0x6f],
    [0xff, 0x2d, 0x6f],
    [0xff, 0x12, 0x7b],
    [0xf5, 0x1e, 0x8c],
    [0xeb, 0x2d, 0xa0],
    [0xe6, 0x37, 0xb4],
    [0xde, 0x44, 0xcf],
    [0xde, 0x44, 0x8d],
    [0xe6, 0x30, 0xb4],
    [0xe6, 0x19, 0xdc],
    [0xe6, 0x00, 0xff],
    [0xdc, 0x00, 0xff],
    [0xcc, 0x00, 0xff],
    [0xb4, 0x32, 0xff],
    [0xb9, 0x3c, 0xff],
    [0xc5, 0x42, 0xff],
    [0xaa, 0x5a, 0xff],
    [0xaa, 0x72, 0xff],
    [0x82, 0x72, 0xff],
    [0x64, 0x73, 0xff],
];

/// The palette colour of a hot cue colour code (1-62).
pub fn hot_cue_palette(code: u8) -> Option<[u8; 3]> {
    HOT_CUE_PALETTE.get(usize::from(code).checked_sub(1)?).copied()
}

#[cfg(test)]
mod tests;
