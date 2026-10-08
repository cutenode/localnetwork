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
mod tests {
    use ratatui::crossterm::event::KeyCode;

    use super::super::app::View;
    use super::super::app::testing::*;
    use super::super::ui::screen;
    use super::*;

    /// The view drawn at `w`×`h`, as text.
    fn render(t: &mut ProfileTab, w: u16, h: u16) -> String {
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw_tab(f, t, f.area())).unwrap();
        let buf = term.backend().buffer().clone();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// A tab holding the profile of the fixture export (twelve tracks, no history).
    fn fixture_tab() -> ProfileTab {
        fixture_tab_with(&[])
    }

    /// [`fixture_tab`], with history lists of the export's tracks.
    fn fixture_tab_with(history: &[&[u32]]) -> ProfileTab {
        let dir = tempfile::tempdir().unwrap();
        let pdb = dir.path().join("PIONEER/rekordbox/export.pdb");
        std::fs::create_dir_all(pdb.parent().unwrap()).unwrap();
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/rekordbox-pdb/tests/data/bigger-export.pdb");
        std::fs::copy(fixture, &pdb).unwrap();
        let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb).unwrap();
        for (i, tracks) in history.iter().enumerate() {
            let id = ed.create_history_playlist(&format!("HISTORY {:03}", i + 1)).unwrap();
            for t in *tracks {
                ed.add_to_history(id, *t).unwrap();
            }
        }
        ed.save(&pdb).unwrap();
        let req = ProfileRequest { drives: vec![dir.path().display().to_string()], ..Default::default() };
        let view = api::profile(None, &req, &Ctx::silent()).unwrap();
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut t = ProfileTab::new(tx);
        t.view = Some(Ok(view));
        t
    }

    #[test]
    fn every_section_draws_at_any_size() {
        let mut t = fixture_tab();
        for (w, h) in [(160, 48), (80, 24), (40, 12), (10, 4)] {
            for i in 0..Section::ALL.len() {
                t.select_section(i);
                let screen = render(&mut t, w, h);
                if w >= 80 {
                    assert!(screen.contains(Section::ALL[i].title()), "{}:\n{screen}", Section::ALL[i].title());
                }
            }
            t.document = true;
            let screen = render(&mut t, w, h);
            if w >= 80 {
                assert!(screen.contains("\"version\": 1"), "{screen}");
            }
            t.document = false;
        }
        t.select_section(Section::ALL.iter().position(|s| *s == Section::Tempo).unwrap());
        let screen = render(&mut t, 120, 40);
        assert!(screen.contains("124–128 BPM"), "{screen}");
    }

    /// Without declared sets, the view counts sessions and calls them that.
    #[test]
    fn sessions_are_called_sessions() {
        let mut t = fixture_tab_with(&[&[1, 2, 3, 4, 5, 6], &[9, 10, 1, 2, 12, 7]]);
        t.select_section(Section::ALL.iter().position(|s| *s == Section::Sets).unwrap());
        let screen = render(&mut t, 160, 30);
        assert!(screen.contains("no sets declared: 12 plays in 2 sessions"), "{screen}");
        assert!(
            screen.contains("Session shape") && screen.contains("2 sessions · 6 tracks in a typical session"),
            "{screen}"
        );
        assert!(screen.contains("First seen") && screen.contains("HISTORY 001"), "{screen}");
        assert!(screen.contains("only on this computer") && !screen.contains("Set shape"), "{screen}");
        assert!(!screen.contains(" Sets ") && !screen.contains("2 sets"), "{screen}");
    }

    #[test]
    fn keys_change_the_view_and_save_choices() {
        use super::super::app::View;

        let store = tempfile::tempdir().unwrap();
        let mut app = App::blank(store.path().to_path_buf(), None);
        app.profile = fixture_tab();
        app.view = View::Profile;
        let published = |app: &App| match &app.profile.view {
            Some(Ok(v)) => v.published.clone(),
            _ => panic!("no profile"),
        };

        key(&mut app, KeyCode::Down);
        assert_eq!(app.profile.section(), Section::Sets);
        assert!(published(&app).contains(&Section::Sets));
        press(&mut app, "i");
        assert!(!published(&app).contains(&Section::Sets));
        let saved = locallibrary::profile::ProfilePrefs::load(&Store::open(store.path()).unwrap()).unwrap();
        assert!(!saved.published().contains(&Section::Sets), "choices are saved as they're made");

        // Into the artist list, and hide the top artist.
        let artists = Section::ALL.iter().position(|s| *s == Section::Artists).unwrap();
        app.profile.select_section(artists);
        key(&mut app, KeyCode::Tab);
        assert_eq!(app.profile.pane, Pane::Detail);
        press(&mut app, "h");
        let Some(Ok(v)) = &app.profile.view else { panic!() };
        assert_eq!(v.prefs.hidden_artists, ["[a]pendics.shuffle"]);
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.profile.pane, Pane::Sections);

        press(&mut app, "#");
        let Some(Ok(v)) = &app.profile.view else { panic!() };
        assert_eq!(v.document.precision, Precision::Exact);
        press(&mut app, "d");
        assert!(!app.profile.document, "d picks the output, as everywhere");
        app.deck.outputs = None;
        press(&mut app, "D");
        assert!(app.profile.document);
        key(&mut app, KeyCode::Esc);
        assert!(!app.profile.document);

        // Keys the view doesn't use still work.
        press(&mut app, "1");
        assert_eq!(app.view, View::Library);
        press(&mut app, "4");
        assert_eq!(app.view, View::Profile);
        press(&mut app, "q");
        assert!(app.quit);
    }

    /// Signed out, the account bar names the key that signs in: `L` (`l` loops the track).
    #[cfg(feature = "account")]
    #[test]
    fn the_account_bar_says_which_key_signs_in() {
        let line: String = Account::default().line(200).spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(line.contains("  L sign in with your Atmosphere account"), "{line}");
    }

    /// A long, wide top genre (the user's own text) is cut to fit the Sections column, and the
    /// section's name stays whole.
    #[test]
    fn a_long_top_genre_fits_the_sections_column() {
        let mut t = fixture_tab();
        let Some(Ok(v)) = &mut t.view else { panic!("no profile") };
        let name = "日本の夜明けとともに始まる長いジャンル".to_string();
        v.report.genres.top.insert(0, locallibrary::profile::Share { name, count: 1, share: 1.0 });
        let screen = render(&mut t, 120, 40);
        // A wide character's second column draws as a space.
        let row = screen.lines().find(|l| l.contains("日 本")).unwrap_or_else(|| panic!("{screen}"));
        assert!(row.contains("Genres") && row.contains('…'), "{row}");
    }

    /// Bars of wide characters fit their columns like any other (`fit` pads to exactly `width`).
    #[test]
    fn fit_counts_columns() {
        use unicode_width::UnicodeWidthStr;
        for s in ["Friday", "日本の夜明けとともに", "ab😹😹😹"] {
            for width in 0..12 {
                assert_eq!(fit(s, width).width(), width, "{s:?} in {width}");
            }
        }
    }

    #[cfg(feature = "account")]
    #[test]
    fn signing_in_takes_a_handle_and_signing_out_two_presses() {
        use super::super::app::View;

        let store = tempfile::tempdir().unwrap();
        let mut app = App::blank(store.path().to_path_buf(), None);
        app.profile = fixture_tab();
        app.view = View::Profile;
        assert!(render(&mut app.profile, 120, 30).contains("Not signed in"));

        press(&mut app, "L");
        press(&mut app, "dj.example.comq");
        assert!(!app.quit, "typing a handle doesn't run commands");
        key(&mut app, KeyCode::Backspace);
        let screen = render(&mut app.profile, 120, 30);
        assert!(screen.contains("Atmosphere handle or server: dj.example.com"), "{screen}");
        key(&mut app, KeyCode::Esc);
        assert!(!app.profile.account.typing());

        let account: locallibrary::api::AccountInfo = serde_json::from_value(serde_json::json!({
            "did": "did:plc:abc",
            "handle": "dj.example.com",
            "pds": "https://pds.example",
            "auth_server": "https://pds.example",
            "scopes": ["atproto"],
            "signed_in_at": "2026-10-05T12:00:00Z",
            "sign_in_again_by": "2099-10-19T12:00:00Z",
            "access_expires_at": null,
            "client": { "kind": "loopback" },
            "storage": "file"
        }))
        .unwrap();
        app.profile.account.info = Some(account);
        let screen = render(&mut app.profile, 140, 30);
        assert!(screen.contains("Atmosphere account @dj.example.com as the development client"), "{screen}");
        assert!(screen.contains("sign in again by"), "{screen}");
        press(&mut app, "L");
        assert!(app.toast.as_ref().is_some_and(|t| t.0.contains("signed in as @dj.example.com: press L again")));
        press(&mut app, "L");
        assert!(app.profile.account.busy(), "the second press signs out");

        // The sign-out thread's answer: nobody was signed in in this store folder.
        let done = super::super::account::AccountEvent::Done { message: "not signed in".into(), error: false };
        super::super::account::on_event(&mut app, done);
        assert!(app.profile.account.info.is_none() && !app.profile.account.busy());
    }

    #[test]
    fn loading_and_errors_draw() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut t = ProfileTab::new(tx);
        t.loading = true;
        assert!(render(&mut t, 80, 10).contains("Building profile"));
        t.view = Some(Err("no store".into()));
        assert!(render(&mut t, 80, 10).contains("R tries again"));
    }

    #[test]
    fn columns_stack_eighths() {
        let lines = columns(&[Some(0.0), Some(1.0), None], 0.0, 2, 2, |_| theme::CYAN);
        assert_eq!(lines.len(), 2);
        let top: String = lines[0].spans.iter().map(|s| s.content.to_string()).collect();
        let bottom: String = lines[1].spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(top, "  █   ");
        assert_eq!(bottom, "▁ █   ");
    }

    #[test]
    fn json_lines_colour_keys_and_values() {
        let l = json_line("  \"headline\": \"Techno warm-ups\",");
        let text: String = l.spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text, "  \"headline\": \"Techno warm-ups\",");
        assert_eq!(json_line("}").spans.len(), 2);
    }

    /// [`fixture_tab_with`] two sessions, as a profile of declared sets, every section filled
    /// in with made-up numbers.
    fn full_tab() -> ProfileTab {
        use locallibrary::profile::{
            ArcSection, DecadeCount, EnergySection, MixingSection, PrepSection, SetInfo, SignatureTransition,
            StyleSection, StyleValue, Tag,
        };
        let mut t = fixture_tab_with(&[&[1, 2, 3, 4, 5, 6], &[9, 10, 1, 2, 12, 7]]);
        let Some(Ok(v)) = &mut t.view else { panic!("no profile") };
        let r = &mut v.report;
        r.basis = Basis::Plays;
        r.summary.tags =
            vec![Tag { id: "harmonic", label: "Mixes in key".into(), why: "77% of 30 transitions".into() }];
        (r.sets.not_archived, r.sets.too_short) = (1, 2);
        r.sets.list.push(SetInfo {
            id: "0123456789abcdef".into(),
            name: "Friday at the Bunker".into(),
            place: "DNA".into(),
            date: None,
            tracks: 6,
            opening_bpm: Some(122.0),
            closing_bpm: Some(128.0),
            harmonic_share: Some(0.5),
        });
        r.arc = ArcSection {
            sets: 2,
            opening_bpm: Some(122.0),
            closing_bpm: Some(127.5),
            builds: 0.6,
            steady: 0.3,
            winds_down: 0.1,
            curve: vec![Some(-2.0), Some(-1.0), None, Some(1.5)],
        };
        r.mixing = MixingSection {
            transitions: 40,
            keyed: 30,
            same_key: 10,
            adjacent: 8,
            relative: 3,
            energy_boost: 2,
            clash: 7,
            harmonic_share: Some(0.77),
            timed: 30,
            median_step_pct: Some(1.2),
            within_3pct: Some(0.8),
            within_6pct: Some(0.95),
            half_double: 1,
            genred: 20,
            genre_change_share: Some(0.35),
        };
        r.energy = Some(EnergySection { tracks: 12, high: 0.5, mid: 0.3, low: 0.2, curve: vec![Some(0.2), Some(1.0)] });
        r.era.tracks = 10;
        (r.era.median_year, r.era.oldest, r.era.newest, r.era.recent_share) =
            (Some(2019), Some(1998), Some(2024), Some(0.3));
        r.era.decades = vec![DecadeCount { decade: 1990, count: 2 }, DecadeCount { decade: 2010, count: 8 }];
        r.signature.transitions =
            vec![SignatureTransition { from: "Ferrule - Night Bus".into(), to: "Ferrule - Halogen".into(), sets: 3 }];
        r.prep = Some(PrepSection {
            tracks: 12,
            hot_cue_share: 0.75,
            hot_cues_per_track: Some(3.5),
            memory_cue_share: 0.5,
            loop_share: 0.1,
        });
        (r.collection.played_share, r.collection.duplicate_files) = (Some(0.4), 3);
        r.style = Some(StyleSection {
            source: "MYSETTING.DAT on DNA".into(),
            values: vec![StyleValue {
                key: "quantize".into(),
                label: "Quantize".into(),
                value: "on".into(),
                value_label: "On".into(),
            }],
        });
        v.prefs.headline = Some("Warm-up specialist".into());
        let played: Vec<String> = r.signature.played.iter().take(2).map(|p| p.name.clone()).collect();
        (v.prefs.pinned, v.prefs.hidden) = (vec![played[0].clone()], vec![played[1].clone()]);
        t
    }

    /// The screen with section `s` selected.
    fn showing(t: &mut ProfileTab, s: Section) -> String {
        t.select_section(Section::ALL.iter().position(|x| *x == s).unwrap());
        render(t, 140, 40)
    }

    #[test]
    fn every_section_says_what_its_numbers_show() {
        let mut t = full_tab();
        for (s, says) in [
            (
                Section::Summary,
                &["Warm-up specialist  (your headline)", "Your headline: Warm-up specialist", "77% of 30 transitions"]
                    [..],
            ),
            (
                Section::Sets,
                &[
                    "1 only on a drive (pull archives them) · 2 shorter sessions left out",
                    "on a drive",
                    "122 → 128",
                    "01234567",
                ],
            ),
            (Section::Arc, &["122.0 → 127.5 BPM", "averaged over 2 sets", "builds", "through a set: -2.0 to +1.5"]),
            (
                Section::Mixing,
                &[
                    "77% of 30 transitions between keyed tracks are harmonic",
                    "clash 7",
                    "median 1.2% · 80% within 3% · 95% within 6% · 1 at half or double time",
                    "35% of transitions change genre",
                ],
            ),
            (Section::Energy, &["mood, 12 tracks", "high", "50%", "Mood through a set"]),
            (Section::Era, &["median 2019", "(1998–2024) · 30% released the year played", "1990s", "2010s"]),
            (Section::Signature, &["Moves played again and again", " 3× Ferrule - Night Bus → Ferrule - Halogen"]),
            (Section::Prep, &["Cues across 12 tracks", "75% · 3.5 each", "memory cues"]),
            (Section::Collection, &["Played in a set", "40%", "3 more files are copies of these"]),
            (Section::Style, &["Quantize", "On", "From MYSETTING.DAT on DNA"]),
        ] {
            let screen = showing(&mut t, s);
            for said in says {
                assert!(screen.contains(said), "{s:?}: {said}\n{screen}");
            }
        }
        // The pinned track is starred, the hidden one struck out.
        let Some(Ok(v)) = &t.view else { panic!("no profile") };
        let (pinned, hidden) = (v.prefs.pinned[0].clone(), v.prefs.hidden[0].clone());
        let screen = showing(&mut t, Section::Signature);
        let row = |name: &str| screen.lines().find(|l| l.contains(name)).unwrap_or_else(|| panic!("{name}\n{screen}"));
        assert!(row(&pinned).contains('★') && row(&hidden).contains('⊘'), "{screen}");
    }

    /// A section without the data it needs says what it needs.
    #[test]
    fn empty_sections_say_what_they_need() {
        let mut t = fixture_tab();
        let Some(Ok(v)) = &mut t.view else { panic!("no profile") };
        let r = &mut v.report;
        (r.tempo.core, r.keys.tracks, r.era.tracks, r.energy, r.prep, r.style) = (None, 0, 0, None, None, None);
        r.genres.top.clear();
        for (s, says) in [
            (Section::Tempo, "No BPMs: tracks need rekordbox's analysis."),
            (Section::Arc, "Needs sets of 6 or more tracks with BPMs."),
            (Section::Mixing, "Needs sets."),
            (Section::Keys, "No keys: tracks need rekordbox's analysis."),
            (Section::Energy, "No phrase analysis read"),
            (Section::Genres, "None tagged."),
            (Section::Era, "No release years tagged."),
            (Section::Prep, "No cues read"),
            (Section::Style, "No My Settings on these drives"),
            (Section::Sets, "No sets declared, and no sessions with enough tracks."),
            (Section::Summary, "No trait stands out yet."),
        ] {
            let screen = showing(&mut t, s);
            assert!(screen.contains(says), "{s:?}: {says}\n{screen}");
        }
    }

    /// The section list sums up how the sets run.
    #[test]
    fn the_arc_is_summed_up_in_a_word() {
        let mut t = full_tab();
        let Some(Ok(v)) = &mut t.view else { panic!("no profile") };
        let r = &mut v.report;
        let mut word = |builds, steady, winds_down| {
            (r.arc.builds, r.arc.steady, r.arc.winds_down) = (builds, steady, winds_down);
            stat(r, Section::Arc)
        };
        assert_eq!(word(0.6, 0.2, 0.2), "builds");
        assert_eq!(word(0.2, 0.6, 0.2), "steady");
        assert_eq!(word(0.2, 0.2, 0.6), "winds down");
        assert_eq!(word(0.4, 0.3, 0.3), "varies");
    }

    /// The app's Profile view showing `t`, its store in a temporary folder.
    fn profile_app(t: ProfileTab) -> (App, tempfile::TempDir) {
        let (mut app, dir) = blank(View::Profile);
        app.profile = t;
        (app, dir)
    }

    /// `m` pins a signature track and `h` hides it, each pressed again undoing it; `H` hides
    /// its artist. Outside a list, they say where to go.
    #[test]
    fn signature_tracks_are_pinned_and_hidden() {
        let (mut app, _store) = profile_app(fixture_tab_with(&[&[1, 2, 3, 4, 5, 6], &[9, 10, 1, 2, 12, 7]]));
        press(&mut app, "m");
        assert_eq!(last_message(&app), "select a track under Signature tracks (tab) to pin it");
        press(&mut app, "h");
        assert!(
            last_message(&app).starts_with("select a track under Signature tracks, or an artist"),
            "{}",
            last_message(&app)
        );

        app.profile.select_section(Section::ALL.iter().position(|s| *s == Section::Signature).unwrap());
        press(&mut app, "\t");
        let Some(Ok(v)) = &app.profile.view else { panic!("no profile") };
        let track = v.report.signature.played[0].clone();
        for (key, said) in [
            ('m', format!("pinned {}", track.name)),
            ('m', format!("unpinned {}", track.name)),
            ('h', format!("{} won't be named", track.name)),
            ('h', format!("{} can be named again", track.name)),
            ('H', format!("{} won't be named", track.artist)),
            ('H', format!("{} can be named again", track.artist)),
        ] {
            press(&mut app, &key.to_string());
            assert_eq!(last_message(&app), said);
        }
        let Some(Ok(v)) = &app.profile.view else { panic!("no profile") };
        assert!(v.prefs.pinned.is_empty() && v.prefs.hidden.is_empty() && v.prefs.hidden_artists.is_empty());

        // Among artists, m pins nothing.
        press(&mut app, "\u{1b}");
        app.profile.select_section(Section::ALL.iter().position(|s| *s == Section::Artists).unwrap());
        press(&mut app, "\t");
        press(&mut app, "m");
        assert_eq!(last_message(&app), "select a track under Signature tracks (tab) to pin it");
    }

    /// `w` writes the document into the store folder, made there if there's none.
    #[test]
    fn w_writes_the_document_to_the_store() {
        let (mut app, _dir) = profile_app(fixture_tab());
        press(&mut app, "w");
        let path = app.store_root.join("profile-document.json");
        assert_eq!(last_message(&app), format!("wrote {}", path.display()));
        let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(doc["version"], 1);

        let (tx, _rx) = std::sync::mpsc::channel();
        app.profile = ProfileTab::new(tx);
        press(&mut app, "w");
        assert_eq!(last_message(&app), "nothing to export yet");
    }

    /// A choice that can't be saved (the store folder is a file) says why, and changes nothing.
    #[test]
    fn a_choice_that_cant_be_saved_says_why() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("store");
        std::fs::write(&file, "not a folder").unwrap();
        let mut app = App::blank(file, None);
        app.profile = fixture_tab();
        app.view = View::Profile;
        let before = match &app.profile.view {
            Some(Ok(v)) => v.published.clone(),
            _ => panic!("no profile"),
        };
        press(&mut app, "i");
        assert!(app.toast.as_ref().is_some_and(|t| t.1 == Level::Error), "{:?}", app.toast);
        press(&mut app, "w");
        assert!(app.toast.as_ref().is_some_and(|t| t.1 == Level::Error), "{:?}", app.toast);
        let Some(Ok(v)) = &app.profile.view else { panic!("no profile") };
        assert_eq!(v.published, before);
    }

    /// `a` builds the profile from the open drive alone, in the background, and the header
    /// says how far it's got.
    #[test]
    fn a_builds_the_profile_from_the_open_drive() {
        let dir = tempfile::tempdir().unwrap();
        let pdb = dir.path().join("PIONEER/rekordbox/export.pdb");
        std::fs::create_dir_all(pdb.parent().unwrap()).unwrap();
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/rekordbox-pdb/tests/data/bigger-export.pdb");
        std::fs::copy(fixture, &pdb).unwrap();
        let (tx, _rx) = std::sync::mpsc::channel();
        let (mut app, _store) = profile_app(ProfileTab::new(tx));
        press(&mut app, "a");
        assert_eq!(last_message(&app), "open a drive in the library first");

        let mut app = App::with_library(dir.path(), library());
        app.view = View::Profile;
        press(&mut app, "a");
        let Scope::Drive(_, name) = app.profile.scope.clone() else { panic!("{:?}", app.profile.scope) };
        assert_eq!(last_message(&app), format!("profile from {name} only"));
        assert!(app.profile.loading);
        // The header says so while it builds.
        app.profile.progress[1].store(12, Ordering::Relaxed);
        app.profile.progress[0].store(3, Ordering::Relaxed);
        let s = screen(&mut app, 140, 30);
        assert!(s.contains("Building profile… 3/12 tracks"), "{s}");
        // A build started before this one is stopped, and what it sends back ignored.
        let old = app.profile.generation;
        press(&mut app, "R");
        app.profile.loaded(old, Err("stale".into()));
        assert!(app.profile.loading && app.profile.view.is_none());

        wait(&mut app, |a| !a.profile.loading);
        assert!(matches!(&app.profile.view, Some(Ok(v)) if v.report.collection.tracks == 12), "{:?}", app.toast);
        let s = screen(&mut app, 140, 30);
        assert!(s.contains(&format!("Profile · {name} only")), "{s}");
    }

    /// The pointer picks a section, a click picks the row of its list under it, and the wheel
    /// moves through either.
    #[test]
    fn the_mouse_picks_sections_and_rows() {
        let (mut app, _dir) = profile_app(fixture_tab_with(&[&[1, 2, 3, 4, 5, 6], &[9, 10, 1, 2, 12, 7]]));
        let signature = spot_in(&mut app, "Signature", |a| a.profile.sections_area);
        mouse(&mut app, LEFT, signature);
        assert_eq!(app.profile.section(), Section::Signature);

        // The row showing the third track.
        let Some(Ok(v)) = &app.profile.view else { panic!("no profile") };
        let third = v.report.signature.played[2].name.clone();
        let row = spot_in(&mut app, &third, |a| a.profile.detail_area);
        mouse(&mut app, LEFT, row);
        assert_eq!((app.profile.pane, app.profile.rows.selected()), (Pane::Detail, Some(2)));
        mouse(&mut app, MouseEventKind::ScrollDown, row);
        assert_eq!(app.profile.rows.selected(), Some(5));
        mouse(&mut app, MouseEventKind::ScrollUp, signature);
        let i = Section::ALL.iter().position(|s| *s == Section::Signature).unwrap();
        assert_eq!((app.profile.pane, app.profile.section()), (Pane::Sections, Section::ALL[i - 3]));
        let detail = app.profile.detail_area;
        // The document scrolls with the wheel.
        press(&mut app, "D");
        mouse(&mut app, MouseEventKind::ScrollDown, (detail.x + 4, detail.y + 6));
        assert_eq!(app.profile.scroll, 3);
        // Outside the view, the pointer does what it does anywhere.
        mouse(&mut app, MouseEventKind::ScrollDown, (0, 0));
        assert_eq!(app.profile.scroll, 3);
    }
}
