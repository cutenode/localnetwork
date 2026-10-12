use super::*;

/// The installed watch syncs a drive's group after pulling it, unless told not to.
#[test]
fn the_installed_watch_syncs_groups_unless_told_not_to() {
    assert_eq!(agent_args(&None, false, SETTLE).unwrap(), ["watch", "--pull", "--sync-group"]);
    assert_eq!(agent_args(&None, true, 9).unwrap(), ["watch", "--pull", "--settle", "9"]);
}

use clap::Parser;
use hegel::TestCase;
use hegel::generators as gs;

/// Whatever store, settle time and grouping it's given, the command `watch --install` installs
/// reads back (as `locallibrary` parses it) as a pulling watch with those: the store made
/// absolute, the group synced unless told not to, and the settle time.
#[hegel::test(test_cases = 100)]
fn the_installed_watch_reads_back_as_given(tc: TestCase) {
    let store = tc.draw(gs::optional(gs::sampled_from(vec!["store", "/Volumes/NAS/my store", "rel/dir with space"])));
    let no_sync_group = tc.draw(gs::booleans());
    let settle = tc.draw(gs::integers::<u64>().max_value(600));
    let store = store.map(PathBuf::from);
    let args = agent_args(&store, no_sync_group, settle).unwrap();
    let cli = crate::Cli::try_parse_from(std::iter::once("locallibrary".to_string()).chain(args.clone()))
        .unwrap_or_else(|e| panic!("{args:?}: {e}"));
    assert_eq!(cli.store, store.map(|s| std::path::absolute(s).unwrap()), "{args:?}");
    let Some(crate::Command::Watch(w)) = cli.command else { panic!("not watch: {args:?}") };
    assert!(w.pull && !w.once && !w.install && !w.uninstall, "{args:?}");
    assert_eq!((w.sync_group, w.settle), (!no_sync_group, settle), "{args:?}");
}

/// What `watch` says of each thing it sees and does names the drive, and says what's particular
/// to it: the history lists new on it (none, one by name, or how many), each drive a sync went
/// to (with why one failed), why it's waiting or left alone. A change to a drive's files says
/// nothing.
#[hegel::test(test_cases = 100)]
fn what_watch_says_names_the_drive_and_what_happened(tc: TestCase) {
    use locallibrary::api::{TargetError, TargetResult};
    use locallibrary::watch::MountedDrive;
    let name = tc.draw(gs::sampled_from(vec!["DJ", "Ünïcödé 日本", "USB DRIVE"])).to_string();
    let drive = MountedDrive { root: PathBuf::from("/Volumes/x"), id: None, name: name.clone(), has_export: true };
    let lists_drawn: Vec<String> = tc
        .draw(gs::vecs(gs::sampled_from(vec!["HISTORY 001", "HISTORY 002"])).max_size(3))
        .into_iter()
        .map(String::from)
        .collect();
    let said = lists(&lists_drawn);
    match lists_drawn.len() {
        0 => assert_eq!(said, ""),
        1 => assert_eq!(said, format!(" (new history list {})", lists_drawn[0])),
        n => assert!(said.starts_with(&format!(" ({n} new history lists: ")), "{said}"),
    }
    let failed = tc.draw(gs::vecs(gs::booleans()).max_size(3));
    let targets: Vec<TargetResult> = failed
        .iter()
        .enumerate()
        .map(|(i, &f)| TargetResult {
            target: PathBuf::from(format!("/Volumes/B{i}")),
            name: format!("B{i}"),
            report: None,
            error: f.then(|| TargetError { kind: "conflict", message: "it changed".into() }),
        })
        .collect();
    let (library, reason) = ("Main".to_string(), "it's in use".to_string());
    let actions = [
        WatchAction::UpToDate { drive: drive.clone(), library: library.clone() },
        WatchAction::Pending {
            drive: drive.clone(),
            library: library.clone(),
            history_lists: lists_drawn.clone(),
            beside: tc.draw(gs::booleans()),
        },
        WatchAction::Synced { drive: drive.clone(), library: library.clone(), targets },
        WatchAction::Waiting { drive: drive.clone(), reason: reason.clone() },
        WatchAction::Skipped { drive: drive.clone(), reason: reason.clone() },
        WatchAction::Failed { drive: drive.clone(), kind: "conflict", message: "it changed".into() },
    ];
    for a in &actions {
        let text = console::strip_ansi_codes(&describe(Watched::Did(a)).unwrap()).into_owned();
        assert!(text.contains(&name), "{text}");
        match a {
            WatchAction::Pending { .. } => assert!(text.contains(&said), "{text}"),
            WatchAction::Synced { targets, .. } => {
                for t in targets {
                    let shown = match &t.error {
                        None => t.name.clone(),
                        Some(e) => format!("{} (failed: {})", t.name, e.message),
                    };
                    assert!(text.contains(&shown), "{text}");
                }
            }
            WatchAction::Waiting { .. } | WatchAction::Skipped { .. } => assert!(text.contains(&reason), "{text}"),
            _ => {}
        }
    }
    for e in [WatchEvent::Attached(drive.clone()), WatchEvent::Detached(drive.clone())] {
        assert!(describe(Watched::Seen(&e)).unwrap().starts_with(&name));
    }
    let changed = WatchEvent::Changed { drive, changed: Vec::new() };
    assert_eq!(describe(Watched::Seen(&changed)), None);
}
