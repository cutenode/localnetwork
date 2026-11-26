//! The keys in effect: the keymap's, with the keys set in `keybinds.json` over them. Every reader
//! of the keymap (looking a key up, the hints, the help, the palette, the menus' letters) reads
//! these, so a changed key shows wherever keys are shown. They're kept per thread, as the theme
//! is: the player's own thread installs them, and each test starts on the defaults.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::LazyLock;

use rustc_hash::FxHashMap;

use locallibrary::keybinds::{Key, Keybinds};
use loclib_plugin::Manifest;
use ratatui::crossterm::event::KeyEvent;

use super::{Act, BY_NAME, Binding, K, KEYMAP, Scope, conflict, ids, is_tip, written};

/// A row of the keys in effect: one of the keymap's, or a plugin's command or one of the
/// library's actions (keyless until a key is set for it).
#[derive(Debug, Clone)]
pub struct Row {
    pub scope: Scope,
    pub act: Act,
    pub keys: Vec<K>,
    /// The keymap's row it comes from; none for a plugin's or the library's own.
    pub b: Option<&'static Binding>,
    /// Its keys are set in `keybinds.json`.
    pub changed: bool,
    /// Where its act is at home: a key set for the act goes on every such row (see [`homes`]).
    pub home: bool,
}

/// The keys in effect.
#[derive(Debug, Clone)]
pub struct Effective {
    rows: Vec<Row>,
    /// The rows of each scope, by the scope's place in [`Scope`].
    by_scope: Vec<Vec<usize>>,
    /// What `keybinds.json` asked that couldn't be done, one line each.
    pub problems: Vec<String>,
}

/// The keys as they came, with no plugin's.
static DEFAULTS: LazyLock<Effective> = LazyLock::new(|| Effective::build(&[]));

thread_local! {
    static CURRENT: RefCell<Rc<Effective>> = RefCell::new(Rc::new(Effective::defaults()));
    /// What's drawn from the keys in effect, the last of each kind made (see [`memo`]).
    static MEMO: RefCell<FxHashMap<TypeId, Box<dyn Any>>> = RefCell::new(FxHashMap::default());
}

/// Use `e` from now on, on this thread: what was drawn from the keys before goes.
pub fn install(e: Effective) {
    CURRENT.with(|c| *c.borrow_mut() = Rc::new(e));
    MEMO.with(|m| m.borrow_mut().clear());
}

/// What `make` makes from the keys in effect, for the key `same` matches: made once, and the same
/// one handed back while the keys and that key stay as they are. A frame draws the tabs, the hints
/// and the help from the keys, and most frames draw them as the last one did, so fetching them then
/// looks nothing up and allocates nothing. One is kept of each kind (`K` and `V`), the last made;
/// `key` makes the key to keep only when it's made.
pub fn memo<K: 'static, V: 'static>(
    same: impl FnOnce(&K) -> bool,
    key: impl FnOnce() -> K,
    make: impl FnOnce() -> V,
) -> Rc<V> {
    let id = TypeId::of::<(K, V)>();
    let kept = MEMO.with(|m| {
        let m = m.borrow();
        let (k, v) = m.get(&id)?.downcast_ref::<(K, Rc<V>)>()?;
        same(k).then(|| v.clone())
    });
    if let Some(v) = kept {
        return v;
    }
    // Made with the cache let go: making one may look another up.
    let v = Rc::new(make());
    let kept: Box<dyn Any> = Box::new((key(), v.clone()));
    MEMO.with(|m| m.borrow_mut().insert(id, kept));
    #[cfg(test)]
    MADE.with(|n| n.set(n.get() + 1));
    v
}

#[cfg(test)]
thread_local! {
    static MADE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many times [`memo`] has made something on this thread.
#[cfg(test)]
pub fn made() -> usize {
    MADE.with(std::cell::Cell::get)
}

/// Let go of what's kept drawn, so it's made anew.
#[cfg(test)]
pub fn forget() {
    MEMO.with(|m| m.borrow_mut().clear());
}

/// The keys in effect on this thread.
pub fn current() -> Rc<Effective> {
    CURRENT.with(|c| c.borrow().clone())
}

/// The keys `act` came with.
pub fn default_keys(act: Act) -> Vec<K> {
    DEFAULTS.keys(act)
}

/// The keys `act` came with, as `keybinds.json` would spell them (ctrl-c, which always quits,
/// left out).
pub fn own(keys: &[K]) -> Vec<K> {
    keys.iter().copied().filter(|k| *k != K::Ctrl('c')).collect()
}

impl Effective {
    /// The keys as they came.
    pub fn defaults() -> Effective {
        DEFAULTS.clone()
    }

    /// The keymap's rows (a run of digits as a key each), then a keyless row for each of the
    /// plugins' commands and operations (everywhere) and for each of the library's own actions.
    fn build(plugins: &[&'static Manifest]) -> Effective {
        let expand = |keys: &[K]| -> Vec<K> {
            keys.iter()
                .flat_map(|k| match *k {
                    K::Digits(a, b) => (a..=b).map(K::Ch).collect(),
                    k => vec![k],
                })
                .collect()
        };
        let keymap = KEYMAP.iter().map(|b| Row {
            scope: b.scope,
            act: b.act,
            keys: expand(b.keys),
            b: Some(b),
            changed: false,
            home: b.act != Act::None && !b.scope.dialog() && !is_tip(b),
        });
        let keyless =
            |scope: Scope| move |act: Act| Row { scope, act, keys: Vec::new(), b: None, changed: false, home: true };
        let plugin_acts = ids::bindable(plugins).into_iter().filter(|a| matches!(a, Act::Plugin(..)));
        let mut rows: Vec<Row> = keymap
            .chain(plugin_acts.map(keyless(Scope::Global)))
            .chain(ids::edits().map(keyless(Scope::Library)))
            .collect();
        // An act only dialogs have (yes, no) is at home in them.
        let homeless: Vec<Act> = rows
            .iter()
            .filter(|r| r.b.is_some_and(|b| !is_tip(b)) && ids::id(r.act).is_some())
            .map(|r| r.act)
            .filter(|act| !rows.iter().any(|r| r.act == *act && r.home))
            .collect();
        for r in rows.iter_mut().filter(|r| homeless.contains(&r.act) && r.b.is_some_and(|b| !is_tip(b))) {
            r.home = true;
        }
        // One for every scope, the last of them included.
        let mut by_scope = vec![Vec::new(); Scope::KeysRefused as usize + 1];
        for (i, r) in rows.iter().enumerate() {
            by_scope[r.scope as usize].push(i);
        }
        Effective { rows, by_scope, problems: Vec::new() }
    }

    /// The keys with `binds` over them, for `plugins`. A key that can't be read, or that another
    /// action has already (the one that came with it, else the first in the file), is left out
    /// and said in [`Effective::problems`]; an action this version doesn't know is left alone.
    pub fn new(binds: &Keybinds, plugins: &[&'static Manifest]) -> Effective {
        let mut e = Effective::build(plugins);
        let mut set: Vec<(Act, String)> = Vec::new();
        for (id, spelled) in &binds.keys {
            let Some(act) = ids::act(id, plugins) else { continue };
            let mut keys = Vec::new();
            for s in spelled {
                match Key::parse(s) {
                    Ok(k) => keys.push(K::from(k)),
                    Err(why) => e.problems.push(format!("{id}: {why}")),
                }
            }
            // With none it can read, it keeps the keys it came with.
            if keys.is_empty() && !spelled.is_empty() {
                continue;
            }
            e.set(act, &keys);
            set.push((act, id.clone()));
        }
        let at = |act: Act| set.iter().position(|(a, _)| *a == act);
        // Each turn takes a key away, so it ends.
        for _ in 0..e.rows.len() {
            // A key one of the two came with goes first, so a line never names as the key's an
            // action that later loses it.
            let mut clashes = conflict::clashes(&e);
            clashes.sort_by_key(|c| at(c.owner).is_some() && at(c.taker).is_some());
            // Of two that have one key, the one set later loses it. The taker loses its own key
            // that the owner's press reaches, which can be another (↑ takes shift ↑ too), so
            // taking `c.key` from it could leave it as it was. Two unset ones never clash here: only
            // set actions' rows change, so an unset owner's press reaches what it did as the keys
            // came, or a set action's row, and `clashes` leaves out an owner that clashed then.
            let later = clashes.into_iter().find_map(|c| {
                let owner = at(c.owner);
                match at(c.taker) {
                    Some(t) if owner.is_none_or(|o| o <= t) => {
                        e.keys(c.taker).into_iter().find(|k| k.matches(&c.key.pressed())).map(|k| (t, c.owner, k))
                    }
                    _ => owner.map(|o| (o, c.taker, c.key)),
                }
            });
            let Some((loser, kept_by, key)) = later else { break };
            let (act, id) = set[loser].clone();
            let keys: Vec<K> = e.keys(act).into_iter().filter(|k| *k != key).collect();
            e.set(act, &own(&keys));
            e.problems.push(format!("{id}: {} is {}'s already", key.name(), conflict::short(kept_by)));
        }
        e
    }

    /// These keys, with `keys` for `act` (in place of the ones it has).
    pub fn with(&self, act: Act, keys: &[K]) -> Effective {
        let mut e = self.clone();
        e.set(act, keys);
        e
    }

    /// Give `act` `keys` wherever it's at home: quitting keeps ctrl-c.
    fn set(&mut self, act: Act, keys: &[K]) {
        let keys: Vec<K> = match act {
            Act::Quit => [K::Ctrl('c')].into_iter().chain(own(keys)).collect(),
            _ => keys.to_vec(),
        };
        for r in self.rows.iter_mut().filter(|r| r.act == act && r.home) {
            r.keys = keys.clone();
            r.changed = true;
        }
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The rows of scope `s`, in order.
    pub fn in_scope(&self, s: Scope) -> impl Iterator<Item = usize> + '_ {
        self.by_scope[s as usize].iter().copied()
    }

    /// The row `key` means in `scopes` (most specific first): the first in its scope.
    pub fn hit(&self, scopes: &[Scope], key: &KeyEvent) -> Option<usize> {
        scopes.iter().find_map(|s| self.in_scope(*s).find(|i| self.rows[*i].keys.iter().any(|k| k.matches(key))))
    }

    /// The row that gives `act` its key in the first of `scopes` that has one.
    pub fn key_row(&self, scopes: &[Scope], act: Act) -> Option<usize> {
        let keyed = |i: &usize| self.rows[*i].act == act && !self.rows[*i].keys.is_empty();
        scopes.iter().find_map(|s| self.in_scope(*s).find(keyed))
    }

    /// `act`'s keys where it's at home (the first such row with any).
    pub fn keys(&self, act: Act) -> Vec<K> {
        let home = self.rows.iter().filter(|r| r.act == act && r.home);
        home.map(|r| &r.keys).find(|k| !k.is_empty()).cloned().unwrap_or_default()
    }

    /// Whether `act`'s keys are set in `keybinds.json`.
    pub fn changed(&self, act: Act) -> bool {
        self.rows.iter().any(|r| r.act == act && r.changed)
    }

    /// The second of the pair row `i` starts (`p` after `n`): the next row, in its scope, that says
    /// nothing of itself and does another action a key can be set for.
    fn partner(&self, i: usize) -> Option<usize> {
        let (row, next) = (&self.rows[i], self.rows.get(i + 1)?);
        let quiet = next.b.is_some_and(|b| b.shown.is_empty() && b.help.is_empty() && !is_tip(b));
        let pair = next.scope == row.scope && next.act != row.act && quiet && ids::id(next.act).is_some();
        pair.then_some(i + 1)
    }

    /// Whether the keys a hint for `act` in `scopes` writes out (`n p`) have changed.
    pub fn pair_changed(&self, scopes: &[Scope], act: Act) -> bool {
        let Some(j) = self.key_row(scopes, act) else { return self.changed(act) };
        self.rows[j].changed || self.partner(j).is_some_and(|p| self.rows[p].changed)
    }

    /// The keys a hint for `act` in `scopes` writes out, as they are now: its key and its pair's,
    /// apart by `gap`, or the first and last of the hot cues.
    pub fn pair_written(&self, scopes: &[Scope], act: Act, gap: &str) -> Option<String> {
        let j = self.key_row(scopes, act)?;
        if act == Act::HotCue {
            return Some(run(&self.rows[j].keys));
        }
        let partner = self.partner(j).and_then(|p| self.rows[p].keys.first());
        Some(self.rows[j].keys[..1].iter().chain(partner).map(K::name).collect::<Vec<_>>().join(gap))
    }

    /// Whether row `i` has no key, so the help says it's found by name in `:`.
    pub fn by_name(&self, i: usize) -> bool {
        let row = &self.rows[i];
        let partner = self.partner(i);
        let changed = row.changed || partner.is_some_and(|p| self.rows[p].changed);
        match row.b.filter(|_| !changed) {
            Some(b) => super::by_name(b),
            None => self.keyless(i) && !super::name(row.act).is_empty(),
        }
    }

    /// Whether row `i` and its pair have no key now.
    fn keyless(&self, i: usize) -> bool {
        let row = &self.rows[i];
        row.act != Act::None && row.keys.is_empty() && self.partner(i).is_none_or(|p| self.rows[p].keys.is_empty())
    }

    /// How the help writes row `i`'s keys: as the keymap does while they and their pair's are as
    /// they came, else each of its keys and then its pair's first (the first and last hot cue's).
    /// With none, `:` when it's found by name there, else `none`, as the Keys screen says.
    pub fn written(&self, i: usize) -> String {
        let row = &self.rows[i];
        let partner = self.partner(i);
        let changed = row.changed || partner.is_some_and(|p| self.rows[p].changed);
        if let Some(b) = row.b.filter(|_| !changed) {
            return written(b).to_string();
        }
        if self.keyless(i) {
            return if self.by_name(i) { BY_NAME } else { "none" }.to_string();
        }
        if row.act == Act::HotCue {
            return run(&row.keys);
        }
        let mut keys: Vec<String> = row.keys.iter().map(K::name).collect();
        keys.extend(partner.and_then(|p| self.rows[p].keys.first()).map(K::name));
        keys.join("  ")
    }
}

/// A run of keys as a hint writes it: the first and the last, or the one.
fn run(keys: &[K]) -> String {
    match keys {
        [first, .., last] => format!("{}…{}", first.name(), last.name()),
        _ => keys.iter().map(K::name).collect(),
    }
}

#[cfg(test)]
mod tests;
