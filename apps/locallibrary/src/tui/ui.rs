//! Drawing.

use locallibrary::api::{DriveStatus, FleetLibrary};
use locallibrary::compare::Relation;
use locallibrary::fleet::{Role, Standing};
use locallibrary::local_time;
use locallibrary::query::{SortField, SortKey};
use locallibrary::{DeviceInfo, Track};
use loclib_waveform::{CueLane, Detail, Overview, PhraseStrip, Pixels, Scope, Spectrum, bar_layout, cue_letter, vinyl};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Flex, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, HighlightSpacing, List, ListItem, ListState, Paragraph, Row, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Table, TableState, Wrap,
};
use uuid::Uuid;

use super::analyzer::Bands;
use super::app::{App, Focus, Level, Matches, Repeat, View, Vis, typed_tail};
use super::browse::{COLOURS, NodeId, colour_name, sort_label, stars};
use super::drives::DriveRow;
use super::edits::{Menu, Modal, Sub};
use super::keys::{Camelot, Relation as KeyRelation};
use super::theme::{self, bold, dim, faint, fg, text};
use crate::output::{bytes, clock, duration, plural};

pub fn draw(f: &mut Frame, app: &mut App) {
    let [header, body, footer] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(f.area());
    draw_header(f, app, header);
    // Panes register where they are as they're drawn; hidden ones mustn't catch clicks.
    let a = &mut app.areas;
    a.spectra.clear();
    a.columns.clear();
    (a.drives, a.playlists, a.tracks, a.details, a.matches, a.crate_panel, a.crate_rows, a.menu, a.overview) =
        Default::default();
    match app.view {
        View::Library if app.fullscreen => draw_player(f, app, body, true),
        View::Library => {
            let player_h = if body.height >= 30 {
                14
            } else if body.height >= 22 {
                12
            } else {
                9
            };
            let [main, player] = Layout::vertical([Constraint::Min(0), Constraint::Length(player_h)]).areas(body);
            draw_library(f, app, main);
            draw_player(f, app, player, false);
        }
        View::Drives => draw_drives(f, app, body),
        View::Snapshots => draw_history(f, app, body),
        View::Profile => super::profile::draw(f, app, body),
        View::Mixes => super::mix::draw(f, app, body),
    }
    draw_footer(f, app, footer);
    match &app.modal {
        Some(Modal::Menu(_)) => draw_menu(f, app, f.area()),
        Some(Modal::Confirm(_)) => draw_confirm(f, app, f.area()),
        Some(Modal::Ask(q, _)) => draw_ask(f, q.clone(), f.area()),
        Some(Modal::Pick(_)) => draw_picker(f, app, f.area()),
        Some(Modal::Report(_)) => draw_report(f, app, f.area()),
        Some(Modal::Form(_)) => draw_form(f, app, f.area()),
        Some(Modal::Prompt(_)) | None => {}
    }
    if app.deck.outputs.is_some() {
        draw_outputs(f, app, f.area());
    }
    if app.help {
        draw_help(f, app, f.area());
    } else if app.show_log {
        draw_log(f, app, f.area());
    } else {
        draw_toasts(f, app, f.area(), footer);
    }
}

fn block(title: impl Into<Line<'static>>, focused: bool) -> Block<'static> {
    Block::bordered().border_type(theme::border_type(focused)).border_style(theme::border(focused)).title(title.into())
}

fn title(s: &str, focused: bool) -> Line<'static> {
    let style = if focused { bold(theme::CYAN) } else { bold(theme::DIM) };
    Line::from(vec![Span::raw(" "), Span::styled(s.to_string(), style), Span::raw(" ")])
}

/// `left` and `right` on one line, `right` flush right when it fits.
fn spread<'a>(mut left: Vec<Span<'a>>, right: Vec<Span<'a>>, width: u16) -> Line<'a> {
    let lw: usize = left.iter().map(Span::width).sum();
    let rw: usize = right.iter().map(Span::width).sum();
    if lw + rw < width as usize {
        left.push(Span::raw(" ".repeat(width as usize - lw - rw)));
        left.extend(right);
    }
    Line::from(left)
}

/// At most `max` columns of `s`, measured as ratatui measures a span (CJK and emoji take two),
/// with an ellipsis when cut.
pub(super) fn clip(s: &str, max: usize) -> String {
    crate::output::clip(s, max, "…")
}

/// Keep spans from the front while they fit in `max` columns, cutting the last one short.
fn fit(spans: Vec<Span<'static>>, max: usize) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut used = 0;
    for s in spans {
        let w = s.width();
        if used + w <= max {
            used += w;
            out.push(s);
        } else {
            if max > used + 1 {
                out.push(Span::styled(clip(&s.content, max - used), s.style));
            }
            break;
        }
    }
    out
}

fn drive_name(d: &DeviceInfo) -> String {
    d.name.clone().unwrap_or_else(|| d.volume_name.clone())
}

fn compact(n: usize) -> String {
    if n >= 10_000 {
        format!("{}k", n / 1000)
    } else if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

fn spinner(app: &App) -> &'static str {
    let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    frames[(app.started.elapsed().as_secs_f32() * 12.0) as usize % frames.len()]
}

// ---------------------------------------------------------------------------------------------
// Header and footer
// ---------------------------------------------------------------------------------------------

fn draw_header(f: &mut Frame, app: &mut App, area: Rect) {
    // The logo dot pulses with the music.
    let pulse = theme::mix(theme::PINK, theme::AMBER, app.analyzer.level);
    let pulsing = app.is_playing() && !theme::reduced_motion();
    let mut spans = vec![
        Span::styled(" ◉ ", bold(if pulsing { pulse } else { theme::PINK })),
        Span::styled("locallibrary", bold(theme::FG)),
        Span::raw("   "),
    ];
    app.areas.tabs.clear();
    let mut x = area.x + spans.iter().map(|s| s.width() as u16).sum::<u16>();
    for (i, v) in View::ALL.into_iter().enumerate() {
        let label = format!(" {} {} ", i + 1, v.title());
        let w = label.chars().count() as u16;
        let style = if app.view == v { theme::on(theme::CYAN, [16, 16, 22]) } else { dim() };
        app.areas.tabs.push((v, Rect::new(x, area.y, w, 1)));
        spans.push(Span::styled(label, style));
        spans.push(Span::raw(" "));
        x += w + 1;
    }
    let room = (area.width as usize).saturating_sub(spans.iter().map(Span::width).sum::<usize>() + 1);
    // A job's progress comes first and is cut short if need be; the rest shows when it fits.
    let mut busy = Vec::new();
    if let Some(j) = &app.job {
        busy.push(Span::styled(format!("{} ", spinner(app)), fg(theme::AMBER)));
        busy.push(Span::styled(format!("{} ", j.label), bold(theme::AMBER)));
        let secs = j.started.elapsed().as_secs();
        if secs > 0 {
            busy.push(Span::styled(format!("{secs}s "), faint()));
        }
        if !j.detail.is_empty() {
            busy.push(Span::styled(format!("{} ", j.detail), dim()));
        }
        if !j.committed {
            busy.push(Span::styled(" esc ", theme::key()));
            busy.push(Span::styled("cancel ", dim()));
        }
        busy.push(Span::raw(" "));
    } else if app.loading.is_some() || app.status_loading || app.profile.loading || app.mix.is_loading() {
        busy.push(Span::styled(format!("{} ", spinner(app)), fg(theme::CYAN)));
        if let Some(m) = &app.loading {
            busy.push(Span::styled(format!("{m}  "), dim()));
        } else if let Some(m) = app.profile.status() {
            busy.push(Span::styled(format!("{m}  "), dim()));
        } else if let Some(m) = app.mix.status() {
            busy.push(Span::styled(format!("{m}  "), dim()));
        }
    }
    let mut right = fit(busy, room);
    let n = app.drives.len();
    let rest = vec![
        Span::styled(format!("{} ", plural(n, "drive")), dim()),
        Span::styled("? ", theme::key()),
        Span::styled("help ", dim()),
    ];
    let used: usize = right.iter().map(Span::width).sum();
    if used + rest.iter().map(Span::width).sum::<usize>() <= room {
        right.extend(rest);
    }
    f.render_widget(Paragraph::new(spread(spans, right, area.width)), area);
}

/// A line being typed: the search box, or a prompt for a name.
fn input_line(label: Vec<Span<'static>>, text_: &str, after: Vec<Span<'static>>, width: u16) -> Line<'static> {
    // A label naming a long playlist mustn't push what's typed off the line.
    let mut spans = fit(label, width as usize * 2 / 3);
    let room = (width as usize).saturating_sub(spans.iter().map(Span::width).sum::<usize>() + 1);
    spans.push(Span::styled(typed_tail(text_, room).to_string(), bold(theme::FG)));
    spans.push(Span::styled("▏", fg(theme::PINK)));
    let used: usize = spans.iter().map(Span::width).sum();
    spans.extend(fit(after, (width as usize).saturating_sub(used)));
    Line::from(spans)
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    if let Some(Modal::Prompt(p)) = &app.modal {
        let label = vec![Span::styled(format!(" {}: ", p.label), theme::key())];
        let after = vec![Span::styled("   enter ok · esc cancel", faint())];
        f.render_widget(Paragraph::new(input_line(label, &p.text, after, area.width)), area);
        return;
    }
    if app.searching {
        let n = app.visible.len();
        let mut after = vec![
            Span::styled(format!("   {n} match{}", if n == 1 { "" } else { "es" }), dim()),
            Span::styled(if app.search_all { "  in the whole drive" } else { "  in this list" }, fg(theme::CYAN)),
        ];
        match (&app.filter_error, &app.filter_note) {
            (Some(e), _) => after.push(Span::styled(format!("   {e}"), fg(theme::AMBER))),
            (None, Some(n)) => after.push(Span::styled(format!("   {n}"), dim())),
            (None, None) => after.push(Span::styled("   tab whole drive · enter keep · esc clear", faint())),
        }
        let label = vec![Span::styled(" / ", theme::key())];
        f.render_widget(Paragraph::new(input_line(label, &app.query, after, area.width)), area);
        return;
    }
    if let Some(p) = &app.picking {
        let spans = vec![
            Span::styled(" ◆ ", bold(theme::AMBER)),
            Span::styled(format!("Add {} to which playlist? ", p.target.what), bold(theme::FG)),
            Span::styled("  enter choose · esc cancel", faint()),
        ];
        f.render_widget(Paragraph::new(Line::from(fit(spans, area.width as usize))), area);
        return;
    }
    // The keymap's hints for where focus is, as many as fit.
    let mut spans = vec![Span::raw(" ")];
    for (k, label) in super::keymap::hints(app, area.width as usize) {
        spans.push(Span::styled(k, theme::key()));
        spans.push(Span::styled(format!(" {label}  "), dim()));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// `s` in lines of at most `width` columns, broken between words, and inside words too long
/// for a line (paths, say).
fn wrap_text(s: &str, width: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthStr;
    let width = width.max(1);
    let mut out = vec![String::new()];
    for word in s.split(' ') {
        let line = out.last_mut().unwrap();
        if !line.is_empty() && line.width() + 1 + word.width() <= width {
            line.push(' ');
            line.push_str(word);
            continue;
        }
        if !line.is_empty() {
            out.push(String::new());
        }
        for c in word.chars() {
            let line = out.last_mut().unwrap();
            if !line.is_empty() && line.width() + unicode_width::UnicodeWidthChar::width(c).unwrap_or(0) > width {
                out.push(String::new());
            }
            out.last_mut().unwrap().push(c);
        }
    }
    out
}

fn level_style(level: Level) -> (&'static str, [u8; 3]) {
    match level {
        Level::Info => ("●", theme::CYAN),
        Level::Error => ("✖", theme::RED),
    }
}

/// A message as lines `width` wide: its icon, then the text wrapped under itself.
fn message_lines(msg: &str, level: Level, width: u16) -> Vec<Line<'static>> {
    let (icon, c) = level_style(level);
    wrap_text(msg, (width as usize).saturating_sub(4))
        .into_iter()
        .enumerate()
        .map(|(i, part)| {
            let head = if i == 0 { Span::styled(format!(" {icon} "), bold(c)) } else { Span::raw("   ") };
            Line::from(vec![head, Span::styled(part, fg(c))])
        })
        .collect()
}

/// The messages: an error that's waiting to be dismissed above the latest message, wrapped to
/// the width. They end on the footer's row, or just above it while it's in use (a prompt, the
/// search box, picking a playlist). With a dialog open, only the latest shows, on the footer's
/// row, so the dialog isn't covered.
fn draw_toasts(f: &mut Frame, app: &App, area: Rect, footer: Rect) {
    let dialog = matches!(
        app.modal,
        Some(Modal::Menu(_) | Modal::Confirm(_) | Modal::Ask(..) | Modal::Pick(_) | Modal::Report(_) | Modal::Form(_))
    );
    let footer_busy = matches!(app.modal, Some(Modal::Prompt(_))) || app.searching || app.picking.is_some();
    let mut lines: Vec<Line> = Vec::new();
    if let Some(o) = app.replay_offer.as_ref().filter(|_| !dialog) {
        let edits = if o.edits == 1 { "1 edit".to_string() } else { format!("{} edits", o.edits) };
        let spans = vec![
            Span::styled(" ◆ ", bold(theme::AMBER)),
            Span::styled(format!("rekordbox exported over {edits} on {}. Replay? ", o.drive), fg(theme::AMBER)),
            Span::styled("y", theme::key()),
            Span::styled(" replay · ", dim()),
            Span::styled("esc", theme::key()),
            Span::styled(" not now", dim()),
        ];
        lines.push(Line::from(fit(spans, footer.width as usize)));
    }
    if let Some(m) = app.held.as_ref().filter(|_| !dialog) {
        lines.extend(message_lines(m, Level::Error, footer.width));
    }
    if let Some((m, level, _)) = &app.toast {
        if dialog {
            let (icon, c) = level_style(*level);
            let spans = vec![Span::styled(format!(" {icon} "), bold(c)), Span::styled(m.clone(), fg(c))];
            lines.push(Line::from(fit(spans, footer.width as usize)));
        } else {
            lines.extend(message_lines(m, *level, footer.width));
        }
    }
    if lines.is_empty() {
        return;
    }
    // Errors stay until esc: say so where there's room.
    let error = app.held.is_some() || app.toast.as_ref().is_some_and(|t| t.1 == Level::Error);
    if let Some(last) = lines.last_mut().filter(|l| error && l.width() + 13 <= footer.width as usize) {
        last.spans.push(Span::styled("  esc hides", faint()));
    }
    // The offer to replay edits keeps the footer's hints in view: it says which key.
    let above = footer_busy || app.replay_offer.is_some();
    let bottom = if above && !dialog { footer.y } else { footer.bottom() };
    // At most a third of the screen; the newest lines when that's not enough.
    let room = (bottom.saturating_sub(area.y + 1)).min((area.height / 3).max(1)) as usize;
    let skip = lines.len().saturating_sub(room);
    let lines: Vec<Line> = lines.into_iter().skip(skip).collect();
    let h = lines.len() as u16;
    let r = Rect { x: footer.x, y: bottom.saturating_sub(h), width: footer.width, height: h };
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines), r);
}

/// How long ago, roughly.
fn ago(d: std::time::Duration) -> String {
    match d.as_secs() {
        s if s < 5 => "just now".into(),
        s if s < 60 => format!("{s} s ago"),
        s if s < 3600 => format!("{} min ago", s / 60),
        s => format!("{} h ago", s / 3600),
    }
}

/// `!`: every message since starting, newest first.
fn draw_log(f: &mut Frame, app: &mut App, area: Rect) {
    let w = 100.min(area.width);
    let inner_w = w.saturating_sub(4);
    let mut lines: Vec<Line> = Vec::new();
    for (msg, level, at) in app.log.iter().rev() {
        let mut msg_lines = message_lines(msg, *level, inner_w.saturating_sub(12));
        if let Some(first) = msg_lines.first_mut() {
            let spans = std::mem::take(&mut first.spans);
            *first = spread(spans, vec![Span::styled(ago(at.elapsed()), faint())], inner_w);
        }
        lines.extend(msg_lines);
    }
    if lines.is_empty() {
        lines.push(Line::styled("Nothing to report yet.", dim()));
    }
    let total = lines.len() as u16;
    let r = popup(area, w, total + 2, None);
    let b = block(title("Messages", true), true)
        .title_bottom(Line::styled(" ↑↓ scroll · any other key closes ", faint()).right_aligned())
        .padding(ratatui::widgets::Padding::horizontal(1));
    let inner = b.inner(r);
    app.log_scroll = app.log_scroll.min(total.saturating_sub(inner.height));
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines).scroll((app.log_scroll, 0)).block(b), r);
}

// ---------------------------------------------------------------------------------------------
// Library
// ---------------------------------------------------------------------------------------------

/// The sidebar (drives and the tree), the track list, and the details and crate panels when
/// they're open: beside the list on wide terminals, in place of the sidebar on narrow ones
/// (the sidebar comes back while it has focus).
fn draw_library(f: &mut Frame, app: &mut App, area: Rect) {
    let side_on = app.show_details || app.show_crate;
    let in_sidebar = matches!(app.focus, Focus::Drives | Focus::Playlists) || app.picking.is_some();
    let sidebar_w = (area.width / 4).clamp(24, 38);
    let (sidebar, tracks, side) = if side_on && area.width >= 120 {
        let [a, b, c] = Layout::horizontal([
            Constraint::Length((area.width / 5).clamp(24, 34)),
            Constraint::Min(0),
            Constraint::Length((area.width / 4).clamp(34, 50)),
        ])
        .areas(area);
        (Some(a), b, Some(c))
    } else if side_on && !in_sidebar {
        let side_w = (area.width * 2 / 5).clamp(30, 44).min(area.width / 2);
        let [b, c] = Layout::horizontal([Constraint::Min(0), Constraint::Length(side_w)]).areas(area);
        (None, b, Some(c))
    } else {
        let [a, b] = Layout::horizontal([Constraint::Length(sidebar_w), Constraint::Min(0)]).areas(area);
        (Some(a), b, None)
    };
    if let Some(side) = sidebar {
        let drives_h = (app.drives.len() as u16 + 2).clamp(3, (side.height / 3).max(3));
        let [drives, playlists] = Layout::vertical([Constraint::Length(drives_h), Constraint::Min(0)]).areas(side);
        app.areas.drives = drives;
        app.areas.playlists = playlists;
        draw_drive_list(f, app, drives, app.focus == Focus::Drives && app.view == View::Library);
        draw_playlists(f, app, playlists);
    }
    app.areas.tracks = tracks;
    draw_tracks(f, app, tracks);
    if let Some(side) = side {
        match (app.show_details, app.show_crate) {
            (true, true) => {
                let [d, t] = Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(side);
                draw_track_details(f, app, d);
                draw_crate(f, app, t);
            }
            (true, false) => draw_track_details(f, app, side),
            _ => draw_crate(f, app, side),
        }
    }
}

fn draw_drive_list(f: &mut Frame, app: &mut App, area: Rect, focused: bool) {
    let loaded_root = app.loaded.as_ref().map(|l| l.root().to_path_buf());
    let inner_w = area.width.saturating_sub(3);
    let items: Vec<ListItem> = (app.drives.iter())
        .map(|d| ListItem::new(drive_line(d, loaded_root.as_deref() == Some(d.root.as_path()), "", inner_w)))
        .collect();
    let list = drive_list(items, focused);
    app.drive_state.fit(area.height.saturating_sub(2) as usize, app.drives.len());
    f.render_stateful_widget(list, area, &mut app.drive_state);
}

/// A drive in a list: a dot (pink: open, green: has an export, hollow: blank), its name and its
/// track count, after `indent`.
fn drive_line(d: &DeviceInfo, open: bool, indent: &str, width: u16) -> Line<'static> {
    let dot = if !d.has_export {
        Span::styled("○ ", faint())
    } else if open {
        Span::styled("● ", bold(theme::PINK))
    } else {
        Span::styled("● ", fg(theme::GREEN))
    };
    let name_style = if open { bold(theme::FG) } else { text() };
    let left = vec![Span::raw(indent.to_string()), dot, Span::styled(drive_name(d), name_style)];
    let right = match &d.summary {
        Some(s) => vec![Span::styled(compact(s.tracks), dim())],
        None => vec![Span::styled("blank", faint())],
    };
    spread(left, right, width)
}

/// The drive lists' frame, and what they say with no drive.
fn drive_list(items: Vec<ListItem<'static>>, focused: bool) -> List<'static> {
    let list = if items.is_empty() {
        List::new([ListItem::new(Line::styled("no rekordbox drives", dim()))])
    } else {
        List::new(items)
    };
    list.block(block(title("Drives", focused), focused))
        .highlight_style(if focused { theme::selected() } else { Style::new() })
        .highlight_symbol(Span::styled("▍", fg(theme::PINK)))
}

/// Keep `sel` in a window of `page` rows starting at `offset`; returns the new offset.
pub(super) fn scroll_window(offset: usize, sel: Option<usize>, page: usize, len: usize) -> usize {
    let mut offset = offset;
    if let Some(s) = sel {
        if s < offset {
            offset = s;
        } else if s >= offset + page {
            offset = s + 1 - page.max(1);
        }
    }
    offset.min(len.saturating_sub(page))
}

/// A list or table kept to [`scroll_window`] before it's drawn: drawn smaller, then larger, it
/// comes back up once its rows fit again (ratatui's own scrolling only ever follows the
/// selection, so rows stayed hidden above empty space).
pub(super) trait Scrolled {
    /// Scroll for a window of `page` rows of `len`.
    fn fit(&mut self, page: usize, len: usize);
}

impl Scrolled for ListState {
    fn fit(&mut self, page: usize, len: usize) {
        *self.offset_mut() = scroll_window(self.offset(), self.selected(), page, len);
    }
}

impl Scrolled for TableState {
    fn fit(&mut self, page: usize, len: usize) {
        *self.offset_mut() = scroll_window(self.offset(), self.selected(), page, len);
    }
}

fn draw_playlists(f: &mut Frame, app: &mut App, area: Rect) {
    let picking = app.picking.is_some();
    let focused = app.focus == Focus::Playlists;
    let inner_w = area.width.saturating_sub(3);
    let heading = match &app.loaded {
        _ if picking => {
            Line::from(vec![Span::raw(" "), Span::styled("Pick a playlist", bold(theme::AMBER)), Span::raw(" ")])
        }
        Some(l) if l.stale => Line::from(vec![
            Span::raw(" "),
            Span::styled("Playlists", bold(theme::DIM)),
            Span::styled(" unplugged ", bold(theme::RED)),
        ]),
        _ => title("Playlists", focused),
    };
    let mut b = block(heading, focused || picking);
    if picking {
        b = b.border_style(fg(theme::AMBER));
    }
    let Some(l) = &app.loaded else {
        f.render_widget(Paragraph::new(Line::styled("no drive open", dim())).block(b), area);
        return;
    };
    // Only the rows on screen: an open category can hold thousands of values.
    let page = area.height.saturating_sub(2) as usize;
    let len = l.tree.len();
    let sel = app.playlist_state.selected();
    let offset = scroll_window(app.playlist_state.offset(), sel, page, len);
    *app.playlist_state.offset_mut() = offset;
    let items: Vec<ListItem> = l.tree[offset..(offset + page).min(len)]
        .iter()
        .map(|n| {
            let indent = "  ".repeat(n.depth);
            let virtual_folder = matches!(n.id, NodeId::Browse | NodeId::Category(_));
            let (icon, icon_style) = match (&n.id, n.folder, n.expanded) {
                (NodeId::All, _, _) => ("♫ ", fg(theme::PINK)),
                (_, true, true) => ("▾ ", fg(if virtual_folder { theme::VIOLET } else { theme::CYAN })),
                (_, true, false) => ("▸ ", fg(if virtual_folder { theme::VIOLET } else { theme::CYAN })),
                (NodeId::Group(..), false, _) => ("· ", faint()),
                _ => ("• ", faint()),
            };
            let current = !n.folder && n.id == l.node;
            let marked = match &n.id {
                NodeId::Playlist(id) => !app.list_marks.is_empty() && app.list_marks.contains(&l.playlist_path(*id)),
                _ => false,
            };
            let name_style = if current {
                bold(theme::PINK)
            } else if virtual_folder {
                bold(theme::VIOLET)
            } else if n.folder {
                bold(theme::FG)
            } else {
                text()
            };
            let (icon, icon_style) = if marked { ("✓ ", bold(theme::PINK)) } else { (icon, icon_style) };
            let left =
                vec![Span::raw(indent), Span::styled(icon, icon_style), Span::styled(n.name.clone(), name_style)];
            let right = vec![Span::styled(compact(n.count), faint())];
            ListItem::new(spread(left, right, inner_w))
        })
        .collect();
    let list = List::new(items)
        .block(b)
        .highlight_style(if focused { theme::selected() } else { Style::new() })
        .highlight_symbol(Span::styled("▍", fg(theme::PINK)));
    let mut window = ListState::default().with_selected(sel.map(|s| s.saturating_sub(offset)));
    f.render_stateful_widget(list, area, &mut window);
}

fn rating(r: u8) -> String {
    stars(r)
}

/// A column of the track list.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Col {
    Title,
    Artist,
    Album,
    Genre,
    /// My Tags (not a sort field).
    Tags,
    Rating,
    Added,
    Plays,
    Bpm,
    Key,
    Time,
}

impl Col {
    fn field(self) -> Option<SortField> {
        Some(match self {
            Col::Title => SortField::Title,
            Col::Artist => SortField::Artist,
            Col::Album => SortField::Album,
            Col::Genre => SortField::Genre,
            Col::Tags => return None,
            Col::Rating => SortField::Rating,
            Col::Added => SortField::Added,
            Col::Plays => SortField::Plays,
            Col::Bpm => SortField::Bpm,
            Col::Key => SortField::Key,
            Col::Time => SortField::Time,
        })
    }

    fn name(self) -> &'static str {
        self.field().map_or("My Tags", sort_label)
    }

    fn of(f: SortField) -> Option<Col> {
        [
            Col::Title,
            Col::Artist,
            Col::Album,
            Col::Genre,
            Col::Rating,
            Col::Added,
            Col::Plays,
            Col::Bpm,
            Col::Key,
            Col::Time,
        ]
        .into_iter()
        .find(|c| c.field() == Some(f))
    }

    fn width(self) -> Constraint {
        match self {
            Col::Title => Constraint::Fill(3),
            Col::Artist | Col::Album => Constraint::Fill(2),
            Col::Genre => Constraint::Fill(1),
            Col::Tags => Constraint::Fill(2),
            Col::Rating => Constraint::Length(7),
            Col::Added => Constraint::Length(10),
            Col::Plays | Col::Bpm => Constraint::Length(6),
            Col::Key => Constraint::Length(4),
            Col::Time => Constraint::Length(6),
        }
    }
}

/// The columns that fit (ratings only when asked for), plus the sort column when it wouldn't
/// show otherwise.
fn columns(width: u16, sort: Option<SortKey>, ratings: bool, tags: bool) -> Vec<Col> {
    let mut v = vec![Col::Title, Col::Artist];
    if width >= 110 {
        v.push(Col::Album);
    }
    if width >= 140 {
        v.push(Col::Genre);
    }
    if tags {
        v.push(Col::Tags);
    }
    if ratings {
        v.push(Col::Rating);
    }
    if let Some(c) =
        sort.and_then(|k| Col::of(k.field)).filter(|c| !v.contains(c) && !matches!(c, Col::Bpm | Col::Key | Col::Time))
    {
        v.push(c);
    }
    v.extend([Col::Bpm, Col::Key, Col::Time]);
    v
}

/// The key the track list's keys are matched against, and whose file it is: the playing track's,
/// or with nothing playing (or no key to go on), the selected track's.
fn mix_from(app: &App) -> Option<(Camelot, String)> {
    if let Some(n) = &app.now
        && let Some(k) = n.key
    {
        return Some((k, n.track.file_path.clone()));
    }
    let l = app.loaded.as_ref()?;
    let t = &l.library.tracks[*app.visible.get(app.track_state.selected()?)?];
    Some((Camelot::parse(&t.key)?, t.file_path.clone()))
}

/// Whether `t` mixes with `from`'s key on the Camelot wheel (same key, ±1 or relative), other
/// than `from`'s own track.
fn mixes_with(from: &Option<(Camelot, String)>, t: &Track) -> bool {
    from.as_ref().is_some_and(|(key, path)| {
        *path != t.file_path && Camelot::parse(&t.key).is_some_and(|k| key.relation(k).is_some())
    })
}

fn draw_tracks(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Tracks;
    let (heading, count) = match &app.loaded {
        Some(l) => (l.playlist_name(), app.visible.len()),
        None => ("Tracks".to_string(), 0),
    };
    let left_out = app.loaded.as_ref().map_or(0, |l| l.left_out());
    let mut left = vec![
        Span::raw(" "),
        Span::styled(heading, if focused { bold(theme::CYAN) } else { bold(theme::DIM) }),
        Span::styled(format!(" {count} "), faint()),
    ];
    if left_out > 0 {
        left.push(Span::styled(format!("· {left_out} not on this drive "), dim()));
    }
    if !app.marks.is_empty() {
        left.push(Span::styled(format!("✓{} ", app.marks.len()), bold(theme::PINK)));
    }
    if app.loaded.as_ref().is_some_and(|l| l.stale) {
        left.push(Span::styled("unplugged ", bold(theme::RED)));
    }
    let mut b = block(Line::from(left), focused);
    let mix = mix_from(app);
    // The key that green keys mix with, the search and the sort, on the right of the title.
    let mut right = Vec::new();
    if let Some((k, _)) = &mix
        && !app.visible.is_empty()
    {
        right.push(Span::styled(format!(" ⇄ {} ", k.code()), fg(theme::GREEN)));
    }
    if !app.query.is_empty() {
        right.push(Span::styled(" / ", theme::key()));
        right.push(Span::styled(clip(&app.query, 28), text()));
        if app.search_all {
            right.push(Span::styled(" whole drive", fg(theme::CYAN)));
        }
        right.push(Span::raw(" "));
    }
    if let Some(k) = app.sort {
        right.push(Span::styled(
            format!(" {} {} ", sort_label(k.field), if k.descending { "↓" } else { "↑" }),
            fg(theme::AMBER),
        ));
    }
    if !right.is_empty() {
        b = b.title_top(Line::from(right).right_aligned());
    }
    let Some(l) = &app.loaded else {
        let msg = if app.loading.is_some() {
            vec![Line::styled("Reading library…", dim())]
        } else {
            vec![
                Line::raw(""),
                Line::styled("No rekordbox drive open.", bold(theme::FG)),
                Line::raw(""),
                Line::styled("Plug in a USB drive with a rekordbox export, or start with", dim()),
                Line::styled("locallibrary tui /path/to/drive", fg(theme::CYAN)),
            ]
        };
        f.render_widget(Paragraph::new(msg).alignment(Alignment::Center).block(b), area);
        return;
    };

    let cols = columns(area.width, app.sort, app.show_ratings, app.show_tags_column);
    let my_tags = match &l.my_tags {
        super::app::MyTags::Read(t) => Some(t.clone()),
        _ => None,
    };
    let playing_path = app.now.as_ref().filter(|n| n.root == l.root()).map(|n| n.track.file_path.as_str());
    let t = app.started.elapsed().as_secs_f32();
    let eq = ["▁", "▃", "▅", "▇", "▅", "▃"];
    let paused = !app.is_playing();

    // Three narrow columns first: playing or marked, colour, problems.
    let mut widths = vec![Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)];
    widths.extend(cols.iter().map(|c| c.width()));
    let header: Vec<Cell> = ["", "", ""]
        .into_iter()
        .map(Cell::from)
        .chain(cols.iter().map(|c| {
            let name = c.name();
            match app.sort.filter(|k| Some(k.field) == c.field()) {
                Some(k) => Cell::from(Span::styled(
                    format!("{name}{}", if k.descending { "↓" } else { "↑" }),
                    bold(theme::AMBER),
                )),
                None => Cell::from(Span::styled(name, bold(theme::DIM))),
            }
        }))
        .collect();

    // Where the headers are, for sorting by clicking one (as the table lays them out: a column
    // for the highlight symbol, then the columns one space apart). Under 3 rows the pane has no
    // room for them: the row under its top border is its bottom border, or another pane's.
    let height = area.height.saturating_sub(2).min(1);
    let inner = Rect { x: area.x + 1, y: area.y + 1, width: area.width.saturating_sub(2), height };
    let cells = Rect { x: inner.x + 1, width: inner.width.saturating_sub(1), ..inner };
    let rects = Layout::horizontal(widths.clone()).flex(Flex::Start).spacing(1).split(cells);
    app.areas.columns = cols.iter().zip(rects.iter().skip(3)).filter_map(|(c, r)| Some((c.field()?, *r))).collect();

    // Build only the rows on screen: libraries run to tens of thousands of tracks, redrawn at 60 fps.
    let page = area.height.saturating_sub(3) as usize;
    let len = app.visible.len();
    let sel = app.track_state.selected();
    let offset = scroll_window(app.track_state.offset(), sel, page, len);
    *app.track_state.offset_mut() = offset;
    let window = &app.visible[offset..(offset + page).min(len)];
    let mix_bg = theme::behind([30, 66, 42]);
    // Without colours, keys that mix are marked and underlined instead of green.
    let (mix_mark, mix_style) = if theme::plain() {
        ("⇄", bold(theme::GREEN).add_modifier(Modifier::UNDERLINED))
    } else {
        (" ", bold(theme::GREEN))
    };

    let rows: Vec<Row> = window
        .iter()
        .map(|&i| {
            let tr: &Track = &l.library.tracks[i];
            let is_playing = playing_path == Some(tr.file_path.as_str());
            let marked = app.marks.contains(&tr.file_path);
            let indicator = if is_playing {
                let sym = match (paused, theme::reduced_motion()) {
                    (true, _) => "‖",
                    (false, true) => "▶",
                    (false, false) => eq[(t * 8.0) as usize % eq.len()],
                };
                Span::styled(sym, bold(theme::PINK))
            } else if marked {
                Span::styled("✓", bold(theme::PINK))
            } else {
                Span::raw(" ")
            };
            let color = tr.color.map_or(Span::raw(" "), theme::colour_dot);
            let problem = if l.problems.is_missing(i) {
                Span::styled("✖", bold(theme::RED))
            } else if l.problems.has_copies(i) {
                Span::styled("≈", fg(theme::AMBER))
            } else {
                Span::raw(" ")
            };
            let title_style = if is_playing { bold(theme::PINK) } else { text() };
            let mut cells = vec![Cell::from(indicator), Cell::from(color), Cell::from(problem)];
            for c in &cols {
                cells.push(match c {
                    Col::Title => Cell::from(Span::styled(tr.title.clone(), title_style)),
                    Col::Artist => {
                        Cell::from(Span::styled(tr.artist.clone(), fg(theme::mix(theme::FG, theme::DIM, 0.4))))
                    }
                    Col::Album => Cell::from(Span::styled(tr.album.clone(), dim())),
                    Col::Genre => Cell::from(Span::styled(tr.genre.clone(), dim())),
                    Col::Tags => {
                        let key = locallibrary::paths::key(&tr.file_path).to_lowercase();
                        let tags = my_tags.as_ref().and_then(|t| t.get(&key));
                        let names: Vec<&str> =
                            tags.into_iter().flatten().map(|t| t.rsplit('/').next().unwrap_or(t)).collect();
                        Cell::from(Span::styled(names.join(", "), fg(theme::VIOLET)))
                    }
                    Col::Rating => Cell::from(Span::styled(rating(tr.rating), fg(theme::AMBER))),
                    Col::Added => Cell::from(Span::styled(tr.date_added.chars().take(10).collect::<String>(), dim())),
                    Col::Plays => Cell::from(Line::styled(tr.play_count.to_string(), dim()).right_aligned()),
                    Col::Bpm => {
                        let bpm = if tr.bpm > 0.0 { format!("{:>6.1}", tr.bpm) } else { String::new() };
                        Cell::from(Line::styled(bpm, fg(theme::AMBER)).right_aligned())
                    }
                    Col::Key if mixes_with(&mix, tr) => {
                        Cell::from(Span::styled(format!("{mix_mark}{}", tr.key), mix_style)).style(mix_bg)
                    }
                    Col::Key => Cell::from(Span::styled(format!(" {}", tr.key), fg(theme::CYAN))),
                    Col::Time => Cell::from(Line::styled(clock(tr.duration_secs as f64), dim()).right_aligned()),
                });
            }
            let row = Row::new(cells);
            if marked { row.style(theme::behind([40, 26, 46])) } else { row }
        })
        .collect();

    let table = Table::new(rows, widths)
        .header(Row::new(header))
        .block(b)
        .column_spacing(1)
        .highlight_spacing(HighlightSpacing::Always)
        .row_highlight_style(if focused { theme::selected() } else { theme::behind([32, 32, 46]) })
        .highlight_symbol(Span::styled("▍", fg(theme::PINK)));
    let mut window_state = TableState::default().with_selected(sel.map(|s| s - offset));
    f.render_stateful_widget(table, area, &mut window_state);

    if app.visible.is_empty() && area.height > 4 {
        let why = if !app.query.is_empty() {
            "Nothing matches the search."
        } else if l.reading_set() {
            "Reading the sets…"
        } else if left_out > 0 {
            "None of its tracks are on this drive."
        } else {
            "Nothing in this list."
        };
        let r = Rect { y: area.y + 3, height: 1, ..area.inner(Margin { vertical: 0, horizontal: 2 }) };
        f.render_widget(Paragraph::new(Line::styled(why, dim())).alignment(Alignment::Center), r);
    }
    if app.visible.len() > page {
        let mut sb = ScrollbarState::new(app.visible.len()).position(app.track_state.selected().unwrap_or(0));
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(fg(theme::DIM))
                .track_style(faint()),
            area.inner(Margin { vertical: 1, horizontal: 0 }),
            &mut sb,
        );
    }
}

/// The selected track's details: everything rekordbox knows about it, what's wrong with it, the
/// playlists it's in, and the tracks that mix into it (playable and taggable from here).
fn draw_track_details(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Details;
    app.areas.details = area;
    let b = block(title("Track", focused), focused);
    let inner = b.inner(area).inner(Margin { vertical: 0, horizontal: 1 });
    f.render_widget(b, area);
    let (Some(l), Some(i)) = (&app.loaded, app.selected_track()) else {
        f.render_widget(Paragraph::new(Line::styled("No track selected.", dim())), inner);
        return;
    };
    let t = &l.library.tracks[i];
    let w = inner.width as usize;
    let mut lines = vec![
        Line::styled(clip(if t.title.is_empty() { "Untitled" } else { &t.title }, w), bold(theme::FG)),
        Line::styled(clip(&t.artist, w), bold(theme::PINK)),
    ];
    let mut chips: Vec<Span> = Vec::new();
    if let Some(c) = t.color {
        chips.extend([theme::colour_dot(c), Span::raw(" ")]);
    }
    if t.bpm > 0.0 {
        chips.push(Span::styled(format!("{:.2} BPM  ", t.bpm), fg(theme::AMBER)));
    }
    match super::keys::Camelot::parse(&t.key) {
        Some(k) => chips.push(Span::styled(format!("{} {}  ", k.code(), k.note()), fg(theme::CYAN))),
        None if !t.key.is_empty() => chips.push(Span::styled(format!("{}  ", t.key), fg(theme::CYAN))),
        None => {}
    }
    chips.push(Span::styled(clock(t.duration_secs as f64), dim()));
    if t.rating > 0 {
        chips.push(Span::styled(format!("  {}", rating(t.rating)), fg(theme::AMBER)));
    }
    lines.push(Line::from(fit(chips, w)));
    // Problems early, so they show even in a short panel.
    if l.problems.is_missing(i) {
        lines.push(Line::styled(clip("✖ the audio file isn't on the drive", w), bold(theme::RED)));
    }
    let others: Vec<usize> = l.problems.others(i).collect();
    if let Some(&o) = others.first() {
        let other = &l.library.tracks[o];
        let more = if others.len() > 1 { format!(" (+{} more)", others.len() - 1) } else { String::new() };
        lines.push(Line::styled(clip(&format!("≈ also at {}{more}", other.file_path), w), fg(theme::AMBER)));
    }
    let mut about: Vec<String> =
        [&t.album, &t.remixer, &t.mix_name].into_iter().filter(|s| !s.is_empty()).cloned().collect();
    if t.year > 0 {
        about.push(t.year.to_string());
    }
    if !about.is_empty() {
        lines.push(Line::styled(clip(&about.join(" · "), w), dim()));
    }
    let kv = |k: &str, v: String| Line::from(vec![Span::styled(format!("{k:<8}"), faint()), Span::styled(v, text())]);
    for (k, v) in [("genre", &t.genre), ("label", &t.label)] {
        if !v.is_empty() {
            lines.push(kv(k, clip(v, w.saturating_sub(8))));
        }
    }
    let played = if t.play_count > 0 { format!("played {}×", t.play_count) } else { "never played".into() };
    let added = if t.date_added.is_empty() { String::new() } else { format!("added {} · ", t.date_added) };
    lines.push(Line::styled(clip(&format!("{added}{played}"), w), dim()));
    let mut format = vec![t.file_path.rsplit('.').next().unwrap_or("").to_uppercase()];
    if t.bitrate > 0 {
        format.push(format!("{} kbps", t.bitrate));
    }
    if t.sample_rate > 0 {
        format.push(format!("{:.1} kHz", t.sample_rate as f64 / 1000.0));
    }
    if t.sample_depth > 0 {
        format.push(format!("{}-bit", t.sample_depth));
    }
    if t.file_size > 0 {
        format.push(bytes(t.file_size));
    }
    lines.push(Line::styled(clip(&format.join(" · "), w), faint()));
    if !t.comment.trim().is_empty() {
        lines.push(Line::styled(clip(&format!("“{}”", t.comment.trim()), w), dim().add_modifier(Modifier::ITALIC)));
    }

    let details = app.details.as_ref().filter(|_| app.details_current());
    let playlists = details.map_or(&[][..], |d| &d.playlists[..]);
    // The details first, then the playlists, then the matches, when there's room for them.
    let matches_h = if inner.height >= 12 { 4 } else { 0 };
    let info_h = (lines.len() as u16).min(inner.height.saturating_sub(matches_h));
    let room = inner.height - info_h - matches_h;
    let lists_h = if playlists.is_empty() || room < 2 { 0 } else { (playlists.len() as u16 + 1).min(room).min(4) };
    let [info, lists_area, mixes] =
        Layout::vertical([Constraint::Length(info_h), Constraint::Length(lists_h), Constraint::Min(0)]).areas(inner);
    f.render_widget(Paragraph::new(lines), info);
    if lists_h > 0 {
        draw_in_playlists(f, lists_area, playlists, lists_h as usize - 1);
    }
    let why = if app.details_lookup_failed() {
        "couldn't look for matches (! says why)"
    } else if details.is_none() {
        "finding matches…"
    } else if super::keys::Camelot::parse(&t.key).is_none() {
        "no key to match"
    } else {
        "nothing in a compatible key and tempo"
    };
    let matches = details.map(|d| &d.matches);
    let rows = draw_matches(f, mixes, matches, None, why, &mut app.details_state, focused);
    app.areas.matches = rows;
}

/// The crate panel.
fn draw_crate(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Crate;
    app.areas.crate_panel = area;
    let list = &app.crate_list;
    let mut heading = vec![
        Span::raw(" "),
        Span::styled("Crate", if focused { bold(theme::CYAN) } else { bold(theme::DIM) }),
        Span::styled(format!(" {} ", list.len()), faint()),
    ];
    if !list.is_empty() {
        heading.push(Span::styled(format!("{} ", clock(list.duration_secs() as f64)), faint()));
    }
    let b = block(Line::from(heading), focused);
    let inner = b.inner(area);
    if list.is_empty() {
        let p = Paragraph::new(vec![
            Line::styled("The crate is empty.", dim()),
            Line::from(vec![
                Span::styled("a", theme::key()),
                Span::styled(" adds the selected or marked tracks to it.", dim()),
            ]),
        ])
        .wrap(Wrap { trim: true })
        .block(b);
        f.render_widget(p, area);
        return;
    }
    app.areas.crate_rows = inner;
    let root = app.loaded.as_ref().map(|l| l.root().to_path_buf());
    let playing = app.now.as_ref().map(|n| (n.root.clone(), n.track.file_path.clone()));
    let w = inner.width.saturating_sub(1);
    let items: Vec<ListItem> = list
        .items
        .iter()
        .enumerate()
        .map(|(n, item)| {
            let t = &item.track;
            let is_playing = playing.as_ref().is_some_and(|(r, p)| *r == item.root && *p == t.file_path);
            let mut left = vec![
                Span::styled(format!("{:>2} ", n + 1), faint()),
                Span::styled(t.title.clone(), if is_playing { bold(theme::PINK) } else { text() }),
                Span::styled(format!("  {}", t.artist), dim()),
            ];
            if root.as_ref() != Some(&item.root) {
                left.push(Span::styled(format!(" · {}", item.drive), faint()));
            }
            let mut right = Vec::new();
            if t.bpm > 0.0 {
                right.push(Span::styled(format!("{:.0} ", t.bpm), fg(theme::AMBER)));
            }
            right.push(Span::styled(format!("{:<3}", t.key), fg(theme::CYAN)));
            let lw: usize = left.iter().map(Span::width).sum::<usize>();
            let rw: usize = right.iter().map(Span::width).sum::<usize>();
            let left = fit(left, (w as usize).saturating_sub(rw + 1).max(lw.min(8)));
            ListItem::new(spread(left, right, w))
        })
        .collect();
    let list = List::new(items)
        .block(b)
        .highlight_style(if focused { theme::selected() } else { Style::new() })
        .highlight_symbol(Span::styled("▍", fg(theme::PINK)));
    app.crate_state.fit(inner.height as usize, app.crate_list.len());
    f.render_stateful_widget(list, area, &mut app.crate_state);
}

// ---------------------------------------------------------------------------------------------
// Player
// ---------------------------------------------------------------------------------------------

fn draw_player(f: &mut Frame, app: &mut App, area: Rect, big: bool) {
    let playing = app.is_playing();
    let b = block(
        Line::from(vec![
            Span::raw(" "),
            Span::styled(if playing { "▶" } else { "■" }, bold(theme::PINK)),
            Span::styled(" Now playing ", bold(theme::FG)),
        ]),
        false,
    )
    .title_top(
        if big {
            Line::from(vec![Span::styled(" f ", theme::key()), Span::styled("back ", dim())])
        } else {
            let mut spans = Vec::new();
            if app.deck.pads {
                spans.extend([Span::styled(" ' ", theme::key()), Span::styled("pads on ", bold(theme::AMBER))]);
            }
            spans.extend([Span::styled(" v ", theme::key()), Span::styled(format!("{} ", app.vis.name()), dim())]);
            Line::from(spans)
        }
        .right_aligned(),
    );
    let inner = b.inner(area);
    f.render_widget(b, area);
    if inner.height < 3 || inner.width < 20 {
        return;
    }

    let (head, visual, overview, progress) = if big {
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
        (head, visual, overview, progress)
    } else {
        let wave_h = if inner.height >= 10 { 4 } else { 3 };
        let [head, overview, progress] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(wave_h), Constraint::Length(1)]).areas(inner);
        (head, Rect::default(), overview, progress)
    };

    // Artwork as a square: cells are about twice as tall as they are wide.
    let art_rows = if big { head.height } else { inner.height };
    let art_cols = (art_rows * 2).min(inner.width / 3);
    let art_area = Rect { x: inner.x, y: inner.y, width: art_cols, height: art_cols / 2 };
    let rest_x = inner.x + art_cols + 2;
    let right = |r: Rect| Rect { x: rest_x, width: (r.x + r.width).saturating_sub(rest_x), ..r };
    // Full screen, the art only sits beside the head; the waveform and clock span the width.
    let (head, overview, progress) =
        if big { (right(head), overview, progress) } else { (right(head), right(overview), right(progress)) };
    let info_w = (head.width * 2 / 5).clamp(28.min(head.width), 52);
    let [info, _, beside] =
        Layout::horizontal([Constraint::Length(info_w), Constraint::Length(2), Constraint::Min(0)]).areas(head);
    app.areas.overview = overview;

    draw_art(f, app, art_area);
    draw_info(f, app, info, big);
    if big {
        if beside.width >= 16 {
            let inner = section(f, beside, "mirror");
            draw_spectrum(f, app, inner, true);
        }
        // Wide enough: track details and mixing suggestions beside the visualisers.
        if visual.width >= 110 {
            let side_w = (visual.width / 3).min(48);
            let [wall, _, side] =
                Layout::horizontal([Constraint::Min(0), Constraint::Length(2), Constraint::Length(side_w)])
                    .areas(visual);
            draw_wall(f, app, wall);
            draw_details(f, app, side);
        } else {
            draw_wall(f, app, visual);
        }
    } else {
        draw_visual(f, app, beside);
    }

    let pos = app.position();
    let cues = app.drawn_cues();
    match &app.now {
        Some(now) => {
            let dur = now.duration.as_secs_f64().max(0.001);
            let progress_frac = (pos.as_secs_f64() / dur) as f32;
            let (overview, strip) = if big && !now.phrases.is_empty() {
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
                    .map(|p| (p.start_ms, p.end_ms, p.kind.name(), theme::phrase_color(p.kind)))
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
            let label = app.now.as_ref().and_then(|n| n.track.color).map_or(theme::PINK, theme::track_color);
            let px = vinyl(area.width, area.height, angle, label, theme::VINYL);
            f.render_widget(Pixels { width: area.width, pixels: &px, palette: theme::palette() }, area);
        }
    }
}

fn chip(s: String, c: [u8; 3]) -> Span<'static> {
    Span::styled(format!(" {s} "), theme::on(c, [14, 14, 20]))
}

fn draw_info(f: &mut Frame, app: &App, area: Rect, big: bool) {
    let Some(now) = &app.now else {
        let mut lines = vec![Line::raw(""), Line::styled("Nothing playing", bold(theme::FG))];
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
            lines.push(Line::styled(format!("No audio output: {e}"), fg(theme::RED)));
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
        return;
    };
    let t = &now.track;
    let mut lines = vec![
        Line::styled(if t.title.is_empty() { "Untitled".into() } else { t.title.clone() }, bold(theme::FG)),
        Line::styled(t.artist.clone(), bold(theme::PINK)),
    ];
    let mut about: Vec<String> =
        [&t.album, &t.remixer, &t.mix_name].into_iter().filter(|s| !s.is_empty()).cloned().collect();
    if big && !t.label.is_empty() {
        about.push(t.label.clone());
    }
    if t.year > 0 {
        about.push(t.year.to_string());
    }
    lines.push(Line::styled(about.join(" · "), dim()));

    let pos_ms = app.position().as_secs_f64() * 1000.0;
    let beat = now.beat_at(pos_ms);
    let bpm = beat.map(|(i, _, _)| now.analysis.beats[i].bpm).filter(|b| *b > 0.0).unwrap_or(t.bpm);
    let mut chips = vec![chip(format!("{bpm:.2} BPM"), theme::AMBER), Span::raw(" ")];
    // Camelot and note name, whichever rekordbox stored.
    match now.key {
        Some(k) => chips.push(chip(format!("{} {}", k.code(), k.note()), theme::CYAN)),
        None if !t.key.is_empty() => chips.push(chip(t.key.clone(), theme::CYAN)),
        None => {}
    }
    if now.key.is_some() || !t.key.is_empty() {
        chips.push(Span::raw(" "));
    }
    if let Some((_, n, bar)) = beat {
        for k in 1..=4u16 {
            let (sym, c) = match (k == n, n == 1) {
                (true, true) => ("■", theme::PINK),
                (true, false) => ("■", theme::CYAN),
                _ => ("□", theme::FAINT),
            };
            chips.push(Span::styled(sym, bold(c)));
        }
        chips.push(Span::styled(format!(" {bar}.{n}"), dim()));
    }
    // A loop playing before the phrase, which the strip under the overview shows too.
    if let Some((_, beats)) = app.loop_playing() {
        chips.push(Span::raw("  "));
        chips.push(chip(format!("LOOP {}", beats.map_or("on".into(), super::deck::beats_label)), theme::LOOP));
    }
    let phrase = now.phrase_at(pos_ms);
    if let Some((_, p, bar)) = phrase {
        chips.push(Span::raw("  "));
        chips.push(chip(p.kind.name().to_uppercase(), theme::phrase_color(p.kind)));
        chips.push(Span::styled(format!(" {bar}/{}", p.bars), dim()));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(chips));
    if big && let Some((i, _, _)) = phrase {
        let mut spans = vec![Span::styled("phrase ", faint())];
        match now.phrases.get(i + 1) {
            Some(next) => {
                spans.push(Span::styled("next ", faint()));
                spans.push(Span::styled(next.kind.name(), bold(theme::phrase_color(next.kind))));
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
    if big && t.sample_depth > 0 {
        fmt.push(format!("{}-bit", t.sample_depth));
    }
    if big && t.file_size > 0 {
        fmt.push(bytes(t.file_size));
    }
    fmt.push(now.drive.clone());
    lines.push(Line::styled(fmt.join(" · "), faint()));

    let cues = &now.analysis.cues;
    if !cues.is_empty() {
        let mut hot: Vec<_> = cues.iter().filter(|c| c.hot_cue.is_some()).collect();
        hot.sort_by_key(|c| c.hot_cue);
        let memory = cues.len() - hot.len();
        // The hot cue the playhead last passed is underlined.
        let current =
            cues.iter().filter(|c| c.hot_cue.is_some() && c.start_ms as f64 <= pos_ms).max_by_key(|c| c.start_ms);
        let mut spans = Vec::new();
        // With the pads on, each hot cue's key before it.
        let pads = app.pads_on();
        for c in &hot {
            if pads && let Some(h) = c.hot_cue.filter(|h| *h < super::deck::PADS) {
                spans.push(Span::styled((h + 1).to_string(), theme::key()));
            }
            let mut s = chip(cue_letter(c).unwrap_or('?').to_string(), theme::palette().cue_color(c));
            if current.is_some_and(|cur| std::ptr::eq(cur, *c)) {
                s.style = s.style.add_modifier(Modifier::UNDERLINED);
            }
            spans.push(s);
        }
        if memory > 0 {
            spans.push(Span::styled(format!(" ◆{memory}"), fg(theme::MEMORY_CUE)));
        }
        let next =
            cues.iter().filter(|c| c.start_ms as f64 > pos_ms + 50.0).min_by_key(|c| (c.start_ms, c.hot_cue.is_none()));
        if let Some(c) = next {
            let mark = cue_letter(c).map_or("◆".to_string(), |l| l.to_string());
            spans.push(Span::styled("  next ", faint()));
            spans.push(Span::styled(mark, bold(theme::palette().cue_color(c))));
            spans.push(Span::styled(format!(" {}", clock((c.start_ms as f64 - pos_ms) / 1000.0)), dim()));
        }
        lines.push(Line::from(spans));
    }
    if big {
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
            add(Span::styled(rating(t.rating), fg(theme::AMBER)));
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
            spans.push(Span::styled("▮", fg(theme::phrase_color(p.kind))));
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
fn draw_matches(
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
        let mark = if theme::plain() { relation_mark(r) } else { "■" };
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
                    let bpm_style = if x.tempo_gap <= 0.02 { fg(theme::AMBER) } else { dim() };
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
                        spans.push(Span::styled(" next", bold(theme::CYAN)));
                    }
                    spans.push(Span::styled(format!("  {}", t.artist), dim()));
                    ListItem::new(Line::from(spans))
                })
                .collect();
            let list = List::new(items)
                .highlight_style(if focused { theme::selected() } else { Style::new() })
                .highlight_symbol(if focused { Span::styled("▍", fg(theme::PINK)) } else { Span::raw("") });
            state.fit(inner.height as usize, m.items.len());
            if focused {
                f.render_stateful_widget(list, inner, state);
            } else {
                // Unfocused, keep the scroll but don't highlight.
                let mut shown = ListState::default().with_offset(state.offset());
                f.render_stateful_widget(list, inner, &mut shown);
            }
            inner
        }
    }
}

/// "in playlists" over the top row of `area`, then up to `lists` of them, the last line saying
/// how many more there are.
fn draw_in_playlists(f: &mut Frame, area: Rect, playlists: &[String], lists: usize) {
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

fn section_with(f: &mut Frame, area: Rect, title: Line<'static>) -> Rect {
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
        KeyRelation::Same => theme::GREEN,
        KeyRelation::Adjacent => theme::CYAN,
        KeyRelation::Relative => theme::VIOLET,
        KeyRelation::EnergyBoost => theme::AMBER,
    }
}

/// The compact player's visualiser: the one picked with `v`, with the cue lane under the spectrum
/// and scope.
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
        Span::styled("■ ", faint())
    } else if app.is_playing() {
        Span::styled("▶ ", bold(theme::PINK))
    } else {
        Span::styled("‖ ", bold(theme::AMBER))
    };
    let left = vec![glyph, Span::styled(clock(if have { pos } else { 0.0 }), bold(theme::FG)), Span::raw(" ")];
    let vol = app.volume;
    let repeat = match app.repeat {
        Repeat::Off => Span::styled("↻ off", faint()),
        Repeat::All => Span::styled("↻ all", fg(theme::CYAN)),
        Repeat::One => Span::styled("↻ one", fg(theme::CYAN)),
    };
    let shuffle = if app.shuffle { Span::styled("⇄ on", fg(theme::CYAN)) } else { Span::styled("⇄ off", faint()) };
    let quantize =
        if app.deck.quantize { Span::styled("Q on", fg(theme::AMBER)) } else { Span::styled("Q off", faint()) };
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
        right.extend([Span::raw("  "), Span::styled("single", fg(theme::CYAN))]);
    }
    if app.deck.auto_cue {
        right.extend([Span::raw("  "), Span::styled("auto cue", fg(theme::CYAN))]);
    }
    let lw: usize = left.iter().map(Span::width).sum();
    let rw: usize = right.iter().map(Span::width).sum();
    let bar_w = (area.width as usize).saturating_sub(lw + rw);
    let frac = if dur > 0.0 { (pos / dur).clamp(0.0, 1.0) } else { 0.0 };
    let filled = (frac * bar_w as f64).round() as usize;
    let mut spans = left;
    if bar_w > 2 {
        spans.push(Span::styled("━".repeat(filled.min(bar_w - 1)), fg(theme::PINK)));
        spans.push(Span::styled("●", bold(theme::FG)));
        spans.push(Span::styled("─".repeat(bar_w.saturating_sub(filled + 1)), faint()));
    }
    spans.extend(right);
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ---------------------------------------------------------------------------------------------
// Drives
// ---------------------------------------------------------------------------------------------

/// A section's heading in the Drives view's right pane.
fn pane_heading(s: &str) -> Line<'static> {
    Line::styled(s.to_uppercase(), bold(theme::CYAN))
}

/// A labelled value in the Drives view's right pane.
fn pane_field(k: &str, v: String) -> Line<'static> {
    Line::from(vec![Span::styled(format!("  {k:<12}"), dim()), Span::styled(v, text())])
}

/// Where a drive stands against the store: a mark and a few words, coloured, and (with `hints`)
/// what to do about it.
fn standing(s: &DriveStatus, hints: bool) -> Vec<Span<'static>> {
    let save = Some("e, then save a snapshot");
    let (icon, msg, hint, c) = match (s.relation, &s.error) {
        (_, Some(e)) => ("✖", format!("error: {e}"), None, theme::RED),
        (None, _) if s.library.is_none() => ("○", "not in the store".to_string(), save, theme::DIM),
        (None, _) => ("○", "library has no snapshots".to_string(), None, theme::DIM),
        (Some(Relation::Identical), _) => ("✓", "up to date".to_string(), None, theme::GREEN),
        (Some(Relation::RightNewer), _) => match s.back_from_set {
            Some(0) => ("▲", "back from a set".to_string(), save, theme::AMBER),
            Some(n) => ("▲", format!("back from a set: {}", plural(n, "new session")), save, theme::AMBER),
            None => ("▲", "has changes not in the store".to_string(), save, theme::AMBER),
        },
        (Some(Relation::LeftNewer), _) => {
            ("▼", "behind the store".to_string(), Some("e, then sync, or locallibrary library push"), theme::AMBER)
        }
        (Some(Relation::Diverged), _) => ("⑂", "diverged from the store".to_string(), None, theme::RED),
        (Some(other), _) => ("?", format!("{other:?}").to_lowercase(), None, theme::DIM),
    };
    let msg = match hint.filter(|_| hints) {
        Some(h) => format!("{msg} ({h})"),
        None => msg,
    };
    vec![Span::styled(format!("{icon} "), bold(c)), Span::styled(msg, fg(c))]
}

fn draw_drives(f: &mut Frame, app: &mut App, area: Rect) {
    let [list, detail] =
        Layout::horizontal([Constraint::Length(36.min(area.width / 2)), Constraint::Min(0)]).areas(area);
    app.areas.drive_list = list;
    draw_drive_rows(f, app, list);
    match app.drive_header {
        Some(DriveRow::Library(id)) => return draw_library_clones(f, app, id, detail),
        Some(_) => {
            let p = Paragraph::new(vec![
                Line::raw(""),
                Line::styled("These drives aren't in the store yet.", bold(theme::FG)),
                Line::styled("Select one, then e to save a snapshot: that starts its library.", dim()),
            ])
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(block(title("No library", false), false));
            return f.render_widget(p, detail);
        }
        None => {}
    }
    let Some(d) = app.drive_state.selected().and_then(|i| app.drives.get(i)) else {
        let p = Paragraph::new(vec![
            Line::raw(""),
            Line::styled("No rekordbox drives mounted.", bold(theme::FG)),
            Line::styled("Drives appear here as soon as they're plugged in.", dim()),
        ])
        .alignment(Alignment::Center)
        .block(block(title("Drive", false), false));
        f.render_widget(p, detail);
        return;
    };
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::styled(drive_name(d), bold(theme::PINK)));
    let id =
        d.id.map_or("not adopted (locallibrary drive adopt)".to_string(), |id| format!("id {}", &id.to_string()[..8]));
    lines.push(Line::styled(format!("{} · {id}", d.root.display()), faint()));
    lines.push(Line::raw(""));

    if let Some(s) = &d.summary {
        lines.push(pane_heading("Library"));
        lines.push(pane_field("tracks", format!("{}  ·  {} artists", s.tracks, s.artists)));
        lines.push(pane_field("playlists", format!("{} in {} folders", s.playlists, s.folders)));
        lines.push(pane_field("duration", duration(s.total_duration_secs)));
        lines.push(pane_field("audio", bytes(s.audio_bytes)));
        if s.tracks_without_artwork > 0 {
            lines
                .push(pane_field("no artwork", format!("{} tracks (locallibrary art auto)", s.tracks_without_artwork)));
        }
        if let Some(at) = d.exported_at {
            let rev = d.pdb.as_ref().map(|p| format!("  ·  export.pdb rev {}", p.sequence)).unwrap_or_default();
            lines.push(pane_field("exported", format!("{}{rev}", local_time(at, "%Y-%m-%d %H:%M"))));
        }
        if d.has_library_plus {
            lines.push(pane_field("players", "OneLibrary (exportLibrary.db) present".into()));
        }
        lines.push(Line::raw(""));
    } else {
        lines.push(Line::styled("No rekordbox export on this drive.", dim()));
        lines.push(Line::raw(""));
    }

    if let (Some(free), Some(total)) = (d.free_space, d.total_space)
        && total > 0
    {
        lines.push(pane_heading("Space"));
        let used = 1.0 - free as f64 / total as f64;
        let w = detail.width.saturating_sub(6).min(48) as usize;
        let filled = (used * w as f64).round() as usize;
        let mut spans = vec![Span::raw("  ")];
        for i in 0..w {
            let c = theme::gradient(&[theme::CYAN, theme::VIOLET, theme::PINK], i as f32 / w as f32);
            spans.push(Span::styled(if i < filled { "█" } else { "░" }, fg(if i < filled { c } else { theme::FAINT })));
        }
        lines.push(Line::from(spans));
        lines.push(Line::styled(
            format!("  {:.0}% used · {} free of {}", used * 100.0, bytes(free), bytes(total)),
            dim(),
        ));
        lines.push(Line::raw(""));
    }

    let fleet = app.fleet_drive(&d.root);
    lines.push(pane_heading("Store"));
    match (&app.status, fleet) {
        (None, _) | (_, None) if app.status_loading => lines.push(Line::styled("  checking…", dim())),
        (Some(Err(e)), _) => lines.push(Line::styled(format!("  {e}"), dim())),
        (_, Some(fd)) => {
            let s = &fd.status;
            lines.push(Line::from([vec![Span::raw("  ")], standing(s, true)].concat()));
            if let Some(lib) = &s.library {
                let group = app
                    .status
                    .as_ref()
                    .and_then(|r| r.as_ref().ok())
                    .map_or(0, |list| list.iter().filter(|o| o.status.library.as_ref() == Some(lib)).count());
                let plugged = if group > 1 { format!("  ·  {group} of its drives plugged in") } else { String::new() };
                lines.push(pane_field("library", format!("{lib}{plugged}")));
            }
            if let Some(h) = &s.head {
                lines
                    .push(pane_field("latest", format!("{}  {}", h.short_id, local_time(h.created, "%Y-%m-%d %H:%M"))));
            }
            if s.settings_drift > 0 {
                lines.push(Line::styled(
                    format!("  {} setting(s) differ from the profile (locallibrary settings apply)", s.settings_drift),
                    fg(theme::AMBER),
                ));
            }
        }
        _ => lines.push(Line::styled("  press R to check", dim())),
    }
    lines.push(Line::raw(""));

    // What the registry knows: its role, its logbook, copies of it.
    if let Some(fd) = fleet {
        let mut warn = Vec::new();
        if fd.read_only {
            warn.push("write-protected: players can't save history or cues to it".to_string());
        }
        if fd.low_space {
            warn.push("nearly full: players need room for history and cues".to_string());
        }
        for other in &fd.same_identity_as {
            warn.push(format!(
                "a copy of it (or it of) {}: locallibrary drive reidentify gives one its own",
                other.display()
            ));
        }
        let role = fd.role.map(super::drives::role_name);
        if role.is_some() || fd.log.is_some() || !warn.is_empty() {
            lines.push(pane_heading("Logbook"));
            if let Some(r) = role {
                lines.push(pane_field("role", r));
            }
            if let Some(log) = &fd.log {
                if log.writes > 0 {
                    lines.push(pane_field("written", format!("{} times, {}", log.writes, bytes(log.bytes_written))));
                }
                if log.clean_ejects > 0 {
                    lines.push(pane_field("ejected", format!("{} times cleanly", log.clean_ejects)));
                }
                for (what, c) in [("verified", &log.last_scrub), ("fs check", &log.last_fs_check)] {
                    if let Some(c) = c {
                        let mark = if c.ok { "✓" } else { "✖" };
                        lines.push(pane_field(what, format!("{mark} {}  {}", local_time(c.at, "%Y-%m-%d"), c.summary)));
                    }
                }
                if let Some(p) = log.problems.last() {
                    lines.push(Line::styled(
                        format!("  ✖ {} {}: {}", local_time(p.at, "%Y-%m-%d"), p.kind.replace('_', " "), p.detail),
                        fg(theme::RED),
                    ));
                }
            }
            for w in warn {
                lines.push(Line::styled(format!("  ⚠ {w}"), fg(theme::AMBER)));
            }
            lines.push(Line::raw(""));
        }
    }

    if !d.settings_files.is_empty() {
        lines.push(pane_heading("My Settings"));
        lines.push(Line::styled(format!("  {}", d.settings_files.join(" · ")), text()));
        lines.push(Line::raw(""));
    }
    if d.lineage_len > 0 {
        lines.push(pane_heading("States"));
        let last = d
            .last_known
            .as_ref()
            .map(|e| format!(" · last: {:?} {}", e.op, local_time(e.at, "%Y-%m-%d %H:%M")).to_lowercase())
            .unwrap_or_default();
        let states = if d.lineage_len == 1 { "state" } else { "states" };
        lines.push(Line::styled(format!("  {} {states} recorded{last}", d.lineage_len), text()));
        lines.push(Line::raw(""));
    }
    if let Some(p) = &d.pending {
        lines.push(Line::styled(
            format!(
                "⚠ interrupted: {} ({}), re-run it to finish",
                p.description,
                local_time(p.started, "%Y-%m-%d %H:%M")
            ),
            bold(theme::AMBER),
        ));
        lines.push(Line::raw(""));
    }
    lines.push(pane_heading("Actions"));
    // `u` undoes on the drive changed last this run, which may be another.
    let undo = app.undo_drive().map(|r| format!("undo the last change to {}", app.drive_label(&r))).unwrap_or_default();
    for (k, what) in [
        ("e", "save a snapshot · check · sync · pack · bring back a playlist · replay edits · eject"),
        ("u", undo.as_str()),
        ("enter", "browse it in the library"),
    ] {
        lines.push(Line::from(vec![Span::styled(format!("  {k:<6}"), theme::key()), Span::styled(what, text())]));
    }

    let p = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(block(title("Drive", false), false).padding(ratatui::widgets::Padding::horizontal(1)));
    f.render_widget(p, detail);
}

/// The Drives view's list: each library (how many of its clones are plugged in, how they stand),
/// its drives under it, then the drives of no library. Each row takes two lines.
fn draw_drive_rows(f: &mut Frame, app: &mut App, area: Rect) {
    let rows = app.drive_rows();
    let loaded_root = app.loaded.as_ref().map(|l| l.root().to_path_buf());
    let inner_w = area.width.saturating_sub(3);
    // Drives are indented under the libraries, when there are any.
    let indent = if rows.iter().any(|r| matches!(r, DriveRow::Library(_))) { "  " } else { "" };
    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| {
            let lines = match *row {
                DriveRow::Library(id) => {
                    let name = app.library_names.get(&id).cloned().unwrap_or_default();
                    let plugged = app.drives.iter().filter(|d| app.library_id(d) == Some(id)).count();
                    let fleet = app.fleet_libraries.iter().find(|l| l.id == id);
                    let count = match fleet {
                        Some(l) => format!("{plugged} of {}", plugged + l.offline.len()),
                        None => plugged.to_string(),
                    };
                    let left = vec![Span::styled(name, bold(theme::FG))];
                    let about = match fleet {
                        Some(l) => library_state(l),
                        None if app.status_loading => vec![Span::styled("checking…", faint())],
                        None => Vec::new(),
                    };
                    vec![
                        spread(left, vec![Span::styled(count, dim())], inner_w),
                        Line::from([vec![Span::raw("  ")], about].concat()),
                    ]
                }
                DriveRow::Unlinked => {
                    vec![Line::styled("No library", bold(theme::DIM)), Line::styled("  not in the store yet", faint())]
                }
                DriveRow::Drive(i) => {
                    let d = &app.drives[i];
                    let open = loaded_root.as_deref() == Some(d.root.as_path());
                    let root = Line::styled(format!("{indent}  {}", d.root.display()), faint());
                    vec![drive_line(d, open, indent, inner_w), root]
                }
            };
            ListItem::new(Text::from(lines))
        })
        .collect();
    app.drive_rows.select(app.drive_row(&rows));
    // Each row takes two lines.
    app.drive_rows.fit(area.height.saturating_sub(2) as usize / 2, rows.len());
    f.render_stateful_widget(drive_list(items, true), area, &mut app.drive_rows);
}

/// How a library's clones plugged in stand together: whether they hold the same, and whether
/// the store has what they hold.
fn library_state(l: &FleetLibrary) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut say = |s: &'static str, c| {
        if !out.is_empty() {
            out.push(Span::styled(" · ", faint()));
        }
        out.push(Span::styled(s, fg(c)));
    };
    match l.identical {
        Some(true) => say("identical", theme::GREEN),
        Some(false) => say("not identical", theme::AMBER),
        None => {}
    }
    match (&l.head, l.unsaved) {
        (None, _) => say("no snapshots yet", theme::DIM),
        (Some(_), true) => say("unsaved", theme::AMBER),
        (Some(_), false) => say("saved", theme::DIM),
    }
    out
}

/// The Drives view's right pane for a library: its clones (those not plugged in too) and where
/// each stands, the store's latest snapshot of it, and what can be done to all of them.
fn draw_library_clones(f: &mut Frame, app: &App, id: Uuid, area: Rect) {
    let name = app.library_names.get(&id).cloned().unwrap_or_default();
    let fleet = app.fleet_libraries.iter().find(|l| l.id == id);
    let here: Vec<&DeviceInfo> = app.drives.iter().filter(|d| app.library_id(d) == Some(id)).collect();
    let away = fleet.map_or(&[][..], |l| l.offline.as_slice());
    let mut lines = vec![
        Line::styled(name, bold(theme::PINK)),
        Line::styled(format!("{} of {} plugged in", here.len(), plural(here.len() + away.len(), "clone")), faint()),
        Line::raw(""),
        pane_heading("Clones"),
    ];
    let name_w = here
        .iter()
        .map(|d| drive_name(d).chars().count())
        .chain(away.iter().map(|k| k.name.chars().count()))
        .max()
        .unwrap_or(0);
    let clone = |dot: Span<'static>, name: String, role: Option<Role>, state: Vec<Span<'static>>| {
        let role = format!("{:<9}", role.map_or("", Role::name));
        Line::from(
            [
                vec![
                    Span::raw("  "),
                    dot,
                    Span::styled(format!("{name:<name_w$}  "), text()),
                    Span::styled(role, dim()),
                ],
                state,
            ]
            .concat(),
        )
    };
    for d in &here {
        let fd = app.fleet_drive(&d.root);
        let state = match fd {
            Some(fd) => standing(&fd.status, false),
            None if app.status_loading => vec![Span::styled("checking…", dim())],
            None => Vec::new(),
        };
        lines.push(clone(Span::styled("● ", fg(theme::GREEN)), drive_name(d), fd.and_then(|f| f.role), state));
    }
    for k in away {
        let behind = match &k.standing {
            Standing::Behind { snapshots, .. } => format!(" · {} behind", plural(*snapshots, "snapshot")),
            Standing::UpToDate => " · up to date".into(),
            Standing::Unsaved => " · had changes not in the store".into(),
            Standing::Unknown => String::new(),
        };
        let state = vec![Span::styled(format!("not plugged in{behind}"), dim())];
        lines.push(clone(Span::styled("○ ", faint()), k.name.clone(), k.role, state));
    }
    lines.push(Line::raw(""));
    lines.push(pane_heading("Store"));
    match (&app.status, fleet) {
        (_, Some(l)) => {
            if let Some(h) = &l.head {
                lines
                    .push(pane_field("latest", format!("{}  {}", h.short_id, local_time(h.created, "%Y-%m-%d %H:%M"))));
            }
            lines.push(Line::from(
                [vec![Span::styled(format!("  {:<12}", "clones"), dim())], library_state(l)].concat(),
            ));
        }
        (Some(Err(e)), _) => lines.push(Line::styled(format!("  {e}"), dim())),
        _ if app.status_loading => lines.push(Line::styled("  checking…", dim())),
        _ => lines.push(Line::styled("  press R to check", dim())),
    }
    lines.push(Line::raw(""));
    lines.push(pane_heading("Actions"));
    for (k, what) in [("enter", "browse its newest clone in the library"), ("e", "sync, check or eject its clones")] {
        lines.push(Line::from(vec![Span::styled(format!("  {k:<6}"), theme::key()), Span::styled(what, text())]));
    }
    let p = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(block(title("Library", false), false).padding(ratatui::widgets::Padding::horizontal(1)));
    f.render_widget(p, area);
}

// ---------------------------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------------------------

fn draw_history(f: &mut Frame, app: &mut App, area: Rect) {
    app.areas.history = area;
    let b = block(title("Snapshots", true), true);
    let libs = match &app.history {
        None => {
            f.render_widget(Paragraph::new(Line::styled("loading…", dim())).block(b), area);
            return;
        }
        Some(Err(e)) => {
            let p = Paragraph::new(vec![Line::raw(""), Line::styled(e.clone(), dim())])
                .alignment(Alignment::Center)
                .block(b);
            f.render_widget(p, area);
            return;
        }
        Some(Ok(libs)) => libs,
    };
    let mut rows = Vec::new();
    for (lib, snaps) in libs {
        for (i, s) in snaps.iter().enumerate() {
            let head = i == 0 && lib.head.as_ref() == Some(&s.id);
            let source = s.source.as_ref().map(|x| x.drive_name.clone()).unwrap_or_default();
            rows.push(Row::new(vec![
                Cell::from(Span::styled(if i == 0 { lib.name.clone() } else { String::new() }, bold(theme::PINK))),
                Cell::from(Span::styled(
                    if head { "●" } else { "│" },
                    fg(if head { theme::GREEN } else { theme::FAINT }),
                )),
                Cell::from(Span::styled(s.short_id.clone(), fg(theme::AMBER))),
                Cell::from(Span::styled(local_time(s.created, "%Y-%m-%d %H:%M").to_string(), text())),
                Cell::from(Line::styled(s.summary.tracks.to_string(), text()).right_aligned()),
                Cell::from(Line::styled(bytes(s.bytes), dim()).right_aligned()),
                Cell::from(Span::styled(source, fg(theme::CYAN))),
                Cell::from(Span::styled(s.message.clone().unwrap_or_default(), dim())),
            ]));
        }
    }
    if rows.is_empty() {
        let p = Paragraph::new(vec![
            Line::raw(""),
            Line::styled("The store has no snapshots yet.", bold(theme::FG)),
            Line::styled("locallibrary pull <drive> -m \"after Friday's export\"", fg(theme::CYAN)),
        ])
        .alignment(Alignment::Center)
        .block(b);
        f.render_widget(p, area);
        return;
    }
    // Under the border and the header row.
    app.history_state.fit(area.height.saturating_sub(3) as usize, rows.len());
    let header = ["Library", "", "Snapshot", "Created", "Tracks", "Size", "From", "Message"];
    let table = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Length(1),
            Constraint::Length(9),
            Constraint::Length(16),
            Constraint::Length(7),
            Constraint::Length(9),
            Constraint::Length(16),
            Constraint::Fill(1),
        ],
    )
    .header(Row::new(header.into_iter().map(|h| Cell::from(Span::styled(h, bold(theme::DIM))))))
    .block(b)
    .column_spacing(2)
    .row_highlight_style(theme::selected())
    .highlight_symbol(Span::styled("▍", fg(theme::PINK)));
    f.render_stateful_widget(table, area, &mut app.history_state);
}

// ---------------------------------------------------------------------------------------------
// Dialogs
// ---------------------------------------------------------------------------------------------

/// A box of `w` x `h` in the middle of `area`, or by the pointer when opened with the mouse.
fn popup(area: Rect, w: u16, h: u16, at: Option<(u16, u16)>) -> Rect {
    let (w, h) = (w.min(area.width), h.min(area.height));
    let (x, y) = match at {
        Some((x, y)) => (x.min(area.right().saturating_sub(w)), (y + 1).min(area.bottom().saturating_sub(h))),
        None => (area.x + (area.width - w) / 2, area.y + (area.height - h) / 2),
    };
    Rect { x, y, width: w, height: h }
}

/// How many rows `lines` take wrapped to `width` (near enough: words may wrap a little earlier).
fn wrapped_height(lines: &[Line], width: u16) -> u16 {
    let w = width.max(1) as usize;
    lines.iter().map(|l| l.width().max(1).div_ceil(w) as u16).sum()
}

fn draw_menu(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(Modal::Menu(m)) = &mut app.modal else { return };
    let w = 50u16.min(area.width);
    let (heading, items, state): (String, Vec<ListItem>, &mut ListState) = match m.sub {
        None => {
            let label_w = (w as usize).saturating_sub(9);
            let items = m
                .items
                .iter()
                .map(|i| {
                    let (k, l) = if i.unavailable.is_some() { (faint(), faint()) } else { (theme::key(), text()) };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:<5}", i.key), k),
                        Span::styled(clip(&i.label, label_w), l),
                    ]))
                })
                .collect();
            (format!("Edit · {}", clip(&m.target.what, (w as usize).saturating_sub(14))), items, &mut m.state)
        }
        Some(sub) => {
            let items = Menu::sub_items(sub)
                .into_iter()
                .map(|a| match a {
                    super::edits::Action::Rate(n) => ListItem::new(Line::from(vec![
                        Span::styled(format!("{n:<5}"), theme::key()),
                        Span::styled(if n == 0 { "no rating".into() } else { stars(n) }, fg(theme::AMBER)),
                    ])),
                    super::edits::Action::Colour(c) => {
                        let n = c.map_or(0, |c| COLOURS.iter().position(|x| *x == c).unwrap_or(0) + 1);
                        ListItem::new(Line::from(vec![
                            Span::styled(format!("{n:<5}"), theme::key()),
                            match c {
                                Some(c) => Span::styled("● ", fg(theme::track_color(c))),
                                None => Span::styled("○ ", faint()),
                            },
                            Span::styled(c.map_or("none", colour_name), text()),
                        ]))
                    }
                    _ => ListItem::new(""),
                })
                .collect();
            let what = if sub == Sub::Rating { "Rating" } else { "Colour" };
            (format!("{what} · {}", clip(&m.target.what, (w as usize).saturating_sub(16))), items, &mut m.sub_state)
        }
    };
    let h = items.len() as u16 + 2;
    let r = popup(area, w, h, m.at);
    state.fit(r.height.saturating_sub(2) as usize, items.len());
    let b = block(title(&heading, true), true)
        .title_bottom(Line::styled(" every change to the drive asks first ", faint()).right_aligned());
    let list = List::new(items)
        .block(b)
        .highlight_style(theme::selected())
        .highlight_symbol(Span::styled("▍", fg(theme::PINK)));
    f.render_widget(Clear, r);
    f.render_stateful_widget(list, r, state);
    app.areas.menu = r;
}

/// A list to choose from (the command palette, a drive, a snapshot): what's typed on top, the
/// matching items under it, each with its detail on the right.
fn draw_picker(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(Modal::Pick(p)) = &mut app.modal else { return };
    let w = 72u16.min(area.width.saturating_sub(2));
    let rows = (p.shown.len().max(1) as u16).min(area.height.saturating_sub(6).clamp(1, 16));
    let h = rows + 3;
    // Near the top, so the list grows down from where the eye is.
    let r = Rect { y: area.y + (area.height.saturating_sub(h) / 4), ..popup(area, w, h, None) };
    let b = block(title(&p.title, true), true)
        .title_bottom(Line::styled(" ↑↓ choose · enter · esc close ", faint()).right_aligned());
    let inner = b.inner(r);
    f.render_widget(Clear, r);
    f.render_widget(b, r);
    let [input, list] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
    let label = vec![Span::styled(" › ", theme::key())];
    let n = p.shown.len();
    let after = vec![Span::styled(format!("  {n} of {}", p.items.len()), faint())];
    f.render_widget(Paragraph::new(input_line(label, &p.query, after, input.width)), input);
    if p.shown.is_empty() {
        f.render_widget(Paragraph::new(Line::styled(format!("  {}", p.empty), dim())), list);
    } else {
        let w = list.width.saturating_sub(2);
        let items: Vec<ListItem> = (p.shown.iter().map(|&i| &p.items[i]))
            .map(|it| {
                let style = if it.unavailable.is_some() { faint() } else { text() };
                let right = vec![Span::styled(format!("{} ", it.detail), theme::key())];
                let rw: usize = right.iter().map(Span::width).sum();
                let left = vec![Span::styled(clip(&it.label, (w as usize).saturating_sub(rw + 1)), style)];
                ListItem::new(spread(left, right, w))
            })
            .collect();
        let l =
            List::new(items).highlight_style(theme::selected()).highlight_symbol(Span::styled("▍", fg(theme::PINK)));
        p.state.fit(list.height as usize, n);
        f.render_stateful_widget(l, list, &mut p.state);
    }
    app.areas.menu = r;
}

/// Results to read (a check's, a plugin's command's): a panel that scrolls, closed by any other
/// key.
fn draw_report(f: &mut Frame, app: &mut App, area: Rect) {
    use super::drives::Tone;
    let Some(Modal::Report(r)) = &mut app.modal else { return };
    let w = 100.min(area.width);
    let inner_w = w.saturating_sub(4) as usize;
    let mut lines: Vec<Line> = Vec::new();
    for (tone, text_) in &r.lines {
        let (mark, style) = match tone {
            Tone::Heading => ("", bold(theme::CYAN)),
            Tone::Ok => ("✓ ", fg(theme::GREEN)),
            Tone::Warning => ("⚠ ", fg(theme::AMBER)),
            Tone::Problem => ("✖ ", fg(theme::RED)),
            Tone::Text | Tone::Row => ("", text()),
            Tone::Faint => ("", faint()),
            Tone::Columns => ("", dim()),
        };
        let parts = match tone {
            Tone::Columns | Tone::Row => vec![clip(text_, inner_w)],
            _ => wrap_text(text_, inner_w.saturating_sub(mark.chars().count())),
        };
        for (i, part) in parts.into_iter().enumerate() {
            let lead = if i == 0 { mark.to_string() } else { " ".repeat(mark.chars().count()) };
            lines.push(Line::from(vec![Span::styled(lead, style), Span::styled(part, style)]));
        }
    }
    let total = lines.len() as u16;
    let r_area = popup(area, w, total + 2, None);
    let b = block(title(&r.title, true), true)
        .title_bottom(Line::styled(" ↑↓ scroll · any other key closes ", faint()).right_aligned())
        .padding(ratatui::widgets::Padding::horizontal(1));
    let inner = b.inner(r_area);
    r.scroll = r.scroll.min(total.saturating_sub(inner.height));
    f.render_widget(Clear, r_area);
    f.render_widget(Paragraph::new(lines).scroll((r.scroll, 0)).block(b), r_area);
}

/// A plugin's params to fill in: a row for each field (its label, then what's typed or picked),
/// then what the selected one is for, and why what's filled in won't do.
fn draw_form(f: &mut Frame, app: &mut App, area: Rect) {
    use unicode_width::UnicodeWidthStr;
    let Some(Modal::Form(form)) = &app.modal else { return };
    let w = 72u16.min(area.width.saturating_sub(2));
    let inner_w = w.saturating_sub(4) as usize;
    let label_w = form.fields.iter().map(|x| x.label().width()).max().unwrap_or(0).min(inner_w / 3);
    let mut lines: Vec<Line> = Vec::new();
    for (i, field) in form.fields.iter().enumerate() {
        let on = i == form.selected;
        let mut spans = vec![
            Span::styled(if on { "▍" } else { " " }, fg(theme::PINK)),
            Span::styled(
                format!("{:<label_w$}  ", clip(&field.label(), label_w)),
                if on { bold(theme::FG) } else { dim() },
            ),
        ];
        let room = inner_w.saturating_sub(label_w + 3);
        match field.text.as_str() {
            "true" if field.is_switch() => spans.push(Span::styled("● yes", fg(theme::PINK))),
            _ if field.is_switch() => spans.push(Span::styled("○ no", faint())),
            picked if !field.choices().is_empty() => {
                spans.push(Span::styled("‹ ", faint()));
                spans.push(Span::styled(if picked.is_empty() { "none" } else { picked }.to_string(), text()));
                spans.push(Span::styled(" ›", faint()));
            }
            typed if on => {
                spans.push(Span::styled(typed_tail(typed, room.saturating_sub(1)).to_string(), bold(theme::FG)));
                spans.push(Span::styled("▏", fg(theme::PINK)));
            }
            typed => spans.push(Span::styled(clip(typed, room), text())),
        }
        if field.required && field.text.is_empty() {
            spans.push(Span::styled("required", faint()));
        }
        let line = Line::from(spans);
        lines.push(if on { line.style(theme::selected()) } else { line });
    }
    let selected = &form.fields[form.selected];
    let about = selected.schema["description"].as_str().or(selected.schema["title"].as_str()).unwrap_or_default();
    if !about.is_empty() {
        lines.push(Line::raw(""));
        lines.extend(wrap_text(about, inner_w).into_iter().map(|l| Line::styled(l, dim())));
    }
    if let Some(e) = &form.error {
        lines.push(Line::raw(""));
        lines.extend(wrap_text(&format!("⚠ {e}"), inner_w).into_iter().map(|l| Line::styled(l, fg(theme::AMBER))));
    }
    let r = popup(area, w, lines.len() as u16 + 2, None);
    let keys = if selected.choices().is_empty() { "" } else { "←→ change · " };
    let b = block(title(&form.title, true), true)
        .title_bottom(Line::styled(format!(" ↑↓ field · {keys}enter run · esc cancel "), faint()).right_aligned())
        .padding(ratatui::widgets::Padding::horizontal(1));
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines).block(b), r);
    app.areas.menu = r;
}

/// The outputs to play through (`d`), the system's default first.
fn draw_outputs(f: &mut Frame, app: &mut App, area: Rect) {
    let playing = app.audio.as_ref().map(|a| a.device().map(str::to_string));
    let Some(o) = &mut app.deck.outputs else { return };
    let w = 56u16.min(area.width);
    let mark = |on: bool| if on { Span::styled("● ", bold(theme::PINK)) } else { Span::raw("  ") };
    let default = match &o.default {
        Some(d) => format!("the system's default ({d})"),
        None => "the system's default".into(),
    };
    let mut items = vec![ListItem::new(Line::from(vec![mark(playing == Some(None)), Span::styled(default, text())]))];
    for n in &o.names {
        let on = playing.as_ref().is_some_and(|p| p.as_deref() == Some(n.as_str()));
        items.push(ListItem::new(Line::from(vec![
            mark(on),
            Span::styled(clip(n, (w as usize).saturating_sub(6)), text()),
        ])));
    }
    let r = popup(area, w, items.len() as u16 + 2, None);
    o.state.fit(r.height.saturating_sub(2) as usize, items.len());
    let b = block(title("Output", true), true)
        .title_bottom(Line::styled(" enter play through it · esc close ", faint()).right_aligned());
    let list = List::new(items)
        .block(b)
        .highlight_style(theme::selected())
        .highlight_symbol(Span::styled("▍", fg(theme::PINK)));
    f.render_widget(Clear, r);
    f.render_stateful_widget(list, r, &mut o.state);
}

/// A playlist and track edit's changes, in detail.
fn edit_lines(p: &locallibrary::edit::EditPlan, lines: &mut Vec<Line<'static>>) {
    for change in &p.changes {
        lines
            .push(Line::from(vec![Span::styled("• ", fg(theme::CYAN)), Span::styled(change.clone(), bold(theme::FG))]));
    }
    for path in &p.playlists_created {
        lines.push(Line::from(vec![Span::styled("  + ", bold(theme::GREEN)), Span::styled(path.clone(), text())]));
    }
    for path in &p.playlists_deleted {
        lines.push(Line::from(vec![Span::styled("  − ", bold(theme::RED)), Span::styled(path.clone(), text())]));
    }
    for ch in &p.playlists_changed {
        let what = if ch.reordered {
            "reordered".to_string()
        } else {
            let mut parts = Vec::new();
            if ch.added > 0 {
                parts.push(format!("+{}", ch.added));
            }
            if ch.removed > 0 {
                parts.push(format!("−{}", ch.removed));
            }
            if parts.is_empty() { "entries changed".into() } else { parts.join(" ") }
        };
        lines.push(Line::from(vec![
            Span::styled("  ~ ", bold(theme::AMBER)),
            Span::styled(ch.path.clone(), text()),
            Span::styled(format!("  {what}, {} → {} tracks", ch.before, ch.after), dim()),
        ]));
    }
    if p.tracks_changed > 0 {
        lines.push(Line::from(vec![
            Span::styled("  ~ ", bold(theme::AMBER)),
            Span::styled(format!("{} changed", plural(p.tracks_changed, "track")), text()),
        ]));
    }
    if !p.tracks_removed.is_empty() {
        let files =
            if p.files.is_empty() { String::new() } else { format!(", {} files, {}", p.files.len(), bytes(p.bytes)) };
        lines.push(Line::from(vec![
            Span::styled("  − ", bold(theme::RED)),
            Span::styled(format!("{} removed{files}", plural(p.tracks_removed.len(), "track")), text()),
        ]));
        for t in p.tracks_removed.iter().take(5) {
            lines.push(Line::styled(format!("      {}", t.name), dim()));
        }
        if p.tracks_removed.len() > 5 {
            lines.push(Line::styled(format!("      and {} more", p.tracks_removed.len() - 5), dim()));
        }
    }
    if p.history_removed > 0 {
        lines.push(Line::from(vec![
            Span::styled("  − ", bold(theme::RED)),
            Span::styled(format!("{} removed", plural(p.history_removed, "history list")), text()),
        ]));
    }
    for w in &p.warnings {
        lines.push(Line::styled(format!("⚠ {w}"), fg(theme::AMBER)));
    }
}

/// Undoing, in detail: the backup, the files, how playlists and tracks change, and the
/// recorded edits that come off the journal.
fn undo_lines(c: &super::edits::Confirm, lines: &mut Vec<Line<'static>>) {
    let Some(u) = c.undo() else { return };
    let b = &u.backup;
    lines.push(Line::from(vec![
        Span::styled(format!("{}: ", c.drive), bold(theme::PINK)),
        Span::styled(format!("put back the backup ({})", b.reason), text()),
    ]));
    lines.push(Line::styled(format!("made {}  ·  {}", local_time(b.created, "%Y-%m-%d %H:%M"), b.id), faint()));
    lines.push(Line::raw(""));
    let d = &u.diff;
    for p in &d.added_playlists {
        lines.push(Line::from(vec![Span::styled("  + ", bold(theme::GREEN)), Span::styled(p.clone(), text())]));
    }
    for p in &d.removed_playlists {
        lines.push(Line::from(vec![Span::styled("  − ", bold(theme::RED)), Span::styled(p.clone(), text())]));
    }
    for p in &d.changed_playlists {
        let mut parts = Vec::new();
        if !p.added.is_empty() {
            parts.push(format!("+{}", p.added.len()));
        }
        if !p.removed.is_empty() {
            parts.push(format!("−{}", p.removed.len()));
        }
        if !p.moved.is_empty() {
            parts.push(format!("{} moved", p.moved.len()));
        }
        lines.push(Line::from(vec![
            Span::styled("  ~ ", bold(theme::AMBER)),
            Span::styled(p.path.clone(), text()),
            Span::styled(format!("  {}", parts.join(" ")), dim()),
        ]));
    }
    let tracks = [
        (d.added_tracks.len(), "back", theme::GREEN, "  + "),
        (d.removed_tracks.len(), "gone", theme::RED, "  − "),
        (d.changed_tracks.len(), "as they were", theme::AMBER, "  ~ "),
    ];
    for (n, what, c, mark) in tracks.into_iter().filter(|t| t.0 > 0) {
        lines.push(Line::from(vec![
            Span::styled(mark, bold(c)),
            Span::styled(format!("{} {what}", plural(n, "track")), text()),
        ]));
    }
    if d.is_empty() {
        lines.push(Line::styled("  playlists and tracks stay as they are", dim()));
    }
    let bytes_back: u64 = u.files.iter().filter(|f| f.change != "removed").map(|f| f.bytes).sum();
    lines.push(Line::styled(format!("{} put back ({})", plural(u.files.len(), "file"), bytes(bytes_back)), dim()));
    for f in u.files.iter().take(4) {
        lines.push(Line::styled(format!("    {} {}", f.change, f.path), faint()));
    }
    if u.files.len() > 4 {
        lines.push(Line::styled(format!("    and {} more", u.files.len() - 4), faint()));
    }
    if !u.journal.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            format!(
                "{} off the journal, so edits replay won't make {} again:",
                match u.journal.len() {
                    1 => "1 recorded edit comes".to_string(),
                    n => format!("{n} recorded edits come"),
                },
                if u.journal.len() == 1 { "it" } else { "them" }
            ),
            dim(),
        ));
        for e in u.journal.iter().flat_map(|e| &e.changes).take(4) {
            lines.push(Line::styled(format!("    {e}"), faint()));
        }
    }
    for w in c.plan.warnings() {
        lines.push(Line::styled(format!("⚠ {w}"), fg(theme::AMBER)));
    }
}

fn draw_confirm(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(Modal::Confirm(c)) = &mut app.modal else { return };
    let wording = super::edits::wording(&*c.plan, &c.drive);
    let mut lines = Vec::new();
    match c.edit() {
        Some(p) => {
            lines.push(Line::from(vec![
                Span::styled(format!("{}: ", c.drive), bold(theme::PINK)),
                Span::styled(p.databases.join(" and "), text()),
            ]));
            lines.push(Line::raw(""));
            edit_lines(p, &mut lines);
            lines.push(Line::raw(""));
            lines.push(Line::styled("The drive is backed up first: u takes this back.", dim()));
            lines.push(Line::styled(
                "rekordbox's next export to this drive replaces edits made here; \
                 `locallibrary edits replay` makes them again.",
                dim(),
            ));
        }
        None if c.undo().is_some() => undo_lines(c, &mut lines),
        None => {
            let plan = &c.plan;
            lines.push(Line::from(vec![
                Span::styled(format!("{}: ", wording.who), bold(theme::PINK)),
                Span::styled(wording.what, text()),
            ]));
            lines.push(Line::raw(""));
            for s in plan.summary() {
                lines.push(Line::from(vec![
                    Span::styled("• ", fg(theme::CYAN)),
                    Span::styled(s.clone(), bold(theme::FG)),
                ]));
            }
            for w in plan.warnings() {
                lines.push(Line::styled(format!("⚠ {w}"), fg(theme::AMBER)));
            }
            if plan.destructive() {
                lines.push(Line::raw(""));
                lines.push(Line::styled("This deletes or replaces what's there now.", fg(theme::AMBER)));
            }
        }
    }
    let w = 76u16.min(area.width.saturating_sub(2));
    let total = wrapped_height(&lines, w.saturating_sub(4)) + 1;
    let h = (total + 2).min(area.height.saturating_sub(2));
    let r = popup(area, w, h, None);
    let visible = h.saturating_sub(2);
    c.scroll = c.scroll.min(total.saturating_sub(visible));
    let more = if total > visible { " ↑↓ scroll ·" } else { "" };
    let b = block(title(&wording.ask, true), true)
        .border_style(fg(theme::AMBER))
        .title_bottom(
            Line::from(vec![
                Span::styled(format!("{more} "), faint()),
                Span::styled("y", theme::key()),
                Span::styled(format!(" {}  ", wording.yes), text()),
                Span::styled("n", theme::key()),
                Span::styled(" cancel ", text()),
            ])
            .right_aligned(),
        )
        .padding(ratatui::widgets::Padding::horizontal(1));
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((c.scroll, 0)).block(b), r);
}

fn draw_ask(f: &mut Frame, question: String, area: Rect) {
    let w = 60u16.min(area.width.saturating_sub(2));
    let lines = vec![Line::styled(question, text())];
    let h = wrapped_height(&lines, w.saturating_sub(4)) + 2;
    let r = popup(area, w, h, None);
    let b = block(title("Sure?", true), true)
        .title_bottom(
            Line::from(vec![
                Span::styled(" y", theme::key()),
                Span::styled(" yes  ", text()),
                Span::styled("n", theme::key()),
                Span::styled(" no ", text()),
            ])
            .right_aligned(),
        )
        .padding(ratatui::widgets::Padding::horizontal(1));
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }).block(b), r);
}

// ---------------------------------------------------------------------------------------------
// Help
// ---------------------------------------------------------------------------------------------

/// The help's sections as lines `width` wide, descriptions wrapped under themselves.
fn help_lines(sections: &[super::keymap::HelpSection], width: u16) -> Vec<Line<'static>> {
    const KEYS: usize = 15;
    let desc_w = (width as usize).saturating_sub(KEYS).max(16);
    let mut lines = Vec::new();
    for (name, keys) in sections {
        lines.push(Line::styled(name.to_uppercase(), bold(theme::CYAN)));
        for (k, d) in keys.iter() {
            for (n, part) in wrap_text(d, desc_w).into_iter().enumerate() {
                let key = if n == 0 { format!("  {k:<13}") } else { " ".repeat(KEYS) };
                let style = if k.is_empty() { dim() } else { text() };
                lines.push(Line::from(vec![Span::styled(key, theme::key()), Span::styled(part, style)]));
            }
        }
        lines.push(Line::raw(""));
    }
    lines
}

fn help_footer(width: u16) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::styled("✓", bold(theme::PINK)),
        Span::styled(" marked  ", dim()),
        Span::styled("✖", bold(theme::RED)),
        Span::styled(" file missing  ", dim()),
        Span::styled("≈", fg(theme::AMBER)),
        Span::styled(" another copy on the drive", dim()),
    ])];
    for text_ in [
        "Mouse: click to select, double-click to play, click a column header to sort, click the waveform to \
         seek, right-click for actions.",
        "Every command is still available on the CLI: locallibrary --help",
    ] {
        lines.extend(wrap_text(text_, width as usize).into_iter().map(|l| Line::styled(l, dim())));
    }
    lines
}

fn draw_help(f: &mut Frame, app: &mut App, area: Rect) {
    // The view's own keys first, then those for everywhere: side by side when there's room,
    // else one column that scrolls.
    let two = area.width >= 120;
    let w = if two { 118.min(area.width) } else { 84.min(area.width) };
    let inner_w = w.saturating_sub(4);
    let col_w = if two { inner_w.saturating_sub(2) / 2 } else { inner_w };
    let (own, everywhere) = super::keymap::help(app.view);
    let columns: Vec<Vec<Line<'static>>> = if two {
        let mut right = help_lines(&everywhere, col_w);
        right.extend(help_footer(col_w));
        vec![help_lines(&own, col_w), right]
    } else {
        let mut all = help_lines(&own, col_w);
        all.extend(help_lines(&everywhere, col_w));
        all.extend(help_footer(col_w));
        vec![all]
    };
    let most = columns.iter().map(Vec::len).max().unwrap_or(0) as u16;
    let r = popup(area, w, most + 2, None);
    let b = block(title(&format!("Keys · {}", app.view.title()), true), true)
        .title_bottom(Line::styled(" ↑↓ scroll · any other key closes ", faint()).right_aligned())
        .padding(ratatui::widgets::Padding::horizontal(1));
    let inner = b.inner(r);
    f.render_widget(Clear, r);
    f.render_widget(b, r);
    app.help_scroll = app.help_scroll.min(most.saturating_sub(inner.height));
    let areas: Vec<Rect> = if two {
        let [a, _, c] =
            Layout::horizontal([Constraint::Length(col_w), Constraint::Length(2), Constraint::Min(0)]).areas(inner);
        vec![a, c]
    } else {
        vec![inner]
    };
    for (lines, a) in columns.into_iter().zip(areas) {
        f.render_widget(Paragraph::new(lines).scroll((app.help_scroll, 0)), a);
    }
}

/// For tests: the screen as text.
#[cfg(test)]
pub(super) fn screen(app: &mut App, w: u16, h: u16) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    let buf = terminal.backend().buffer();
    (0..h).map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>() + "\n").collect()
}

#[cfg(test)]
mod tests;
