//! The Profile view: what the DJ's sets, collection and settings say about how they play, and
//! the part of it a public profile would show. Choices (sections, pinned and hidden tracks,
//! rounding) are saved to the store as they're made, and the document follows at once; only a
//! rebuild (`R`, or changing scope) reads the drives again.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;

use locallibrary::Store;
use locallibrary::api::{self, ProfileRequest, ProfileView};
use locallibrary::profile::{self, Basis, Precision, PrefsUpdate, ProfileReport, RankedSection, Section};
use locallibrary::progress::{CancelToken, Ctx, Event};
use ratatui::Frame;
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState, Wrap};

use super::account::Account;
use super::app::{App, Level, Msg};
use super::keymap::Act;
use super::theme::{self, bold, dim, faint, fg, rgb3, text};
use super::ui::Scrolled;
use crate::output::plural;

#[derive(Debug, Clone, PartialEq)]
pub enum Scope {
    /// Every mounted drive and the store.
    Everything,
    /// One drive (and the store's archive of its library).
    Drive(PathBuf, String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Sections,
    Detail,
}

pub struct ProfileTab {
    pub(super) tx: Sender<Msg>,
    pub view: Option<Result<ProfileView, String>>,
    pub loading: bool,
    cancel: Option<CancelToken>,
    /// Analysis files read so far, and how many there are.
    progress: Arc<[AtomicU64; 2]>,
    generation: u64,
    pub scope: Scope,
    pub pane: Pane,
    pub sections: ListState,
    pub rows: TableState,
    pub document: bool,
    pub scroll: u16,
    sections_area: Rect,
    detail_area: Rect,
    /// The Atmosphere account, and signing in or out (the bar at the top).
    pub account: Account,
}

impl ProfileTab {
    pub fn new(tx: Sender<Msg>) -> ProfileTab {
        ProfileTab {
            tx,
            view: None,
            loading: false,
            cancel: None,
            progress: Arc::new([AtomicU64::new(0), AtomicU64::new(0)]),
            generation: 0,
            scope: Scope::Everything,
            pane: Pane::Sections,
            sections: ListState::default().with_selected(Some(0)),
            rows: TableState::default(),
            document: false,
            scroll: 0,
            sections_area: Rect::default(),
            detail_area: Rect::default(),
            account: Account::default(),
        }
    }

    /// What the header shows while building.
    pub fn status(&self) -> Option<String> {
        if !self.loading {
            return None;
        }
        let (done, total) = (self.progress[0].load(Ordering::Relaxed), self.progress[1].load(Ordering::Relaxed));
        Some(if total > 0 {
            format!("Building profile… {done}/{total} tracks")
        } else {
            "Building profile…".to_string()
        })
    }

    pub fn loaded(&mut self, generation: u64, r: Result<ProfileView, String>) {
        if generation != self.generation {
            return; // an older build, replaced since
        }
        self.loading = false;
        self.cancel = None;
        self.view = Some(r);
        self.clamp_rows();
    }

    fn section(&self) -> Section {
        Section::ALL[self.sections.selected().unwrap_or(0).min(Section::ALL.len() - 1)]
    }

    fn report(&self) -> Option<&ProfileReport> {
        match &self.view {
            Some(Ok(v)) => Some(&v.report),
            _ => None,
        }
    }

    /// Rows of the selected section's list (0: the section has no list).
    fn detail_len(&self) -> usize {
        let Some(r) = self.report() else { return 0 };
        match self.section() {
            Section::Sets => r.sets.list.len(),
            Section::Signature => r.signature.played.len(),
            Section::Genres => r.genres.top.len(),
            Section::Labels => r.labels.top.len(),
            Section::Artists => r.artists.top.len(),
            _ => 0,
        }
    }

    fn clamp_rows(&mut self) {
        let n = self.detail_len();
        let sel = self.rows.selected().unwrap_or(0).min(n.saturating_sub(1));
        self.rows.select((n > 0).then_some(sel));
        if n == 0 {
            self.pane = Pane::Sections;
        }
    }

    fn select_section(&mut self, i: usize) {
        let i = i.min(Section::ALL.len() - 1);
        if self.sections.selected() != Some(i) {
            self.sections.select(Some(i));
            self.rows = TableState::default();
            self.clamp_rows();
        }
    }

    fn move_by(&mut self, delta: isize) {
        if self.document {
            self.scroll = (self.scroll as isize + delta).max(0) as u16;
            return;
        }
        match self.pane {
            Pane::Sections => {
                let cur = self.sections.selected().unwrap_or(0) as isize;
                self.select_section((cur + delta).clamp(0, Section::ALL.len() as isize - 1) as usize);
            }
            Pane::Detail => {
                let n = self.detail_len();
                if n > 0 {
                    let cur = self.rows.selected().unwrap_or(0) as isize;
                    self.rows.select(Some((cur + delta).clamp(0, n as isize - 1) as usize));
                }
            }
        }
    }
}

/// Rebuild the profile in the background (a running build is cancelled).
pub fn refresh(app: &mut App) {
    let root = app.store_root.clone();
    let t = &mut app.profile;
    t.account.load(&root);
    if let Some(c) = t.cancel.take() {
        c.cancel();
    }
    t.generation += 1;
    let generation = t.generation;
    let cancel = CancelToken::new();
    t.cancel = Some(cancel.clone());
    t.loading = true;
    t.progress[0].store(0, Ordering::Relaxed);
    t.progress[1].store(0, Ordering::Relaxed);
    let req = match &t.scope {
        Scope::Everything => ProfileRequest::default(),
        Scope::Drive(path, _) => ProfileRequest { drives: vec![path.display().to_string()], ..Default::default() },
    };
    let tx = t.tx.clone();
    let progress = t.progress.clone();
    let job = move || {
        let counter = move |e: &Event| match e {
            Event::PhaseStart { items, .. } => {
                progress[0].store(0, Ordering::Relaxed);
                progress[1].store(*items, Ordering::Relaxed);
            }
            Event::Item { .. } => {
                progress[0].fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        };
        let ctx = Ctx::new(&counter, cancel);
        let store = Store::open(&root).ok();
        let r = api::profile(store.as_ref(), &req, &ctx).map_err(|e| e.to_string());
        Msg::Profile(generation, Box::new(r))
    };
    super::app::worker(tx, job, move |e| Msg::Profile(generation, Box::new(Err(e))));
}

/// Save a change to the preferences and redo the document.
fn change(app: &mut App, update: PrefsUpdate, done: String) {
    let r = Store::open_or_init(&app.store_root)
        .and_then(|s| api::profile_prefs_update(&s, &update))
        .map_err(|e| e.to_string());
    match r {
        Ok(prefs) => {
            let aliases_changed =
                matches!(&app.profile.view, Some(Ok(v)) if v.prefs.genre_aliases != prefs.genre_aliases);
            if let Some(Ok(v)) = &mut app.profile.view {
                v.prefs = prefs;
                v.published = v.prefs.published().into_iter().collect();
                v.document = profile::document(&v.report, &v.prefs);
            }
            app.toast(done, Level::Info);
            if aliases_changed {
                refresh(app);
            }
        }
        Err(e) => app.toast(e, Level::Error),
    }
}

fn export(app: &mut App) {
    let Some(Ok(v)) = &app.profile.view else {
        app.toast("nothing to export yet", Level::Info);
        return;
    };
    // Into the store, made here if there's none yet: a file there first would stop that.
    let path = app.store_root.join("profile-document.json");
    let r = Store::open_or_init(&app.store_root)
        .and_then(|_| locallibrary::fsutil::write_json_atomic(&path, &v.document))
        .map_err(|e| e.to_string());
    match r {
        Ok(()) => app.toast(format!("wrote {}", path.display()), Level::Info),
        Err(e) => app.toast(e, Level::Error),
    }
}

/// Up, down, a page or to an end (`delta` rows) in the pane with focus.
pub fn move_by(app: &mut App, delta: isize) {
    app.profile.move_by(delta);
}

/// Tab or enter: between the sections and the selected section's list.
pub fn switch_pane(app: &mut App) {
    let t = &mut app.profile;
    if t.document {
        return;
    }
    t.pane = match t.pane {
        Pane::Sections if t.detail_len() > 0 => Pane::Detail,
        _ => Pane::Sections,
    };
    t.clamp_rows();
}

/// Esc: close the document, or go back to the sections. Returns whether there was either.
pub fn escape(app: &mut App) -> bool {
    let t = &mut app.profile;
    if t.document {
        t.document = false;
    } else if t.pane == Pane::Detail {
        t.pane = Pane::Sections;
    } else {
        return false;
    }
    true
}

/// The Profile view's own actions.
pub fn perform(app: &mut App, act: Act) {
    match act {
        Act::ProfileDocument => {
            let t = &mut app.profile;
            t.document = !t.document;
            t.scroll = 0;
        }
        Act::ProfileWrite => export(app),
        Act::ProfileScope => {
            let next = match (&app.profile.scope, &app.loaded) {
                (Scope::Everything, Some(l)) => Scope::Drive(l.root().to_path_buf(), l.device.name()),
                (Scope::Everything, None) => {
                    app.toast("open a drive in the library first", Level::Info);
                    return;
                }
                (Scope::Drive(..), _) => Scope::Everything,
            };
            let label = match &next {
                Scope::Everything => "every drive and the store".to_string(),
                Scope::Drive(_, name) => format!("{name} only"),
            };
            app.profile.scope = next;
            refresh(app);
            app.toast(format!("profile from {label}"), Level::Info);
        }
        Act::ProfileInclude => {
            let t = &app.profile;
            let Some(Ok(v)) = &t.view else { return };
            let s = t.section();
            let on = v.published.contains(&s);
            let u = if on {
                PrefsUpdate { exclude: vec![s], ..Default::default() }
            } else {
                PrefsUpdate { include: vec![s], ..Default::default() }
            };
            let msg = if on {
                format!("{} left out of the document", s.title())
            } else {
                format!("{} in the document", s.title())
            };
            change(app, u, msg);
        }
        Act::ProfileRounding => {
            let Some(Ok(v)) = &app.profile.view else { return };
            let (p, name) = match v.prefs.precision {
                Precision::Rounded => (Precision::Exact, "exact"),
                Precision::Exact => (Precision::Rounded, "rounded"),
            };
            change(
                app,
                PrefsUpdate { precision: Some(p), ..Default::default() },
                format!("{name} numbers in the document"),
            );
        }
        Act::ProfilePin => track_action(app, 'm'),
        Act::ProfileHide => track_action(app, 'h'),
        Act::ProfileHideArtist => track_action(app, 'H'),
        Act::ProfileAccount => super::account::press(app),
        _ => {}
    }
}

/// Pin or hide the selected track, or hide an artist.
fn track_action(app: &mut App, c: char) {
    let t = &app.profile;
    let Some(Ok(v)) = &t.view else { return };
    let row = t.rows.selected().filter(|_| t.pane == Pane::Detail);
    let p = &v.prefs;
    let update = match (t.section(), row) {
        (Section::Signature, Some(i)) => {
            let Some(track) = v.report.signature.played.get(i) else { return };
            match c {
                'm' if p.is_pinned(&track.name) => (
                    PrefsUpdate { unpin: vec![track.name.clone()], ..Default::default() },
                    format!("unpinned {}", track.name),
                ),
                'm' => (
                    PrefsUpdate { pin: vec![track.name.clone()], ..Default::default() },
                    format!("pinned {}", track.name),
                ),
                'h' if p.is_hidden_track(&track.name) => (
                    PrefsUpdate { unhide: vec![track.name.clone()], ..Default::default() },
                    format!("{} can be named again", track.name),
                ),
                'h' => (
                    PrefsUpdate { hide: vec![track.name.clone()], ..Default::default() },
                    format!("{} won't be named", track.name),
                ),
                _ => artist_toggle(p, &track.artist),
            }
        }
        (Section::Artists, Some(i)) if c != 'm' => {
            let Some(a) = v.report.artists.top.get(i) else { return };
            artist_toggle(p, &a.name)
        }
        _ => {
            let msg = match c {
                'm' => "select a track under Signature tracks (tab) to pin it",
                _ => "select a track under Signature tracks, or an artist under Artists (tab), to hide it",
            };
            app.toast(msg, Level::Info);
            return;
        }
    };
    change(app, update.0, update.1);
}

fn artist_toggle(p: &locallibrary::profile::ProfilePrefs, artist: &str) -> (PrefsUpdate, String) {
    let exact =
        p.hidden_artists.iter().find(|h| locallibrary::model::normalize(h) == locallibrary::model::normalize(artist));
    match exact {
        Some(h) => (
            PrefsUpdate { unhide_artist: vec![h.clone()], ..Default::default() },
            format!("{artist} can be named again"),
        ),
        None => (
            PrefsUpdate { hide_artist: vec![artist.to_string()], ..Default::default() },
            format!("{artist} won't be named"),
        ),
    }
}

pub fn on_mouse(app: &mut App, m: MouseEvent) -> bool {
    let p = Position::new(m.column, m.row);
    let t = &mut app.profile;
    let in_sections = t.sections_area.contains(p);
    let in_detail = t.detail_area.contains(p);
    if !in_sections && !in_detail {
        return false;
    }
    match m.kind {
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            t.pane = if in_sections || t.detail_len() == 0 { Pane::Sections } else { Pane::Detail };
            t.move_by(if m.kind == MouseEventKind::ScrollUp { -3 } else { 3 });
        }
        MouseEventKind::Down(MouseButton::Left) if in_sections => {
            t.pane = Pane::Sections;
            let row = m.row.saturating_sub(t.sections_area.y + 1) as usize + t.sections.offset();
            if m.row > t.sections_area.y && row < Section::ALL.len() {
                t.select_section(row);
            }
        }
        MouseEventKind::Down(MouseButton::Left) if t.detail_len() > 0 && !t.document => {
            t.pane = Pane::Detail;
            // The list's header row sits under the border, what the section reveals (two rows)
            // and the section's note (two rows).
            let top = t.detail_area.y + 5;
            if m.row > top {
                let i = (m.row - top - 1) as usize + t.rows.offset();
                if i < t.detail_len() {
                    t.rows.select(Some(i));
                }
            }
        }
        _ => {}
    }
    true
}

// ---------------------------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------------------------

const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

fn block(title: Line<'static>, focused: bool) -> Block<'static> {
    Block::bordered().border_type(theme::border_type(focused)).border_style(theme::border(focused)).title(title)
}

fn title(s: &str, focused: bool) -> Line<'static> {
    let style = if focused { bold(theme::CYAN) } else { bold(theme::DIM) };
    Line::from(vec![Span::raw(" "), Span::styled(s.to_string(), style), Span::raw(" ")])
}

fn pct(x: f64) -> String {
    format!("{:.0}%", x * 100.0)
}

fn opt_pct(x: Option<f64>) -> String {
    x.map_or_else(|| "–".to_string(), pct)
}

/// A horizontal bar of `frac` × `width` cells, in eighths.
fn bar(frac: f64, width: usize) -> String {
    let eighths = (frac.clamp(0.0, 1.0) * width as f64 * 8.0).round() as usize;
    let mut s = "█".repeat(eighths / 8);
    if !eighths.is_multiple_of(8) {
        s.push(['▏', '▎', '▍', '▌', '▋', '▊', '▉'][eighths % 8 - 1]);
    }
    s
}

/// `s` in exactly `width` columns: cut short with an ellipsis, or padded.
fn fit(s: &str, width: usize) -> String {
    crate::output::fit_with(s, width, "…")
}

/// A column chart `height` rows tall: one column per value, `width` cells wide, scaled between
/// `lo` and the largest value.
fn columns(
    values: &[Option<f64>],
    lo: f64,
    height: usize,
    width: usize,
    color: impl Fn(usize) -> [u8; 3],
) -> Vec<Line<'static>> {
    let hi = values.iter().flatten().copied().fold(lo, f64::max);
    let span = (hi - lo).max(f64::EPSILON);
    let eighths: Vec<usize> = values
        .iter()
        .map(|v| v.map_or(0, |x| (((x - lo) / span).clamp(0.0, 1.0) * (height * 8) as f64).round().max(1.0) as usize))
        .collect();
    (0..height)
        .rev()
        .map(|row| {
            let spans: Vec<Span<'static>> = eighths
                .iter()
                .enumerate()
                .map(|(i, &e)| {
                    let filled = e.saturating_sub(row * 8).min(8);
                    let ch = if filled == 0 { ' ' } else { BLOCKS[filled - 1] };
                    Span::styled(format!("{}{}", ch.to_string().repeat(width.saturating_sub(1)), " "), fg(color(i)))
                })
                .collect();
            Line::from(spans)
        })
        .collect()
}

fn bar_line(label: &str, label_w: usize, frac: f64, width: usize, color: [u8; 3], value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(fit(label, label_w), text()),
        Span::raw(" "),
        Span::styled(fit(&bar(frac, width), width), fg(color)),
        Span::styled(format!(" {value}"), dim()),
    ])
}

fn note(s: impl Into<String>) -> Line<'static> {
    Line::styled(s.into(), dim())
}

/// One-line digest of each section for the section list.
fn stat(r: &ProfileReport, s: Section) -> String {
    match s {
        Section::Summary => format!("{} traits", r.summary.tags.len()),
        Section::Sets => r.sets.count.to_string(),
        Section::Tempo => r.tempo.core.map(|c| format!("{}–{}", c.low.round(), c.high.round())).unwrap_or_default(),
        Section::Arc => {
            if r.arc.sets == 0 {
                String::new()
            } else if r.arc.builds >= 0.5 {
                "builds".into()
            } else if r.arc.steady >= 0.5 {
                "steady".into()
            } else if r.arc.winds_down >= 0.5 {
                "winds down".into()
            } else {
                "varies".into()
            }
        }
        Section::Mixing => r.mixing.harmonic_share.map(|h| format!("{} key", pct(h))).unwrap_or_default(),
        Section::Keys => r.keys.top.first().map(|k| k.name.clone()).unwrap_or_default(),
        Section::Energy => r.energy.as_ref().map(|e| format!("{} high", pct(e.high))).unwrap_or_default(),
        Section::Genres => r.genres.top.first().map(|g| g.name.clone()).unwrap_or_default(),
        Section::Labels => r.labels.distinct.to_string(),
        Section::Artists => r.artists.distinct.to_string(),
        Section::Era => r.era.median_year.map(|y| y.to_string()).unwrap_or_default(),
        Section::Signature => r.signature.tracks.len().to_string(),
        Section::Prep => r.prep.as_ref().map(|p| pct(p.hot_cue_share)).unwrap_or_default(),
        Section::Collection => r.collection.tracks.to_string(),
        Section::Style => r.style.as_ref().map(|s| s.values.len().to_string()).unwrap_or_default(),
    }
}

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    draw_tab(f, &mut app.profile, area);
}

fn draw_tab(f: &mut Frame, t: &mut ProfileTab, area: Rect) {
    let [bar, area] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
    f.render_widget(Paragraph::new(t.account.line(bar.width)), bar);
    let status = t.status();
    let v = match &t.view {
        Some(Ok(v)) => v,
        other => {
            let lines = match other {
                Some(Err(e)) => {
                    vec![Line::raw(""), Line::styled(e.clone(), fg(theme::RED)), Line::styled("R tries again", dim())]
                }
                _ => vec![
                    Line::raw(""),
                    Line::styled(status.unwrap_or_else(|| "Building profile…".into()), bold(theme::FG)),
                    Line::styled(
                        "Reading sets, history lists, tags and analysis from your drives and the store",
                        dim(),
                    ),
                ],
            };
            let p = Paragraph::new(lines).alignment(Alignment::Center).block(block(title("Profile", true), true));
            f.render_widget(p, area);
            t.sections_area = Rect::default();
            t.detail_area = Rect::default();
            return;
        }
    };
    let r = &v.report;
    let [top, main] = Layout::vertical([Constraint::Length(5), Constraint::Min(0)]).areas(area);

    // Summary.
    let scope = match &t.scope {
        Scope::Everything => "every drive and the store".to_string(),
        Scope::Drive(_, name) => format!("{name} only"),
    };
    let headline = v.prefs.headline.clone().unwrap_or_else(|| r.summary.headline.clone());
    let mut tags: Vec<Span<'static>> = Vec::new();
    for tag in &r.summary.tags {
        tags.push(Span::styled("✓ ", bold(theme::GREEN)));
        tags.push(Span::styled(format!("{}  ", tag.label), text()));
    }
    if tags.is_empty() {
        tags.push(Span::styled("no traits stand out yet (more sets will tell)", dim()));
    }
    let sources: Vec<&str> = r.sources.iter().map(|s| s.name.as_str()).collect();
    let basis = match r.basis {
        Basis::Plays => format!("{} in {}", plural(r.sets.plays, "play"), plural(r.sets.count, "set")),
        Basis::Sessions => {
            format!("no sets declared: {} in {}", plural(r.sets.plays, "play"), plural(r.sets.count, "session"))
        }
        Basis::Collection => "no sets: from the collection".to_string(),
    };
    let rounding = match v.prefs.precision {
        Precision::Rounded => "rounded",
        Precision::Exact => "exact",
    };
    let mut info = vec![Span::styled(
        format!(
            "{} · {basis} · document: {} of {} sections, {rounding}",
            sources.join(", "),
            v.published.iter().filter(|s| r.basis.publishes(**s)).count(),
            Section::ALL.len()
        ),
        dim(),
    )];
    if let Some(s) = status {
        info.push(Span::styled(format!("  ⟳ {s}"), fg(theme::CYAN)));
    }
    let mut head = vec![Span::styled(headline, bold(theme::CYAN))];
    if v.prefs.headline.is_some() {
        head.push(Span::styled("  (your headline)", faint()));
    }
    let summary = Paragraph::new(vec![Line::from(head), Line::from(tags), Line::from(info)])
        .block(block(title(&format!("Profile · {scope}"), false), false))
        .wrap(Wrap { trim: true });
    f.render_widget(summary, top);

    // Sections.
    let [side, detail] = Layout::horizontal([Constraint::Length(32), Constraint::Min(0)]).areas(main);
    t.sections_area = side;
    t.detail_area = detail;
    let focused = t.pane == Pane::Sections && !t.document;
    let inner_w = side.width.saturating_sub(3) as usize;
    let items: Vec<ListItem> = Section::ALL
        .iter()
        .map(|&s| {
            let on = v.published.contains(&s) && r.basis.publishes(s);
            let mark = if on { Span::styled("● ", fg(theme::GREEN)) } else { Span::styled("○ ", faint()) };
            // The stat can be user text (the top genre or key): it gets half the column at most.
            let st = super::ui::clip(&stat(r, s), inner_w / 2);
            let name_w = inner_w.saturating_sub(unicode_width::UnicodeWidthStr::width(st.as_str()) + 3).max(4);
            ListItem::new(Line::from(vec![
                mark,
                Span::styled(fit(s.title_in(r.basis), name_w), if on { text() } else { dim() }),
                Span::styled(format!(" {st}"), faint()),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(block(title("Sections", focused), focused))
        .highlight_style(if focused { theme::selected() } else { Style::new().add_modifier(Modifier::BOLD) })
        .highlight_symbol(Span::styled("▍", fg(theme::PINK)));
    t.sections.fit(side.height.saturating_sub(2) as usize, Section::ALL.len());
    f.render_stateful_widget(list, side, &mut t.sections);

    if t.document {
        draw_document(f, v, t.scroll, detail);
        return;
    }
    let section = Section::ALL[t.sections.selected().unwrap_or(0).min(Section::ALL.len() - 1)];
    let on = v.published.contains(&section);
    let state = if on && r.basis.publishes(section) {
        Span::styled(" in the document ", fg(theme::GREEN))
    } else {
        Span::styled(" only on this computer ", faint())
    };
    let detail_focused = t.pane == Pane::Detail;
    let b = block(title(section.title_in(r.basis), detail_focused), detail_focused)
        .title(Line::from(state).right_aligned());
    let inner = b.inner(detail);
    f.render_widget(b, detail);
    let [reveal, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(inner);
    let reveal_text = match (r.basis.publishes(section), on) {
        (false, _) => "Sessions aren't published. With sets declared, this shows ",
        (true, true) => "Published, this shows ",
        (true, false) => "Published, this would show ",
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {reveal_text}"), faint()),
            Span::styled(section.reveals().to_string(), dim()),
            Span::styled("   i ", theme::key()),
            Span::styled(if on { "leave out" } else { "include" }, faint()),
        ]))
        .wrap(Wrap { trim: true }),
        reveal,
    );
    let body = Rect { x: body.x + 1, width: body.width.saturating_sub(2), ..body };
    let w = body.width as usize;
    let lines: Vec<Line<'static>> = match section {
        Section::Summary => summary_lines(v),
        Section::Sets => return draw_sets(f, r, &mut t.rows, detail_focused, body),
        Section::Tempo => tempo_lines(r, w),
        Section::Arc => arc_lines(r, w),
        Section::Mixing => mixing_lines(r, w),
        Section::Keys => keys_lines(r, w),
        Section::Energy => energy_lines(r, w),
        Section::Genres | Section::Labels | Section::Artists => {
            return draw_ranked(f, v, section, &mut t.rows, detail_focused, body);
        }
        Section::Era => era_lines(r, w),
        Section::Signature => return draw_signature(f, v, &mut t.rows, detail_focused, body),
        Section::Prep => prep_lines(r, w),
        Section::Collection => collection_lines(r),
        Section::Style => style_lines(r),
    };
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
}

fn summary_lines(v: &ProfileView) -> Vec<Line<'static>> {
    let r = &v.report;
    let mut lines = vec![Line::styled(r.summary.headline.clone(), bold(theme::FG))];
    if let Some(h) = &v.prefs.headline {
        lines
            .push(Line::from(vec![Span::styled("Your headline: ", dim()), Span::styled(h.clone(), bold(theme::CYAN))]));
    } else {
        lines.push(note("Your own: locallibrary profile headline \"…\""));
    }
    lines.push(Line::raw(""));
    if r.summary.tags.is_empty() {
        lines.push(note("No trait stands out yet. Traits need evidence: 20+ transitions, 3+ sets, 20+ dated plays."));
    }
    for t in &r.summary.tags {
        lines.push(Line::from(vec![
            Span::styled("✓ ", bold(theme::GREEN)),
            Span::styled(t.label.clone(), bold(theme::FG)),
        ]));
        lines.push(Line::styled(format!("  {}", t.why), dim()));
    }
    lines
}

fn draw_sets(f: &mut Frame, r: &ProfileReport, rows: &mut TableState, focused: bool, area: Rect) {
    let s = &r.sets;
    let unit = r.basis.unit();
    let mut head = vec![Line::from(vec![
        Span::styled(plural(s.count, unit), bold(theme::FG)),
        Span::styled(
            format!(
                " · {} tracks in a typical {unit} (longest {}) · {} plays of {} tracks",
                s.median_tracks, s.longest, s.plays, s.distinct_tracks
            ),
            text(),
        ),
    ])];
    let mut notes = Vec::new();
    if r.basis == Basis::Sessions {
        notes.push("no sets declared (locallibrary sets new --from ID declares one)".to_string());
    }
    if s.not_archived > 0 {
        notes.push(format!("{} only on a drive (pull archives them)", s.not_archived));
    }
    if s.too_short > 0 {
        notes.push(format!("{} left out", plural(s.too_short, "shorter session")));
    }
    head.push(note(notes.join(" · ")));
    let [h, table] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
    f.render_widget(Paragraph::new(head), h);
    if s.list.is_empty() {
        f.render_widget(Paragraph::new(note("No sets declared, and no sessions with enough tracks.")), table);
        return;
    }
    let data: Vec<Row> = s
        .list
        .iter()
        .map(|set| {
            let when = set.date.map_or_else(|| "on a drive".to_string(), |d| d.to_string());
            let bpm = match (set.opening_bpm, set.closing_bpm) {
                (Some(a), Some(b)) => format!("{a:.0} → {b:.0}"),
                _ => String::new(),
            };
            Row::new(vec![
                Cell::from(Span::styled(when, fg(theme::AMBER))),
                Cell::from(Span::styled(set.name.clone(), text())),
                Cell::from(Line::styled(set.tracks.to_string(), text()).right_aligned()),
                Cell::from(Span::styled(bpm, fg(theme::CYAN))),
                Cell::from(Line::styled(set.harmonic_share.map(pct).unwrap_or_default(), text()).right_aligned()),
                Cell::from(Span::styled(set.place.clone(), dim())),
                Cell::from(Span::styled(set.id.get(..8).unwrap_or(&set.id).to_string(), faint())),
            ])
        })
        .collect();
    let header = match r.basis {
        Basis::Sessions => ["First seen", "Session", "Tracks", "BPM", "In key", "Drive", "Id"],
        Basis::Plays | Basis::Collection => ["Date", "Set", "Tracks", "BPM", "In key", "Where", "Id"],
    };
    let t = Table::new(
        data,
        [
            Constraint::Length(10),
            Constraint::Fill(2),
            Constraint::Length(6),
            Constraint::Length(9),
            Constraint::Length(6),
            Constraint::Fill(1),
            Constraint::Length(8),
        ],
    )
    .header(Row::new(header.into_iter().map(|h| Cell::from(Span::styled(h, bold(theme::DIM))))))
    .column_spacing(2)
    .row_highlight_style(if focused { theme::selected() } else { Style::new() })
    .highlight_symbol(Span::styled("▍", fg(if focused { theme::PINK } else { theme::FAINT })));
    // Under the header row.
    rows.fit(table.height.saturating_sub(1) as usize, s.list.len());
    f.render_stateful_widget(t, table, rows);
}

fn tempo_lines(r: &ProfileReport, w: usize) -> Vec<Line<'static>> {
    let t = &r.tempo;
    let (Some(core), Some(median)) = (t.core, t.median) else {
        return vec![note("No BPMs: tracks need rekordbox's analysis.")];
    };
    let (lo, hi) = (core.low, core.high);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(profile::bpm_range(lo, hi), bold(theme::CYAN)),
            Span::styled(
                format!(
                    " in the middle 80% · median {median:.1} · half between {:.1} and {:.1}",
                    t.p25.unwrap_or(median),
                    t.p75.unwrap_or(median)
                ),
                text(),
            ),
        ]),
        Line::raw(""),
    ];
    let n = t.histogram.len().max(1);
    let col_w = (w / n).clamp(2, 6);
    let values: Vec<Option<f64>> = t.histogram.iter().map(|b| Some(b.count as f64)).collect();
    let core = |i: usize| {
        let start = t.histogram[i].bpm;
        if start + t.bin_width > lo && start <= hi { theme::CYAN } else { theme::FAINT }
    };
    lines.extend(columns(&values, 0.0, 8, col_w, core));
    // Labels under every column that has room.
    let every = (5 / col_w).max(1);
    let labels: String = t
        .histogram
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let label = if i % every == 0 { format!("{:.0}", b.bpm) } else { String::new() };
            fit(&label, col_w).chars().take(col_w).collect::<String>()
        })
        .collect();
    lines.push(Line::styled(labels, dim()));
    lines.push(note(format!("BPM, {:.0} per column · cyan: the middle 80%", t.bin_width)));
    lines
}

fn arc_lines(r: &ProfileReport, w: usize) -> Vec<Line<'static>> {
    let a = &r.arc;
    let unit = r.basis.unit();
    if a.sets == 0 {
        return vec![note(format!("Needs {unit}s of 6 or more tracks with BPMs."))];
    }
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{:.1} → {:.1} BPM", a.opening_bpm.unwrap_or(0.0), a.closing_bpm.unwrap_or(0.0)),
                bold(theme::CYAN),
            ),
            Span::styled(format!("  opening and closing quarter, averaged over {}", plural(a.sets, unit)), text()),
        ]),
        Line::raw(""),
    ];
    let bw = w.saturating_sub(20).clamp(10, 40);
    lines.push(bar_line("builds", 12, a.builds, bw, theme::GREEN, pct(a.builds)));
    lines.push(bar_line("steady", 12, a.steady, bw, theme::CYAN, pct(a.steady)));
    lines.push(bar_line("winds down", 12, a.winds_down, bw, theme::AMBER, pct(a.winds_down)));
    lines.push(Line::raw(""));
    let lo = a.curve.iter().flatten().copied().fold(f64::INFINITY, f64::min);
    if lo.is_finite() {
        let col_w = (w / 10).clamp(3, 8);
        lines.extend(columns(&a.curve, lo - 0.5, 6, col_w, |_| theme::VIOLET));
        let mut axis = fit("start", col_w * 5);
        axis.push_str(&format!("{:>width$}", "end", width = col_w * 5 - 1));
        lines.push(Line::styled(axis, dim()));
        let hi = a.curve.iter().flatten().copied().fold(f64::NEG_INFINITY, f64::max);
        lines.push(note(format!("BPM against each {unit}'s median, through a {unit}: {lo:+.1} to {hi:+.1}")));
    }
    lines
}

fn mixing_lines(r: &ProfileReport, w: usize) -> Vec<Line<'static>> {
    let m = &r.mixing;
    if m.transitions == 0 {
        return vec![note(format!("Needs {}s.", r.basis.unit()))];
    }
    let mut lines = Vec::new();
    if m.keyed > 0 {
        lines.push(Line::from(vec![
            Span::styled(opt_pct(m.harmonic_share), bold(theme::CYAN)),
            Span::styled(format!(" of {} transitions between keyed tracks are harmonic", m.keyed), text()),
        ]));
        lines.push(Line::raw(""));
        let parts = [
            ("same key", m.same_key, theme::GREEN),
            ("±1", m.adjacent, theme::CYAN),
            ("relative", m.relative, theme::VIOLET),
            ("energy boost", m.energy_boost, theme::AMBER),
            ("clash", m.clash, theme::RED),
        ];
        let width = w.clamp(10, 60);
        let mut stacked = Vec::new();
        let mut used = 0;
        for (i, (_, n, c)) in parts.iter().enumerate() {
            let cells = if i == parts.len() - 1 {
                width - used
            } else {
                ((*n as f64 / m.keyed as f64) * width as f64).round() as usize
            };
            let cells = cells.min(width - used);
            used += cells;
            stacked.push(Span::styled("█".repeat(cells), fg(*c)));
        }
        lines.push(Line::from(stacked));
        let legend: Vec<Span<'static>> = parts
            .iter()
            .flat_map(|(name, n, c)| [Span::styled("■ ", fg(*c)), Span::styled(format!("{name} {n}   "), dim())])
            .collect();
        lines.push(Line::from(legend));
        lines.push(Line::raw(""));
    }
    if m.timed > 0 {
        lines.push(Line::styled(
            format!(
                "Tempo steps: median {:.1}% · {} within 3% · {} within 6%{}",
                m.median_step_pct.unwrap_or(0.0),
                opt_pct(m.within_3pct),
                opt_pct(m.within_6pct),
                if m.half_double > 0 { format!(" · {} at half or double time", m.half_double) } else { String::new() }
            ),
            text(),
        ));
    }
    if m.genred > 0 {
        lines.push(Line::styled(format!("{} of transitions change genre", opt_pct(m.genre_change_share)), text()));
    }
    lines
}

fn keys_lines(r: &ProfileReport, w: usize) -> Vec<Line<'static>> {
    let k = &r.keys;
    if k.tracks == 0 {
        return vec![note("No keys: tracks need rekordbox's analysis.")];
    }
    let cell = (w.saturating_sub(3) / 12).clamp(3, 6);
    let max = k.wheel.iter().map(|x| x.count).max().unwrap_or(1).max(1);
    let mut lines = vec![Line::from(
        std::iter::once(Span::raw("   "))
            .chain((1..=12).map(|n| Span::styled(format!("{n:^cell$}"), dim())))
            .collect::<Vec<_>>(),
    )];
    for (row, letter) in [(0usize, "A"), (1, "B")] {
        let mut spans = vec![Span::styled(format!(" {letter} "), dim())];
        for n in 0..12 {
            let count = k.wheel[n * 2 + row].count;
            let t = count as f32 / max as f32;
            let bg = theme::mix(theme::VINYL, theme::CYAN, t.sqrt());
            let fgc = if t > 0.45 { [16, 16, 22] } else { theme::FG };
            let label = if count == 0 { "·".to_string() } else { count.to_string() };
            spans.push(Span::styled(format!("{label:^cell$}"), Style::new().bg(rgb3(bg)).fg(rgb3(fgc))));
        }
        lines.push(Line::from(spans));
    }
    lines.push(note("   A: minor keys · B: major keys · neighbours on the wheel mix"));
    lines.push(Line::raw(""));
    let top: Vec<String> = k.top.iter().take(5).map(|s| format!("{} {}", s.name, pct(s.share))).collect();
    lines.push(Line::styled(format!("Most played: {}", top.join(" · ")), text()));
    lines.push(Line::styled(format!("Minor keys: {}", opt_pct(k.minor_share)), text()));
    lines
}

fn energy_lines(r: &ProfileReport, w: usize) -> Vec<Line<'static>> {
    let Some(e) = &r.energy else {
        return vec![note("No phrase analysis read: it needs the drives' analysis files.")];
    };
    let bw = w.saturating_sub(16).clamp(10, 40);
    let mut lines = vec![
        note(format!("rekordbox's phrase-analysis mood, {} tracks", e.tracks)),
        Line::raw(""),
        bar_line("high", 6, e.high, bw, theme::PINK, pct(e.high)),
        bar_line("mid", 6, e.mid, bw, theme::VIOLET, pct(e.mid)),
        bar_line("low", 6, e.low, bw, theme::CYAN, pct(e.low)),
        Line::raw(""),
    ];
    if e.curve.iter().flatten().count() > 1 {
        let col_w = (w / 10).clamp(3, 8);
        lines.extend(columns(&e.curve, 0.0, 5, col_w, |_| theme::PINK));
        let mut axis = fit("start", col_w * 5);
        axis.push_str(&format!("{:>width$}", "end", width = col_w * 5 - 1));
        lines.push(Line::styled(axis, dim()));
        lines.push(note(format!("Mood through a {} (high 1, mid ½, low 0)", r.basis.unit())));
    }
    lines
}

fn draw_ranked(f: &mut Frame, v: &ProfileView, s: Section, rows: &mut TableState, focused: bool, area: Rect) {
    let r = &v.report;
    let ranked: &RankedSection = match s {
        Section::Genres => &r.genres,
        Section::Labels => &r.labels,
        _ => &r.artists,
    };
    let noun = match r.basis {
        Basis::Plays | Basis::Sessions => "plays",
        Basis::Collection => "tracks",
    };
    let hint = match s {
        Section::Artists => " · h hides an artist everywhere",
        Section::Genres => " · locallibrary profile alias spells a genre your way",
        _ => "",
    };
    let [h, table] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
    f.render_widget(
        Paragraph::new(note(format!("Shares of {} {noun} · {} different{hint}", ranked.known, ranked.distinct))),
        h,
    );
    if ranked.top.is_empty() {
        f.render_widget(Paragraph::new(note("None tagged.")), table);
        return;
    }
    let max = ranked.top.iter().map(|x| x.share).fold(0.0, f64::max).max(f64::EPSILON);
    let bw = (area.width as usize).saturating_sub(44).clamp(8, 40);
    let data: Vec<Row> = ranked
        .top
        .iter()
        .map(|x| {
            let hidden = s == Section::Artists && v.prefs.is_hidden_artist(&x.name);
            let color = match s {
                Section::Genres => theme::CYAN,
                Section::Labels => theme::AMBER,
                _ => theme::VIOLET,
            };
            Row::new(vec![
                Cell::from(Span::styled(x.name.clone(), if hidden { faint() } else { text() })),
                Cell::from(Span::styled(bar(x.share / max, bw), fg(if hidden { theme::FAINT } else { color }))),
                Cell::from(Line::styled(pct(x.share), text()).right_aligned()),
                Cell::from(Span::styled(if hidden { "hidden" } else { "" }, faint())),
            ])
        })
        .collect();
    let t = Table::new(
        data,
        [Constraint::Length(26), Constraint::Length(bw as u16), Constraint::Length(5), Constraint::Length(7)],
    )
    .header(Row::new(["Name", "", "Share", ""].into_iter().map(|h| Cell::from(Span::styled(h, bold(theme::DIM))))))
    .column_spacing(1)
    .row_highlight_style(if focused { theme::selected() } else { Style::new() })
    .highlight_symbol(Span::styled("▍", fg(if focused { theme::PINK } else { theme::FAINT })));
    rows.fit(table.height.saturating_sub(1) as usize, ranked.top.len());
    f.render_stateful_widget(t, table, rows);
}

fn era_lines(r: &ProfileReport, w: usize) -> Vec<Line<'static>> {
    let e = &r.era;
    if e.tracks == 0 {
        return vec![note("No release years tagged.")];
    }
    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("median {}", e.median_year.unwrap_or(0)), bold(theme::CYAN)),
            Span::styled(
                format!(
                    " ({}–{}) · {} released the year played or the year before",
                    e.oldest.unwrap_or(0),
                    e.newest.unwrap_or(0),
                    opt_pct(e.recent_share)
                ),
                text(),
            ),
        ]),
        Line::raw(""),
    ];
    let max = e.decades.iter().map(|d| d.count).max().unwrap_or(1).max(1);
    let bw = w.saturating_sub(16).clamp(10, 40);
    for (decade, n) in e.decades.iter().map(|d| (&d.decade, &d.count)) {
        lines.push(bar_line(&format!("{decade}s"), 6, *n as f64 / max as f64, bw, theme::AMBER, n.to_string()));
    }
    lines
}

fn draw_signature(f: &mut Frame, v: &ProfileView, rows: &mut TableState, focused: bool, area: Rect) {
    let r = &v.report;
    let s = &r.signature;
    let p = &v.prefs;
    let unit = r.basis.unit();
    let moves = s.transitions.len().min(4);
    let [h, table, foot] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(if moves > 0 { moves as u16 + 1 } else { 0 }),
    ])
    .areas(area);
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("★ pinned ", fg(theme::AMBER)),
                Span::styled(format!("tracks lead the document's list, then those in the most {unit}s. "), dim()),
                Span::styled("m", theme::key()),
                Span::styled(" pin  ", dim()),
                Span::styled("h", theme::key()),
                Span::styled(" hide  ", dim()),
                Span::styled("H", theme::key()),
                Span::styled(" hide artist", dim()),
            ]),
            note(format!(
                "{} tracks played · {} in two or more of {} {unit}s",
                s.played.len(),
                s.tracks.len(),
                r.sets.count
            )),
        ]),
        h,
    );
    let data: Vec<Row> = s
        .played
        .iter()
        .map(|t| {
            let (mark, mark_style) = if !p.may_name(&t.name, &t.artist) {
                ("⊘", fg(theme::RED))
            } else if p.is_pinned(&t.name) {
                ("★", fg(theme::AMBER))
            } else if t.sets >= 2 {
                ("•", fg(theme::GREEN))
            } else {
                (" ", faint())
            };
            let name_style = if !p.may_name(&t.name, &t.artist) { faint() } else { text() };
            Row::new(vec![
                Cell::from(Span::styled(mark, mark_style)),
                Cell::from(Line::styled(t.sets.to_string(), text()).right_aligned()),
                Cell::from(Line::styled(t.plays.to_string(), dim()).right_aligned()),
                Cell::from(
                    Line::styled(if t.opened > 0 { t.opened.to_string() } else { String::new() }, dim())
                        .right_aligned(),
                ),
                Cell::from(
                    Line::styled(if t.closed > 0 { t.closed.to_string() } else { String::new() }, dim())
                        .right_aligned(),
                ),
                Cell::from(Span::styled(t.name.clone(), name_style)),
            ])
        })
        .collect();
    let in_how_many = if r.basis == Basis::Sessions { "Sessions" } else { "Sets" };
    let header = ["", in_how_many, "Plays", "Opened", "Closed", "Track"];
    let t = Table::new(
        data,
        [
            Constraint::Length(1),
            Constraint::Length(in_how_many.len() as u16),
            Constraint::Length(5),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Fill(1),
        ],
    )
    .header(Row::new(header.into_iter().map(|h| Cell::from(Span::styled(h, bold(theme::DIM))))))
    .column_spacing(1)
    .row_highlight_style(if focused { theme::selected() } else { Style::new() })
    .highlight_symbol(Span::styled("▍", fg(if focused { theme::PINK } else { theme::FAINT })));
    rows.fit(table.height.saturating_sub(1) as usize, s.played.len());
    f.render_stateful_widget(t, table, rows);
    if moves > 0 {
        let mut lines = vec![note("Moves played again and again")];
        for m in s.transitions.iter().take(moves) {
            lines.push(Line::from(vec![
                Span::styled(format!("{:>2}× ", m.sets), fg(theme::AMBER)),
                Span::styled(format!("{} → {}", m.from, m.to), text()),
            ]));
        }
        f.render_widget(Paragraph::new(lines), foot);
    }
}

fn prep_lines(r: &ProfileReport, w: usize) -> Vec<Line<'static>> {
    let Some(p) = &r.prep else {
        return vec![note("No cues read: it needs the drives' analysis files.")];
    };
    let bw = w.saturating_sub(24).clamp(10, 40);
    vec![
        note(format!("Cues across {} tracks in the collection", p.tracks)),
        Line::raw(""),
        bar_line(
            "hot cues",
            12,
            p.hot_cue_share,
            bw,
            theme::HOT_CUE,
            format!(
                "{}{}",
                pct(p.hot_cue_share),
                p.hot_cues_per_track.map(|n| format!(" · {n:.1} each")).unwrap_or_default()
            ),
        ),
        bar_line("memory cues", 12, p.memory_cue_share, bw, theme::MEMORY_CUE, pct(p.memory_cue_share)),
        bar_line("loops", 12, p.loop_share, bw, theme::LOOP, pct(p.loop_share)),
    ]
}

fn collection_lines(r: &ProfileReport) -> Vec<Line<'static>> {
    let c = &r.collection;
    let row = |k: &str, v: String| Line::from(vec![Span::styled(fit(k, 20), dim()), Span::styled(v, text())]);
    let mut lines = vec![
        row("Tracks", c.tracks.to_string()),
        row("Playlists", format!("{} in {} folders", c.playlists, c.folders)),
        row("Rated", opt_pct(c.rated_share)),
        row("Colour-tagged", opt_pct(c.coloured_share)),
    ];
    if let Some(p) = c.played_share {
        lines.push(row(&format!("Played in a {}", r.basis.unit()), pct(p)));
    }
    if c.duplicate_files > 0 {
        lines.push(Line::raw(""));
        lines.push(note(format!(
            "{} more files are copies of these and count once (locallibrary doctor lists them)",
            c.duplicate_files
        )));
    }
    lines
}

fn style_lines(r: &ProfileReport) -> Vec<Line<'static>> {
    let Some(s) = &r.style else {
        return vec![note("No My Settings on these drives (locallibrary settings import sets a profile).")];
    };
    let mut lines: Vec<Line<'static>> = s
        .values
        .iter()
        .map(|v| {
            Line::from(vec![
                Span::styled(fit(&v.label, 22), dim()),
                Span::styled(v.value_label.clone(), bold(theme::FG)),
            ])
        })
        .collect();
    lines.push(Line::raw(""));
    lines.push(note(format!("From {}", s.source)));
    lines
}

/// The document as coloured JSON.
fn draw_document(f: &mut Frame, v: &ProfileView, scroll: u16, area: Rect) {
    let json = serde_json::to_string_pretty(&v.document).unwrap_or_default();
    let lines: Vec<Line<'static>> = json.lines().map(json_line).collect();
    let b = block(title("Document", true), true)
        .title(Line::from(Span::styled(" what a public profile would show · d closes ", faint())).right_aligned());
    let max = (lines.len() as u16).saturating_sub(area.height.saturating_sub(2));
    f.render_widget(Paragraph::new(lines).block(b).scroll((scroll.min(max), 0)), area);
}

fn json_line(line: &str) -> Line<'static> {
    let indent = line.len() - line.trim_start().len();
    let rest = line.trim_start();
    let mut spans = vec![Span::raw(" ".repeat(indent))];
    let (key, value) = match rest.strip_prefix('"').and_then(|r| r.split_once("\": ")) {
        Some((k, v)) => (Some(k), v),
        None => (None, rest),
    };
    if let Some(k) = key {
        spans.push(Span::styled(format!("\"{k}\""), fg(theme::CYAN)));
        spans.push(Span::styled(": ", faint()));
    }
    let style = if value.starts_with('"') {
        fg(theme::GREEN)
    } else if value.starts_with(|c: char| c.is_ascii_digit() || c == '-') {
        fg(theme::AMBER)
    } else if value.starts_with("true") || value.starts_with("false") || value.starts_with("null") {
        fg(theme::VIOLET)
    } else {
        faint()
    };
    spans.push(Span::styled(value.to_string(), style));
    Line::from(spans)
}

#[cfg(test)]
mod tests;
