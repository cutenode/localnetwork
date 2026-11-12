use super::*;

/// A track's three analysis files are found on its drive and read, and what `load` picks for a
/// player is among their waveforms. A track without files, or whose files are gone, has none.
#[test]
fn a_tracks_waveforms_are_read_from_its_drive() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rekordcrate/demo_tracks");
    let device = Device::open_export(&root).unwrap();
    let track = device.library().unwrap().tracks.remove(0);
    let w = load_waves(&device, &track);
    assert!(w.blue_preview.is_some() && w.rgb_detail.is_some() && w.band_detail.is_some(), "the .DAT, .EXT and .2EX");
    let a = load(&device, &track);
    assert_eq!(a.detail.as_ref(), w.rgb_detail.as_ref());
    assert_eq!(a.preview.as_ref(), w.rgb_preview.as_ref());

    let none = Track { analyze_path: String::new(), ..track.clone() };
    assert_eq!(load_waves(&device, &none), TrackWaves::default());
    let gone = Track { analyze_path: "/PIONEER/USBANLZ/nothing/ANLZ0000.DAT".into(), ..track };
    assert_eq!(load_waves(&device, &gone), TrackWaves::default());
}
