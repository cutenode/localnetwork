//! The deck strip, on screen in every view: what's playing, and its waveform with the playhead on
//! it. [`draw_strip`] is all the rest of the player knows of it.

use std::ptr;

use locallibrary::anlz::Cue;
use loclib_waveform::{PhraseStrip, WaveOverview, ZoomRing, playhead_cells};
use ratatui::Frame;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::Color;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::output::clock;
use crate::tui::app::{App, Repeat};
use crate::tui::deck::BandMode;
use crate::tui::icons;
use crate::tui::theme::{self, bold, dim, fg, text};

use super::*;

/// The least the title keeps of the line, ahead of the hot cues.
const TITLE: usize = 16;

/// What "Nothing playing" takes of the line, ahead of the deck's states.
const IDLE: usize = 20;

/// What the waveforms are drawn from: while it's the same, so are they.
#[derive(Clone, Copy, PartialEq)]
struct Drawn {
    track: Option<u64>,
    waves: (usize, usize),
    bands: BandMode,
    phrases: usize,
    cues: usize,
    looped: Option<(u32, u32)>,
    theme: usize,
    tier: theme::Tier,
}

/// The size an overview's drawn at: its area, and its most rows of cues and whether phrases show.
type Size = (u16, u16, u16, bool);

/// What's kept of the playing track's waveforms from one frame to the next. The overview is kept
/// drawn twice, ahead of the playhead and behind it, for a frame to pick from, and the zoomed
/// view's columns are kept in a ring.
#[derive(Default)]
pub struct DeckWaves {
    drawn: Option<Drawn>,
    /// The cues as they're drawn: a track's own and the loop on the deck.
    pub(super) cues: Vec<Cue>,
    size: Option<Size>,
    ahead: Buffer,
    played: Buffer,
    /// The overview's rows over the waveform, which the playhead passes under.
    markers: u16,
    pub(super) zoom: ZoomRing,
}

/// What's playing, over its waveform. The waveform has no rows when there are too few.
pub(super) fn draw_strip(f: &mut Frame, app: &mut App, area: Rect) {
    let [line, wave] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
    f.render_widget(strip_line(app, line.width as usize), line);
    if wave.height >= 2 {
        app.areas.overview = wave;
        draw_overview(app, f.buffer_mut(), wave, 1, false);
    }
}

/// Forget what's kept if it was drawn from something else, and have the cues to draw.
pub(super) fn keep_waves(app: &mut App) {
    let mode = app.band_mode();
    let now = app.now.as_ref();
    let waves = now.map_or((0, 0), |n| {
        (n.overview_wave(mode).map_or(0, |(w, _)| w.len()), n.zoom_wave(mode).map_or(0, |(w, _)| w.len()))
    });
    let drawn = Drawn {
        track: now.map(|n| n.id),
        waves,
        bands: mode,
        phrases: now.map_or(0, |n| n.phrases.len()),
        cues: now.map_or(0, |n| n.analysis.cues.len()),
        looped: app.deck.looped.filter(|l| l.on).map(|l| (l.start as u32, l.end as u32)),
        theme: ptr::from_ref(theme::current()) as usize,
        tier: theme::tier(),
    };
    if app.waves.drawn != Some(drawn) {
        app.waves.drawn = Some(drawn);
        app.waves.cues = app.drawn_cues();
        app.waves.size = None;
        app.waves.zoom.forget();
    }
}

/// The overview at `area`, its played part behind the playhead, with `cue_rows` for its cues at
/// most and the phrases in its last row.
pub(super) fn draw_overview(app: &mut App, buf: &mut Buffer, area: Rect, cue_rows: u16, phrases: bool) {
    keep_waves(app);
    let size = (area.width, area.height, cue_rows, phrases);
    if app.waves.size != Some(size) {
        redraw_overview(app, area, cue_rows, phrases);
        app.waves.size = Some(size);
    }
    let progress = app.now.as_ref().map_or(0.0, |n| app.position().as_secs_f64() / n.duration.as_secs_f64().max(0.001));
    // In eighths of a cell, where the glyphs for them can be drawn.
    let steps = if icons::ascii() { 1.0 } else { 8.0 };
    let at = (progress.clamp(0.0, 1.0) * f64::from(area.width) * steps) as u32;
    let head = ((f64::from(at) / steps) as u16).min(area.width - 1);
    let eighth = if steps > 1.0 { (at % 8) as u8 } else { 0 };
    for y in 0..area.height {
        for x in 0..area.width {
            let from = if x < head { &app.waves.played } else { &app.waves.ahead };
            let Some(cell) = from.cell(Position::new(x, y)) else { continue };
            let Some(to) = buf.cell_mut((area.x + x, area.y + y)) else { continue };
            *to = cell.clone();
        }
    }
    let palette = theme::palette();
    let playhead = (palette.paint)(palette.playhead);
    let wave = app.waves.drawn.is_some_and(|d| d.waves.0 > 0);
    for y in app.waves.markers.min(area.height)..area.height {
        let (x, y) = (area.x + head, area.y + y);
        // The phrases' row moves a cell at a time, with the names under it left whole.
        if phrases && y + 1 == area.bottom() {
            palette.head_on_a_phrase(&mut buf[(x, y)]);
            continue;
        }
        if !wave {
            if let Some(cell) = buf.cell_mut((x, y)) {
                on_rail(cell, playhead);
            }
            continue;
        }
        let next = (head + 1 < area.width).then(|| buf[(x + 1, y)].clone());
        let (mut there, mut ahead) = (buf[(x, y)].clone(), next);
        // The cell behind the playhead's own colour, as the part of it already played.
        there.fg = app.waves.played.cell(Position::new(head, y - area.y)).map_or(there.fg, |c| c.fg);
        playhead_cells(&mut there, ahead.as_mut(), eighth, playhead);
        buf[(x, y)] = there;
        if let Some(ahead) = ahead.filter(|_| eighth > 0) {
            buf[(x + 1, y)] = ahead;
        }
    }
}

/// Draw the overview twice, as it is ahead of the playhead and behind it.
fn redraw_overview(app: &mut App, area: Rect, cue_rows: u16, with_phrases: bool) {
    let at = Rect { x: 0, y: 0, ..area };
    let (rows, row) = match with_phrases {
        true => (Rect { height: at.height - 1, ..at }, Some(Rect { y: at.height - 1, height: 1, ..at })),
        false => (at, None),
    };
    let palette = theme::palette();
    let mode = app.band_mode();
    let wave = app.now.as_ref().and_then(|n| n.overview_wave(mode)).map(|(w, _)| w);
    let duration_ms = app.now.as_ref().map_or(0.0, |n| n.duration.as_secs_f64() * 1000.0);
    let phrases: Vec<(f64, f64, String, [u8; 3])> = (app.now.iter().flat_map(|n| &n.phrases))
        .map(|p| (p.start_ms, p.end_ms, p.kind.name(), theme::current().phrase(p.kind)))
        .collect();
    let cues = &app.waves.cues;
    let mut buffers = [Buffer::empty(at), Buffer::empty(at)];
    let overview = |progress| WaveOverview { wave, progress, cues, duration_ms, cue_rows, palette };
    for (buffer, progress) in buffers.iter_mut().zip([0.0, 1.0]) {
        overview(progress).render(rows, buffer);
        if let Some(row) = row {
            PhraseStrip { phrases: &phrases, duration_ms, progress, palette }.render(row, buffer);
        }
    }
    let markers = overview(0.0).marker_rows(rows);
    let [ahead, played] = buffers;
    (app.waves.ahead, app.waves.played, app.waves.markers) = (ahead, played, markers);
}

/// The playhead on the rail a track with no waveform gets: a dot, on the rail's row.
fn on_rail(cell: &mut Cell, colour: Color) {
    if matches!(cell.symbol(), "─" | "━") {
        cell.set_symbol(icons::DOT.get()).set_fg(colour);
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

    right.push(Span::styled(format!(" {:.2}", now.bpm_at(pos_ms)), bold(theme::current().warning)));
    let key = now.key.map_or_else(|| t.key.clone(), |k| app.deck.notation.write(k));
    if !key.is_empty() {
        right.push(Span::styled(format!("  {key}"), bold(theme::current().info)));
    }
    let left_secs = (now.duration.as_secs_f64() - pos_ms / 1000.0).max(0.0);
    right.push(Span::styled(format!("  -{} ", clock(left_secs)), end_style(app).unwrap_or(text())));
    if let Some(chip) = loop_chip(app) {
        right.extend([chip, Span::raw(" ")]);
    }
    let room = width.saturating_sub(right.iter().map(Span::width).sum::<usize>() + TITLE);
    // Letters over the same letters on the waveform: beside a table, chips would shout.
    let cues = pad_chips(app, now, pos_ms, true);
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
        // The empty state says what comes first.
        None if app.empty() => {}
        None => spans.push(Span::styled("Pick a track and press enter.", dim())),
    }
    spans
}

#[cfg(test)]
mod tests;
