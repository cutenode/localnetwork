use super::*;

/// Steps are named as requests and progress events name them, in any case; an unknown one is
/// refused with the names it could have been.
#[test]
fn steps_are_parsed_by_their_ids() {
    assert_eq!(StepKind::parse(" Sync ").unwrap(), StepKind::Sync);
    assert!(StepKind::ALL.iter().all(|k| StepKind::parse(k.id()).ok() == Some(*k)));
    let err = StepKind::parse("dance").unwrap_err().to_string();
    assert!(
        err.contains("unknown step \"dance\" (clean, replay, smart, player, settings, pull, sync, check, eject)"),
        "{err}"
    );
}

/// Regression: when ejecting the drives failed part way, the report dropped the ones already
/// ejected.
#[test]
fn a_failed_eject_still_reports_the_drives_ejected() {
    let roots = [PathBuf::from("/drives/A"), PathBuf::from("/drives/B")];
    let mut done = StepReport { step: StepKind::Eject, done: vec![], warnings: vec![], reports: vec![] };
    let result = eject_each(&roots, &mut done, |root| match root.ends_with("A") {
        true => Ok(EjectReport { name: "A".into(), root: root.into(), commands: vec![], powered_off: true }),
        false => Err(Error::invalid("B is busy")),
    });
    assert!(result.is_err());
    assert_eq!(done.done, ["A ejected; it can be pulled out"]);
    assert_eq!(done.reports.len(), 1);
}
