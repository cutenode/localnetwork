//! Edits that change only tracks' fields, planned on a model of the tracks they name rather than
//! the whole library. Imports and analyses make one such edit for each track, and planning each
//! one against every track took time in the square of the library's size.

use rustc_hash::FxHashMap;

use super::{Edit, ModelDb, Op, Resolver, plan_edit};
use crate::Result;
use crate::model::Library;
use crate::paths;
use crate::query::TrackSelector;

/// Where each track id and audio file (key) is in a library's tracks. Edits that change only
/// tracks' fields keep it true: no track comes or goes, or moves to another file.
#[derive(Default)]
pub(super) struct TrackIndex {
    by_id: FxHashMap<u32, Vec<usize>>,
    by_key: FxHashMap<String, Vec<usize>>,
}

impl TrackIndex {
    pub(super) fn of(lib: &Library) -> TrackIndex {
        let mut index = TrackIndex::default();
        for (i, t) in lib.tracks.iter().enumerate() {
            index.by_id.entry(t.id).or_default().push(i);
            index.by_key.entry(paths::file_key(&t.file_path)).or_default().push(i);
        }
        index
    }

    /// The positions of every track with one of `ids`, of the files those tracks and `keys` name,
    /// and with the id of one of those, in library order. A model of these alone selects, finds
    /// by file and changes what the whole library's would for an edit naming `ids` or `keys`.
    fn around(&self, lib: &Library, ids: &[u32], keys: &[String]) -> Vec<usize> {
        let with_id = |id: u32| self.by_id.get(&id).into_iter().flatten().copied();
        let named: Vec<usize> = ids.iter().flat_map(|&id| with_id(id)).collect();
        let keys = keys.iter().cloned().chain(named.iter().map(|&i| paths::file_key(&lib.tracks[i].file_path)));
        let of_files: Vec<usize> = keys.flat_map(|k| self.by_key.get(&k).into_iter().flatten().copied()).collect();
        let mut around: Vec<usize> = (of_files.iter().flat_map(|&i| with_id(lib.tracks[i].id)))
            .chain(named)
            .chain(of_files.iter().copied())
            .collect();
        around.sort_unstable();
        around.dedup();
        around
    }

    /// What a set-fields op naming the files `keys` changes: the id of the last track of each file
    /// there is a track of (the others' keys are missing), and where every track with one of those
    /// ids is, in library order.
    pub(super) fn targets(&self, lib: &Library, keys: &[String]) -> (Vec<u32>, Vec<usize>, Vec<String>) {
        let mut missing = Vec::new();
        let ids: Vec<u32> = keys
            .iter()
            .filter_map(|k| {
                let last = self.by_key.get(k).and_then(|at| at.last());
                if last.is_none() {
                    missing.push(k.clone());
                }
                last.map(|&i| lib.tracks[i].id)
            })
            .collect();
        let mut at: Vec<usize> = ids.iter().flat_map(|id| self.by_id.get(id).into_iter().flatten().copied()).collect();
        at.sort_unstable();
        at.dedup();
        (ids, at, missing)
    }
}

/// The ids and files `edit` selects, when it changes only those tracks' fields and names them by
/// id and by file alone (as a replay's edits name them).
fn fields_named(edit: &Edit) -> Option<(&[u32], Vec<String>)> {
    let Edit::EditTracks { tracks, .. } = edit else { return None };
    let named = TrackSelector { ids: tracks.ids.clone(), paths: tracks.paths.clone(), ..TrackSelector::default() };
    (*tracks == named).then(|| (&tracks.ids[..], tracks.paths.iter().map(|p| paths::file_key(p)).collect()))
}

/// Run `f` on a model of only the tracks at `positions`, and put them back when it succeeds.
fn on_tracks<T>(
    model: &mut ModelDb<'_>,
    positions: &[usize],
    f: impl FnOnce(&mut ModelDb<'_>) -> Result<T>,
) -> Result<T> {
    let tracks = positions.iter().map(|&i| model.lib.tracks[i].clone()).collect();
    let mut part = ModelDb { lib: Library { tracks, ..Library::default() }, pdb: model.pdb };
    let made = f(&mut part)?;
    for (&i, t) in positions.iter().zip(part.lib.tracks) {
        model.lib.tracks[i] = t;
    }
    Ok(made)
}

/// [`plan_edit`], made on the tracks it names when it changes only their fields. `index` is the
/// model's, built when first needed and dropped by any other edit.
pub(super) fn plan_edit_indexed(
    resolver: &Resolver,
    model: &mut ModelDb<'_>,
    index: &mut Option<TrackIndex>,
    edit: &Edit,
    warnings: &mut Vec<String>,
) -> Result<(Vec<Op>, String, Vec<Edit>)> {
    let Some((ids, keys)) = fields_named(edit) else {
        *index = None;
        return plan_edit(resolver, model, edit, warnings);
    };
    let around = index.get_or_insert_with(|| TrackIndex::of(&model.lib)).around(&model.lib, ids, &keys);
    on_tracks(model, &around, |part| plan_edit(resolver, part, edit, warnings))
}
