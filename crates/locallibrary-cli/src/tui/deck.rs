//! The deck: what the player does with the playing track beyond play, pause and seek, the way a
//! CDJ does it: hot cue pads, beat jumps with quantize, and loops; and which output it plays
//! through. Its keys are in the keymap (`keymap.rs`); [`on_outputs_key`] has the keys while the
//! outputs are listed.

use std::time::Duration;

use locallibrary::anlz::{Beat, Cue, TrackAnalysis};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::ListState;

use super::app::{App, Level, View};
use super::theme;
use super::widgets::cue_color;

/// Hot cues the digit row plays: 1-8 for A-H, as many as a CDJ has pads.
pub const PADS: u8 = 8;

/// The outputs listed to pick from (`d`).
#[derive(Debug)]
pub struct Outputs {
    /// The devices' names; the list shows "the system's default" before them.
    pub names: Vec<String>,
    /// Which of them is the default.
    pub default: Option<String>,
    pub state: ListState,
}

/// A loop on the deck: a saved one, or one made at the playhead. Positions in ms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loop {
    pub start: f64,
    pub end: f64,
    /// Playing round; off once exited, and playback carried on through its end.
    pub on: bool,
}

impl Loop {
    pub fn contains(&self, ms: f64) -> bool {
        self.start <= ms && ms < self.end
    }
}

/// How the deck is set.
#[derive(Debug)]
pub struct Deck {
    /// The digit row plays hot cues instead of switching views (full screen always does).
    pub pads: bool,
    /// Jumps keep to the beat grid (see [`Snap`]); on, as on a CDJ, until turned off.
    pub quantize: bool,
    /// The playing track's loop: the one playing, or the last one exited.
    pub looped: Option<Loop>,
    /// Single play, as a CDJ's play mode: stop after each track (else play on through the queue).
    pub single: bool,
    /// Auto cue: tracks start at their first memory cue or beat (see [`auto_cue`]).
    pub auto_cue: bool,
    /// The output picked (`None`: the system's default), kept while it's unplugged.
    pub output: Option<String>,
    /// The outputs, while they're listed to pick from.
    pub outputs: Option<Outputs>,
}

impl Default for Deck {
    fn default() -> Deck {
        Deck { pads: false, quantize: true, looped: None, single: false, auto_cue: false, output: None, outputs: None }
    }
}

impl App {
    /// Whether the digit row plays hot cues: in the library, full screen or with the pads on.
    pub fn pads_on(&self) -> bool {
        self.view == View::Library && (self.fullscreen || self.deck.pads)
    }

    /// The playhead, in milliseconds.
    pub fn position_ms(&self) -> f64 {
        self.position().as_secs_f64() * 1000.0
    }

    /// How a jump made now keeps to the grid.
    pub fn snap(&self) -> Snap {
        match (self.deck.quantize, self.is_playing()) {
            (false, _) => Snap::Off,
            (true, true) => Snap::Phase,
            (true, false) => Snap::Beat,
        }
    }
}

/// `'`: the digit row plays hot cues in the library, or switches views again.
pub fn toggle_pads(app: &mut App) {
    app.deck.pads = !app.deck.pads;
    let msg = if app.deck.pads { "pads on: 1-8 play hot cues A-H" } else { "pads off: 1-5 switch views" };
    app.toast(msg, Level::Info);
}

/// `Q`: jumps keep to the beat grid, or don't.
pub fn toggle_quantize(app: &mut App) {
    app.deck.quantize = !app.deck.quantize;
    app.toast(if app.deck.quantize { "quantize on" } else { "quantize off" }, Level::Info);
}

/// `S`: stop after each track, or play on.
pub fn toggle_single(app: &mut App) {
    app.drop_upcoming();
    app.deck.single = !app.deck.single;
    let msg = if app.deck.single { "single play: stops after each track" } else { "continuous play" };
    app.toast(msg, Level::Info);
}

/// `A`: start tracks at their first memory cue or beat, or at the beginning.
pub fn toggle_auto_cue(app: &mut App) {
    app.drop_upcoming();
    app.deck.auto_cue = !app.deck.auto_cue;
    let msg = match app.deck.auto_cue {
        true => "auto cue on: tracks start at their first memory cue or beat",
        false => "auto cue off: tracks start at the beginning",
    };
    app.toast(msg, Level::Info);
}

/// Hot cue `slot` (0 is A), like a CDJ's pad: jump to it and play (a hot cue saved as a loop
/// plays the loop). Paused, it plays from there too: a terminal doesn't say when a key is let
/// go, so there's no previewing while it's held.
pub fn hot_cue(app: &mut App, slot: u8) {
    let Some(now) = &app.now else { return };
    let letter = (b'A' + slot) as char;
    let Some(cue) = now.analysis.cues.iter().find(|c| c.hot_cue == Some(slot)) else {
        let msg = if now.loading { "reading the cues…".to_string() } else { format!("no hot cue {letter}") };
        app.toast(msg, Level::Info);
        return;
    };
    let at = cue_jump(&now.analysis.beats, app.position_ms(), cue.start_ms as f64, app.snap());
    let (color, bounds) = (cue_color(cue), cue.end_ms.map(|end| (cue.start_ms as f64, end as f64)));
    // The jump first: it leaves the loop playing, if any, and may land a little before this one.
    app.jump(at, Some(color));
    if let Some((start, end)) = bounds {
        set_loop(app, Some(Loop { start, end, on: true }));
    }
    if let Some(a) = &app.audio
        && a.is_loaded()
        && a.is_paused()
    {
        a.toggle_pause();
    }
}

/// Move `by` beats along the grid (back when negative). A track without a grid moves by its BPM.
pub fn beat_jump(app: &mut App, by: f64) {
    let Some(now) = &app.now else { return };
    let pos = app.position_ms();
    let to = match jump_by(&now.analysis.beats, pos, by, app.snap()) {
        Some(t) => t,
        None if now.track.bpm > 0.0 => pos + by * 60_000.0 / now.track.bpm,
        None => {
            let msg = if now.loading { "reading the beat grid…" } else { "no beat grid or tempo to jump by" };
            app.toast(msg, Level::Info);
            return;
        }
    };
    app.jump(to.max(0.0), None);
}

/// `N` `P`: the next or previous phrase of rekordbox's phrase analysis, landing as a cue jump does.
pub fn phrase_jump(app: &mut App, forward: bool) {
    let Some(now) = &app.now else { return };
    if now.phrases.is_empty() {
        let msg = if now.loading { "reading the phrases…" } else { "no phrase analysis for this track" };
        app.toast(msg, Level::Info);
        return;
    }
    let starts: Vec<f64> = now.phrases.iter().map(|p| p.start_ms).collect();
    let Some((start, at)) = mark_jump(&now.analysis.beats, app.position_ms(), &starts, forward, app.snap()) else {
        app.toast("no more phrases", Level::Info);
        return;
    };
    let color = now.phrases.iter().find(|p| p.start_ms == start).map(|p| theme::phrase_color(p.kind));
    app.jump(at, color);
}

// ---------------------------------------------------------------------------------------------
// Loops
// ---------------------------------------------------------------------------------------------

/// Put a loop on the deck and tell the audio (`None`: none at all, for a new track).
pub(super) fn set_loop(app: &mut App, l: Option<Loop>) {
    app.deck.looped = l;
    apply_loop(app);
}

/// Tell the audio about the deck's loop (again: once a track opening has its audio).
pub(super) fn apply_loop(app: &App) {
    let Some(a) = &app.audio else { return };
    let at = |ms: f64| Duration::from_micros((ms.max(0.0) * 1000.0).round() as u64);
    match app.deck.looped {
        Some(l) if l.on => a.set_loop(Some((at(l.start), at(l.end)))),
        _ => a.set_loop(None),
    }
}

/// A seek to `to`: out of the loop playing, that's the loop left, as on a CDJ.
pub(super) fn seeking(app: &mut App, to: Duration) {
    let ms = to.as_secs_f64() * 1000.0;
    if let Some(l) = app.deck.looped.filter(|l| l.on && !l.contains(ms)) {
        set_loop(app, Some(Loop { on: false, ..l }));
    }
}

/// `l`: exit the loop playing; else loop the one the playhead is in (the last one exited, or a
/// saved loop), else make one of 4 beats from the beat the playhead is in (from the playhead
/// itself with quantize off).
pub fn toggle_loop(app: &mut App) {
    let Some(now) = &app.now else { return };
    if let Some(l) = app.deck.looped.filter(|l| l.on) {
        set_loop(app, Some(Loop { on: false, ..l }));
        return;
    }
    let pos = app.position_ms();
    let saved = (now.analysis.cues.iter())
        .filter_map(|c| Some(Loop { start: c.start_ms as f64, end: c.end_ms? as f64, on: true }))
        .filter(|l| l.contains(pos))
        .min_by(|a, b| (a.end - a.start).total_cmp(&(b.end - b.start)));
    let made = || {
        let (start, end) = match auto_loop(&now.analysis.beats, pos, 4.0, app.snap() != Snap::Off) {
            Some(bounds) => bounds,
            None if now.track.bpm > 0.0 => (pos, pos + 4.0 * 60_000.0 / now.track.bpm),
            None => return None,
        };
        // Made in the last beats, it ends with the track (else it would never come round).
        let end = if now.duration.is_zero() { end } else { end.min(now.duration.as_secs_f64() * 1000.0) };
        (end - start >= 15.0).then_some(Loop { start, end, on: true })
    };
    match app.deck.looped.filter(|l| l.contains(pos)).map(|l| Loop { on: true, ..l }).or(saved).or_else(made) {
        Some(l) => set_loop(app, Some(l)),
        None => {
            let msg = if now.loading { "reading the beat grid…" } else { "no beat grid or tempo to loop by" };
            app.toast(msg, Level::Info);
        }
    }
}

/// `(` `)`: halve or double the loop, in beats along the grid (by time without one). Shorter
/// with the playhead past its new end, playback goes back round as far as it was.
pub fn resize_loop(app: &mut App, factor: f64) {
    let Some(l) = app.deck.looped.filter(|_| app.now.is_some()) else {
        app.toast("no loop: l makes one", Level::Info);
        return;
    };
    let Some(now) = &app.now else { return };
    let beats = &now.analysis.beats;
    let end = match (beat_index(beats, l.start), beat_index(beats, l.end)) {
        (Some(a), Some(b)) => beat_time(beats, a + (b - a) * factor).unwrap_or(l.end),
        _ => l.start + (l.end - l.start) * factor,
    };
    // From a 32nd of a beat (at 120 BPM) to five minutes, and not past the track's end.
    let len = (end - l.start).clamp(15.0, 300_000.0);
    let end = (l.start + len).min(now.duration.as_secs_f64() * 1000.0);
    if end - l.start >= 15.0 {
        set_loop(app, Some(Loop { end, ..l }));
    }
}

/// Where auto cue starts a track, like a CDJ: at its first memory cue, else its first beat
/// (where the sound starts, near enough). `None`: at the beginning.
pub fn auto_cue(a: &TrackAnalysis) -> Option<Duration> {
    let memory = a.cues.iter().filter(|c| c.hot_cue.is_none()).map(|c| c.start_ms).min();
    let ms = memory.or_else(|| a.beats.first().map(|b| b.time_ms)).filter(|&ms| ms > 0)?;
    Some(Duration::from_millis(ms as u64))
}

/// A loop `beats` long from the beat the playhead is in (`on_beat`) or the playhead itself, along
/// the grid. `None` without a grid.
pub fn auto_loop(beats: &[Beat], ms: f64, len: f64, on_beat: bool) -> Option<(f64, f64)> {
    let at = beat_index(beats, ms)?;
    // On the beat the playhead is in, unless that one's before the track starts.
    let at = match on_beat {
        true if beat_time(beats, at.floor())? < 0.0 => at.ceil(),
        true => at.floor(),
        false => at,
    };
    Some((beat_time(beats, at)?, beat_time(beats, at + len)?))
}

/// A loop's length in beats as a label: `4`, `1/2`.
pub fn beats_label(b: f64) -> String {
    if b >= 0.9 { format!("{:.0}", b.round()) } else { format!("1/{:.0}", (1.0 / b.max(0.01)).round()) }
}

impl App {
    /// The loop playing and its length in beats (when there's a grid).
    pub fn loop_playing(&self) -> Option<(Loop, Option<f64>)> {
        let l = self.deck.looped.filter(|l| l.on)?;
        let a = &self.now.as_ref()?.analysis;
        // A saved loop as rekordbox measured it, else along the grid.
        let saved = (a.cues.iter())
            .find(|c| c.start_ms == l.start as u32 && c.end_ms == Some(l.end as u32))
            .and_then(|c| c.loop_beats)
            .map(|(n, d)| n as f64 / d as f64);
        let len = saved.or_else(|| beat_index(&a.beats, l.end).zip(beat_index(&a.beats, l.start)).map(|(b, a)| b - a));
        Some((l, len))
    }

    /// The playing track's cues as drawn: the saved ones, and the loop on the deck when it's
    /// one made here (or a saved one resized).
    pub fn drawn_cues(&self) -> Vec<Cue> {
        let Some(now) = &self.now else { return Vec::new() };
        let mut cues = now.analysis.cues.clone();
        if let Some(l) = self.deck.looped.filter(|l| l.on)
            && !cues.iter().any(|c| c.start_ms == l.start as u32 && c.end_ms == Some(l.end as u32))
        {
            let label = self.loop_playing().and_then(|(_, b)| b).map(|b| format!("loop ↻{}", beats_label(b)));
            cues.push(Cue {
                hot_cue: None,
                start_ms: l.start as u32,
                end_ms: Some(l.end as u32),
                comment: label.unwrap_or_else(|| "loop".into()),
                color: None,
                color_id: 0,
                loop_beats: None,
            });
        }
        cues
    }
}

// ---------------------------------------------------------------------------------------------
// The output
// ---------------------------------------------------------------------------------------------

/// `d`: list the outputs, the one playing selected.
pub fn list_outputs(app: &mut App) {
    let (names, default) = (app.outputs.list)();
    let playing = app.audio.as_ref().and_then(|a| a.device().map(str::to_string));
    let at = playing.and_then(|p| names.iter().position(|n| *n == p)).map_or(0, |i| i + 1);
    app.deck.outputs = Some(Outputs { names, default, state: ListState::default().with_selected(Some(at)) });
}

/// Keys while the outputs are listed: move, pick one, or close the list.
pub fn on_outputs_key(app: &mut App, key: KeyEvent) {
    let Some(o) = &mut app.deck.outputs else { return };
    let last = o.names.len();
    let sel = o.state.selected().unwrap_or(0);
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => o.state.select(Some(sel.saturating_sub(1))),
        KeyCode::Down | KeyCode::Char('j') => o.state.select(Some((sel + 1).min(last))),
        KeyCode::Enter => {
            let name = sel.checked_sub(1).and_then(|i| o.names.get(i).cloned());
            app.deck.outputs = None;
            use_output(app, name);
        }
        _ => app.deck.outputs = None,
    }
}

/// Play through the output called `name` (`None`: the system's default) from now on, the track
/// carrying on as it was: from where it was (or was to start), looping, paused or playing.
fn use_output(app: &mut App, name: Option<String>) {
    match (app.outputs.open)(name.as_deref()) {
        Ok(mut a) => {
            a.set_volume(app.volume);
            let (at, playing) = app.playback();
            app.audio = Some(a); // the old output closes here
            app.audio_error = None;
            app.deck.output = name;
            let label = app.deck.output.clone().unwrap_or_else(|| "the default output".into());
            app.toast(format!("playing through {label}"), Level::Info);
            app.restart(at, playing);
        }
        Err(e) => app.toast(format!("{e:#}"), Level::Error),
    }
}

// ---------------------------------------------------------------------------------------------
// The beat grid
// ---------------------------------------------------------------------------------------------

/// How a jump keeps to the beat grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Snap {
    /// Quantize off: exactly where asked.
    Off,
    /// Quantized while paused: on a beat.
    Beat,
    /// Quantized while playing: as far before or after a beat as the playhead is now, so the
    /// rhythm carries on through the jump, as if it had waited for the nearest beat.
    Phase,
}

/// How long beat `i` lasts: until the next one, or at its own tempo for the last.
fn beat_len(beats: &[Beat], i: usize) -> f64 {
    match (beats.get(i), beats.get(i + 1)) {
        (Some(b), Some(next)) if next.time_ms > b.time_ms => (next.time_ms - b.time_ms) as f64,
        (Some(b), _) if b.bpm > 0.0 => 60_000.0 / b.bpm,
        _ => 500.0,
    }
}

/// Where `ms` falls on the grid, in beats from the first (index 0): 12.25 is a quarter of the
/// way from beat 12 to beat 13. Before the first beat and after the last, the grid carries on
/// at the tempo of the beat at that end. `None` without a grid.
pub fn beat_index(beats: &[Beat], ms: f64) -> Option<f64> {
    let first = beats.first()?;
    let Some(i) = beats.partition_point(|b| b.time_ms as f64 <= ms).checked_sub(1) else {
        return Some((ms - first.time_ms as f64) / beat_len(beats, 0));
    };
    Some(i as f64 + (ms - beats[i].time_ms as f64) / beat_len(beats, i))
}

/// The time of a beat index: the inverse of [`beat_index`].
pub fn beat_time(beats: &[Beat], index: f64) -> Option<f64> {
    let last = beats.len().checked_sub(1)?;
    let i = index.floor().clamp(0.0, last as f64) as usize;
    Some(beats[i].time_ms as f64 + (index - i as f64) * beat_len(beats, i))
}

/// Where a jump of `by` beats from `ms` lands. Quantized, it moves along the grid (keeping the
/// playhead's place in the beat while playing, onto a beat when paused); otherwise by `by`
/// lengths of the beat it's in.
pub fn jump_by(beats: &[Beat], ms: f64, by: f64, snap: Snap) -> Option<f64> {
    let at = beat_index(beats, ms)?;
    match snap {
        Snap::Phase => beat_time(beats, at + by),
        Snap::Beat => beat_time(beats, at.round() + by),
        Snap::Off => {
            let i = at.floor().clamp(0.0, (beats.len() - 1) as f64) as usize;
            Some(ms + by * beat_len(beats, i))
        }
    }
}

/// Where a jump from `ms` to a cue at `cue` lands: on the cue, or while playing quantized, as
/// far before or after it (half a beat at most) as the playhead is from its nearest beat.
pub fn cue_jump(beats: &[Beat], ms: f64, cue: f64, snap: Snap) -> f64 {
    match (snap, beat_index(beats, ms), beat_index(beats, cue)) {
        (Snap::Phase, Some(at), Some(c)) => beat_time(beats, c + (at - at.round())).unwrap_or(cue),
        _ => cue,
    }
}

/// A jump from `ms` to the next (`forward`) or previous of `marks` (cues or phrase starts, in
/// time order): the mark, and where the jump lands ([`cue_jump`]). Back skips a mark just passed,
/// so pressing again goes further back, and with none before it goes to the start. `None`: no
/// mark ahead.
pub fn mark_jump(beats: &[Beat], ms: f64, marks: &[f64], forward: bool, snap: Snap) -> Option<(f64, f64)> {
    let land = |t: f64| cue_jump(beats, ms, t, snap);
    let mark = match forward {
        true => marks.iter().copied().find(|&t| land(t) > ms + 250.0)?,
        false => marks.iter().copied().rev().find(|&t| land(t) + 1500.0 < ms).unwrap_or(0.0),
    };
    Some((mark, land(mark)))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use locallibrary::anlz::{PhraseKind, TrackAnalysis};

    use super::super::app::PhraseSpan;
    use super::super::app::testing::*;
    use super::*;

    /// Where playback goes once the track is open.
    fn target(app: &App) -> Option<u64> {
        app.pending_seek.map(|d| d.as_millis() as u64)
    }

    #[test]
    fn pads_play_hot_cues_in_full_screen_or_when_turned_on() {
        let cues = vec![cue(Some(0), 1_000, None), cue(Some(3), 64_000, None), cue(None, 30_000, None)];
        let (mut app, _dir) = playing(TrackAnalysis { cues, ..Default::default() });
        // Off: digits switch views as ever.
        press(&mut app, "2");
        assert_eq!(app.view, View::Drives);
        press(&mut app, "1");
        assert_eq!((app.view, target(&app)), (View::Library, None));

        // Full screen: 4 is hot cue D.
        press(&mut app, "f");
        press(&mut app, "4");
        assert_eq!((app.view, target(&app)), (View::Library, Some(64_000)));
        press(&mut app, "1");
        assert_eq!(target(&app), Some(1_000));
        // A pad with no hot cue says so and stays put.
        app.toast = None;
        press(&mut app, "2");
        assert_eq!(target(&app), Some(1_000));
        assert_eq!(app.toast.as_ref().map(|t| t.0.as_str()), Some("no hot cue B"));

        // Out of full screen, ' turns the pads on and off.
        press(&mut app, "f");
        press(&mut app, "'");
        assert!(app.pads_on());
        press(&mut app, "4");
        assert_eq!((app.view, target(&app)), (View::Library, Some(64_000)));
        press(&mut app, "'");
        press(&mut app, "3");
        assert_eq!(app.view, View::Snapshots);
        assert!(!app.pads_on(), "only in the library");
    }

    #[test]
    fn beats_and_times_on_a_steady_grid() {
        let g = grid(100, 120.0, 64);
        assert_eq!(beat_index(&[], 1000.0), None);
        assert_eq!(beat_index(&g, 100.0), Some(0.0));
        assert_eq!(beat_index(&g, 1225.0), Some(2.25));
        // Before the first beat and after the last, the grid carries on.
        assert_eq!(beat_index(&g, 0.0), Some(-0.2));
        assert_eq!(beat_index(&g, 100.0 + 70.0 * 500.0), Some(70.0));
        for at in [-1.5, 0.0, 2.25, 33.75, 63.0, 80.5] {
            let t = beat_time(&g, at).unwrap();
            assert!((beat_index(&g, t).unwrap() - at).abs() < 1e-9, "{at} -> {t}");
        }
    }

    #[test]
    fn beat_jumps_keep_the_beat_or_land_on_one() {
        let g = grid(100, 120.0, 64);
        // Playing, quantized: a bar on, as far into the beat as before.
        assert_eq!(jump_by(&g, 1225.0, 4.0, Snap::Phase), Some(3225.0));
        assert_eq!(jump_by(&g, 1225.0, -1.0, Snap::Phase), Some(725.0));
        // Paused, quantized: onto the nearest beat, then along.
        assert_eq!(jump_by(&g, 1225.0, 1.0, Snap::Beat), Some(1600.0));
        assert_eq!(jump_by(&g, 1400.0, 1.0, Snap::Beat), Some(2100.0));
        // Not quantized: by beat lengths from wherever it is.
        assert_eq!(jump_by(&g, 1234.0, 2.0, Snap::Off), Some(2234.0));
        assert_eq!(jump_by(&[], 1234.0, 2.0, Snap::Off), None);

        // A grid that speeds up from 120 to 150 BPM at beat 8: quantized jumps count beats, so
        // a phrase back from the faster part lands in the same place in the beat.
        let mut g = grid(0, 120.0, 8);
        g.extend(grid(4000, 150.0, 56));
        let at = 4000.0 + 10.5 * 400.0;
        let back = jump_by(&g, at, -16.0, Snap::Phase).unwrap();
        assert_eq!(back, 1250.0, "beat 2.5 of the 120 BPM part");
        assert_eq!(jump_by(&g, back, 16.0, Snap::Phase), Some(at));
    }

    #[test]
    fn quantized_cue_jumps_keep_the_beat_while_playing() {
        let g = grid(0, 120.0, 128);
        // A quarter beat past a beat: lands a quarter beat past the cue; just before a beat,
        // just before the cue.
        assert_eq!(cue_jump(&g, 10_125.0, 32_000.0, Snap::Phase), 32_125.0);
        assert_eq!(cue_jump(&g, 10_400.0, 32_000.0, Snap::Phase), 31_900.0);
        // Paused or not quantized: on the cue.
        for snap in [Snap::Beat, Snap::Off] {
            assert_eq!(cue_jump(&g, 10_125.0, 32_000.0, snap), 32_000.0);
        }
        assert_eq!(cue_jump(&[], 10_125.0, 32_000.0, Snap::Phase), 32_000.0);
    }

    #[test]
    fn beat_jump_keys() {
        let (mut app, _dir) = playing(TrackAnalysis { beats: grid(100, 120.0, 600), ..Default::default() });
        // Paused (the audio's still opening), quantized: along the grid from the beat nearest 0.
        for (key, to) in [('.', 600), ('>', 2100), ('}', 16_100)] {
            app.pending_seek = None;
            press(&mut app, &key.to_string());
            assert_eq!(target(&app), Some(to), "{key}");
        }
        press(&mut app, "Q");
        assert!(!app.deck.quantize && app.snap() == Snap::Off);
        app.pending_seek = None;
        press(&mut app, ">");
        assert_eq!(target(&app), Some(2000), "4 beats of 500 ms from 0");
        // Back past the start stops at it.
        press(&mut app, "{");
        assert_eq!(target(&app), Some(0));
        // No grid: the track's BPM (121) says how long a beat is.
        app.now.as_mut().unwrap().analysis.beats.clear();
        press(&mut app, ">");
        assert_eq!(target(&app), Some(1983));
    }

    #[test]
    fn phrase_and_cue_jumps_land_on_the_next_or_previous_mark() {
        let g = grid(0, 120.0, 128);
        let marks = [0.0, 16_000.0, 32_000.0, 48_000.0];
        let at = |ms: f64, forward: bool, snap: Snap| mark_jump(&g, ms, &marks, forward, snap);
        assert_eq!(at(1_000.0, true, Snap::Beat), Some((16_000.0, 16_000.0)));
        // One just ahead is passed over; playing quantized, the jump keeps the beat.
        assert_eq!(at(15_900.0, true, Snap::Beat), Some((32_000.0, 32_000.0)));
        assert_eq!(at(10_125.0, true, Snap::Phase), Some((16_000.0, 16_125.0)));
        assert_eq!(at(50_000.0, true, Snap::Off), None, "none ahead");
        // Back: past one just passed, and to the start before the first.
        assert_eq!(at(20_000.0, false, Snap::Off), Some((16_000.0, 16_000.0)));
        assert_eq!(at(17_000.0, false, Snap::Off), Some((0.0, 0.0)));
        assert_eq!(mark_jump(&g, 5_000.0, &[8_000.0], false, Snap::Off), Some((0.0, 0.0)));
    }

    #[test]
    fn phrase_jump_keys() {
        use locallibrary::anlz::PhraseKind;

        use super::super::app::PhraseSpan;
        let (mut app, _dir) = playing(TrackAnalysis { beats: grid(0, 120.0, 600), ..Default::default() });
        press(&mut app, "N");
        assert_eq!(
            (target(&app), app.toast.as_ref().map(|t| t.0.as_str())),
            (None, Some("no phrase analysis for this track"))
        );
        let span = |kind, start_ms, end_ms| PhraseSpan { kind, start_ms, end_ms, bars: 8 };
        app.now.as_mut().unwrap().phrases =
            vec![span(PhraseKind::Intro, 0.0, 16_000.0), span(PhraseKind::Chorus, 16_000.0, 32_000.0)];
        // (The audio's still opening: jumps go from the start.)
        press(&mut app, "N");
        assert_eq!(target(&app), Some(16_000));
        press(&mut app, "P");
        assert_eq!(target(&app), Some(0));
    }

    #[test]
    fn auto_cue_finds_the_first_memory_cue_or_beat() {
        let beats = grid(320, 120.0, 8);
        let cues = vec![cue(Some(0), 100, None), cue(None, 2_000, None), cue(None, 900, Some(1_900))];
        let at = |beats: &[Beat], cues: &[Cue]| {
            auto_cue(&TrackAnalysis { beats: beats.to_vec(), cues: cues.to_vec(), ..Default::default() })
        };
        assert_eq!(at(&beats, &cues), Some(Duration::from_millis(900)), "hot cues don't count");
        assert_eq!(at(&beats, &cues[..1]), Some(Duration::from_millis(320)));
        assert_eq!(at(&grid(0, 120.0, 8), &[]), None, "the first beat is the start");
        assert_eq!(at(&[], &[]), None);
    }

    #[test]
    fn loops_from_the_grid() {
        let g = grid(100, 120.0, 64);
        // From the beat the playhead is in, or from the playhead.
        assert_eq!(auto_loop(&g, 1225.0, 4.0, true), Some((1100.0, 3100.0)));
        assert_eq!(auto_loop(&g, 1225.0, 4.0, false), Some((1225.0, 3225.0)));
        assert_eq!(auto_loop(&[], 1225.0, 4.0, true), None);
        // The beat before the first starts before the track: the first one instead.
        assert_eq!(auto_loop(&g, 0.0, 4.0, true), Some((100.0, 2100.0)));
        assert_eq!((beats_label(4.0), beats_label(0.5), beats_label(0.25)), ("4".into(), "1/2".into(), "1/4".into()));
    }

    #[test]
    fn loop_keys() {
        let cues = vec![cue(Some(1), 8_000, Some(10_000)), cue(None, 20_000, Some(22_000))];
        let (mut app, _dir) = playing(TrackAnalysis { beats: grid(0, 120.0, 600), cues, ..Default::default() });
        let looped = |app: &App| app.deck.looped.map(|l| (l.start as u32, l.end as u32, l.on));
        // Nothing saved here: 4 beats from the beat the playhead's in.
        press(&mut app, "l");
        assert_eq!(looped(&app), Some((0, 2000, true)));
        assert_eq!(app.loop_playing().unwrap().1, Some(4.0));
        // Halved, doubled twice: along the grid.
        press(&mut app, "(");
        assert_eq!(looped(&app), Some((0, 1000, true)));
        press(&mut app, ")");
        press(&mut app, ")");
        assert_eq!(looped(&app), Some((0, 4000, true)));
        assert!(app.drawn_cues().iter().any(|c| c.end_ms == Some(4000) && c.comment == "loop ↻8"));
        let s = super::super::ui::screen(&mut app, 120, 50);
        assert!(s.contains("LOOP 8"), "{s}");
        // Exited: playback carries on; l again (still in it) loops it again.
        press(&mut app, "l");
        assert_eq!(looped(&app), Some((0, 4000, false)));
        assert!(app.loop_playing().is_none() && app.drawn_cues().len() == 2);
        press(&mut app, "l");
        assert_eq!(looped(&app), Some((0, 4000, true)));
        // A hot cue saved as a loop plays it; seeking out of it leaves it.
        app.deck.pads = true;
        press(&mut app, "2");
        assert_eq!(looped(&app), Some((8_000, 10_000, true)));
        assert_eq!(target(&app), Some(8_000));
        assert_eq!(app.drawn_cues().len(), 2, "a saved loop is drawn already");
        app.seek_to(Duration::from_secs(60));
        assert_eq!(looped(&app), Some((8_000, 10_000, false)));
    }

    /// A loop made in the track's last beats ends with it (else it would never come round), and
    /// a seek in a track whose length isn't known goes where it's asked.
    #[test]
    fn made_loops_and_seeks_keep_to_the_track() {
        let (mut app, _dir) = playing(TrackAnalysis { beats: grid(0, 120.0, 600), ..Default::default() });
        app.now.as_mut().unwrap().duration = Duration::from_millis(1500);
        press(&mut app, "l");
        assert_eq!(app.deck.looped.map(|l| (l.start, l.end)), Some((0.0, 1500.0)));
        app.now.as_mut().unwrap().duration = Duration::ZERO;
        app.seek_to(Duration::from_secs(30));
        assert_eq!(target(&app), Some(30_000));
    }

    #[test]
    fn outputs_list_and_close() {
        let (mut app, _dir) = playing(TrackAnalysis::default());
        press(&mut app, "d");
        let o = app.deck.outputs.as_ref().expect("listed");
        assert_eq!(o.state.selected(), Some(0), "the default, with nothing picked");
        let n = o.names.len();
        let s = super::super::ui::screen(&mut app, 100, 30);
        assert!(s.contains("Output") && s.contains("the system's default"), "{s}");
        // Keys move through the list and nothing else.
        for _ in 0..n + 2 {
            key(&mut app, KeyCode::Down);
        }
        assert_eq!(app.deck.outputs.as_ref().unwrap().state.selected(), Some(n));
        press(&mut app, "2");
        assert!(app.deck.outputs.is_none() && app.view == View::Library, "closed, not switched");
        // Picking the default: plays through it (with a device), or says there isn't one.
        press(&mut app, "d");
        key(&mut app, KeyCode::Enter);
        let said = app.toast.as_ref().map(|t| t.0.clone()).unwrap_or_default();
        match &app.audio {
            Some(_) => assert_eq!(said, "playing through the default output"),
            None => assert!(said.contains("no audio output"), "{said}"),
        }
        assert_eq!(app.deck.output, None);
    }

    /// Full screen, each hot cue has its key before it, with colours or without.
    #[test]
    fn full_screen_shows_which_key_plays_which_pad() {
        use super::super::theme::{self, Mode};
        use super::super::ui::screen;
        let cues = vec![cue(Some(0), 1_000, None), cue(Some(3), 64_000, None)];
        let (mut app, _dir) = playing(TrackAnalysis { cues, ..Default::default() });
        app.now.as_mut().unwrap().opening = false;
        let s = screen(&mut app, 120, 50);
        assert!(s.contains(" A  D ") && !s.contains("1 A 4 D"), "{s}");
        app.deck.pads = true;
        let s = screen(&mut app, 120, 50);
        assert!(s.contains("1 A 4 D") && s.contains("' pads on"), "{s}");
        app.deck.pads = false;
        app.fullscreen = true;
        for mode in [None, Some(Mode::Plain)] {
            theme::set_test_mode(mode);
            let s = screen(&mut app, 160, 45);
            theme::set_test_mode(None);
            assert!(s.contains("1 A 4 D") && s.contains("1-8 hot cue"), "{s}");
        }
    }

    /// With no beat grid and no tempo there's nothing to jump or loop by, which is said (while
    /// the track's analysis is still being read, that it is); without a loop, there's nothing
    /// to resize.
    #[test]
    fn says_why_a_jump_or_loop_cant_be_made() {
        let (mut app, _dir) = playing(TrackAnalysis::default());
        app.now.as_mut().unwrap().track.bpm = 0.0;
        press(&mut app, ".");
        assert_eq!(last_message(&app), "no beat grid or tempo to jump by");
        press(&mut app, "l");
        assert_eq!(last_message(&app), "no beat grid or tempo to loop by");
        press(&mut app, "(");
        assert_eq!(last_message(&app), "no loop: l makes one");
        app.now.as_mut().unwrap().loading = true;
        press(&mut app, ".");
        assert_eq!(last_message(&app), "reading the beat grid…");
        press(&mut app, "l");
        assert_eq!(last_message(&app), "reading the beat grid…");
        press(&mut app, "N");
        assert_eq!(last_message(&app), "reading the phrases…");
        press(&mut app, "'1");
        assert_eq!(last_message(&app), "reading the cues…");
        assert!(app.deck.looped.is_none() && target(&app).is_none());

        // Past the last phrase there's nowhere to go.
        app.now.as_mut().unwrap().phrases =
            vec![PhraseSpan { kind: PhraseKind::Intro, start_ms: 0.0, end_ms: 16_000.0, bars: 8 }];
        press(&mut app, "N");
        assert_eq!(last_message(&app), "no more phrases");
    }

    /// `A` turns auto cue on and off, and says what it does.
    #[test]
    fn auto_cue_is_turned_on_and_off() {
        let (mut app, _dir) = playing(TrackAnalysis::default());
        press(&mut app, "A");
        assert!(app.deck.auto_cue);
        assert!(last_message(&app).starts_with("auto cue on"), "{}", last_message(&app));
        press(&mut app, "A");
        assert!(!app.deck.auto_cue);
        assert!(last_message(&app).starts_with("auto cue off"), "{}", last_message(&app));
    }

    /// An output that won't open is said, and playback stays where it was.
    #[test]
    fn an_output_that_wont_open_is_said() {
        let (mut app, _dir) = playing(TrackAnalysis::default());
        app.outputs = super::super::audio::Outputs {
            list: || (vec!["Interface".into()], None),
            open: |_| anyhow::bail!("the interface is gone"),
        };
        // The default and the interface: the list stops at either end.
        press(&mut app, "djjk");
        assert_eq!(app.deck.outputs.as_ref().and_then(|o| o.state.selected()), Some(0));
        press(&mut app, "j\n");
        assert_eq!(last_message(&app), "the interface is gone");
        assert!(app.deck.output.is_none() && app.now.is_some());
    }

    /// A hot cue pressed while paused plays from there, as a CDJ's pad does; pressed while
    /// playing, it jumps back there and plays on.
    #[test]
    fn a_hot_cue_plays_from_its_place() {
        let (_dir, mut app, mut out) = playing_wavs(&[8000]);
        opened(&mut app);
        app.now.as_mut().unwrap().analysis.cues = vec![cue(Some(0), 200, None)];
        app.deck.pads = true;
        press(&mut app, " ");
        assert!(!app.is_playing());
        press(&mut app, "1");
        assert!(app.is_playing());
        assert_eq!(app.position(), Duration::from_millis(200));
        (&mut out).take(800).for_each(drop);
        assert_eq!(app.position(), Duration::from_millis(300));
        press(&mut app, "1");
        assert!(app.is_playing());
        assert_eq!(app.position(), Duration::from_millis(200));
    }

    /// Without a beat grid a loop is made, and halved, by the track's tempo.
    #[test]
    fn loops_by_the_tempo_without_a_grid() {
        let (mut app, _dir) = playing(TrackAnalysis::default());
        app.now.as_mut().unwrap().track.bpm = 120.0;
        press(&mut app, "l");
        assert_eq!(app.deck.looped.map(|l| (l.start, l.end)), Some((0.0, 2000.0)));
        press(&mut app, "(");
        assert_eq!(app.deck.looped.map(|l| (l.start, l.end)), Some((0.0, 1000.0)));
    }
}
