//! Choosing from a list by typing: the command palette (`:`), which lists every action of the
//! keymap by name, and the lists actions ask for an answer from (a drive, a snapshot, a tag, a
//! plugin's question). What's typed narrows the list fuzzily: its letters in order, best where
//! they start words. A list some of which may be chosen is marked with space.

use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;
use ratatui::widgets::ListState;
use uuid::Uuid;

use super::app::{App, View, edit_line};
use super::drives::{DriveKey, PackWhat};
use super::edits::Modal;
use super::keymap::{Act, K, KEYMAP, Scope};
use super::mytags::TagChoice;
use super::plugins::Asking;

/// A list to choose from, narrowed by what's typed.
pub struct Picker {
    pub title: String,
    pub items: Vec<Item>,
    pub query: String,
    /// The items matching the query (indexes into `items`), best first.
    pub shown: Vec<usize>,
    pub state: ListState,
    /// What choosing an item does.
    pub then: Then,
    /// Said under the list when nothing matches.
    pub empty: &'static str,
    /// Which items are marked, for a list any of which may be chosen (space marks one, enter
    /// takes those marked); `None` for a list one is chosen from.
    pub marks: Option<Vec<bool>>,
}

/// One choice.
#[derive(Debug, Clone, Default)]
pub struct Item {
    pub label: String,
    /// On the right: a key, a date, a count.
    pub detail: String,
    /// Can't be chosen now, and why.
    pub unavailable: Option<String>,
}

impl Item {
    pub fn new(label: impl Into<String>, detail: impl Into<String>) -> Item {
        Item { label: label.into(), detail: detail.into(), unavailable: None }
    }
}

/// What choosing does.
pub enum Then {
    /// Do the action (the palette): one per item.
    Act(Vec<Act>),
    /// Do one of a drive's actions to `drive` (called `name`), wherever it's mounted by then (one
    /// per item).
    Drive { drive: DriveKey, name: String, acts: Vec<Act> },
    /// Do one of a library's actions to its clones plugged in by then (one per item).
    Library { id: Uuid, acts: Vec<Act> },
    /// Packing: what goes on the drive (one per item), onto `target` when it's known.
    PackWhat { source: PathBuf, target: Option<PathBuf>, choices: Vec<PackWhat> },
    /// Packing `what`: onto the drive picked (one per item).
    PackOnto { source: PathBuf, what: PackWhat, roots: Vec<PathBuf> },
    /// Bring `playlist` back onto the drive at `root` from the snapshot picked (one per item;
    /// `None` for the newest that has it).
    Snapshot { root: PathBuf, playlist: String, snapshots: Vec<Option<String>> },
    /// Give `tracks` on the drive at `root` the tag picked, or take it off (one per item).
    Tag { root: PathBuf, tracks: Vec<String>, choices: Vec<TagChoice> },
    /// Answer a plugin's question: its items are the question's.
    Choose(Box<Asking>),
}

impl Picker {
    pub fn new(title: impl Into<String>, items: Vec<Item>, then: Then) -> Picker {
        let mut p = Picker {
            title: title.into(),
            shown: (0..items.len()).collect(),
            items,
            query: String::new(),
            state: ListState::default(),
            then,
            empty: "nothing matches",
            marks: None,
        };
        p.state.select(p.first_available());
        p
    }

    fn first_available(&self) -> Option<usize> {
        let i = self.shown.iter().position(|&i| self.items[i].unavailable.is_none());
        i.or((!self.shown.is_empty()).then_some(0))
    }

    /// Narrow the list to what's typed, in the label or the detail (so a command's name finds it),
    /// best matches first (ties keep the list's order).
    pub fn filter(&mut self) {
        let mut scored: Vec<(i32, usize)> = (self.items.iter().enumerate())
            .filter_map(|(i, it)| fuzzy(&self.query, &it.label).max(fuzzy(&self.query, &it.detail)).map(|s| (s, i)))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        self.shown = scored.into_iter().map(|(_, i)| i).collect();
        self.state = ListState::default();
        self.state.select(self.first_available());
    }

    /// The item selected, as an index into `items`.
    pub fn selected(&self) -> Option<usize> {
        self.state.selected().and_then(|s| self.shown.get(s)).copied()
    }

    /// Marks the item selected, or takes its mark off, in a list any of which may be chosen.
    fn toggle(&mut self) {
        if let (Some(i), Some(marks)) = (self.selected(), self.marks.as_mut()) {
            marks[i] = !marks[i];
        }
    }

    fn step(&mut self, delta: isize) {
        let n = self.shown.len();
        if n == 0 {
            return;
        }
        let cur = self.state.selected().unwrap_or(0) as isize;
        self.state.select(Some((cur + delta).clamp(0, n as isize - 1) as usize));
    }
}

/// How well `query` matches `text`: all of the query's letters (spaces aside) in order, ignoring
/// case, scoring more where they start words or follow each other and less for what's skipped.
/// `None` when they aren't all there. An empty query matches everything equally.
pub fn fuzzy(query: &str, text: &str) -> Option<i32> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    // The best placing of the query's letters, found letter by letter: `best[i]` is the best score
    // with the letter so far at `t[i]`.
    let word_start = |i: usize| i == 0 || !t[i - 1].is_alphanumeric();
    let gain = |i: usize| if word_start(i) { 6 } else { 1 };
    let mut best: Vec<Option<i32>> =
        (0..t.len()).map(|i| (t[i] == q[0]).then(|| gain(i) - (i as i32).min(3))).collect();
    for &c in &q[1..] {
        let next = (0..t.len())
            .map(|i| {
                if t[i] != c {
                    return None;
                }
                let link = |j: usize| if j + 1 == i { 6 } else { -2 * ((i - j - 1) as i32).min(3) };
                (0..i).filter_map(|j| Some(best[j]? + link(j))).max().map(|s| s + gain(i))
            })
            .collect();
        best = next;
    }
    best.into_iter().flatten().max()
}

/// How a key is written in the palette: as the help writes it (a test checks every command's).
fn key_name(k: &K) -> String {
    let code = |c: &KeyCode| match c {
        KeyCode::Up => "↑".to_string(),
        KeyCode::Down => "↓".into(),
        KeyCode::Left => "←".into(),
        KeyCode::Right => "→".into(),
        other => format!("{other:?}").to_lowercase(),
    };
    match k {
        K::Ch(' ') => "space".into(),
        K::Ch(c) => c.to_string(),
        K::Code(c) => code(c),
        K::Shift(c) => format!("shift {}", code(c)),
        K::Ctrl(c) => format!("ctrl-{c}"),
        K::Digits(a, b) => format!("{a}-{b}"),
    }
}

/// The palette's commands in `view`: the view's own first, then those for everywhere (the keymap
/// names each command once in a view, which a test checks).
pub fn commands(view: View) -> Vec<(Act, Item)> {
    let own: &[Scope] = match view {
        View::Library => &[Scope::Tracks, Scope::Tree, Scope::Details, Scope::Library, Scope::Crate],
        View::Drives => &[Scope::Drives],
        View::Snapshots => &[Scope::Snapshots],
        View::Profile => &[Scope::Profile],
        View::Mixes => &[Scope::Mixes],
    };
    let scopes = own.iter().chain(&[Scope::Global]);
    let named = scopes.flat_map(|scope| KEYMAP.iter().filter(move |b| b.scope == *scope && !b.name.is_empty()));
    named
        .map(|b| {
            let detail = b.keys.first().map_or_else(|| b.shown.to_string(), key_name);
            (b.act, Item::new(b.name, detail))
        })
        .collect()
}

impl App {
    /// `:`: every action by name, then plugins' commands, operations and analyzers, in the
    /// library the columns of their fields, and their settings.
    pub(super) fn open_palette(&mut self) {
        let acts = commands(self.view)
            .into_iter()
            .chain(self.plugin_items(self.action_drive().as_deref(), &super::plugins::EVERYWHERE));
        let fields = (self.view == View::Library).then(|| self.field_items()).into_iter().flatten();
        let (acts, items): (Vec<Act>, Vec<Item>) = acts.chain(fields).chain(self.settings_items()).unzip();
        let mut p = Picker::new("Commands", items, Then::Act(acts));
        p.empty = "no command has those letters";
        self.modal = Some(Modal::Pick(p));
    }

    /// Keys while a list to choose from is open: typing narrows it (in a question taking several,
    /// space marks an item), the arrows move, enter chooses, esc closes it.
    pub(super) fn on_picker_key(&mut self, mut p: Picker, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => return,
            KeyCode::Enter => return self.choose_item(p),
            KeyCode::Up => p.step(-1),
            KeyCode::Down => p.step(1),
            KeyCode::PageUp => p.step(-10),
            KeyCode::PageDown => p.step(10),
            KeyCode::Char(' ') if p.marks.is_some() => p.toggle(),
            _ => {
                if edit_line(&mut p.query, key) {
                    p.filter();
                }
            }
        }
        self.modal = Some(Modal::Pick(p));
    }

    /// Clicks while a list is open: an item is chosen (marked, in a list any of which may be);
    /// a click outside closes it.
    pub(super) fn on_picker_mouse(&mut self, mut p: Picker, m: MouseEvent) {
        let r = self.areas.menu;
        match m.kind {
            MouseEventKind::ScrollUp => p.step(-3),
            MouseEventKind::ScrollDown => p.step(3),
            MouseEventKind::Down(MouseButton::Left) if r.contains(Position::new(m.column, m.row)) => {
                // The list starts under the line being typed.
                let row = m.row.saturating_sub(r.y + 2) as usize + p.state.offset();
                if m.row >= r.y + 2 && row < p.shown.len() {
                    p.state.select(Some(row));
                    if p.marks.is_none() {
                        return self.choose_item(p);
                    }
                    p.toggle();
                }
            }
            MouseEventKind::Down(_) => return,
            _ => {}
        }
        self.modal = Some(Modal::Pick(p));
    }

    fn choose_item(&mut self, p: Picker) {
        // Those marked, whichever are shown: none is an answer too.
        if let (Some(marks), Then::Choose(asking)) = (&p.marks, &p.then) {
            let values = (marks.iter().zip(asking.choice().items.iter())).filter(|(m, _)| **m);
            let values = values.map(|(_, item)| item.value.clone()).collect();
            let Then::Choose(asking) = p.then else { unreachable!("matched above") };
            return self.answer(*asking, values);
        }
        let Some(i) = p.selected() else {
            self.toast(p.empty, super::app::Level::Info);
            self.modal = Some(Modal::Pick(p));
            return;
        };
        if let Some(why) = p.items[i].unavailable.clone() {
            self.toast(why, super::app::Level::Info);
            self.modal = Some(Modal::Pick(p));
            return;
        }
        match p.then {
            Then::Act(acts) => self.perform(acts[i], None),
            Then::Drive { drive, name, acts } => match self.mounted(&drive) {
                Some(root) => self.drive_act(acts[i], root),
                None => self.toast(format!("{name} isn't plugged in any more"), super::app::Level::Info),
            },
            Then::Library { id, acts } => self.library_act(acts[i], id),
            Then::PackWhat { source, target, mut choices } => self.pack_what(source, target, choices.swap_remove(i)),
            Then::PackOnto { source, what, mut roots } => self.pack_onto(source, roots.swap_remove(i), what),
            Then::Snapshot { root, playlist, mut snapshots } => {
                self.plan_restore(root, playlist, snapshots.swap_remove(i))
            }
            Then::Tag { root, tracks, mut choices } => self.tag_chosen(root, tracks, choices.swap_remove(i)),
            Then::Choose(asking) => {
                let value = asking.choice().items[i].value.clone();
                self.answer(*asking, vec![value])
            }
        }
    }
}

#[cfg(test)]
mod tests;
