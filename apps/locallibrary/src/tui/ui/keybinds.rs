//! The Keys screen (`change the keys`, by name in `:`): every action and its keys, with the one
//! selected told of beside the list, or under it on a narrow screen. While a key is being set,
//! that's where it asks for the key and then says what the key would do. It's drawn as the help
//! is: over the view, typed into on its first line, its keys on the bottom border.

use loclib_plugin::Manifest;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget,
};
use unicode_width::UnicodeWidthStr;

use crate::tui::app::App;
use crate::tui::icons;
use crate::tui::keybinds::{self, KeysRow, Stage, manifests, rows};
use crate::tui::keymap::conflict::{self, Answer, Verdict};
use crate::tui::keymap::effective::{self, default_keys, own};
use crate::tui::keymap::{self, Act, K, Scope, dialogs};
use crate::tui::theme::{self, bold, dim, faint, fg, text};

use super::*;

/// From this wide, what's selected is told of beside the list; narrower, under it.
const SIDE_BY_SIDE: u16 = 100;
/// The widest the screen gets.
const WIDEST: u16 = 120;
/// The fewest columns told of the selected action take beside the list, its rule included; on a
/// wider screen they take half.
const SIDE: u16 = 44;
/// The rows it takes under the list while choosing, its rule included; past that it takes all
/// but the row being changed and its heading.
const UNDER: u16 = 7;
/// The columns of a row's keys.
const KEYS: usize = 14;

/// The Keys screen as it is now.
pub(in crate::tui) struct KeysDialog {
    query: String,
    rows: Vec<KeysRow>,
    /// The row selected, among `rows`; none when nothing's left of them.
    row: Option<usize>,
    stage: Stage,
    plugins: Vec<&'static Manifest>,
    /// Where the keys are kept and whether they're on the account, under what's told of the
    /// action: lines in the colour they're said in.
    kept: Vec<(String, Style)>,
}

impl KeysDialog {
    /// The Keys screen, when it's open.
    pub(in crate::tui) fn of(app: &App) -> Option<KeysDialog> {
        let screen = app.keys.as_ref()?;
        let plugins = manifests();
        let rows = rows(&effective::current(), &plugins, &screen.query);
        let row = (!rows.is_empty()).then(|| screen.row.min(rows.len() - 1));
        Some(KeysDialog {
            query: screen.query.clone(),
            rows,
            row,
            stage: screen.stage.clone(),
            plugins,
            kept: kept(app),
        })
    }

    /// The scopes its hints come from, as it is now.
    fn scopes(&self) -> &'static [Scope] {
        match &self.stage {
            Stage::List => &dialogs::KEYS,
            Stage::Capture { .. } => &dialogs::KEYS_CAPTURE,
            Stage::Answer(a) => match a.verdict {
                Verdict::Save => &dialogs::KEYS_ANSWER,
                Verdict::Reset => &dialogs::KEYS_RESET,
                Verdict::Swap { .. } => &dialogs::KEYS_SWAP,
                Verdict::Refused(_) => &dialogs::KEYS_REFUSED,
            },
        }
    }

    /// What the bottom border says: what enter and esc do now, and in the list, that the arrows
    /// move. Enter does nothing on a key that can't change, or with no row left.
    fn hints(&self) -> Line<'static> {
        let listing = self.stage == Stage::List;
        let can_change = self.row.is_some_and(|i| !self.rows[i].fixed);
        let shown = |a: &Act| match a {
            Act::Move(_) => listing,
            Act::Activate => can_change || !listing,
            _ => true,
        };
        let hints: Vec<_> = keymap::dialog_hints(self.scopes()).into_iter().filter(|h| shown(&h.act)).collect();
        hint_bar(&hints, &[])
    }

    /// The list's lines, under the headings, and which of them is the row selected (its heading
    /// too, when it's the first under it).
    fn list(&self, width: usize) -> (Vec<Line<'static>>, std::ops::Range<usize>) {
        let mut lines = Vec::new();
        let mut selected = 0..0;
        let mut group = "";
        for (i, r) in self.rows.iter().enumerate() {
            let start = lines.len();
            if r.group != group {
                group = r.group;
                lines.push(Line::styled(clip(&group.to_uppercase(), width), bold(theme::current().info)));
            }
            let mut line = row_line(r, width);
            if Some(i) == self.row {
                line.style = theme::selected();
                selected = start..lines.len() + 1;
            }
            lines.push(line);
        }
        if self.rows.is_empty() {
            lines.push(Line::styled(clip("No action has those words.", width), dim()));
        }
        (lines, selected)
    }

    /// What's told of the action selected, as it is now, `width` wide.
    fn told(&self, width: usize) -> Vec<Line<'static>> {
        let wrapped = |s: &str, style: Style| -> Vec<Line<'static>> {
            wrap_text(s, width).into_iter().map(|l| Line::styled(l, style)).collect()
        };
        match &self.stage {
            Stage::List => {
                let Some(r) = self.row.map(|i| &self.rows[i]) else {
                    return wrapped("Type less to see more actions.", dim());
                };
                let mut lines = wrapped(&r.name, bold(theme::current().text));
                lines.push(keys_line(r));
                if let Some(act) = r.act {
                    lines.extend(wrapped(keymap::about(act), dim()));
                }
                if let Some(id) = &r.id {
                    lines.push(Line::styled(clip(id, width), faint()));
                }
                lines
            }
            Stage::Capture { act } => {
                let name = keybinds::title(*act, &self.plugins);
                let mut lines = wrapped(&format!("Press the new key for {name}"), bold(theme::current().accent));
                // The row being given a key is the one selected.
                let row = self.row.map(|i| &self.rows[i]);
                lines.extend(row.map(keys_line));
                // Its own key back is how one action goes back as it came, which nothing else says.
                // One that came with none goes back by the key it has now.
                let back = row.filter(|r| r.changed).map(|r| match r.default.as_str() {
                    "none" => format!("{} puts it back to no key", r.keys),
                    default => format!("{default} puts its default back"),
                });
                lines.extend(back.into_iter().flat_map(|s| wrapped(&s, dim())));
                lines
            }
            Stage::Answer(a) => self.answer(a, width),
        }
    }

    /// What the key pressed would do: what that comes to, then where it changes what.
    fn answer(&self, a: &Answer, width: usize) -> Vec<Line<'static>> {
        let name =
            |act: Option<Act>| act.map_or_else(|| "nothing".to_string(), |act| keybinds::title(act, &self.plugins));
        let said = |s: &str, style: Style| -> Vec<Line<'static>> {
            wrap_text(s, width).into_iter().map(|l| Line::styled(l, style)).collect()
        };
        let mut lines = said(&format!("{} for {}", a.key.name(), name(Some(a.act))), bold(theme::current().text));
        let verdict = match &a.verdict {
            Verdict::Save => None,
            Verdict::Refused(why) => return [lines, said(why, fg(theme::current().warning))].concat(),
            Verdict::Reset => Some(match own(&default_keys(a.act)) {
                came if came.is_empty() => "back to no key".to_string(),
                came => format!("back to its default: {}", conflict::written(&came)),
            }),
            Verdict::Swap { with, gets } => {
                Some(swapped(&name(Some(*with)), &own(&effective::current().keys(*with)), gets))
            }
        };
        lines.extend(verdict.iter().flat_map(|v| said(v, bold(theme::current().info))));
        // Another action losing the key is what to look at twice, so it comes first.
        let loses = |e: &&conflict::Effect| e.before.is_some_and(|b| b != a.act);
        let (losing, rest): (Vec<_>, Vec<_>) = a.effects.iter().partition(loses);
        // Only a reset can give more than the key pressed, so otherwise each change is that key's.
        let lead = match a.verdict {
            Verdict::Reset => String::new(),
            _ => format!("{} ", a.key.name()),
        };
        for e in losing.iter().chain(&rest) {
            let style = if loses(e) { fg(theme::current().warning) } else { text() };
            let change =
                clip(&format!("{} → {}", name(e.before), name(e.after)), width.saturating_sub(2 + lead.width()));
            let key = Span::styled(lead.clone(), theme::key());
            lines.push(Line::from(vec![Span::raw("  "), key, Span::styled(change, style)]));
            let at = places(&e.places, width.saturating_sub(4));
            lines.push(Line::from(vec![Span::raw("    "), Span::styled(at, faint())]));
        }
        lines
    }

    /// The told lines with where the keys are kept pinned under them, in the list, `height` rows
    /// `width` wide: what's told gives way first.
    fn panel(&self, width: usize, height: usize) -> Vec<Line<'static>> {
        let foot: Vec<Line<'static>> = match self.stage {
            Stage::List => (self.kept.iter())
                .flat_map(|(s, style)| wrap_text(s, width).into_iter().map(|l| Line::styled(l, *style)))
                .collect(),
            _ => Vec::new(),
        };
        let mut lines = self.told(width);
        // A blank line between, where there's a foot.
        let room = height.saturating_sub(foot.len() + usize::from(!foot.is_empty()));
        lines.truncate(room);
        if !foot.is_empty() {
            let gap = height.saturating_sub(lines.len() + foot.len());
            lines.extend(std::iter::repeat_n(Line::raw(""), gap));
            lines.extend(foot);
        }
        lines.truncate(height);
        lines
    }
}

impl Widget for &KeysDialog {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let side_by_side = area.width >= SIDE_BY_SIDE;
        let r = if side_by_side {
            popup(area, area.width.saturating_sub(4).min(WIDEST), area.height.saturating_sub(2), None)
        } else {
            area
        };
        let b = block(title("Change keys", true), true)
            .title_bottom(self.hints().right_aligned())
            .padding(Padding::horizontal(1));
        let inner = b.inner(r);
        Clear.render(r, buf);
        b.render(r, buf);
        let [input, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
        let n = self.rows.iter().filter(|r| !r.fixed).count();
        let count = vec![Span::styled(format!("  {n} action{}", if n == 1 { "" } else { "s" }), faint())];
        let label = vec![Span::styled("› ", theme::key())];
        let typed = if self.stage == Stage::List { self.query.as_str() } else { "" };
        let line = if self.stage == Stage::List {
            input_line(label, typed, count, input.width)
        } else {
            Line::from(vec![Span::styled("› ", faint()), Span::styled(self.query.clone(), dim())])
        };
        Paragraph::new(line).render(input, buf);

        let (list, side, rule) = if side_by_side {
            let side = (body.width / 2).max(SIDE);
            let [list, side] = Layout::horizontal([Constraint::Min(0), Constraint::Length(side)]).areas(body);
            (list, side, Borders::LEFT)
        } else {
            // Past the list, what a key would do needs the room more than the rows do: the row
            // being changed and its heading stay.
            let under = match self.stage {
                Stage::List => UNDER.min(body.height / 2),
                _ => body.height.saturating_sub(2),
            };
            let [list, side] = Layout::vertical([Constraint::Min(0), Constraint::Length(under)]).areas(body);
            (list, side, Borders::TOP)
        };
        // A column for the scrollbar.
        let (lines, selected) = self.list(list.width.saturating_sub(1) as usize);
        // The row selected in view; the list moves under it only as far as it has to.
        let top = selected.end.saturating_sub(list.height as usize).min(selected.start);
        let total = lines.len();
        Paragraph::new(lines).scroll((top as u16, 0)).render(list, buf);
        if total > list.height as usize {
            let mut bar = ScrollbarState::new(total.saturating_sub(list.height as usize)).position(top);
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(fg(theme::current().dim))
                .track_style(faint())
                .render(list, buf, &mut bar);
        }

        let ruled = Block::new().borders(rule).border_style(theme::rule()).padding(if side_by_side {
            Padding::left(2)
        } else {
            Padding::ZERO
        });
        let told = ruled.inner(side);
        ruled.render(side, buf);
        Paragraph::new(self.panel(told.width as usize, told.height as usize)).render(told, buf);
    }
}

/// A row of the list `width` wide: a dot for keys that changed, its keys, its name.
fn row_line(r: &KeysRow, width: usize) -> Line<'static> {
    let mark = match r.changed {
        true => Span::styled(format!("{} ", icons::BULLET.get()), fg(theme::current().accent)),
        false => Span::raw("  "),
    };
    // An action with no key is found by name; `:` there would read as its key.
    let none = r.act.is_some() && r.keys == ":";
    let (key_style, name_style) = match (r.fixed, none) {
        (true, _) => (faint(), faint()),
        (_, true) => (faint(), text()),
        _ => (theme::key(), text()),
    };
    let keys = clip(if none { "none" } else { &r.keys }, KEYS - 1);
    let pad = " ".repeat(KEYS - keys.width());
    let name = clip(&r.name, width.saturating_sub(KEYS + 2));
    // Filled out to the width, so the row selected is marked across it.
    let fill = " ".repeat(width.saturating_sub(KEYS + 2 + name.width()));
    Line::from(vec![mark, Span::styled(keys, key_style), Span::raw(pad), Span::styled(name + &fill, name_style)])
}

/// A row's keys, and what they were as they came when they've changed, or that they can't.
fn keys_line(r: &KeysRow) -> Line<'static> {
    let none = r.act.is_some() && r.keys == ":";
    let mut spans = match none {
        true => vec![Span::styled("no key: find it by name in :", faint())],
        false => vec![Span::styled(r.keys.clone(), theme::key())],
    };
    if r.fixed {
        spans.push(Span::styled("   can't change", faint()));
    } else if r.changed {
        spans.extend([Span::styled("   default ", faint()), Span::styled(r.default.clone(), text())]);
    }
    Line::from(spans)
}

/// The places an effect is in, as many as fit in `width`, then how many more.
fn places(all: &[&str], width: usize) -> String {
    let said = |shown: usize| {
        let left = all.len() - shown;
        let tail = if left > 0 { format!(" and {left} more") } else { String::new() };
        let lead = if all.first() == Some(&"everywhere") { "" } else { "in " };
        format!("{lead}{}{tail}", all[..shown].join(", "))
    };
    let mut shown = all.len().min(1);
    while shown < all.len() && said(shown + 1).width() <= width {
        shown += 1;
    }
    clip(&said(shown), width)
}

/// What a swap does to the other action `with`, whose keys are `had`: what it gets in place of
/// what it gives up, or that it's left with none.
fn swapped(with: &str, had: &[K], gets: &[K]) -> String {
    let given: Vec<K> = gets.iter().filter(|k| !had.contains(k)).copied().collect();
    let taken: Vec<K> = had.iter().filter(|k| !gets.contains(k)).copied().collect();
    match (gets, &given[..]) {
        ([], _) => format!("{with} will have no key"),
        (_, []) => format!("{with} keeps {}", conflict::written(gets)),
        _ => format!("{with} gets {} in place of {}", conflict::written(&given), conflict::written(&taken)),
    }
}

/// Where the keys are kept, whether they're on the account, and anything wrong with the file.
fn kept(app: &App) -> Vec<(String, Style)> {
    let warning = fg(theme::current().warning);
    let mut out = Vec::new();
    if app.keybinds_unreadable {
        out.push(("keybinds.json can't be read, so changes last until you quit.".into(), warning));
    }
    let problems = &effective::current().problems;
    if let Some(first) = problems.first() {
        let more = match problems.len() {
            1 => String::new(),
            n => format!(" ({} more)", n - 1),
        };
        out.push((format!("keybinds.json: {first}{more}"), warning));
    }
    let account = &app.profile.account;
    let p = &app.publishing;
    let who = account.handle().map_or_else(|| "your account".to_string(), |h| format!("@{h}"));
    let line = match (account.signed_in(), &p.error) {
        // One arm a build: under `cfg!` the other would be built and never run.
        #[cfg(feature = "account")]
        (false, _) => ("Kept on this computer. Sign in to use them anywhere.".into(), dim()),
        #[cfg(not(feature = "account"))]
        (false, _) => ("Kept on this computer.".into(), dim()),
        (true, Some(why)) => (format!("Not published: {why}"), warning),
        (true, None) if p.due.is_some() || p.running => (format!("Publishing to {who}…"), dim()),
        (true, None) if p.published => (format!("Published to {who}"), fg(theme::current().good)),
        (true, None) => (format!("Changes publish to {who}"), dim()),
    };
    out.push(line);
    if let Some(from) = &app.keybinds.from {
        out.push((format!("Some keys from {from}"), faint()));
    }
    out
}

#[cfg(test)]
mod tests;
