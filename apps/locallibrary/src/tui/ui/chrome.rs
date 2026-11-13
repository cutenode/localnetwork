//! What's on every screen: the header, the footer, toasts and the log, and the small pieces every
//! view draws with.

use locallibrary::DeviceInfo;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::output::bytes;
use crate::tui::app::{App, Focus, Level, UNDO_HINT, View, typed_tail};
use crate::tui::edits::Modal;
use crate::tui::first_run::intro_line;
use crate::tui::theme::{self, bold, dim, faint, fg, text};
use crate::tui::{icons, keymap};

use super::*;

const NAME: &str = "locallibrary";

pub(in crate::tui) fn block(title: impl Into<Line<'static>>, focused: bool) -> Block<'static> {
    Block::bordered().border_type(theme::border_type(focused)).border_style(theme::border(focused)).title(title.into())
}

pub(in crate::tui) fn title(s: &str, focused: bool) -> Line<'static> {
    Line::from(vec![Span::raw(" "), Span::styled(s.to_string(), title_style(focused)), Span::raw(" ")])
}

fn title_style(focused: bool) -> Style {
    if focused { bold(theme::current().info) } else { bold(theme::current().dim) }
}

/// How a pane's title starts: a marker when it has focus, so that reads without colour, and the
/// number that jumps to it.
pub(in crate::tui) fn pane_start(number: u8, focused: bool) -> Vec<Span<'static>> {
    let mark = if focused { Span::styled(icons::FOCUS.padded(), title_style(true)) } else { Span::raw(" ") };
    vec![mark, Span::styled(format!("{number} "), if focused { theme::key() } else { faint() })]
}

/// A pane's title.
pub(in crate::tui) fn pane_title(number: u8, s: &str, focused: bool) -> Line<'static> {
    let mut spans = pane_start(number, focused);
    spans.extend([Span::styled(s.to_string(), title_style(focused)), Span::raw(" ")]);
    Line::from(spans)
}

/// A pane with no room for more than its title, one row of the border.
pub(super) fn title_row(f: &mut Frame, title: impl Into<Line<'static>>, focused: bool, area: Rect) {
    let b = Block::new().borders(Borders::TOP).border_type(theme::border_type(focused));
    f.render_widget(b.border_style(theme::border(focused)).title(title.into()), area);
}

/// `left` and `right` on one line, `right` flush right when it fits.
pub(in crate::tui) fn spread<'a>(mut left: Vec<Span<'a>>, right: Vec<Span<'a>>, width: u16) -> Line<'a> {
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
pub(in crate::tui) fn clip(s: &str, max: usize) -> String {
    crate::output::clip(s, max, "…")
}

/// Keep spans from the front while they fit in `max` columns, cutting the last one short.
pub(super) fn fit(spans: Vec<Span<'static>>, max: usize) -> Vec<Span<'static>> {
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

pub(super) fn drive_name(d: &DeviceInfo) -> String {
    d.name.clone().unwrap_or_else(|| d.volume_name.clone())
}

pub(super) fn compact(n: usize) -> String {
    if n >= 10_000 {
        format!("{}k", n / 1000)
    } else if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

pub(super) fn spinner(app: &App) -> &'static str {
    let frames = icons::spinner();
    frames[(app.started.elapsed().as_secs_f32() * 12.0) as usize % frames.len()]
}

/// What the screen's about, as far as a line can say: the drive, the list shown, the search and
/// how much is marked. Nothing until a drive is open.
fn context(app: &App) -> Vec<Span<'static>> {
    let Some(l) = &app.loaded else { return Vec::new() };
    let mut spans = vec![
        Span::styled(app.drive_label(l.root()), bold(theme::current().text)),
        Span::styled(" › ", faint()),
        Span::styled(l.playlist_name(), dim()),
    ];
    if !app.query.is_empty() {
        spans.extend([Span::styled("  / ", theme::key()), Span::styled(clip(&app.query, 28), text())]);
        if app.search_all {
            spans.push(Span::styled(" whole drive", fg(theme::current().info)));
        }
    }
    let marked = app.marks.len() + app.list_marks.len();
    if marked > 0 {
        spans.push(Span::styled(format!("  {}{marked} marked", icons::CHECK.padded()), bold(theme::current().accent)));
    }
    spans
}

pub(super) fn draw_context(f: &mut Frame, app: &App, area: Rect) {
    if !area.is_empty() {
        let mut spans = vec![Span::raw(" ")];
        spans.extend(fit(context(app), (area.width as usize).saturating_sub(1)));
        f.render_widget(Line::from(spans), area);
    }
}

/// The logo, a tab for each view and, when it has no row of its own (`carries_context`), the
/// context on the right. The player's name goes first as the width runs out.
pub(super) fn draw_header(f: &mut Frame, app: &mut App, area: Rect, carries_context: bool) {
    // The logo dot pulses with the music.
    let pulse = theme::mix(theme::current().accent, theme::current().warning, app.analyzer.level);
    let pulsing = app.is_playing() && !theme::reduced_motion();
    let logo_style = bold(if pulsing { pulse } else { theme::current().accent });
    let about = if carries_context { context(app) } else { Vec::new() };
    let about_w: usize = about.iter().map(Span::width).sum();
    let tabs: Vec<(View, String)> =
        View::ALL.into_iter().map(|v| (v, format!(" {} {} ", keymap::view_letter(v), v.title()))).collect();
    let tabs_w: usize = tabs.iter().map(|(_, l)| l.len()).sum();
    let mut spans = vec![Span::styled(icons::LOGO.padded(), logo_style)];
    if area.width as usize >= tabs_w + NAME.len() + 6 + about_w {
        spans.extend([Span::styled(NAME, bold(theme::current().text)), Span::raw("   ")]);
    }
    let mut x = area.x + spans.iter().map(|s| s.width() as u16).sum::<u16>();
    for (v, label) in tabs {
        let w = label.chars().count() as u16;
        let style = if app.view == v { theme::chip(theme::current().info) } else { dim() };
        app.areas.tabs.push((v, Rect::new(x, area.y, w, 1)));
        spans.push(Span::styled(label, style));
        x += w;
    }
    let room = (area.width as usize).saturating_sub(spans.iter().map(Span::width).sum::<usize>() + 1);
    f.render_widget(spread(spans, fit(about, room), area.width), area);
}

/// What the player's doing: the job's progress first, cut short if need be, else the first of what
/// the job list (`&`) holds, so the two can't say different things.
fn busy(app: &App) -> Vec<Span<'static>> {
    let mut busy = Vec::new();
    if let Some(j) = &app.job {
        busy.push(Span::styled(format!("{} ", spinner(app)), fg(theme::current().warning)));
        busy.push(Span::styled(format!("{} ", j.label), bold(theme::current().warning)));
        let secs = j.started.elapsed().as_secs();
        if secs > 0 {
            busy.push(Span::styled(format!("{secs}s "), faint()));
        }
        if j.committed {
            busy.push(Span::styled("can't stop now ", fg(theme::current().warning)));
        }
        if !j.detail.is_empty() {
            busy.push(Span::styled(format!("{} ", j.detail), dim()));
        }
        if !j.committed {
            busy.push(Span::styled(" esc ", theme::key()));
            busy.push(Span::styled("cancel ", dim()));
        }
        busy.push(Span::styled(" & ", theme::key()));
        busy.push(Span::styled("jobs ", dim()));
    } else if let Some(row) = crate::tui::jobs::rows(app).first() {
        busy.push(Span::styled(format!("{} ", spinner(app)), fg(theme::current().info)));
        busy.push(Span::styled(format!("{} ", row.label), dim()));
    }
    busy
}

/// What keys do now, for the status line.
fn mode(app: &App) -> &'static str {
    let typing = matches!(app.modal, Some(Modal::Prompt(_))) || app.mix.input.is_some() || app.profile.account.typing();
    if app.chord.is_some() {
        "GO TO"
    } else if app.searching {
        "SEARCH"
    } else if typing {
        "TYPING"
    } else if app.picking.is_some() {
        "PICK"
    } else if app.deck_showing() {
        "DECK"
    } else if app.deck.pads {
        "PADS"
    } else {
        "BROWSE"
    }
}

/// Where the selection is in its list, the space left on the drive, and who's signed in. What's
/// about the drive comes from the last read of the drive list, so a frame touches no disk.
fn whereabouts(app: &mut App) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let (selected, len) = match (app.view, app.focus) {
        (View::Library, Focus::Drives) if !app.deck_showing() => (app.source_row(), app.source_rows()),
        _ => {
            let (selected, len) = app.list_selection();
            (*selected, len)
        }
    };
    if let Some(at) = selected.filter(|_| len > 0) {
        spans.push(Span::styled(format!("{}/{len}", at.min(len - 1) + 1), dim()));
    }
    let root = app.action_drive();
    let free = app.drives.iter().find(|d| Some(&d.root) == root.as_ref()).and_then(|d| d.free_space);
    if let Some(free) = free {
        spans.push(Span::styled(format!("  {} free", bytes(free)), dim()));
    }
    if let Some(handle) = app.profile.account.handle() {
        spans.push(Span::styled(format!("  @{handle}"), fg(theme::current().info)));
    }
    spans.push(Span::raw(" "));
    spans
}

/// The status line: what keys do on the left, where things are on the right and between them
/// what's under way. Returns where that leaves for the latest message.
pub(super) fn draw_status(f: &mut Frame, app: &mut App, area: Rect) -> Rect {
    if area.is_empty() {
        return area;
    }
    let width = area.width as usize;
    let mode = Span::styled(format!(" {} ", mode(app)), theme::chip(theme::current().dim));
    let mode_w = mode.width() + 1;
    // The right side gives way first, from its end.
    let mut right = whereabouts(app);
    while right.len() > 1 && right.iter().map(Span::width).sum::<usize>() * 2 > width.saturating_sub(mode_w) {
        right.remove(right.len() - 2);
    }
    let right_w: usize = right.iter().map(Span::width).sum();
    let room = width.saturating_sub(mode_w + right_w + 1);
    let doing = fit(busy(app), room);
    let doing_w: u16 = doing.iter().map(|s| s.width() as u16).sum();
    let mut left = vec![mode, Span::raw(" ")];
    left.extend(doing);
    f.render_widget(spread(left, right, area.width), area);
    let x = area.x + mode_w as u16 + doing_w;
    Rect { x, width: (room as u16).saturating_sub(doing_w), ..area }
}

/// A line being typed: the search box, or a prompt for a name.
pub(super) fn input_line(
    label: Vec<Span<'static>>,
    text_: &str,
    after: Vec<Span<'static>>,
    width: u16,
) -> Line<'static> {
    // A label naming a long playlist mustn't push what's typed off the line.
    let mut spans = fit(label, width as usize * 2 / 3);
    let room = (width as usize).saturating_sub(spans.iter().map(Span::width).sum::<usize>() + 1);
    spans.push(Span::styled(typed_tail(text_, room).to_string(), bold(theme::current().text)));
    spans.push(Span::styled(icons::CURSOR.get(), fg(theme::current().accent)));
    let used: usize = spans.iter().map(Span::width).sum();
    spans.extend(fit(after, (width as usize).saturating_sub(used)));
    Line::from(spans)
}

pub(super) fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
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
            Span::styled(
                if app.search_all { "  in the whole drive" } else { "  in this list" },
                fg(theme::current().info),
            ),
        ];
        match (&app.filter_error, &app.filter_note) {
            (Some(e), _) => after.push(Span::styled(format!("   {e}"), fg(theme::current().warning))),
            (None, Some(n)) => after.push(Span::styled(format!("   {n}"), dim())),
            (None, None) => after.push(Span::styled("   tab whole drive · enter keep · esc clear", faint())),
        }
        let label = vec![Span::styled(" / ", theme::key())];
        f.render_widget(Paragraph::new(input_line(label, &app.query, after, area.width)), area);
        return;
    }
    if let Some(p) = &app.picking {
        let spans = vec![
            Span::styled(icons::DIAMOND.padded(), bold(theme::current().warning)),
            Span::styled(format!("Add {} to which playlist? ", p.target.what), bold(theme::current().text)),
            Span::styled("  enter choose · esc cancel", faint()),
        ];
        f.render_widget(Paragraph::new(Line::from(fit(spans, area.width as usize))), area);
        return;
    }
    if app.intro {
        return f.render_widget(Line::from(fit(intro_line(), area.width as usize)), area);
    }
    // The keymap's hints for where focus is, as many as fit.
    let mut spans = vec![Span::raw(" ")];
    for (k, label) in crate::tui::keymap::hints(app, area.width as usize) {
        spans.push(Span::styled(k, theme::key()));
        spans.push(Span::styled(format!(" {label}  "), dim()));
    }
    f.render_widget(Line::from(spans), area);
}

/// `s` in lines of at most `width` columns, broken between words, and inside words too long
/// for a line (paths, say).
pub(super) fn wrap_text(s: &str, width: usize) -> Vec<String> {
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
        Level::Info => (icons::DOT.get(), theme::current().info),
        Level::Error => (icons::CROSS.get(), theme::current().critical),
    }
}

/// A message as lines `width` wide: its icon, then the text wrapped under itself, with the key that
/// takes a change back drawn as a key.
fn message_lines(msg: &str, level: Level, width: u16) -> Vec<Line<'static>> {
    let (icon, c) = level_style(level);
    let mut lines: Vec<Line> = wrap_text(msg, (width as usize).saturating_sub(4))
        .into_iter()
        .enumerate()
        .map(|(i, part)| {
            let head = if i == 0 { Span::styled(format!(" {icon} "), bold(c)) } else { Span::raw("   ") };
            Line::from(vec![head, Span::styled(part, fg(c))])
        })
        .collect();
    if let Some(last) = lines.last_mut()
        && let Some((said, rest)) = last.spans[1].content.clone().split_once(UNDO_HINT)
    {
        last.spans.truncate(1);
        last.spans.extend([
            Span::styled(format!("{said} · "), fg(c)),
            Span::styled("u", theme::key()),
            Span::styled(" undo", dim()),
            Span::styled(rest.to_string(), fg(c)),
        ]);
    }
    lines
}

/// The messages: an error that's waiting to be dismissed above the latest message. One line that
/// fits goes on the status line, in `room`. More are wrapped to the width and end just above the
/// deck. With a dialog open, only the latest shows, in `room`, so the dialog isn't covered.
pub(super) fn draw_toasts(f: &mut Frame, app: &App, area: Rect, room: Rect) {
    let dialog = matches!(
        app.modal,
        Some(
            Modal::Menu(_)
                | Modal::Confirm(_)
                | Modal::Ask(..)
                | Modal::Pick(_)
                | Modal::Report(_)
                | Modal::Form(_)
                | Modal::Jobs(_)
        )
    );
    let width = area.width;
    let mut lines: Vec<Line> = Vec::new();
    if let Some(o) = app.replay_offer.as_ref().filter(|_| !dialog) {
        let edits = if o.edits == 1 { "1 edit".to_string() } else { format!("{} edits", o.edits) };
        let spans = vec![
            Span::styled(icons::DIAMOND.padded(), bold(theme::current().warning)),
            Span::styled(
                format!("rekordbox exported over {edits} on {}. Replay? ", o.drive),
                fg(theme::current().warning),
            ),
            Span::styled("y", theme::key()),
            Span::styled(" replay · ", dim()),
            Span::styled("esc", theme::key()),
            Span::styled(" not now", dim()),
        ];
        lines.push(Line::from(fit(spans, width as usize)));
    }
    if let Some(m) = app.held.as_ref().filter(|_| !dialog) {
        lines.extend(message_lines(m, Level::Error, width));
    }
    if let Some((m, level, _)) = &app.toast {
        if dialog {
            let (icon, c) = level_style(*level);
            let spans = vec![Span::styled(format!(" {icon} "), bold(c)), Span::styled(m.clone(), fg(c))];
            lines.push(Line::from(fit(spans, room.width as usize)));
        } else {
            lines.extend(message_lines(m, *level, width));
        }
    }
    if lines.is_empty() {
        return;
    }
    // Errors stay until esc: say so where there's room.
    let error = app.held.is_some() || app.toast.as_ref().is_some_and(|t| t.1 == Level::Error);
    if let Some(last) = lines.last_mut().filter(|l| error && l.width() + 13 <= width as usize) {
        last.spans.push(Span::styled("  esc hides", faint()));
    }
    if let [line] = &lines[..]
        && line.width() <= room.width as usize
    {
        f.render_widget(Paragraph::new(line.clone()), room);
        return;
    }
    // At most a third of the screen; the newest lines when that's not enough.
    let bottom = area.bottom();
    let rows = (bottom.saturating_sub(area.y + 1)).min((bottom / 3).max(1)) as usize;
    let skip = lines.len().saturating_sub(rows);
    let lines: Vec<Line> = lines.into_iter().skip(skip).collect();
    let h = lines.len() as u16;
    let r = Rect { x: area.x, y: bottom.saturating_sub(h), width, height: h };
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines), r);
}

/// How long ago, roughly.
pub(super) fn ago(d: std::time::Duration) -> String {
    match d.as_secs() {
        s if s < 5 => "just now".into(),
        s if s < 60 => format!("{s} s ago"),
        s if s < 3600 => format!("{} min ago", s / 60),
        s => format!("{} h ago", s / 3600),
    }
}

/// `!`: every message since starting, newest first.
pub(super) fn draw_log(f: &mut Frame, app: &mut App, area: Rect) {
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
