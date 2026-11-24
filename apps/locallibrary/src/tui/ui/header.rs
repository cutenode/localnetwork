//! The top row, the same at every size: the logo, a tab for each view, what the view is about, and
//! localnetwork's tab at the right end. Each tab shows the digit that goes there.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::output::bytes;
use crate::tui::app::{App, View};
use crate::tui::theme::{self, bold, dim, faint, fg, text};
use crate::tui::{icons, keymap};

use super::{clip, fit};

/// The view whose tab stands apart at the right end: localnetwork, what goes out to Local Network
/// rather than work on the music.
const ASIDE: View = View::Profile;

/// From this wide, every tab has room around its name.
const ROOMY: u16 = 120;

/// The header: which view is open, what it's about and how the logo looks.
pub struct Header {
    pub view: View,
    /// What the view's about, as far as a line can say.
    pub context: Vec<Span<'static>>,
    /// Which of `context` is the list's name, which gives way first.
    pub list: Option<usize>,
    pub logo: Style,
    /// The digits are hot cues: the tabs don't show them.
    pub pads: bool,
}

impl Header {
    pub fn of(app: &App) -> Header {
        // The logo dot pulses with the music.
        let pulse = theme::mix(theme::current().accent, theme::current().warning, app.analyzer.level);
        let pulsing = app.is_playing() && !theme::reduced_motion();
        let (context, list) = context(app);
        let logo = bold(if pulsing { pulse } else { theme::current().accent });
        Header { view: app.view, context, list, logo, pads: app.pads_on() }
    }

    /// The tabs, left to right, then the aside's: each tab's spans (its number, the key that goes
    /// there, then its name) and the view it opens.
    fn tabs(&self, roomy: bool) -> Vec<(View, Vec<Span<'static>>)> {
        let style = |v: View| if self.view == v { theme::chip(theme::current().info) } else { dim() };
        let order = View::ALL.into_iter().filter(|v| *v != ASIDE).chain([ASIDE]);
        order
            .map(|v| {
                let padded = roomy || self.view == v;
                // With the pads on the digits are hot cues, so they aren't the views' then.
                let key = if self.pads { String::new() } else { keymap::view_key(v) };
                let (number, name) = match (padded, key.is_empty()) {
                    (true, _) => (key, format!(" {} ", v.title())),
                    (false, false) => (format!("{key} "), v.title().to_string()),
                    (false, true) => (key, v.title().to_string()),
                };
                (v, vec![Span::styled(number, theme::key()), Span::styled(name, style(v))])
            })
            .collect()
    }

    /// The row's spans, and where each tab is (its view, its column, its width) for clicks.
    fn laid_out(&self, width: u16) -> (Vec<Span<'static>>, Vec<(View, u16, u16)>) {
        let roomy = width >= ROOMY;
        let mut tabs = self.tabs(roomy);
        let (aside, aside_spans) = tabs.pop().unwrap_or((ASIDE, Vec::new()));
        let mut left = vec![Span::styled(icons::LOGO.spaced(), self.logo)];
        let mut at = Vec::new();
        let mut x: usize = left.iter().map(Span::width).sum();
        // Each tab starts with its digit, so one that follows a name with no space after it needs one; the
        // logo and a padded tab end in a space.
        let mut spaced = true;
        for (v, spans) in tabs {
            if !spaced {
                left.push(Span::raw(" "));
                x += 1;
            }
            spaced = spans.last().is_some_and(|s| s.content.ends_with(' '));
            let w: usize = spans.iter().map(Span::width).sum();
            at.push((v, x as u16, w as u16));
            left.extend(spans);
            x += w;
        }
        let aside_w: usize = aside_spans.iter().map(Span::width).sum();
        // The context fits between the tabs and the aside, two spaces from each.
        let room = (width as usize).saturating_sub(x + aside_w + 4);
        let about = fitted(&self.context, self.list, room);
        let about_w: usize = about.iter().map(Span::width).sum();
        let after = if about.is_empty() { 0 } else { 2 };
        let gap = (width as usize).saturating_sub(x + about_w + after + aside_w).max(1);
        left.push(Span::raw(" ".repeat(gap)));
        left.extend(about);
        left.push(Span::raw(" ".repeat(after)));
        let aside_x = (width as usize).saturating_sub(aside_w) as u16;
        if x + 1 + about_w + after + aside_w <= width as usize {
            at.push((aside, aside_x, aside_w as u16));
            left.extend(aside_spans);
        }
        (left, at)
    }

    /// Where each tab is in `area`, as clicks find them.
    pub fn tab_areas(&self, area: Rect) -> Vec<(View, Rect)> {
        let (_, at) = self.laid_out(area.width);
        let drawn = at.into_iter().filter(|(_, x, w)| x + w <= area.width);
        drawn.map(|(v, x, w)| (v, Rect::new(area.x + x, area.y, w, 1))).collect()
    }
}

impl Widget for &Header {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let (spans, _) = self.laid_out(area.width);
        Line::from(fit(spans, area.width as usize)).render(area, buf);
    }
}

/// `context` in `room` columns, so the search and the marks stay: the list's name (span `list`) is
/// cut, then goes with the mark before it; then the drive's (the first span) is cut, then goes too
/// where there's a list, taking the space after it; then it goes from the end.
fn fitted(context: &[Span<'static>], list: Option<usize>, room: usize) -> Vec<Span<'static>> {
    let mut out = context.to_vec();
    let total = |s: &[Span]| s.iter().map(Span::width).sum::<usize>();
    let cut = |out: &mut Vec<Span<'static>>, i: usize| {
        let over = total(out).saturating_sub(room);
        if over > 0 && i < out.len() {
            let keep = out[i].width().saturating_sub(over).max(2);
            out[i].content = clip(&out[i].content, keep).into();
        }
    };
    if let Some(i) = list {
        cut(&mut out, i);
        if total(&out) > room {
            out.drain(i.saturating_sub(1)..=i);
        }
    }
    cut(&mut out, 0);
    if list.is_some() && out.len() > 1 && total(&out) > room {
        out.remove(0);
        // What followed the drive's name kept its two spaces from it, which the gap before it gives now.
        out[0].content = out[0].content.trim_start().to_string().into();
    }
    while !out.is_empty() && total(&out) > room {
        out.pop();
    }
    out
}

/// What the view's about, and which span is the list's name. The library: the drive, the list
/// shown, the search and how much is marked. Prepare: the drive its jobs run on. Drives: the one
/// selected and its space left. Recordings: the recording open. localnetwork: who's signed in.
fn context(app: &App) -> (Vec<Span<'static>>, Option<usize>) {
    let spans = match app.view {
        View::Library => return library(app),
        View::Prepare => match app.loaded.as_ref().map(|l| app.drive_label(l.root())) {
            Some(d) => vec![Span::styled("on ", faint()), Span::styled(d, bold(theme::current().text))],
            None => vec![Span::styled("no drive open", faint())],
        },
        View::Drives => {
            let Some(root) = app.action_drive() else { return (Vec::new(), None) };
            let free = app.drives.iter().find(|d| d.root == root).and_then(|d| d.free_space);
            let mut spans = vec![Span::styled(app.drive_label(&root), bold(theme::current().text))];
            if let Some(free) = free {
                spans.push(Span::styled(format!(" · {} free", bytes(free)), dim()));
            }
            spans
        }
        View::Mixes => {
            app.mix.title().map_or_else(Vec::new, |t| vec![Span::styled(t.to_string(), bold(theme::current().text))])
        }
        View::Profile => match app.profile.account.handle() {
            Some(h) => vec![Span::styled(format!("@{h}"), fg(theme::current().info))],
            None => vec![Span::styled("not signed in", faint())],
        },
    };
    (spans, None)
}

/// The library's: the drive, the list shown (the third span), the search and how much is marked.
/// Nothing until a drive is open.
fn library(app: &App) -> (Vec<Span<'static>>, Option<usize>) {
    let Some(l) = &app.loaded else { return (Vec::new(), None) };
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
    (spans, Some(2))
}

#[cfg(test)]
mod tests;
