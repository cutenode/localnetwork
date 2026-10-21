//! The check before a set, against drives with faults drawn into them: files taken away, an
//! interrupted operation, a OneLibrary that drifted from export.pdb, a copy that moved on, and
//! player models to check against. The report names exactly the faults put in, and nothing else.

use crate::{common, drawn};

use rustc_hash::FxHashSet;
use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::check::{self, CheckOptions, CheckReport, Level};
use locallibrary::device::{Device, LineageOp};
use locallibrary::equipment::{self, Codec, MODELS};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::{Library, paths};
use rekordbox_pdb::PdbEditor;

/// Every check, in the order the report gives a drive's items.
const ORDER: &[&str] = &[
    "export",
    "same_library",
    "databases",
    "files",
    "audio",
    "loudness",
    "large_files",
    "interrupted",
    "history",
    "file_system",
    "players",
    "formats",
];

fn items<'a>(r: &'a CheckReport, drive: &str, check: &str) -> Vec<(Level, &'a str)> {
    r.items.iter().filter(|i| i.drive == drive && i.check == check).map(|i| (i.level, i.message.as_str())).collect()
}

/// The number of distinct audio files, as the report counts tracks, among `tracks`.
fn files_of(lib: &Library, ids: &FxHashSet<u32>) -> usize {
    lib.tracks.iter().filter(|t| ids.contains(&t.id)).map(|t| t.file_path.clone()).collect::<FxHashSet<_>>().len()
}

#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn the_check_names_exactly_the_faults_put_in(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let (a, _) = drawn::drive(&tc, dir.path(), "A");
    // Two more tracks in drawn formats, for the players to take or not.
    for n in 5..7 {
        drawn::track(&tc, &a, n);
    }
    let lib = locallibrary::pdb::read_library(&a.join(paths::EXPORT_PDB)).unwrap();
    let plus = tc.draw(gs::booleans());
    if plus {
        common::add_onelibrary(&a);
    }
    let pulled = tc.draw(gs::booleans());
    if pulled {
        sync::pull(&store, &mut Device::open_export(&a).unwrap(), &PullOptions::default(), &Ctx::silent()).unwrap();
    }

    // Faults: audio and analysis files taken away, OneLibrary left behind by an export.pdb edit.
    let ids: Vec<u32> = lib.tracks.iter().map(|t| t.id).collect();
    let no_audio: FxHashSet<u32> = drawn::some(&tc, &ids).into_iter().take(3).collect();
    let no_analysis: FxHashSet<u32> =
        drawn::some(&tc, &ids).into_iter().filter(|id| !no_audio.contains(id)).take(3).collect();
    for t in &lib.tracks {
        if no_audio.contains(&t.id) {
            let _ = fs::remove_file(paths::to_fs(&a, &paths::key(&t.file_path)));
        }
        if no_analysis.contains(&t.id) {
            let _ = fs::remove_file(paths::to_fs(&a, &paths::key(&t.analyze_path)));
        }
    }
    let drifted = plus && tc.draw(gs::booleans());
    if drifted {
        let mut ed = PdbEditor::from_file(a.join(paths::EXPORT_PDB)).unwrap();
        ed.create_playlist("Only in export.pdb", 0, false).unwrap();
        ed.save(a.join(paths::EXPORT_PDB)).unwrap();
    }
    // A copy of A as it is now, which may then get a new export of its own.
    let copied = tc.draw(gs::booleans());
    let moved_on = copied && tc.draw(gs::booleans());
    let b = dir.path().join("B");
    if copied {
        common::copy_dir(&a, &b);
        // As a fresh drive: locallibrary's own folder would give it A's name and id.
        let _ = fs::remove_dir_all(b.join(paths::STATE_DIR));
        if moved_on {
            common::export_new_track(&b, "New", "Export");
        }
    }
    let interrupted = tc.draw(gs::booleans());
    if interrupted {
        Device::open(&a).unwrap().begin(LineageOp::Sync, "sync from B", None).unwrap();
    }
    let mut given: Vec<String> = drawn::some(&tc, MODELS).into_iter().take(3).map(|m| m.name.to_string()).collect();
    if tc.draw(gs::weighted_booleans(0.2)) {
        given.push("XDJ-9999".into());
    }
    let audio = tc.draw(gs::weighted_booleans(0.3));

    let mut devices = vec![Device::open_export(&a).unwrap()];
    if copied {
        devices.push(Device::open_export(&b).unwrap());
    }
    let before: Vec<common::Files> = devices.iter().map(|d| common::drive_files(d.root())).collect();
    let opts = CheckOptions { players: given.clone(), audio };
    let r = check::check(&devices, Some(&store), &opts, &Ctx::silent()).unwrap();

    // The tallies and the order.
    let names: Vec<String> = devices.iter().map(|d| d.name()).collect();
    assert_eq!(r.drives, names);
    assert_eq!(r.problems, r.items.iter().filter(|i| i.level == Level::Problem).count());
    assert_eq!(r.warnings, r.items.iter().filter(|i| i.level == Level::Warning).count());
    assert_eq!(r.ok(), r.problems == 0);
    let order: Vec<(usize, usize)> = r
        .items
        .iter()
        .map(|i| (names.iter().position(|n| *n == i.drive).unwrap(), ORDER.iter().position(|c| *c == i.check).unwrap()))
        .collect();
    assert!(order.is_sorted(), "items go by drive, then check: {:#?}", r.items);

    for d in &devices {
        let name = d.name();
        // Files: one item per kind of fault, naming how many tracks have it.
        let files = items(&r, &name, "files");
        let missing_audio = files_of(&lib, &no_audio);
        let missing_analysis = files_of(&lib, &no_analysis);
        let said = |level: Level, what: &str, n: usize| {
            files.iter().any(|(l, m)| *l == level && m.starts_with(&format!("{n} track(s) {what}")))
        };
        assert_eq!(said(Level::Problem, "have no audio file", missing_audio), missing_audio > 0, "{files:?}");
        assert_eq!(said(Level::Warning, "have no analysis file", missing_analysis), missing_analysis > 0, "{files:?}");
        assert_eq!(files.iter().any(|(l, _)| *l == Level::Info), missing_audio + missing_analysis == 0, "{files:?}");

        let databases = items(&r, &name, "databases");
        assert_eq!(databases.len(), 1);
        // B's new export went to export.pdb alone.
        let drifted = drifted || (name == "B" && moved_on && plus);
        assert_eq!(databases[0].0 == Level::Problem, drifted, "{databases:?}");

        let pending = items(&r, &name, "interrupted");
        let expect = if interrupted && name == names[0] { Level::Problem } else { Level::Info };
        assert_eq!(pending.iter().map(|(l, _)| *l).collect::<Vec<_>>(), [expect]);

        // History lists are safe only once they're in the store.
        let history = items(&r, &name, "history");
        let lists = !lib.history.is_empty();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].0 == Level::Info, !lists || pulled, "{history:?}");

        // Players: each one given, Ok exactly when it reads a database the drive has (the
        // temporary folder isn't a volume, so file systems aren't checked).
        let players = items(&r, &name, "players");
        let per_model = if given.is_empty() { usize::from(!plus) } else { given.len() };
        assert_eq!(players.len(), per_model, "{players:?}");
        for g in &given {
            let (level, message) = *players.iter().find(|(_, m)| m.starts_with(&format!("{g}:"))).unwrap();
            match equipment::find(g) {
                None => assert!(level == Level::Warning && message.contains("not a model"), "{message}"),
                Some(m) => {
                    let reads = m.is_mixer() || m.export_pdb.value || (m.onelibrary.value && plus);
                    assert_eq!(level == Level::Info, reads, "{message}");
                }
            }
        }

        // Tracks a player won't load: real tracks of the drive, each in one group per player,
        // and lossless ones can be packed as copies it plays.
        let groups: Vec<_> = r.unloadable.iter().filter(|u| u.drive == name).collect();
        let mut seen = FxHashSet::default();
        for g in &groups {
            assert!(given.contains(&g.player));
            let lossless = Codec::ALL.iter().any(|c| c.name() == g.format && c.lossless());
            assert_eq!(g.transcodable, lossless, "{g:?}");
            for t in &g.tracks {
                assert!(seen.insert((g.player.clone(), t.file_path.clone())), "{} twice for {}", t.file_path, g.player);
                if let (Some(rate), Some(track)) = (g.sample_rate, lib.tracks.iter().find(|x| x.id == t.id)) {
                    assert!(track.sample_rate == 0 || track.sample_rate == rate, "{g:?}");
                }
            }
            assert!(items(&r, &name, "formats").iter().any(|(l, m)| *l == g.level && *m == g.message));
        }

        if audio {
            assert!(!items(&r, &name, "audio").is_empty(), "every audio file is opened");
        }
    }

    if copied {
        let same = items(&r, "B", "same_library");
        assert_eq!(same.len(), 1);
        assert_eq!(same[0].0 == Level::Info, !moved_on, "{same:?}");
    }

    // Nothing on the drives changed, and checking again says the same.
    for (d, files) in devices.iter().zip(&before) {
        assert!(common::changed_files(files, &common::drive_files(d.root())).is_empty());
    }
    let again = check::check(&devices, Some(&store), &opts, &Ctx::silent()).unwrap();
    let flat = |r: &CheckReport| -> Vec<(String, String, Level, String)> {
        r.items.iter().map(|i| (i.drive.clone(), i.check.clone(), i.level, i.message.clone())).collect()
    };
    assert_eq!(flat(&again), flat(&r));
}

/// A folder with no export is one problem, and it stops there.
#[hegel::test(test_cases = 10)]
fn a_drive_without_an_export_is_one_problem(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::blank(dir.path(), "Empty");
    if tc.draw(gs::booleans()) {
        fs::create_dir_all(root.join("Contents")).unwrap();
        fs::write(root.join("Contents/loose.mp3"), common::placeholder_audio("loose")).unwrap();
    }
    let players: Vec<String> = drawn::some(&tc, MODELS).into_iter().take(2).map(|m| m.name.to_string()).collect();
    let d = Device::open(&root).unwrap();
    let r = check::check(&[d], None, &CheckOptions { players, audio: false }, &Ctx::silent()).unwrap();
    let export = items(&r, "Empty", "export");
    assert_eq!(export.len(), 1);
    assert_eq!(export[0].0, Level::Problem);
    assert!(!r.ok());
    assert!(r.unloadable.is_empty() && r.audio.is_empty());
}

/// Without models given, each drive is checked against those remembered for its library, else
/// the default ones; a drive packed from the first is up to date until the first changes.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn remembered_players_and_packs_are_checked_against(tc: TestCase) {
    use locallibrary::check::PlayersFrom;
    use locallibrary::equipment::Remembered;
    use locallibrary::pack::{self, PackOptions};
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let (a, lib) = drawn::drive(&tc, dir.path(), "A");
    let mut da = Device::open_export(&a).unwrap();
    da.adopt(Some("A")).unwrap();
    sync::pull(&store, &mut da, &PullOptions::default(), &Ctx::silent()).unwrap();
    let library = Device::open(&a).unwrap().state().unwrap().library_id.unwrap();
    let names: Vec<String> = MODELS.iter().map(|m| m.name.to_string()).collect();
    let own: Vec<String> = drawn::some(&tc, &names).into_iter().take(2).collect();
    let default: Vec<String> = drawn::some(&tc, &names).into_iter().take(2).collect();
    let mut remembered = Remembered { default: default.clone(), ..Default::default() };
    if !own.is_empty() {
        remembered.libraries.insert(library, own.clone());
    }
    store.save_player_models(&remembered).unwrap();

    // B: packed from A's playlist "Packed", which may then change.
    common::apply(
        &a,
        &[locallibrary::edit::Edit::SetTracks {
            playlist: "Packed".into(),
            tracks: locallibrary::query::TrackSelector::paths([lib.tracks[0].file_path.clone()]),
            sort: None,
            limit: None,
            create: true,
        }],
    );
    let b = common::blank(dir.path(), "B");
    let (src, mut club) = (Device::open_export(&a).unwrap(), Device::open(&b).unwrap());
    let opts = PackOptions { playlists: vec!["Packed".into()], ..Default::default() };
    let plan = pack::plan(&src, &club, &opts, &Ctx::silent()).unwrap();
    pack::apply(&plan, &src, &mut club, Some(&store), &Ctx::silent()).unwrap();
    let moved_on = tc.draw(gs::booleans());
    if moved_on {
        common::export_new_track(&a, "Moved", "On");
    }

    let devices = vec![Device::open_export(&a).unwrap(), Device::open_export(&b).unwrap()];
    let r = check::check(&devices, Some(&store), &CheckOptions::default(), &Ctx::silent()).unwrap();
    let p = &r.players[0];
    let (want, from) = match (own.is_empty(), default.is_empty()) {
        (false, _) => (own.clone(), PlayersFrom::Library),
        (true, false) => (default.clone(), PlayersFrom::Default),
        (true, true) => (Vec::new(), PlayersFrom::None),
    };
    assert_eq!((&p.models, p.from), (&want, from));
    assert_eq!(p.library.is_some(), from == PlayersFrom::Library);
    let packed = items(&r, &r.drives[1], "same_library");
    assert_eq!(packed.len(), 1);
    let (level, message) = packed[0];
    assert!(message.contains("packed from"), "{message}");
    assert_eq!(level == Level::Info, !moved_on, "{message}");
}
