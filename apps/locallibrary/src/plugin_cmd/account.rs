//! `plugin account`: a plugin's account on another service, signed in to in the browser (the
//! host keeps its tokens, never shown), signed out of, and whether it's signed in.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, Subcommand};
use console::style;
use locallibrary::Store;
use locallibrary::plugin::{AccountStatus, Plugins};
use locallibrary::progress::Ctx;

use crate::browser::{BrowserArgs, round_trip};
use crate::output::{Output, dim, ok};

#[derive(Args)]
pub struct AccountArgs {
    /// The plugin's id
    plugin: String,
    /// The account's id (`plugin info PLUGIN` lists them)
    account: String,
    #[command(subcommand)]
    command: Option<AccountCommand>,
}

#[derive(Subcommand)]
enum AccountCommand {
    /// Whether it's signed in (the default)
    Status,
    /// Sign in in the browser
    SignIn(BrowserArgs),
    /// Sign out: its tokens are removed from this computer, and the service is told when it can be
    SignOut,
}

pub fn account(
    plugins: &Plugins,
    args: AccountArgs,
    store_path: &Option<PathBuf>,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    let AccountArgs { plugin, account, command } = args;
    let root = store_path.clone().unwrap_or_else(Store::default_root);
    match command.unwrap_or(AccountCommand::Status) {
        AccountCommand::Status => {
            let store = Store::open_existing(&root)?;
            out.emit(&plugins.account(&plugin, &account, store.as_ref())?, status);
        }
        AccountCommand::SignIn(browser) => {
            let signed_in = sign_in(plugins, &plugin, &account, &Store::open_or_init(&root)?, browser, out, ctx)?;
            return Ok(out.done(&signed_in, |a| ok(format!("Signed in to {}", style(&a.title).bold()))));
        }
        AccountCommand::SignOut => {
            let r = plugins.sign_out(&plugin, &account, &Store::open_or_init(&root)?)?;
            return Ok(out.done(&r, |r| match (r.was_signed_in, r.revoked) {
                (false, _) => say!("{}", dim(format!("Not signed in to {}.", r.status.title))),
                (true, true) => ok(format!("Signed out of {}", r.status.title)),
                (true, false) => ok(format!("Signed out of {} on this computer", r.status.title)),
            }));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn status(a: &AccountStatus) {
    match a.signed_in {
        true => say!("{}: {}", a.title, style("signed in").green()),
        false => say!("{}: {}", a.title, dim("not signed in")),
    }
    if !a.scopes.is_empty() {
        say!("  {}", dim(format!("allowed: {}", a.scopes.join(", "))));
    }
    if a.signed_in && !a.refreshes {
        say!("  {}", dim("sign in again once it expires"));
    }
}

/// The browser round trip of a sign-in to one of a plugin's accounts.
fn sign_in(
    plugins: &Plugins,
    plugin: &str,
    account: &str,
    store: &Store,
    browser: BrowserArgs,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<AccountStatus> {
    let start = plugins.begin_sign_in(plugin, account, store)?;
    let finish = |wait| plugins.finish_sign_in(&start.id, wait, ctx);
    let paste = |url: &str| plugins.finish_sign_in_with_url(&start.id, url);
    round_trip(&start.url, browser, out, finish, paste, || plugins.cancel_sign_in(&start.id))
}
