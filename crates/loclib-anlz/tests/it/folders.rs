//! Where rekordbox puts a track's analysis files: the folders its exports in locallibrary's test
//! fixtures hold.

use loclib_anlz::AnalysisFolder;

/// The demo tracks' folders, a path with characters outside the BMP (hashed as surrogate pairs)
/// and two paths from the large export that hash alike, whose files rekordbox numbered in one
/// folder.
#[test]
fn folders_are_rekordbox_s() {
    for (path, folder) in [
        ("/Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3", "PIONEER/USBANLZ/P016/0000875E"),
        ("/Contents/Loopmasters/UnknownAlbum/Demo Track 2.mp3", "PIONEER/USBANLZ/P053/0001D21F"),
        (
            "/Contents/Far East Movement, The Cataracs, DEV/\u{1F480}\u{1F480}\u{1F480}/Like A G6 (Second Degree Remix).mp3",
            "PIONEER/USBANLZ/P01C/000042F2",
        ),
        (
            "/Contents/Andreas Gehm/The Worst of Gehm/9840607_My_So_Called_Robot_Life_Part_2_Origi.mp3",
            "PIONEER/USBANLZ/P05F/0001CAFF",
        ),
        ("/Contents/Miki craven/0 Kelvin EP/5510349_Rusty_Mountain_Original_Mix.aiff", "PIONEER/USBANLZ/P05F/0001CAFF"),
    ] {
        assert_eq!(AnalysisFolder::of(path).to_string(), folder, "{path}");
    }
}

/// A path hashed in parts is the same folder as the whole path, and the folder's numbers are
/// what its name says.
#[test]
fn a_path_in_parts_is_the_same_folder() {
    let key = "Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3";
    let folder = AnalysisFolder::of_parts(&["/", key]);
    assert_eq!(folder, AnalysisFolder::of(&format!("/{key}")));
    assert_eq!((folder.bucket(), folder.hash()), (0x16, 0x875E));
}

/// Paths that hash alike are one folder, in a set as anywhere, and its debug form says its
/// numbers.
#[test]
fn paths_that_hash_alike_are_one_folder() {
    let folders: std::collections::HashSet<AnalysisFolder> = [
        "/Contents/Andreas Gehm/The Worst of Gehm/9840607_My_So_Called_Robot_Life_Part_2_Origi.mp3",
        "/Contents/Miki craven/0 Kelvin EP/5510349_Rusty_Mountain_Original_Mix.aiff",
    ]
    .into_iter()
    .map(AnalysisFolder::of)
    .collect();
    assert_eq!(folders.len(), 1);
    let folder = folders.into_iter().next().unwrap();
    assert_eq!(format!("{folder:?}"), "AnalysisFolder { bucket: 95, hash: 117503 }");
}
