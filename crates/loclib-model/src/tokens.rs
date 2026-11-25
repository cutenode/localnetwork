//! Colours that mean the same wherever DJ data is drawn: the player, locallibrary's HTML report
//! and the website's mix report. Each keeps its own chrome and accent.
//!
//! Generated from packages/tokens/colors.json in the repository: don't edit it by hand. After
//! changing the JSON, regenerate it with `LOCALLIBRARY_BLESS=1 cargo test -p loclib-model --test tokens`.

/// An 8-bit sRGB colour.
pub type Rgb = [u8; 3];

/// The 3-band waveform.
pub mod bands {
    use super::Rgb;
    /// The low band.
    pub const LOW: Rgb = [57, 135, 229];
    /// The mid band.
    pub const MID: Rgb = [217, 89, 38];
    /// The high band.
    pub const HIGH: Rgb = [236, 233, 223];
    /// Each band's height against the lows' (low, mid, high): lows outermost, the highs a bright
    /// core.
    pub const SCALES: [f32; 3] = [1.0, 0.66, 0.32];
}

/// rekordbox's phrase kinds by role, for a dark background.
pub mod phrases {
    use super::Rgb;
    /// The intro phrase kind.
    pub const INTRO: Rgb = [70, 112, 232];
    /// The up phrase kind.
    pub const UP: Rgb = [232, 168, 60];
    /// The verse phrase kind.
    pub const VERSE: Rgb = [232, 168, 60];
    /// The chorus phrase kind.
    pub const CHORUS: Rgb = [236, 82, 142];
    /// The down phrase kind.
    pub const DOWN: Rgb = [64, 184, 222];
    /// The bridge phrase kind.
    pub const BRIDGE: Rgb = [64, 184, 222];
    /// The outro phrase kind.
    pub const OUTRO: Rgb = [132, 92, 222];
    /// The unknown phrase kind.
    pub const UNKNOWN: Rgb = [90, 90, 112];
}

/// The phrase kinds for a light background: 3:1 on a near-white page, so the strips stand out.
pub mod phrases_light {
    use super::Rgb;
    /// The intro phrase kind.
    pub const INTRO: Rgb = [70, 112, 232];
    /// The up phrase kind.
    pub const UP: Rgb = [175, 117, 21];
    /// The verse phrase kind.
    pub const VERSE: Rgb = [175, 117, 21];
    /// The chorus phrase kind.
    pub const CHORUS: Rgb = [204, 42, 108];
    /// The down phrase kind.
    pub const DOWN: Rgb = [30, 140, 174];
    /// The bridge phrase kind.
    pub const BRIDGE: Rgb = [30, 140, 174];
    /// The outro phrase kind.
    pub const OUTRO: Rgb = [132, 92, 222];
    /// The unknown phrase kind.
    pub const UNKNOWN: Rgb = [90, 90, 112];
}

/// Cues without a colour of their own, for a dark background.
pub mod cues {
    use super::Rgb;
    /// A hot cue.
    pub const HOT: Rgb = [40, 226, 20];
    /// A memory cue.
    pub const MEMORY: Rgb = [232, 48, 48];
    /// A loop cue.
    pub const LOOP: Rgb = [255, 140, 30];
}

/// The cue defaults for a light background: 3:1 on a near-white page, so the marks stand out.
pub mod cues_light {
    use super::Rgb;
    /// A hot cue.
    pub const HOT: Rgb = [27, 151, 13];
    /// A memory cue.
    pub const MEMORY: Rgb = [232, 48, 48];
    /// A loop cue.
    pub const LOOP: Rgb = [203, 99, 0];
}

/// rekordbox's track colours (and memory cue colour table), roughly as its players show them.
pub mod track_colors {
    use super::Rgb;
    /// rekordbox's pink.
    pub const PINK: Rgb = [248, 82, 201];
    /// rekordbox's red.
    pub const RED: Rgb = [232, 48, 48];
    /// rekordbox's orange.
    pub const ORANGE: Rgb = [255, 140, 30];
    /// rekordbox's yellow.
    pub const YELLOW: Rgb = [240, 220, 40];
    /// rekordbox's green.
    pub const GREEN: Rgb = [60, 200, 80];
    /// rekordbox's aqua.
    pub const AQUA: Rgb = [40, 200, 220];
    /// rekordbox's blue.
    pub const BLUE: Rgb = [48, 96, 240];
    /// rekordbox's purple.
    pub const PURPLE: Rgb = [150, 72, 230];
}

/// Good, warning and critical.
pub mod status {
    use super::Rgb;

    /// For a dark background.
    pub mod dark {
        use super::Rgb;
        /// The good level.
        pub const GOOD: Rgb = [120, 222, 140];
        /// The warning level.
        pub const WARNING: Rgb = [255, 196, 86];
        /// The critical level.
        pub const CRITICAL: Rgb = [255, 100, 100];
    }

    /// For a light background: 4.5:1 on a near-white page, so they work as text.
    pub mod light {
        use super::Rgb;
        /// The good level.
        pub const GOOD: Rgb = [30, 123, 52];
        /// The warning level.
        pub const WARNING: Rgb = [138, 90, 0];
        /// The critical level.
        pub const CRITICAL: Rgb = [198, 40, 40];
    }
}

/// The playhead.
pub const PLAYHEAD: Rgb = [255, 255, 255];
