//! Cues worked out from a track's beat grid and phrases (`cues auto`): where a style puts them,
//! before they're set as [`super::CueEdit::SetMany`] sets cues.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{CuePosition, HotCue, SetCue};
use crate::anlz::{Beat, PhraseKind, SongStructure};

/// rekordbox's name for the memory cue it puts on the first downbeat: bar 1, beat 1.
pub const FIRST_CUE_NAME: &str = "1.1Bars";

/// Where a style puts cues. Every style puts a memory cue on the first downbeat, named as
/// rekordbox names its own (`1.1Bars`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CueStyle {
    /// Only the memory cue on the first downbeat, as rekordbox's first cue does.
    FirstCue,
    /// Hot cues A-H on where each new phrase starts (a run of the same kind is one), named after
    /// the phrase: hot cues from phrase analysis as rekordbox offers them, not yet checked against
    /// tracks rekordbox gave its own.
    Rekordbox,
    /// Hot cue A in green on the first downbeat and hot cues H, G and F in pink on the first three
    /// choruses (H the earliest).
    IntroChorus,
}

impl CueStyle {
    pub const ALL: [CueStyle; 3] = [CueStyle::Rekordbox, CueStyle::IntroChorus, CueStyle::FirstCue];

    /// `intro-chorus`
    pub fn name(self) -> &'static str {
        match self {
            CueStyle::FirstCue => "first-cue",
            CueStyle::Rekordbox => "rekordbox",
            CueStyle::IntroChorus => "intro-chorus",
        }
    }

    /// One line on what it does, for a picker.
    pub fn about(self) -> &'static str {
        match self {
            CueStyle::FirstCue => "a memory cue on the first downbeat only",
            CueStyle::Rekordbox => "hot cues A-H where each phrase starts, named after it",
            CueStyle::IntroChorus => "A (green) on the intro, H/G/F (pink) on the first three choruses",
        }
    }
}

impl std::str::FromStr for CueStyle {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<CueStyle, String> {
        CueStyle::ALL.into_iter().find(|c| c.name() == s.trim().to_ascii_lowercase()).ok_or_else(|| {
            let names: Vec<&str> = CueStyle::ALL.iter().map(|c| c.name()).collect();
            format!("unknown cue style {s:?} (one of {})", names.join(", "))
        })
    }
}

/// What `cues auto` does to the tracks it's given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AutoCues {
    pub style: CueStyle,
    /// Replace hot cues already in the slots the style uses (by letter). Without it they're kept
    /// and the style's cue there is left out. A memory cue within a bar of the first downbeat is
    /// the track's first cue either way, and is never replaced.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub overwrite: bool,
}

const GREEN: &str = "#28e214";
const PINK: &str = "#ff127b";

/// The cues `style` puts on a track with `beats` and `phrases`, in the order to set them. None for
/// a track with no grid; only the first memory cue for one without phrases.
pub fn place(style: CueStyle, beats: &[Beat], phrases: Option<&SongStructure>) -> Vec<SetCue> {
    let Some(first) = beats.iter().position(|b| b.beat_number == 1).or((!beats.is_empty()).then_some(0)) else {
        return Vec::new();
    };
    let at = |i: usize| CuePosition::Beat(u32::try_from(i + 1).unwrap_or(u32::MAX));
    let cue = |hot: Option<u8>, i: usize, name: Option<String>, color: Option<&str>| SetCue {
        hot_cue: hot.map(HotCue),
        at: at(i),
        snap: false,
        loop_beats: None,
        name,
        color: color.map(String::from),
    };
    let mut out = vec![cue(None, first, Some(FIRST_CUE_NAME.into()), None)];
    let starts = phrase_starts(beats.len(), phrases);
    match style {
        CueStyle::FirstCue => {}
        CueStyle::Rekordbox => {
            for (slot, (i, kind)) in (0..HotCue::COUNT).zip(starts) {
                out.push(cue(Some(slot), i, Some(kind.name()), None));
            }
        }
        CueStyle::IntroChorus => {
            out.push(cue(Some(0), first, None, Some(GREEN)));
            let choruses = starts.iter().filter(|(_, k)| *k == PhraseKind::Chorus);
            for (slot, (i, _)) in [7, 6, 5].into_iter().zip(choruses) {
                out.push(cue(Some(slot), *i, None, Some(PINK)));
            }
        }
    }
    out
}

/// Where each new phrase starts on a grid `grid_beats` long, as an index into it: the first of
/// each run of phrases of one kind, in time order, leaving out any past the grid's end.
fn phrase_starts(grid_beats: usize, phrases: Option<&SongStructure>) -> Vec<(usize, PhraseKind)> {
    let Some(s) = phrases else { return Vec::new() };
    let scale = s.beat_scale(grid_beats);
    let mut all: Vec<(usize, PhraseKind)> = s
        .phrases
        .iter()
        .map(|p| (SongStructure::grid_index(p.start_beat, scale), p.kind))
        .filter(|(i, _)| *i < grid_beats)
        .collect();
    all.sort_by_key(|(i, _)| *i);
    all.dedup_by_key(|(i, _)| *i);
    let mut out: Vec<(usize, PhraseKind)> = Vec::new();
    for (i, kind) in all {
        if out.last().is_none_or(|(_, k)| *k != kind) {
            out.push((i, kind));
        }
    }
    out
}

#[cfg(test)]
mod tests;
