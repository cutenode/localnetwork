//! Feature `analysis`: an analyzer's structured outputs, fields of type `beats` (a beat grid),
//! `phrases` (a song structure) and `waveform` (three bands, in a blob), answered with its other
//! fields from `track/analyze`. The host keeps them apart from the columns, draws them where a
//! track's drive has none, and writes them to the tracks' analysis files only when asked.
//!
//! No method of its own: every plugin the SDK serves has it, and a host without it refuses a
//! manifest declaring such a field.

use loclib_model::analysis::{Beat, SongStructure};
use serde::{Deserialize, Serialize};

use crate::manifest::invalid;
use crate::{Feature, FieldType, FieldValue, Manifest, ManifestError};

pub(crate) const FEATURE: Feature = Feature {
    name: "analysis",
    methods: &[],
    sends: &[],
    contributed: |m| m.contributes.analyzers.iter().any(|a| a.fields.values().any(|t| t.is_structured())),
};

/// A `waveform` field's value: a file in the request's scratch folder holding three bytes a
/// column, its lows, mids and highs, 0-255 ([`loclib_model::analysis::Bands`] packed), and how
/// many columns a second of audio it has.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaveformBlob {
    pub blob: String,
    pub columns_per_sec: f64,
}

/// A structured field's value, as its type says.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    Beats(Vec<Beat>),
    Phrases(SongStructure),
    Waveform(WaveformBlob),
}

impl Output {
    /// `value` as a field of type `kind` holds it (none for null: nothing to report), or why it
    /// isn't one.
    pub fn of(kind: FieldType, value: &FieldValue) -> Result<Option<Output>, String> {
        if *value == FieldValue::Null {
            return Ok(None);
        }
        let json = serde_json::to_value(value).map_err(|e| e.to_string())?;
        let parsed = match kind {
            FieldType::Beats => serde_json::from_value(json).map(Output::Beats),
            FieldType::Phrases => serde_json::from_value(json).map(Output::Phrases),
            FieldType::Waveform => serde_json::from_value(json).map(Output::Waveform),
            _ => return Err(format!("{kind:?} isn't a structured field's type")),
        };
        let out = parsed.map_err(|e| e.to_string())?;
        match &out {
            Output::Waveform(w) if !(w.columns_per_sec.is_finite() && w.columns_per_sec > 0.0) => {
                Err(format!("a waveform's columns a second, {}, aren't more than 0", w.columns_per_sec))
            }
            _ => Ok(Some(out)),
        }
    }
}

impl FieldValue {
    /// A `beats` field's value.
    pub fn beats(beats: &[Beat]) -> FieldValue {
        FieldValue::Structured(serde_json::to_value(beats).expect("beats are JSON"))
    }

    /// A `phrases` field's value.
    pub fn phrases(phrases: &SongStructure) -> FieldValue {
        FieldValue::Structured(serde_json::to_value(phrases).expect("phrases are JSON"))
    }

    /// A `waveform` field's value: the file `blob` names in the scratch folder.
    pub fn waveform(blob: impl Into<String>, columns_per_sec: f64) -> FieldValue {
        let w = WaveformBlob { blob: blob.into(), columns_per_sec };
        FieldValue::Structured(serde_json::to_value(w).expect("a waveform's blob is JSON"))
    }
}

/// An analyzer has at most one structured field of each type.
pub(crate) fn validate(m: &Manifest) -> Result<(), ManifestError> {
    for (i, a) in m.contributes.analyzers.iter().enumerate() {
        for kind in [FieldType::Beats, FieldType::Phrases, FieldType::Waveform] {
            let mut named = a.fields.iter().filter(|(_, t)| **t == kind);
            if let (Some(_), Some((second, _))) = (named.next(), named.next()) {
                let kind = serde_json::to_value(kind).unwrap_or_default();
                let message = format!("another field is its {} already", kind.as_str().unwrap_or("?"));
                return Err(invalid(format!("contributes.analyzers[{i}].fields.{second}"), message));
            }
        }
    }
    Ok(())
}
