//! Where rekordbox puts a track's analysis files: the folders its exports in locallibrary's test
//! fixtures hold.

use loclib_anlz::analysis_folder;

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
        assert_eq!(analysis_folder(path), folder, "{path}");
    }
}
