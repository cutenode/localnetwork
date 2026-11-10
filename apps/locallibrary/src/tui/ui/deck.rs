//! The Deck layout: the track's waveforms and numbers on the whole screen, as on a CDJ's display.
//! The tempo and key are the track playing's: the player has one deck, so it's the master, and
//! what the library says of tempo and key is against it.

use loclib_waveform::Zoom;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};

use crate::output::clock;
use crate::tui::app::{App, NowPlaying, Repeat, Vis};
use crate::tui::icons;
use crate::tui::keys::Notation;
use crate::tui::theme::{self, bold, dim, faint, fg, text};

use super::*;

/// From this tall, the head has its art, tempo frame and visualiser, and the panels show.
const ROOMY: u16 = 34;
const SPACIOUS: u16 = 28;
/// The head's rows where it's roomy: the large tempo and time, a row between, and their frame.
const TALL_HEAD: u16 = 9;
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
    fn of(height: u16, width: u16, has_cues: bool) -> Rows {
        let head = if height >= ROOMY { TALL_HEAD } else { 3 };
        let panels = if height >= ROOMY { ((height - 2) / 6).min(10) } else { 0 };
        let readout = if width >= ONE_READOUT { 1 } else { 2 };
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
    let rows = Rows::of(area.height, area.width, !app.waves.cues.is_empty());
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
    draw_readout(f, app, readout);
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
    match tall {
        true => draw_info(f, app, info),
        false => draw_brief(f, app, info),
    }
    draw_tempo(f, app, tempo);
    if vis_w > 0 {
        let inner = section_with(f, vis, Line::styled(format!("{} ", app.vis.name()), dim()));
        draw_visualiser(f, app, inner);
    }
}

/// What three rows have room for: the track, where the beat and phrase are, its cues.
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
    let pos_ms = app.position_ms();
    let title = if t.title.is_empty() { "Untitled" } else { t.title.as_str() };
    let mut lines = vec![
        Line::styled(title.to_string(), bold(theme::current().text)),
        Line::styled(t.artist.clone(), bold(theme::current().accent)),
    ];
    let mut beat = beat_spans(now, pos_ms);
    if let Some((_, p, bar)) = now.phrase_at(pos_ms) {
        beat.extend([
            Span::raw(" "),
            chip(p.kind.name().to_uppercase(), theme::current().phrase(p.kind)),
            Span::styled(format!(" {bar}/{}", p.bars), dim()),
        ]);
    }
    lines.push(Line::from(beat));
    f.render_widget(Paragraph::new(lines), area);
}

/// Where the beat is in the bar, in four squares, and the bar and beat.
pub(super) fn beat_spans(now: &NowPlaying, pos_ms: f64) -> Vec<Span<'static>> {
    let Some((_, n, bar)) = now.beat_at(pos_ms) else { return Vec::new() };
    let mut spans = Vec::with_capacity(5);
    for k in 1..=4u16 {
        let (sym, c) = match (k == n, n == 1) {
            (true, true) => (icons::SQUARE.get(), theme::current().accent),
            (true, false) => (icons::SQUARE.get(), theme::current().info),
            _ => (icons::HOLLOW.get(), theme::current().faint),
        };
        spans.push(Span::styled(sym, bold(c)));
    }
    spans.push(Span::styled(format!(" {bar}.{n}"), dim()));
    spans
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
        return TEMPO + 4;
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

/// The tempo and the time left, large, with the key loud beside them. They're in a frame as a
/// CDJ-3000X frames the master's where there's room. Where only three rows are left and the width's
/// narrow too, the time drops to a line beside the digits.
fn draw_tempo(f: &mut Frame, app: &App, area: Rect) {
    let framed = area.height >= TALL_HEAD;
    let inner = if framed {
        let frame = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(faint())
            .padding(Padding::horizontal(1))
            .title(Line::styled(" BPM ", dim()));
        let inner = frame.inner(area);
        f.render_widget(frame, area);
        inner
    } else {
        area
    };
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
    let pos_ms = app.position_ms();
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
    let left = (now.duration.as_secs_f64() - app.position().as_secs_f64()).max(0.0);
    Some(Span::styled(format!("-{}", clock(left)), end_style(app).unwrap_or(bold(theme::current().text))))
}

/// How the time left is drawn while the end of the track is near: `None` before it is. It blinks
/// twice a second while playing, and with less motion it's only its colour.
pub(super) fn end_style(app: &App) -> Option<Style> {
    let now = app.now.as_ref()?;
    let left = now.duration.as_secs_f64() - app.position().as_secs_f64();
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

/// From this wide the readout fits on one row, and from this wide it spells out what its numbers
/// are.
const ONE_READOUT: u16 = 76;
const WORDY_READOUT: u16 = 100;

/// Where the readout's columns end, in terse and in spelled out, so nothing moves as its numbers
/// change: the time, the next cue and the next phrase.
const STOPS: [[usize; 3]; 2] = [[15, 25, 41], [15, 35, 65]];

/// `spans` padded out to `to` columns.
fn pad_to(spans: &mut Vec<Span<'static>>, to: usize) {
    let used: usize = spans.iter().map(Span::width).sum();
    if used < to {
        spans.push(Span::raw(" ".repeat(to - used)));
    }
}

/// Under the waveforms, in columns: the time, how far the next cue and phrase are in bars, and at
/// the right how the deck's set. On two rows where one won't take it.
fn draw_readout(f: &mut Frame, app: &App, area: Rect) {
    let width = area.width as usize;
    let wordy = area.width >= WORDY_READOUT;
    let [to_time, to_cue, to_phrase] = STOPS[usize::from(wordy)];
    let mut left = transport(app);
    pad_to(&mut left, to_time);
    if let Some(now) = &app.now {
        let pos_ms = app.position_ms();
        let away = |to_ms: f64| match (now.bars_to(pos_ms, to_ms), wordy) {
            (Some((bars, beats)), true) => format!(" {bars}.{beats} bars"),
            (Some((bars, beats)), false) => format!(" {bars}.{beats}"),
            (None, _) => format!(" {}", clock((to_ms - pos_ms) / 1000.0)),
        };
        if let Some(c) = now.next_cue(pos_ms) {
            let mark = loclib_waveform::cue_letter(c).map_or(icons::DIAMOND.get().to_string(), |l| l.to_string());
            if wordy {
                left.push(Span::styled("cue ", faint()));
            }
            left.extend([chip(mark, theme::palette().cue_color(c)), Span::styled(away(f64::from(c.start_ms)), text())]);
        }
        pad_to(&mut left, to_cue);
        if let Some((i, _, _)) = now.phrase_at(pos_ms) {
            if wordy {
                left.push(Span::styled("phrase ", faint()));
            }
            match now.phrases.get(i + 1) {
                Some(next) => left.extend([
                    chip(next.kind.name(), theme::current().phrase(next.kind)),
                    Span::styled(away(next.start_ms), text()),
                ]),
                None => left.push(Span::styled("last", dim())),
            }
        }
        pad_to(&mut left, to_phrase);
    }
    if end_style(app).is_some() {
        left.extend([chip("END".into(), theme::current().critical), Span::raw(" ")]);
    }
    if let Some(c) = loop_chip(app) {
        left.push(c);
    }
    let lw: usize = left.iter().map(Span::width).sum();
    let mut right = deck_states(app, true);
    if area.height > 1 {
        let [first, second] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);
        f.render_widget(Line::from(fit(left, width)), first);
        return f.render_widget(spread(Vec::new(), right, area.width), second);
    }
    // What's off goes before what's on.
    if lw + right.iter().map(Span::width).sum::<usize>() >= width {
        right = deck_states(app, false);
    }
    let rw: usize = right.iter().map(Span::width).sum();
    f.render_widget(spread(fit(left, width.saturating_sub(rw + 1)), right, area.width), area);
}

/// The readout's first column: playing or not, how far in, and how long the track is.
fn transport(app: &App) -> Vec<Span<'static>> {
    let pos = app.position().as_secs_f64();
    let (dur, have) = app.now.as_ref().map_or((0.0, false), |n| (n.duration.as_secs_f64(), true));
    let glyph = if !have {
        Span::styled(icons::SQUARE.spaced(), faint())
    } else if app.is_playing() {
        Span::styled(icons::PLAY.spaced(), bold(theme::current().accent))
    } else {
        Span::styled(icons::PAUSE.spaced(), bold(theme::current().warning))
    };
    vec![
        glyph,
        Span::styled(clock(if have { pos } else { 0.0 }), bold(theme::current().text)),
        Span::styled(format!(" / {}", clock(dur)), faint()),
    ]
}

/// How the deck's set, for the readout's right side: the waveform's colours, the volume and the
/// modes, those that are off only with `all`.
fn deck_states(app: &App, all: bool) -> Vec<Span<'static>> {
    let on = fg(theme::current().info);
    let mut spans = vec![
        Span::styled(drawn_bands(app), on),
        Span::styled("  vol ", faint()),
        Span::styled(format!("{:.0}%", app.volume * 100.0), text()),
    ];
    let mut state = |shown: String, style| spans.extend([Span::raw("  "), Span::styled(shown, style)]);
    match app.repeat {
        Repeat::Off if all => state(format!("{} off", icons::REPEAT.get()), faint()),
        Repeat::Off => {}
        Repeat::All => state(format!("{} all", icons::REPEAT.get()), on),
        Repeat::One => state(format!("{} one", icons::REPEAT.get()), on),
    }
    if app.shuffle {
        state(format!("{} on", icons::MIX.get()), on);
    } else if all {
        state(format!("{} off", icons::MIX.get()), faint());
    }
    if app.deck.quantize {
        state("Q on".into(), fg(theme::current().warning));
    } else if all {
        state("Q off".into(), faint());
    }
    if app.deck.single {
        state("single".into(), on);
    }
    if app.deck.auto_cue {
        state("auto cue".into(), on);
    }
    spans
}

/// How the waveform's coloured, as rekordbox says it.
fn drawn_bands(app: &App) -> &'static str {
    let mode = app.band_mode();
    let drawn = app.now.as_ref().and_then(|n| n.zoom_wave(mode)).map(|(_, m)| m);
    drawn.flatten().unwrap_or(mode).title()
}

/// The waveform going past the playhead, or why it isn't there.
fn draw_zoom(f: &mut Frame, app: &mut App, area: Rect, cue_rows: u16) {
    keep_waves(app);
    let (position_secs, visible_secs, mode) = (app.position().as_secs_f64(), app.deck_secs, app.band_mode());
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
                None => "deck: play a track",
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
