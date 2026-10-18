use rodio::buffer::SamplesBuffer;

use super::*;

/// A stereo track at 1 kHz whose samples count up from 0: frame `f` is `[2f, 2f + 1]`.
fn counting(frames: u32) -> SamplesBuffer {
    let data: Vec<f32> = (0..frames * 2).map(|i| i as f32).collect();
    SamplesBuffer::new(ChannelCount::new(2).unwrap(), SampleRate::new(1000).unwrap(), data)
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// The frames `n` frames of playback play (by their left sample).
fn play<S: Source>(l: &mut Looper<S>, n: usize) -> Vec<u32> {
    (0..n * 2).filter_map(|_| l.next()).step_by(2).map(|s| s as u32 / 2).collect()
}

#[test]
fn loops_repeat_on_the_frame_and_play_on_once_exited() {
    let head = Arc::new(Playhead::default());
    let mut l = Looper::new(counting(100), head.clone());
    head.set_loop(Some((ms(10), ms(14))));
    assert_eq!(play(&mut l, 20), [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 10, 11, 12, 13, 10, 11]);
    assert!(l.replay.is_some(), "the second time round comes from memory");
    assert_eq!(head.position(), ms(12));
    // Exited: round to the loop's end, then on through it.
    head.set_loop(None);
    assert_eq!(play(&mut l, 5), [12, 13, 14, 15, 16]);
    assert_eq!(head.position(), ms(17));
}

#[test]
fn a_loop_set_inside_it_goes_back_by_seeking_then_from_memory() {
    let head = Arc::new(Playhead::default());
    let mut l = Looper::new(counting(100), head.clone());
    play(&mut l, 12);
    head.set_loop(Some((ms(10), ms(14))));
    assert_eq!(play(&mut l, 10), [12, 13, 10, 11, 12, 13, 10, 11, 12, 13]);
    assert!(l.replay.is_some());
    // Seeking inside the loop stays in memory; seeking out of it plays the track.
    l.try_seek(ms(11)).unwrap();
    assert_eq!(play(&mut l, 4), [11, 12, 13, 10]);
    head.set_loop(None);
    l.try_seek(ms(50)).unwrap();
    assert_eq!(play(&mut l, 3), [50, 51, 52]);
}

#[test]
fn halving_a_loop_keeps_the_playhead_as_far_round() {
    let head = Arc::new(Playhead::default());
    let mut l = Looper::new(counting(100), head.clone());
    head.set_loop(Some((ms(10), ms(18))));
    play(&mut l, 24); // twice round to frame 16, from memory
    assert_eq!(head.position(), ms(16));
    head.set_loop(Some((ms(10), ms(14))));
    assert_eq!(play(&mut l, 6), [12, 13, 10, 11, 12, 13]);
    // Doubled again at its end: on to the new end.
    head.set_loop(Some((ms(10), ms(18))));
    assert_eq!(play(&mut l, 10), [14, 15, 16, 17, 10, 11, 12, 13, 14, 15]);
    // Exited while keeping it: nothing more is kept past its end.
    head.set_loop(None);
    play(&mut l, 20);
    assert_eq!(l.kept.len(), 16);
}

#[test]
fn a_seek_that_keeps_time_adds_what_played_meanwhile() {
    let head = Arc::new(Playhead::default());
    let mut l = Looper::new(counting(100), head.clone());
    play(&mut l, 5);
    head.since.store(3, Ordering::Relaxed);
    l.try_seek(ms(40)).unwrap();
    assert_eq!(play(&mut l, 2), [42, 43]);
    // Only that seek: the next lands where it's asked.
    l.try_seek(ms(40)).unwrap();
    assert_eq!(play(&mut l, 1), [40]);
}

/// A 16-bit mono WAV file at `rate`: `n` samples of `value`.
pub fn wav(rate: u32, n: usize, value: i16) -> Vec<u8> {
    wav_of(rate, &vec![value; n])
}

/// A 16-bit mono WAV file at `rate` holding `samples`.
pub fn wav_of(rate: u32, samples: &[i16]) -> Vec<u8> {
    let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let mut wav = Vec::new();
    wav.extend(b"RIFF");
    wav.extend((36 + data.len() as u32).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    for (v, n) in [(16u32, 4), (1, 2), (1, 2), (rate, 4), (rate * 2, 4), (2, 2), (16, 2)] {
        wav.extend(&v.to_le_bytes()[..n]);
    }
    wav.extend(b"data");
    wav.extend((data.len() as u32).to_le_bytes());
    wav.extend(data);
    wav
}

/// A file that panics the decoder (a WAV saying 0 Hz) fails to open on the worker, rather
/// than taking the player down.
#[test]
fn a_file_that_crashes_the_decoder_fails_to_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zero.wav");
    std::fs::write(&path, wav(0, 100, 1)).unwrap();
    let err = open_file(&path).err().expect("no decoder for it");
    assert!(format!("{err:#}").contains("crashed"), "{err:#}");
}

/// An M4A (with no audio) whose ALAC setup says 0 Hz where its container says 8 kHz: the
/// decoder opens, and only asking it for the rate panics. That's asked on the worker too, so
/// the file fails to open there instead of panicking the UI thread when it starts playing.
#[test]
fn a_file_whose_decoder_says_0_hz_fails_to_open() {
    let atom = |kind: &[u8], body: &[u8]| [&(8 + body.len() as u32).to_be_bytes(), kind, body].concat();
    let full = |kind: &[u8], body: &[u8]| atom(kind, &[&[0; 4], body].concat());
    let be = |vs: &[u32]| vs.iter().flat_map(|v| v.to_be_bytes()).collect::<Vec<u8>>();
    let matrix = be(&[0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x4000_0000]);
    let mvhd = full(
        b"mvhd",
        &[be(&[0, 0, 1000, 0, 0x10000, 0x0100_0000, 0, 0]), matrix.clone(), vec![0; 24], be(&[2])].concat(),
    );
    let tkhd = full(b"tkhd", &[be(&[0, 0, 1, 0, 0, 0, 0, 0, 0x0100_0000]), matrix, be(&[0, 0])].concat());
    let mdhd = full(b"mdhd", &be(&[0, 0, 8000, 0, 0x55c4_0000]));
    let hdlr = full(b"hdlr", &[&be(&[0])[..], b"soun", &[0; 13]].concat());
    // Frame length, version, bit depth, rice parameters, 2 channels, run, frame and bit rate
    // limits, and the rate: 0.
    let cookie = be(&[4096, 16 << 16 | 40 << 8 | 10, 14 << 24 | 2 << 16 | 255, 0, 0, 0]);
    let entry = atom(b"alac", &[be(&[0, 1, 0, 0, 2 << 16 | 16, 0, 8000 << 16]), full(b"alac", &cookie)].concat());
    let stbl = [
        full(b"stsd", &[be(&[1]), entry].concat()),
        full(b"stts", &be(&[0])),
        full(b"stsc", &be(&[0])),
        full(b"stsz", &be(&[0, 0])),
        full(b"stco", &be(&[0])),
    ]
    .concat();
    let minf = atom(b"minf", &[full(b"smhd", &be(&[0])), atom(b"stbl", &stbl)].concat());
    let trak = atom(b"trak", &[tkhd, atom(b"mdia", &[mdhd, hdlr, minf].concat())].concat());
    let m4a = [atom(b"ftyp", b"M4A \0\0\0\0M4A isom"), atom(b"moov", &[mvhd, trak].concat()), atom(b"mdat", &[])];

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zero.m4a");
    std::fs::write(&path, m4a.concat()).unwrap();
    let err = open_file(&path).err().expect("no decoder for it");
    assert!(format!("{err:#}").contains("crashed"), "{err:#}");
}

/// A file opened to start further in (auto cue) plays from there, and says so.
#[test]
fn files_start_where_asked() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ramp.wav");
    std::fs::write(&path, wav_of(8000, &(0..800).collect::<Vec<i16>>())).unwrap();
    let (mut audio, mut out) = Audio::offline(8000);
    audio.set_volume(1.0);
    let mut file = open_file(&path).unwrap();
    file.start = Some(ms(50));
    audio.play(file, false);
    assert_eq!(audio.position(), ms(50));
    let first = out.next().unwrap();
    assert_eq!((first * 32768.0).round(), 400.0);
}

/// A track queued behind the playing one starts the moment it ends; one queued and dropped
/// never plays.
#[test]
fn queued_tracks_follow_without_a_gap() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b, c) = (dir.path().join("a.wav"), dir.path().join("b.wav"), dir.path().join("c.wav"));
    std::fs::write(&a, wav(8000, 800, 1000)).unwrap();
    std::fs::write(&b, wav(8000, 800, -2000)).unwrap();
    std::fs::write(&c, wav(16000, 800, 1000)).unwrap();
    let (mut audio, mut out) = Audio::offline(8000);
    audio.set_volume(1.0);
    assert!(audio.queue(open_file(&b).unwrap()).is_err(), "nothing playing");
    audio.play(open_file(&a).unwrap(), false);
    assert!(audio.queue(open_file(&c).unwrap()).is_err(), "another sample rate");
    assert!(audio.queue(open_file(&b).unwrap()).is_ok());
    assert!(!audio.advanced());
    let played: Vec<f32> = (&mut out).take(1000).collect();
    let near = |s: &[f32], v: f32| s.iter().all(|x| (x - v / 32768.0).abs() < 1e-3);
    assert!(near(&played[..800], 1000.0) && near(&played[800..], -2000.0), "straight on: {:?}", &played[795..805]);
    assert!(audio.advanced() && !audio.advanced());
    assert_eq!(audio.position(), ms(25));
    // Queued, then dropped: once b ends, nothing.
    assert!(audio.queue(open_file(&a).unwrap()).is_ok());
    audio.unqueue();
    let rest: Vec<f32> = (&mut out).take(800).collect();
    assert!(near(&rest[..600], -2000.0) && rest[600..].iter().all(|s| *s == 0.0));
    assert!(audio.finished() && !audio.advanced());
}

/// Made-up outputs for tests (which never open the machine's): the default, and "Phones".
/// What plays through them goes nowhere.
pub const OUTPUTS: Outputs = Outputs {
    list: || (vec!["Phones".into()], None),
    open: |name| match name {
        Some("Phones") | None => {
            let (mut a, _) = Audio::offline(8000);
            a.device = name.map(str::to_string);
            Ok(a)
        }
        Some(n) => anyhow::bail!("no output called {n}"),
    },
};

/// An output that isn't there doesn't open; asked for, the default opens instead and says
/// why.
#[test]
fn outputs_by_name_fall_back_to_the_default() {
    let outputs = OUTPUTS;
    let (audio, why) = outputs.open_preferring(Some("No Such Output 1234"));
    assert!(why.is_some_and(|w| w.contains("No Such Output 1234")));
    assert_eq!(audio.unwrap().device(), None, "the default");
    let (audio, why) = outputs.open_preferring(Some("Phones"));
    assert_eq!((audio.unwrap().device(), why), (Some("Phones"), None));
    assert!((Outputs::NONE.open)(None).is_err() && (Outputs::NONE.list)().0.is_empty());
}

/// The silent output plays to no one, at the pace a device would: half a second on, the track
/// has moved on by about that much, not run to its end.
#[test]
fn the_silent_output_plays_in_real_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ten-seconds.wav");
    std::fs::write(&path, wav(8000, 80_000, 1000)).unwrap();
    assert!((Outputs::SILENT.list)().0.is_empty());
    let mut audio = (Outputs::SILENT.open)(None).unwrap();
    audio.play(open_file(&path).unwrap(), false);
    std::thread::sleep(ms(500));
    let at = audio.position();
    assert!(at >= ms(100) && at <= ms(3000), "{at:?}");
}

/// A loop goes round as the track plays through the player, and once exited, playback
/// carries on through its end.
#[test]
fn loops_through_the_output() {
    let (mut audio, mut out) = Audio::offline(8000);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("silence.wav");
    std::fs::write(&path, wav(8000, 3 * 8000, 0)).unwrap();
    audio.play(open_file(&path).unwrap(), false);
    audio.set_loop(Some((ms(200), ms(400))));
    // Three times round and most of the way to the loop's end again.
    (&mut out).take(7000).for_each(drop);
    let pos = audio.position();
    assert!(pos >= ms(200) && pos < ms(400), "{pos:?}");
    audio.set_loop(None);
    (&mut out).take(4000).for_each(drop);
    assert!(audio.position() > ms(400), "{:?}", audio.position());
}

/// A seek is asked of the audio thread, which makes it before its next frame: asking never
/// waits for it (an output that's gone never would), and the playhead says where it's going
/// meanwhile. (The player is on a thread of its own, so a seek that waits fails the test
/// rather than hanging it.)
#[test]
fn seeks_wait_for_the_audio_thread_not_the_other_way_round() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ramp.wav");
    std::fs::write(&path, wav_of(8000, &(0..8000).collect::<Vec<i16>>())).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (mut audio, mut out) = Audio::offline(8000);
        audio.set_volume(1.0);
        audio.play(open_file(&path).unwrap(), false);
        (&mut out).take(10).for_each(drop);
        // Nothing reads the mixer while it's asked, as with an output that's been unplugged.
        audio.seek(ms(500), false);
        let _ = tx.send(audio.position());
        let sample = out.next().unwrap();
        assert!(!audio.seek_failed());
        let _ = tx.send(ms((sample * 32768.0).round() as u64 / 8));
    });
    let said = rx.recv_timeout(Duration::from_secs(10)).expect("the seek waited for the output");
    assert_eq!(said, ms(500));
    assert_eq!(rx.recv().unwrap(), ms(500), "it plays on from there");
}

/// Seeking out of a loop playing from memory, asked in the middle of a frame: the frame
/// finishes, then the track plays on from the seek, left channel first.
#[test]
fn seeks_out_of_a_kept_loop_keep_the_channels_in_order() {
    let head = Arc::new(Playhead::default());
    let mut l = Looper::new(counting(100), head.clone());
    head.set_loop(Some((ms(10), ms(14))));
    play(&mut l, 16);
    assert!(l.replay.is_some());
    let left = l.next().unwrap() as u32;
    l.try_seek(ms(50)).unwrap();
    let rest: Vec<u32> = (&mut l).take(5).map(|s| s as u32).collect();
    assert_eq!(rest, [left + 1, 100, 101, 102, 103]);
}

/// A source that panics after so many samples, as a decoder tripping over a bad frame might.
struct Breaks<S>(S, usize);

impl<S: Source> Iterator for Breaks<S> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        self.1 = self.1.checked_sub(1).expect("a bad frame");
        self.0.next()
    }
}

impl<S: Source> Source for Breaks<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.0.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.0.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.0.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.0.total_duration()
    }
}

/// A track whose decoder panics part-way ends there and says why, rather than taking the
/// output's stream (or the player) down with it.
#[test]
fn a_track_that_panics_ends_and_says_why() {
    let head = Arc::new(Playhead::default());
    let mut l = Looper::new(Breaks(counting(100), 6), head.clone());
    assert_eq!(play(&mut l, 5), [0, 1, 2]);
    assert!(l.next().is_none() && head.ended.load(Ordering::Acquire));
    let why = head.broke.lock().unwrap().clone().unwrap_or_default();
    assert!(why.contains("a bad frame"), "{why}");
}

/// An output that went away says so, and a glitch after it doesn't hide that.
#[cfg(feature = "playback")]
#[test]
fn stream_errors_say_when_the_output_is_gone() {
    use rodio::cpal::StreamError;
    let errors = StreamErrors::default();
    stream_error(&errors, StreamError::BufferUnderrun);
    assert!(errors.lock().unwrap().as_ref().is_some_and(|(_, gone)| !gone));
    stream_error(&errors, StreamError::DeviceNotAvailable);
    stream_error(&errors, StreamError::BufferUnderrun);
    let (said, gone) = errors.lock().unwrap().take().unwrap();
    assert!(gone && said.contains("no longer available"), "{said}");
}

#[test]
fn tap_mixes_frames_to_mono_and_keeps_the_latest() {
    let tap = Arc::new(Tap::default());
    let source = rodio::source::SineWave::new(441.0).take_duration(Duration::from_millis(10));
    let stereo = rodio::source::ChannelVolume::new(source, vec![1.0, 0.5]);
    let n = Tapped::new(stereo, tap.clone()).count();
    assert!(n > 0 && n % 2 == 0);
    assert_eq!(tap.written(), n / 2);
    let mut out = vec![0.0; 4];
    tap.latest(&mut out);
    // Each mono sample is the average of the two channels: 0.75 x the sine.
    assert!(out.iter().all(|s| s.abs() <= 0.75 + 1e-4));
    assert!(out.iter().any(|s| s.abs() > 0.01));
}

/// A track that can't seek ([`Breaks`] never breaks this soon, and doesn't seek).
fn unseekable() -> Breaks<SamplesBuffer> {
    Breaks(counting(100), usize::MAX)
}

/// A seek the track can't make is flagged for the UI, and the track plays on from where
/// it was.
#[test]
fn a_seek_the_track_cant_make_plays_on() {
    let head = Arc::new(Playhead::default());
    let mut l = Looper::new(unseekable(), head.clone());
    play(&mut l, 3);
    l.try_seek(ms(50)).unwrap();
    assert_eq!(play(&mut l, 2), [3, 4]);
    assert!(head.seek_failed.load(Ordering::Relaxed));
}

/// A loop set inside it, in a track that can't seek back to its start, plays on through
/// its end and stops looping.
#[test]
fn a_loop_the_track_cant_go_back_in_plays_on() {
    let head = Arc::new(Playhead::default());
    let mut l = Looper::new(unseekable(), head.clone());
    play(&mut l, 12);
    head.set_loop(Some((ms(10), ms(14))));
    assert_eq!(play(&mut l, 4), [12, 13, 14, 15]);
    assert!(!head.is_looping());
}

/// The loop playing set again, unchanged: it goes on from memory where it was.
#[test]
fn setting_the_same_loop_again_changes_nothing() {
    let head = Arc::new(Playhead::default());
    let mut l = Looper::new(counting(100), head.clone());
    head.set_loop(Some((ms(10), ms(14))));
    play(&mut l, 20);
    head.set_loop(Some((ms(10), ms(14))));
    assert_eq!(play(&mut l, 4), [12, 13, 10, 11]);
}

/// A track played to its end has nothing left of its span, so a queue goes on to the next
/// track's channels and rate.
#[test]
fn a_track_at_its_end_ends_its_span() {
    let head = Arc::new(Playhead::default());
    let mut l = Looper::new(counting(2), head.clone());
    assert_eq!(play(&mut l, 3), [0, 1]);
    assert_eq!(l.current_span_len(), Some(0));
}

/// What a person hears from a track played with loops set, changed and let go of, and seeks:
/// a playhead moving through the track a frame at a time, which a loop playing sends back to its
/// start at its end; a loop made longer or shorter (same start) takes a playhead past its new end
/// as far round it as it was; one set behind the playhead isn't gone back to; a seek moves the
/// playhead, before the next frame. What's played round a loop from memory is the same.
#[derive(Debug, Default)]
struct Heard {
    at: u64,
    bounds: Option<(u64, u64)>,
    /// The loop asked for since the last frame, and whether playing it.
    asked: Option<Option<(u64, u64)>>,
    looping: bool,
    seek: Option<u64>,
}

impl Heard {
    /// The frame played next.
    fn frame(&mut self) -> u64 {
        if let Some(to) = self.seek.take() {
            self.at = to;
        }
        if let Some(new) = self.asked.take() {
            let new = new.filter(|(a, b)| a < b);
            if new != self.bounds {
                let resized = self.bounds.is_some_and(|(old, _)| Some(old) == new.map(|(a, _)| a));
                self.bounds = new;
                if let Some((a, b)) = new
                    && resized
                    && self.at >= b
                {
                    self.at = a + (self.at - a) % (b - a);
                }
            }
        }
        if let Some((a, b)) = self.bounds
            && self.looping
            && self.at == b
        {
            self.at = a;
        }
        self.at += 1;
        self.at - 1
    }
}

#[hegel::test(test_cases = 300)]
fn loops_and_seeks_play_what_a_playhead_would(tc: hegel::TestCase) {
    use hegel::generators as gs;
    const FRAMES: u32 = 400;
    let head = Arc::new(Playhead::default());
    let mut l = Looper::new(counting(FRAMES), head.clone());
    let mut heard = Heard::default();
    // Samples into the frame being played: 0 or 1 (left or right).
    let mut half = false;
    // First a loop set, played round from memory and let go of partway (as drawn, the steps
    // seldom line up so), then anything.
    const SCRIPT: [u8; 6] = [2, 0, 0, 3, 0, 0];
    for step in 0..SCRIPT.len() + tc.draw(gs::integers::<usize>().max_value(25)) {
        let scripted = SCRIPT.get(step).copied();
        match scripted.unwrap_or_else(|| tc.draw(gs::integers::<u8>().max_value(5))) {
            0 | 1 => {
                let n = if scripted.is_some() { 40 } else { tc.draw(gs::integers::<usize>().max_value(40)) };
                for _ in 0..n {
                    // Keep short of the end, where the track would stop.
                    if heard.at >= FRAMES as u64 - 2 && !half {
                        break;
                    }
                    let s = l.next().unwrap() as u32;
                    if !half {
                        let f = heard.frame();
                        assert_eq!(s, 2 * f as u32, "left of frame {f}");
                    } else {
                        assert_eq!(s, 2 * (heard.at - 1) as u32 + 1, "right of frame {}", heard.at - 1);
                    }
                    half = !half;
                }
                if !half && heard.seek.is_none() {
                    assert_eq!(head.position(), ms(heard.at), "the playhead");
                }
                // Short of the end, what's next is never an empty span (that's the end).
                if heard.at < FRAMES as u64 - 2 {
                    assert!(l.current_span_len().is_none_or(|n| n > 0), "an empty span at {}", heard.at);
                }
            }
            2 => {
                let a = tc.draw(gs::sampled_from(&[0u64, 10, 20][..]));
                let len = tc.draw(gs::sampled_from(if scripted.is_some() {
                    &[2u64, 4, 8][..]
                } else {
                    &[0u64, 2, 4, 8][..]
                }));
                head.set_loop(Some((ms(a), ms(a + len))));
                heard.asked = Some(Some((a, a + len)));
                heard.looping = true;
            }
            3 => {
                head.set_loop(None);
                heard.looping = false;
            }
            4 => {
                let to = tc.draw(gs::sampled_from(&[0u64, 5, 11, 13, 21, 60][..]));
                l.try_seek(ms(to)).unwrap();
                heard.seek = Some(to);
                assert_eq!(head.position(), ms(to), "where it's going");
            }
            _ => {
                // The same loop set again: what's playing it goes on as it was.
                if let Some((a, b)) = heard.bounds.filter(|_| heard.asked.is_none()) {
                    head.set_loop(Some((ms(a), ms(b))));
                    heard.asked = Some(Some((a, b)));
                    heard.looping = true;
                }
            }
        }
        assert_eq!(head.is_looping(), heard.looping);
    }
}

/// What an output's stream said last is kept for the player: the output going away (unplugged,
/// or its stream no good any more) once it has, over any glitch after it; else the latest glitch.
/// Asked, it's said once.
#[cfg(feature = "playback")]
#[hegel::test(test_cases = 300)]
fn a_stream_says_its_output_went_away_over_any_glitch(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use rodio::cpal::{BackendSpecificError, StreamError};
    let errors = StreamErrors::default();
    let mut kept: Option<(String, bool)> = None;
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(8)) {
        if tc.draw(gs::integers::<u8>().max_value(3)) == 0 {
            assert_eq!(errors.lock().unwrap().take(), kept.take(), "said once");
            continue;
        }
        let e = match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => StreamError::DeviceNotAvailable,
            1 => StreamError::StreamInvalidated,
            2 => StreamError::BufferUnderrun,
            _ => {
                StreamError::BackendSpecific { err: BackendSpecificError { description: "the driver hiccuped".into() } }
            }
        };
        let gone = matches!(e, StreamError::DeviceNotAvailable | StreamError::StreamInvalidated);
        let said = e.to_string();
        stream_error(&errors, e);
        if gone || !kept.as_ref().is_some_and(|k| k.1) {
            kept = Some((said, gone));
        }
        assert_eq!(*errors.lock().unwrap(), kept);
    }
}

/// An output plays what it's given as asked: a file from its start (or where it's to start),
/// paused or playing, its time passing only while it plays; a seek is where it is at once; space
/// pauses and plays; the volume stays within bounds; only a track of the same format queues
/// behind the playing one, which the player goes straight on to; stopped, nothing's loaded.
#[hegel::test(test_cases = 100)]
fn an_output_plays_pauses_seeks_and_queues_as_asked(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let file_at = |rate: u32, frames: i16| {
        let path = dir.path().join(format!("{rate}-{frames}.wav"));
        std::fs::write(&path, wav_of(rate, &(0..frames).collect::<Vec<i16>>())).unwrap();
        open_file(&path).unwrap()
    };
    let (mut audio, mut out) = Audio::offline(8000);
    let (mut loaded, mut paused, mut queued) = (false, true, false);
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(12)) {
        match tc.draw(gs::integers::<u8>().max_value(7)) {
            0 => {
                let mut file = file_at(8000, 800);
                let start = tc.draw(gs::optional(gs::sampled_from(&[10u64, 50][..])));
                file.start = start.map(ms);
                paused = tc.draw(gs::booleans());
                audio.play(file, paused);
                (loaded, queued) = (true, false);
                assert_eq!(audio.position(), ms(start.unwrap_or(0)));
            }
            1 => {
                audio.toggle_pause();
                paused = !paused || !loaded;
            }
            2 => {
                let v = tc.draw(gs::sampled_from(&[-1.0f32, 0.0, 0.7, 1.5, 3.0][..]));
                audio.set_volume(v);
                assert_eq!(audio.volume(), v.clamp(0.0, 1.5));
            }
            3 => {
                let to = ms(tc.draw(gs::sampled_from(&[0u64, 20, 90][..])));
                audio.seek(to, tc.draw(gs::booleans()));
                if loaded && !audio.finished() {
                    assert_eq!(audio.position(), to, "where it's going");
                }
            }
            4 => {
                let rate = tc.draw(gs::sampled_from(&[8000u32, 16000][..]));
                let ok = audio.queue(file_at(rate, 400)).is_ok();
                assert_eq!(ok, loaded && rate == 8000, "rate {rate}: only the playing track's format queues");
                queued |= ok;
            }
            5 => {
                audio.unqueue();
                queued = false;
            }
            6 => {
                audio.stop();
                (loaded, paused, queued) = (false, true, false);
            }
            _ => {
                // Time passes: the output asks for samples.
                let before = audio.position();
                let n = tc.draw(gs::sampled_from(&[0usize, 80, 400, 1200][..]));
                (&mut out).take(n).for_each(drop);
                if audio.advanced() {
                    assert!(queued, "went on to nothing queued");
                    queued = false;
                } else if loaded && paused {
                    assert_eq!(audio.position(), before, "paused, time stands still");
                }
            }
        }
        assert_eq!(audio.is_loaded(), loaded);
        assert_eq!(audio.is_paused(), paused || !loaded, "loaded {loaded}");
    }
}

/// A track that can't seek plays on from where it was when asked to (saying so), and through
/// the end of a loop set behind the playhead (no longer looping), while a loop set ahead of it
/// still goes round, from memory; a track that breaks part-way ends there, saying why, with
/// nothing left of its span; one dropped from the queue ends as it starts.
#[hegel::test(test_cases = 200)]
fn a_track_that_cant_seek_or_breaks_plays_on_or_ends(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let ahead = tc.draw(gs::integers::<u64>().min_value(1).max_value(20));
    let len = tc.draw(gs::integers::<u64>().min_value(2).max_value(8));

    // Seeks it can't make: flagged, and it plays on.
    let head = Arc::new(Playhead::default());
    let mut l = Looper::new(unseekable(), head.clone());
    let at = play(&mut l, ahead as usize).len() as u64;
    l.try_seek(ms(tc.draw(gs::integers::<u64>().max_value(90)))).unwrap();
    assert_eq!(play(&mut l, 2), [at as u32, at as u32 + 1], "on from where it was");
    assert!(head.seek_failed.load(Ordering::Relaxed));

    // A loop behind the playhead: through its end, looping no more.
    let a = (at + 2).saturating_sub(len.min(at + 2));
    head.set_loop(Some((ms(a), ms(at + 3))));
    let on = play(&mut l, 6);
    assert_eq!(on, (at as u32 + 2..at as u32 + 8).collect::<Vec<_>>(), "loop {a}..{} from {}", at + 3, at + 2);
    assert!(!head.is_looping());

    // A loop ahead of it: round and round, from memory.
    let head = Arc::new(Playhead::default());
    let mut l = Looper::new(unseekable(), head.clone());
    head.set_loop(Some((ms(ahead), ms(ahead + len))));
    let heard = play(&mut l, (ahead + 3 * len) as usize);
    let want: Vec<u32> = (0..ahead).chain((0..3).flat_map(|_| ahead..ahead + len)).map(|f| f as u32).collect();
    assert_eq!(heard, want);

    // Breaking after so many samples: ended there, saying why.
    let k = tc.draw(gs::integers::<usize>().min_value(1).max_value(40));
    let head = Arc::new(Playhead::default());
    let mut l = Looper::new(Breaks(counting(100), k), head.clone());
    let samples: Vec<f32> = (&mut l).take(100).collect();
    assert_eq!(samples.len(), k);
    assert!(l.next().is_none() && head.ended.load(Ordering::Acquire));
    assert!(head.broke.lock().unwrap().as_deref().is_some_and(|w| w.contains("a bad frame")));
    assert_eq!(l.current_span_len(), Some(0));

    // Dropped from the queue: it ends as it starts.
    let head = Arc::new(Playhead::default());
    let mut l = Looper::new(counting(10), head.clone());
    head.dropped.store(true, Ordering::Relaxed);
    assert!(l.next().is_none() && head.ended.load(Ordering::Acquire) && head.started.load(Ordering::Acquire));
}
