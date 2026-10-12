use super::*;

/// A tentative match says so in the terminal's tracklist too.
#[test]
fn tentative_plays_say_so() {
    let pdb = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/locallibrary/tests/fixtures/rekordcrate/demo_tracks/PIONEER/rekordbox/export.pdb");
    let track = locallibrary::pdb::read_library(&pdb).unwrap().tracks.remove(0);
    let mut p = Play {
        track,
        start: 0.0,
        end: 60.0,
        track_start: 0.0,
        track_end: 60.0,
        tempo: 1.0,
        played_bpm: None,
        key_lock: None,
        semitones: 0.0,
        key: None,
        played_key: None,
        confidence: 0.9,
        votes: 100,
        presence: Vec::new(),
        presence_step: 2.0,
        jumps: 0,
        alternates: Vec::new(),
    };
    assert!(!played(&p).contains("tentative"), "{}", played(&p));
    p.confidence = 0.42;
    assert!(played(&p).ends_with(" · tentative (42%)"), "{}", played(&p));
}
