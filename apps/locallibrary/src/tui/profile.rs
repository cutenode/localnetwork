//! The localnetwork view: the sections of the DJ profile that can be published (what the
//! collection and the sets say), and which of them the document holds. Choices (sections, hidden
//! artists, rounding) are saved to the store as they're made, and the document follows at once;
//! only a rebuild (`R`) reads the drives again.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;

use locallibrary::Store;
use locallibrary::api::{self, ProfileRequest, ProfileView};
use locallibrary::profile::{Basis, Precision, PrefsUpdate, ProfileReport, RankedSection, Section};
use locallibrary::progress::{CancelToken, Ctx, Event};
use ratatui::Frame;
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState, Wrap};

use super::account::Account;
use super::app::{App, Level, Msg};
use super::icons;
use super::keymap::Act;
use super::theme::{self, bold, dim, faint, fg, text};
use super::ui::{Scrolled, block, pane_title, title};
use crate::output::{BLOCKS, bar, opt_pct, pct, plural};

/// The sections the view lists: the ones that can be published ([`Section::publishable`]).
/// What only describes plays (tempo, keys, mixing…) stays out of this view.
pub const SECTIONS: [Section; 6] =
    [Section::Sets, Section::Energy, Section::Genres, Section::Artists, Section::Era, Section::Collection];

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
    /// Read only this drive (and the store's archive of it), not every drive and the store. The
    /// player never sets it; tests do, so they don't read the drives of the machine running them.
    pub only_drive: Option<PathBuf>,
    pub pane: Pane,
    pub sections: ListState,
    pub rows: TableState,
    pub document: bool,
    pub scroll: u16,
    sections_area: Rect,
    detail_area: Rect,
    /// The Atmosphere account, and signing in or out (the bar at the top).
    pub account: Account,
    /// rekordbox's collection as it was when the profile was last built: coming back to the tab
    /// after rekordbox saved builds it again.
    collection_seen: Option<super::app::CollectionStamp>,
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
            only_drive: None,
            pane: Pane::Sections,
            sections: ListState::default().with_selected(Some(0)),
            rows: TableState::default(),
            document: false,
            scroll: 0,
            sections_area: Rect::default(),
            detail_area: Rect::default(),
            account: Account::default(),
            collection_seen: None,
        }
    }

    /// What the status line shows while building.
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
        SECTIONS[self.sections.selected().unwrap_or(0).min(SECTIONS.len() - 1)]
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
            Section::Genres => r.genres.top.len(),
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
        let i = i.min(SECTIONS.len() - 1);
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
                self.select_section((cur + delta).clamp(0, SECTIONS.len() as isize - 1) as usize);
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

/// Stop the build under way: what it sends back is for an older generation.
pub fn stop(app: &mut App) {
    let t = &mut app.profile;
    if let Some(c) = t.cancel.take() {
        c.cancel();
    }
    t.generation += 1;
    t.loading = false;
}

/// The collection this tab's profile reads, when it reads one: rekordbox's on this computer.
fn collection(app: &App) -> Option<&std::path::Path> {
    app.collection_db.as_deref().filter(|_| app.profile.only_drive.is_none())
}

/// Whether rekordbox saved its collection since the profile was built from it.
pub fn collection_changed(app: &App) -> bool {
    let t = &app.profile;
    t.view.is_some()
        && !t.loading
        && collection(app).is_some_and(|db| t.collection_seen != Some(super::app::CollectionStamp::of(db)))
}

/// Rebuild the profile in the background (a running build is cancelled).
pub fn refresh(app: &mut App) {
    let root = app.store_root.clone();
    // Taken before the build: a save while it reads shows as a change, and is read next time.
    let seen = collection(app).map(super::app::CollectionStamp::of);
    let t = &mut app.profile;
    t.collection_seen = seen;
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
    let req = match &t.only_drive {
        None => ProfileRequest::default(),
        Some(path) => ProfileRequest { drives: vec![path.display().to_string()], ..Default::default() },
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
            let aliases_changed = matches!(&app.profile.view, Some(Ok(v)) if v.prefs.genre_aliases != prefs.genre_aliases
                || v.prefs.artists_together != prefs.artists_together
                || v.prefs.artists_apart != prefs.artists_apart);
            app.profile.view = app.profile.view.take().map(|v| v.map(|v| ProfileView::new(v.report, prefs)));
            app.toast(done, Level::Info);
            if aliases_changed {
                refresh(app);
            }
        }
        Err(e) => app.toast(e, Level::Error),
    }
}

/// Keep `credit` together as one artist (`together`), or count it for each of its artists, here
/// and in Browse › Artist, which is grouped again.
pub fn collab(app: &mut App, credit: &str, together: bool) {
    let (update, done) = match together {
        true => {
            (PrefsUpdate { together: vec![credit.into()], ..Default::default() }, format!("{credit} is one artist"))
        }
        false => (
            PrefsUpdate { apart: vec![credit.into()], ..Default::default() },
            format!("{credit} counts for each artist"),
        ),
    };
    change(app, update, done);
    if let Some(l) = app.loaded.as_mut() {
        l.collabs = crate::tui::browse::Collabs::load(&app.store_root);
    }
    app.regroup(crate::tui::browse::Category::Artist);
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

/// A digit: the pane with that number, when there's something in it.
pub fn jump_to_pane(app: &mut App, n: u32) {
    let t = &mut app.profile;
    match n {
        1 if !t.document => t.pane = Pane::Sections,
        2 if !t.document && t.detail_len() > 0 => t.pane = Pane::Detail,
        _ => return,
    }
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
        Act::ProfileHideArtist => hide_artist(app),
        Act::ProfileAccount => super::account::press(app),
        _ => {}
    }
}

/// Hide the selected artist under Artists, or name them again.
fn hide_artist(app: &mut App) {
    let t = &app.profile;
    let Some(Ok(v)) = &t.view else { return };
    let row = t.rows.selected().filter(|_| t.pane == Pane::Detail);
    let artist = match (t.section(), row) {
        (Section::Artists, Some(i)) => v.report.artists.top.get(i),
        _ => None,
    };
    let Some(a) = artist else {
        app.toast("select an artist under Artists (tab) to hide them", Level::Info);
        return;
    };
    let (update, msg) = artist_toggle(&v.prefs, &a.name);
    change(app, update, msg);
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
            if m.row > t.sections_area.y && row < SECTIONS.len() {
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
        Section::Sets => r.sets.count.to_string(),
        Section::Energy => r.energy.as_ref().map(|e| format!("{} high", pct(e.high))).unwrap_or_default(),
        Section::Genres => r.genres.top.first().map(|g| g.name.clone()).unwrap_or_default(),
        Section::Artists => r.artists.distinct.to_string(),
        Section::Era => r.era.median_year.map(|y| y.to_string()).unwrap_or_default(),
        Section::Collection => r.collection.tracks.to_string(),
        _ => String::new(),
    }
}

/// The line at the top: an overview of the collection (and how many sets), counted as the
/// sections count it. It's for the DJ; the document has no summary.
fn overview(r: &ProfileReport) -> String {
    let mut parts = vec![plural(r.collection.tracks, "track")];
    if r.genres.distinct > 0 {
        parts.push(plural(r.genres.distinct, "genre"));
    }
    if r.artists.distinct > 0 {
        parts.push(plural(r.artists.distinct, "artist"));
    }
    match (r.era.oldest, r.era.newest) {
        (Some(a), Some(b)) if a != b => parts.push(format!("released {a}–{b}")),
        (Some(a), _) => parts.push(format!("released {a}")),
        _ => {}
    }
    if r.basis != Basis::Collection && r.sets.count > 0 {
        parts.push(plural(r.sets.count, r.basis.unit()));
    }
    parts.join(" · ")
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
                    vec![
                        Line::raw(""),
                        Line::styled(e.clone(), fg(theme::current().critical)),
                        Line::styled("R tries again", dim()),
                    ]
                }
                _ => vec![
                    Line::raw(""),
                    Line::styled(status.unwrap_or_else(|| "Building profile…".into()), bold(theme::current().text)),
                    Line::styled("Reading your collection, its analysis and your sets", dim()),
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
    let [top, main] = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(area);

    // The overview.
    let mut head = vec![Span::styled(overview(r), bold(theme::current().info))];
    if let Some(s) = status {
        head.push(Span::styled(format!("  {} {s}", icons::BUSY.get()), fg(theme::current().info)));
    }
    let summary = Paragraph::new(Line::from(head)).block(block(title("Profile", false), false));
    f.render_widget(summary, top);

    // Sections.
    let [side, detail] = Layout::horizontal([Constraint::Length(32), Constraint::Min(0)]).areas(main);
    t.sections_area = side;
    t.detail_area = detail;
    let focused = t.pane == Pane::Sections && !t.document;
    let inner_w = side.width.saturating_sub(3) as usize;
    let items: Vec<ListItem> = SECTIONS
        .iter()
        .map(|&s| {
            let on = v.published.contains(&s) && r.basis.publishes(s);
            let mark = if on {
                Span::styled(icons::DOT.spaced(), fg(theme::current().good))
            } else {
                Span::styled(icons::RING.spaced(), faint())
            };
            // The stat can be user text (the top genre): it gets half the column at most.
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
        .block(block(pane_title(1, "Sections", focused), focused))
        .highlight_style(if focused { theme::selected() } else { theme::unfocused() })
        .highlight_symbol(Span::styled(icons::SELECTED.get(), fg(theme::current().accent)));
    t.sections.fit(side.height.saturating_sub(2) as usize, SECTIONS.len());
    f.render_stateful_widget(list, side, &mut t.sections);

    if t.document {
        draw_document(f, v, t.scroll, detail);
        return;
    }
    let section = t.section();
    let on = v.published.contains(&section);
    let state = if on && r.basis.publishes(section) {
        Span::styled(" in the document ", fg(theme::current().good))
    } else {
        Span::styled(" only on this computer ", faint())
    };
    let detail_focused = t.pane == Pane::Detail;
    let b = block(pane_title(2, section.title_in(r.basis), detail_focused), detail_focused)
        .title(Line::from(state).right_aligned());
    let inner = b.inner(detail);
    f.render_widget(b, detail);
    let [reveal, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(inner);
    let reveal_text = match (r.basis.publishes(section), on) {
        (false, _) => "Sessions aren't published. With sets declared, this shows ",
        (true, true) => "Published, this shows ",
        (true, false) => "Published, this would show ",
    };
    let spans = vec![
        Span::styled(format!(" {reveal_text}"), faint()),
        Span::styled(section.reveals().to_string(), dim()),
        Span::styled("   i ", theme::key()),
        Span::styled(if on { "leave out" } else { "include" }, faint()),
    ];
    f.render_widget(Paragraph::new(Line::from(spans)).wrap(Wrap { trim: true }), reveal);
    let body = Rect { x: body.x + 1, width: body.width.saturating_sub(2), ..body };
    let w = body.width as usize;
    let lines: Vec<Line<'static>> = match section {
        Section::Sets => return draw_sets(f, r, &mut t.rows, detail_focused, body),
        Section::Energy => energy_lines(r, w),
        Section::Genres | Section::Artists => {
            return draw_ranked(f, v, section, &mut t.rows, detail_focused, body);
        }
        Section::Era => era_lines(r, w),
        Section::Collection => collection_lines(r),
        // Not in SECTIONS.
        _ => Vec::new(),
    };
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
}

fn draw_sets(f: &mut Frame, r: &ProfileReport, rows: &mut TableState, focused: bool, area: Rect) {
    let s = &r.sets;
    let unit = r.basis.unit();
    let mut head = vec![Line::from(vec![
        Span::styled(plural(s.count, unit), bold(theme::current().text)),
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
                Cell::from(Span::styled(when, fg(theme::current().warning))),
                Cell::from(Span::styled(set.name.clone(), text())),
                Cell::from(Line::styled(set.tracks.to_string(), text()).right_aligned()),
                Cell::from(Span::styled(bpm, fg(theme::current().info))),
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
    .header(Row::new(header.into_iter().map(|h| Cell::from(Span::styled(h, bold(theme::current().dim))))))
    .column_spacing(2)
    .row_highlight_style(if focused { theme::selected() } else { theme::unfocused() })
    .highlight_symbol(Span::styled(
        icons::SELECTED.get(),
        fg(if focused { theme::current().accent } else { theme::current().faint }),
    ));
    // Under the header row.
    rows.fit(table.height.saturating_sub(1) as usize, s.list.len());
    f.render_stateful_widget(t, table, rows);
}

fn energy_lines(r: &ProfileReport, w: usize) -> Vec<Line<'static>> {
    let Some(e) = &r.energy else {
        return vec![note("No phrase analysis read: it needs the drives' analysis files.")];
    };
    let bw = w.saturating_sub(16).clamp(10, 40);
    let mut lines = vec![
        note(format!("rekordbox's phrase-analysis mood, {} tracks in your collection", e.tracks)),
        Line::raw(""),
        bar_line("high", 6, e.high, bw, theme::current().accent, pct(e.high)),
        bar_line("mid", 6, e.mid, bw, theme::current().secondary, pct(e.mid)),
        bar_line("low", 6, e.low, bw, theme::current().info, pct(e.low)),
        Line::raw(""),
    ];
    if e.curve.iter().flatten().count() > 1 {
        let col_w = (w / 10).clamp(3, 8);
        lines.extend(columns(&e.curve, 0.0, 5, col_w, |_| theme::current().accent));
        let mut axis = fit("start", col_w * 5);
        axis.push_str(&format!("{:>width$}", "end", width = col_w * 5 - 1));
        lines.push(Line::styled(axis, dim()));
        lines.push(note(format!("Mood through a {} (high 1, mid ½, low 0)", r.basis.unit())));
    }
    lines
}

fn draw_ranked(f: &mut Frame, v: &ProfileView, s: Section, rows: &mut TableState, focused: bool, area: Rect) {
    let r = &v.report;
    let ranked: &RankedSection = if s == Section::Genres { &r.genres } else { &r.artists };
    let noun = "tracks in your collection";
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
            let color = if s == Section::Genres { theme::current().info } else { theme::current().secondary };
            Row::new(vec![
                Cell::from(Span::styled(x.name.clone(), if hidden { faint() } else { text() })),
                Cell::from(Span::styled(
                    bar(x.share / max, bw),
                    fg(if hidden { theme::current().faint } else { color }),
                )),
                Cell::from(Line::styled(pct(x.share), text()).right_aligned()),
                Cell::from(Span::styled(if hidden { "hidden" } else { "" }, faint())),
            ])
        })
        .collect();
    let t = Table::new(
        data,
        [Constraint::Length(26), Constraint::Length(bw as u16), Constraint::Length(5), Constraint::Length(7)],
    )
    .header(Row::new(
        ["Name", "", "Share", ""].into_iter().map(|h| Cell::from(Span::styled(h, bold(theme::current().dim)))),
    ))
    .column_spacing(1)
    .row_highlight_style(if focused { theme::selected() } else { theme::unfocused() })
    .highlight_symbol(Span::styled(
        icons::SELECTED.get(),
        fg(if focused { theme::current().accent } else { theme::current().faint }),
    ));
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
            Span::styled(format!("median {}", e.median_year.unwrap_or(0)), bold(theme::current().info)),
            Span::styled(
                format!(
                    " ({}–{}) · {} released this year or last, of {} tracks in your collection",
                    e.oldest.unwrap_or(0),
                    e.newest.unwrap_or(0),
                    opt_pct(e.recent_share),
                    e.tracks
                ),
                text(),
            ),
        ]),
        Line::raw(""),
    ];
    let max = e.decades.iter().map(|d| d.count).max().unwrap_or(1).max(1);
    let bw = w.saturating_sub(16).clamp(10, 40);
    for (decade, n) in e.decades.iter().map(|d| (&d.decade, &d.count)) {
        lines.push(bar_line(
            &format!("{decade}s"),
            6,
            *n as f64 / max as f64,
            bw,
            theme::current().warning,
            n.to_string(),
        ));
    }
    lines
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

/// The document as coloured JSON.
fn draw_document(f: &mut Frame, v: &ProfileView, scroll: u16, area: Rect) {
    let json = serde_json::to_string_pretty(&v.document).unwrap_or_default();
    let lines: Vec<Line<'static>> = json.lines().map(json_line).collect();
    let b = block(title("Document", true), true)
        .title(Line::from(Span::styled(" what a public profile would show · D closes ", faint())).right_aligned());
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
        spans.push(Span::styled(format!("\"{k}\""), fg(theme::current().info)));
        spans.push(Span::styled(": ", faint()));
    }
    let style = if value.starts_with('"') {
        fg(theme::current().good)
    } else if value.starts_with(|c: char| c.is_ascii_digit() || c == '-') {
        fg(theme::current().warning)
    } else if value.starts_with("true") || value.starts_with("false") || value.starts_with("null") {
        fg(theme::current().secondary)
    } else {
        faint()
    };
    spans.push(Span::styled(value.to_string(), style));
    Line::from(spans)
}

#[cfg(test)]
mod tests;
