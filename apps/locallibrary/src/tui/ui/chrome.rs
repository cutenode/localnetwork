//! What's on every screen besides the header and the footer (see [`super::Header`] and
//! [`super::Footer`]): messages and the log, and the small pieces every view draws with.

use locallibrary::DeviceInfo;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::tui::app::{App, Level, UNDO_HINT, typed_tail};
use crate::tui::edits::Modal;
use crate::tui::icons;
use crate::tui::keymap::{self, Act, Hint, Scope};
use crate::tui::theme::{self, bold, dim, faint, fg};

use super::*;

pub(in crate::tui) fn block(title: impl Into<Line<'static>>, focused: bool) -> Block<'static> {
    Block::bordered().border_type(theme::border_type(focused)).border_style(theme::border(focused)).title(title.into())
}

pub(in crate::tui) fn title(s: &str, focused: bool) -> Line<'static> {
    Line::from(vec![Span::raw(" "), Span::styled(s.to_string(), title_style(focused)), Span::raw(" ")])
}

fn title_style(focused: bool) -> Style {
    if focused { bold(theme::current().info) } else { bold(theme::current().dim) }
}

/// How a pane's title starts: a marker when it has focus, so that reads without colour.
pub(in crate::tui) fn pane_start(focused: bool) -> Vec<Span<'static>> {
    vec![if focused { Span::styled(icons::FOCUS.padded(), title_style(true)) } else { Span::raw(" ") }]
}

/// A pane's title.
pub(in crate::tui) fn pane_title(s: &str, focused: bool) -> Line<'static> {
    let mut spans = pane_start(focused);
    spans.extend([Span::styled(s.to_string(), title_style(focused)), Span::raw(" ")]);
    Line::from(spans)
}

/// A pane with no room for more than its title, one row of the border.
pub(super) fn title_row(f: &mut Frame, title: impl Into<Line<'static>>, focused: bool, area: Rect) {
    let b = Block::new().borders(Borders::TOP).border_type(theme::border_type(focused));
    f.render_widget(b.border_style(theme::border(focused)).title(title.into()), area);
}

/// Hints as a dialog's border writes them: each key in the key colour and what it does after it,
/// ` · ` between; `says` names an act's hint otherwise here (what enter does in this dialog).
pub(in crate::tui) fn hint_bar(hints: &[Hint], says: &[(Act, &str)]) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (i, h) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", faint()));
        }
        let label = says.iter().find(|(act, _)| *act == h.act).map_or(h.label, |(_, s)| s);
        spans.push(Span::styled(h.keys.clone(), theme::key()));
        spans.push(Span::styled(format!(" {label}"), faint()));
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

/// `act`'s key where `scopes` apply, drawn as a key, then `says`; with no key there, what `:`
/// finds it by, or nothing when `:` doesn't list it either.
pub(in crate::tui) fn key_says(scopes: &[Scope], act: Act, says: &str, style: Style) -> Vec<Span<'static>> {
    match keymap::key_name(scopes, act) {
        Some(k) => vec![Span::styled(k, theme::key()), Span::styled(says.to_string(), style)],
        None => keymap::typed_name(act).map_or_else(Vec::new, |n| vec![Span::styled(format!("{n} in :"), style)]),
    }
}

/// With nothing playing: the key that carries on where the last run left off, or the one that
/// plays the track picked, as they are where the player is now.
pub(super) fn idle_line(app: &App) -> Line<'static> {
    let scopes = keymap::scopes(app);
    let Some((t, at)) = app.resumable() else {
        let pick = keymap::key_name(&scopes, Act::Activate)
            .map_or_else(|| "Pick a track.".to_string(), |k| format!("Pick a track and press {k}."));
        return Line::styled(pick, dim());
    };
    let carries = format!(" carries on with {} from {}", t.display_name(), crate::output::clock(at.as_secs_f64()));
    match keymap::key_name(&scopes, Act::PlayPause) {
        Some(k) => Line::from(vec![Span::styled(k, theme::key()), Span::styled(carries, dim())]),
        None => Line::styled(format!("{} in :{carries}", keymap::name(Act::PlayPause)), dim()),
    }
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

/// `s` in lines of at most `width` columns, broken between words, and inside words too long
/// for a line (paths, say).
pub(super) fn wrap_text(s: &str, width: usize) -> Vec<String> {
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

/// A message's text in colour `c`, with the key that takes a change back (where `scopes` apply)
/// drawn as a key in place of [`UNDO_HINT`].
fn undo_spans(text: &str, c: [u8; 3], scopes: &[Scope]) -> Vec<Span<'static>> {
    let Some((said, rest)) = text.split_once(UNDO_HINT) else { return vec![Span::styled(text.to_string(), fg(c))] };
    let mut spans = vec![Span::styled(format!("{said} · "), fg(c))];
    spans.extend(key_says(scopes, Act::Undo, " undo", dim()));
    spans.push(Span::styled(rest.to_string(), fg(c)));
    spans
}

/// A message as lines `width` wide: its icon, then the text wrapped under itself, with the key that
/// takes a change back (where `scopes` apply) drawn as a key.
fn message_lines(msg: &str, level: Level, width: u16, scopes: &[Scope]) -> Vec<Line<'static>> {
    let (icon, c) = level_style(level);
    let mut lines: Vec<Line> = wrap_text(msg, (width as usize).saturating_sub(4))
        .into_iter()
        .enumerate()
        .map(|(i, part)| {
            let head = if i == 0 { Span::styled(format!(" {icon} "), bold(c)) } else { Span::raw("   ") };
            Line::from(vec![head, Span::styled(part, fg(c))])
        })
        .collect();
    if let Some(last) = lines.last_mut() {
        let text = last.spans.pop().map(|s| s.content.into_owned()).unwrap_or_default();
        last.spans.extend(undo_spans(&text, c, scopes));
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
    ) || app.keys.is_some();
    let width = area.width;
    let scopes = keymap::scopes(app);
    let mut lines: Vec<Line> = Vec::new();
    if let Some(o) = app.replay_offer.as_ref().filter(|_| !dialog) {
        let edits = if o.edits == 1 { "1 edit".to_string() } else { format!("{} edits", o.edits) };
        let mut spans = vec![
            Span::styled(icons::DIAMOND.padded(), bold(theme::current().warning)),
            Span::styled(
                format!("rekordbox exported over {edits} on {}. Replay? ", o.drive),
                fg(theme::current().warning),
            ),
        ];
        spans.extend(key_says(&scopes, Act::Replay, " replay", dim()));
        spans.push(Span::styled(" · ", dim()));
        spans.extend(key_says(&scopes, Act::Escape, " not now", dim()));
        lines.push(Line::from(fit(spans, width as usize)));
    }
    if let Some(m) = app.held.as_ref().filter(|_| !dialog) {
        lines.extend(message_lines(m, Level::Error, width, &scopes));
    }
    if let Some((m, level, _)) = &app.toast {
        if dialog {
            let (icon, c) = level_style(*level);
            let mut spans = vec![Span::styled(format!(" {icon} "), bold(c))];
            spans.extend(undo_spans(m, c, &scopes));
            lines.push(Line::from(fit(spans, room.width as usize)));
        } else {
            lines.extend(message_lines(m, *level, width, &scopes));
        }
    }
    if lines.is_empty() {
        return;
    }
    // Errors stay until esc: say so where there's room.
    let error = app.held.is_some() || app.toast.as_ref().is_some_and(|t| t.1 == Level::Error);
    let hides = keymap::key_name(&scopes, Act::Escape).map(|k| format!("  {k} hides"));
    if let Some(hides) =
        hides.filter(|h| error && lines.last().is_some_and(|l| l.width() + h.width() + 2 <= width as usize))
        && let Some(last) = lines.last_mut()
    {
        last.spans.push(Span::styled(hides, faint()));
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

/// The messages (in `:`): every message since starting, newest first.
pub(super) fn draw_log(f: &mut Frame, app: &mut App, area: Rect) {
    let w = 100.min(area.width);
    let inner_w = w.saturating_sub(4);
    let mut lines: Vec<Line> = Vec::new();
    let scopes = keymap::scopes(app);
    for (msg, level, at) in app.log.iter().rev() {
        let mut msg_lines = message_lines(msg, *level, inner_w.saturating_sub(12), &scopes);
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
        .title_bottom(hint_bar(&keymap::dialog_hints(&keymap::dialogs::MESSAGES), &[]).right_aligned())
        .padding(ratatui::widgets::Padding::horizontal(1));
    let inner = b.inner(r);
    app.log_scroll = app.log_scroll.min(total.saturating_sub(inner.height));
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines).scroll((app.log_scroll, 0)).block(b), r);
}
