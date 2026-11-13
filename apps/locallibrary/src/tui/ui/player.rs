//! What the Deck layout says of the playing track beside its waveforms: the art, what's known of
//! it, what mixes into it and the visualisers.

use loclib_waveform::analyzer::Bands;
use loclib_waveform::{Pixels, Scope, Spectrum, bar_layout, cue_letter, vinyl};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui_image::Image;

use crate::output::{bytes, clock, plural};
use crate::tui::app::{App, Matches, NowPlaying, Vis};
use crate::tui::art::Picture;
use crate::tui::icons;
use crate::tui::keys::Relation as KeyRelation;
use crate::tui::theme::{self, bold, dim, faint, fg, text};

use super::*;

/// The playing track's cover once it's ready for this size, or a record when it has none.
pub(super) fn draw_art(f: &mut Frame, app: &mut App, area: Rect) {
    if area.width < 2 || area.height < 1 {
        return;
    }
    let t = app.started.elapsed().as_secs_f32();
    let playing = app.is_playing();
    let has_art = app.now.as_ref().is_some_and(|n| n.art.is_some());
    match app.cover((area.width, area.height)) {
        Some(Picture::Pixels(px)) => {
            f.render_widget(Pixels { width: area.width, pixels: px, palette: theme::palette() }, area);
        }
        Some(Picture::Graphics(p)) => f.render_widget(Image::new(p), area),
        None if has_art => {}
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

pub(super) fn chip(s: String, c: [u8; 3]) -> Span<'static> {
    Span::styled(format!(" {s} "), theme::chip(c))
}

/// The loop playing, as a chip.
pub(super) fn loop_chip(app: &App) -> Option<Span<'static>> {
    let (_, beats) = app.loop_playing()?;
    Some(chip(format!("LOOP {}", beats.map_or("on".into(), crate::tui::deck::beats_label)), theme::current().loop_cue))
}

/// The track's hot cues in their order, each a chip or with `quiet` a letter in its colour, the one
/// the playhead last passed underlined; with the pads on, each with the key that plays it before it.
pub(super) fn pad_chips(app: &App, now: &NowPlaying, pos_ms: f64, quiet: bool) -> Vec<Span<'static>> {
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
        let (letter, colour) = (cue_letter(c).unwrap_or('?'), theme::palette().cue_color(c));
        let mut s =
            if quiet { Span::styled(format!(" {letter} "), bold(colour)) } else { chip(letter.to_string(), colour) };
        if current.is_some_and(|cur| std::ptr::eq(cur, c)) {
            s.style = s.style.add_modifier(Modifier::UNDERLINED);
        }
        spans.push(s);
    }
    spans
}

pub(super) fn draw_info(f: &mut Frame, app: &App, area: Rect) {
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
    let mut chips = beat_spans(now, pos_ms);
    // A loop playing before the phrase, which the strip under the overview shows too.
    if let Some(c) = loop_chip(app) {
        chips.extend([Span::raw("  "), c]);
    }
    if let Some((_, p, bar)) = now.phrase_at(pos_ms) {
        chips.push(Span::raw("  "));
        chips.push(chip(p.kind.name().to_uppercase(), theme::current().phrase(p.kind)));
        chips.push(Span::styled(format!(" {bar}/{}", p.bars), dim()));
        if let Some(st) = &now.analysis.structure {
            chips.push(Span::styled(format!("  {:?} mood", st.mood).to_lowercase(), faint()));
        }
    }
    lines.push(Line::from(chips));

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
        let mut spans = pad_chips(app, now, pos_ms, false);
        if memory > 0 {
            spans.push(Span::styled(format!(" {}{memory}", icons::DIAMOND.get()), fg(theme::current().memory_cue)));
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
    // A gap after the track's names, if it costs nothing else a line.
    if lines.len() < area.height as usize {
        lines.insert(3, Line::raw(""));
    }
    let lines: Vec<Line> = lines.into_iter().take(area.height as usize).collect();
    f.render_widget(Paragraph::new(lines), area);
}

/// Under the waveforms: what mixes into the playing track (to pick the next one from), where it's
/// filed, and what rekordbox's analysis found, side by side.
pub(super) fn draw_details(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(now) = &app.now else { return };
    let mut about: Vec<Line> = Vec::new();
    let beats = &now.analysis.beats;
    if !beats.is_empty() {
        let (lo, hi) =
            beats.iter().filter(|b| b.bpm > 0.0).fold((f64::MAX, 0.0f64), |(l, h), b| (l.min(b.bpm), h.max(b.bpm)));
        let tempo = if hi - lo > 0.05 { format!("{lo:.1}–{hi:.1} BPM") } else { "steady tempo".into() };
        about.push(Line::from(vec![
            Span::styled(
                format!("{} · {} · ", plural(beats.len(), "beat"), plural(beats.len().div_ceil(4), "bar")),
                text(),
            ),
            Span::styled(tempo, dim()),
        ]));
    }
    let cues = &now.analysis.cues;
    if !cues.is_empty() {
        let hot = cues.iter().filter(|c| c.hot_cue.is_some()).count();
        let loops = cues.iter().filter(|c| c.end_ms.is_some()).count();
        let about_cues =
            format!("{} ({}) · {} memory", plural(hot, "hot cue"), plural(loops, "loop"), cues.len() - hot);
        about.push(Line::styled(about_cues, text()));
    }
    if !now.phrases.is_empty() {
        let mut spans = vec![Span::styled(format!("{} ", plural(now.phrases.len(), "phrase")), text())];
        for p in &now.phrases {
            spans.push(Span::styled(icons::PHRASE.get(), fg(theme::current().phrase(p.kind))));
        }
        about.push(Line::from(spans));
    }

    let lists = now.playlists.len().min(4).min(usize::from(area.height.saturating_sub(1)));
    let [mixes, _, lists_area, _, about_area] = Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(if lists > 0 { (area.width / 5).max(20) } else { 0 }),
        Constraint::Length(1),
        Constraint::Length(if about.is_empty() { 0 } else { 42 }),
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
    let mut title = vec![Span::styled("mixes into ", dim())];
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
/// how many more there are. Each shows by its own name, then the folder it's in: playlists
/// often share their top folders, which would fill a narrow column.
pub(super) fn draw_in_playlists(f: &mut Frame, area: Rect, playlists: &[String], lists: usize) {
    let heading = Line::from(Span::styled(format!("in {} ", plural(playlists.len(), "playlist")), dim()));
    let inner = section_with(f, area, heading);
    let line = |path: &String| {
        let (folders, name) = path.rsplit_once(" › ").unwrap_or(("", path));
        let folder = folders.rsplit_once(" › ").map_or(folders, |(_, f)| f);
        Line::from(vec![Span::styled(name.to_string(), text()), Span::styled(format!("  {folder}"), dim())])
    };
    let mut lines: Vec<Line> = playlists.iter().take(lists).map(line).collect();
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

/// The visualiser `v` picked.
pub(super) fn draw_visualiser(f: &mut Frame, app: &mut App, area: Rect) {
    match app.vis {
        Vis::Bars => draw_spectrum(f, app, area, false),
        Vis::Mirror => draw_spectrum(f, app, area, true),
        Vis::Scope => {
            f.render_widget(Scope { samples: &app.analyzer.scope, tint: app.flash(), palette: theme::palette() }, area);
        }
        Vis::Deck => {}
    }
}

/// A labelled divider above a panel; returns the space below it.
fn section(f: &mut Frame, area: Rect, name: &str) -> Rect {
    section_with(f, area, Line::from(vec![Span::styled(format!("{name} "), dim())]))
}

pub(in crate::tui) fn section_with(f: &mut Frame, area: Rect, title: Line<'static>) -> Rect {
    let b = Block::new().borders(Borders::TOP).border_style(faint()).title(title);
    let inner = b.inner(area);
    f.render_widget(b, area);
    inner
}

/// How a key relates, without colours: = the same key, ± a step round the wheel, ↔ its
/// relative, ↑ an energy boost.
pub(super) fn relation_mark(r: KeyRelation) -> &'static str {
    match r {
        KeyRelation::Same => "=",
        KeyRelation::Adjacent => "±",
        KeyRelation::Relative => "↔",
        KeyRelation::EnergyBoost => "↑",
    }
}

pub(super) fn key_relation_color(r: KeyRelation) -> [u8; 3] {
    match r {
        KeyRelation::Same => theme::current().good,
        KeyRelation::Adjacent => theme::current().info,
        KeyRelation::Relative => theme::current().secondary,
        KeyRelation::EnergyBoost => theme::current().warning,
    }
}
