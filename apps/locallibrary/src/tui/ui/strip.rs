//! The deck strip, on screen in every view: what's playing, and its waveform with the playhead on
//! it. [`draw_strip`] is all the rest of the player knows of it.

use std::ptr;

use loclib_waveform::Overview;
use ratatui::Frame;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::Color;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::output::clock;
use crate::tui::app::{App, Repeat};
use crate::tui::icons;
use crate::tui::theme::{self, bold, dim, fg, text};

use super::*;

/// The least the title keeps of the line, ahead of the hot cues.
const TITLE: usize = 16;

/// What "Nothing playing" takes of the line, ahead of the deck's states.
const IDLE: usize = 20;

/// What the waveform was drawn from: while it's the same, the drawing is too.
#[derive(Clone, Copy, PartialEq)]
struct Drawn {
    track: Option<u64>,
    columns: usize,
    cues: usize,
    looped: Option<(u32, u32)>,
    size: (u16, u16),
    theme: usize,
    tier: theme::Tier,
}

/// The waveform drawn twice, ahead of the playhead and behind it: a frame only picks from these.
#[derive(Default)]
pub struct StripWave {
    drawn: Option<Drawn>,
    ahead: Buffer,
    played: Buffer,
}

/// What's playing, over its waveform. The waveform has no rows when there are too few.
pub(super) fn draw_strip(f: &mut Frame, app: &mut App, area: Rect) {
    let [line, wave] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
    f.render_widget(strip_line(app, line.width as usize), line);
    if wave.height >= 2 {
        app.areas.overview = wave;
        draw_wave(app, f.buffer_mut(), wave);
    }
}

/// The waveform at `area`, its played part behind the playhead.
fn draw_wave(app: &mut App, buf: &mut Buffer, area: Rect) {
    let progress = app.now.as_ref().map_or(0.0, |n| app.position().as_secs_f64() / n.duration.as_secs_f64().max(0.001));
    let drawn = Drawn {
        track: app.now.as_ref().map(|n| n.id),
        columns: app.now.as_ref().and_then(|n| n.analysis.preview.as_ref()).map_or(0, |w| w.columns.len()),
        cues: app.now.as_ref().map_or(0, |n| n.analysis.cues.len()),
        looped: app.deck.looped.filter(|l| l.on).map(|l| (l.start as u32, l.end as u32)),
        size: (area.width, area.height),
        theme: ptr::from_ref(theme::current()) as usize,
        tier: theme::tier(),
    };
    if app.strip.drawn != Some(drawn) {
        let cues = app.drawn_cues();
        let wave = app.now.as_ref().and_then(|n| n.analysis.preview.as_ref());
        let duration_ms = app.now.as_ref().map_or(0.0, |n| n.duration.as_secs_f64() * 1000.0);
        let at = Rect { x: 0, y: 0, ..area };
        let overview = |progress| Overview { wave, progress, cues: &cues, duration_ms, palette: theme::palette() };
        let mut buffers = [Buffer::empty(at), Buffer::empty(at)];
        overview(0.0).render(at, &mut buffers[0]);
        overview(1.0).render(at, &mut buffers[1]);
        let [ahead, played] = buffers;
        app.strip = StripWave { drawn: Some(drawn), ahead, played };
    }
    let head = ((progress.clamp(0.0, 1.0) * f64::from(area.width)) as u16).min(area.width - 1);
    let palette = theme::palette();
    let playhead = (palette.paint)(palette.playhead);
    for y in 0..area.height {
        for x in 0..area.width {
            let at = Position::new(x, y);
            let from = if x < head { &app.strip.played } else { &app.strip.ahead };
            let Some(cell) = from.cell(at) else { continue };
            let Some(to) = buf.cell_mut((area.x + x, area.y + y)) else { continue };
            *to = cell.clone();
            // The row above the waveform holds markers, which the playhead passes under.
            if x == head && y > 0 {
                head_cell(to, playhead, drawn.columns > 0);
            }
        }
    }
}

/// Turn a cell of the waveform (or the rail with none) into the playhead's. Above a quiet column
/// the waveform's playhead is a line; beside the rail there's nothing to draw.
fn head_cell(cell: &mut Cell, colour: Color, wave: bool) {
    match cell.symbol() {
        " " if wave => {
            cell.set_symbol("│");
        }
        "─" | "━" => {
            cell.set_symbol(icons::DOT.get());
        }
        _ => {}
    }
    cell.set_fg(colour);
    if cell.bg != Color::Reset {
        cell.set_bg(colour);
    }
}

/// The line over the waveform: the track, then its tempo, key, time left and hot cues, and the
/// state of the deck, as much of that as fits.
fn strip_line(app: &App, width: usize) -> Line<'static> {
    let mut right = Vec::new();
    if app.deck.pads {
        right.extend([Span::styled(" ' ", theme::key()), Span::styled("pads on ", bold(theme::current().warning))]);
    }
    let Some(now) = &app.now else {
        let (states, room) = (states(app), width.saturating_sub(right.iter().map(Span::width).sum()));
        if states.iter().map(Span::width).sum::<usize>() + IDLE < room {
            right.extend(states);
        }
        let used = right.iter().map(Span::width).sum::<usize>() + 1;
        return spread(fit(idle(app), width.saturating_sub(used)), right, width as u16);
    };
    let t = &now.track;
    let pos_ms = app.position().as_secs_f64() * 1000.0;
    let (glyph, c) = if app.is_playing() {
        (icons::PLAY, theme::current().accent)
    } else {
        (icons::PAUSE, theme::current().warning)
    };
    let mut left = vec![Span::styled(glyph.padded(), bold(c))];
    left.push(Span::styled(
        if t.title.is_empty() { "Untitled".into() } else { t.title.clone() },
        bold(theme::current().text),
    ));
    if !t.artist.is_empty() {
        left.push(Span::styled(format!("  {}", t.artist), fg(theme::current().accent)));
    }

    let bpm = now.beat_at(pos_ms).map(|(i, _, _)| now.analysis.beats[i].bpm).filter(|b| *b > 0.0).unwrap_or(t.bpm);
    right.push(Span::styled(format!(" {bpm:.2}"), bold(theme::current().warning)));
    let key = now.key.map_or_else(|| t.key.clone(), |k| k.code());
    if !key.is_empty() {
        right.push(Span::styled(format!("  {key}"), bold(theme::current().info)));
    }
    let left_secs = (now.duration.as_secs_f64() - pos_ms / 1000.0).max(0.0);
    right.push(Span::styled(format!("  -{} ", clock(left_secs)), text()));
    if let Some(chip) = loop_chip(app) {
        right.extend([chip, Span::raw(" ")]);
    }
    let room = width.saturating_sub(right.iter().map(Span::width).sum::<usize>() + TITLE);
    let cues = pad_chips(app, now, pos_ms);
    let cues_w: usize = cues.iter().map(Span::width).sum();
    if cues_w <= room {
        right.extend(cues);
    }
    let states = states(app);
    if cues_w + states.iter().map(Span::width).sum::<usize>() <= room {
        right.extend(states);
    }
    let used: usize = right.iter().map(Span::width).sum();
    spread(fit(left, width.saturating_sub(used + 1)), right, width as u16)
}

/// The volume, and how the deck differs from how it starts.
fn states(app: &App) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(format!(" {:.0}%", app.volume * 100.0), dim())];
    let on = fg(theme::current().info);
    let mut say = |shown: String| spans.push(Span::styled(format!("  {shown}"), on));
    match app.repeat {
        Repeat::Off => {}
        Repeat::All => say(format!("{} all", icons::REPEAT.get())),
        Repeat::One => say(format!("{} one", icons::REPEAT.get())),
    }
    if app.shuffle {
        say(icons::MIX.get().to_string());
    }
    if app.deck.single {
        say("single".into());
    }
    if app.deck.auto_cue {
        say("auto cue".into());
    }
    if !app.deck.quantize {
        say("Q off".into());
    }
    spans
}

/// The line with nothing playing: what would carry on, or why nothing can.
fn idle(app: &App) -> Vec<Span<'static>> {
    let mut spans = vec![
        Span::styled(icons::SQUARE.padded(), dim()),
        Span::styled("Nothing playing  ", bold(theme::current().text)),
    ];
    if let Some(e) = &app.audio_error {
        spans.push(Span::styled(format!("No audio output: {e}"), fg(theme::current().critical)));
        return spans;
    }
    match app.resumable() {
        // Where the last run left off.
        Some((t, at)) => spans.extend([
            Span::styled("space", theme::key()),
            Span::styled(format!(" carries on with {} from {}", t.display_name(), clock(at.as_secs_f64())), dim()),
        ]),
        None => spans.push(Span::styled("Pick a track and press enter.", dim())),
    }
    spans
}

#[cfg(test)]
mod tests;
