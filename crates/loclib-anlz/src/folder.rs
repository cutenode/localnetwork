//! Where rekordbox puts a track's analysis files on a drive.

/// The folder rekordbox gives the analysis files of the audio file at `file_path`, as `export.pdb`
/// stores it (`/Contents/...`): `PIONEER/USBANLZ/Pxxx/xxxxxxxx`, relative to the drive's root.
///
/// It isn't free to choose. An XDJ-RX3 looks for a track's analysis in this folder, whatever
/// path the track's `export.pdb` row gives, and finds none anywhere else: a track whose files are
/// elsewhere loads with no waveforms and no beat grid.
///
/// The folder is a hash of the path's UTF-16 code units (surrogate pairs as two):
///
/// 1. Each code unit `c` goes in as `h = (h * 0x5BC9 + c) * 0x93B5 + c`, wrapping at 32 bits,
///    from `h = 0`.
/// 2. `r = h % 200003` is the second folder, as eight hex digits.
/// 3. `Pxxx` is seven of `r`'s bits, from bit 0, 2, 6, 7, 9, 13 and 16 in turn, as three hex
///    digits.
///
/// That places every analysed track of three rekordbox exports: 2,244 of 2,244 on a rekordbox
/// 7.2.14 drive, and all 3,886 and 2 of the `large` and `demo_tracks` exports in locallibrary's
/// test fixtures. Two paths can hash alike; rekordbox then numbers the second track's files
/// `ANLZ0001.DAT` and up in the same folder.
pub fn analysis_folder(file_path: &str) -> String {
    let h = file_path
        .encode_utf16()
        .map(u32::from)
        .fold(0u32, |h, c| h.wrapping_mul(0x5BC9).wrapping_add(c).wrapping_mul(0x93B5).wrapping_add(c));
    let r = h % 200_003;
    let p = [0, 2, 6, 7, 9, 13, 16].iter().enumerate().fold(0, |p, (i, bit)| p | ((r >> bit) & 1) << i);
    format!("PIONEER/USBANLZ/P{p:03X}/{r:08X}")
}
