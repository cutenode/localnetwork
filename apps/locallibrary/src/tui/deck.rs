//! The deck: what the player does with the playing track beyond play, pause and seek, the way a
//! CDJ does it: hot cue pads, beat jumps with quantize, and loops; and which output it plays
//! through. Its keys are in the keymap (`keymap.rs`); [`on_outputs_key`] has the keys while the
//! outputs are listed.

use std::time::Duration;

use locallibrary::anlz::{Beat, Cue, TrackAnalysis};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::ListState;

use super::app::{App, Level, Msg, View};
use super::audio::{Audio, Listed};
use super::theme;

/// Hot cues the digit row plays: 1-8 for A-H, as many as a CDJ has pads.
pub const PADS: u8 = 8;

/// The outputs listed to pick from (`d`).
#[derive(Debug, Default)]
pub struct Outputs {
    /// The devices' names; the list shows "the system's default" before them.
    pub names: Vec<String>,
    /// Which of them is the default.
    pub default: Option<String>,
    /// The listing of the devices still running (see `Msg::Outputs`), so one that comes back
    /// after the list was closed or opened again is dropped.
    pub listing: Option<u64>,
    /// Why the devices couldn't be listed.
    pub failed: Option<String>,
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
    /// The output still opening in the background (see `Msg::Output`): the last asked for.
    pub opening: Option<u64>,
}

impl Default for Deck {
    fn default() -> Deck {
        Deck {
            pads: false,
            quantize: true,
            looped: None,
            single: false,
            auto_cue: false,
            output: None,
            outputs: None,
            opening: None,
        }
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
    let msg = if app.deck.single { "autoplay off: stops after each track" } else { "autoplay on" };
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
    let (color, bounds) = (theme::palette().cue_color(cue), cue.end_ms.map(|end| (cue.start_ms as f64, end as f64)));
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
    let color = now.phrases.iter().find(|p| p.start_ms == start).map(|p| theme::current().phrase(p.kind));
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

/// Playback reaching a loop rekordbox marks active (from `prev` to `pos` ms, playing on rather
/// than seeking) plays round it, as a CDJ does, unless a loop is playing already.
pub(super) fn reach_active_loop(app: &mut App, prev: f64, pos: f64) {
    let played_on = pos > prev && pos - prev < 500.0;
    if !played_on || app.deck.looped.is_some_and(|l| l.on) {
        return;
    }
    let Some(now) = &app.now else { return };
    let reached = (now.analysis.cues.iter().filter(|c| c.active_loop))
        .filter_map(|c| Some(Loop { start: c.start_ms as f64, end: c.end_ms? as f64, on: true }))
        .find(|l| prev <= l.start && l.contains(pos));
    if reached.is_some() {
        set_loop(app, reached);
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
                active_loop: false,
            });
        }
        cues
    }
}

// ---------------------------------------------------------------------------------------------
// The output
// ---------------------------------------------------------------------------------------------

/// `d`: list the outputs. The list opens at once with the system's default, and the devices are
/// listed in the background: that can take a while, and keys pressed meanwhile mustn't go astray.
pub fn list_outputs(app: &mut App) {
    let (id, list) = (app.next_id(), app.outputs.list);
    let state = ListState::default().with_selected(Some(0));
    app.deck.outputs = Some(Outputs { listing: Some(id), state, ..Default::default() });
    app.spawn_or(move || Msg::Outputs(id, list().map_err(|e| format!("{e:#}"))), move |e| Msg::Outputs(id, Err(e)));
}

/// The devices listed for listing `id`, the one playing selected; dropped when the list has been
/// closed or opened again since.
pub fn listed_outputs(app: &mut App, id: u64, listed: Result<Listed, String>) {
    let playing = app.audio.as_ref().and_then(|a| a.device().map(str::to_string));
    let Some(o) = app.deck.outputs.as_mut().filter(|o| o.listing == Some(id)) else { return };
    o.listing = None;
    match listed {
        Ok((names, default)) => {
            let at = playing.and_then(|p| names.iter().position(|n| *n == p)).map_or(0, |i| i + 1);
            o.state.select(Some(at));
            (o.names, o.default) = (names, default);
        }
        Err(e) => o.failed = Some(e),
    }
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

/// Play through the output called `name` (`None`: the system's default) from now on. Opening it
/// can take a while (a Bluetooth or AirPlay device), so it opens in the background, and the track
/// plays on through the output it's on till then (see [`picked_output`]).
fn use_output(app: &mut App, name: Option<String>) {
    app.toast(format!("opening {}…", output_label(name.as_deref())), Level::Info);
    app.open_in_background(OutputAsk::Pick(name));
}

/// What an output was opened for (see `Msg::Output`).
#[derive(Debug, Clone)]
pub enum OutputAsk {
    /// Picked from the outputs list (`None`: the system's default).
    Pick(Option<String>),
    /// To play through, with none open: the one picked before, else the default.
    Play,
}

/// An output opened, and why the one asked for didn't (the default opened in its place).
pub type OpenedOutput = (Box<Audio>, Option<String>);

/// The output picked, `name`, opened: the track carries on through it as it was, from where it is
/// now (or was to start), looping, paused or playing.
pub(super) fn picked_output(app: &mut App, name: Option<String>, mut a: Audio) {
    a.set_volume(app.volume);
    let (at, playing) = app.playback();
    app.audio = Some(a); // the old output closes here
    app.audio_error = None;
    app.toast(format!("playing through {}", output_label(name.as_deref())), Level::Info);
    app.deck.output = name;
    app.restart(at, playing);
}

fn output_label(name: Option<&str>) -> &str {
    name.unwrap_or("the default output")
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
mod tests;
