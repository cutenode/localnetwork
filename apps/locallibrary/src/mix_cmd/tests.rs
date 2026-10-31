use super::*;

/// A tentative match says so in the terminal's tracklist too, and so does a ridden pitch fader.
#[test]
fn tentative_and_ridden_plays_say_so() {
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
        speed_range: None,
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
    assert!(!played(&p).contains("ridden"), "{}", played(&p));
    p.speed_range = Some((1.01, 1.034));
    assert!(played(&p).ends_with(" · ridden +1.0% to +3.4% · tentative (42%)"), "{}", played(&p));
}
