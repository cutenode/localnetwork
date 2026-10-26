//! A track's files on a drive (its audio, analysis files and artwork), and which of them
//! removing tracks leaves unused.

use indexmap::IndexMap;

use rustc_hash::FxHashSet;

use crate::Result;
use crate::device::Device;
use crate::model::{Library, Track};
use crate::onelibrary::OneLibrary;
use crate::paths::{self, FileClass, fold};
use crate::progress::Ctx;
use crate::tree::FileTree;

/// The files only tracks matching `doomed` use (audio, analysis files, artwork), as drive keys
/// with their sizes, plus audio outside `Contents/` that such tracks reference and that is left
/// where it is. Files a remaining track in any of `libraries` still uses are never included
/// (two rows can share one file, and the two databases share audio and analysis), nor are images
/// OneLibrary's albums, playlists and hot cue bank lists use.
pub(crate) fn removable_files(
    device: &Device,
    libraries: &[&Library],
    plus: Option<&OneLibrary>,
    doomed: &dyn Fn(&Track) -> bool,
    ctx: &Ctx<'_>,
) -> Result<(IndexMap<String, u64>, IndexMap<String, String>)> {
    // Scan with every track either database references (audio outside Contents/ included).
    let everything =
        Library { tracks: libraries.iter().flat_map(|l| l.tracks.iter().cloned()).collect(), ..Default::default() };
    let tree = device.scan(Some(&everything), ctx)?;
    let mut in_use: FxHashSet<String> = FxHashSet::default();
    for t in libraries.iter().flat_map(|l| l.tracks.iter()).filter(|t| !doomed(t)) {
        in_use.insert(paths::file_key(&t.file_path));
        for e in analysis_files(&t.analyze_path, &tree) {
            in_use.insert(fold(&e));
        }
        if let Some(a) = &t.artwork_path {
            in_use.insert(paths::file_key(a));
            in_use.insert(fold(&crate::artwork::medium_variant(&paths::key(a))));
        }
    }
    if let Some(one) = plus {
        for (_, a) in one.images_used_outside_tracks()? {
            in_use.insert(paths::file_key(&a));
            in_use.insert(fold(&crate::artwork::medium_variant(&paths::key(&a))));
        }
    }
    let mut files: IndexMap<String, u64> = IndexMap::new();
    let add = |key: &str, class: FileClass, files: &mut IndexMap<String, u64>| {
        // Only files that really are what the database says, inside PIONEER/ or Contents/: a
        // crafted database must not get us to delete its own database files or anything else.
        if let Some(e) = tree.resolve(key)
            && e.class == class
            && paths::classify(&e.path) == Some(class)
            && !in_use.contains(&fold(&e.path))
        {
            files.insert(e.path.clone(), e.size);
        }
    };
    let mut left_in_place = IndexMap::new();
    for t in libraries.iter().flat_map(|l| l.tracks.iter()).filter(|t| doomed(t)) {
        let audio = paths::key(&t.file_path);
        if paths::classify(&audio).is_none() && tree.resolve(&audio).is_some() {
            left_in_place.insert(fold(&audio), t.file_path.clone());
        }
        add(&audio, FileClass::Audio, &mut files);
        for k in analysis_files(&t.analyze_path, &tree) {
            add(&k, FileClass::Analysis, &mut files);
        }
        if let Some(a) = &t.artwork_path {
            let key = paths::key(a);
            add(&crate::artwork::medium_variant(&key), FileClass::Artwork, &mut files);
            add(&key, FileClass::Artwork, &mut files);
        }
    }
    files.sort_unstable_keys();
    left_in_place.sort_unstable_keys();
    Ok((files, left_in_place))
}

/// A track's analysis files (`.DAT`, `.EXT`, `.2EX`) present on the drive.
pub(crate) fn analysis_files(analyze_path: &str, tree: &FileTree) -> Vec<String> {
    let dat = paths::key(analyze_path);
    if !paths::is_safe_key(&dat) || paths::classify(&dat) != Some(FileClass::Analysis) {
        return Vec::new();
    }
    let Some((stem, _)) = dat.rsplit_once('.') else { return Vec::new() };
    tree.with_prefix(&format!("{stem}."))
        .into_iter()
        .filter(|e| {
            e.class == FileClass::Analysis && e.path.get(stem.len() + 1..).is_some_and(|ext| !ext.contains('/'))
        })
        .map(|e| e.path.clone())
        .collect()
}
