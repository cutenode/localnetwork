//! `locallibrary bug`: report a bug. Signed in to an Atmosphere account, with a space on
//! userinput.app to post to, it posts the report from that account (showing it and asking first,
//! and signing in again when the session doesn't allow it yet). Otherwise, or when posting
//! doesn't work out, it opens GitHub's bug report form, filled in with this version, the system
//! and how locallibrary was installed.

#[cfg(feature = "account")]
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, CommandFactory};
use locallibrary::progress::Ctx;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Value, json};

use crate::output::{Output, dim};

/// GitHub's bug report form for locallibrary (`.github/ISSUE_TEMPLATE/locallibrary-bug.yml`).
const NEW_ISSUE: &str = "https://github.com/cutenode/localnetwork/issues/new?template=locallibrary-bug.yml";

/// The longest form address made: about what browsers and GitHub take.
const MAX_URL: usize = 8000;

/// The form's answers to "How did you install it?" that this binary can tell from where it is.
/// It can't tell a release archive.
const CARGO_INSTALL: &str = "Built from source (cargo install)";
const INSTALLER: &str = "Installer script";
const HOMEBREW: &str = "Homebrew";

/// What a query value may hold as it is (RFC 3986's unreserved characters).
const QUERY: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');

#[derive(Args)]
pub struct BugArgs {
    /// A line saying what went wrong [default: asked, when posting from your account]
    #[arg(long)]
    title: Option<String>,
    /// What happened: what you were doing and what went wrong
    #[arg(long, short)]
    message: Option<String>,
    #[cfg(feature = "account")]
    #[command(flatten)]
    browser: crate::browser::BrowserArgs,
    /// Print the link instead of opening the browser
    #[cfg(not(feature = "account"))]
    #[arg(long)]
    no_browser: bool,
}

impl BugArgs {
    fn opens_browser(&self) -> bool {
        #[cfg(feature = "account")]
        return self.browser.opens();
        #[cfg(not(feature = "account"))]
        return !self.no_browser;
    }
}

/// A bug report: what was typed, and the details the form asks for about this computer.
#[derive(Debug, Clone, PartialEq)]
struct Report {
    title: Option<String>,
    message: Option<String>,
    /// As `locallibrary --version` says it.
    version: String,
    os: String,
    install: Option<&'static str>,
}

impl Report {
    fn here(title: Option<String>, message: Option<String>) -> Report {
        let version = crate::Cli::command().render_version().trim().to_string();
        let exe = std::env::current_exe().and_then(std::fs::canonicalize).ok();
        Report { title, message, version, os: os(), install: exe.as_deref().and_then(install_method) }
    }

    /// What's posted: what happened, then the details the form asks for, a line each.
    #[cfg(feature = "account")]
    fn body(&self) -> String {
        let typed = self.message.as_deref().map(str::trim).filter(|m| !m.is_empty());
        let mut lines: Vec<String> = typed.map(|m| format!("{m}\n")).into_iter().collect();
        lines.extend([format!("Version: {}", self.version), format!("Operating system: {}", self.os)]);
        lines.extend(self.install.map(|i| format!("Installed: {i}")));
        lines.join("\n")
    }

    /// The form's fields this fills in, by their ids (`title` is the issue's own), the longest
    /// last.
    fn fields(&self) -> Vec<(&'static str, &str)> {
        let mut fields = vec![("version", self.version.as_str()), ("os", self.os.as_str())];
        fields.extend(self.install.map(|i| ("install", i)));
        fields.extend(self.title.as_deref().map(|t| ("title", t)));
        fields.extend(self.message.as_deref().map(|m| ("what-happened", m)));
        fields
    }

    /// GitHub's bug report form, filled in: at most [`MAX_URL`] long, cutting the text short and
    /// leaving out the fields after it fills up.
    fn github_url(&self) -> String {
        let mut url = NEW_ISSUE.to_string();
        for (id, value) in self.fields() {
            let key = format!("&{id}=");
            if url.len() + key.len() > MAX_URL {
                break;
            }
            url.push_str(&key);
            let room = MAX_URL - url.len();
            let mut encoded = String::new();
            for c in value.chars() {
                let piece = utf8_percent_encode(c.encode_utf8(&mut [0; 4]), QUERY).to_string();
                if encoded.len() + piece.len() > room {
                    break;
                }
                encoded.push_str(&piece);
            }
            url.push_str(&encoded);
        }
        url
    }
}

/// The system, as the form asks for it: "macOS 26.1.0, Apple silicon".
fn os() -> String {
    let info = os_info::get();
    os_label(info.os_type(), info.version(), std::env::consts::OS, std::env::consts::ARCH)
}

/// [`os`] from its parts, so every system's wording can be checked from any one of them.
fn os_label(kind: os_info::Type, version: &os_info::Version, os: &str, arch: &str) -> String {
    let name = match kind {
        os_info::Type::Macos => "macOS".to_string(),
        other => other.to_string(),
    };
    let named = match version {
        os_info::Version::Unknown => name,
        v => format!("{name} {v}"),
    };
    let arch = match (os, arch) {
        ("macos", "aarch64") => "Apple silicon",
        ("macos", "x86_64") => "Intel",
        (_, arch) => arch,
    };
    format!("{named}, {arch}")
}

/// How the binary at `exe` (its real path) was installed, when its place tells: Homebrew keeps
/// it in its Cellar; `cargo install` and the installer script both put it in a `bin` folder
/// (`~/.cargo/bin`), but only `cargo install` lists it in that folder's `.crates2.json`.
fn install_method(exe: &Path) -> Option<&'static str> {
    if exe.components().any(|c| c.as_os_str() == "Cellar") {
        return Some(HOMEBREW);
    }
    let bin = exe.parent().filter(|b| b.file_name().is_some_and(|n| n == "bin"))?;
    let root = bin.parent()?;
    let names = [exe.file_name()?.to_str()?, exe.file_stem()?.to_str()?];
    let installs = std::fs::read(root.join(".crates2.json")).ok();
    let installs: Value = installs.and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let listed = installs["installs"]
        .as_object()
        .into_iter()
        .flat_map(|i| i.values())
        .filter_map(|p| p["bins"].as_array())
        .flatten()
        .any(|b| b.as_str().is_some_and(|b| names.contains(&b)));
    if listed {
        return Some(CARGO_INSTALL);
    }
    // The installer script puts it in `$CARGO_HOME/bin`, `~/.cargo/bin` unless that's set.
    let cargo_home = std::env::var_os("CARGO_HOME");
    let in_cargo_home =
        cargo_home.is_some_and(|h| Path::new(&h) == root) || root.file_name().is_some_and(|n| n == ".cargo");
    in_cargo_home.then_some(INSTALLER)
}

/// `locallibrary bug`.
#[cfg_attr(not(feature = "account"), allow(unused_variables, unused_mut))]
pub fn run(args: BugArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let mut report = Report::here(args.title.clone(), args.message.clone());
    #[cfg(feature = "account")]
    if let Some(code) =
        post(&args, &store.clone().unwrap_or_else(locallibrary::Store::default_root), &mut report, out, ctx)?
    {
        return Ok(code);
    }
    Ok(on_github(&report, args.opens_browser(), out))
}

/// GitHub's form, filled in: opened in the browser (unless `open` is false or it's `--json`),
/// and its address printed.
fn on_github(report: &Report, open: bool, out: Output) -> ExitCode {
    let url = report.github_url();
    let opened = open && !out.json && locallibrary::opener::open_url(&url).is_ok();
    out.emit(&json!({ "url": url, "opened": opened }), |_| match opened {
        true => {
            say!("Opened GitHub's bug report form in your browser, with this version and system filled in.");
            say!("{}", dim(format!("If it didn't open: {url}")));
        }
        false => say!("Report the bug on GitHub (this version and system are filled in):\n  {url}"),
    });
    ExitCode::SUCCESS
}

/// Post the report from the signed-in account, when there's an account and a space to post it
/// to: the exit status, or `None` to report it on GitHub instead (with what was typed).
#[cfg(feature = "account")]
fn post(args: &BugArgs, root: &Path, report: &mut Report, out: Output, ctx: &Ctx<'_>) -> Result<Option<ExitCode>> {
    use crate::output::{Outcome, heading, hint, ok, warn};
    use locallibrary::atmosphere::{self, bug_report, lexicon};

    let Some(space) = bug_report::space()? else { return Ok(None) };
    let Some(mut me) = atmosphere::status(root)? else { return Ok(None) };
    if report.title.is_none() && !out.json && std::io::stdin().is_terminal() {
        let mut input = std::io::stdin().lock();
        report.title = ask("Title (a line saying what went wrong): ", &mut input)?;
        if report.message.is_none() {
            report.message = ask_lines("What happened? What were you doing? (an empty line ends it)", &mut input)?;
        }
    }
    let Some(title) = report.title.clone().filter(|t| !t.trim().is_empty()) else { return Ok(None) };
    let body = report.body();
    bug_report::check(&title, &body)?;
    if !out.json {
        heading(&format!("Bug report from {}, to Local Network's board on userinput.app", me.name()));
        say!("  {}", console::style(&title).bold());
        for line in body.lines() {
            say!("  {line}");
        }
        say!("{}", dim("Anyone can read what you post, and copies stay on the network even after it's deleted."));
    }
    if !atmosphere::may_create(&me, lexicon::DISCUSSION) {
        let Some(again) = allow(root, &me, args, out, ctx) else { return Ok(None) };
        me = again;
    }
    let plan = json!({ "account": me.did, "space": space.uri(), "title": title, "body": body });
    if let Some(o) = out.decide(false, &format!("Post it as {}? Anyone can read it", me.name())) {
        if !out.json {
            hint(format!("Or report it on GitHub: {}", report.github_url()));
        }
        return Ok(Some(out.not_applied(&plan, o)));
    }
    match bug_report::post(root, &me, &space, &title, &body) {
        Ok(posted) => Ok(Some(out.applied(&plan, &posted, Outcome::Applied, || {
            ok("posted your bug report");
            say!("  {}", posted.url);
        }))),
        Err(e) => {
            warn(format!("couldn't post it ({e}); report it on GitHub instead"));
            Ok(None)
        }
    }
}

/// Sign in again allowing bug reports, which the session doesn't: the me then signed in, or
/// `None` (declined, it failed, or the browser didn't allow it) to report on GitHub instead.
#[cfg(feature = "account")]
fn allow(
    root: &Path,
    me: &locallibrary::api::AccountInfo,
    args: &BugArgs,
    out: Output,
    ctx: &Ctx<'_>,
) -> Option<locallibrary::api::AccountInfo> {
    use crate::output::warn;
    use locallibrary::atmosphere::{self, REPORT_SCOPE, lexicon};

    if out.json || !std::io::stdin().is_terminal() {
        warn(
            "this session can't post bug reports yet: run `locallibrary bug` in a terminal to sign in again, allowing them",
        );
        return None;
    }
    eprintln!(
        "Posting it needs your permission for locallibrary to post bug reports from your account, and nothing \
         else. That means signing in again."
    );
    if !out.confirm("Sign in again to allow bug reports?") {
        return None;
    }
    let more = [REPORT_SCOPE.to_string()];
    match crate::account_cmd::sign_in_again(root, me, &more, "bug reports", args.browser, out, ctx) {
        Ok(info) if atmosphere::may_create(&info, lexicon::DISCUSSION) => Some(info),
        Ok(info) => {
            warn(format!("{} didn't allow bug reports", info.name()));
            None
        }
        Err(e) => {
            warn(format!("couldn't sign in again ({e})"));
            None
        }
    }
}

/// A line typed after `prompt` (`None` when it's empty).
#[cfg(feature = "account")]
fn ask(prompt: &str, input: &mut impl BufRead) -> Result<Option<String>> {
    eprint!("{prompt}");
    std::io::stderr().flush().ok();
    let mut line = String::new();
    input.read_line(&mut line)?;
    Ok(Some(line.trim().to_string()).filter(|l| !l.is_empty()))
}

/// Lines typed after `prompt`, up to an empty one (`None` when there are none).
#[cfg(feature = "account")]
fn ask_lines(prompt: &str, input: &mut impl BufRead) -> Result<Option<String>> {
    eprintln!("{prompt}");
    let mut lines = Vec::new();
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            break;
        }
        lines.push(line);
    }
    Ok(Some(lines.join("\n")).filter(|t| !t.is_empty()))
}

#[cfg(test)]
mod tests;
