//! Whether a key can go to an action: where the player is (a view, a pane, player mode, a dialog)
//! decides what a key does, so a key set for an action is checked in every such place. A key
//! another action has there is offered as a swap, one two others have is refused, and none is
//! ever kept for two. Where a view takes a key everywhere has, as it came (the digits are hot cues
//! in player mode), that stays its own.

use std::sync::LazyLock;

use locallibrary::keybinds::{Key, Keybinds};

use super::effective::{Effective, default_keys, own};
use super::{Act, K, Scope, Situation, dialogs, ids, label, scopes_of};
use crate::tui::app::{DrivePane, Focus, View};

/// Somewhere the player can be, and the scopes keys are looked up in there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub name: &'static str,
    pub scopes: Vec<Scope>,
}

/// Every place, each set of scopes once: the views in each of their situations, then the dialogs.
pub fn places() -> &'static [Place] {
    static PLACES: LazyLock<Vec<Place>> = LazyLock::new(|| {
        let mut out: Vec<Place> = Vec::new();
        let focuses = [Focus::Drives, Focus::Playlists, Focus::Tracks, Focus::Details, Focus::Crate];
        let panes = [DrivePane::List, DrivePane::Snapshots, DrivePane::History];
        for view in View::ALL {
            for (replay, deck, pads, empty) in (0..16).map(|n| (n & 1 > 0, n & 2 > 0, n & 4 > 0, n & 8 > 0)) {
                for (focus, drive_pane) in focuses.into_iter().flat_map(|f| panes.map(|p| (f, p))) {
                    let deck = deck && view == View::Library;
                    let s = Situation { replay, deck, view, focus, empty, pads: pads || deck, drive_pane };
                    let scopes = scopes_of(&s);
                    if !out.iter().any(|p| p.scopes == scopes) {
                        // The replay offer is up over a view, which names it: every situation
                        // has its view's scope, so one always does.
                        let name = scopes.iter().find_map(|s| place_name(*s)).unwrap_or("everywhere");
                        out.push(Place { name, scopes });
                    }
                }
            }
        }
        out.extend(DIALOGS.iter().map(|(name, scopes)| Place { name, scopes: scopes.to_vec() }));
        out
    });
    &PLACES
}

/// Every dialog, by what it's called where a key's effects are listed.
const DIALOGS: [(&str, [Scope; 3]); 17] = [
    ("the actions menu", dialogs::ACTIONS),
    ("the actions menu's filter", dialogs::FILTERING),
    ("the rating or colour menu", dialogs::SUB_MENU),
    ("a plan", dialogs::CONFIRM),
    ("a question", dialogs::ASK),
    ("results", dialogs::REPORT),
    ("a form", dialogs::FORM),
    ("a list", dialogs::PICKER),
    ("the jobs", dialogs::JOBS),
    ("the help", dialogs::HELP),
    ("the messages", dialogs::MESSAGES),
    ("the outputs", dialogs::OUTPUTS),
    ("the search", dialogs::SEARCH),
    ("a prompt", dialogs::PROMPT),
    ("picking a playlist", dialogs::PICKING),
    ("the keys", dialogs::KEYS),
    ("a key's answer", dialogs::KEYS_ANSWER),
];

/// What a place is called by scope `s`, the most specific of its scopes that names one: none for
/// the replay offer, which is up over a view.
fn place_name(s: Scope) -> Option<&'static str> {
    Some(match s {
        Scope::Deck => "player mode",
        Scope::DriveList => "the drive list",
        Scope::Steps => "the next steps",
        Scope::Tree => "the playlists",
        Scope::Details => "the details",
        Scope::Crate => "the queue",
        Scope::Pads => "pads on",
        Scope::Tracks | Scope::Library => "Library",
        Scope::Prepare => "Prepare",
        Scope::Drives => "Drives",
        Scope::Snapshots => "Snapshots",
        Scope::History => "History",
        Scope::Profile => "localnetwork",
        Scope::Mixes => "Recordings",
        _ => return None,
    })
}

/// A key an action has where it's at home that does another there: `owner` doesn't get it,
/// `taker` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clash {
    /// Its index in [`places`].
    pub place: usize,
    pub owner: Act,
    pub taker: Act,
    pub key: K,
    /// The taker has it where it's at home, so it can give it up.
    pub swappable: bool,
}

/// Every clash in `e`, as it came or not.
fn all_clashes(e: &Effective) -> Vec<Clash> {
    let mut out = Vec::new();
    for (place, p) in places().iter().enumerate() {
        for i in p.scopes.iter().flat_map(|s| e.in_scope(*s)) {
            let row = &e.rows()[i];
            for key in row.keys.iter().filter(|_| row.home) {
                let Some(hit) = e.hit(&p.scopes, &key.pressed()) else { continue };
                let taker = &e.rows()[hit];
                if taker.act != row.act {
                    out.push(Clash { place, owner: row.act, taker: taker.act, key: *key, swappable: taker.home });
                }
            }
        }
    }
    out
}

/// Where a view takes, as it came, the keys of an action it does its own way (space plays the set
/// in Recordings, n and p skip its tracks; the digits are hot cues in player mode): the places,
/// and whose keys. There the action isn't reached by its keys, whichever they are.
fn as_it_came() -> &'static [(usize, Act)] {
    static CAME: LazyLock<Vec<(usize, Act)>> = LazyLock::new(|| {
        let mut out: Vec<(usize, Act)> = Vec::new();
        for c in all_clashes(&Effective::defaults()) {
            if !out.contains(&(c.place, c.owner)) {
                out.push((c.place, c.owner));
            }
        }
        out
    });
    &CAME
}

/// The clashes in `e`, but where an action isn't reached as it came, and the hot cues' keys with
/// the pads on: the pads take the keys they're given there, as they take the digits.
pub fn clashes(e: &Effective) -> Vec<Clash> {
    let came = as_it_came();
    let kept = |c: &Clash| c.taker != Act::HotCue && !came.contains(&(c.place, c.owner));
    all_clashes(e).into_iter().filter(kept).collect()
}

/// What an action is called in a line about its key: its hint's label, else what the Keys screen
/// calls it.
pub fn short(act: Act) -> String {
    match label(act) {
        "" => crate::tui::keybinds::title(act, &crate::tui::keybinds::manifests()),
        said => said.into(),
    }
}

/// What a key did somewhere, and does with the change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effect {
    /// The places (each once), or `everywhere` for every view.
    pub places: Vec<&'static str>,
    pub before: Option<Act>,
    pub after: Option<Act>,
}

/// What pressing `key` for `act` comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// `act` gets the key.
    Save,
    /// The key is one `act` came with (or, for one that came with none, one it has now): it gets
    /// the keys it came with back.
    Reset,
    /// Another action has the key: `act` takes it, and `with` gets `gets` in its place (none when
    /// `act` had none).
    Swap { with: Act, gets: Vec<K> },
    /// It can't, and why.
    Refused(String),
}

/// The answer to a key pressed for an action, before it's kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub act: Act,
    pub key: K,
    pub effects: Vec<Effect>,
    pub verdict: Verdict,
}

/// What giving `act` the key `key` would do in `e`.
pub fn answer(e: &Effective, act: Act, key: K) -> Answer {
    let refused = |why: String| Answer { act, key, effects: Vec::new(), verdict: Verdict::Refused(why) };
    if let Some(why) = refusal(act, key) {
        return refused(why);
    }
    let came = own(&default_keys(act));
    let has = own(&e.keys(act));
    // An action that came with no key has none to press to go back: the key it has now takes it
    // back to none.
    let unbinds = came.is_empty() && has.contains(&key);
    let keys = if unbinds || came.contains(&key) { came.clone() } else { vec![key] };
    let after = e.with(act, &keys);
    let mut others: Vec<Clash> = Vec::new();
    for c in clashes(&after) {
        let other = if c.owner == act { c.taker } else { c.owner };
        if !others.iter().any(|o| o.owner == other) {
            others.push(Clash { owner: other, swappable: c.owner != act || c.swappable, ..c });
        }
    }
    let mut swapped = None;
    let verdict = match &others[..] {
        [] if keys == came => Verdict::Reset,
        [] => Verdict::Save,
        [c] if c.swappable => {
            let given: Vec<K> = own(&e.keys(act));
            let mut gets: Vec<K> = own(&after.keys(c.owner)).into_iter().filter(|k| !keys.contains(k)).collect();
            for k in given {
                if !gets.contains(&k) {
                    gets.push(k);
                }
            }
            let both = after.with(c.owner, &gets);
            match clashes(&both).is_empty() {
                true => {
                    swapped = Some(both);
                    Verdict::Swap { with: c.owner, gets }
                }
                false => Verdict::Refused(format!(
                    "{} is {}'s, which can't take {} in its place: pick another key",
                    key.name(),
                    short(c.owner),
                    written(&own(&e.keys(act)))
                )),
            }
        }
        [c] => Verdict::Refused(format!(
            "{} is {} in {}: pick another key",
            key.name(),
            short(c.owner),
            places()[c.place].name
        )),
        _ => {
            let names: Vec<String> = others.iter().map(|c| short(c.owner)).collect();
            Verdict::Refused(format!(
                "{} does {} things here: {}. Pick another key",
                key.name(),
                names.len(),
                names.join(", ")
            ))
        }
    };
    // Going back to none, the keys it gives up are what changes.
    let effects = effects(e, swapped.as_ref().unwrap_or(&after), if unbinds { &has } else { &keys });
    Answer { act, key, effects, verdict }
}

/// Why `key` can't go to `act` whatever the others have, if it can't.
fn refusal(act: Act, key: K) -> Option<String> {
    let Some(portable) = key.portable() else {
        return Some(match key {
            K::Code(ratatui::crossterm::event::KeyCode::Esc) => "esc always closes".into(),
            _ => format!("{} can't be kept as a key", key.name()),
        });
    };
    if let Err(why) = Key::parse(&portable.to_string()) {
        return Some(why);
    }
    // A key that types something stays out of the dialogs it would type in.
    let typed = DIALOGS.iter().find(|(_, scopes)| scopes[1] == Scope::Typed && at_home_in(act, scopes[0]));
    match (typed, key) {
        (Some((place, _)), K::Ch(_)) if !default_keys(act).contains(&key) => {
            Some(format!("characters type in {place}"))
        }
        _ => None,
    }
}

/// Whether `act` is at home in scope `s`, as the keys came.
fn at_home_in(act: Act, s: Scope) -> bool {
    let e = Effective::defaults();
    e.in_scope(s).any(|i| e.rows()[i].act == act && e.rows()[i].home)
}

/// What each of `keys` does in each place before and after, where that changes, the places with
/// the same change together.
fn effects(before: &Effective, after: &Effective, keys: &[K]) -> Vec<Effect> {
    let views: Vec<&'static str> = names(places().iter().filter(|p| !p.scopes[0].dialog()));
    let mut out: Vec<Effect> = Vec::new();
    for p in places() {
        for key in keys {
            let does = |e: &Effective| e.hit(&p.scopes, &key.pressed()).map(|i| e.rows()[i].act);
            let (was, will) = (does(before), does(after));
            if was == will {
                continue;
            }
            match out.iter_mut().find(|x| x.before == was && x.after == will) {
                Some(x) if !x.places.contains(&p.name) => x.places.push(p.name),
                Some(_) => {}
                None => out.push(Effect { places: vec![p.name], before: was, after: will }),
            }
        }
    }
    for x in &mut out {
        if views.iter().all(|v| x.places.contains(v)) {
            x.places.retain(|p| !views.contains(p));
            x.places.insert(0, "everywhere");
        }
    }
    out
}

/// The names of `places`, each once.
fn names<'a>(places: impl Iterator<Item = &'a Place>) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for p in places {
        if !out.contains(&p.name) {
            out.push(p.name);
        }
    }
    out
}

/// Keys as a line about them writes them: by name, apart; `nothing` for none.
pub fn written(keys: &[K]) -> String {
    match keys {
        [] => "nothing".into(),
        _ => keys.iter().map(K::name).collect::<Vec<_>>().join(" "),
    }
}

/// `binds` with what `a` says kept: `act`'s keys, and the other's in a swap. Keys an action came
/// with aren't kept for it, so it has them back.
pub fn apply(binds: &mut Keybinds, a: &Answer) {
    let mut keep = |act: Act, keys: &[K]| {
        let Some(id) = ids::id(act) else { return };
        let keys = own(keys);
        if keys == own(&default_keys(act)) {
            binds.keys.shift_remove(id.as_ref());
        } else {
            let spelled = keys.iter().filter_map(|k| k.portable()).map(|k| k.to_string()).collect();
            binds.keys.insert(id.into_owned(), spelled);
        }
    };
    let mine = |key: K| {
        let came = own(&default_keys(a.act));
        if came.contains(&key) { came } else { vec![key] }
    };
    match &a.verdict {
        Verdict::Save => keep(a.act, &mine(a.key)),
        // A reset is the keys it came with, none included, whichever key asked for it.
        Verdict::Reset => keep(a.act, &own(&default_keys(a.act))),
        Verdict::Swap { with, gets } => {
            keep(a.act, &mine(a.key));
            keep(*with, gets);
        }
        Verdict::Refused(_) => {}
    }
}

#[cfg(test)]
mod tests;
