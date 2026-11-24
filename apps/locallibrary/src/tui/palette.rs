//! Choosing from a list by typing: the command palette (`:`), which lists every action of the
//! keymap by name, with the key each has, the ones used lately first and those that can't run now
//! greyed with the reason; the lists actions ask for an answer from (a drive, a snapshot, a tag, a
//! plugin's question); and the actions menu (`e`) outside the Library view, which works as the
//! Library's does: its letters run their item, and `/` starts narrowing it. What's typed narrows
//! the list fuzzily: its letters in order, best where they start words. A list some of which may be
//! chosen is marked with space.

use std::cmp::Reverse;
use std::path::PathBuf;

use ratatui::crossterm::event::{KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;
use ratatui::widgets::ListState;
use uuid::Uuid;

use super::app::{App, View, edit_line, step_list};
use super::drives::{DriveKey, PackWhat};
use super::edits::Modal;
use super::keymap::{self, Act, KEYMAP, Scope, dialogs};
use super::mytags::TagChoice;
use super::plugins::Asking;
use super::prepare;

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
    /// How recently each item was used, higher for later, 0 for not lately; empty but in the
    /// palette, whose choices count as used.
    recency: Vec<usize>,
    /// An actions menu: its letters run their item until `/` starts a filter.
    pub actions: bool,
    /// An actions menu whose filter `/` started, with nothing typed yet.
    pub filtering: bool,
}

/// One choice.
#[derive(Debug, Clone, Default)]
pub struct Item {
    pub label: String,
    /// On the right: a key, a date, a count.
    pub detail: String,
    /// Can't be chosen now, and why.
    pub unavailable: Option<String>,
    /// More words it's found by, not shown: comma-separated phrases, each matched on its own.
    pub words: String,
}

impl Item {
    pub fn new(label: impl Into<String>, detail: impl Into<String>) -> Item {
        Item { label: label.into(), detail: detail.into(), unavailable: None, words: String::new() }
    }

    /// How well `query` matches it (see [`fuzzy`]): the best of its label, its detail and each of
    /// its words.
    pub fn score(&self, query: &str) -> Option<i32> {
        let words = self.words.split(',').filter(|w| !w.trim().is_empty()).map(|w| fuzzy(query, w.trim()));
        [fuzzy(query, &self.label), fuzzy(query, &self.detail)].into_iter().chain(words).max().flatten()
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
    /// Take someone's keys, or a preset's: those of the actions marked.
    Keys(Box<super::keybinds::KeysImport>),
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
            recency: Vec::new(),
            actions: false,
            filtering: false,
        };
        p.state.select(p.first_available());
        p
    }

    /// An actions menu: as [`Picker::new`], its letters running their item.
    pub fn actions(title: impl Into<String>, items: Vec<Item>, then: Then) -> Picker {
        Picker { actions: true, ..Picker::new(title, items, then) }
    }

    /// Whether letters are typed into the filter, rather than chosen by.
    pub fn typing(&self) -> bool {
        !self.actions || !self.query.is_empty() || self.filtering
    }

    /// What each item does, for a list whose items are actions.
    fn acts(&self) -> &[Act] {
        match &self.then {
            Then::Act(acts) | Then::Drive { acts, .. } | Then::Library { acts, .. } => acts,
            _ => &[],
        }
    }

    fn first_available(&self) -> Option<usize> {
        let i = self.shown.iter().position(|&i| self.items[i].unavailable.is_none());
        i.or((!self.shown.is_empty()).then_some(0))
    }

    /// Rank the items used lately (`recent`, newest first, by label) ahead of the others.
    fn rank(&mut self, recent: &[String]) {
        let at = |it: &Item| recent.iter().position(|r| *r == it.label).map_or(0, |i| recent.len() - i);
        self.recency = self.items.iter().map(at).collect();
        self.filter();
    }

    /// Narrow the list to what's typed, in the label, the detail (so a command's name finds it) or
    /// the words it's found by, best matches first, with the edge a command used lately has (ties
    /// go to the later one, then keep the list's order). A label or one of its words typed out in
    /// full comes first whatever was used lately, so "cover art" isn't "no cover art" just because
    /// that ran last.
    pub fn filter(&mut self) {
        let recency = |i: usize| self.recency.get(i).copied().unwrap_or(0);
        let typed = self.query.trim().to_lowercase();
        let mut scored: Vec<(bool, i32, usize)> = (self.items.iter().enumerate())
            .filter_map(|(i, it)| it.score(&self.query).map(|s| (s, i)))
            .map(|(s, i)| {
                let it = &self.items[i];
                let word = it.words.split(',').any(|w| w.trim().to_lowercase() == typed);
                let named = !typed.is_empty() && (it.label.to_lowercase() == typed || word);
                (named, if recency(i) > 0 { s + RECENT_EDGE } else { s }, i)
            })
            .collect();
        scored.sort_by_key(|&(named, s, i)| (Reverse(named), Reverse(s), Reverse(recency(i)), i));
        self.shown = scored.into_iter().map(|(_, _, i)| i).collect();
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

    /// Select the item that does `act`, when it's shown.
    fn select_act(&mut self, act: Act) -> bool {
        let at = self.acts().iter().position(|a| *a == act).and_then(|i| self.shown.iter().position(|s| *s == i));
        if at.is_some() {
            self.state.select(at);
        }
        at.is_some()
    }
}

/// How many of the commands run lately are kept.
const RECENT: usize = 20;
/// What being used lately is worth, against [`fuzzy`]'s scores: one word start, enough to settle
/// near ties and not enough to lift a scattered match over a clean one.
const RECENT_EDGE: i32 = 6;

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

/// The palette's commands in `view` as it opens (the track list with focus, nothing over it).
#[cfg(test)]
pub fn commands(view: View) -> Vec<(Act, Item)> {
    use super::app::{DrivePane, Focus};
    let s = keymap::Situation {
        replay: false,
        deck: false,
        view,
        focus: Focus::Tracks,
        empty: false,
        pads: false,
        drive_pane: DrivePane::List,
    };
    commands_in(view, &keymap::scopes_of(&s))
}

/// The palette's commands in `view`: the view's own first, then those for everywhere (the keymap
/// names each command once in a view, which a test checks), each with the key that does it where
/// `scopes` apply. One with no key there says where it lives and the word it's known by.
pub fn commands_in(view: View, scopes: &[Scope]) -> Vec<(Act, Item)> {
    let own: &[Scope] = match view {
        View::Library => &[Scope::Tracks, Scope::Tree, Scope::Details, Scope::Library, Scope::Crate],
        View::Prepare => &[Scope::Prepare],
        View::Drives => &[Scope::Drives],
        View::Profile => &[Scope::Profile],
        View::Mixes => &[Scope::Mixes],
    };
    let named = own
        .iter()
        .chain(&[Scope::Global])
        .flat_map(|scope| KEYMAP.iter().filter(move |b| b.scope == *scope && !b.name.is_empty()));
    // A key is shown where it does the command: not where another wins it (the digits are hot
    // cues in player mode, n skips the set's tracks in Recordings).
    let does = |act: Act, k: &keymap::K| keymap::lookup(scopes, &k.pressed()).is_some_and(|h| h.act == act);
    // The actions menu's letters run these too, with its key (none: there's no menu to name).
    let in_menu = menu_key(scopes).map_or_else(String::new, |k| format!("in {k}"));
    named
        .map(|b| {
            let key = keymap::key_for(scopes, b.act).filter(|k| does(b.act, k));
            // The word a job's known by stays findable. With no key, where else it is, if anywhere.
            let detail = match key {
                Some(k) => k.name(),
                None if prepare::holds(b.act) => format!("{} · {}", View::Prepare.title(), b.shown),
                None if !in_menu.is_empty() && !keymap::menu_letters(&[b.act])[0].is_empty() => in_menu.clone(),
                // Its key does something else here.
                None if !b.keys.is_empty() => String::new(),
                None => b.shown.to_string(),
            };
            (b.act, Item { words: b.words.to_string(), ..Item::new(b.name, detail) })
        })
        .collect()
}

/// The key that opens the actions menu where `scopes` apply: the library's, a drive's or a pane's.
fn menu_key(scopes: &[Scope]) -> Option<String> {
    [Act::Actions, Act::DriveActions, Act::PaneActions].into_iter().find_map(|act| keymap::key_name(scopes, act))
}

/// That a search comes first, with the key the library searches with.
pub(super) fn search_first() -> String {
    keymap::key_name(&[Scope::Library, Scope::Global], Act::Search)
        .map_or_else(|| "search first".into(), |k| format!("search first ({k})"))
}

impl App {
    /// `:`: every action by name, the player's looks, then plugins' commands, operations and analyzers, in the
    /// library the columns of their fields, their settings, what they suggest, and the fixes the
    /// last check found.
    pub(super) fn open_palette(&mut self) {
        // The library's actions menu, by name: what each does to what's selected, and that it's in `e`. The
        // player's own, so before the plugins'.
        let target = (self.view == View::Library).then(|| self.target()).flatten();
        let in_menu = menu_key(&keymap::scopes(self)).map_or_else(String::new, |k| format!("in {k}"));
        let edits = target.iter().flat_map(super::edits::menu_items).filter_map(|m| {
            let named = !matches!(m.action, super::edits::Action::Plugin(..) | super::edits::Action::Fetch(_));
            // Typed, "delete" is the playlist's, before the store's commands that delete.
            let words =
                if m.action == super::edits::Action::Delete { "delete, delete playlist, delete folder" } else { "" };
            let item = Item { unavailable: m.unavailable, words: words.into(), ..Item::new(m.label, in_menu.clone()) };
            named.then_some((Act::Edit(m.action), item))
        });
        let acts = commands_in(self.view, &keymap::scopes(self))
            .into_iter()
            .chain(super::looks::items())
            .chain(super::choices::items())
            .chain(edits)
            .chain(self.plugin_items(self.action_drive().as_deref(), &super::plugins::EVERYWHERE));
        let fields = (self.view == View::Library).then(|| self.field_items()).into_iter().flatten();
        let fixes = super::plugins::checks::items(self);
        let (acts, mut items): (Vec<Act>, Vec<Item>) =
            acts.chain(fields).chain(self.settings_items()).chain(self.suggested_items()).chain(fixes).unzip();
        for (act, item) in acts.iter().zip(&mut items) {
            item.unavailable = item.unavailable.take().or_else(|| self.unavailable(*act));
        }
        let mut p = Picker::new("Commands", items, Then::Act(acts));
        p.empty = "no command has those letters";
        p.rank(&self.recent_commands);
        self.modal = Some(Modal::Pick(p));
    }

    /// The palette, with `query` typed in it: what a search in the help that no key matched
    /// looks for next.
    pub(super) fn open_palette_with(&mut self, query: &str) {
        self.open_palette();
        if let Some(Modal::Pick(p)) = &mut self.modal {
            p.query = query.to_string();
            p.filter();
        }
    }

    /// `e` where a pane has no menu of its own: what enter does there, then the pane's actions
    /// (Prepare's jobs), each with its letter.
    pub(super) fn open_actions(&mut self) {
        let panes = [Scope::Prepare, Scope::Snapshots, Scope::History, Scope::Profile, Scope::Mixes];
        let scopes = keymap::scopes(self);
        let Some(&scope) = scopes.iter().find(|s| panes.contains(s)) else { return };
        let mine = || KEYMAP.iter().filter(|b| b.scope == scope);
        let enter = mine().find(|b| b.act == Act::Activate && b.hint.is_some());
        let enter_key = keymap::key_name(&dialogs::ACTIONS, Act::Activate).unwrap_or_default();
        let enter = enter.map(|b| (Act::Activate, Item::new(b.label, enter_key)));
        // What acts on the selection, or plays, isn't an action of the pane.
        let skipped =
            [Act::Activate, Act::PaneActions, Act::MixForget, Act::MixPlay, Act::MixTrack(true), Act::MixTrack(false)];
        let own = mine().filter(|b| !b.name.is_empty() && !skipped.contains(&b.act)).map(|b| b.act);
        let acts: Vec<Act> = if scope == Scope::Prepare { prepare::jobs().to_vec() } else { own.collect() };
        let letters = keymap::menu_letters(&acts);
        let named = acts.into_iter().zip(letters).map(|(act, letter)| {
            let name = match act {
                Act::ProfileAccount => self.profile.account.action(),
                _ => keymap::name(act).to_string(),
            };
            (act, Item { unavailable: self.unavailable(act), ..Item::new(name, letter) })
        });
        let (acts, items): (Vec<Act>, Vec<Item>) = enter.into_iter().chain(named).unzip();
        let title = format!("Actions · {}", self.view.title());
        self.modal = Some(Modal::Pick(Picker::actions(title, items, Then::Act(acts))));
    }

    /// Why `act` can't run now, for the palette and the Prepare view to say.
    pub(super) fn unavailable(&self, act: Act) -> Option<String> {
        let need = |cond: bool, why: &str| cond.then(|| why.to_string());
        let no_drive = "open a drive first";
        let library = need(self.loaded.is_none(), no_drive);
        let writes = |drive: bool| {
            let busy = self.job.as_ref().map(|j| format!("wait for {} to finish", j.label.to_lowercase()));
            need(!drive, no_drive).or(busy)
        };
        let silent = need(self.now.is_none(), "nothing's playing");
        // A library's row in the Drives view has its own sync, check and eject.
        let header = self.view == View::Drives && self.drive_header.is_some();
        match act {
            Act::SyncGroup | Act::SetPrep | Act::Eject if header => None,
            Act::Pull | Act::Check | Act::RestorePlaylist | Act::Replay | Act::SmartApply | Act::Run(_) if header => {
                Some("select one of the library's drives".into())
            }
            Act::Pull
            | Act::Check
            | Act::SetPrep
            | Act::SyncGroup
            | Act::Pack
            | Act::RestorePlaylist
            | Act::Replay
            | Act::Eject
            | Act::SmartApply
            | Act::Run(_) => writes(self.action_drive().is_some()),
            Act::Undo => writes(self.undo_drive().is_some()),
            Act::SmartNew => library.or(need(self.query.trim().is_empty(), &search_first())),
            Act::Actions
            | Act::Search
            | Act::Sort
            | Act::Tag
            | Act::Mark
            | Act::MarkAll
            | Act::CrateAdd
            | Act::ExportList
            | Act::Details => library,
            Act::SortReverse => library.or(need(self.sort.is_none(), "sort the list first (click a column)")),
            Act::CrateSave | Act::CrateClear => need(self.crate_list.is_empty(), "the crate is empty"),
            Act::Stop
            | Act::Seek(_)
            | Act::Next
            | Act::Prev
            | Act::Cue(_)
            | Act::Phrase(_)
            | Act::BeatJump(_)
            | Act::Loop
            | Act::LoopSize(_)
            | Act::GoTo => silent,
            _ => None,
        }
    }

    /// Keep a command the palette ran for next time, first.
    fn used(&mut self, label: String) {
        self.recent_commands.retain(|r| *r != label);
        self.recent_commands.insert(0, label);
        self.recent_commands.truncate(RECENT);
    }

    /// Keys while a list to choose from is open: typing narrows it (in a question taking several,
    /// space marks an item), the arrows move, enter chooses, esc closes it. An actions menu runs
    /// an item by its letter until `/` starts a filter; esc then clears the filter first.
    pub(super) fn on_picker_key(&mut self, mut p: Picker, key: KeyEvent) {
        // An action's letter, where nothing's being typed.
        if let Some(act) = keymap::menu_act(p.acts(), &key).filter(|_| !p.typing())
            && p.select_act(act)
        {
            return self.choose_item(p);
        }
        let scopes: &[Scope] = match (p.actions, p.typing()) {
            (false, _) => &dialogs::PICKER,
            (true, true) => &dialogs::FILTERING,
            (true, false) => &dialogs::ACTIONS,
        };
        match keymap::dialog_act(scopes, &key) {
            Some(Act::Escape) if p.actions && p.typing() => {
                (p.query, p.filtering) = (String::new(), false);
                p.filter();
            }
            Some(Act::Escape) => return,
            Some(Act::Activate) => return self.choose_item(p),
            Some(Act::Search) if p.actions => p.filtering = true,
            Some(Act::Mark) if p.marks.is_some() => p.toggle(),
            Some(act @ (Act::Move(_) | Act::Page(_) | Act::End(_))) => {
                let mut state = std::mem::take(&mut p.state);
                step_list(&mut state, p.shown.len(), act);
                p.state = state;
            }
            _ => {
                if p.typing() && edit_line(&mut p.query, key) {
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

    fn choose_item(&mut self, mut p: Picker) {
        // Those marked, whichever are shown: none is an answer too.
        if let (Some(marks), Then::Choose(asking)) = (&p.marks, &p.then) {
            let values = (marks.iter().zip(asking.choice().items.iter())).filter(|(m, _)| **m);
            let values = values.map(|(_, item)| item.value.clone()).collect();
            let Then::Choose(asking) = p.then else { unreachable!("matched above") };
            return self.answer(*asking, values);
        }
        // An import of keys takes its marked rows too, whatever's selected.
        let import = matches!(p.then, Then::Keys(_));
        let selected = p.selected();
        let why = match selected {
            None => Some(p.empty.to_string()),
            Some(i) => p.items[i].unavailable.clone(),
        };
        if let Some(why) = why.filter(|_| !import) {
            self.toast(why, super::app::Level::Info);
            self.modal = Some(Modal::Pick(p));
            return;
        }
        let i = selected.unwrap_or_default();
        match p.then {
            Then::Act(acts) => {
                // What's plugin-made and passing (a suggestion, a fix) won't be there to come back to.
                let lasting = !matches!(acts[i], Act::Review(_) | Act::Dismiss(_) | Act::Fix(_));
                if !p.recency.is_empty() && lasting {
                    self.used(p.items.swap_remove(i).label);
                }
                self.perform(acts[i], None)
            }
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
            // `preview_keys` makes it with every row marked.
            Then::Keys(import) => self.take_keys(*import, p.marks.unwrap_or_default()),
        }
    }
}

#[cfg(test)]
mod tests;
