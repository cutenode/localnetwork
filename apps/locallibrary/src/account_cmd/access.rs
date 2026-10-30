//! Plugins' access to the accounts the store keeps: `plugin access PLUGIN` lists a plugin's,
//! gives it one, makes an account its default for the account's type or takes its access away;
//! and before a plugin's destination is used, a terminal is asked about what keeps it from using
//! an account of each type it needs.

use std::mem::discriminant;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use indexmap::IndexSet;
use locallibrary::Store;
use locallibrary::plugin::{Access, Account, AccountId, Plugins, Unready, destination_spec};
use locallibrary::progress::Ctx;
use loclib_plugin::{AccountNeed, Choice, ChoiceItem};
use serde_json::json;

use super::accounts::{account_type, called, come_back, declined, find, named, titles};
use crate::browser::BrowserArgs;
use crate::jobs_cmd::usage;
use crate::output::{Output, dim, heading, ok};
use crate::plugin_cmd::choose::{answer, interactive};
use crate::plugins;

/// What `plugin access PLUGIN` changes, at most one of: the account made its default, the one its
/// access is taken away from, or the one it's given access to, with the scopes it can do without
/// that it's given besides.
pub struct Change {
    pub default: Option<String>,
    pub take_away: Option<String>,
    pub give: Option<String>,
    pub scopes: Vec<String>,
}

/// `plugin access PLUGIN`: its accesses; with `--default ACCOUNT`, that account made its default
/// for its type; with `--take-away ACCOUNT`, its access to it taken away; with `--give ACCOUNT`,
/// access to it given (no longer declined).
pub fn access(plugin: &str, change: Change, store_path: &Option<PathBuf>, out: Output) -> Result<ExitCode> {
    let plugins = plugins::host()?;
    let manifest = plugins.manifest(plugin)?;
    let root = store_path.clone().unwrap_or_else(Store::default_root);
    let Change { default, take_away, give, scopes } = change;
    let Some(named) = default.as_deref().or(take_away.as_deref()).or(give.as_deref()) else {
        let store = Store::open_existing(&root)?;
        let accesses = store.as_ref().map(|s| plugins.accesses(s, Some(plugin), None)).transpose()?;
        out.emit(&accesses.unwrap_or_default(), |a| list(plugins, plugin, &manifest.accounts, a));
        return Ok(ExitCode::SUCCESS);
    };
    let store = Store::open_or_init(&root)?;
    let all = plugins.accounts_and_accesses(&store)?;
    let mut kinds: IndexSet<&str> = manifest.accounts.iter().map(|n| n.name.as_str()).collect();
    kinds.extend(all.accesses.iter().filter(|a| a.plugin == plugin).map(|a| a.kind.as_str()));
    let a = named_among(&all.accounts, &kinds, named)?;
    let account = AccountId { plugin: a.plugin.clone(), subject: a.subject.clone() };
    if default.is_some() {
        let access = plugins.set_default_access(plugin, &account, &store)?;
        return Ok(out.done(&access, |_| ok(format!("{plugin} uses the {}", called(plugins, a)))));
    }
    if give.is_some() {
        let access = plugins.give_access(plugin, &account, &scopes, &store)?;
        let gave =
            |x: &Access| ok(format!("Gave {plugin} access to the {}: {}", called(plugins, a), x.scopes.join(", ")));
        return Ok(out.done(&access, gave));
    }
    let access = plugins.take_away_access(plugin, &account, &store)?;
    Ok(out.done(&access, |_| ok(format!("Took {plugin}'s access to the {} away", called(plugins, a)))))
}

/// The account `named` among `all`, of one of `kinds`: as TYPE:NAME, or by its name or subject
/// alone when one account of those types has it.
fn named_among<'a>(all: &'a [Account], kinds: &IndexSet<&str>, named: &str) -> Result<&'a Account> {
    let found: Vec<&Account> = kinds.iter().filter_map(|kind| find(all, kind, named).ok()).collect();
    match (found.as_slice(), named.split_once(':')) {
        ([one], _) => Ok(one),
        (_, Some((kind, rest))) if kinds.contains(&kind) => find(all, kind, rest),
        ([], _) => {
            Err(locallibrary::Error::NotFound(format!("the account {named:?} (`locallibrary account` lists them)"))
                .into())
        }
        _ => Err(usage(format!("several accounts are {named}: name one as TYPE:NAME"))),
    }
}

/// A plugin's accesses, by each account type it asks for (and any other it has access to).
fn list(plugins: &Plugins, plugin: &str, needs: &[AccountNeed], accesses: &[Access]) {
    let mut kinds: IndexSet<&str> = needs.iter().map(|n| n.name.as_str()).collect();
    kinds.extend(accesses.iter().map(|a| a.kind.as_str()));
    if kinds.is_empty() {
        say!("{}", dim(format!("{plugin} asks for no accounts.")));
    }
    for kind in kinds {
        let why = needs.iter().find(|n| n.name == kind).map(|n| format!(": {}", n.why)).unwrap_or_default();
        heading(&format!("{kind}{why}"));
        let of_kind: Vec<&Access> = accesses.iter().filter(|a| a.kind == kind).collect();
        if of_kind.is_empty() {
            say!("  {}", dim(format!("no access: `locallibrary account {kind} sign-in --for {plugin}` gives it one")));
        }
        for a in of_kind {
            let default = if a.default { " (default)" } else { "" };
            let with = match plugins.providing(kind).len() > 1 {
                true => format!(" through {}", a.account.plugin),
                false => String::new(),
            };
            say!("  {}{default}{} · {}", a.name, dim(with), dim(a.scopes.join(", ")));
        }
    }
}

/// Before the plugin a destination `spec` names is used: in a terminal (not with `--json` or
/// `--yes`), whatever keeps it from using an account of a type it asks for (see [`Unready`]) is
/// asked about once: which account it may use when it has no access (made its default, signing in
/// to another when that's chosen), signing in again when the account is signed out, won't stay
/// signed in or wasn't granted what it needs, and giving it the scopes its access lacks. Nothing's
/// asked elsewhere: the destination is refused, saying how to resolve it.
pub fn ask(spec: &str, store: &Store, out: Output, ctx: &Ctx<'_>) -> Result<()> {
    if !interactive(out) || out.yes {
        return Ok(());
    }
    let (plugin, _) = destination_spec(spec)?;
    let plugins = plugins::host()?;
    // One turned down isn't asked again: the destination refuses it.
    let mut asked = Vec::new();
    while let Some(u) =
        plugins.unready(plugin, store)?.into_iter().find(|u| !asked.contains(&(u.kind().to_string(), discriminant(u))))
    {
        asked.push((u.kind().to_string(), discriminant(&u)));
        resolve(plugins, plugin, u, store, out, ctx)?;
    }
    Ok(())
}

/// Asks about what keeps `plugin` from using an account of a type (see [`ask`]), and resolves it
/// as the user says.
fn resolve(plugins: &Plugins, plugin: &str, u: Unready, store: &Store, out: Output, ctx: &Ctx<'_>) -> Result<()> {
    let manifest = plugins.manifest(plugin)?;
    let name = &manifest.name;
    let a = match &u {
        Unready::NoAccess(kind) => {
            let need = manifest.accounts.iter().find(|n| n.name == *kind);
            return need.map_or(Ok(()), |need| give(plugins, plugin, need, store, out, ctx));
        }
        Unready::SignedOut(a) | Unready::Expiring(a) | Unready::Ungiven(a, _) | Unready::Ungranted(a, _) => a,
    };
    let t = account_type(plugins, &a.account.plugin, &a.kind);
    let account = named(plugins, &a.kind, &a.account.plugin, &a.name);
    let question = match &u {
        Unready::Ungiven(_, scopes) => {
            if out.confirm(&format!(
                "{name} needs to {} with the {account}. Give it access?",
                titles(t, scopes).join(", ")
            )) {
                plugins.give_access(plugin, &a.account, &[], store)?;
                ok(format!("{name} has access to the {account}"));
            }
            return Ok(());
        }
        Unready::Ungranted(_, scopes) => {
            format!("The {account} doesn't allow {name} to {}. Sign in again?", titles(t, scopes).join(", "))
        }
        Unready::Expiring(_) => format!("The {account} won't stay signed in long enough for {name}. Sign in again?"),
        _ => format!("The {account}, which {name} uses, is signed out. Sign in again?"),
    };
    if !out.confirm(&question) {
        return Ok(());
    }
    let start = plugins.begin_account_sign_in(
        &a.kind,
        &a.account.plugin,
        Some(plugin),
        Some(&a.account.subject),
        &[],
        store,
    )?;
    let done = come_back(plugins, &start, BrowserArgs::default(), out, ctx)?;
    ok(format!("Signed in to the {}", called(plugins, &done.account)));
    declined(plugins, &done);
    Ok(())
}

/// Asks which account of the type `need` asks for `plugin` may use, and makes it its default.
fn give(plugins: &Plugins, plugin: &str, need: &AccountNeed, store: &Store, out: Output, ctx: &Ctx<'_>) -> Result<()> {
    let (kind, name) = (&need.name, &plugins.manifest(plugin)?.name);
    let providing = plugins.providing(kind);
    let kept = plugins.store_accounts(store)?;
    let signed_in: Vec<&Account> = kept.iter().filter(|a| a.kind == *kind && a.signed_in).collect();
    let t = providing.iter().find_map(|with| account_type(plugins, with, kind));
    let title = t.map_or(kind.as_str(), |t| &t.title);
    let scopes = titles(t, &need.scopes);
    let several = providing.len() > 1;
    let mut items: Vec<ChoiceItem> = signed_in
        .iter()
        .enumerate()
        .map(|(i, a)| ChoiceItem { label: a.name.clone(), detail: several.then(|| a.plugin.clone()), value: json!(i) })
        .collect();
    let another = if signed_in.is_empty() { "Sign in" } else { "Sign in to another" };
    for (i, with) in providing.iter().enumerate() {
        let label = match several {
            true => format!("{another} through {with}"),
            false => another.to_string(),
        };
        items.push(ChoiceItem { label, detail: None, value: json!(signed_in.len() + i) });
    }
    let question = format!("Which {title} account may {name} use, to {}? {}", scopes.join(", "), need.why);
    let choice = Choice { title: question, param: kind.clone(), multi: false, items, more: Vec::new() };
    let picked = answer(&choice, "", &mut std::io::stdin().lock())?;
    let i = picked.first().and_then(|v| v.as_u64()).unwrap_or_default() as usize;
    let a = match signed_in.get(i) {
        Some(a) => (*a).clone(),
        None => {
            let with = &providing[i - signed_in.len()];
            let start = plugins.begin_account_sign_in(kind, with, Some(plugin), None, &[], store)?;
            let done = come_back(plugins, &start, BrowserArgs::default(), out, ctx)?;
            declined(plugins, &done);
            done.account
        }
    };
    plugins.set_default_access(plugin, &AccountId { plugin: a.plugin.clone(), subject: a.subject.clone() }, store)?;
    ok(format!("{name} uses the {}", called(plugins, &a)));
    Ok(())
}
