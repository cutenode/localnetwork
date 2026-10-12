//! Converting between the two databases, and exporting rekordbox XML, from drawn drives whose
//! databases may have drifted apart either way. Adding keeps everything both sides had,
//! replacing makes the target the source exactly, a conversion run again has nothing to add, and
//! undoing it gives the drive back. The XML holds every track and playlist the drive does.

use crate::{common, drawn};

use indexmap::IndexMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use hegel::generators as gs;
use hegel::{Generator, HealthCheck, TestCase};
use locallibrary::convert::{self, ConvertOptions, Target};
use locallibrary::device::{Device, LibraryDrift};
use locallibrary::onelibrary::OneLibrary;
use locallibrary::progress::Ctx;
use locallibrary::xml::{self, XmlOptions};
use locallibrary::{Library, backup, paths, pdb};
use rekordbox_pdb::PdbEditor;

fn fold(s: &str) -> String {
    paths::key(s).to_lowercase()
}

/// What a database holds, comparable across databases: track files, every playlist's entries
/// (as files, by folded path; same-named playlists pooled) and history list names.
#[derive(Debug, Default)]
struct Held {
    tracks: BTreeSet<String>,
    playlists: IndexMap<String, BTreeSet<String>>,
    history: BTreeSet<String>,
}

fn held(lib: &Library) -> Held {
    let file = |id: &u32| lib.track(*id).map(|t| fold(&t.file_path));
    let mut h = Held { tracks: lib.tracks.iter().map(|t| fold(&t.file_path)).collect(), ..Default::default() };
    for p in lib.playlists.iter().filter(|p| !p.is_folder) {
        h.playlists
            .entry(lib.playlist_path(p.id).to_lowercase())
            .or_default()
            .extend(p.track_ids.iter().filter_map(file));
    }
    h.history = lib.history.iter().filter(|p| !p.is_folder).map(|p| p.name.to_lowercase()).collect();
    h
}

/// Everything `small` holds, `big` holds too.
fn covers(big: &Held, small: &Held) -> Result<(), String> {
    if let Some(t) = small.tracks.difference(&big.tracks).next() {
        return Err(format!("lost the track {t}"));
    }
    for (path, entries) in &small.playlists {
        match big.playlists.get(path) {
            None => return Err(format!("lost the playlist {path}")),
            Some(e) if !entries.is_subset(e) => return Err(format!("{path} lost entries")),
            Some(_) => {}
        }
    }
    match small.history.difference(&big.history).next() {
        Some(h) => Err(format!("lost the history list {h}")),
        None => Ok(()),
    }
}

fn libraries(root: &Path) -> (Option<Library>, Option<Library>) {
    let pdb = root.join(paths::EXPORT_PDB).is_file().then(|| pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap());
    let plus = root.join(paths::LIBRARY_PLUS_DB).is_file().then(|| common::plus_library(root));
    (pdb, plus)
}

/// A drawn drive with OneLibrary beside export.pdb or instead of it, each side perhaps holding a
/// playlist, entries or a history list the other lacks.
#[hegel::test_helper]
fn drifted_drive(tc: &TestCase, dir: &Path) -> std::path::PathBuf {
    let (root, lib) = drawn::drive(tc, dir, "A");
    if tc.draw(gs::weighted_booleans(0.4)) {
        common::media::set_art(&root, &[lib.tracks[0].id]);
    }
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => return root,
        1 => common::onelibrary_only(&root),
        _ => common::add_onelibrary(&root),
    }
    let ids: Vec<u32> = lib.tracks.iter().map(|t| t.id).collect();
    if root.join(paths::EXPORT_PDB).is_file() && tc.draw(gs::booleans()) {
        let mut ed = PdbEditor::from_file(root.join(paths::EXPORT_PDB)).unwrap();
        let p = ed.create_playlist(tc.draw(gs::sampled_from(drawn::NAMES)), 0, false).unwrap();
        for t in tc.draw(gs::vecs(gs::sampled_from(ids.clone())).max_size(3)) {
            ed.add_to_playlist(p, t, None).unwrap();
        }
        ed.save(root.join(paths::EXPORT_PDB)).unwrap();
    }
    if tc.draw(gs::booleans()) {
        let mut one = OneLibrary::open_root(&root).unwrap();
        let plus_ids: Vec<u32> = one.library().unwrap().tracks.iter().map(|t| t.id).collect();
        let p = one.create_playlist(tc.draw(gs::sampled_from(&["Only Plus", "Peak"][..])), 0, false).unwrap();
        for t in tc.draw(gs::vecs(gs::sampled_from(plus_ids.clone())).max_size(3)) {
            one.add_to_playlist(p, t).unwrap();
        }
        if tc.draw(gs::booleans()) {
            let h = one
                .create_history(&format!("HISTORY {}", tc.draw(gs::sampled_from(&["P01", "001"][..]))), 0, false)
                .unwrap();
            one.add_to_history(h, plus_ids[0]).unwrap();
        }
        fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    }
    root
}

#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn converting_keeps_both_sides_and_undoes_exactly(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = drifted_drive(&tc, dir.path());
    let (pdb_before, plus_before) = libraries(&root);
    let to = match (&pdb_before, &plus_before) {
        (Some(_), Some(_)) => {
            tc.draw(gs::sampled_from(vec![Target::OneLibrary, Target::DeviceLibrary]).print_as_debug())
        }
        (Some(_), None) => Target::OneLibrary,
        _ => Target::DeviceLibrary,
    };
    let replace = tc.draw(gs::booleans());
    let opts = ConvertOptions { to, replace };
    let (source, target) = match to {
        Target::OneLibrary => (pdb_before.clone().unwrap(), plus_before.clone()),
        Target::DeviceLibrary => (plus_before.clone().unwrap(), pdb_before.clone()),
    };
    let files = common::drive_files(&root);

    let mut d = Device::open_export(&root).unwrap();
    let plan = convert::plan(&d, &opts, &Ctx::silent()).unwrap();
    assert_eq!((plan.adds, plan.replaces), (target.is_some() && !replace, target.is_some() && replace));
    let report = convert::apply(&plan, &mut d, &opts, &Ctx::silent()).unwrap();
    assert_eq!(report.tracks, plan.tracks);
    assert_eq!(report.history_playlists, plan.history_playlists);

    let (pdb_after, plus_after) = libraries(&root);
    let (source_after, made) = match to {
        Target::OneLibrary => (pdb_after.unwrap(), plus_after.unwrap()),
        Target::DeviceLibrary => (plus_after.unwrap(), pdb_after.unwrap()),
    };
    assert_eq!(held(&source_after).tracks, held(&source).tracks, "the source database isn't touched");
    covers(&held(&made), &held(&source)).unwrap();
    match (&target, replace) {
        (Some(old), false) => covers(&held(&made), &held(old)).unwrap(),
        // Rebuilt, it holds the source's library and nothing else.
        _ => {
            let drift = match to {
                Target::OneLibrary => LibraryDrift::between(&source, &made),
                Target::DeviceLibrary => LibraryDrift::between(&made, &source),
            };
            assert!(drift.only_in_device_library.is_empty() && drift.only_in_onelibrary.is_empty(), "{drift:?}");
            covers(&held(&source), &held(&made)).unwrap();
        }
    }
    // Artwork comes along, under the converted database's own image names.
    for t in source.tracks.iter().filter(|t| t.artwork_path.is_some()) {
        let m = made.tracks.iter().find(|x| fold(&x.file_path) == fold(&t.file_path)).unwrap();
        let art = m.artwork_path.as_deref().unwrap_or_else(|| panic!("{} lost its artwork", t.file_path));
        assert!(paths::to_fs(&root, &paths::key(art)).is_file(), "{art}");
    }
    if to == Target::OneLibrary {
        common::sqlcipher_reads(&root);
    } else {
        let bytes = fs::read(root.join(paths::EXPORT_PDB)).unwrap();
        assert_eq!(common::rekordcrate_rows(&bytes, rekordcrate::pdb::PageType::Tracks), made.tracks.len());
    }

    // Again, adding: nothing more to add.
    let again = convert::plan(&d, &ConvertOptions { to, replace: false }, &Ctx::silent()).unwrap();
    assert!(again.adds && again.tracks == 0, "{again:?}");

    // Undone, every file is as it was; images it added stay, referenced by nothing.
    backup::restore(&mut d, report.backup.as_deref(), false).unwrap();
    let back = common::drive_files(&root);
    for key in common::changed_files(&files, &back) {
        let added_image = !files.contains_key(&key) && paths::classify(&key) == Some(paths::FileClass::Artwork);
        assert!(added_image, "{key} isn't as it was");
    }
}

// ---------------------------------------------------------------------------------------------
// rekordbox XML
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default)]
struct Xml {
    entries: usize,
    /// TrackID -> (Name, Artist, Location).
    tracks: IndexMap<u32, (String, String, String)>,
    /// Playlist paths (from ROOT down) and their track keys.
    playlists: Vec<(String, Vec<u32>)>,
}

fn read_xml(text: &str) -> Xml {
    use quick_xml::events::Event;
    let mut r = quick_xml::Reader::from_str(text);
    let mut out = Xml::default();
    let mut folders: Vec<String> = Vec::new();
    let mut open_playlist = false;
    loop {
        let event = r.read_event().expect("well-formed XML");
        let (e, empty) = match &event {
            Event::Start(e) => (e.clone(), false),
            Event::Empty(e) => (e.clone(), true),
            Event::End(e) if e.name().as_ref() == b"NODE" => {
                if open_playlist {
                    open_playlist = false;
                } else {
                    folders.pop();
                }
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let attr = |name: &str| {
            e.attributes()
                .flatten()
                .find(|a| a.key.as_ref() == name.as_bytes())
                .map(|a| a.unescape_value().unwrap().into_owned())
        };
        match e.name().as_ref() {
            b"COLLECTION" => out.entries = attr("Entries").unwrap().parse().unwrap(),
            b"TRACK" if attr("Key").is_some() => {
                out.playlists.last_mut().unwrap().1.push(attr("Key").unwrap().parse().unwrap())
            }
            b"TRACK" => {
                let id = attr("TrackID").unwrap().parse().unwrap();
                out.tracks.insert(id, (attr("Name").unwrap(), attr("Artist").unwrap(), attr("Location").unwrap()));
            }
            b"NODE" => {
                let name = attr("Name").unwrap();
                match attr("Type").as_deref() {
                    Some("1") => {
                        let path = folders.iter().skip(1).cloned().chain([name]).collect::<Vec<_>>().join("/");
                        out.playlists.push((path, Vec::new()));
                        open_playlist = !empty;
                    }
                    _ if !empty => folders.push(name),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

fn unpercent(s: &str) -> String {
    let mut bytes = Vec::new();
    let mut rest = s.as_bytes();
    while let Some((&b, tail)) = rest.split_first() {
        if b == b'%' {
            bytes.push(u8::from_str_radix(std::str::from_utf8(&tail[..2]).unwrap(), 16).unwrap());
            rest = &tail[2..];
        } else {
            bytes.push(b);
            rest = tail;
        }
    }
    String::from_utf8(bytes).unwrap()
}

/// The XML lists every track of the drive's whole library (OneLibrary's too) with its details
/// and the file where rekordbox will find it, and every playlist with its entries in order;
/// with `only`, exactly the chosen playlists and the tracks they hold.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn xml_holds_the_drives_library(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    // rekordbox's demo export has beat grids and cues for the XML to carry.
    let root = match tc.draw(gs::weighted_booleans(0.3)) {
        true => common::demo_drive(dir.path(), "A"),
        false => drifted_drive(&tc, dir.path()),
    };
    let d = Device::open_export(&root).unwrap();
    let lib = convert::combined_library(&d).unwrap();
    let playlists: Vec<String> = lib.playlists.iter().map(|p| lib.playlist_path(p.id)).collect();
    let only: Vec<String> = drawn::some(&tc, &playlists).into_iter().take(2).collect();
    let location = tc.draw(gs::optional(gs::sampled_from(vec!["/Music", "/Volumes/Old USB"])));
    let opts = XmlOptions {
        location_root: location.map(Into::into),
        skip_analysis: tc.draw(gs::booleans()),
        only: only.clone(),
    };
    if only.iter().any(|q| lib.find_playlist(q).is_none()) {
        // A name two playlists share picks neither.
        assert!(xml::export(&d, &opts, &Ctx::silent()).is_err());
        return;
    }
    let (text, report) = xml::export(&d, &opts, &Ctx::silent()).unwrap();
    let x = read_xml(&text);

    // Which playlists, and so which tracks, the XML should hold.
    let mut kept = BTreeSet::new();
    for q in &only {
        let p = lib.find_playlist(q).unwrap();
        let mut queue = vec![p.id];
        while let Some(id) = queue.pop() {
            if kept.insert(id) {
                queue.extend(lib.children(Some(id)).iter().map(|c| c.id));
            }
        }
    }
    let shown: Vec<_> =
        lib.playlists.iter().filter(|p| !p.is_folder && (only.is_empty() || kept.contains(&p.id))).collect();
    let used: BTreeSet<u32> = shown.iter().flat_map(|p| p.track_ids.iter().copied()).collect();
    let tracks: Vec<_> = lib.tracks.iter().filter(|t| only.is_empty() || used.contains(&t.id)).collect();

    assert_eq!((x.entries, x.tracks.len(), report.tracks), (tracks.len(), tracks.len(), tracks.len()));
    assert_eq!(report.playlists, shown.len());
    let base = location.map_or(d.root().to_path_buf(), Into::into);
    for t in &tracks {
        let (name, artist, loc) = &x.tracks[&t.id];
        assert_eq!((name, artist), (&t.title, &t.artist));
        let want = paths::to_fs(&base, &paths::key(&t.file_path)).to_string_lossy().into_owned();
        assert_eq!(unpercent(loc.strip_prefix("file://localhost").unwrap()), want);
    }
    let mut want: Vec<(String, Vec<u32>)> =
        shown.iter().map(|p| (lib.playlist_path(p.id), p.track_ids.clone())).collect();
    let mut got = x.playlists;
    want.sort();
    got.sort();
    assert_eq!(got, want);
    if opts.skip_analysis {
        assert_eq!((report.tracks_with_beatgrid, report.cues), (0, 0));
    }
    assert_eq!(text.matches("<POSITION_MARK").count(), report.cues);
}
