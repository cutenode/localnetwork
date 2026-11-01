//! A theme: every colour DJ data and a player's own chrome are drawn in, for a dark background
//! (a host turns them round for a light one, and leaves them out for NO_COLOR). [`Theme::DEFAULT`]
//! is the shared colours ([`crate::tokens`]) and locallibrary's player's own; a theme sets any of
//! them by name ([`TOKENS`]), as data: the colours of packages/tokens/colors.schema.json for a
//! dark background (`cues.hot`, `trackColors.pink`, `status.dark.good`...), the player's roles
//! (`ui.accent`) and the waveform widgets' gradients (`waveform.spectrum`).

use std::borrow::Cow;

use crate::analysis::PhraseKind;
use crate::model::TrackColor;
use crate::tokens::{self, Rgb};

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// Text.
    pub text: Rgb,
    /// Text that matters less: labels, hints, beat ticks.
    pub dim: Rgb,
    /// What's barely there: rails, rules, empty states.
    pub faint: Rgb,
    /// The first accent: artists, the selection's mark, key hints, the played part of a track.
    pub accent: Rgb,
    /// The second: headings, the focused pane, information, keys.
    pub info: Rgb,
    /// The third: virtual folders, My Tags, relative keys.
    pub secondary: Rgb,
    /// Behind the selected row.
    pub selection: Rgb,
    /// A record drawn for a track without cover art.
    pub vinyl: Rgb,
    pub good: Rgb,
    pub warning: Rgb,
    pub critical: Rgb,
    /// The 3-band waveform: low, mid and high.
    pub bands: [Rgb; 3],
    /// Phrases by kind: intro, up, verse, chorus, down, bridge, outro and unknown.
    pub phrases: [Rgb; 8],
    /// A hot cue without a colour of its own.
    pub hot_cue: Rgb,
    /// A memory cue without a colour of its own.
    pub memory_cue: Rgb,
    /// A loop without a colour of its own.
    pub loop_cue: Rgb,
    /// rekordbox's track colours, pink to purple (also the memory cue colour table).
    pub track_colors: [Rgb; 8],
    pub playhead: Rgb,
    /// The spectrum's bars by height, bottom to top: two colours or more.
    pub spectrum: Cow<'static, [Rgb]>,
    /// The scope's trace by loudness, quiet to loud: two colours or more.
    pub scope: Cow<'static, [Rgb]>,
}

/// The default spectrum: blue to violet to pink to amber.
pub const SPECTRUM: &[Rgb] = &[[64, 140, 255], [158, 112, 255], [255, 92, 160], [255, 196, 86]];
/// The default scope: cyan to violet to pink.
pub const SCOPE: &[Rgb] = &[[92, 214, 255], [158, 112, 255], [255, 92, 160]];

/// Every colour a theme may set, by name: colors.schema.json's (but a light background's phrase,
/// cue and status colours, which hosts take from the tokens or work out from the dark ones), in its
/// order, then the player's. One colour
/// each, the `waveform` gradients two or more.
pub const TOKENS: [&str; 36] = [
    "bands.low",
    "bands.mid",
    "bands.high",
    "phrases.intro",
    "phrases.up",
    "phrases.verse",
    "phrases.chorus",
    "phrases.down",
    "phrases.bridge",
    "phrases.outro",
    "phrases.unknown",
    "cues.hot",
    "cues.memory",
    "cues.loop",
    "trackColors.pink",
    "trackColors.red",
    "trackColors.orange",
    "trackColors.yellow",
    "trackColors.green",
    "trackColors.aqua",
    "trackColors.blue",
    "trackColors.purple",
    "status.dark.good",
    "status.dark.warning",
    "status.dark.critical",
    "playhead",
    "ui.text",
    "ui.dim",
    "ui.faint",
    "ui.accent",
    "ui.info",
    "ui.secondary",
    "ui.selection",
    "ui.vinyl",
    "waveform.spectrum",
    "waveform.scope",
];

/// Where a token's colours are in a theme.
enum Slot<'a> {
    One(&'a mut Rgb),
    Stops(&'a mut Cow<'static, [Rgb]>),
}

impl Default for Theme {
    fn default() -> Theme {
        Theme::DEFAULT
    }
}

impl Theme {
    /// The shared colours and the player's own, as locallibrary draws them unless a theme says.
    pub const DEFAULT: Theme = Theme {
        text: [226, 226, 236],
        dim: [120, 120, 142],
        faint: [72, 72, 92],
        accent: [255, 92, 160],
        info: SCOPE[0],
        secondary: SPECTRUM[1],
        selection: [44, 44, 70],
        vinyl: [40, 40, 52],
        good: tokens::status::dark::GOOD,
        warning: tokens::status::dark::WARNING,
        critical: tokens::status::dark::CRITICAL,
        bands: [tokens::bands::LOW, tokens::bands::MID, tokens::bands::HIGH],
        phrases: {
            use tokens::phrases::*;
            [INTRO, UP, VERSE, CHORUS, DOWN, BRIDGE, OUTRO, UNKNOWN]
        },
        hot_cue: tokens::cues::HOT,
        memory_cue: tokens::cues::MEMORY,
        loop_cue: tokens::cues::LOOP,
        track_colors: [
            TrackColor::Pink.rgb(),
            TrackColor::Red.rgb(),
            TrackColor::Orange.rgb(),
            TrackColor::Yellow.rgb(),
            TrackColor::Green.rgb(),
            TrackColor::Aqua.rgb(),
            TrackColor::Blue.rgb(),
            TrackColor::Purple.rgb(),
        ],
        playhead: tokens::PLAYHEAD,
        spectrum: Cow::Borrowed(SPECTRUM),
        scope: Cow::Borrowed(SCOPE),
    };

    /// The colour phrases of `kind` are drawn in (every verse alike).
    pub fn phrase(&self, kind: PhraseKind) -> Rgb {
        self.phrases[match kind {
            PhraseKind::Intro => 0,
            PhraseKind::Up => 1,
            PhraseKind::Verse(_) => 2,
            PhraseKind::Chorus => 3,
            PhraseKind::Down => 4,
            PhraseKind::Bridge => 5,
            PhraseKind::Outro => 6,
            PhraseKind::Unknown(_) => 7,
        }]
    }

    /// The colour rekordbox's track colour `c` is drawn in.
    pub fn track_color(&self, c: TrackColor) -> Rgb {
        self.track_colors[usize::from(c.id() - 1)]
    }

    /// The colours of the token `name` (one of [`TOKENS`]): one, or a gradient's.
    pub fn get(&self, name: &str) -> Option<Vec<Rgb>> {
        Some(match self.clone().slot(name)? {
            Slot::One(c) => vec![*c],
            Slot::Stops(s) => s.to_vec(),
        })
    }

    /// Sets the token `name` (one of [`TOKENS`]) to `colors`: one colour, or for a gradient
    /// (`waveform.*`) two or more. Says why it can't otherwise.
    pub fn set(&mut self, name: &str, colors: &[Rgb]) -> Result<(), String> {
        let slot = self.slot(name).ok_or_else(|| format!("{name:?} isn't a colour a theme sets"))?;
        match (slot, colors) {
            (Slot::One(c), [one]) => *c = *one,
            (Slot::One(_), _) => return Err(format!("{name} is one colour")),
            (Slot::Stops(s), all) if all.len() >= 2 => *s = Cow::Owned(all.to_vec()),
            (Slot::Stops(_), _) => return Err(format!("{name} is a gradient: two colours or more")),
        }
        Ok(())
    }

    fn slot(&mut self, name: &str) -> Option<Slot<'_>> {
        let (group, key) = name.split_once('.').unwrap_or((name, ""));
        let index = |keys: &[&str]| keys.iter().position(|k| *k == key);
        let one = match (group, key) {
            ("bands", _) => &mut self.bands[index(&["low", "mid", "high"])?],
            ("phrases", _) => {
                &mut self.phrases[index(&["intro", "up", "verse", "chorus", "down", "bridge", "outro", "unknown"])?]
            }
            ("cues", "hot") => &mut self.hot_cue,
            ("cues", "memory") => &mut self.memory_cue,
            ("cues", "loop") => &mut self.loop_cue,
            ("trackColors", _) => {
                &mut self.track_colors[index(&["pink", "red", "orange", "yellow", "green", "aqua", "blue", "purple"])?]
            }
            ("status", "dark.good") => &mut self.good,
            ("status", "dark.warning") => &mut self.warning,
            ("status", "dark.critical") => &mut self.critical,
            ("playhead", "") => &mut self.playhead,
            ("ui", "text") => &mut self.text,
            ("ui", "dim") => &mut self.dim,
            ("ui", "faint") => &mut self.faint,
            ("ui", "accent") => &mut self.accent,
            ("ui", "info") => &mut self.info,
            ("ui", "secondary") => &mut self.secondary,
            ("ui", "selection") => &mut self.selection,
            ("ui", "vinyl") => &mut self.vinyl,
            ("waveform", "spectrum") => return Some(Slot::Stops(&mut self.spectrum)),
            ("waveform", "scope") => return Some(Slot::Stops(&mut self.scope)),
            _ => return None,
        };
        Some(Slot::One(one))
    }
}

/// `#rrggbb` (lowercase, as colors.schema.json has them) as a colour.
pub fn hex(s: &str) -> Option<Rgb> {
    let digits =
        s.strip_prefix('#').filter(|d| d.len() == 6 && d.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))?;
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

#[cfg(test)]
mod tests;
