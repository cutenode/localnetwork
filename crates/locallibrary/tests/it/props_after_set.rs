//! A drive back from a set while the library moved on at home, with the changes on each side
//! drawn: what players do (history lists, TAG LIST playlists, memory cues in analysis files) is
//! told apart from everything else, and the sync may go ahead only when exactly one drive's
//! changes are all the players'.

use crate::common;

use std::fs;
use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::after_set;
use locallibrary::device::Device;
use locallibrary::edit::Edit;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions, SyncOptions};
use locallibrary::{paths, pdb};

/// What was done to one drive since the two were last in sync.
#[derive(Debug, Default)]
struct Done {
    history: Vec<String>,
    tag_lists: Vec<String>,
    cue_files: usize,
    /// Changes no player makes.
    other: bool,
}

/// Draw changes on the drive at `root`, players' ones only unless `others`.
#[hegel::test_helper]
fn change(tc: &TestCase, root: &Path, side: &str, others: bool) -> Done {
    let mut done = Done::default();
    let lib = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    for n in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
        let name = format!("HISTORY {side}{n}");
        common::add_history(root, &name, &[lib.tracks[n].id]);
        done.history.push(name);
    }
    if tc.draw(gs::booleans()) {
        let name = format!("TAG LIST {side}");
        common::apply(root, &[Edit::CreatePlaylist { path: name.clone(), folder: false }]);
        done.tag_lists.push(name);
    }
    for t in lib.tracks.iter().take(tc.draw(gs::integers::<usize>().max_value(2))) {
        // A memory cue saved on the player rewrites the track's analysis file.
        let anlz = paths::to_fs(root, &paths::key(&t.analyze_path));
        let mut bytes = fs::read(&anlz).unwrap();
        bytes.extend_from_slice(b"PCOB");
        fs::write(&anlz, bytes).unwrap();
        done.cue_files += 1;
    }
    if others {
        match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => {
                common::export_new_track(root, "Fresh", side);
            }
            1 => {
                common::apply(root, &[Edit::CreatePlaylist { path: format!("Friday {side}"), folder: false }]);
            }
            _ => fs::write(paths::to_fs(root, &paths::key(&lib.tracks[0].file_path)), b"re-encoded").unwrap(),
        }
        done.other = true;
    }
    done
}

#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn only_a_drive_changed_the_way_players_do_is_back_from_a_set(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut main = Device::open(common::demo_drive(dir.path(), "Main")).unwrap();
    main.adopt(Some("Main")).unwrap();
    let club_root = common::blank(dir.path(), "Club");
    let mut club = Device::open(&club_root).unwrap();
    sync::sync_drive_to_drive(&mut main, &mut club, &SyncOptions::default(), &Ctx::silent()).unwrap();
    sync::pull(&store, &mut main, &PullOptions::default(), &Ctx::silent()).unwrap();

    let at_home = change(&tc, main.root(), "home", tc.draw(gs::booleans()));
    let at_set = change(&tc, &club_root, "set", tc.draw(gs::weighted_booleans(0.3)));
    let (main, club) = (Device::open(main.root()).unwrap(), Device::open(&club_root).unwrap());
    let common = after_set::common_snapshot(&store, &main, &club).unwrap().expect("both held what was pulled");
    let changes = [(&main, &at_home), (&club, &at_set)].map(|(d, done)| {
        let c = after_set::changes_since(&store, d, &common, &Ctx::silent()).unwrap();
        let mut history = c.history_lists.clone();
        history.sort();
        let mut want = done.history.clone();
        want.sort();
        assert_eq!(history, want, "{}", d.name());
        assert_eq!(c.tag_lists, done.tag_lists);
        assert_eq!(c.cue_files, done.cue_files);
        assert_eq!(c.other.is_empty(), !done.other, "{:?}", c.other);
        let players = !done.history.is_empty() || !done.tag_lists.is_empty() || done.cue_files > 0;
        assert_eq!(c.player_only(), players && !done.other);
        assert_eq!(c.is_empty(), !players && !done.other);
        c
    });

    let left_first = tc.draw(gs::booleans());
    let (left, right) = if left_first { (&main, &club) } else { (&club, &main) };
    let resolved = after_set::resolve(&store, left, right, &Ctx::silent()).unwrap();
    let [home, set] = &changes;
    let club_back = set.player_only() && !home.is_empty() && !home.player_only();
    let main_back = home.player_only() && !set.is_empty() && !set.player_only();
    match resolved {
        None => assert!(!club_back && !main_back),
        Some(r) => {
            let back = if r.left_is_back_from_set == left_first { "Main" } else { "Club" };
            assert_eq!(back == "Club", club_back, "{r:?}");
            assert_eq!(back == "Main", main_back, "{r:?}");
            assert_eq!(r.back_from_set.drive, back);
            assert_eq!(r.common.id, common.id);
        }
    }
}
