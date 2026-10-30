//! `account TYPE sign-in` and `account TYPE:NAME status|sign-out`: the accounts the store keeps
//! besides the DJ account, signed in to in the browser through account plugins (their tokens
//! never shown), and listed with the plugins that have access to them.

use std::path::Path;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, ValueEnum};
use locallibrary::Store;
use locallibrary::local_time;
use locallibrary::plugin::{Access, Account, AccountSignIn, AccountSignedIn, Accounts, Plugins};
use locallibrary::progress::Ctx;
use loclib_plugin::AccountType;

use crate::browser::{BrowserArgs, round_trip};
use crate::jobs_cmd::usage;
use crate::output::{Output, dim, heading, ok, warn};
use crate::plugins;

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(super) enum Action {
    /// Show it
    Status,
    /// Sign in to an account of the type in the browser (TYPE:NAME: again, as that account)
    SignIn,
    /// Sign out: its tokens are removed from this computer, and the service is told when it can
    /// be; plugins keep their access, unused until it's signed in again
    SignOut,
}

#[derive(Args)]
pub(super) struct SignInArgs {
    /// With sign-in: the account plugin to sign in through [default: the only one providing the
    /// type]
    #[arg(long, value_name = "PLUGIN")]
    with: Option<String>,
    /// With sign-in: give this plugin access to it (its default for the type when it has none)
    #[arg(long = "for", value_name = "PLUGIN")]
    for_plugin: Option<String>,
    /// With sign-in: a scope of the type's to ask for besides (repeatable; `plugin info` lists
    /// them)
    #[arg(long = "scope", value_name = "SCOPE")]
    scopes: Vec<String>,
    #[command(flatten)]
    browser: BrowserArgs,
}

pub(super) fn run(
    target: &str,
    action: Action,
    args: SignInArgs,
    root: &Path,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    let plugins = plugins::host()?;
    let (kind, named) = match target.split_once(':') {
        Some((kind, named)) => (kind, Some(named)),
        None => (target, None),
    };
    if action != Action::SignIn && (args.with.is_some() || args.for_plugin.is_some() || !args.scopes.is_empty()) {
        return Err(usage("--with, --for and --scope go with sign-in"));
    }
    if action == Action::SignIn {
        let store = Store::open_or_init(root)?;
        // Signed in again as an account kept, or one signed out that plugins still have access to.
        let known = plugins.accounts_and_accesses(&store)?.accounts;
        let again = named.map(|named| find(&known, kind, named)).transpose()?;
        let with = match (args.with, again) {
            (Some(with), _) => with,
            (None, Some(a)) => a.plugin.clone(),
            (None, None) => plugins.provider(kind, None)?,
        };
        let subject = again.map(|a| a.subject.as_str());
        let start =
            plugins.begin_account_sign_in(kind, &with, args.for_plugin.as_deref(), subject, &args.scopes, &store)?;
        let done = come_back(plugins, &start, args.browser, out, ctx)?;
        return Ok(out.done(&done, |d| signed_in(plugins, d, args.for_plugin.as_deref())));
    }
    let named =
        named.ok_or_else(|| usage(format!("name an account: {kind}:NAME (`locallibrary account` lists them)")))?;
    let store = Store::open_existing(root)?;
    let all = store.as_ref().map(|s| plugins.accounts_and_accesses(s)).transpose()?.unwrap_or_default();
    let a = find(&all.accounts, kind, named)?.clone();
    if action == Action::Status {
        let accesses =
            all.accesses.into_iter().filter(|x| x.account.plugin == a.plugin && x.account.subject == a.subject);
        let accesses = accesses.collect();
        let one = Accounts { accounts: vec![a], accesses };
        out.emit(&one, |one| list(plugins, one));
        return Ok(ExitCode::SUCCESS);
    }
    let store = store.ok_or_else(|| usage(format!("{} isn't signed in", a.name)))?;
    let r = plugins.sign_out_account(&a.plugin, &a.subject, &store)?;
    Ok(out.done(&r, |r| match r.revoked || !r.account.signed_in {
        true => ok(format!("Signed out of {}", r.account.name)),
        false => ok(format!("Signed out of {} on this computer", r.account.name)),
    }))
}

/// The browser round trip of the sign-in `start` began.
pub(super) fn come_back(
    plugins: &Plugins,
    start: &AccountSignIn,
    browser: BrowserArgs,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<AccountSignedIn> {
    let finish = |wait| plugins.finish_account_sign_in(&start.id, wait, ctx);
    let paste = |url: &str| plugins.finish_account_sign_in_with_url(&start.id, url);
    round_trip(&start.url, browser, out, finish, paste, || plugins.cancel_sign_in(&start.id))
}

/// The account of type `kind` named `named` (its subject, or what it's shown as) among `all`.
pub(super) fn find<'a>(all: &'a [Account], kind: &str, named: &str) -> Result<&'a Account> {
    let of_kind = || all.iter().filter(|a| a.kind == kind);
    if let Some(a) = of_kind().find(|a| a.subject == named) {
        return Ok(a);
    }
    let by_name: Vec<&Account> = of_kind().filter(|a| a.name == named).collect();
    match by_name.as_slice() {
        [one] => Ok(one),
        [] => Err(locallibrary::Error::NotFound(format!(
            "the {kind} account {named:?} (`locallibrary account` lists them)"
        ))
        .into()),
        several => {
            let subjects: Vec<&str> = several.iter().map(|a| a.subject.as_str()).collect();
            Err(usage(format!(
                "several {kind} accounts are {named}: name one as {kind}:SUBJECT ({})",
                subjects.join(", ")
            )))
        }
    }
}

/// The account type `kind` as the account plugin `with` provides it, if it's installed.
pub(super) fn account_type<'a>(plugins: &'a Plugins, with: &str, kind: &str) -> Option<&'a AccountType> {
    plugins.manifest(with).ok()?.contributes.account_types.iter().find(|t| t.name == kind)
}

/// What `a` is called: see [`named`].
pub(super) fn called(plugins: &Plugins, a: &Account) -> String {
    named(plugins, &a.kind, &a.plugin, &a.name)
}

/// What the account of type `kind` shown as `name` that `with` signed in is called: its type's
/// title, `account` and its name, then the account plugin when several provide the type.
pub(super) fn named(plugins: &Plugins, kind: &str, with: &str, name: &str) -> String {
    let title = account_type(plugins, with, kind).map_or(kind, |t| &t.title);
    match plugins.providing(kind).len() > 1 {
        true => format!("{title} account {name} (through {with})"),
        false => format!("{title} account {name}"),
    }
}

/// The titles of `scopes` (by name) as `t` has them, their names where it doesn't.
pub(super) fn titles<'a>(t: Option<&'a AccountType>, scopes: &'a [String]) -> Vec<&'a str> {
    scopes.iter().map(|s| t.and_then(|t| t.scopes.get(s)).map_or(s.as_str(), |s| &s.title)).collect()
}

/// What signing in did, for `for_plugin` when it was for one.
fn signed_in(plugins: &Plugins, d: &AccountSignedIn, for_plugin: Option<&str>) {
    ok(format!("Signed in to the {}", called(plugins, &d.account)));
    if let Some(other) = &d.instead_of {
        let unused = for_plugin.map(|p| format!(", so {p} wasn't given access")).unwrap_or_default();
        warn(format!("the browser signed in to another account than {other}, which is still kept{unused}"));
    }
    if let Some(access) = &d.access {
        let default = if access.default { ", its default" } else { "" };
        say!("  {}", dim(format!("{} has access to it{default}", access.plugin)));
    }
    declined(plugins, d);
}

/// What the service declined of what signing in `d` was to give a plugin, if it declined any.
pub(super) fn declined(plugins: &Plugins, d: &AccountSignedIn) {
    let Some(access) = d.access.as_ref().filter(|_| !d.declined.is_empty()) else { return };
    let t = account_type(plugins, &d.account.plugin, &d.account.kind);
    warn(format!("the service didn't allow {} to {}", access.plugin, titles(t, &d.declined).join(", ")));
}

/// When an account that signs itself out at `at` does: on its day and in how many days, at its
/// time when that's today, or that it has.
pub fn signing_out(at: chrono::DateTime<chrono::Utc>) -> String {
    let left = at - chrono::Utc::now();
    match left.num_days() {
        _ if left <= chrono::Duration::zero() => format!("signed itself out at {}", local_time(at, "%Y-%m-%d %H:%M")),
        0 => format!("signs itself out at {} (today)", local_time(at, "%Y-%m-%d %H:%M")),
        days => format!("signs itself out on {} ({days} days)", local_time(at, "%Y-%m-%d")),
    }
}

/// Each of `all`'s accounts: its type, name and account plugin, whether it's signed in and when
/// it signs itself out (warning in its last days), what it was granted, and which plugins have
/// access to it.
pub(super) fn list(plugins: &Plugins, all: &Accounts) {
    for a in &all.accounts {
        let accesses: Vec<&Access> =
            all.accesses.iter().filter(|x| x.account.plugin == a.plugin && x.account.subject == a.subject).collect();
        show(plugins, a, &accesses);
    }
}

fn show(plugins: &Plugins, a: &Account, accesses: &[&Access]) {
    heading(&called(plugins, a));
    let again = format!("`locallibrary account {}:{} sign-in` signs in again", a.kind, a.subject);
    let mut state = match (a.signed_in, a.refreshes) {
        (false, _) => format!("signed out: {again}"),
        (true, true) => "signed in, refreshed on its own".into(),
        (true, false) => "signed in".into(),
    };
    if let Some(at) = a.signs_out.filter(|_| a.signed_in) {
        state = format!("{state} · {}", signing_out(at));
    }
    match a.scopes.is_empty() {
        true => say!("  {}", dim(state)),
        false => say!("  {}", dim(format!("{state} · allowed: {}", a.scopes.join(" ")))),
    }
    if let Some(at) = a.signs_out.filter(|_| a.due_soon()) {
        warn(format!("it {}: {again}", signing_out(at)));
    }
    if !accesses.is_empty() {
        let who: Vec<String> = accesses
            .iter()
            .map(|x| if x.default { format!("{} (default)", x.plugin) } else { x.plugin.clone() })
            .collect();
        say!("  {}", dim(format!("access: {}", who.join(", "))));
    }
}
