//! The full-screen deck, with the art, the track's info, the waveforms and the visualisers.

use loclib_waveform::{CueLane, Detail, Overview, PhraseStrip, Pixels, Scope, Spectrum, bar_layout, cue_letter, vinyl};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use crate::output::{bytes, clock, plural};
use crate::tui::analyzer::Bands;
use crate::tui::app::{App, Matches, NowPlaying, Repeat, Vis};
use crate::tui::icons;
use crate::tui::keys::Relation as KeyRelation;
use crate::tui::theme::{self, bold, dim, faint, fg, text};

use super::*;

pub(super) fn draw_player(f: &mut Frame, app: &mut App, area: Rect) {
    let playing = app.is_playing();
    let b = block(
        Line::from(vec![
            Span::raw(" "),
            Span::styled(if playing { icons::PLAY.get() } else { icons::SQUARE.get() }, bold(theme::current().accent)),
            Span::styled(" Now playing ", bold(theme::current().text)),
        ]),
        false,
    )
    .title_top(Line::from(vec![Span::styled(" f ", theme::key()), Span::styled("back ", dim())]).right_aligned());
    let inner = b.inner(area);
    f.render_widget(b, area);
    if inner.height < 3 || inner.width < 20 {
        return;
    }

    let art_rows = (inner.height / 3).clamp(6, 16);
    // One more row under the overview for the phrase strip, when there are phrases.
    let strip = u16::from(app.now.as_ref().is_some_and(|n| !n.phrases.is_empty()));
    let [head, _, visual, overview, progress] = Layout::vertical([
        Constraint::Length(art_rows),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(5 + strip),
        Constraint::Length(1),
    ])
    .areas(inner);

    // Artwork as a square: cells are about twice as tall as they are wide. It sits beside the head;
    // the waveform and clock span the width.
    let art_cols = (head.height * 2).min(inner.width / 3);
    let art_area = Rect { x: inner.x, y: inner.y, width: art_cols, height: art_cols / 2 };
    let rest_x = inner.x + art_cols + 2;
    let head = Rect { x: rest_x, width: (head.x + head.width).saturating_sub(rest_x), ..head };
    let info_w = (head.width * 2 / 5).clamp(28.min(head.width), 52);
    let [info, _, beside] =
        Layout::horizontal([Constraint::Length(info_w), Constraint::Length(2), Constraint::Min(0)]).areas(head);
    app.areas.overview = overview;

    draw_art(f, app, art_area);
    draw_info(f, app, info);
    if beside.width >= 16 {
        let inner = section(f, beside, "mirror");
        draw_spectrum(f, app, inner, true);
    }
    // Wide enough: track details and mixing suggestions beside the visualisers.
    if visual.width >= 110 {
        let side_w = (visual.width / 3).min(48);
        let [wall, _, side] =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(2), Constraint::Length(side_w)]).areas(visual);
        draw_wall(f, app, wall);
        draw_details(f, app, side);
    } else {
        draw_wall(f, app, visual);
    }

    let pos = app.position();
    let cues = app.drawn_cues();
    match &app.now {
        Some(now) => {
            let dur = now.duration.as_secs_f64().max(0.001);
            let progress_frac = (pos.as_secs_f64() / dur) as f32;
            let (overview, strip) = if !now.phrases.is_empty() {
                let [o, s] = Layout::vertical([Constraint::Min(2), Constraint::Length(1)]).areas(overview);
                (o, Some(s))
            } else {
                (overview, None)
            };
            app.areas.overview = overview;
            f.render_widget(
                Overview {
                    wave: now.analysis.preview.as_ref(),
                    progress: progress_frac,
                    cues: &cues,
                    duration_ms: dur * 1000.0,
                    palette: theme::palette(),
                },
                overview,
            );
            if let Some(strip) = strip {
                let phrases: Vec<(f64, f64, String, [u8; 3])> = now
                    .phrases
                    .iter()
                    .map(|p| (p.start_ms, p.end_ms, p.kind.name(), theme::current().phrase(p.kind)))
                    .collect();
                f.render_widget(
                    PhraseStrip {
                        phrases: &phrases,
                        duration_ms: dur * 1000.0,
                        progress: progress_frac,
                        palette: theme::palette(),
                    },
                    strip,
                );
            }
        }
        None => {
            f.render_widget(
                Overview { wave: None, progress: 0.0, cues: &[], duration_ms: 0.0, palette: theme::palette() },
                overview,
            );
        }
    }
    draw_progress(f, app, progress);
}

fn draw_art(f: &mut Frame, app: &mut App, area: Rect) {
    if area.width < 2 || area.height < 1 {
        return;
    }
    let t = app.started.elapsed().as_secs_f32();
    let playing = app.is_playing();
    match app.now.as_mut().and_then(|n| n.art.as_mut()) {
        Some(art) => {
            let px = art.pixels(area.width, area.height);
            f.render_widget(Pixels { width: area.width, pixels: px, palette: theme::palette() }, area);
        }
        None => {
            // 33⅓ rpm, in the light streak's terms.
            let angle = if playing && !theme::reduced_motion() { t * std::f32::consts::TAU * 0.555 } else { 0.6 };
            let label = app
                .now
                .as_ref()
                .and_then(|n| n.track.color)
                .map_or(theme::current().accent, |c| theme::current().track_color(c));
            let px = vinyl(area.width, area.height, angle, label, theme::current().vinyl);
            f.render_widget(Pixels { width: area.width, pixels: &px, palette: theme::palette() }, area);
        }
    }
}

fn chip(s: String, c: [u8; 3]) -> Span<'static> {
    Span::styled(format!(" {s} "), theme::chip(c))
}

/// The loop playing, as a chip.
pub(super) fn loop_chip(app: &App) -> Option<Span<'static>> {
    let (_, beats) = app.loop_playing()?;
    Some(chip(format!("LOOP {}", beats.map_or("on".into(), crate::tui::deck::beats_label)), theme::current().loop_cue))
}

/// The track's hot cues in their order, each a chip, the one the playhead last passed underlined;
/// with the pads on, each with the key that plays it before it.
pub(super) fn pad_chips(app: &App, now: &NowPlaying, pos_ms: f64) -> Vec<Span<'static>> {
    let cues = &now.analysis.cues;
    let mut hot: Vec<_> = cues.iter().filter(|c| c.hot_cue.is_some()).collect();
    hot.sort_by_key(|c| c.hot_cue);
    let current = cues.iter().filter(|c| c.hot_cue.is_some() && c.start_ms as f64 <= pos_ms).max_by_key(|c| c.start_ms);
    let pads = app.pads_on();
    let mut spans = Vec::new();
    for c in hot {
        if pads && let Some(h) = c.hot_cue.filter(|h| *h < crate::tui::deck::PADS) {
            spans.push(Span::styled((h + 1).to_string(), theme::key()));
        }
        let mut s = chip(cue_letter(c).unwrap_or('?').to_string(), theme::palette().cue_color(c));
        if current.is_some_and(|cur| std::ptr::eq(cur, c)) {
            s.style = s.style.add_modifier(Modifier::UNDERLINED);
        }
        spans.push(s);
    }
    spans
}

fn draw_info(f: &mut Frame, app: &App, area: Rect) {
    let Some(now) = &app.now else {
        let mut lines = vec![Line::raw(""), Line::styled("Nothing playing", bold(theme::current().text))];
        match app.resumable() {
            // Where the last run left off.
            Some((t, at)) => lines.push(Line::from(vec![
                Span::styled("space", theme::key()),
                Span::styled(format!(" carries on with {} from {}", t.display_name(), clock(at.as_secs_f64())), dim()),
            ])),
            None => lines.push(Line::styled("Pick a track and press enter.", dim())),
        }
        if let Some(e) = &app.audio_error {
            lines.push(Line::raw(""));
            lines.push(Line::styled(format!("No audio output: {e}"), fg(theme::current().critical)));
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
        return;
    };
    let t = &now.track;
    let mut lines = vec![
        Line::styled(if t.title.is_empty() { "Untitled".into() } else { t.title.clone() }, bold(theme::current().text)),
        Line::styled(t.artist.clone(), bold(theme::current().accent)),
    ];
    let mut about: Vec<String> =
        [&t.album, &t.remixer, &t.mix_name].into_iter().filter(|s| !s.is_empty()).cloned().collect();
    if !t.label.is_empty() {
        about.push(t.label.clone());
    }
    if t.year > 0 {
        about.push(t.year.to_string());
    }
    lines.push(Line::styled(about.join(" · "), dim()));

    let pos_ms = app.position().as_secs_f64() * 1000.0;
    let beat = now.beat_at(pos_ms);
    let bpm = beat.map(|(i, _, _)| now.analysis.beats[i].bpm).filter(|b| *b > 0.0).unwrap_or(t.bpm);
    let mut chips = vec![chip(format!("{bpm:.2} BPM"), theme::current().warning), Span::raw(" ")];
    // Camelot and note name, whichever rekordbox stored.
    match now.key {
        Some(k) => chips.push(chip(format!("{} {}", k.code(), k.note()), theme::current().info)),
        None if !t.key.is_empty() => chips.push(chip(t.key.clone(), theme::current().info)),
        None => {}
    }
    if now.key.is_some() || !t.key.is_empty() {
        chips.push(Span::raw(" "));
    }
    if let Some((_, n, bar)) = beat {
        for k in 1..=4u16 {
            let (sym, c) = match (k == n, n == 1) {
                (true, true) => (icons::SQUARE.get(), theme::current().accent),
                (true, false) => (icons::SQUARE.get(), theme::current().info),
                _ => (icons::HOLLOW.get(), theme::current().faint),
            };
            chips.push(Span::styled(sym, bold(c)));
        }
        chips.push(Span::styled(format!(" {bar}.{n}"), dim()));
    }
    // A loop playing before the phrase, which the strip under the overview shows too.
    if let Some(c) = loop_chip(app) {
        chips.extend([Span::raw("  "), c]);
    }
    let phrase = now.phrase_at(pos_ms);
    if let Some((_, p, bar)) = phrase {
        chips.push(Span::raw("  "));
        chips.push(chip(p.kind.name().to_uppercase(), theme::current().phrase(p.kind)));
        chips.push(Span::styled(format!(" {bar}/{}", p.bars), dim()));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(chips));
    if let Some((i, _, _)) = phrase {
        let mut spans = vec![Span::styled("phrase ", faint())];
        match now.phrases.get(i + 1) {
            Some(next) => {
                spans.push(Span::styled("next ", faint()));
                spans.push(Span::styled(next.kind.name(), bold(theme::current().phrase(next.kind))));
                spans.push(Span::styled(format!(" in {}", clock((next.start_ms - pos_ms) / 1000.0)), dim()));
            }
            None => spans.push(Span::styled("last", dim())),
        }
        if let Some(st) = &now.analysis.structure {
            spans.push(Span::styled(format!("  ·  {:?} mood", st.mood).to_lowercase(), faint()));
        }
        lines.push(Line::from(spans));
    }

    let ext = t.file_path.rsplit('.').next().unwrap_or("").to_uppercase();
    let mut fmt = vec![ext];
    if t.bitrate > 0 {
        fmt.push(format!("{} kbps", t.bitrate));
    }
    if t.sample_rate > 0 {
        fmt.push(format!("{:.1} kHz", t.sample_rate as f64 / 1000.0));
    }
    if t.sample_depth > 0 {
        fmt.push(format!("{}-bit", t.sample_depth));
    }
    if t.file_size > 0 {
        fmt.push(bytes(t.file_size));
    }
    fmt.push(now.drive.clone());
    lines.push(Line::styled(fmt.join(" · "), faint()));
    // What a plugin's analyzer worked out, drawn for what the drive lacks.
    if let Some(from) = &now.from_plugins {
        lines.push(Line::styled(from.clone(), dim()));
    }

    let cues = &now.analysis.cues;
    if !cues.is_empty() {
        let memory = cues.iter().filter(|c| c.hot_cue.is_none()).count();
        let mut spans = pad_chips(app, now, pos_ms);
        if memory > 0 {
            spans.push(Span::styled(format!(" {}{memory}", icons::DIAMOND.get()), fg(theme::current().memory_cue)));
        }
        let next =
            cues.iter().filter(|c| c.start_ms as f64 > pos_ms + 50.0).min_by_key(|c| (c.start_ms, c.hot_cue.is_none()));
        if let Some(c) = next {
            let mark = cue_letter(c).map_or(icons::DIAMOND.get().to_string(), |l| l.to_string());
            spans.push(Span::styled("  next ", faint()));
            spans.push(Span::styled(mark, bold(theme::palette().cue_color(c))));
            spans.push(Span::styled(format!(" {}", clock((c.start_ms as f64 - pos_ms) / 1000.0)), dim()));
        }
        lines.push(Line::from(spans));
    }
    let mut meta: Vec<Span> = Vec::new();
    let mut add = |s: Span<'static>| {
        if !meta.is_empty() {
            meta.push(Span::styled(" · ", faint()));
        }
        meta.push(s);
    };
    if let Some(c) = t.color {
        add(theme::colour_dot(c));
    }
    if !t.genre.is_empty() {
        add(Span::styled(t.genre.clone(), text()));
    }
    if t.rating > 0 {
        add(Span::styled(rating(t.rating), fg(theme::current().warning)));
    }
    if t.play_count > 0 {
        add(Span::styled(format!("played {}×", t.play_count), dim()));
    }
    if !t.date_added.is_empty() {
        add(Span::styled(format!("added {}", t.date_added), dim()));
    }
    if !meta.is_empty() {
        lines.push(Line::from(meta));
    }
    if !t.comment.trim().is_empty() {
        lines.push(Line::styled(format!("“{}”", t.comment.trim()), dim().add_modifier(Modifier::ITALIC)));
    }
    let credits: Vec<String> = [("composer", &t.composer), ("original", &t.original_artist), ("ISRC", &t.isrc)]
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{k} {v}"))
        .collect();
    if !credits.is_empty() {
        lines.push(Line::styled(credits.join(" · "), faint()));
    }
    let lines: Vec<Line> = lines.into_iter().take(area.height as usize).collect();
    f.render_widget(Paragraph::new(lines), area);
}

/// Full screen, beside the visualisers: what mixes into the playing track (to pick the next one
/// from), where it's filed, and what rekordbox's analysis found.
fn draw_details(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(now) = &app.now else { return };
    let mut about: Vec<Line> = Vec::new();
    let beats = &now.analysis.beats;
    if !beats.is_empty() {
        let (lo, hi) =
            beats.iter().filter(|b| b.bpm > 0.0).fold((f64::MAX, 0.0f64), |(l, h), b| (l.min(b.bpm), h.max(b.bpm)));
        let tempo = if hi - lo > 0.05 { format!("{lo:.1}–{hi:.1} BPM") } else { "steady tempo".into() };
        about.push(Line::from(vec![
            Span::styled(format!("{} beats · {} bars · ", beats.len(), beats.len().div_ceil(4)), text()),
            Span::styled(tempo, dim()),
        ]));
    }
    let cues = &now.analysis.cues;
    if !cues.is_empty() {
        let hot = cues.iter().filter(|c| c.hot_cue.is_some()).count();
        let loops = cues.iter().filter(|c| c.end_ms.is_some()).count();
        about.push(Line::styled(format!("{hot} hot cues ({loops} loops) · {} memory", cues.len() - hot), text()));
    }
    if !now.phrases.is_empty() {
        let mut spans = vec![Span::styled(format!("{} phrases ", now.phrases.len()), text())];
        for p in &now.phrases {
            spans.push(Span::styled(icons::PHRASE.get(), fg(theme::current().phrase(p.kind))));
        }
        about.push(Line::from(spans));
    }

    let lists = now.playlists.len().min(4);
    let [mixes, lists_area, about_area] = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(if lists > 0 { lists as u16 + 2 } else { 0 }),
        Constraint::Length(if about.is_empty() { 0 } else { about.len() as u16 + 2 }),
    ])
    .areas(area);

    let why = if now.loading {
        "finding matches…"
    } else if now.key.is_none() {
        "no key to match"
    } else if now.matches.is_none() {
        "open this drive to see suggestions"
    } else {
        "nothing in a compatible key and tempo"
    };
    // The queue's next track, marked when it's one of the matches.
    let q = &app.queue;
    let next = q.order.get(q.pos + 1).map(|&i| q.track(i).file_path.as_str());
    app.areas.matches = draw_matches(f, mixes, now.matches.as_ref(), next, why, &mut app.next_pick, true);

    if lists > 0 {
        draw_in_playlists(f, lists_area, &now.playlists, lists);
    }
    if !about.is_empty() {
        let inner = section(f, about_area, "analysis");
        f.render_widget(Paragraph::new(about), inner);
    }
}

/// Tracks that mix into a track, under a header that doubles as the legend for the key colours,
/// with the one at `next` (a file path) marked as playing next. Returns where the rows are, for
/// clicks.
pub(super) fn draw_matches(
    f: &mut Frame,
    area: Rect,
    matches: Option<&Matches>,
    next: Option<&str>,
    why: &str,
    state: &mut ListState,
    focused: bool,
) -> Rect {
    let mut title = vec![Span::styled(" mixes into ", dim())];
    let boost = matches.is_some_and(|m| m.items.iter().any(|x| x.relation == KeyRelation::EnergyBoost));
    let relations = [KeyRelation::Same, KeyRelation::Adjacent, KeyRelation::Relative, KeyRelation::EnergyBoost];
    for r in relations.into_iter().take(if boost { 4 } else { 3 }) {
        let mark = if theme::plain() { relation_mark(r) } else { icons::SQUARE.get() };
        title.push(Span::styled(mark, fg(key_relation_color(r))));
        title.push(Span::styled(format!(" {} ", r.describe()), faint()));
    }
    if area.height == 0 {
        return Rect::default();
    }
    let inner = section_with(f, area, Line::from(title));
    match matches.filter(|m| !m.items.is_empty()) {
        None => {
            f.render_widget(Paragraph::new(Line::styled(why.to_string(), dim())), inner);
            Rect::default()
        }
        Some(m) => {
            let items: Vec<ListItem> = m
                .items
                .iter()
                .map(|x| {
                    let t = m.track(x);
                    let key_color = key_relation_color(x.relation);
                    let bpm_style = if x.tempo_gap <= 0.02 { fg(theme::current().warning) } else { dim() };
                    let key = match theme::plain() {
                        true => format!("{}{:<3}", relation_mark(x.relation), x.key.code()),
                        false => format!("{:<4}", x.key.code()),
                    };
                    let mut spans = vec![
                        Span::styled(key, bold(key_color)),
                        Span::styled(format!("{:>5.1} ", t.bpm), bpm_style),
                        Span::styled(t.title.clone(), text()),
                    ];
                    if next == Some(t.file_path.as_str()) {
                        spans.push(Span::styled(" next", bold(theme::current().info)));
                    }
                    spans.push(Span::styled(format!("  {}", t.artist), dim()));
                    ListItem::new(Line::from(spans))
                })
                .collect();
            let list = List::new(items)
                .highlight_style(if focused { theme::selected() } else { theme::unfocused() })
                .highlight_symbol(Span::styled(icons::SELECTED.get(), fg(theme::current().accent)));
            state.fit(inner.height as usize, m.items.len());
            f.render_stateful_widget(list, inner, state);
            inner
        }
    }
}

/// "in playlists" over the top row of `area`, then up to `lists` of them, the last line saying
/// how many more there are.
pub(super) fn draw_in_playlists(f: &mut Frame, area: Rect, playlists: &[String], lists: usize) {
    let heading = Line::from(Span::styled(format!(" in {} ", plural(playlists.len(), "playlist")), dim()));
    let inner = section_with(f, area, heading);
    let mut lines: Vec<Line> = playlists.iter().take(lists).map(|p| Line::styled(p.clone(), text())).collect();
    if playlists.len() > lists
        && let Some(last) = lines.last_mut()
    {
        *last = Line::styled(format!("+ {} more", playlists.len() - lists + 1), dim());
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// A spectrum, registering its size so the analyser keeps bands for it.
fn draw_spectrum(f: &mut Frame, app: &mut App, area: Rect, mirror: bool) {
    if area.width < 4 || area.height < 1 {
        return;
    }
    app.areas.spectra.push((area.width, mirror));
    let empty = Bands::default();
    let bands = app.analyzer.bands(bar_layout(area.width, mirror).0).unwrap_or(&empty);
    f.render_widget(
        Spectrum { bands: &bands.levels, peaks: &bands.peaks, mirror, tint: app.flash(), palette: theme::palette() },
        area,
    );
}

fn draw_scope(f: &mut Frame, app: &App, area: Rect) {
    f.render_widget(Scope { samples: &app.analyzer.scope, tint: app.flash(), palette: theme::palette() }, area);
}

fn draw_deck(f: &mut Frame, app: &App, area: Rect) {
    match app.now.as_ref().and_then(|n| n.analysis.detail.as_ref().map(|d| (n, d))) {
        Some((now, wave)) => f.render_widget(
            Detail {
                wave,
                position_secs: app.position().as_secs_f64(),
                visible_secs: app.deck_secs,
                beats: &now.analysis.beats,
                cues: &app.drawn_cues(),
                palette: theme::palette(),
            },
            area,
        ),
        None => {
            let msg = match &app.now {
                Some(n) if n.loading => "reading the analysis…",
                Some(_) => "no detail waveform for this track",
                None => "deck: play a track",
            };
            let y = area.y + area.height / 2;
            f.render_widget(
                Paragraph::new(Line::styled(msg, dim())).alignment(Alignment::Center),
                Rect { y, height: 1, ..area },
            );
        }
    }
}

/// The cue lane, when the playing track has cues (or a loop).
fn draw_lane(f: &mut Frame, app: &App, area: Rect) {
    let Some(now) = app.now.as_ref().filter(|_| has_cues(app)) else { return };
    f.render_widget(
        CueLane {
            cues: &app.drawn_cues(),
            position_secs: app.position().as_secs_f64(),
            visible_secs: app.lane_secs(),
            bpm: now.track.bpm,
            palette: theme::palette(),
        },
        area,
    );
}

fn has_cues(app: &App) -> bool {
    app.now.as_ref().is_some_and(|n| !n.analysis.cues.is_empty()) || app.loop_playing().is_some()
}

/// A labelled divider above a panel of the wall; returns the space below it.
fn section(f: &mut Frame, area: Rect, name: &str) -> Rect {
    section_with(f, area, Line::from(vec![Span::styled(format!(" {name} "), dim())]))
}

pub(in crate::tui) fn section_with(f: &mut Frame, area: Rect, title: Line<'static>) -> Rect {
    let b = Block::new().borders(Borders::TOP).border_style(faint()).title(title);
    let inner = b.inner(area);
    f.render_widget(b, area);
    inner
}

/// How a key relates, without colours: = the same key, ± a step round the wheel, ↔ its
/// relative, ↑ an energy boost.
fn relation_mark(r: KeyRelation) -> &'static str {
    match r {
        KeyRelation::Same => "=",
        KeyRelation::Adjacent => "±",
        KeyRelation::Relative => "↔",
        KeyRelation::EnergyBoost => "↑",
    }
}

fn key_relation_color(r: KeyRelation) -> [u8; 3] {
    match r {
        KeyRelation::Same => theme::current().good,
        KeyRelation::Adjacent => theme::current().info,
        KeyRelation::Relative => theme::current().secondary,
        KeyRelation::EnergyBoost => theme::current().warning,
    }
}

/// The visualiser picked with `v`, with the cue lane under the spectrum and scope: all there's room
/// for in a small full screen.
fn draw_visual(f: &mut Frame, app: &mut App, area: Rect) {
    if area.width < 4 || area.height < 1 {
        return;
    }
    let area = if app.vis != Vis::Deck && has_cues(app) && area.height >= 4 {
        let lane_h = if area.height >= 7 { 2 } else { 1 };
        let [vis, _, lane] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(lane_h - 1), Constraint::Length(lane_h)])
                .areas(area);
        draw_lane(f, app, lane);
        vis
    } else {
        area
    };
    match app.vis {
        Vis::Bars => draw_spectrum(f, app, area, false),
        Vis::Mirror => draw_spectrum(f, app, area, true),
        Vis::Scope => draw_scope(f, app, area),
        Vis::Deck => draw_deck(f, app, area),
    }
}

/// Full screen: every visualiser at once. The deck waveform across the top with the spectrum and
/// the scope side by side under it (all three in one row when the terminal is short), and the cue
/// lane along the bottom. The mirrored spectrum sits beside the track details, see `draw_player`.
fn draw_wall(f: &mut Frame, app: &mut App, area: Rect) {
    if area.height < 6 || area.width < 40 {
        return draw_visual(f, app, area);
    }
    // The lane's own rail doubles as its divider: a blank row above, then rail and names.
    let lane_h = match (has_cues(app), area.height) {
        (false, _) => 0,
        (true, h) if h >= 16 => 3,
        (true, _) => 2,
    };
    let [panels, lane] = Layout::vertical([Constraint::Min(4), Constraint::Length(lane_h)]).areas(area);
    let (deck, bars, scope) = if panels.height >= 13 {
        let [deck, _, lower] =
            Layout::vertical([Constraint::Percentage(50), Constraint::Length(1), Constraint::Min(3)]).areas(panels);
        let [bars, _, scope] =
            Layout::horizontal([Constraint::Fill(3), Constraint::Length(2), Constraint::Fill(2)]).areas(lower);
        (deck, bars, scope)
    } else {
        let [deck, _, bars, _, scope] = Layout::horizontal([
            Constraint::Fill(3),
            Constraint::Length(2),
            Constraint::Fill(2),
            Constraint::Length(2),
            Constraint::Fill(2),
        ])
        .areas(panels);
        (deck, bars, scope)
    };
    let inner = section(f, deck, "deck");
    draw_deck(f, app, inner);
    let inner = section(f, bars, "spectrum");
    draw_spectrum(f, app, inner, false);
    let inner = section(f, scope, "scope");
    draw_scope(f, app, inner);
    if lane_h > 0 {
        draw_lane(f, app, Rect { y: lane.y + 1, height: lane.height - 1, ..lane });
    }
}

fn draw_progress(f: &mut Frame, app: &App, area: Rect) {
    let pos = app.position().as_secs_f64();
    let (dur, have) = app.now.as_ref().map_or((0.0, false), |n| (n.duration.as_secs_f64(), true));
    let glyph = if !have {
        Span::styled(icons::SQUARE.spaced(), faint())
    } else if app.is_playing() {
        Span::styled(icons::PLAY.spaced(), bold(theme::current().accent))
    } else {
        Span::styled(icons::PAUSE.spaced(), bold(theme::current().warning))
    };
    let left =
        vec![glyph, Span::styled(clock(if have { pos } else { 0.0 }), bold(theme::current().text)), Span::raw(" ")];
    let vol = app.volume;
    let repeat = match app.repeat {
        Repeat::Off => Span::styled(format!("{} off", icons::REPEAT.get()), faint()),
        Repeat::All => Span::styled(format!("{} all", icons::REPEAT.get()), fg(theme::current().info)),
        Repeat::One => Span::styled(format!("{} one", icons::REPEAT.get()), fg(theme::current().info)),
    };
    let shuffle = if app.shuffle {
        Span::styled(format!("{} on", icons::MIX.get()), fg(theme::current().info))
    } else {
        Span::styled(format!("{} off", icons::MIX.get()), faint())
    };
    let quantize = if app.deck.quantize {
        Span::styled("Q on", fg(theme::current().warning))
    } else {
        Span::styled("Q off", faint())
    };
    let mut right = vec![
        Span::styled(format!(" -{}", clock(if have { dur - pos } else { 0.0 })), dim()),
        Span::styled(format!("  {}", clock(dur)), faint()),
        Span::styled("   vol ", faint()),
        Span::styled(format!("{:>3}%", (vol * 100.0).round()), text()),
        Span::raw("   "),
        repeat,
        Span::raw("  "),
        shuffle,
        Span::raw("  "),
        quantize,
    ];
    if app.deck.single {
        right.extend([Span::raw("  "), Span::styled("single", fg(theme::current().info))]);
    }
    if app.deck.auto_cue {
        right.extend([Span::raw("  "), Span::styled("auto cue", fg(theme::current().info))]);
    }
    let lw: usize = left.iter().map(Span::width).sum();
    let rw: usize = right.iter().map(Span::width).sum();
    let bar_w = (area.width as usize).saturating_sub(lw + rw);
    let frac = if dur > 0.0 { (pos / dur).clamp(0.0, 1.0) } else { 0.0 };
    let filled = (frac * bar_w as f64).round() as usize;
    let mut spans = left;
    if bar_w > 2 {
        spans.push(Span::styled("━".repeat(filled.min(bar_w - 1)), fg(theme::current().accent)));
        spans.push(Span::styled(icons::DOT.get(), bold(theme::current().text)));
        spans.push(Span::styled("─".repeat(bar_w.saturating_sub(filled + 1)), faint()));
    }
    spans.extend(right);
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}
