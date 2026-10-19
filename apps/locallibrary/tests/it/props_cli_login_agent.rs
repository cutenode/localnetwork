//! Property tests of `locallibrary watch --install` and `--uninstall`: the login agent (a launchd
//! agent on macOS, a systemd user service on Linux) written into a home folder of the test's own,
//! with `launchctl` and `systemctl` stand-ins that note what they're asked and fail when told to.
//! The real launchd and systemd are never reached: the stand-ins are the only programs on `PATH`.

#![cfg(any(target_os = "macos", target_os = "linux"))]

use std::path::{Path, PathBuf};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use serde_json::Value;

use crate::support::{Env, Run, path, set_mode};

const LABEL: &str = "local.locallibrary.watch";
const UNIT: &str = "locallibrary-watch.service";

/// The service manager's command line tool on this system.
const TOOL: &str = if cfg!(target_os = "macos") { "launchctl" } else { "systemctl" };

/// A folder holding only stand-ins for `launchctl` and `systemctl`: each notes its arguments as a
/// line of `calls`, and fails (saying so on stderr) when its first word that isn't a flag is a
/// line of `fail`.
struct Tools {
    bin: PathBuf,
    calls: PathBuf,
    fail: PathBuf,
}

impl Tools {
    fn new(fx: &Env) -> Tools {
        let bin = fx.outside("bin");
        std::fs::create_dir(&bin).unwrap();
        let (calls, fail) = (fx.outside("calls"), fx.outside("fail"));
        for tool in ["launchctl", "systemctl"] {
            let script = format!(
                // Only the shell's own commands: nothing else is on `PATH`.
                "#!/bin/sh\necho \"{tool} $*\" >> '{calls}'\nfor a in \"$@\"; do case \"$a\" in --*) ;; *) verb=$a; break ;; \
                 esac; done\nwhile read -r f; do if [ \"$f\" = \"$verb\" ]; then echo \"$verb refused\" >&2; exit 1; fi; \
                 done < '{fail}'\n",
                calls = path(&calls),
                fail = path(&fail),
            );
            std::fs::write(bin.join(tool), script).unwrap();
            set_mode(&bin.join(tool), 0o755);
        }
        Tools { bin, calls, fail }
    }

    /// What the tools were asked since the last time, a line each.
    fn take(&self) -> Vec<String> {
        let calls = std::fs::read_to_string(&self.calls).unwrap_or_default();
        let _ = std::fs::remove_file(&self.calls);
        calls.lines().map(String::from).collect()
    }
}

/// Where the agent's definition goes, for a home `home` and (on Linux) `XDG_CONFIG_HOME` `config`.
fn definition(home: &Path, config: Option<&Path>) -> PathBuf {
    if cfg!(target_os = "macos") {
        return home.join("Library/LaunchAgents").join(format!("{LABEL}.plist"));
    }
    config.map_or_else(|| home.join(".config"), Path::to_path_buf).join("systemd/user").join(UNIT)
}

/// The arguments the agent runs `locallibrary` with, read back out of its definition.
fn agent_arguments(text: &str) -> Vec<String> {
    if cfg!(target_os = "macos") {
        let array = &text[text.find("<array>\n").unwrap() + 8..text.find("  </array>").unwrap()];
        return array
            .lines()
            .map(|l| l.trim().trim_start_matches("<string>").trim_end_matches("</string>").replace("&amp;", "&"))
            .skip(1)
            .collect();
    }
    let line = text.lines().find_map(|l| l.strip_prefix("ExecStart=")).unwrap();
    line.split("\" \"").map(|w| w.trim_matches('"').to_string()).skip(1).collect()
}

/// What `watch --install` and `--uninstall` ask the service manager, in order: the verbs.
fn verbs(calls: &[String]) -> Vec<String> {
    calls.iter().map(|c| c.split(' ').skip(1).find(|w| !w.starts_with("--")).unwrap_or_default().to_string()).collect()
}

/// Installing and uninstalling the login agent in any order, with any settle time and with or
/// without syncing the group, printed or as JSON, while the service manager takes or refuses
/// each step: installing writes the definition running `watch --pull` with the options asked for
/// and starts it (stopping one that was running first on macOS; reloading, enabling and restarting
/// it on Linux), failing on the first step refused but a stop, with the file left written;
/// uninstalling stops it and removes the file (only saying there was nothing when none was there),
/// failing where a step is refused; and what's printed says what the JSON does.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn the_login_agent_runs_watch_as_asked(tc: TestCase) {
    let fx = Env::new();
    let tools = Tools::new(&fx);
    let home = fx.outside("home");
    std::fs::create_dir(&home).unwrap();
    let config = tc.draw(gs::booleans()).then(|| fx.outside("config"));
    let file = definition(&home, config.as_deref());
    let run = |args: &[&str]| -> Run {
        let mut c = fx.command();
        c.env("HOME", &home).env("PATH", &tools.bin).env_remove("XDG_CONFIG_HOME").args(args);
        if let Some(config) = &config {
            c.env("XDG_CONFIG_HOME", config);
        }
        let out = c.output().unwrap();
        Run {
            out: String::from_utf8(out.stdout).unwrap(),
            err: String::from_utf8(out.stderr).unwrap(),
            code: out.status.code().unwrap(),
        }
    };
    let manager_steps: &[&str] = if cfg!(target_os = "macos") {
        &["bootout", "bootstrap"]
    } else {
        &["daemon-reload", "enable", "restart", "disable"]
    };

    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        let refused: Vec<&str> = tc.draw(gs::subsequences(manager_steps.to_vec()));
        std::fs::write(&tools.fail, refused.iter().map(|v| format!("{v}\n")).collect::<String>()).unwrap();
        let json = tc.draw(gs::booleans());
        let was = file.exists();
        let install = tc.draw(gs::weighted_booleans(0.6));
        let mut args = if json { vec!["--json", "watch"] } else { vec!["watch"] };
        let settle = tc.draw(gs::sampled_from(&[None, Some("5"), Some("0"), Some("30")][..]));
        let no_sync = tc.draw(gs::booleans());
        if install {
            args.push("--install");
            if let Some(s) = settle {
                args.extend(["--settle", s]);
            }
            if no_sync {
                args.push("--no-sync-group");
            }
        } else {
            args.push("--uninstall");
        }
        let r = run(&args);
        let calls = tools.take();
        let said =
            format!("{args:?} with {refused:?} refused exited {}\n{}\n{}\ncalls: {calls:?}", r.code, r.out, r.err);
        assert!(calls.iter().all(|c| c.starts_with(TOOL)), "{said}");
        let asked = verbs(&calls);
        // The steps each command takes, up to the first refused one that isn't a stop of an
        // agent that may not be running.
        let steps: &[&str] = match (install, cfg!(target_os = "macos")) {
            (true, true) => &["bootout", "bootstrap"],
            (true, false) => &["daemon-reload", "enable", "restart"],
            (false, _) if !was => &[],
            (false, true) => &["bootout"],
            (false, false) => &["disable", "daemon-reload"],
        };
        let failed = steps.iter().position(|s| refused.contains(s) && *s != "bootout");
        let taken = &steps[..failed.map_or(steps.len(), |f| f + 1)];
        assert_eq!(asked, taken, "{said}");

        if let Some(f) = failed {
            assert_eq!(r.code, 1, "{said}");
            let message = match json {
                true => {
                    serde_json::from_str::<Value>(&r.out).unwrap()["error"]["message"].as_str().unwrap().to_string()
                }
                false => r.err.clone(),
            };
            assert!(message.contains(&format!("{} refused", steps[f])), "{said}");
            assert!(message.contains(&format!("`{TOOL} ")) && message.contains("failed"), "{said}");
            // Installing wrote the file before starting it; uninstalling on Linux removes it
            // between disabling and reloading.
            let there = install || (steps[f] == "disable");
            assert_eq!(file.exists(), there, "{said}");
            continue;
        }
        assert_eq!(r.code, 0, "{said}");
        if !install {
            assert!(!file.exists(), "{said}");
            match json {
                true => {
                    let v: Value = serde_json::from_str(&r.out).unwrap();
                    let gone = if was { Value::from(path(&file)) } else { Value::Null };
                    assert_eq!(v["report"]["uninstalled"], gone, "{said}");
                }
                false if was => assert_eq!(r.out, "✓ Drives are no longer saved as they're plugged in\n", "{said}"),
                false => assert_eq!(r.out, "`watch` wasn't set to run at log in\n", "{said}"),
            }
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{e}: {said}"));
        let mut want = vec!["--store".to_string(), path(&fx.store).to_string(), "watch".into(), "--pull".into()];
        if !no_sync {
            want.push("--sync-group".into());
        }
        if let Some(s) = settle.filter(|s| *s != "5") {
            want.extend(["--settle".into(), s.into()]);
        }
        assert_eq!(agent_arguments(&text), want, "{text}\n{said}");
        if cfg!(target_os = "macos") {
            let target = calls[1].rsplit(' ').next().unwrap();
            assert_eq!(target, path(&file), "{said}");
            assert!(calls[0].ends_with(&format!("/{LABEL}")), "{said}");
        }
        match json {
            true => {
                let v: Value = serde_json::from_str(&r.out).unwrap();
                assert_eq!(v["report"]["installed"], path(&file), "{said}");
                assert_eq!(v["report"]["command"], serde_json::json!(want), "{said}");
            }
            false => {
                let log = match cfg!(target_os = "macos") {
                    true => path(&home.join("Library/Logs/locallibrary-watch.log")).to_string(),
                    false => format!("journalctl --user -u {UNIT}"),
                };
                let expected = format!(
                    "✓ Drives are now saved into the store as they're plugged in, whenever you're logged in\n  log: \
                     {log}; `locallibrary watch --uninstall` stops it\n"
                );
                assert_eq!(r.out, expected, "{said}");
            }
        }
    }
}
