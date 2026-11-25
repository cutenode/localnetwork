//! Player mode: the track's waveforms and numbers on the whole screen, as on a CDJ's display.
//! The tempo and key are the track playing's: the player has one deck, so it's the master, and
//! what the library says of tempo and key is against it.

use loclib_waveform::Zoom;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::output::clock;
use crate::tui::app::{App, NowPlaying, Repeat, Vis};
use crate::tui::icons;
use crate::tui::keys::Notation;
use crate::tui::theme::{self, bold, dim, faint, fg, text};

use super::*;

/// From this tall, the head has its art, titled tempo and visualiser, and the panels show.
const ROOMY: u16 = 34;
const SPACIOUS: u16 = 28;
/// The head's rows where it's roomy: the tempo's title, the large tempo and time and a row between.
/// The art and the visualiser are as tall.
const TALL_HEAD: u16 = 8;
/// The zoomed view stops growing here, and the overview has the rest.
const ZOOM_MOST: u16 = 28;

/// The numbers' font, three rows of each character: half blocks, at a weight a glance can read, or
/// with ASCII seven segments.
fn big_glyph(c: char) -> [&'static str; 3] {
    if icons::ascii() {
        return match c {
            '0' => [" _ ", "| |", "|_|"],
            '1' => ["   ", "  |", "  |"],
            '2' => [" _ ", " _|", "|_ "],
            '3' => [" _ ", " _|", " _|"],
            '4' => ["   ", "|_|", "  |"],
            '5' => [" _ ", "|_ ", " _|"],
            '6' => [" _ ", "|_ ", "|_|"],
            '7' => [" _ ", "  |", "  |"],
            '8' => [" _ ", "|_|", "|_|"],
            '9' => [" _ ", "|_|", " _|"],
            '.' => [" ", " ", "."],
            ':' => [" ", ".", "."],
            '-' => ["   ", " _ ", "   "],
            _ => ["   ", "   ", "   "],
        };
    }
    match c {
        '0' => ["█▀█", "█ █", "█▄█"],
        '1' => ["▀█ ", " █ ", "▄█▄"],
        '2' => ["▀▀█", "█▀▀", "█▄▄"],
        '3' => ["▀▀█", " ▀█", "▄▄█"],
        '4' => ["█ █", "▀▀█", "  █"],
        '5' => ["█▀▀", "▀▀█", "▄▄█"],
        '6' => ["█▀▀", "█▀█", "█▄█"],
        '7' => ["▀▀█", "  █", "  █"],
        '8' => ["█▀█", "█▀█", "█▄█"],
        '9' => ["█▀█", "▀▀█", "▄▄█"],
        '.' => [" ", " ", "▄"],
        ':' => ["▄", " ", "▀"],
        '-' => ["   ", "▀▀▀", "   "],
        _ => ["   ", "   ", "   "],
    }
}

/// `text` in the big font, a line for each of its rows.
fn big(text: &str, style: Style) -> [Line<'static>; 3] {
    std::array::from_fn(|row| {
        let mut line = String::with_capacity(text.len() * 12);
        for c in text.chars() {
            line.push_str(big_glyph(c)[row]);
            line.push(' ');
        }
        Line::styled(line, style)
    })
}

/// How the rows go, top to bottom.
struct Rows {
    head: u16,
    zoom: u16,
    zoom_cues: u16,
    overview: u16,
    overview_cues: u16,
    panels: u16,
    readout: u16,
}

impl Rows {
    /// The head, the panels and the readout take what they need, and the overview the least it
    /// can. The zoomed view has what's left, up to [`ZOOM_MOST`], then the overview grows.
    fn of(height: u16, has_cues: bool) -> Rows {
        let head = if height >= ROOMY { TALL_HEAD } else { 3 };
        let panels = if height >= ROOMY { ((height - 2) / 6).min(10) } else { 0 };
        // Where the head's tall the readout is in it, and its row keeps the panels off the status line.
        let readout = 1;
        let cues = if height >= SPACIOUS { 2 } else { 1 };
        let zoom_cues = if has_cues { cues } else { 0 };
        let least_overview = match height {
            ROOMY.. => 5,
            20.. => 3,
            _ => 2,
        };
        // The overview's markers and phrases are rows it always has.
        let waves = height.saturating_sub(head + panels + readout + cues + 1);
        let zoom = waves.saturating_sub(least_overview).min(ZOOM_MOST);
        // A mirrored waveform wants an even number of rows, the ruler having one.
        let odd = zoom.saturating_sub(1 + zoom_cues) % 2;
        let overview = waves - zoom + odd + cues + 1;
        Rows { head, zoom: zoom - odd, zoom_cues, overview, overview_cues: cues, panels, readout }
    }

    /// With no detail waveform, a line says so and the panels get the rest of its rows.
    fn without_zoom(self) -> Rows {
        Rows { zoom: 1, zoom_cues: 0, panels: self.panels + self.zoom - 1, ..self }
    }
}

pub(super) fn draw_deck(f: &mut Frame, app: &mut App, area: Rect) {
    keep_waves(app);
    // One edge for everything on the deck, the one the header's text keeps.
    let area = area.inner(Margin::new(1, 0));
    let rows = Rows::of(area.height, !app.waves.cues.is_empty());
    // Only once the analysis is read, so the rows don't jump while it loads.
    let mode = app.band_mode();
    let no_zoom = app.now.as_ref().is_some_and(|n| !n.loading && n.zoom_wave(mode).is_none());
    let rows = if no_zoom { rows.without_zoom() } else { rows };
    let [head, zoom, overview, panels, readout] = Layout::vertical([
        Constraint::Length(rows.head),
        Constraint::Length(rows.zoom),
        Constraint::Length(rows.overview),
        Constraint::Length(rows.panels),
        Constraint::Length(rows.readout),
    ])
    .areas(area);
    draw_head(f, app, head);
    draw_zoom(f, app, zoom, rows.zoom_cues);
    let phrases = app.now.as_ref().is_some_and(|n| !n.phrases.is_empty());
    app.areas.overview = overview;
    draw_overview(app, f.buffer_mut(), overview, rows.overview_cues, phrases);
    if panels.height > 0 {
        draw_details(f, app, panels);
    }
    if rows.head < TALL_HEAD {
        draw_readout(f, app, readout);
    }
}

/// The art, what's known of the track, a visualiser where there's room, and the tempo at the right
/// edge.
fn draw_head(f: &mut Frame, app: &mut App, area: Rect) {
    let tall = area.height >= TALL_HEAD;
    let tempo_w = tempo_width(app, area);
    let room = area.width.saturating_sub(tempo_w + 1);
    let art_w = if room >= area.height * 2 + 2 + BRIEF && app.shows_art() { area.height * 2 } else { 0 };
    let rest = room.saturating_sub(art_w + 2);
    let vis_w = if tall && app.vis != Vis::Deck && rest >= 76 { (rest / 3).min(56) } else { 0 };
    let [art, info, vis, _, tempo] = Layout::horizontal([
        Constraint::Length(if art_w > 0 { art_w + 2 } else { 0 }),
        Constraint::Min(0),
        Constraint::Length(vis_w),
        Constraint::Length(1),
        Constraint::Length(tempo_w),
    ])
    .areas(area);
    if art_w > 0 {
        draw_art(f, app, Rect { width: art_w, ..art });
    }
    if tall {
        // Playing or not and what's next sit under the track's names, level with the art's foot.
        let [about, readout] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(info);
        draw_info(f, app, about);
        draw_readout(f, app, readout);
    } else {
        draw_brief(f, app, info);
    }
    draw_tempo(f, app, tempo);
    if vis_w > 0 {
        let inner = section_with(f, vis, Line::styled(format!("{} ", app.vis.name()), dim()));
        draw_visualiser(f, app, inner);
    }
}

/// What three rows have room for: the track and what it's from. Where it is is the readout's.
fn draw_brief(f: &mut Frame, app: &App, area: Rect) {
    let Some(now) = &app.now else {
        let line = app.resumable().map_or_else(
            || Line::styled("Pick a track and press enter.", dim()),
            |(t, at)| {
                Line::from(vec![
                    Span::styled("space", theme::key()),
                    Span::styled(
                        format!(" carries on with {} from {}", t.display_name(), clock(at.as_secs_f64())),
                        dim(),
                    ),
                ])
            },
        );
        let lines = vec![Line::styled("Nothing playing", bold(theme::current().text)), line];
        return f.render_widget(Paragraph::new(lines), area);
    };
    let t = &now.track;
    let title = if t.title.is_empty() { "Untitled" } else { t.title.as_str() };
    let mut lines = vec![Line::styled(title.to_string(), bold(theme::current().text)), artist_line(now)];
    lines.extend(about_line(now));
    f.render_widget(Paragraph::new(lines), area);
}

/// How wide the tempo's large digits are, the key's block, and the two with a column between.
const DIGITS: u16 = 22;
const KEY: u16 = 8;
const TEMPO: u16 = DIGITS + 1 + KEY;
/// "BPM" on the digits' baseline, where there's no frame to say it.
const UNIT: u16 = 3;
/// What the art leaves the track's name.
const BRIEF: u16 = 24;
/// From this wide the time left goes beside the tempo in large digits on a short head too.
const WIDE_TEMPO: u16 = 76;

/// How much of `area` the tempo takes: the digits and key, in a frame where it's tall, and with the
/// time left beside them where it isn't but it's wide. It's sized for the track's whole length, so
/// nothing moves as the time counts down.
fn tempo_width(app: &App, area: Rect) -> u16 {
    if area.height >= TALL_HEAD {
        return TEMPO;
    }
    if area.width < WIDE_TEMPO {
        return TEMPO;
    }
    let longest = clock(app.now.as_ref().map_or(0.0, |n| n.duration.as_secs_f64()));
    let time: u16 = longest.chars().map(|c| big_glyph(c)[0].chars().count() as u16 + 1).sum();
    // The unit, the time's sign, and a column more to the key than the font leaves after a
    // character.
    DIGITS + 1 + UNIT + 2 + 4 + time + 1 + KEY
}

/// The tempo and the time left, large, with the key loud beside them. They're under a title, as the
/// visualiser is, where there's room. Where only three rows are left and the width's
/// narrow too, the time drops to a line beside the digits.
fn draw_tempo(f: &mut Frame, app: &App, area: Rect) {
    let framed = area.height >= TALL_HEAD;
    let inner = if framed { section_with(f, area, Line::styled("BPM ", dim())) } else { area };
    let beside = [Constraint::Length(DIGITS), Constraint::Length(1), Constraint::Min(0)];
    let (digits, time, key) = if framed {
        let [top, _, time] =
            Layout::vertical([Constraint::Length(3), Constraint::Length(1), Constraint::Length(3)]).areas(inner);
        let [digits, _, key] = Layout::horizontal(beside).areas(top);
        (digits, time, key)
    } else if inner.width > TEMPO {
        let [digits, _, unit, _, time, key] = Layout::horizontal([
            Constraint::Length(DIGITS),
            Constraint::Length(1),
            Constraint::Length(UNIT),
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(KEY),
        ])
        .areas(inner);
        let baseline = Rect { y: unit.bottom().saturating_sub(1), height: unit.height.min(1), ..unit };
        f.render_widget(Line::styled("BPM", dim()), baseline);
        (digits, time, key)
    } else {
        let [digits, _, side] = Layout::horizontal(beside).areas(inner);
        (digits, Rect::default(), side)
    };
    let pos_ms = app.heard_ms();
    let bpm = app.now.as_ref().map_or(0.0, |n| n.bpm_at(pos_ms));
    // Signed pitch goes first here, once a track's speed can change. Until then a figure would
    // suggest a control that isn't there.
    let (shown, style) = match bpm > 0.0 {
        true => (format!("{bpm:>6.2}"), bold(theme::current().warning)),
        false => ("  -.--".to_string(), faint()),
    };
    f.render_widget(Paragraph::new(big(&shown, style).to_vec()), digits);
    if time.is_empty() {
        let mut lines = Vec::with_capacity(3);
        lines.extend(app.now.as_ref().map(|now| Line::from(key_chip(app, now))));
        lines.extend(time_left(app).map(Line::from));
        lines.push(Line::styled("BPM", dim()));
        return f.render_widget(Paragraph::new(lines), key);
    }
    let shown = time_left(app).unwrap_or_else(|| Span::styled("-:--", faint()));
    f.render_widget(Paragraph::new(big(&shown.content, shown.style).to_vec()), time);
    if let Some(now) = &app.now {
        let chip = key_chip(app, now);
        let row = Line::raw(chip.content.trim().to_string());
        let block = Paragraph::new(vec![Line::default(), row]).style(chip.style).alignment(Alignment::Center);
        f.render_widget(block, key);
    }
}

/// The playing track's key as a chip: in the notation asked for and as a note, or as rekordbox
/// stored it, and its own colour if the key palette gives it one.
fn key_chip(app: &App, now: &NowPlaying) -> Span<'static> {
    let Some(k) = now.key else {
        return Span::styled(now.track.key.clone(), bold(theme::current().info));
    };
    let shown = match app.deck.notation {
        Notation::Musical => k.note(),
        notation => format!("{} {}", notation.write(k), k.note()),
    };
    chip(shown, app.deck.key_palette.colour(k).unwrap_or(theme::current().info))
}

/// `-3:42`, warning of the end of the track.
fn time_left(app: &App) -> Option<Span<'static>> {
    let now = app.now.as_ref()?;
    let left = (now.duration.as_secs_f64() - app.heard().as_secs_f64()).max(0.0);
    Some(Span::styled(format!("-{}", clock(left)), end_style(app).unwrap_or(bold(theme::current().text))))
}

/// How the time left is drawn while the end of the track is near: `None` before it is. It blinks
/// twice a second while playing, and with less motion it's only its colour.
pub(super) fn end_style(app: &App) -> Option<Style> {
    let now = app.now.as_ref()?;
    let left = now.duration.as_secs_f64() - app.heard().as_secs_f64();
    if app.deck.end_warning == 0 || left > f64::from(app.deck.end_warning) || now.duration.is_zero() {
        return None;
    }
    let now_lit = lit(theme::reduced_motion(), app.is_playing(), app.started.elapsed().subsec_millis());
    Some(if now_lit { bold(theme::current().critical) } else { dim() })
}

/// Whether a warning is lit, `ms` into a second.
fn lit(less_motion: bool, playing: bool, ms: u32) -> bool {
    less_motion || !playing || ms < 500
}

/// From this wide the readout spells out what its numbers are.
const WORDY_READOUT: u16 = 60;

/// The gap between the readout's phrases and its next cue. Their numbers are padded, so the cue
/// only moves when the phrase changes.
const GAP: &str = "   ";

/// `spans` padded out to `to` columns.
fn pad_to(spans: &mut Vec<Span<'static>>, to: usize) {
    let used: usize = spans.iter().map(Span::width).sum();
    if used < to {
        spans.push(Span::raw(" ".repeat(to - used)));
    }
}

/// Under the track's names (or the waveforms, where the head's short), in columns: playing or not,
/// and how far the next cue and phrase are in bars. The time is the large one beside the tempo, and
/// the modes that are on are the status line's.
fn draw_readout(f: &mut Frame, app: &mut App, area: Rect) {
    let width = area.width as usize;
    let wordy = area.width >= WORDY_READOUT;
    let mut left = transport(app);
    pad_to(&mut left, 2);
    if let Some(now) = &app.now {
        let pos_ms = app.heard_ms();
        // How far, right-aligned so the name after it doesn't move as it counts down.
        let away = |to_ms: f64| match (now.bars_to(pos_ms, to_ms), wordy) {
            (Some((bars, beats)), true) => {
                format!("{:>4} {}", format!("{bars}.{beats}"), if (bars, beats) == (1, 0) { "bar " } else { "bars" })
            }
            (Some((bars, beats)), false) => format!("{:>4}", format!("{bars}.{beats}")),
            (None, true) => format!("{:>4}     ", clock((to_ms - pos_ms) / 1000.0)),
            (None, false) => format!("{:>4}", clock((to_ms - pos_ms) / 1000.0)),
        };
        let till = |what: &'static str| Span::styled(if wordy { what } else { " " }, faint());
        left.extend(beat_squares(now, pos_ms));
        // The phrase playing beside the next one, then the next cue. A column that has nothing to
        // say takes no room: what's after it moves up.
        if let Some((i, p, bar)) = now.phrase_at(pos_ms) {
            left.extend(phrase_chip(p, bar));
            match now.phrases.get(i + 1) {
                Some(next) => left.extend([
                    Span::styled(away(next.start_ms), text()),
                    till(" till "),
                    chip(next.kind.name(), theme::current().phrase(next.kind)),
                ]),
                None => left.push(Span::styled(if wordy { "   last phrase" } else { "   last" }, dim())),
            }
            left.push(Span::raw(GAP));
        }
        if let Some(c) = now.next_cue(pos_ms) {
            let mark = loclib_waveform::cue_letter(c).map_or(icons::DIAMOND.get().to_string(), |l| l.to_string());
            left.extend([
                Span::styled(away(f64::from(c.start_ms)), text()),
                till(" till cue "),
                chip(mark, theme::palette().cue_color(c)),
            ]);
            left.push(Span::raw(GAP));
        }
    }
    if end_style(app).is_some() {
        left.extend([chip("END".into(), theme::current().critical), Span::raw(" ")]);
    }
    // The loop chip is a button: a click ends the loop. It's where it's drawn whole.
    let chip_at: usize = left.iter().map(Span::width).sum();
    let chip_w = loop_chip(app).map(|c| {
        let w = c.width();
        left.push(c);
        w
    });
    if let Some(w) = chip_w.filter(|w| chip_at + w < width) {
        app.areas.loop_chip = Rect { x: area.x + chip_at as u16, y: area.y, width: w as u16, height: 1 };
    }
    f.render_widget(Line::from(fit(left, width)), area);
}

/// Where the beat is in the bar, in four squares: the downbeat in the accent, the others in blue.
pub(super) fn beat_squares(now: &NowPlaying, pos_ms: f64) -> Vec<Span<'static>> {
    let Some((_, n, _)) = now.beat_at(pos_ms) else { return Vec::new() };
    let mut spans: Vec<Span> = (1..=4u16)
        .map(|k| match (k == n, n == 1) {
            (true, true) => Span::styled(icons::SQUARE.get(), bold(theme::current().accent)),
            (true, false) => Span::styled(icons::SQUARE.get(), bold(theme::current().info)),
            _ => Span::styled(icons::HOLLOW.get(), bold(theme::current().faint)),
        })
        .collect();
    spans.push(Span::raw(" "));
    spans
}

/// The readout's first column: playing or not.
fn transport(app: &App) -> Vec<Span<'static>> {
    let glyph = if app.now.is_none() {
        Span::styled(icons::SQUARE.spaced(), faint())
    } else if app.is_playing() {
        Span::styled(icons::PLAY.spaced(), bold(theme::current().accent))
    } else {
        Span::styled(icons::PAUSE.spaced(), bold(theme::current().warning))
    };
    vec![glyph]
}

/// The volume when it's turned down, and the modes that are on, for the status line while the deck
/// shows. What's off, and the waveform's colours (which `w` says as they change), aren't said.
pub(super) fn deck_states(app: &App) -> Vec<Span<'static>> {
    let on = fg(theme::current().info);
    let mut spans = Vec::new();
    let mut state = |shown: String, style| {
        if !spans.is_empty() {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(shown, style));
    };
    if app.volume < 0.995 {
        state(format!("vol {:.0}%", app.volume * 100.0), text());
    }
    match app.repeat {
        Repeat::Off => {}
        Repeat::All => state(format!("{} all", icons::REPEAT.get()), on),
        Repeat::One => state(format!("{} one", icons::REPEAT.get()), on),
    }
    if app.shuffle {
        state(format!("{} on", icons::MIX.get()), on);
    }
    if app.deck.quantize {
        state("Q on".into(), fg(theme::current().warning));
    }
    if app.deck.single {
        state("single".into(), on);
    }
    if app.deck.auto_cue {
        state("auto cue".into(), on);
    }
    spans
}

/// The waveform going past the playhead, or why it isn't there.
fn draw_zoom(f: &mut Frame, app: &mut App, area: Rect, cue_rows: u16) {
    keep_waves(app);
    let (position_secs, visible_secs, mode) = (app.heard().as_secs_f64(), app.deck_secs, app.band_mode());
    let palette = theme::palette();
    match app.now.as_ref().and_then(|n| Some((n, n.zoom_wave(mode)?.0))) {
        Some(_) if area.height < 3 => {}
        Some((now, wave)) => {
            let beats = &now.analysis.beats;
            let cues = &app.waves.cues;
            let bar_numbers = true;
            let zoom = Zoom { wave, position_secs, visible_secs, beats, cues, cue_rows, bar_numbers, palette };
            app.waves.zoom.draw(&zoom, area, f.buffer_mut());
        }
        None => {
            let msg = match &app.now {
                Some(n) if n.loading => "reading the analysis…",
                Some(_) => "no detail waveform for this track",
                None => "player mode: play a track",
            };
            let y = area.y + area.height / 2;
            f.render_widget(
                Paragraph::new(Line::styled(msg, dim())).alignment(Alignment::Center),
                Rect { y, height: area.height.min(1), ..area },
            );
        }
    }
}

#[cfg(test)]
mod tests;
