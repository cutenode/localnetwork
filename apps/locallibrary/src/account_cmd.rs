//! `locallibrary account`: the accounts the store keeps. With no arguments, the DJ account (an
//! Atmosphere account, see [`atmosphere`]) then every other; `account TYPE sign-in` signs in to
//! one through an account plugin, and `account TYPE:NAME status|sign-out` shows one or signs out
//! of it (see [`accounts`]). [`access`] gives plugins access to them.

pub mod access;
mod accounts;
#[cfg(feature = "account")]
mod atmosphere;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use locallibrary::Store;
use locallibrary::plugin::Accounts;
use locallibrary::progress::Ctx;
use serde::Serialize;

#[cfg(feature = "account")]
pub use atmosphere::sign_in_again;

use crate::output::Output;
use crate::plugins;

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct AccountArgs {
    #[cfg(feature = "account")]
    #[command(subcommand)]
    command: Option<atmosphere::AccountCommand>,
    /// An account type to sign in to (`google`), or an account, as TYPE:NAME (or TYPE:SUBJECT)
    #[arg(value_name = "TARGET")]
    target: Option<String>,
    /// What to do with it [default: status]
    #[arg(value_enum, requires = "target")]
    action: Option<accounts::Action>,
    #[command(flatten)]
    sign_in: accounts::SignInArgs,
}

/// What `account` lists: the DJ account as `account status` says it, then the others.
#[derive(Serialize)]
struct Listing {
    #[cfg(feature = "account")]
    #[serde(flatten)]
    dj: locallibrary::api::AccountStatus,
    #[serde(flatten)]
    accounts: Accounts,
}

pub fn run(args: AccountArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let root = store.clone().unwrap_or_else(Store::default_root);
    #[cfg(feature = "account")]
    if let Some(command) = args.command {
        return atmosphere::run(command, store, out, ctx);
    }
    if let Some(target) = args.target {
        let action = args.action.unwrap_or(accounts::Action::Status);
        return accounts::run(&target, action, args.sign_in, &root, out, ctx);
    }
    let plugins = plugins::host()?;
    let kept = Store::open_existing(&root)?.map(|s| plugins.accounts_and_accesses(&s)).transpose()?;
    #[cfg(feature = "account")]
    let dj = locallibrary::api::account_status(&root)?;
    let listing = Listing {
        #[cfg(feature = "account")]
        dj,
        accounts: kept.unwrap_or_default(),
    };
    out.emit(&listing, |l| {
        #[cfg(feature = "account")]
        atmosphere::render_status(&l.dj);
        accounts::list(plugins, &l.accounts);
    });
    Ok(ExitCode::SUCCESS)
}
