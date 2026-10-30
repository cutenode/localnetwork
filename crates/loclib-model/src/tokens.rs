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
    pub const LOW: Rgb = [57, 135, 229];
    pub const MID: Rgb = [217, 89, 38];
    pub const HIGH: Rgb = [236, 233, 223];
    /// Each band's height against the lows' (low, mid, high): lows outermost, the highs a bright
    /// core.
    pub const SCALES: [f32; 3] = [1.0, 0.66, 0.32];
}

/// rekordbox's phrase kinds by role, for a dark background.
pub mod phrases {
    use super::Rgb;
    pub const INTRO: Rgb = [70, 112, 232];
    pub const UP: Rgb = [232, 168, 60];
    pub const VERSE: Rgb = [232, 168, 60];
    pub const CHORUS: Rgb = [236, 82, 142];
    pub const DOWN: Rgb = [64, 184, 222];
    pub const BRIDGE: Rgb = [64, 184, 222];
    pub const OUTRO: Rgb = [132, 92, 222];
    pub const UNKNOWN: Rgb = [90, 90, 112];
}

/// The phrase kinds for a light background: 3:1 on a near-white page, so the strips stand out.
pub mod phrases_light {
    use super::Rgb;
    pub const INTRO: Rgb = [70, 112, 232];
    pub const UP: Rgb = [175, 117, 21];
    pub const VERSE: Rgb = [175, 117, 21];
    pub const CHORUS: Rgb = [234, 61, 128];
    pub const DOWN: Rgb = [30, 140, 174];
    pub const BRIDGE: Rgb = [30, 140, 174];
    pub const OUTRO: Rgb = [132, 92, 222];
    pub const UNKNOWN: Rgb = [90, 90, 112];
}

/// Cues without a colour of their own, for a dark background.
pub mod cues {
    use super::Rgb;
    pub const HOT: Rgb = [40, 226, 20];
    pub const MEMORY: Rgb = [232, 48, 48];
    pub const LOOP: Rgb = [255, 140, 30];
}

/// The cue defaults for a light background: 3:1 on a near-white page, so the marks stand out.
pub mod cues_light {
    use super::Rgb;
    pub const HOT: Rgb = [27, 151, 13];
    pub const MEMORY: Rgb = [232, 48, 48];
    pub const LOOP: Rgb = [203, 99, 0];
}

/// rekordbox's track colours (and memory cue colour table), roughly as its players show them.
pub mod track_colors {
    use super::Rgb;
    pub const PINK: Rgb = [248, 82, 201];
    pub const RED: Rgb = [232, 48, 48];
    pub const ORANGE: Rgb = [255, 140, 30];
    pub const YELLOW: Rgb = [240, 220, 40];
    pub const GREEN: Rgb = [60, 200, 80];
    pub const AQUA: Rgb = [40, 200, 220];
    pub const BLUE: Rgb = [48, 96, 240];
    pub const PURPLE: Rgb = [150, 72, 230];
}

/// Good, warning and critical.
pub mod status {
    use super::Rgb;

    /// For a dark background.
    pub mod dark {
        use super::Rgb;
        pub const GOOD: Rgb = [120, 222, 140];
        pub const WARNING: Rgb = [255, 196, 86];
        pub const CRITICAL: Rgb = [255, 100, 100];
    }

    /// For a light background: 4.5:1 on a near-white page, so they work as text.
    pub mod light {
        use super::Rgb;
        pub const GOOD: Rgb = [30, 123, 52];
        pub const WARNING: Rgb = [138, 90, 0];
        pub const CRITICAL: Rgb = [198, 40, 40];
    }
}

/// The playhead.
pub const PLAYHEAD: Rgb = [255, 255, 255];
