#[test]
fn command_line_is_well_formed() {
    use clap::CommandFactory;
    super::Cli::command().debug_assert();
    super::help::command().debug_assert();
}

/// `--cluster-size` takes sizes as people write them.
#[test]
fn sizes_read_as_written() {
    for (written, bytes) in [("32768", 32768), ("32K", 32768), ("32kb", 32768), ("64 KiB", 65536), ("1M", 1 << 20)] {
        assert_eq!(super::parse_size(written), Ok(bytes), "{written}");
    }
    for wrong in ["", "K", "big", "-1K", "4096M"] {
        assert!(super::parse_size(wrong).is_err(), "{wrong}");
    }
}

/// The filter syntax follows the help of every command that takes `--filter`, or its group's.
#[test]
fn every_filter_flag_has_the_syntax_after_it() {
    fn walk(cmd: &clap::Command, path: &str, missing: &mut Vec<String>) {
        let shows = |c: &clap::Command| c.get_after_help().is_some_and(|h| h.to_string() == super::FILTER_HELP);
        for sc in cmd.get_subcommands() {
            let path = format!("{path} {}", sc.get_name());
            let filters = sc.get_arguments().any(|a| a.get_long() == Some("filter"));
            if filters && !shows(sc) && !shows(cmd) {
                missing.push(path.clone());
            }
            walk(sc, &path, missing);
        }
    }
    let mut missing = Vec::new();
    walk(&super::help::command(), "locallibrary", &mut missing);
    assert!(missing.is_empty(), "no filter syntax after {missing:?}");
}

/// `--version` is answered before clap is, in its words.
#[test]
fn the_version_is_what_clap_would_say() {
    assert_eq!(super::help::command().render_version(), super::version_line());
}

/// The commands that can write to a drive are told from those that only read: on macOS, from a
/// terminal, the first run as a launchd job.
#[test]
fn drive_writes_are_told_from_reads() {
    use clap::Parser;
    let writes = |args: &[&str]| {
        let cli = super::Cli::try_parse_from(std::iter::once("locallibrary").chain(args.iter().copied())).unwrap();
        cli.command.as_ref().is_some_and(super::Command::writes_drives)
    };
    for args in [&["sync"][..], &["prep"], &["drive", "tidy", "X"], &["collection", "export", "X"], &["watch"]] {
        assert!(writes(args), "{args:?} writes drives");
    }
    // Bare `locallibrary` is the player, which draws the terminal itself.
    for args in [&["status"][..], &["check"], &["tracks", "X"], &["players"], &[]] {
        assert!(!writes(args), "{args:?} only reads");
    }
}
