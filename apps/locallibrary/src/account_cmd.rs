//! `locallibrary account`: sign in with an Atmosphere account (the account people use for
//! Bluesky and other AT Protocol apps), see who's signed in, and sign out.

use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use console::style;
use locallibrary::Store;
use locallibrary::api::{self, AccountInfo, AccountStatus, Client, Storage};
use locallibrary::atmosphere;
use locallibrary::local_time;
use locallibrary::progress::Ctx;

use crate::output::{Output, dim, heading, ok, warn};

#[derive(Args)]
pub struct AccountArgs {
    #[command(subcommand)]
    command: Option<AccountCommand>,
}

#[derive(Subcommand)]
enum AccountCommand {
    /// Who's signed in (the default)
    Status {
        /// Also ask the account's server, refreshing the session if needed
        #[arg(long)]
        check: bool,
    },
    /// Sign in with your Atmosphere account in the browser
    Login(LoginArgs),
    /// Sign out: revoke the session and remove it from this computer
    Logout,
    /// Write the client metadata document servers fetch to learn who's signing in (Local
    /// Network's web app serves locallibrary's, from apps/web/src/lib/tools/locallibrary/). For
    /// locallibrary's developers: not in `--help`.
    #[command(hide = true)]
    ClientMetadata {
        /// Where it will be hosted
        #[arg(long, value_name = "URL", default_value = atmosphere::CLIENT_ID)]
        client_id: String,
        /// Scopes to allow besides atproto and publishing, which it always allows (repeatable)
        #[arg(long = "scope", value_name = "SCOPE")]
        scopes: Vec<String>,
        /// Write it to a file
        #[arg(long, short, value_name = "FILE")]
        output: Option<PathBuf>,
        /// With -o: replace the file if it's there already
        #[arg(long, requires = "output")]
        replace: bool,
    },
}

#[derive(Args)]
struct LoginArgs {
    /// Your handle (alice.bsky.social), DID, or your account's server (https://bsky.social)
    /// [default: the handle you last signed in with]
    handle: Option<String>,
    /// Another client metadata document [default: $LOCALLIBRARY_OAUTH_CLIENT_ID, else
    /// locallibrary's own, or the development client while that isn't online]
    #[arg(long, value_name = "URL", conflicts_with = "dev_client")]
    client_id: Option<String>,
    /// Sign in as the http://localhost development client (servers ask about it every time)
    #[arg(long)]
    dev_client: bool,
    /// Keep the session in a file only you can read, not the system keychain (for computers
    /// without one)
    #[arg(long)]
    session_file: bool,
    /// Also allow publishing your My Settings, profile and sets (those three record types, nothing
    /// else); publishing asks for this when it's needed
    #[arg(long)]
    publish: bool,
    #[command(flatten)]
    browser: BrowserArgs,
}

/// How the browser round trip goes.
#[derive(Args, Clone, Copy)]
pub struct BrowserArgs {
    /// Print the sign-in link instead of opening the browser
    #[arg(long)]
    no_browser: bool,
    /// The browser runs on another computer: paste the address it ends up on after approving
    #[arg(long)]
    paste: bool,
    /// Seconds to wait for the browser (an hour at most)
    #[arg(long, value_name = "SECS", default_value_t = 300)]
    timeout: u64,
}

impl BrowserArgs {
    /// How long the browser is waited for: `--timeout`, up to the longest sign-in waits.
    fn wait(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.timeout).min(atmosphere::LONGEST_LOGIN_WAIT)
    }
}

fn root(store: &Option<PathBuf>) -> PathBuf {
    store.clone().unwrap_or_else(Store::default_root)
}

pub fn run(args: AccountArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let root = root(store);
    match args.command.unwrap_or(AccountCommand::Status { check: false }) {
        AccountCommand::Status { check: false } => {
            let s = api::account_status(&root)?;
            out.emit(&s, render_status);
        }
        AccountCommand::Status { check: true } => {
            let info = api::account_check(&root, ctx)?;
            out.emit(&info, |i| {
                render_account(i);
                ok(format!("{} answered for this session", i.auth_server));
            });
        }
        AccountCommand::Login(args) => return login(args, &root, out, ctx),
        AccountCommand::Logout => {
            let r = api::account_logout(&root)?;
            out.done(&r, |r| {
                match &r.account {
                    None => say!("{}", dim("Not signed in to an Atmosphere account.")),
                    Some(a) if r.revoked => ok(format!("Signed out of {}", a.name())),
                    Some(a) => {
                        ok(format!("Signed out of {} on this computer", a.name()));
                        warn("the server couldn't be told; the session ends on its own within two weeks");
                    }
                }
                if let Some(why) = &r.left_behind {
                    warn(format!(
                        "its secrets are still on this computer ({why}); `locallibrary account logout` tries again"
                    ));
                }
            });
        }
        AccountCommand::ClientMetadata { client_id, scopes, output, replace } => {
            let doc =
                api::account_client_metadata(&api::ClientMetadataRequest { client_id: client_id.clone(), scopes })?;
            let text = format!("{}\n", serde_json::to_string_pretty(&doc)?);
            match output {
                Some(path) => {
                    locallibrary::fsutil::write_output(&path, text.as_bytes(), replace)?;
                    out.emit(&serde_json::json!({ "output": path, "metadata": doc }), |_| {
                        ok(format!("wrote {}", path.display()));
                        let how = if client_id == atmosphere::CLIENT_ID {
                            format!("Host it at {client_id}; sign-in uses it once it's online")
                        } else {
                            format!("Host it at {client_id}, then sign in with --client-id {client_id}")
                        };
                        say!("  {}", dim(how));
                    });
                }
                None => print!("{text}"),
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn login(args: LoginArgs, root: &std::path::Path, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let interactive = !out.json && std::io::stdin().is_terminal();
    let mut handle = args.handle.clone();
    if handle.is_none() && interactive {
        let last = atmosphere::last_handle(root);
        eprint!(
            "Your Atmosphere handle, or your account's server{}: ",
            last.as_ref().map(|h| format!(" [{h}]")).unwrap_or_default()
        );
        std::io::stderr().flush().ok();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        handle = Some(line.trim().to_string()).filter(|s| !s.is_empty()).or(last);
    }
    let req = api::AccountLoginRequest {
        handle,
        client_id: args.client_id.clone(),
        dev_client: args.dev_client,
        session_file: args.session_file.then_some(true),
        scopes: Vec::new(),
        publish: args.publish,
    };
    let info = sign_in(root, &req, args.browser, out, ctx)?;
    out.done(&info, |i| {
        ok(format!("Signed in with your Atmosphere account as {}", style(i.name()).bold()));
        render_details(i);
    });
    Ok(ExitCode::SUCCESS)
}

/// The browser round trip: open the sign-in page, then wait for it to come back (or for the
/// address it ended up on to be pasted).
pub fn sign_in(
    root: &std::path::Path,
    req: &api::AccountLoginRequest,
    browser: BrowserArgs,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<AccountInfo> {
    let interactive = !out.json && std::io::stdin().is_terminal();
    let start = api::account_login_begin(root, req)?;
    if let Some(note) = &start.note {
        warn(note);
    }

    // The link goes to stderr so stdout stays the result (JSON with --json).
    let opened = !browser.no_browser && !browser.paste && locallibrary::opener::open_url(&start.url).is_ok();
    if opened {
        eprintln!("Opening your browser to sign in with your Atmosphere account at {}…", style(&start.server).bold());
        eprintln!("{}", dim(format!("If it didn't open: {}", start.url)));
    } else {
        eprintln!("Sign in with your Atmosphere account at {}: open this link", style(&start.server).bold());
        eprintln!("  {}", start.url);
    }

    if browser.paste {
        if !interactive {
            api::account_login_cancel(&api::AccountLoginCancel { id: start.id.clone() });
            bail!("--paste needs a terminal to paste into");
        }
        eprint!("After approving, paste the address your browser ended up on: ");
        std::io::stderr().flush().ok();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        Ok(api::account_login_paste(&api::AccountLoginPaste { id: start.id, url: line.trim().to_string() })?)
    } else {
        eprintln!(
            "{}",
            dim(format!("Waiting for you to approve (up to {}; Ctrl-C stops)…", atmosphere::span(browser.wait())))
        );
        let finish = api::AccountLoginFinish { id: start.id, timeout_secs: Some(browser.wait().as_secs()) };
        Ok(api::account_login_finish(&finish, ctx)?)
    }
}

/// Sign the signed-in account in again, allowing publishing too (the same storage and, when it
/// isn't locallibrary's own, the same client document). The browser may approve another
/// account: the result says which.
pub fn sign_in_to_publish(
    root: &std::path::Path,
    account: &AccountInfo,
    browser: BrowserArgs,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<AccountInfo> {
    let req = api::AccountLoginRequest {
        handle: Some(account.handle.clone().unwrap_or_else(|| account.did.clone())),
        client_id: match &account.client {
            Client::Hosted { client_id } if client_id != atmosphere::CLIENT_ID => Some(client_id.clone()),
            _ => None,
        },
        session_file: Some(account.storage == Storage::File),
        publish: true,
        ..Default::default()
    };
    let info = sign_in(root, &req, browser, out, ctx)?;
    if !out.json {
        ok(format!("Signed in again as {}, allowing publishing", style(info.name()).bold()));
    }
    Ok(info)
}

fn storage_name(s: Storage) -> &'static str {
    match s {
        Storage::Keychain if cfg!(target_os = "macos") => "in the macOS Keychain",
        Storage::Keychain if cfg!(windows) => "in Windows Credential Manager",
        Storage::Keychain => "in the Secret Service keyring",
        Storage::File => "in atmosphere-session.json, readable only by you",
    }
}

fn render_status(s: &AccountStatus) {
    match &s.account {
        Some(a) => render_account(a),
        None => {
            say!("{}", dim("Not signed in to an Atmosphere account."));
            let hint = s.last_handle.as_ref().map_or_else(
                || "locallibrary account login <handle>".to_string(),
                |h| format!("locallibrary account login {h}"),
            );
            say!(
                "{}",
                dim(format!("`{hint}` signs in with the account you use for Bluesky and other AT Protocol apps."))
            );
        }
    }
}

fn render_account(a: &AccountInfo) {
    heading(&format!("Atmosphere account {}", a.name()));
    render_details(a);
}

fn render_details(a: &AccountInfo) {
    let host = |url: &str| url.split("://").nth(1).unwrap_or(url).trim_end_matches('/').to_string();
    say!("  {}", dim(format!("{} · data on {} · signed in at {}", a.did, host(&a.pds), host(&a.auth_server))));
    let again = if a.days_left() >= 1 {
        format!("sign in again by {} ({} days)", local_time(a.sign_in_again_by, "%Y-%m-%d"), a.days_left())
    } else if !a.expired() {
        format!("sign in again by {} (today)", local_time(a.sign_in_again_by, "%Y-%m-%d %H:%M"))
    } else {
        "the session has likely ended; sign in again".to_string()
    };
    say!("  signed in {} · {again}", local_time(a.signed_in_at, "%Y-%m-%d"));
    let can = if a.scopes.iter().all(|s| s == "atproto") {
        "knows who you are; can't post or change anything".to_string()
    } else if a.scopes.iter().all(|s| s == "atproto" || atmosphere::PUBLISH_SCOPES.contains(&s.as_str())) {
        "can publish your My Settings, profile and sets; can't post or change anything else".to_string()
    } else {
        format!("allowed: {}", a.scopes.join(" "))
    };
    say!("  {}", dim(format!("{can} · session kept {}", storage_name(a.storage))));
    match &a.client {
        Client::Hosted { client_id } => say!("  {}", dim(format!("client {client_id}"))),
        Client::Loopback => {
            say!("  {}", dim("signed in as the development client, which servers show as a local app"))
        }
    }
}

#[cfg(test)]
mod tests;
