use super::*;

#[test]
fn only_places_in_the_music_folder_are_looked_for() {
    assert!(in_music_folder("/OliverTree/UnknownAlbum/026EE51D_Lizard (Once).mp3"));
    for outside in
        ["/../../etc/x", "/a/../b", "/a/b", "/a/b/c/d", "a/b/c", "/C:/b/c", r"/a\..\b/c/d", "/a//c", "/./b/c"]
    {
        assert!(!in_music_folder(outside), "{outside}");
    }
}
