//! Restoring access: signing out of an account, or its session ending, takes every plugin's access
//! to it, which the store remembers until signing in again restores it (as the sign-in says, else
//! as Automatically Restore Access does, and the plugin it's for always), the user restores all of
//! it or what they choose, or resets it.

use hegel::TestCase;
use hegel::generators as gs;
use indexmap::IndexMap;
use locallibrary::error::Reason;
use locallibrary::plugin::{Access, Chosen, Restoring};

use super::accesses::account;
use super::shared_accounts::{Losing, Mock, WITH, asking, mock_among, options, who};
use super::*;
use crate::mock_oauth::account_plugin;

/// The plugins given access here: echo, and another like it.
const PLUGINS: [&str; 2] = ["echo", "other"];

/// The accounts signed in to, by subject.
const SUBJECTS: [&str; 2] = ["a", "b"];

/// The mock service, its account plugin, echo and `other`, each needing `read` of a mock account
/// and maybe `write`.
fn two_plugins() -> Mock {
    let echo = asking(&["read"], &["write"]);
    let other = echo.replace("id = \"echo\"", "id = \"other\"");
    let manifests = move |server: &str| vec![account_plugin(server, &format!("{server}/userinfo"), ""), other];
    mock_among(3600, manifests, &echo, options())
}

/// An access as the model has it: its id, its scopes (sorted), and whether it's the plugin's
/// default.
#[derive(Debug, Clone, PartialEq)]
struct Had {
    id: String,
    scopes: Vec<String>,
    default: bool,
}

/// Accesses by plugin and account, by their places in [`PLUGINS`] and [`SUBJECTS`], in any order.
type Book = IndexMap<(usize, usize), Had>;

/// `scopes` and `more`, sorted, once each.
fn joined(scopes: &[String], more: &[&str]) -> Vec<String> {
    let mut all: Vec<String> = scopes.iter().cloned().chain(more.iter().map(|s| s.to_string())).collect();
    all.sort_unstable();
    all.dedup();
    all
}

/// `accesses` as the model has them, each of the mock type, never declining anything.
fn booked(accesses: &[Access]) -> Book {
    let place = |all: &[&str], name: &str| all.iter().position(|x| *x == name).unwrap();
    let book: Book = accesses
        .iter()
        .map(|a| {
            assert!(a.kind == "mock" && a.account.plugin == WITH && a.declined.is_empty(), "{a:?}");
            let key = (place(&PLUGINS, &a.plugin), place(&SUBJECTS, &a.account.subject));
            (key, Had { id: a.id.clone(), scopes: joined(&a.scopes, &[]), default: a.default })
        })
        .collect();
    assert_eq!(book.len(), accesses.len(), "two accesses of a plugin to one account: {accesses:?}");
    book
}

/// What the store should have, as [`prop_access_is_remembered_and_restored_as_the_model_says`]
/// models it: the accounts signed in, the accesses live and remembered, and Automatically Restore
/// Access for each account and for all. An id the model doesn't know yet (an access given anew)
/// is empty till the store says it.
#[derive(Debug, Default)]
struct Model {
    signed_in: [bool; 2],
    live: Book,
    remembered: Book,
    auto: [Option<bool>; 2],
    all: Option<bool>,
}

impl Model {
    /// Gives plugin `p` back the access to account `a` it had: joined to the one it has now when
    /// it has one, else as it was, its default when it was or it has no other, and has no default.
    fn put_back(&mut self, (p, a): (usize, usize), had: Had) -> Had {
        if let Some(live) = self.live.get_mut(&(p, a)) {
            live.scopes = joined(&live.scopes, &had.scopes.iter().map(String::as_str).collect::<Vec<_>>());
            return live.clone();
        }
        let theirs: Vec<&Had> = self.live.iter().filter(|((q, _), _)| *q == p).map(|(_, h)| h).collect();
        let default = !theirs.iter().any(|h| h.default) && (had.default || theirs.is_empty());
        let had = Had { default, ..had };
        self.live.insert((p, a), had.clone());
        had
    }

    /// Account `a` signed out or its session ended: every access to it remembered, in place of
    /// what was remembered for the same plugin, no plugin's default moved.
    fn lose(&mut self, a: usize) {
        self.signed_in[a] = false;
        let gone: Vec<(usize, usize)> = self.live.keys().filter(|(_, x)| *x == a).copied().collect();
        for key in gone {
            let had = self.live.shift_remove(&key).unwrap();
            self.remembered.insert(key, had);
        }
    }

    /// Restores what's remembered of account `a`: each plugin's, or with `chosen` those it names
    /// (each with `read` and maybe `write`), the rest forgotten. What's restored, as it is now.
    fn restore(&mut self, a: usize, chosen: Option<&[(usize, bool)]>) -> Book {
        let theirs: Vec<(usize, usize)> = self.remembered.keys().filter(|(_, x)| *x == a).copied().collect();
        let mut restored = Book::new();
        for key in theirs {
            let mut had = self.remembered.shift_remove(&key).unwrap();
            if let Some(chosen) = chosen {
                let Some((_, write)) = chosen.iter().find(|(p, _)| *p == key.0) else { continue };
                had.scopes = joined(&[], if *write { &["read", "write"] } else { &["read"] });
            }
            restored.insert(key, self.put_back(key, had));
        }
        restored
    }

    /// Gives plugin `p` access to account `a`'s `read` and maybe `write`, besides what it has: its
    /// default when it has no other access.
    fn give(&mut self, (p, a): (usize, usize), write: bool) -> Had {
        let scopes: &[&str] = if write { &["read", "write"] } else { &["read"] };
        let first = !self.live.keys().any(|(q, _)| *q == p);
        let had = self.live.entry((p, a)).or_insert(Had { id: String::new(), scopes: Vec::new(), default: first });
        had.scopes = joined(&had.scopes, scopes);
        had.clone()
    }

    /// Whether signing in to account `a` restores everything remembered of it without asking.
    fn automatically(&self, a: usize) -> bool {
        self.auto[a].or(self.all).unwrap_or(false)
    }

    /// Why account `a` can't be given to a plugin, nor restored: the error's code and reason.
    fn unkept(&self, a: usize) -> (&'static str, Option<Reason>) {
        match self.remembered.keys().any(|(_, x)| *x == a) {
            true => ("account", Some(Reason::NotSignedIn)),
            false => ("not_found", None),
        }
    }

    /// Learns the id the store gave the access `key` given anew, and says whether `got` is it.
    fn learn(&mut self, key: (usize, usize), got: &Access) -> bool {
        let had = self.live.get_mut(&key).unwrap();
        if had.id.is_empty() {
            had.id.clone_from(&got.id);
        }
        booked(std::slice::from_ref(got)).get(&key) == Some(had)
    }
}

/// A step of [`prop_access_is_remembered_and_restored_as_the_model_says`], naming plugins and
/// accounts by their places in [`PLUGINS`] and [`SUBJECTS`].
#[derive(Debug, Clone)]
enum Step {
    /// Signing in as an account, for a plugin or none, restoring or resetting what's remembered of
    /// it or neither, asking for `write` besides or not.
    SignIn {
        account: usize,
        plugin: Option<usize>,
        restoring: Option<Restoring>,
        write: bool,
    },
    SignOut(usize),
    /// Its session ends: its token expired, and the service won't refresh it.
    SessionEnds(usize),
    /// Restoring what's remembered of an account: all of it, or each plugin chosen, with `write`
    /// or not (none chosen: reset).
    Restore(usize, Option<Vec<(usize, bool)>>),
    Give {
        plugin: usize,
        account: usize,
        write: bool,
    },
    SetDefault(usize, usize),
    TakeAway(usize, usize),
    /// Automatically Restore Access on or off, for an account or for all.
    Automatically(Option<usize>, bool),
}

/// What restoring an account can choose: none (a reset), echo with `write` or not, the other
/// plugin, or both.
fn choices() -> Vec<Vec<(usize, bool)>> {
    vec![vec![], vec![(0, false)], vec![(0, true)], vec![(1, false)], vec![(0, false), (1, true)]]
}

/// A step: signing in, losing an account or restoring it most often, since signing in has the
/// most ways to go and what it restores has to be lost first.
fn step(tc: &TestCase) -> Step {
    let one = || tc.draw(gs::integers::<usize>().max_value(1));
    let (account, plugin, write) = (one(), one(), tc.draw(gs::booleans()));
    match tc.draw(gs::integers::<usize>().max_value(10)) {
        0..=2 => Step::SignIn {
            account,
            plugin: tc.draw(gs::sampled_from(vec![None, Some(0), Some(1)])),
            restoring: tc.draw(gs::sampled_from(vec![None, None, Some(Restoring::Restore), Some(Restoring::Reset)])),
            write,
        },
        3 => Step::SignOut(account),
        4 => Step::SessionEnds(account),
        5 | 6 => Step::Restore(account, tc.draw(gs::optional(gs::sampled_from(choices())))),
        7 => Step::Give { plugin, account, write },
        8 => Step::SetDefault(plugin, account),
        9 => Step::TakeAway(plugin, account),
        _ => Step::Automatically(tc.draw(gs::sampled_from(vec![None, Some(0), Some(1)])), write),
    }
}

/// For any order of signing in to two accounts (for echo, another plugin or none, restoring or
/// resetting or neither, asking for `write` besides or not), signing out, their sessions ending,
/// restoring what's remembered (all of it, chosen plugins with chosen scopes, or none: reset),
/// giving access, making it the default, taking it away, and turning Automatically Restore Access
/// on or off for an account or for all: the live accesses and those remembered are what a model
/// says after each step. Losing an account remembers every access to it in place of any
/// remembered for the same plugin, its default moved nowhere; signing in again restores all of it
/// when the sign-in says to or (saying neither) Automatically Restore Access is on for the account
/// (else for all), forgets it on a reset, and gives the plugin it's for its own back either way;
/// what's restored keeps its id and scopes, joined to an access given since, and is the plugin's
/// default when it was or the plugin has no other, and has no default since. Giving access to an
/// account signed out, making it a default or restoring it fails as `not_signed_in` while anything
/// of it is remembered (`not_found` once nothing is), and resetting it works signed out. The
/// listing has each account signed in and each signed out with access to restore, and the
/// settings.
#[hegel::test(test_cases = 16)]
fn prop_access_is_remembered_and_restored_as_the_model_says(tc: TestCase) {
    let m = two_plugins();
    let mut model = Model::default();
    // Echo given `a`, and the other plugin `a` and `b`, with Automatically Restore Access set any
    // way: then any steps.
    let settings = || tc.draw(gs::sampled_from(vec![None, Some(false), Some(true)]));
    let preamble = [Step::SignIn { account: 0, plugin: Some(0), restoring: None, write: false }]
        .into_iter()
        .chain([0, 1].map(|account| Step::SignIn { account, plugin: Some(1), restoring: None, write: false }))
        .chain([None, Some(0), Some(1)].into_iter().filter_map(|of| Some(Step::Automatically(of, settings()?))));
    let drawn = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(12))).map(|_| step(&tc));
    for (n, step) in preamble.chain(drawn).enumerate() {
        model.take(&m, step, n);
    }
}

/// For either way of losing an account (signing out, its session ending) that echo and the other
/// plugin had access to, every way Automatically Restore Access can be set for it and for all (on,
/// off, or not at all), and every way signing in to it again can go (restoring, resetting or
/// neither), for echo, the other plugin or neither, then restoring what's left of it (all of it,
/// any choice, or none): what's restored, what's still remembered and what each plugin has are
/// what [`prop_access_is_remembered_and_restored_as_the_model_says`]'s model says.
#[hegel::test(test_cases = 1)]
fn prop_signing_in_again_restores_as_it_says_else_as_set(tc: TestCase) {
    let m = two_plugins();
    let settings = [None, Some(false), Some(true)];
    for (own, all) in settings.into_iter().flat_map(|own| settings.map(|all| (own, all))) {
        for restoring in [None, Some(Restoring::Restore), Some(Restoring::Reset)] {
            let losing = tc.draw(gs::sampled_from(vec![Step::SignOut(0), Step::SessionEnds(0)]));
            let plugin = tc.draw(gs::sampled_from(vec![None, Some(0), Some(1)]));
            // A store with no accounts again: a mock service and plugins each time run out of
            // files to open.
            for file in ["accounts.json", "secrets/accounts.json"] {
                drop(std::fs::remove_file(m.f.store.root().join(file)));
            }
            let mut model = Model::default();
            let given = (0..2).map(|p| Step::SignIn { account: 0, plugin: Some(p), restoring: None, write: p == 1 });
            let set = [own.map(|on| Step::Automatically(Some(0), on)), all.map(|on| Step::Automatically(None, on))];
            let again = Step::SignIn { account: 0, plugin, restoring, write: false };
            let then = Step::Restore(0, tc.draw(gs::optional(gs::sampled_from(choices()))));
            for (n, step) in given.chain(set.into_iter().flatten()).chain([losing.clone(), again, then]).enumerate() {
                model.take(&m, step, n);
            }
        }
    }
}

impl Model {
    /// Takes step `n`, `step`, on `m`'s store and as the model, which the store then agrees with.
    fn take(&mut self, m: &Mock, step: Step, n: usize) {
        let (plugins, store) = (&m.f.plugins, &m.f.store);
        let model = self;
        let said = format!("step {n} {step:?}");
        match step {
            Step::SignIn { account: a, plugin, restoring, write } => {
                let scopes: &[&str] = if write { &["write"] } else { &[] };
                let who = who(json!(SUBJECTS[a]), None);
                let done = m.sign_in_restoring(plugin.map(|p| PLUGINS[p]), who, None, scopes, restoring).0;
                let done = done.unwrap();
                model.signed_in[a] = true;
                let mut restored = match restoring {
                    Some(Restoring::Reset) => {
                        model.remembered.retain(|(_, x), _| *x != a);
                        Book::new()
                    }
                    Some(Restoring::Restore) => model.restore(a, None),
                    None if model.automatically(a) => model.restore(a, None),
                    None => Book::new(),
                };
                if let Some(p) = plugin {
                    if let Some(had) = model.remembered.shift_remove(&(p, a)) {
                        restored.insert((p, a), model.put_back((p, a), had));
                    }
                    model.give((p, a), write);
                    assert!(model.learn((p, a), done.access.as_ref().unwrap()), "{said}: {:?}", done.access);
                }
                assert_eq!(booked(&done.restored), restored, "{said}: restored");
                let remembered: Book = model.remembered.clone().into_iter().filter(|((_, x), _)| *x == a).collect();
                assert_eq!(booked(&done.remembered), remembered, "{said}: still remembered");
            }
            Step::SignOut(a) => {
                let out = plugins.sign_out_account(WITH, SUBJECTS[a], store);
                match model.signed_in[a] {
                    true => drop(out.unwrap()),
                    false => assert_eq!(out.unwrap_err().code(), "not_found", "{said}"),
                }
                model.lose(a);
            }
            Step::SessionEnds(a) => {
                match model.signed_in[a] {
                    true => m.lose(SUBJECTS[a], Losing::SessionEnds),
                    false => assert_eq!(m.token(SUBJECTS[a]).unwrap_err().code(), "not_found", "{said}"),
                }
                model.lose(a);
            }
            Step::Restore(a, chosen) => {
                let named: Option<Vec<Chosen>> = chosen.as_ref().map(|chosen| {
                    let scopes = |write: bool| if write { vec!["write".to_string()] } else { Vec::new() };
                    chosen
                        .iter()
                        .map(|(p, write)| Chosen { plugin: PLUGINS[*p].into(), scopes: scopes(*write) })
                        .collect()
                });
                let done = plugins.restore_access(&account(SUBJECTS[a]), named.as_deref(), store);
                let resetting = chosen.as_ref().is_some_and(Vec::is_empty);
                if !resetting && !model.signed_in[a] {
                    let e = done.unwrap_err();
                    assert_eq!((e.code(), e.reason()), model.unkept(a), "{said}: {e}");
                } else {
                    let expected = model.restore(a, chosen.as_deref());
                    assert_eq!(booked(&done.unwrap()), expected, "{said}: restored");
                }
            }
            Step::Give { plugin: p, account: a, write } => {
                let scopes: Vec<String> = if write { vec!["write".into()] } else { Vec::new() };
                let given = plugins.give_access(PLUGINS[p], &account(SUBJECTS[a]), &scopes, store);
                match model.signed_in[a] {
                    true => {
                        model.give((p, a), write);
                        assert!(model.learn((p, a), &given.unwrap()), "{said}");
                    }
                    false => {
                        let e = given.unwrap_err();
                        assert_eq!((e.code(), e.reason()), model.unkept(a), "{said}: {e}");
                    }
                }
            }
            Step::SetDefault(p, a) => {
                let made = plugins.set_default_access(PLUGINS[p], &account(SUBJECTS[a]), store);
                match model.signed_in[a] {
                    true => {
                        if !model.live.contains_key(&(p, a)) {
                            model.give((p, a), false);
                        }
                        model.live.iter_mut().filter(|((q, _), _)| *q == p).for_each(|((_, x), h)| h.default = *x == a);
                        assert!(model.learn((p, a), &made.unwrap()), "{said}");
                    }
                    false => {
                        let e = made.unwrap_err();
                        assert_eq!((e.code(), e.reason()), model.unkept(a), "{said}: {e}");
                    }
                }
            }
            Step::TakeAway(p, a) => {
                let taken = plugins.take_away_access(PLUGINS[p], &account(SUBJECTS[a]), store);
                match model.live.shift_remove(&(p, a)) {
                    Some(had) => {
                        assert_eq!(booked(&[taken.unwrap()]).get(&(p, a)), Some(&had), "{said}");
                        let mut left: Vec<&mut Had> =
                            model.live.iter_mut().filter(|((q, _), _)| *q == p).map(|(_, h)| h).collect();
                        if let [only] = left.as_mut_slice() {
                            only.default |= had.default;
                        }
                    }
                    None => assert_eq!(taken.unwrap_err().code(), "not_found", "{said}"),
                }
            }
            Step::Automatically(of, on) => {
                let setting = plugins.restore_automatically(of.map(|a| account(SUBJECTS[a])).as_ref(), on, store);
                assert_eq!(setting.unwrap().on, on, "{said}");
                match of {
                    Some(a) => model.auto[a] = Some(on),
                    None => model.all = Some(on),
                }
            }
        }
        let listing = plugins.accounts_and_accesses(store).unwrap();
        assert_eq!(booked(&listing.accesses), model.live, "{said}: live");
        assert_eq!(booked(&listing.remembered), model.remembered, "{said}: remembered");
        let mut listed: Vec<(&str, bool)> =
            listing.accounts.iter().map(|k| (k.subject.as_str(), k.signed_in)).collect();
        listed.sort_unstable();
        let lost = |a: usize| model.remembered.keys().any(|(_, x)| *x == a);
        let expected: Vec<(&str, bool)> =
            (0..2).filter(|a| model.signed_in[*a] || lost(*a)).map(|a| (SUBJECTS[a], model.signed_in[a])).collect();
        assert_eq!(listed, expected, "{said}: listed");
        let mut settings: Vec<(Option<String>, bool)> = listing
            .restore_automatically
            .iter()
            .map(|s| (s.account.as_ref().map(|a| a.subject.clone()), s.on))
            .collect();
        settings.sort_unstable();
        let all = model.all.map(|on| (None, on));
        let each = (0..2).filter_map(|a| Some((Some(SUBJECTS[a].to_string()), model.auto[a]?)));
        assert_eq!(settings, all.into_iter().chain(each).collect::<Vec<_>>(), "{said}: settings");
    }
}

/// For a store from before signing out took access away, whose accesses to an account it no
/// longer keeps are still among its live ones (that account echo's default or not): they're read
/// as remembered, as they were, the other access kept and no default moved to it; the account
/// lists signed out, giving it to echo says to sign in to it first, and signing in to it again
/// for echo gives the access back with its id, its default again when it was.
#[hegel::test(test_cases = 2)]
fn prop_an_old_stores_access_to_an_account_signed_out_reads_as_remembered(tc: TestCase) {
    let gone = tc.draw(gs::integers::<usize>().max_value(1));
    let m = two_plugins();
    let (plugins, store) = (&m.f.plugins, &m.f.store);
    for subject in SUBJECTS {
        m.sign_in(who(json!(subject), None), None, &[]).0.unwrap();
    }
    let before = plugins.accesses(store, Some("echo"), None).unwrap();
    m.edit("accounts.json", |book| {
        book["accounts"].as_array_mut().unwrap().retain(|k| k["subject"] != SUBJECTS[gone]);
    });
    let (was, left): (Vec<Access>, Vec<Access>) = before.into_iter().partition(|a| a.account.subject == SUBJECTS[gone]);
    let listing = plugins.accounts_and_accesses(store).unwrap();
    assert_eq!((&listing.accesses, &listing.remembered), (&left, &was));
    let listed: Vec<(&str, bool)> = listing.accounts.iter().map(|k| (k.subject.as_str(), k.signed_in)).collect();
    assert_eq!(listed, [(SUBJECTS[1 - gone], true), (SUBJECTS[gone], false)]);
    let e = plugins.give_access("echo", &account(SUBJECTS[gone]), &[], store).unwrap_err();
    assert_eq!(e.to_string(), format!("the mock account {} is signed out: sign in to it first", SUBJECTS[gone]));

    let done = m.sign_in(who(json!(SUBJECTS[gone]), None), None, &[]).0.unwrap();
    assert_eq!((done.restored.as_slice(), done.remembered.as_slice()), (was.as_slice(), &[][..]));
    assert_eq!(plugins.accesses(store, Some("echo"), None).unwrap(), [left, was].concat());
}

/// How [`prop_an_id_taken_since_is_restored_anew`] restores echo's access.
#[derive(Debug, Clone, Copy)]
enum Restored {
    /// Signing in again restoring all.
    SigningIn,
    /// Signing in again for echo.
    ForEcho,
    /// Signing in again, then restoring all.
    Afterwards,
}

/// For echo's access to an account signed out or whose session ended, its id since taken by
/// another plugin's access to another account, however it's restored (signing in again saying so,
/// or for echo, or restoring afterwards): it gets a new id, the other access keeps its own, and a
/// request naming the new one gets the account's token.
#[hegel::test(test_cases = 4)]
fn prop_an_id_taken_since_is_restored_anew(tc: TestCase) {
    let losing = tc.draw(gs::sampled_from(Losing::BOTH.to_vec()));
    let how = tc.draw(gs::sampled_from(vec![Restored::SigningIn, Restored::ForEcho, Restored::Afterwards]));
    let m = two_plugins();
    let (plugins, store) = (&m.f.plugins, &m.f.store);
    let was = m.sign_in(who(json!("a"), None), None, &[]).0.unwrap().access.unwrap().id;
    m.lose("a", losing);
    m.sign_in_for(Some("other"), who(json!("b"), None), None, &[]).0.unwrap();
    m.edit("accounts.json", |book| book["accesses"][0]["id"] = json!(was));

    let (plugin, restoring) = match how {
        Restored::SigningIn => (None, Some(Restoring::Restore)),
        Restored::ForEcho => (Some("echo"), None),
        Restored::Afterwards => (None, None),
    };
    m.sign_in_restoring(plugin, who(json!("a"), None), None, &[], restoring).0.unwrap();
    if let Restored::Afterwards = how {
        assert_eq!(plugins.restore_access(&account("a"), None, store).unwrap().len(), 1, "{how:?}");
    }
    let ids = |plugin: &str| plugins.accesses(store, Some(plugin), None).unwrap().into_iter().map(|a| a.id);
    let [now] = ids("echo").collect::<Vec<_>>().try_into().unwrap();
    assert_eq!(ids("other").collect::<Vec<_>>(), std::slice::from_ref(&was), "{how:?}");
    assert!(now != was && now.len() == 8 && now.chars().all(|c| c.is_ascii_hexdigit()), "{how:?}: {now}");
    let answer = m.fetch_as("GET", "/r/x", &format!("{{account:mock/{now}}}"), None);
    assert_eq!(answer["result"]["status"], 200, "{how:?}: {answer}");
}

/// For an account signed out or whose session ended, that echo had access to: restoring it, all
/// of it or what's chosen, says to sign in to it first and keeps it remembered; resetting it forgets
/// it, the account no longer listed; and then, as for an account never signed in, restoring it or
/// giving it to echo finds no such account.
#[hegel::test(test_cases = 4)]
fn prop_only_an_account_signed_in_is_restored_but_one_signed_out_is_reset(tc: TestCase) {
    let losing = tc.draw(gs::sampled_from(Losing::BOTH.to_vec()));
    let chosen = tc.draw(gs::sampled_from(vec![None, Some(vec![Chosen { plugin: "echo".into(), scopes: vec![] }])]));
    let m = two_plugins();
    let (plugins, store) = (&m.f.plugins, &m.f.store);
    m.sign_in(who(json!("a"), Some("a@mock.example")), None, &[]).0.unwrap();
    m.lose("a", losing);
    let remembered = || plugins.accounts_and_accesses(store).unwrap().remembered;
    let was = remembered();

    let e = plugins.restore_access(&account("a"), chosen.as_deref(), store).unwrap_err();
    assert_eq!((e.code(), e.reason()), ("account", Some(Reason::NotSignedIn)), "{e}");
    assert_eq!(e.to_string(), "the mock account a@mock.example is signed out: sign in to it first");
    assert_eq!((remembered(), was.len()), (was, 1), "not remembered as it was");

    assert_eq!(plugins.restore_access(&account("a"), Some(&[]), store).unwrap(), []);
    let listing = plugins.accounts_and_accesses(store).unwrap();
    assert!(listing.remembered.is_empty() && listing.accounts.is_empty(), "{listing:?}");
    for e in [
        plugins.restore_access(&account("a"), chosen.as_deref(), store).unwrap_err(),
        plugins.give_access("echo", &account("a"), &[], store).unwrap_err(),
        plugins.restore_access(&account("nobody"), None, store).unwrap_err(),
    ] {
        assert_eq!(e.code(), "not_found", "{e}");
    }
}

/// What's remembered of echo's access in [`prop_restoring_joins_drops_and_gives_what_is_chosen`],
/// and how it's restored.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Waiting {
    /// Echo was given access to the account again since: restoring joins the two.
    GivenSince,
    /// Beside a remembered access of a plugin that isn't installed any more: restoring forgets it.
    Uninstalled,
    /// Chosen with a scope echo doesn't ask for: refused, still remembered.
    Unasked,
    /// Declining `write`, chosen with it: given it, declining nothing.
    Declined,
    /// Not echo's default, with echo having no access of the type: restored as its default.
    NotDefault,
}

/// For echo's access to an account signed out or whose session ended, the account signed in to
/// again for the other plugin (so echo's stays remembered), restoring it: joins it to the access
/// echo was given since (one access, with the scopes of both); forgets a remembered access of a
/// plugin that isn't installed; refuses a scope echo doesn't ask for, remembering it still; gives
/// a scope declined before when it's chosen, declining it no more; and makes it echo's default
/// when echo has no other access of the type.
#[hegel::test(test_cases = 10)]
fn prop_restoring_joins_drops_and_gives_what_is_chosen(tc: TestCase) {
    use Waiting::*;
    let losing = tc.draw(gs::sampled_from(Losing::BOTH.to_vec()));
    let waiting = tc.draw(gs::sampled_from(vec![GivenSince, Uninstalled, Unasked, Declined, NotDefault]));
    let m = two_plugins();
    let (plugins, store) = (&m.f.plugins, &m.f.store);
    m.sign_in(who(json!("a"), None), None, &["write"]).0.unwrap();
    m.lose("a", losing);
    m.sign_in_for(Some("other"), who(json!("a"), None), None, &[]).0.unwrap();
    let remembered = || plugins.accounts_and_accesses(store).unwrap().remembered;
    let [was] = remembered().try_into().unwrap();
    assert_eq!((was.scopes.as_slice(), was.default), (&["read".to_string(), "write".into()][..], true), "{waiting:?}");
    let restore = |chosen: Option<&[Chosen]>| plugins.restore_access(&account("a"), chosen, store);
    let echo = || plugins.accesses(store, Some("echo"), None).unwrap();
    let chose = |scopes: &[&str]| {
        vec![Chosen { plugin: "echo".into(), scopes: scopes.iter().map(|s| s.to_string()).collect() }]
    };

    match waiting {
        GivenSince => {
            let given = plugins.give_access("echo", &account("a"), &[], store).unwrap();
            assert_eq!(given.scopes, ["read"]);
            let [back] = restore(None).unwrap().try_into().unwrap();
            assert_eq!((back.id.as_str(), back.scopes.as_slice()), (given.id.as_str(), was.scopes.as_slice()));
            assert_eq!(echo(), [back]);
        }
        Uninstalled => {
            m.edit("accounts.json", |book| {
                let mut gone = book["remembered"][0].clone();
                (gone["plugin"], gone["id"]) = (json!("gone"), json!("0a1b2c3d"));
                book["remembered"].as_array_mut().unwrap().push(gone);
            });
            assert_eq!(remembered().len(), 2);
            assert_eq!(restore(None).unwrap(), std::slice::from_ref(&was));
            assert!(remembered().is_empty(), "the uninstalled plugin's is still remembered");
        }
        Unasked => {
            let e = restore(Some(&chose(&["peek"]))).unwrap_err();
            assert_eq!(e.code(), "invalid", "{e}");
            assert_eq!(remembered(), std::slice::from_ref(&was));
            assert!(echo().is_empty());
        }
        Declined => {
            m.edit("accounts.json", |book| {
                (book["remembered"][0]["scopes"], book["remembered"][0]["declined"]) =
                    (json!(["read"]), json!(["write"]));
            });
            let [back] = restore(Some(&chose(&["write"]))).unwrap().try_into().unwrap();
            assert_eq!((back.scopes.as_slice(), back.declined.as_slice()), (was.scopes.as_slice(), &[][..]));
        }
        NotDefault => {
            m.edit("accounts.json", |book| book["remembered"][0]["default"] = json!(false));
            let [back] = restore(None).unwrap().try_into().unwrap();
            assert!(back.default, "echo has no default of the type");
        }
    }
}
