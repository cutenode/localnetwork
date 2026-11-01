//! `plugin account`: a plugin's account on another service, signed in to in the browser (the
//! host keeps its tokens, never shown), signed out of, and whether it's signed in.

use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use console::style;
use locallibrary::Store;
use locallibrary::callback::span;
use locallibrary::plugin::{AccountStatus, Plugins};
use locallibrary::progress::Ctx;

use crate::browser::BrowserArgs;
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
    match command.unwrap_or(AccountCommand::Status) {
        AccountCommand::Status => {
            let store = crate::diff_cmd::existing_store(store_path)?;
            out.emit(&plugins.account(&plugin, &account, store.as_ref())?, status);
        }
        AccountCommand::SignIn(browser) => {
            let signed_in = sign_in(plugins, &plugin, &account, &crate::open_store(store_path)?, browser, out, ctx)?;
            return Ok(out.done(&signed_in, |a| ok(format!("Signed in to {}", style(&a.title).bold()))));
        }
        AccountCommand::SignOut => {
            let r = plugins.sign_out(&plugin, &account, &crate::open_store(store_path)?)?;
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

/// The browser round trip: open the sign-in page, then wait for it to come back (or for the
/// address it ended up on to be pasted), as `account login` does.
fn sign_in(
    plugins: &Plugins,
    plugin: &str,
    account: &str,
    store: &Store,
    browser: BrowserArgs,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<AccountStatus> {
    let interactive = !out.json && std::io::stdin().is_terminal();
    let start = plugins.begin_sign_in(plugin, account, store)?;
    // The link goes to stderr so stdout stays the result (JSON with --json).
    let opened = browser.opens() && locallibrary::opener::open_url(&start.url).is_ok();
    match opened {
        true => {
            eprintln!("Opening your browser to sign in…");
            eprintln!("{}", dim(format!("If it didn't open: {}", start.url)));
        }
        false => eprintln!("Sign in: open this link\n  {}", start.url),
    }
    if !browser.paste {
        eprintln!("{}", dim(format!("Waiting for you to approve (up to {}; Ctrl-C stops)…", span(browser.wait()))));
        return Ok(plugins.finish_sign_in(&start.id, browser.wait(), ctx)?);
    }
    if !interactive {
        plugins.cancel_sign_in(&start.id);
        bail!("--paste needs a terminal to paste into");
    }
    eprint!("After approving, paste the address your browser ended up on: ");
    std::io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(plugins.finish_sign_in_with_url(&start.id, line.trim())?)
}
