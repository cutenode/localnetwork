#[cfg(unix)]
use loclib_format::test_util::FakeTools;

use super::*;
use crate::Error;

/// Ejecting doesn't wait for a running operation, and asks nothing of the system meanwhile.
#[test]
fn a_drive_in_use_is_not_ejected() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open(dir.path()).unwrap();
    let _syncing = d.lock().unwrap();
    assert!(matches!(eject(&d), Err(Error::Locked(_))));
}

/// Ejecting never waits for a running operation, nor asks the system anything meanwhile, whoever
/// holds the drive's lock.
#[hegel::test(test_cases = 6)]
fn prop_a_drive_being_written_is_never_ejected(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let adopted = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open(dir.path()).unwrap();
    if adopted {
        d.adopt(Some("DNA")).unwrap();
    }
    // Held throughout: were it not, the system's eject would run on this folder's volume.
    let _writing = d.lock().unwrap();
    let again = Device::open(dir.path()).unwrap();
    match eject(&again) {
        Err(Error::Locked(root)) => assert_eq!(root, d.root()),
        other => panic!("{other:?}"),
    }
}

/// Ejecting runs the platform's commands for the drive's root, in order, and reports them as
/// run; the first that fails stops it in the tool's own words, with what to do when the drive is
/// busy.
#[cfg(unix)]
#[hegel::test(test_cases = 20)]
fn prop_ejecting_runs_its_commands_in_order_until_one_fails(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut fake = FakeTools::new();
    std::fs::create_dir(fake.dir().join("DNA")).unwrap();
    let d = Device::open(fake.dir().join("DNA")).unwrap();
    fake.volume_at(d.root(), "vfat", true);
    let cmds = commands(d.root()).unwrap();
    let fails_at =
        tc.draw(gs::weighted_booleans(0.4)).then(|| tc.draw(gs::integers::<usize>().max_value(cmds.len() - 1)));
    let why = tc.draw(gs::sampled_from(vec!["Device or resource busy", "Not authorized to perform operation"]));
    for (i, c) in cmds.iter().enumerate() {
        let words: Vec<&str> = c.args.iter().map(String::as_str).collect();
        if fails_at == Some(i) {
            fake.refuse(&c.program, &words, why);
        } else {
            fake.answer(&c.program, &words, "");
        }
    }

    let ejected = eject(&d);
    let ran: Vec<Cmd> = fake.calls().into_iter().filter(|c| c.program != "lsblk").collect();
    assert_eq!(ran, cmds[..fails_at.map_or(cmds.len(), |i| i + 1)]);
    match (ejected, fails_at) {
        (Ok(report), None) => {
            assert_eq!(report.commands, cmds.iter().map(Cmd::display).collect::<Vec<_>>());
            assert_eq!((report.root.as_path(), report.powered_off), (d.root(), true));
        }
        (Err(Error::External { command, message }), Some(i)) => {
            assert_eq!(command, cmds[i].display());
            let hint = " (close any window or program using the drive, then try again)";
            assert_eq!(message, format!("{why}{}", if why.contains("busy") { hint } else { "" }));
        }
        (other, _) => panic!("{other:?}"),
    }
}
