//! Where a merge puts the source's folders and the playlists in them.

use crate::common;

use std::path::{Path, PathBuf};

use locallibrary::device::Device;
use locallibrary::merge::{self, MergeOptions, PlaylistMode};
use locallibrary::progress::Ctx;
use locallibrary::{Library, paths};

/// The twelve-track drive with a folder "Shows" holding "Friday" (its first two tracks).
fn with_shows(dir: &Path) -> PathBuf {
    let root = common::bigger_drive(dir, "Src");
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    let shows = ed.create_playlist("Shows", 0, true).unwrap();
    let friday = ed.create_playlist("Friday", shows, false).unwrap();
    ed.set_playlist_entries(friday, &[1, 2]).unwrap();
    ed.save(&pdb_path).unwrap();
    root
}

/// Merge `src` into a new demo drive: the target's playlists by path, and Friday's titles.
fn merged(src: &Path, name: &str, opts: &MergeOptions) -> (Vec<String>, Vec<String>, String) {
    let dir = src.parent().unwrap();
    let (src, mut dst) =
        (Device::open_export(src).unwrap(), Device::open_export(common::demo_drive(dir, name)).unwrap());
    let plan = merge::plan(&src, &dst, opts, &Ctx::silent()).unwrap();
    merge::apply(&plan, &src, &mut dst, opts, &Ctx::silent()).unwrap();
    let lib = dst.library().unwrap();
    let mut lists: Vec<String> = lib.playlists.iter().map(|p| lib.playlist_path(p.id)).collect();
    lists.sort();
    let friday = lib.playlists.iter().find(|p| p.name == "Friday").unwrap();
    let titles = |lib: &Library| friday.track_ids.iter().map(|id| lib.track(*id).unwrap().title.clone()).collect();
    (lists, titles(&lib), lib.playlist_path(friday.id))
}

#[test]
fn folders_are_recreated_in_the_merge_folder_or_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let src = with_shows(dir.path());
    let lib = locallibrary::pdb::read_library(&src.join(paths::EXPORT_PDB)).unwrap();
    let want: Vec<String> = [1, 2].iter().map(|id| lib.track(*id).unwrap().title.clone()).collect();

    let (lists, titles, friday) = merged(&src, "Folder", &MergeOptions::default());
    assert_eq!(lists, ["Src", "Src/Shows", "Src/Shows/Friday", "Src/aab"]);
    assert_eq!((titles, friday.as_str()), (want.clone(), "Src/Shows/Friday"));

    // A playlist chosen without its folder goes straight into the merge folder.
    let only = MergeOptions { only_playlists: vec!["Shows/Friday".into()], ..Default::default() };
    let (lists, titles, _) = merged(&src, "Chosen", &only);
    assert_eq!(lists, ["Src", "Src/Friday"]);
    assert_eq!(titles, want);

    // By name, the folder the target lacks is made.
    let by_name = MergeOptions { playlists: PlaylistMode::ByName, ..Default::default() };
    let (lists, titles, _) = merged(&src, "ByName", &by_name);
    assert_eq!(lists, ["Shows", "Shows/Friday", "aab"]);
    assert_eq!(titles, want);
}

/// Hot cue bank lists come along inside the merge folder, with the folders above them.
#[test]
fn hot_cue_bank_lists_keep_their_folders() {
    use locallibrary::onelibrary::OneLibrary;
    let dir = tempfile::tempdir().unwrap();
    let src_root = common::demo_drive(dir.path(), "Guest");
    common::add_onelibrary(&src_root);
    let one = OneLibrary::open_root(&src_root).unwrap();
    one.connection()
        .execute_batch(
            "INSERT INTO cue (cue_id, content_id, kind, inUsec) VALUES (5, 1, 0, 2000000);
             INSERT INTO hotCueBankList VALUES (1, 0, 'Gigs', NULL, 1, 0), (2, 0, 'Bank', NULL, 0, 1);
             INSERT INTO hotCueBankList_cue VALUES (2, 5, 1);",
        )
        .unwrap();
    std::fs::write(src_root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    let dst_root = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&dst_root);
    let (src, mut dst) = (Device::open_export(&src_root).unwrap(), Device::open_export(&dst_root).unwrap());

    let plan = merge::plan(&src, &dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    let report = merge::apply(&plan, &src, &mut dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(report.hot_cue_bank_lists_added, 1);
    let lists = OneLibrary::open(&dst).unwrap().hot_cue_bank_lists().unwrap();
    let path = |name: &str| -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = lists.iter().find(|l| l.name == name);
        while let Some(l) = cur {
            out.insert(0, l.name.clone());
            cur = l.parent_id.and_then(|p| lists.iter().find(|x| x.id == p));
        }
        out
    };
    assert_eq!(path("Bank"), ["Guest", "Gigs", "Bank"]);
    assert_eq!(lists.iter().find(|l| l.name == "Bank").unwrap().track_ids.len(), 1);
}

/// A hot cue bank list whose folder is missing is shown at the top, so it comes along at the top
/// of the merge folder rather than being left behind.
#[test]
fn a_hot_cue_bank_list_whose_folder_is_missing_comes_along() {
    use locallibrary::onelibrary::OneLibrary;
    let dir = tempfile::tempdir().unwrap();
    let src_root = common::demo_drive(dir.path(), "Guest");
    common::add_onelibrary(&src_root);
    let one = OneLibrary::open_root(&src_root).unwrap();
    one.connection()
        .execute_batch(
            "INSERT INTO cue (cue_id, content_id, kind, inUsec) VALUES (5, 1, 0, 2000000);
             INSERT INTO hotCueBankList VALUES (3, 0, 'Lost', NULL, 0, 9);
             INSERT INTO hotCueBankList_cue VALUES (3, 5, 1);",
        )
        .unwrap();
    std::fs::write(src_root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    let dst_root = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&dst_root);
    let (src, mut dst) = (Device::open_export(&src_root).unwrap(), Device::open_export(&dst_root).unwrap());

    let plan = merge::plan(&src, &dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    let report = merge::apply(&plan, &src, &mut dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(report.hot_cue_bank_lists_added, 1);
    let lists = OneLibrary::open(&dst).unwrap().hot_cue_bank_lists().unwrap();
    let lost = lists.iter().find(|l| l.name == "Lost").expect("Lost was merged");
    let guest = lists.iter().find(|l| l.name == "Guest").unwrap();
    assert_eq!((lost.parent_id, lost.track_ids.len()), (Some(guest.id), 1));
}
