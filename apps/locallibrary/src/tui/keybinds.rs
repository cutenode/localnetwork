//! Changing the keys: the Keys screen (`change the keys`, by name in `:`), where any action gets a
//! key by picking it and pressing the key; importing someone's published keys, by their handle;
//! the presets; putting every key back. What's set is kept in `keybinds.json` (see
//! [`locallibrary::keybinds`]) and put in effect at once (see [`keymap::effective`]); signed in,
//! it's published as the account's keybinds record a moment after the last change, and signing
//! in loads the account's.

use std::time::{Duration, Instant};

use locallibrary::Store;
use locallibrary::keybinds::{self, Keybinds};
use loclib_plugin::Manifest;
use ratatui::crossterm::event::KeyEvent;

use super::app::{App, Level, edit_line, step_list};
use super::edits::{Action, Modal};
use super::keymap::conflict::{self, Answer, Verdict};
use super::keymap::effective::{self, Effective, default_keys, own};
use super::keymap::{self, Act, Group, K, KEYMAP, dialogs, ids};
use super::palette::{Item, Picker, Then};

pub mod presets;

/// How long after the last change the keys are published, so a run of changes publishes once.
const SETTLE: Duration = Duration::from_secs(2);

/// The Keys screen while it's open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeysScreen {
    /// What's typed, narrowing the actions.
    pub query: String,
    /// The row selected, among those `query` leaves.
    pub row: usize,
    pub stage: Stage,
}

/// Where the Keys screen is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    /// Choosing an action.
    List,
    /// Waiting for the key to give `act`.
    Capture { act: Act },
    /// What that key would do, before it's kept.
    Answer(Answer),
}

/// A row of the Keys screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeysRow {
    /// None for a fixed key.
    pub act: Option<Act>,
    pub id: Option<String>,
    pub name: String,
    /// Its keys now, by name, two spaces apart; `:` when it has none (found by name, as the help
    /// says).
    pub keys: String,
    /// The keys it came with; `none` for none.
    pub default: String,
    pub changed: bool,
    /// esc, ?, : and ctrl-c, which can't change.
    pub fixed: bool,
    /// The heading it's under: the help's sections, then the plugins', then the fixed keys.
    pub group: &'static str,
}

/// Publishing the keys to the account, signed in.
#[derive(Debug, Default)]
pub struct Publishing {
    /// When the keys changed last, published once it's settled.
    pub due: Option<Instant>,
    /// A publish is under way.
    pub running: bool,
    /// The last publish failed, and why.
    pub error: Option<String>,
    /// The keys here are the account's: a publish went through, or signing in found them so.
    pub published: bool,
    /// "Sign in again" was said already this run.
    #[cfg(feature = "account")]
    scope_said: bool,
}

/// The keys that never change, and what they do.
const FIXED: [(&str, &str); 4] =
    [("esc", "close, go back"), ("?", "the help"), (":", "commands, by name"), ("ctrl-c", "quit")];

/// The heading of plugins' actions.
const PLUGINS: &str = "Plugins";
/// The heading of the keys that never change.
const FIXED_KEYS: &str = "Can't change";

/// The plugins' manifests, as the keys see them: none when they couldn't be loaded.
pub fn manifests() -> Vec<&'static Manifest> {
    crate::plugins::host().map(|p| p.manifests().collect()).unwrap_or_default()
}

/// What `act` is called on the Keys screen and in an import's list: its palette name, else its
/// hint's label, else what it does.
pub fn title(act: Act, plugins: &[&'static Manifest]) -> String {
    if let Act::Plugin(plugin, name) = act {
        let m = plugins.iter().find(|m| m.id == plugin);
        let c = m.map(|m| &m.contributes);
        let hooks = c.into_iter().flat_map(|c| {
            c.commands.iter().map(|x| (&x.name, &x.title)).chain(c.operations.iter().map(|x| (&x.name, &x.title)))
        });
        let named = hooks.into_iter().find(|(n, _)| *n == name).map(|(_, t)| t.clone());
        return match (named, m) {
            (Some(t), Some(m)) => format!("{t} ({})", m.name),
            _ => format!("{plugin}: {name}"),
        };
    }
    let own = match act {
        Act::Activate => "do what's selected: play, open, browse",
        Act::Move(-1) => "move up",
        Act::Move(1) => "move down",
        Act::Page(-1) => "a page up",
        Act::Page(1) => "a page down",
        Act::End(-1) => "the first row",
        Act::End(1) => "the last row",
        Act::Focus(true) => "previous pane",
        Act::HotCue => "hot cues A to H, in order",
        Act::Remove => "remove from the list, delete the playlist",
        Act::MixForget => "forget a recording",
        Act::PaneActions => "the pane's actions",
        Act::Browse => "browse over player mode",
        Act::Answer(true) => "yes, in a plan or a question",
        Act::Answer(false) => "no, in a plan or a question",
        Act::Change(1) => "a form's next choice",
        Act::Change(-1) => "a form's previous choice",
        Act::CancelJob => "cancel the job selected",
        Act::SearchWhere => "search the whole drive, or the list",
        Act::Edit(a) => edit_title(a),
        _ => "",
    };
    [keymap::name(act), own, keymap::label(act)]
        .into_iter()
        .find(|s| !s.is_empty())
        .map_or_else(|| ids::id(act).map(|id| id.into_owned()).unwrap_or_default(), String::from)
}

/// What one of the library's own actions is called, wherever it's run on.
fn edit_title(a: Action) -> &'static str {
    match a {
        Action::RatingMenu => "set the rating…",
        Action::Rate(0) => "rating: none",
        Action::Rate(1) => "rating: 1 star",
        Action::Rate(2) => "rating: 2 stars",
        Action::Rate(3) => "rating: 3 stars",
        Action::Rate(4) => "rating: 4 stars",
        Action::Rate(_) => "rating: 5 stars",
        Action::ColourMenu => "set the colour…",
        Action::AddToPlaylist => "add to a playlist…",
        Action::NewPlaylistWith => "new playlist with the tracks…",
        Action::RemoveFromList => "remove from the playlist shown",
        Action::SortList => "sort the playlist on the drive",
        Action::NewPlaylist => "new playlist…",
        Action::NewFolder => "new folder…",
        Action::Rename => "rename the playlist…",
        Action::Delete => "delete the playlist…",
        _ => "",
    }
}

/// The help's section `act` is under, else where the library's own actions are.
fn group(act: Act) -> Group {
    let b = KEYMAP.iter().find(|b| b.act == act && !b.help.is_empty());
    b.or_else(|| KEYMAP.iter().find(|b| b.act == act)).map_or(Group::Changing, |b| b.group)
}

/// Keys by name, two spaces apart; `empty` for none.
fn named(keys: &[K], empty: &str) -> String {
    if keys.is_empty() {
        return empty.to_string();
    }
    keys.iter().map(K::name).collect::<Vec<_>>().join("  ")
}

/// The Keys screen's rows: every action a key can be set for, under the help's headings, then the
/// plugins', then the keys that can't change. Typing narrows them to rows holding each word typed,
/// in the name, what it does, the words the palette finds it by, its id or its keys.
pub fn rows(e: &Effective, plugins: &[&'static Manifest], query: &str) -> Vec<KeysRow> {
    let mut acts = ids::bindable(plugins);
    acts.sort_by_key(|a| (matches!(a, Act::Plugin(..)), group(*a) as u8));
    let row = |act: Act| {
        let plugin = matches!(act, Act::Plugin(..));
        KeysRow {
            act: Some(act),
            id: ids::id(act).map(|id| id.into_owned()),
            name: title(act, plugins),
            keys: named(&own(&e.keys(act)), ":"),
            default: named(&own(&default_keys(act)), "none"),
            changed: e.changed(act),
            fixed: false,
            group: if plugin { PLUGINS } else { group(act).title() },
        }
    };
    let fixed = FIXED.map(|(keys, name)| KeysRow {
        act: None,
        id: None,
        name: name.into(),
        keys: keys.into(),
        default: keys.into(),
        changed: false,
        fixed: true,
        group: FIXED_KEYS,
    });
    let all = acts.into_iter().map(row).chain(fixed);
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let holds = |r: &KeysRow| {
        let found_by = r.act.map(|a| KEYMAP.iter().find(|b| b.act == a).map_or("", |b| b.words)).unwrap_or("");
        let about = r.act.map_or("", keymap::about);
        let text = format!("{} {about} {found_by} {} {}", r.name, r.id.as_deref().unwrap_or(""), r.keys).to_lowercase();
        words.iter().all(|w| text.contains(w.as_str()))
    };
    all.filter(holds).collect()
}

impl App {
    /// `change the keys`: the Keys screen, from the top.
    pub(in crate::tui) fn open_keys(&mut self) {
        self.keys = Some(KeysScreen { query: String::new(), row: 0, stage: Stage::List });
    }

    /// Keys while the Keys screen is open: typing narrows the list, the arrows move, enter waits
    /// for the key to give the action selected, and the key pressed then shows what it would do
    /// before enter keeps it. Esc goes back a step, and closes it from the list.
    pub(in crate::tui) fn on_keys_key(&mut self, key: KeyEvent) {
        let Some(mut screen) = self.keys.take() else { return };
        match screen.stage.clone() {
            Stage::List => match keymap::dialog_act(&dialogs::KEYS, &key) {
                Some(Act::Escape) => return,
                Some(Act::Activate) => {
                    let rows = rows(&effective::current(), &manifests(), &screen.query);
                    if let Some(act) = rows.get(screen.row).and_then(|r| r.act) {
                        screen.stage = Stage::Capture { act };
                    }
                }
                Some(act @ (Act::Move(_) | Act::Page(_) | Act::End(_))) => {
                    let n = rows(&effective::current(), &manifests(), &screen.query).len();
                    let mut state = ratatui::widgets::ListState::default().with_selected(Some(screen.row));
                    step_list(&mut state, n, act);
                    screen.row = state.selected().unwrap_or(0);
                }
                _ => {
                    if edit_line(&mut screen.query, key) {
                        screen.row = 0;
                    }
                }
            },
            Stage::Capture { act } => match K::of(&key) {
                Some(K::Code(ratatui::crossterm::event::KeyCode::Esc)) => screen.stage = Stage::List,
                Some(k) => screen.stage = Stage::Answer(conflict::answer(&effective::current(), act, k)),
                None => self.toast("the player doesn't take that key (alt- keys aren't kept)", Level::Info),
            },
            Stage::Answer(a) => match keymap::dialog_act(&dialogs::KEYS_ANSWER, &key) {
                Some(Act::Escape) => screen.stage = Stage::List,
                Some(Act::Activate) if matches!(a.verdict, Verdict::Refused(_)) => {
                    screen.stage = Stage::Capture { act: a.act };
                }
                Some(Act::Activate) => {
                    let mut binds = self.keybinds.clone();
                    conflict::apply(&mut binds, &a);
                    self.set_keybinds(binds);
                    screen.stage = Stage::List;
                }
                _ => {}
            },
        }
        self.keys = Some(screen);
    }

    /// Keep `binds` as the keys: written to `keybinds.json` (without what's as it came), in effect
    /// at once, and, signed in, published once the changes settle.
    pub(in crate::tui) fn set_keybinds(&mut self, mut binds: Keybinds) {
        let plugins = manifests();
        binds.keys.retain(|id, keys| ids::act(id, &plugins).is_none_or(|act| !as_it_came(act, keys)));
        binds.updated_at = Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
        if self.keybinds_unreadable {
            self.toast("keybinds.json can't be read, so the keys aren't saved over it", Level::Error);
        } else if let Err(e) = keybinds::save(&self.store_root, &binds) {
            self.toast(format!("the keys weren't saved: {e}"), Level::Error);
        }
        effective::install(Effective::new(&binds, &plugins));
        self.keybinds = binds;
        if self.profile.account.signed_in() {
            self.publishing.due = Some(Instant::now() + SETTLE);
        }
    }

    /// At the start: the keys plugins' settings had move to `keybinds.json`, which is put in
    /// effect, saying what of it can't be. One that can't be read leaves the keys as they came,
    /// and is never saved over.
    pub(in crate::tui) fn load_keybinds(&mut self) {
        if let Ok(store) = Store::open(&self.store_root) {
            match keybinds::migrate::plugin_keys(&store) {
                Ok(dropped) => {
                    for line in dropped {
                        self.toast(format!("a plugin's key wasn't moved to keybinds.json: {line}"), Level::Error);
                    }
                }
                Err(e) => self.toast(format!("plugins' keys weren't moved to keybinds.json: {e}"), Level::Error),
            }
        }
        match keybinds::load(&self.store_root) {
            Ok(binds) => {
                let e = Effective::new(&binds, &manifests());
                for line in &e.problems {
                    self.toast(format!("keybinds.json: {line}"), Level::Error);
                }
                effective::install(e);
                self.keybinds = binds;
            }
            Err(e) => {
                self.keybinds_unreadable = true;
                self.toast(format!("the keys are as they came: {e}"), Level::Error);
            }
        }
    }

    /// Every key back as it came, after asking, listing what changes.
    pub(in crate::tui) fn reset_keys(&mut self) {
        let e = effective::current();
        let plugins = manifests();
        let changed: Vec<String> = (ids::bindable(&plugins).into_iter().filter(|a| e.changed(*a)))
            .map(|a| {
                format!(
                    "{}: {} → {}",
                    title(a, &plugins),
                    named(&own(&e.keys(a)), "none"),
                    named(&own(&default_keys(a)), "none")
                )
            })
            .collect();
        if changed.is_empty() {
            return self.toast("every key is as it came", Level::Info);
        }
        let question = format!("Put the keys of {} back as they came? {}", plural(changed.len()), changed.join("; "));
        self.modal = Some(Modal::Ask(
            question,
            Box::new(|app: &mut App| {
                app.set_keybinds(Keybinds::default());
                app.toast("every key is as it came", Level::Info);
            }),
        ));
    }

    /// The import list for `theirs` (from `from`, a DID, when someone published them): every
    /// action whose keys they'd change, each marked, `yours → theirs`. Enter takes those marked.
    /// `skipped` is how many of their bindings couldn't be read; with their actions this version
    /// doesn't know, the list says how many it leaves out.
    pub(in crate::tui) fn preview_keys(
        &mut self,
        title_line: String,
        theirs: Keybinds,
        from: Option<String>,
        skipped: usize,
    ) {
        let e = effective::current();
        let plugins = manifests();
        let unknown = theirs.keys.keys().filter(|id| ids::act(id, &plugins).is_none()).count();
        let theirs_effective = Effective::new(&theirs, &plugins);
        let differ: Vec<(Act, Item)> = ids::bindable(&plugins)
            .into_iter()
            .filter_map(|act| {
                let (mine, given) = (own(&e.keys(act)), own(&theirs_effective.keys(act)));
                let detail = format!("{} → {}", conflict::written(&mine), conflict::written(&given));
                (mine != given).then(|| (act, Item::new(title(act, &plugins), detail)))
            })
            .collect();
        if differ.is_empty() {
            return self.toast("those keys are the ones you have", Level::Info);
        }
        let (acts, items): (Vec<Act>, Vec<Item>) = differ.into_iter().unzip();
        let import = KeysImport { theirs, from, acts, left_out: skipped + unknown };
        let mut p = Picker::new(title_line, items, Then::Keys(Box::new(import)));
        p.marks = Some(vec![true; p.items.len()]);
        self.modal = Some(Modal::Pick(p));
    }

    /// Take the marked rows of an import: each action gets their keys (or its own back, where
    /// they had it as it came), after asking. Taken without another row that holds a key one of
    /// them would double, it says which and takes nothing.
    pub(in crate::tui) fn take_keys(&mut self, import: KeysImport, marked: Vec<bool>) {
        let plugins = manifests();
        let mut binds = self.keybinds.clone();
        let mut lines = Vec::new();
        for (act, _) in import.acts.iter().zip(&marked).filter(|(_, m)| **m) {
            let Some(id) = ids::id(*act) else { continue };
            match import.theirs.keys.get(id.as_ref()) {
                Some(keys) => binds.keys.insert(id.to_string(), keys.clone()),
                None => binds.keys.shift_remove(id.as_ref()),
            };
            lines.push(title(*act, &plugins));
        }
        if lines.is_empty() {
            return self.toast("nothing marked: no key changed", Level::Info);
        }
        if let Some(problem) = Effective::new(&binds, &plugins).problems.first() {
            return self.toast(format!("{problem}: mark that one's row too, or leave this one"), Level::Error);
        }
        binds.from = import.from.clone().or(binds.from);
        let question = format!("Change the keys of {}? {}", plural(lines.len()), lines.join(", "));
        self.modal = Some(Modal::Ask(
            question,
            Box::new(move |app: &mut App| {
                app.set_keybinds(binds);
                app.toast("keys changed", Level::Info);
            }),
        ));
    }
}

/// Whether `spelled` are the keys `act` came with.
fn as_it_came(act: Act, spelled: &[String]) -> bool {
    let read: Vec<K> = spelled.iter().filter_map(|s| keybinds::Key::parse(s).ok()).map(K::from).collect();
    read.len() == spelled.len() && read == own(&default_keys(act))
}

fn plural(n: usize) -> String {
    if n == 1 { "1 action".into() } else { format!("{n} actions") }
}

/// What choosing in an import's list takes: their keys, and the action each row is.
#[derive(Debug)]
pub struct KeysImport {
    pub theirs: Keybinds,
    /// Their DID, for credit.
    pub from: Option<String>,
    pub acts: Vec<Act>,
    /// How many of theirs this version can't take: actions it doesn't know, keys it can't read.
    pub left_out: usize,
}

#[cfg(feature = "account")]
mod account;
#[cfg(feature = "account")]
pub use account::KeysEvent;

#[cfg(test)]
mod tests;
