//! `doctor --fix-tempo`: doubling the beat grid of a track whose stored tempo an analysis of the
//! audio hears as half the real one, and halving it where it hears double, as analysis edits
//! ([`crate::analysis_edit`]): planned, backed up, journaled and undone like any other, with the
//! track's BPM set from the new grid. Only the `.DAT`'s grid (`PQTZ`) is doubled or halved; the
//! `.EXT`'s extended grid (`PQT2`) isn't decoded, so it isn't rescaled (the edit takes it out, as
//! with any new grid, and says so).

use crate::analysis_edit::NewAnalysis;
use crate::anlz::{self, Beat};
use crate::device::Device;
use crate::edit::Edit;
use crate::sound::{Plausible, TempoFinding, TempoRelation};
use crate::{Error, Result};

/// What the plan says about the extended grid, which it leaves as the edit does.
const EXTENDED_GRID: &str = "only the beat grid (PQTZ) is doubled or halved: the .EXT's extended grid (PQT2) isn't decoded, so it isn't rescaled";

/// The analysis edits that fix each of `findings` (a doctor report's, of `device`) that's half
/// or double: its track's grid doubled or halved ([`anlz::doubled_grid`], [`anlz::halved_grid`]).
/// Left out, with a line each saying why: findings the library's other tempos favour the stored
/// tempo for, tracks with no grid or one at another tempo than the stored one (doubling it
/// wouldn't fix the stored tempo), and grids that can't be doubled or halved.
pub fn tempo_fixes(device: &Device, findings: &[TempoFinding]) -> Result<(Vec<Edit>, Vec<String>)> {
    let library = device.library()?;
    let (mut edits, mut notes) = (Vec::new(), Vec::new());
    let (mut favoured, mut no_grid, mut other_tempo) = (0, 0, 0);
    for f in findings {
        let scaled: fn(&[Beat]) -> Vec<Beat> = match f.relation {
            TempoRelation::Half => anlz::doubled_grid,
            TempoRelation::Double => anlz::halved_grid,
            _ => continue,
        };
        if f.likelier == Some(Plausible::Stored) {
            favoured += 1;
            continue;
        }
        let Some(t) = library.tracks.iter().find(|t| t.id == f.track.id) else {
            return Err(Error::NotFound(format!("track {} on {}", f.track.name, device.name())));
        };
        let beats = anlz::load_grid(device, t).beats;
        let Some(tempo) = NewAnalysis { beats: Some(beats.clone()), ..Default::default() }.tempo() else {
            no_grid += 1;
            continue;
        };
        if TempoRelation::between(t.bpm, tempo) != Some(TempoRelation::Agrees) {
            other_tempo += 1;
            continue;
        }
        match anlz::checked_grid(&scaled(&beats)) {
            Ok(beats) => edits.push(Edit::SetAnalysis {
                track: t.file_path.clone(),
                analysis: NewAnalysis { beats: Some(beats), ..Default::default() },
            }),
            Err(e) => notes.push(format!("{} is left as it is: {e}", t.display_name())),
        }
    }
    if favoured > 0 {
        notes.push(format!("{favoured} track(s) left as they are: the library's other tempos favour the stored one"));
    }
    if no_grid > 0 {
        notes.push(format!("{no_grid} track(s) left as they are: no beat grid to double or halve"));
    }
    if other_tempo > 0 {
        notes.push(format!("{other_tempo} track(s) left as they are: their beat grid isn't at their stored tempo"));
    }
    if !edits.is_empty() {
        notes.push(EXTENDED_GRID.into());
    }
    Ok((edits, notes))
}
