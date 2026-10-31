//! `locallibrary account`: the accounts the store keeps. With no arguments, the DJ account (an
//! Atmosphere account, see [`atmosphere`]) then every other; `account TYPE sign-in` signs in to
//! one through an account plugin, and `account TYPE:NAME status|sign-out` shows one or signs out
//! of it (see [`accounts`]). [`access`] gives plugins access to them, and [`asking`] asks in the
//! terminal when a plugin wants more of one.

pub mod access;
mod accounts;
pub mod asking;
#[cfg(feature = "account")]
mod atmosphere;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
#[cfg(feature = "account")]
use locallibrary::Store;
use locallibrary::plugin::Accounts;
use locallibrary::progress::Ctx;
use serde::Serialize;

#[cfg(feature = "tui")]
pub use accounts::signing_out;
#[cfg(feature = "account")]
pub use atmosphere::{again, sign_in_again};

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
    #[cfg(feature = "account")]
    if let Some(command) = args.command {
        return atmosphere::run(command, store, out, ctx);
    }
    if let Some(target) = args.target {
        let action = args.action.unwrap_or(accounts::Action::Status);
        return accounts::run(&target, action, args.sign_in, store, out, ctx);
    }
    let plugins = plugins::host()?;
    let kept = crate::diff_cmd::existing_store(store)?.map(|s| plugins.accounts_and_accesses(&s)).transpose()?;
    #[cfg(feature = "account")]
    let dj = locallibrary::api::account_status(&store.clone().unwrap_or_else(Store::default_root))?;
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
