use super::*;
use crate::anlz::{Mood, Phrase};

/// `n` beats at 120 BPM from 1 s, the first `pickup` of them before bar 1.
fn grid(n: u32, pickup: u32) -> Vec<Beat> {
    (0..n)
        .map(|i| Beat { beat_number: ((i + 4 - pickup) % 4 + 1) as u16, bpm: 120.0, time_ms: 1000 + i * 500 })
        .collect()
}

fn song(phrases: &[(u16, PhraseKind)], end: u16) -> SongStructure {
    let mut out = Vec::new();
    for (i, (start, kind)) in phrases.iter().enumerate() {
        let end_beat = phrases.get(i + 1).map_or(end, |(s, _)| *s);
        out.push(Phrase { kind: *kind, start_beat: *start, end_beat, fill_beat: None });
    }
    SongStructure { mood: Mood::High, phrases: out }
}

/// A placed cue: its hot cue letter, beat, name and colour.
type Placed<'a> = (Option<char>, u32, Option<&'a str>, Option<&'a str>);

fn summary(cues: &[SetCue]) -> Vec<Placed<'_>> {
    cues.iter()
        .map(|c| {
            let CuePosition::Beat(b) = c.at else { panic!("placed by beat") };
            (c.hot_cue.map(HotCue::letter), b, c.name.as_deref(), c.color.as_deref())
        })
        .collect()
}

#[test]
fn the_first_cue_goes_on_the_first_downbeat() {
    // A one-beat pickup: bar 1 is beat 2.
    let cues = place(CueStyle::FirstCue, &grid(64, 1), None);
    assert_eq!(summary(&cues), [(None, 2, Some("1.1Bars"), None)]);
    assert!(place(CueStyle::Rekordbox, &[], None).is_empty(), "no grid, no cues");
}

#[test]
fn rekordbox_puts_a_hot_cue_where_each_new_phrase_starts() {
    use PhraseKind::*;
    let s = song(&[(1, Intro), (17, Up), (33, Chorus), (49, Chorus), (65, Down), (81, Outro)], 97);
    let cues = place(CueStyle::Rekordbox, &grid(96, 0), Some(&s));
    assert_eq!(
        summary(&cues),
        [
            (None, 1, Some("1.1Bars"), None),
            (Some('A'), 1, Some("Intro"), None),
            (Some('B'), 17, Some("Up"), None),
            // Two choruses running on are one.
            (Some('C'), 33, Some("Chorus"), None),
            (Some('D'), 65, Some("Down"), None),
            (Some('E'), 81, Some("Outro"), None),
        ]
    );
}

#[test]
fn rekordbox_stops_at_h() {
    let kinds = [PhraseKind::Up, PhraseKind::Chorus];
    let phrases: Vec<(u16, PhraseKind)> = (0..12).map(|i| (1 + i * 16, kinds[i as usize % 2])).collect();
    let cues = place(CueStyle::Rekordbox, &grid(192, 0), Some(&song(&phrases, 193)));
    assert_eq!(cues.iter().filter(|c| c.hot_cue.is_some()).count(), 8);
    assert_eq!(cues.last().unwrap().hot_cue, Some(HotCue(7)));
}

#[test]
fn intro_chorus_puts_a_on_the_intro_and_h_g_f_on_the_first_choruses() {
    use PhraseKind::*;
    let s = song(
        &[(1, Intro), (17, Chorus), (33, Up), (49, Chorus), (65, Down), (81, Chorus), (97, Up), (113, Chorus)],
        129,
    );
    let cues = place(CueStyle::IntroChorus, &grid(128, 0), Some(&s));
    assert_eq!(
        summary(&cues),
        [
            (None, 1, Some("1.1Bars"), None),
            (Some('A'), 1, None, Some("#28e214")),
            (Some('H'), 17, None, Some("#ff127b")),
            (Some('G'), 49, None, Some("#ff127b")),
            (Some('F'), 81, None, Some("#ff127b")),
        ]
    );
}

#[test]
fn phrases_counted_at_half_tempo_land_on_the_whole_grid() {
    use PhraseKind::*;
    // rekordbox counted these at half tempo: they end at beat 49 of a 96-beat grid.
    let s = song(&[(1, Intro), (17, Chorus)], 49);
    let cues = place(CueStyle::Rekordbox, &grid(96, 0), Some(&s));
    assert_eq!(summary(&cues)[2], (Some('B'), 33, Some("Chorus"), None));
}

#[test]
fn phrases_past_the_grid_are_left_out() {
    let s = song(&[(1, PhraseKind::Intro), (200, PhraseKind::Chorus)], 220);
    let cues = place(CueStyle::Rekordbox, &grid(32, 0), Some(&s));
    assert_eq!(cues.iter().filter(|c| c.hot_cue.is_some()).count(), 1);
}

#[test]
fn styles_read_back_by_name() {
    for s in CueStyle::ALL {
        assert_eq!(s.name().parse::<CueStyle>(), Ok(s));
        assert_eq!(serde_json::to_value(s).unwrap(), s.name());
    }
    assert!("nope".parse::<CueStyle>().is_err());
}
