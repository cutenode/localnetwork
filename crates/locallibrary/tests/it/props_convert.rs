//! Converting between the two databases, and exporting rekordbox XML, from drawn drives whose
//! databases may have drifted apart either way. Adding keeps everything both sides had,
//! replacing makes the target the source exactly, a conversion run again has nothing to add, and
//! undoing it gives the drive back. The XML holds every track and playlist the drive does.

use crate::common::{self, XmlOptions};
use crate::drawn;

use indexmap::IndexMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::convert::{self, ConvertOptions, Target};
use locallibrary::device::{Device, LibraryDrift};
use locallibrary::onelibrary::OneLibrary;
use locallibrary::progress::Ctx;
use locallibrary::{Library, backup, paths, pdb};
use rekordbox_pdb::PdbEditor;

/// What a database holds, comparable across databases: track files, every playlist's entries
/// (as files, by folded path; same-named playlists pooled) and history list names.
#[derive(Debug, Default)]
struct Held {
    tracks: BTreeSet<String>,
    playlists: IndexMap<String, BTreeSet<String>>,
    history: BTreeSet<String>,
}

fn held(lib: &Library) -> Held {
    let file = |id: &u32| lib.track(*id).map(|t| paths::file_key(&t.file_path));
    let mut h =
        Held { tracks: lib.tracks.iter().map(|t| paths::file_key(&t.file_path)).collect(), ..Default::default() };
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
        (Some(_), Some(_)) => tc.draw(gs::sampled_from(vec![Target::OneLibrary, Target::DeviceLibrary])),
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
        let m = made.tracks.iter().find(|x| paths::file_key(&x.file_path) == paths::file_key(&t.file_path)).unwrap();
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
            Event::End(e) if e.name().as_ref() == "NODE" => {
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
                .find(|a| a.key.as_ref() == name)
                .map(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).unwrap().into_owned())
        };
        match e.name().as_ref() {
            "COLLECTION" => out.entries = attr("Entries").unwrap().parse().unwrap(),
            "TRACK" if attr("Key").is_some() => {
                out.playlists.last_mut().unwrap().1.push(attr("Key").unwrap().parse().unwrap())
            }
            "TRACK" => {
                let id = attr("TrackID").unwrap().parse().unwrap();
                out.tracks.insert(id, (attr("Name").unwrap(), attr("Artist").unwrap(), attr("Location").unwrap()));
            }
            "NODE" => {
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
        assert!(common::rekordbox_xml(&d, &opts).is_err());
        return;
    }
    let (text, report) = common::rekordbox_xml(&d, &opts).unwrap();
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
        // A URL's path: `/`-separated, and on Windows `/C:/...`.
        let want = paths::to_fs(&base, &paths::key(&t.file_path)).to_string_lossy().replace('\\', "/");
        let want = if want.starts_with('/') { want } else { format!("/{want}") };
        assert_eq!(unpercent(loc.strip_prefix("file://localhost").unwrap()), want);
    }
    let mut want: Vec<(String, Vec<u32>)> =
        shown.iter().map(|p| (lib.playlist_path(p.id), p.track_ids.clone())).collect();
    let mut got = x.playlists;
    want.sort();
    got.sort();
    assert_eq!(got, want);
    if opts.skip_analysis {
        assert_eq!((report.beat_grids, report.cues), (0, 0));
    }
    assert_eq!(text.matches("<POSITION_MARK").count(), report.cues);
}

/// Give OneLibrary the categories and tags exportExt.pdb has, as rekordbox exports both.
fn mirror_tags(root: &Path) {
    let ext = rekordbox_pdb::ExtDatabase::from_file(root.join(paths::EXPORT_EXT_PDB)).unwrap();
    let mut one = OneLibrary::open_root(root).unwrap();
    for c in ext.categories() {
        one.add_my_tag(c.id.into(), 0, &c.name, true).unwrap();
        for t in ext.tags_in(c.id) {
            one.add_my_tag(t.id.into(), c.id.into(), &t.name, false).unwrap();
        }
    }
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
}

/// The files of tracks carrying each My Tag, by path, in the database `source` names.
fn tagged(d: &Device, source: locallibrary::tags::Source) -> BTreeSet<(String, String)> {
    let mine = locallibrary::tags::read(d).unwrap();
    mine.categories
        .iter()
        .flat_map(|c| &c.tags)
        .filter(|t| t.sources.contains(&source))
        .flat_map(|t| t.tracks.iter().map(move |r| (t.path.clone(), paths::file_key(&r.file_path))))
        .collect()
}

/// Conversions [`converting_keeps_both_sides_and_undoes_exactly`] doesn't draw: rekordbox's demo
/// export (real analysis files, its My Tags carried across), a track or playlist only one side
/// has, hot cue bank lists a rebuild drops, artwork missing on the drive, and a target that can't
/// be read (added to, it's refused with a way on; rebuilt, it's made from scratch).
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn converting_carries_my_tags_and_says_what_it_leaves(tc: TestCase) {
    for variant in 0..4 {
        converting_carries_my_tags_and_says_what_it_leaves_with(&tc, variant);
    }
}

/// One case of [`converting_carries_my_tags_and_says_what_it_leaves`], `variant` choosing what's drawn.
#[hegel::test_helper]
fn converting_carries_my_tags_and_says_what_it_leaves_with(tc: &TestCase, variant: u8) {
    use locallibrary::onelibrary::ContentRecord;
    use locallibrary::tags::{self, Source};
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    let mut d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    let ids: Vec<u32> = lib.tracks.iter().map(|t| t.id).collect();
    let tag_some = drawn::some(tc, &ids);
    if !tag_some.is_empty() {
        let plan = tags::plan(&d, &tags::Edit::Add { tag: "Genre/Deep".into(), tracks: tag_some }).unwrap();
        tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    }
    let art = tc.draw(gs::booleans());
    if art {
        common::media::set_art(&root, &[ids[0]]);
    }
    common::add_onelibrary(&root);
    mirror_tags(&root);
    let mut one = OneLibrary::open_root(&root).unwrap();
    // A track only OneLibrary has (a player's own import, say), a hot cue bank list.
    if tc.draw(gs::booleans()) {
        let t = &lib.tracks[0];
        let file = "/Contents/Plus/Only.mp3";
        fs::create_dir_all(root.join("Contents/Plus")).unwrap();
        fs::write(root.join("Contents/Plus/Only.mp3"), common::placeholder_audio(file)).unwrap();
        let rec = ContentRecord::from_track(t, false, &one.content_defaults().unwrap());
        one.add_track(&rec, file, "", None).unwrap();
    }
    let banks = tc.draw(gs::booleans());
    if banks {
        let first = one.library().unwrap().tracks[0].id;
        one.connection()
            .execute("INSERT INTO cue (cue_id, content_id, kind, inUsec) VALUES (900, ?1, 0, 2000000)", [first])
            .unwrap();
        let list = one.create_hot_cue_bank_list("Bank", 0, false).unwrap();
        one.add_to_hot_cue_bank_list(list, 900).unwrap();
    }
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    // A playlist only export.pdb has.
    if tc.draw(gs::booleans()) {
        let mut ed = PdbEditor::from_file(root.join(paths::EXPORT_PDB)).unwrap();
        let p = ed.create_playlist("Only Pdb", 0, false).unwrap();
        ed.add_to_playlist(p, ids[0], None).unwrap();
        ed.save(root.join(paths::EXPORT_PDB)).unwrap();
    }
    // The artwork file gone from the drive.
    let art_gone = art && tc.draw(gs::booleans());
    if art_gone {
        let path = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap().tracks[0].artwork_path.clone().unwrap();
        fs::remove_file(paths::to_fs(&root, &paths::key(&path))).unwrap();
    }
    let to = tc.draw(gs::sampled_from(vec![Target::OneLibrary, Target::DeviceLibrary]));
    let replace = variant & 2 != 0;
    let opts = ConvertOptions { to, replace };
    // At times the target can't be read.
    let unreadable = variant & 1 != 0;
    if unreadable {
        let key = match to {
            Target::OneLibrary => paths::LIBRARY_PLUS_DB,
            Target::DeviceLibrary => paths::EXPORT_PDB,
        };
        fs::write(root.join(key), b"not a database").unwrap();
    }
    let pdb_before = pdb::read_library(&root.join(paths::EXPORT_PDB)).ok();
    let plus_before = OneLibrary::open_root(&root).ok().and_then(|o| o.library().ok());
    let d = Device::open_export(&root).unwrap();
    let from_tags = match to {
        Target::OneLibrary => tagged(&d, Source::ExportExt),
        Target::DeviceLibrary => BTreeSet::new(),
    };

    let plan = match convert::plan(&d, &opts, &Ctx::silent()) {
        Err(e) => {
            assert!(unreadable && !replace, "{e}");
            assert!(e.to_string().contains("can't be read, so nothing can be added to it"), "{e}");
            let retry: Vec<_> = e.detail().actions.iter().map(|a| a.params.get("replace").cloned()).collect();
            assert_eq!(retry, [Some(serde_json::json!(true))]);
            return;
        }
        Ok(p) => p,
    };
    let says = |what: &str| plan.warnings.iter().any(|w| w.contains(what));
    assert_eq!(says("can't be read; it's rebuilt from scratch"), unreadable, "{:?}", plan.warnings);
    // Only tracks it converts need their artwork: added to, OneLibrary has them already.
    let converts_art = art_gone && to == Target::OneLibrary && replace;
    assert_eq!(says("artwork image(s) the tracks point at are missing"), converts_art, "{:?}", plan.warnings);
    if to == Target::OneLibrary && replace && !unreadable {
        assert_eq!(says("hot cue bank list(s) go too"), banks, "{:?}", plan.warnings);
    }
    let mut d = Device::open_export(&root).unwrap();
    let report = convert::apply(&plan, &mut d, &opts, &Ctx::silent()).unwrap();
    assert_eq!(report.tracks, plan.tracks);

    let d = Device::open_export(&root).unwrap();
    let (source, made) = match to {
        Target::OneLibrary => (pdb_before.unwrap(), common::plus_library(&root)),
        Target::DeviceLibrary => (plus_before.unwrap(), pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap()),
    };
    covers(&held(&made), &held(&source)).unwrap();
    // Converting to OneLibrary carries exportExt.pdb's My Tags; to export.pdb, exportExt.pdb
    // keeps the ones whose tracks it still has.
    if to == Target::OneLibrary {
        let got = tagged(&d, Source::OneLibrary);
        assert!(from_tags.is_subset(&got), "{from_tags:?} in {got:?}");
    }
    if to == Target::OneLibrary {
        common::sqlcipher_reads(&root);
    }
}

/// Each `TRACK` element's attributes, and its `POSITION_MARK`s' attributes, by `TrackID`.
type Marks = IndexMap<u32, (IndexMap<String, String>, Vec<IndexMap<String, String>>)>;

fn xml_marks(text: &str) -> Marks {
    use quick_xml::events::Event;
    let mut r = quick_xml::Reader::from_str(text);
    let mut out = Marks::new();
    let mut current = None;
    loop {
        let e = match r.read_event().expect("well-formed XML") {
            Event::Start(e) | Event::Empty(e) => e.into_owned(),
            Event::Eof => break,
            _ => continue,
        };
        let attrs: IndexMap<String, String> = e
            .attributes()
            .flatten()
            .map(|a| {
                (
                    a.key.as_ref().to_owned(),
                    a.normalized_value(quick_xml::XmlVersion::Implicit1_0).unwrap().into_owned(),
                )
            })
            .collect();
        match e.name().as_ref() {
            "TRACK" if attrs.contains_key("TrackID") => {
                let id = attrs["TrackID"].parse().unwrap();
                out.insert(id, (attrs, Vec::new()));
                current = Some(id);
            }
            "POSITION_MARK" => out[&current.unwrap()].1.push(attrs),
            _ => {}
        }
    }
    out
}

/// The XML gives each track the kind of file it is, its colour when it has one (each colour its
/// own), and its cues as the drive lists them: where each starts and a loop ends, which hot cue
/// (-1 for a memory cue), and a hot cue's colour.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn xml_tells_each_tracks_kind_colour_and_cues(tc: TestCase) {
    use locallibrary::cue_edit::{self, CueEdit, CuePosition, HotCue, SetCue};
    use locallibrary::edit::{Edit, TrackFields};
    use locallibrary::query::TrackSelector;
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    for n in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
        drawn::track(&tc, &root, n);
    }
    let lib = Device::open_export(&root).unwrap().library().unwrap();
    let colours = vec!["pink", "red", "orange", "yellow", "green", "aqua", "blue", "purple"];
    for t in &lib.tracks {
        if let Some(c) = tc.draw(gs::optional(gs::sampled_from(colours.clone()))) {
            let fields = TrackFields { color: Some(c.into()), ..Default::default() };
            common::apply(&root, &[Edit::EditTracks { tracks: TrackSelector::paths([t.file_path.clone()]), fields }]);
        }
    }
    // Cues on the demo export's two tracks, which have beat grids for loops.
    let mut d = Device::open_export(&root).unwrap();
    for id in [1, 2] {
        for _ in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
            let set = SetCue {
                hot_cue: tc.draw(gs::optional(gs::integers::<u8>().max_value(HotCue::COUNT - 1))).map(HotCue),
                at: CuePosition::Ms(tc.draw(gs::integers::<u32>().min_value(1000).max_value(60_000))),
                snap: false,
                loop_beats: tc.draw(gs::optional(gs::sampled_from(vec![1.0, 4.0]))),
                name: None,
                color: tc.draw(gs::optional(gs::sampled_from(vec!["red", "blue"]))).map(String::from),
            };
            if let Ok(plan) = cue_edit::plan(&d, &[id], &CueEdit::Set(set), &Ctx::silent()) {
                cue_edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
            }
        }
    }
    let d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    let (text, report) = common::rekordbox_xml(&d, &XmlOptions::default()).unwrap();
    let marks = xml_marks(&text);
    let mut hexes: IndexMap<String, String> = IndexMap::new();
    for t in &lib.tracks {
        let (attrs, got) = &marks[&t.id];
        let ext = t.file_path.rsplit('.').next().unwrap().to_ascii_lowercase();
        let kind = match ext.as_str() {
            "m4a" | "mp4" | "aac" => "M4A File",
            "wav" => "WAV File",
            "aif" | "aiff" => "AIFF File",
            "flac" => "FLAC File",
            _ => "MP3 File",
        };
        assert_eq!(attrs["Kind"], kind, "{}", t.file_path);
        match t.color {
            Some(c) => {
                let hex = attrs.get("Colour").expect("a colour").clone();
                let name = format!("{c:?}");
                assert_eq!(hexes.entry(name.clone()).or_insert_with(|| hex.clone()), &hex, "{name} is one colour");
            }
            None => assert!(!attrs.contains_key("Colour")),
        }
        let listed = &cue_edit::list(&d, &[t.id]).unwrap()[0].cues;
        assert_eq!(got.len(), listed.len(), "{}: {got:?}", t.file_path);
        let mut listed = listed.clone();
        listed.sort_by_key(|c| c.start_ms);
        let mut got = got.clone();
        got.sort_by(|a, b| a["Start"].parse::<f64>().unwrap().total_cmp(&b["Start"].parse().unwrap()));
        for (m, c) in got.iter().zip(&listed) {
            assert_eq!(m["Start"], format!("{:.3}", f64::from(c.start_ms) / 1000.0));
            assert_eq!(m.get("End").cloned(), c.end_ms.map(|e| format!("{:.3}", f64::from(e) / 1000.0)));
            assert_eq!(m["Type"], if c.end_ms.is_some() { "4" } else { "0" });
            let num = c.hot_cue.as_deref().map_or(-1, |h| i32::from(h.as_bytes()[0] - b'A'));
            assert_eq!(m["Num"], num.to_string());
            let rgb = c.color.as_deref().filter(|h| h.starts_with('#') && c.hot_cue.is_some()).map(|h| {
                let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap().to_string();
                (byte(1), byte(3), byte(5))
            });
            // A hot cue's palette colour is its colour in the XML too (one rekordbox stored without
            // a palette entry is listed with none).
            if rgb.is_some() {
                assert_eq!(m.get("Red").map(|r| (r.clone(), m["Green"].clone(), m["Blue"].clone())), rgb);
            }
            if c.hot_cue.is_none() {
                assert!(!m.contains_key("Red"), "a memory cue has no colour in the XML");
            }
        }
    }
    let distinct: BTreeSet<&String> = hexes.values().collect();
    assert_eq!(distinct.len(), hexes.len(), "each colour its own: {hexes:?}");
    assert_eq!(report.cues, marks.values().map(|(_, m)| m.len()).sum::<usize>());
}

/// A drive's whole library, both its databases put together: every track either lists (once,
/// by file), every playlist and folder either has with its entries, and every history list. What
/// a player added to OneLibrary alone (a track, a playlist in a folder, a history list) is there
/// beside what export.pdb has.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn the_whole_library_is_both_databases_put_together(tc: TestCase) {
    use locallibrary::onelibrary::ContentRecord;
    let dir = tempfile::tempdir().unwrap();
    let (root, _) = drawn::drive(&tc, dir.path(), "A");
    common::add_onelibrary(&root);
    let mut one = OneLibrary::open_root(&root).unwrap();
    let plus = one.library().unwrap();
    let ids: Vec<u32> = plus.tracks.iter().map(|t| t.id).collect();
    // A track only OneLibrary has.
    let mut entries = drawn::some(&tc, &ids);
    if tc.draw(gs::booleans()) {
        let file = "/Contents/Player/Imported.mp3";
        fs::create_dir_all(root.join("Contents/Player")).unwrap();
        fs::write(root.join("Contents/Player/Imported.mp3"), common::placeholder_audio(file)).unwrap();
        let rec = ContentRecord::from_track(&plus.tracks[0], false, &one.content_defaults().unwrap());
        entries.push(one.add_track(&rec, file, "", None).unwrap());
    }
    // A playlist only OneLibrary has (perhaps in a folder of its own), and a history list.
    let folder = tc.draw(gs::booleans()).then(|| one.create_playlist("Player Folder", 0, true).unwrap());
    let p = one.create_playlist("Made On The Player", folder.unwrap_or(0), false).unwrap();
    for id in &entries {
        one.add_to_playlist(p, *id).unwrap();
    }
    if tc.draw(gs::booleans()) {
        let h = one.create_history("HISTORY P01", 0, false).unwrap();
        one.add_to_history(h, ids[0]).unwrap();
    }
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();

    let (pdb, plus) = libraries(&root);
    let (pdb, plus) = (pdb.unwrap(), plus.unwrap());
    let whole = convert::combined_library(&Device::open_export(&root).unwrap()).unwrap();
    let (w, a, b) = (held(&whole), held(&pdb), held(&plus));
    assert_eq!(w.tracks, &a.tracks | &b.tracks, "every track once, by file");
    assert_eq!(whole.tracks.len(), w.tracks.len(), "no file twice");
    covers(&w, &a).unwrap();
    covers(&w, &b).unwrap();
    assert_eq!(w.history, &a.history | &b.history);
    let path = match folder {
        Some(_) => "player folder/made on the player",
        None => "made on the player",
    };
    assert_eq!(w.playlists[path], b.playlists[path]);
}
