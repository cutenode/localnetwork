//! What's drawn over a view: the menu, the picker, reports, forms, the outputs list, confirmations,
//! questions and the help.

use locallibrary::local_time;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::output::{bytes, plural};
use crate::tui::app::{App, typed_tail};
use crate::tui::browse::{COLOURS, colour_name, stars};
use crate::tui::edits::{Menu, Modal, Sub};
use crate::tui::icons;
use crate::tui::theme::{self, bold, dim, faint, fg, text};

use super::*;

/// A box of `w` x `h` in the middle of `area`, or by the pointer when opened with the mouse.
pub(super) fn popup(area: Rect, w: u16, h: u16, at: Option<(u16, u16)>) -> Rect {
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

pub(super) fn draw_menu(f: &mut Frame, app: &mut App, area: Rect) {
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
                    crate::tui::edits::Action::Rate(n) => ListItem::new(Line::from(vec![
                        Span::styled(format!("{n:<5}"), theme::key()),
                        Span::styled(if n == 0 { "no rating".into() } else { stars(n) }, fg(theme::current().warning)),
                    ])),
                    crate::tui::edits::Action::Colour(c) => {
                        let n = c.map_or(0, |c| COLOURS.iter().position(|x| *x == c).unwrap_or(0) + 1);
                        ListItem::new(Line::from(vec![
                            Span::styled(format!("{n:<5}"), theme::key()),
                            match c {
                                Some(c) => Span::styled(icons::DOT.spaced(), fg(theme::current().track_color(c))),
                                None => Span::styled(icons::RING.spaced(), faint()),
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
        .highlight_symbol(Span::styled(icons::SELECTED.get(), fg(theme::current().accent)));
    f.render_widget(Clear, r);
    f.render_stateful_widget(list, r, state);
    app.areas.menu = r;
}

/// A list to choose from (the command palette, a drive, a snapshot): what's typed on top, the
/// matching items under it, each with its detail on the right.
pub(super) fn draw_picker(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(Modal::Pick(p)) = &mut app.modal else { return };
    let w = 72u16.min(area.width.saturating_sub(2));
    let rows = (p.shown.len().max(1) as u16).min(area.height.saturating_sub(6).clamp(1, 16));
    let h = rows + 3;
    // Near the top, so the list grows down from where the eye is.
    let r = Rect { y: area.y + (area.height.saturating_sub(h) / 4), ..popup(area, w, h, None) };
    let hint = match p.marks {
        Some(_) => " ↑↓ choose · space mark · enter takes the marked · esc close ",
        None => " ↑↓ choose · enter · esc close ",
    };
    let b = block(title(&p.title, true), true).title_bottom(Line::styled(hint, faint()).right_aligned());
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
        let items: Vec<ListItem> = (p.shown.iter().map(|&i| (i, &p.items[i])))
            .map(|(i, it)| {
                let style = if it.unavailable.is_some() { faint() } else { text() };
                let right = vec![Span::styled(format!("{} ", it.detail), theme::key())];
                let rw: usize = right.iter().map(Span::width).sum();
                // Marked as the tracks are, in a list any of which may be chosen.
                let mark = match p.marks.as_ref().map(|m| m[i]) {
                    Some(true) => Some(Span::styled(icons::CHECK.spaced(), bold(theme::current().accent))),
                    Some(false) => Some(Span::raw("  ")),
                    None => None,
                };
                let room = (w as usize).saturating_sub(rw + 1 + mark.as_ref().map_or(0, Span::width));
                let left: Vec<Span> = mark.into_iter().chain([Span::styled(clip(&it.label, room), style)]).collect();
                ListItem::new(spread(left, right, w))
            })
            .collect();
        let l = List::new(items)
            .highlight_style(theme::selected())
            .highlight_symbol(Span::styled(icons::SELECTED.get(), fg(theme::current().accent)));
        p.state.fit(list.height as usize, n);
        f.render_stateful_widget(l, list, &mut p.state);
    }
    app.areas.menu = r;
}

/// How a line of results is drawn: the mark before it, and its style.
pub(in crate::tui) fn tone_style(tone: crate::tui::drives::Tone) -> (&'static str, Style) {
    use crate::tui::drives::Tone;
    match tone {
        Tone::Heading => ("", bold(theme::current().info)),
        Tone::Ok => (icons::CHECK.spaced(), fg(theme::current().good)),
        Tone::Warning => (icons::WARNING.spaced(), fg(theme::current().warning)),
        Tone::Problem => (icons::CROSS.spaced(), fg(theme::current().critical)),
        Tone::Text | Tone::Row => ("", text()),
        Tone::Faint => ("", faint()),
        Tone::Columns => ("", dim()),
    }
}

/// Results to read (a check's, a plugin's command's): a panel that scrolls, closed by any other
/// key.
pub(super) fn draw_report(f: &mut Frame, app: &mut App, area: Rect) {
    use crate::tui::drives::Tone;
    let Some(Modal::Report(r)) = &mut app.modal else { return };
    let w = 100.min(area.width);
    let inner_w = w.saturating_sub(4) as usize;
    let mut lines: Vec<Line> = Vec::new();
    for (tone, text_) in &r.lines {
        let (mark, style) = tone_style(*tone);
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
pub(super) fn draw_form(f: &mut Frame, app: &mut App, area: Rect) {
    use unicode_width::UnicodeWidthStr;
    let Some(Modal::Form(form)) = &app.modal else { return };
    let w = 72u16.min(area.width.saturating_sub(2));
    let inner_w = w.saturating_sub(4) as usize;
    let label_w = form.fields.iter().map(|x| x.label().width()).max().unwrap_or(0).min(inner_w / 3);
    let mut lines: Vec<Line> = Vec::new();
    for (i, field) in form.fields.iter().enumerate() {
        let on = i == form.selected;
        let mut spans = vec![
            Span::styled(if on { icons::SELECTED.get() } else { " " }, fg(theme::current().accent)),
            Span::styled(
                format!("{:<label_w$}  ", clip(&field.label(), label_w)),
                if on { bold(theme::current().text) } else { dim() },
            ),
        ];
        let room = inner_w.saturating_sub(label_w + 3);
        // A secret as dots, one for each character typed.
        let shown = |typed: &str| match field.is_secret() {
            true => icons::BULLET.get().repeat(typed.chars().count()),
            false => typed.to_string(),
        };
        match field.text.as_str() {
            "true" if field.is_switch() => {
                spans.push(Span::styled(format!("{} yes", icons::DOT.get()), fg(theme::current().accent)))
            }
            _ if field.is_switch() => spans.push(Span::styled(format!("{} no", icons::RING.get()), faint())),
            picked if !field.choices().is_empty() => {
                spans.push(Span::styled("‹ ", faint()));
                spans.push(Span::styled(if picked.is_empty() { "none" } else { picked }.to_string(), text()));
                spans.push(Span::styled(" ›", faint()));
            }
            typed if on => {
                spans.push(Span::styled(
                    typed_tail(&shown(typed), room.saturating_sub(1)).to_string(),
                    bold(theme::current().text),
                ));
                spans.push(Span::styled(icons::CURSOR.get(), fg(theme::current().accent)));
            }
            typed => spans.push(Span::styled(clip(&shown(typed), room), text())),
        }
        if field.required && field.text.is_empty() {
            spans.push(Span::styled(if on { " required" } else { "required" }, faint()));
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
        lines.extend(
            wrap_text(&format!("{} {e}", icons::WARNING.get()), inner_w)
                .into_iter()
                .map(|l| Line::styled(l, fg(theme::current().warning))),
        );
    }
    let r = popup(area, w, lines.len() as u16 + 2, None);
    let keys = if selected.choices().is_empty() { "" } else { "←→ change · " };
    let then = match form.then {
        crate::tui::form::For::Run(_)
        | crate::tui::form::For::Export { .. }
        | crate::tui::form::For::Import { .. }
        | crate::tui::form::For::Source { .. }
        | crate::tui::form::For::ApplyFields { .. }
        | crate::tui::form::For::Fetch { .. } => "run",
        crate::tui::form::For::Settings(_)
        | crate::tui::form::For::ProviderSettings
        | crate::tui::form::For::CheckSettings
        | crate::tui::form::For::Access(..) => "save",
        crate::tui::form::For::AskAccess(_) => "answer",
        crate::tui::form::For::Keys(..) => "sign in",
    };
    let b = block(title(&form.title, true), true)
        .title_bottom(Line::styled(format!(" ↑↓ field · {keys}enter {then} · esc cancel "), faint()).right_aligned())
        .padding(ratatui::widgets::Padding::horizontal(1));
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines).block(b), r);
    app.areas.menu = r;
}

/// The outputs to play through (`d`), the system's default first.
pub(super) fn draw_outputs(f: &mut Frame, app: &mut App, area: Rect) {
    let playing = app.audio.as_ref().map(|a| a.device().map(str::to_string));
    let Some(o) = &mut app.deck.outputs else { return };
    let w = 56u16.min(area.width);
    let mark =
        |on: bool| if on { Span::styled(icons::DOT.spaced(), bold(theme::current().accent)) } else { Span::raw("  ") };
    let default = match &o.default {
        Some(d) => format!("the system's default ({d})"),
        None => "the system's default".into(),
    };
    let mut items = vec![ListItem::new(Line::from(vec![mark(playing == Some(None)), Span::styled(default, text())]))];
    // Until the devices are in, a row where they go says so (or why they can't be listed).
    let why = match (&o.listing, &o.failed) {
        (Some(_), _) => Some(("listing the outputs…".to_string(), dim())),
        (_, Some(e)) => Some((format!("can't list the outputs: {e}"), fg(theme::current().critical))),
        _ => None,
    };
    if let Some((why, style)) = why {
        let why = Span::styled(clip(&why, (w as usize).saturating_sub(6)), style);
        items.push(ListItem::new(Line::from(vec![Span::raw("  "), why])));
    }
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
        .highlight_symbol(Span::styled(icons::SELECTED.get(), fg(theme::current().accent)));
    f.render_widget(Clear, r);
    f.render_stateful_widget(list, r, &mut o.state);
}

/// A playlist and track edit's changes, in detail.
fn edit_lines(p: &locallibrary::edit::EditPlan, lines: &mut Vec<Line<'static>>) {
    for change in &p.changes {
        lines.push(Line::from(vec![
            Span::styled(icons::BULLET.spaced(), fg(theme::current().info)),
            Span::styled(change.clone(), bold(theme::current().text)),
        ]));
    }
    for path in &p.playlists_created {
        lines.push(Line::from(vec![
            Span::styled("  + ", bold(theme::current().good)),
            Span::styled(path.clone(), text()),
        ]));
    }
    for path in &p.playlists_deleted {
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", icons::MINUS.get()), bold(theme::current().critical)),
            Span::styled(path.clone(), text()),
        ]));
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
                parts.push(format!("{}{}", icons::MINUS.get(), ch.removed));
            }
            if parts.is_empty() { "entries changed".into() } else { parts.join(" ") }
        };
        lines.push(Line::from(vec![
            Span::styled("  ~ ", bold(theme::current().warning)),
            Span::styled(ch.path.clone(), text()),
            Span::styled(format!("  {what}, {} → {} tracks", ch.before, ch.after), dim()),
        ]));
    }
    if p.tracks_changed > 0 {
        lines.push(Line::from(vec![
            Span::styled("  ~ ", bold(theme::current().warning)),
            Span::styled(format!("{} changed", plural(p.tracks_changed, "track")), text()),
        ]));
    }
    if !p.tracks_removed.is_empty() {
        let files =
            if p.files.is_empty() { String::new() } else { format!(", {} files, {}", p.files.len(), bytes(p.bytes)) };
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", icons::MINUS.get()), bold(theme::current().critical)),
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
            Span::styled(format!("  {} ", icons::MINUS.get()), bold(theme::current().critical)),
            Span::styled(format!("{} removed", plural(p.history_removed, "history list")), text()),
        ]));
    }
    for w in &p.warnings {
        lines.push(Line::styled(format!("{} {w}", icons::WARNING.get()), fg(theme::current().warning)));
    }
}

/// Undoing, in detail: the backup, the files, how playlists and tracks change, and the
/// recorded edits that come off the journal.
fn undo_lines(c: &crate::tui::edits::Confirm, lines: &mut Vec<Line<'static>>) {
    let Some(u) = c.undo() else { return };
    let b = &u.backup;
    lines.push(Line::from(vec![
        Span::styled(format!("{}: ", c.drive), bold(theme::current().accent)),
        Span::styled(format!("put back the backup ({})", b.reason), text()),
    ]));
    lines.push(Line::styled(format!("made {}  ·  {}", local_time(b.created, "%Y-%m-%d %H:%M"), b.id), faint()));
    lines.push(Line::raw(""));
    let d = &u.diff;
    for p in &d.added_playlists {
        lines
            .push(Line::from(vec![Span::styled("  + ", bold(theme::current().good)), Span::styled(p.clone(), text())]));
    }
    for p in &d.removed_playlists {
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", icons::MINUS.get()), bold(theme::current().critical)),
            Span::styled(p.clone(), text()),
        ]));
    }
    for p in &d.changed_playlists {
        let mut parts = Vec::new();
        if !p.added.is_empty() {
            parts.push(format!("+{}", p.added.len()));
        }
        if !p.removed.is_empty() {
            parts.push(format!("{}{}", icons::MINUS.get(), p.removed.len()));
        }
        if !p.moved.is_empty() {
            parts.push(format!("{} moved", p.moved.len()));
        }
        lines.push(Line::from(vec![
            Span::styled("  ~ ", bold(theme::current().warning)),
            Span::styled(p.path.clone(), text()),
            Span::styled(format!("  {}", parts.join(" ")), dim()),
        ]));
    }
    let tracks = [
        (d.added_tracks.len(), "back", theme::current().good, "+"),
        (d.removed_tracks.len(), "gone", theme::current().critical, icons::MINUS.get()),
        (d.changed_tracks.len(), "as they were", theme::current().warning, "~"),
    ];
    for (n, what, c, mark) in tracks.into_iter().filter(|t| t.0 > 0) {
        lines.push(Line::from(vec![
            Span::styled(format!("  {mark} "), bold(c)),
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
        lines.push(Line::styled(format!("{} {w}", icons::WARNING.get()), fg(theme::current().warning)));
    }
}

/// A plugin's export, as `export --dry-run` shows it: what it writes and what goes in, its first
/// files (what each holds, and those there already, by their place in the output's folder), what
/// the format can't hold, then problems with the library.
fn export_lines(c: &crate::tui::edits::Confirm, lines: &mut Vec<Line<'static>>) {
    use crate::export_cmd::{FILES_SHOWN, about};
    let Some(p) = c.export() else { return };
    // The summary says where (`write 2 file(s) to ...`).
    for s in &p.summary {
        lines.push(Line::from(vec![
            Span::styled(icons::BULLET.spaced(), fg(theme::current().info)),
            Span::styled(s.clone(), bold(theme::current().text)),
        ]));
    }
    let dir = p.plan.output.parent().unwrap_or(&p.plan.output);
    for f in p.plan.files.iter().take(FILES_SHOWN) {
        let path = f.file.path.strip_prefix(dir).unwrap_or(&f.file.path).display().to_string();
        let mut spans = vec![Span::styled(format!("  {path}  "), text()), Span::styled(about(f), dim())];
        if f.exists {
            spans.push(Span::styled(" (already there)", fg(theme::current().warning)));
        }
        lines.push(Line::from(spans));
    }
    if p.plan.files.len() > FILES_SHOWN {
        lines.push(Line::styled(format!("  and {} more", p.plan.files.len() - FILES_SHOWN), dim()));
    }
    for n in &p.plan.notes {
        lines.push(Line::styled(n.clone(), dim()));
    }
    for w in &p.warnings {
        lines.push(Line::styled(format!("{} {w}", icons::WARNING.get()), fg(theme::current().warning)));
    }
    if p.destructive {
        lines.push(Line::raw(""));
        lines.push(Line::styled("This replaces files already there.", fg(theme::current().warning)));
    }
}

pub(super) fn draw_confirm(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(Modal::Confirm(c)) = &mut app.modal else { return };
    let wording = crate::tui::edits::wording(&*c.plan, &c.drive);
    let mut lines = Vec::new();
    // A guard made blocking found something: applying it is refused.
    let blocked = c.plan.blocked().map(|check| {
        Line::styled(format!("{} {check} stops these changes", icons::REFUSED.get()), fg(theme::current().critical))
    });
    match c.edit() {
        Some(p) => {
            lines.push(Line::from(vec![
                Span::styled(format!("{}: ", c.drive), bold(theme::current().accent)),
                Span::styled(p.databases.join(" and "), text()),
            ]));
            // A plugin's operation: what the plugin says it does first.
            let by = crate::tui::plugins::planned_by(c.plan.operation(), false);
            if let Some((by, said)) = by.zip(c.plan.summary().first()) {
                lines.push(Line::from(vec![
                    Span::styled(format!("{by}: "), bold(theme::current().info)),
                    Span::styled(said.clone(), text()),
                ]));
            }
            lines.push(Line::raw(""));
            edit_lines(p, &mut lines);
            // What's said about the plan as a whole (a plugin's warnings), besides its edits'.
            for w in c.plan.warnings().iter().filter(|w| !p.warnings.contains(w)) {
                lines.push(Line::styled(format!("{} {w}", icons::WARNING.get()), fg(theme::current().warning)));
            }
            lines.extend(blocked);
            lines.push(Line::raw(""));
            lines.push(Line::styled("The drive is backed up first: u takes this back.", dim()));
            lines.push(Line::styled(
                "rekordbox's next export to this drive replaces edits made here; \
                 `locallibrary edits replay` makes them again.",
                dim(),
            ));
        }
        None if c.undo().is_some() => undo_lines(c, &mut lines),
        None if c.export().is_some() => export_lines(c, &mut lines),
        None => {
            let plan = &c.plan;
            lines.push(Line::from(vec![
                Span::styled(format!("{}: ", wording.who), bold(theme::current().accent)),
                Span::styled(wording.what, text()),
            ]));
            lines.push(Line::raw(""));
            for s in plan.summary() {
                lines.push(Line::from(vec![
                    Span::styled(icons::BULLET.spaced(), fg(theme::current().info)),
                    Span::styled(s.clone(), bold(theme::current().text)),
                ]));
            }
            for w in plan.warnings() {
                lines.push(Line::styled(format!("{} {w}", icons::WARNING.get()), fg(theme::current().warning)));
            }
            lines.extend(blocked);
            if plan.destructive() {
                lines.push(Line::raw(""));
                lines.push(Line::styled("This deletes or replaces what's there now.", fg(theme::current().warning)));
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
        .border_style(fg(theme::current().warning))
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

pub(super) fn draw_ask(f: &mut Frame, question: String, area: Rect) {
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

/// The help's sections as lines `width` wide, descriptions wrapped under themselves.
pub(super) fn help_lines(sections: &[crate::tui::keymap::HelpSection], width: u16) -> Vec<Line<'static>> {
    const KEYS: usize = 15;
    let desc_w = (width as usize).saturating_sub(KEYS).max(16);
    let mut lines = Vec::new();
    for (name, keys) in sections {
        lines.push(Line::styled(name.to_uppercase(), bold(theme::current().info)));
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
        Span::styled(icons::CHECK.get(), bold(theme::current().accent)),
        Span::styled(" marked  ", dim()),
        Span::styled(icons::CROSS.get(), bold(theme::current().critical)),
        Span::styled(" file missing  ", dim()),
        Span::styled(icons::NEAR.get(), fg(theme::current().warning)),
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

pub(super) fn draw_help(f: &mut Frame, app: &mut App, area: Rect) {
    // The view's own keys first, then those for everywhere: side by side when there's room,
    // else one column that scrolls.
    let two = area.width >= 120;
    let w = if two { 118.min(area.width) } else { 84.min(area.width) };
    let inner_w = w.saturating_sub(4);
    let col_w = if two { inner_w.saturating_sub(2) / 2 } else { inner_w };
    let (own, everywhere) = crate::tui::keymap::help(app.view);
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
