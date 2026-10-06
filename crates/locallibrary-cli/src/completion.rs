//! Shell completion and man pages. `locallibrary completions SHELL` prints a script that has the
//! shell ask `locallibrary` itself on Tab (`COMPLETE=SHELL locallibrary -- ...`, clap_complete's
//! dynamic completion), so drives, playlist paths, My Tags and history lists complete from what's
//! mounted now, and libraries from the store; `--static` prints a script that completes only
//! commands and flags.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Arg, Args, Command};
use clap_complete::env::{EnvCompleter, Shells};
use clap_complete::{ArgValueCandidates, CompletionCandidate, Shell};
use locallibrary::api;

/// `completions`.
#[derive(Args)]
#[command(after_long_help = "Install:
  bash        echo 'source <(locallibrary completions bash)' >> ~/.bashrc
  zsh         echo 'source <(locallibrary completions zsh)' >> ~/.zshrc
  fish        locallibrary completions fish > ~/.config/fish/completions/locallibrary.fish
  elvish      echo 'eval (locallibrary completions elvish | slurp)' >> ~/.elvish/rc.elv
  powershell  locallibrary completions powershell | Out-String | Invoke-Expression   (in $PROFILE)

The script calls locallibrary on Tab, so it completes the drives mounted now, their playlists, My
Tags and history lists. Load it from your shell's startup file rather than saving it, so it
matches the locallibrary you have. --static completes commands and flags only, without running
locallibrary.")]
pub struct CompletionsArgs {
    /// bash, zsh, fish, elvish or powershell
    shell: Shell,
    /// A script that completes commands and flags only, without asking locallibrary on Tab
    #[arg(long = "static")]
    fixed: bool,
}

/// `man`.
#[derive(Args)]
pub struct ManArgs {
    /// Write a page for every command into this folder (locallibrary.1, locallibrary-drive.1...)
    /// [default: print the top-level page]
    #[arg(long, short, value_name = "DIR")]
    output: Option<PathBuf>,
}

/// The command line, with what each argument completes to.
pub fn command() -> Command {
    with_candidates(crate::help::command())
}

/// Answer a completion request (`COMPLETE=bash locallibrary -- ...`) and exit; otherwise do
/// nothing. First thing in `main`: nothing may be printed before it.
pub fn complete() {
    clap_complete::CompleteEnv::with_factory(command).bin("locallibrary").complete();
}

pub fn completions(args: CompletionsArgs) -> Result<ExitCode> {
    let mut out = std::io::stdout().lock();
    if args.fixed {
        let mut cmd = crate::help::command();
        clap_complete::generate(args.shell, &mut cmd, "locallibrary", &mut out);
        return Ok(ExitCode::SUCCESS);
    }
    let name = args.shell.to_string();
    let shells = Shells::builtins();
    let shell: &dyn EnvCompleter = shells.completer(&name).context("no completion for that shell")?;
    shell.write_registration("COMPLETE", "locallibrary", "locallibrary", &completer(), &mut out)?;
    out.flush()?;
    Ok(ExitCode::SUCCESS)
}

/// The binary the shell runs on Tab: this one, by its full path unless it was found on the PATH.
fn completer() -> String {
    let argv0 = std::env::args().next().unwrap_or_else(|| "locallibrary".into());
    if Path::new(&argv0).components().count() <= 1 {
        return argv0;
    }
    std::env::current_exe().map(|p| p.display().to_string()).unwrap_or(argv0)
}

pub fn man(args: ManArgs) -> Result<ExitCode> {
    use clap::CommandFactory;
    // No page for `help` at every level.
    fn no_help(cmd: Command) -> Command {
        cmd.disable_help_subcommand(true).mut_subcommands(no_help)
    }
    let cmd = no_help(crate::Cli::command());
    match args.output {
        Some(dir) => {
            std::fs::create_dir_all(&dir).with_context(|| format!("making {}", dir.display()))?;
            clap_mangen::generate_to(cmd, &dir).with_context(|| format!("writing to {}", dir.display()))?;
            crate::output::ok(format!("wrote the man pages to {}", dir.display()));
        }
        None => clap_mangen::Man::new(cmd).render(&mut std::io::stdout().lock())?,
    }
    Ok(ExitCode::SUCCESS)
}

/// What an argument takes, for completing it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Takes {
    Drive,
    Playlist,
    Tag,
    HistoryList,
    Library,
    /// A player model (`--players`, `players set`).
    Model,
    /// A step `refresh` can leave out (`--skip`).
    Step,
}

fn takes(a: &Arg) -> Option<Takes> {
    if !a.get_action().takes_values() {
        return None;
    }
    let name = a.get_value_names().and_then(|v| v.first()).map(|s| s.as_str().to_ascii_uppercase());
    match (a.get_id().as_str(), name.as_deref()) {
        (_, Some("PLAYLIST")) | ("playlist" | "playlists", _) => Some(Takes::Playlist),
        (_, Some("DRIVE")) => Some(Takes::Drive),
        ("drive" | "drives" | "source" | "target" | "left" | "right" | "like" | "against", _) => Some(Takes::Drive),
        ("tag", _) => Some(Takes::Tag),
        ("list" | "history", _) => Some(Takes::HistoryList),
        ("library", _) | (_, Some("LIBRARY")) => Some(Takes::Library),
        ("players" | "models", _) => Some(Takes::Model),
        ("skip", _) => Some(Takes::Step),
        _ => None,
    }
}

fn with_candidates(cmd: Command) -> Command {
    cmd.mut_args(|a| match takes(&a) {
        Some(Takes::Drive) => a.add(ArgValueCandidates::new(drives)),
        Some(Takes::Playlist) => a.add(ArgValueCandidates::new(playlists)),
        Some(Takes::Tag) => a.add(ArgValueCandidates::new(tags)),
        Some(Takes::HistoryList) => a.add(ArgValueCandidates::new(history_lists)),
        Some(Takes::Library) => a.add(ArgValueCandidates::new(libraries)),
        Some(Takes::Model) => a.add(ArgValueCandidates::new(models)),
        Some(Takes::Step) => a.add(ArgValueCandidates::new(steps)),
        None => a,
    })
    .mut_subcommands(with_candidates)
}

/// The mounted drives, by name and by mount path, and the drives the store knows by name.
fn drives() -> Vec<CompletionCandidate> {
    let mut out = Vec::new();
    for d in api::drives_mounted() {
        let root = d.root.display().to_string();
        out.push(CompletionCandidate::new(&d.name).help(Some(root.clone().into())));
        out.push(CompletionCandidate::new(&root).help(Some(d.name.clone().into())));
    }
    if let Ok(store) = locallibrary::Store::open(store_root()) {
        for r in store.drives().map(|d| d.into_values().collect::<Vec<_>>()).unwrap_or_default() {
            if !out.iter().any(|c| c.get_value() == r.name.as_str()) {
                out.push(CompletionCandidate::new(&r.name).help(Some("not mounted".into())));
            }
        }
    }
    out
}

fn store_root() -> PathBuf {
    std::env::var_os("LOCALLIBRARY_STORE").map(PathBuf::from).unwrap_or_else(locallibrary::Store::default_root)
}

/// The mounted drives the command line names so far, else every mounted drive with a library.
fn drives_in_command_line() -> Vec<String> {
    let words: Vec<String> = std::env::args().skip_while(|a| a != "--").collect();
    let mounted: Vec<_> = api::drives_mounted().into_iter().filter(|d| d.has_export).collect();
    let named: Vec<String> = mounted
        .iter()
        .filter(|d| {
            words.iter().any(|w| {
                w.eq_ignore_ascii_case(&d.name) || locallibrary::device::dunce_canonicalize(Path::new(w)) == d.root
            })
        })
        .map(|d| d.root.display().to_string())
        .collect();
    match named.is_empty() {
        true => mounted.iter().map(|d| d.root.display().to_string()).collect(),
        false => named,
    }
}

/// Playlist and folder paths on the drives the command line names (or every mounted drive).
fn playlists() -> Vec<CompletionCandidate> {
    let mut out = Vec::new();
    for drive in drives_in_command_line() {
        let Ok(view) = api::drive_library(&api::DriveRef { drive }) else { continue };
        let lib = &view.library;
        for p in &lib.playlists {
            let path = lib.playlist_path(p.id);
            let help = if p.is_folder { "folder".to_string() } else { format!("{} tracks", p.track_ids.len()) };
            out.push(CompletionCandidate::new(path).help(Some(help.into())));
        }
    }
    out
}

/// My Tags (Category/Tag) on the drives the command line names (or every mounted drive).
fn tags() -> Vec<CompletionCandidate> {
    let mut out = Vec::new();
    for drive in drives_in_command_line() {
        let Ok(t) = api::tags_list(&api::DriveRef { drive }) else { continue };
        for c in &t.categories {
            out.push(CompletionCandidate::new(&c.name).help(Some("category".into())));
            for tag in &c.tags {
                out.push(CompletionCandidate::new(&tag.path).help(Some(format!("{} tracks", tag.tracks.len()).into())));
            }
        }
    }
    out
}

/// History lists on the drives the command line names (or every mounted drive).
fn history_lists() -> Vec<CompletionCandidate> {
    let mut out = Vec::new();
    for drive in drives_in_command_line() {
        let Ok(h) = api::drive_history(&api::DriveRef { drive }) else { continue };
        for l in &h.lists {
            out.push(CompletionCandidate::new(&l.name).help(Some(format!("{} tracks", l.tracks.len()).into())));
        }
    }
    out
}

/// The libraries in the store.
fn libraries() -> Vec<CompletionCandidate> {
    let Ok(store) = locallibrary::Store::open(store_root()) else { return Vec::new() };
    store.libraries().unwrap_or_default().into_iter().map(|l| CompletionCandidate::new(l.name)).collect()
}

/// The player models locallibrary knows.
fn models() -> Vec<CompletionCandidate> {
    api::player_models().iter().map(|m| CompletionCandidate::new(m.name)).collect()
}

/// The steps `refresh --skip` leaves out.
fn steps() -> Vec<CompletionCandidate> {
    api::StepKind::ALL
        .into_iter()
        .filter(|s| !matches!(s, api::StepKind::Check | api::StepKind::Eject))
        .map(|s| CompletionCandidate::new(s.id()).help(Some(s.title().into())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_complete_to_what_they_take() {
        use clap::CommandFactory;
        let cmd = crate::Cli::command();
        let find = |path: &[&str], id: &str| {
            let mut c = &cmd;
            for p in path {
                c = c.find_subcommand(p).unwrap_or_else(|| panic!("no {p}"));
            }
            let a = c.get_arguments().find(|a| a.get_id() == id).unwrap_or_else(|| panic!("no {id} in {path:?}"));
            takes(a)
        };
        assert_eq!(find(&["sync"], "drives"), Some(Takes::Drive));
        assert_eq!(find(&["sync"], "from"), Some(Takes::Drive));
        assert_eq!(find(&["playlist", "add"], "target"), Some(Takes::Playlist));
        assert_eq!(find(&["playlist", "add"], "drive"), Some(Takes::Drive));
        assert_eq!(find(&["pack"], "target"), Some(Takes::Drive));
        assert_eq!(find(&["pack"], "playlists"), Some(Takes::Playlist));
        assert_eq!(find(&["tags", "add"], "tag"), Some(Takes::Tag));
        assert_eq!(find(&["plays", "export"], "list"), Some(Takes::HistoryList));
        assert_eq!(find(&["merge"], "history"), None, "a flag takes nothing");
        assert_eq!(find(&["refresh"], "drive"), Some(Takes::Drive));
        assert_eq!(find(&["refresh"], "skip"), Some(Takes::Step));
        assert_eq!(find(&["set"], "drives"), Some(Takes::Drive));
        assert_eq!(find(&["set"], "library"), Some(Takes::Library));
        assert_eq!(find(&["set"], "players"), Some(Takes::Model));
        assert_eq!(find(&["setup"], "primary"), Some(Takes::Drive));
        assert_eq!(find(&["pull"], "library"), Some(Takes::Library));
        assert_eq!(find(&["players", "set"], "models"), Some(Takes::Model));
        assert_eq!(find(&["drive", "format"], "erase"), None);
    }
}
