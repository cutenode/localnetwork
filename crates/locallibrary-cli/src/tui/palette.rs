//! Choosing from a list by typing: the command palette (`:`), which lists every action of the
//! keymap by name, and the lists actions ask for an answer from (a drive, a snapshot, a tag).
//! What's typed narrows the list fuzzily: its letters in order, best where they start words.

use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;
use ratatui::widgets::ListState;

use super::app::{App, View, edit_line};
use super::drives::{DriveKey, PackWhat};
use super::edits::Modal;
use super::keymap::{Act, K, KEYMAP, Scope};
use super::mytags::TagChoice;

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
    /// Packing: what goes on the drive (one per item), onto `target` when it's known.
    PackWhat { source: PathBuf, target: Option<PathBuf>, choices: Vec<PackWhat> },
    /// Packing `what`: onto the drive picked (one per item).
    PackOnto { source: PathBuf, what: PackWhat, roots: Vec<PathBuf> },
    /// Bring `playlist` back onto the drive at `root` from the snapshot picked (one per item;
    /// `None` for the newest that has it).
    Snapshot { root: PathBuf, playlist: String, snapshots: Vec<Option<String>> },
    /// Give `tracks` on the drive at `root` the tag picked, or take it off (one per item).
    Tag { root: PathBuf, tracks: Vec<String>, choices: Vec<TagChoice> },
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

/// How a key is written in the palette.
fn key_name(k: &K) -> String {
    let code = |c: &KeyCode| match c {
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Esc => "esc".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::Up => "↑".into(),
        KeyCode::Down => "↓".into(),
        KeyCode::Left => "←".into(),
        KeyCode::Right => "→".into(),
        KeyCode::Delete => "del".into(),
        KeyCode::Char(' ') => "space".into(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::F(n) => format!("F{n}"),
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

/// The palette's commands in `view`: the view's own first, then those for everywhere, each once.
pub fn commands(view: View) -> Vec<(Act, Item)> {
    let own: &[Scope] = match view {
        View::Library => &[Scope::Tracks, Scope::Tree, Scope::Details, Scope::Library, Scope::Crate],
        View::Drives => &[Scope::Drives],
        View::Snapshots => &[Scope::Snapshots],
        View::Profile => &[Scope::Profile],
        View::Mixes => &[Scope::Mixes],
    };
    let mut out: Vec<(Act, Item)> = Vec::new();
    for scope in own.iter().chain(&[Scope::Global]) {
        for b in KEYMAP.iter().filter(|b| b.scope == *scope && !b.name.is_empty()) {
            if out.iter().any(|(a, _)| *a == b.act) {
                continue;
            }
            let detail = match b.keys.first() {
                Some(k) => key_name(k),
                None => b.shown.to_string(),
            };
            out.push((b.act, Item::new(b.name, detail)));
        }
    }
    out
}

impl App {
    /// `:`: every action by name.
    pub(super) fn open_palette(&mut self) {
        let (acts, items): (Vec<Act>, Vec<Item>) = commands(self.view).into_iter().unzip();
        let mut p = Picker::new("Commands", items, Then::Act(acts));
        p.empty = "no command has those letters";
        self.modal = Some(Modal::Pick(p));
    }

    /// Keys while a list to choose from is open: typing narrows it, the arrows move, enter
    /// chooses, esc closes it.
    pub(super) fn on_picker_key(&mut self, mut p: Picker, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => return,
            KeyCode::Enter => return self.choose_item(p),
            KeyCode::Up => p.step(-1),
            KeyCode::Down => p.step(1),
            KeyCode::PageUp => p.step(-10),
            KeyCode::PageDown => p.step(10),
            _ => {
                if edit_line(&mut p.query, key) {
                    p.filter();
                }
            }
        }
        self.modal = Some(Modal::Pick(p));
    }

    /// Clicks while a list is open: an item is chosen; a click outside closes it.
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
                    return self.choose_item(p);
                }
            }
            MouseEventKind::Down(_) => return,
            _ => {}
        }
        self.modal = Some(Modal::Pick(p));
    }

    fn choose_item(&mut self, p: Picker) {
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
            Then::PackWhat { source, target, mut choices } => self.pack_what(source, target, choices.swap_remove(i)),
            Then::PackOnto { source, what, mut roots } => self.pack_onto(source, roots.swap_remove(i), what),
            Then::Snapshot { root, playlist, mut snapshots } => {
                self.plan_restore(root, playlist, snapshots.swap_remove(i))
            }
            Then::Tag { root, tracks, mut choices } => self.tag_chosen(root, tracks, choices.swap_remove(i)),
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::KeyModifiers;

    use super::*;

    #[test]
    fn fuzzy_matching_prefers_word_starts_and_runs() {
        assert!(fuzzy("", "anything").is_some());
        assert!(fuzzy("xyz", "next track").is_none());
        assert!(fuzzy("nt", "next track").is_some());
        // Starts of words beat letters in the middle.
        assert!(fuzzy("nt", "next track") > fuzzy("nt", "print"));
        // A run beats the same letters spread out.
        assert!(fuzzy("loop", "loop here") > fuzzy("loop", "l o o p"));
        assert_eq!(fuzzy("NEXT", "next track"), fuzzy("next", "Next Track"), "case doesn't matter");
        assert!(fuzzy("next track", "next track").is_some(), "spaces in the query are ignored");
    }

    #[test]
    fn the_palette_lists_the_views_commands_first_and_runs_one() {
        let cmds = commands(View::Profile);
        assert_eq!(cmds[0].0, Act::ProfileInclude, "the view's own first");
        assert!(cmds.iter().any(|(a, _)| *a == Act::Quit));
        assert!(!cmds.iter().any(|(a, _)| *a == Act::CrateSave), "not the library's in the profile");
        let acts: Vec<Act> = cmds.iter().map(|(a, _)| *a).collect();
        let mut dedup = acts.clone();
        dedup.dedup();
        assert_eq!(acts.len(), dedup.len());

        let dir = tempfile::tempdir().unwrap();
        let mut app = App::with_library(dir.path(), super::super::browse::tests::library());
        let press = |app: &mut App, code: KeyCode| app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
        press(&mut app, KeyCode::Char(':'));
        assert!(matches!(app.modal, Some(Modal::Pick(_))));
        for c in "drives".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        let Some(Modal::Pick(p)) = &app.modal else { panic!() };
        assert_eq!(p.items[p.selected().unwrap()].label, "the drives");
        // Keys go to the box, not to the player: q didn't quit.
        press(&mut app, KeyCode::Char('q'));
        assert!(!app.quit);
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Enter);
        assert!(app.modal.is_none());
        assert_eq!(app.view, View::Drives);
        // Nothing matching: enter says so and stays.
        press(&mut app, KeyCode::Char(':'));
        for c in "zzzz".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.modal, Some(Modal::Pick(_))));
        press(&mut app, KeyCode::Esc);
        assert!(app.modal.is_none());
    }

    #[test]
    fn a_command_is_found_by_the_name_beside_it() {
        let (acts, items): (Vec<Act>, Vec<Item>) = commands(View::Drives).into_iter().unzip();
        let mut p = Picker::new("Commands", items, Then::Act(acts.clone()));
        for (typed, act) in [("pull", Act::Pull), ("set prep", Act::SetPrep), ("restore", Act::RestorePlaylist)] {
            p.query = typed.into();
            p.filter();
            assert_eq!(acts[p.selected().unwrap()], act, "{typed}");
        }
    }
}
