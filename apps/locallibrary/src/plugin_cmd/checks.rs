//! `plugin checks`: the plugins' checks and guards, whether each runs, and how long one has; and
//! the command line a finding's fix runs with.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use locallibrary::plugin::{CheckChange, PluginFix, Plugins, set_check_timeout};
use unicode_width::UnicodeWidthStr;

use super::listing::invocation;
use crate::jobs_cmd::usage;
use crate::output::{self, Output, dim};
use crate::plugins::checks::Checks;

/// What `plugin checks` was asked to change.
pub(super) struct Change {
    pub check: Option<String>,
    pub check_change: CheckChange,
    pub timeout: Option<f64>,
}

pub(super) fn checks(plugins: &Plugins, change: Change, store_path: &Option<PathBuf>, out: Output) -> Result<ExitCode> {
    let Change { check, check_change: c, timeout } = change;
    let Some(check) = check else {
        if timeout.is_none() && !c.reset {
            let store = crate::diff_cmd::existing_store(store_path)?;
            out.emit(&Checks::of(plugins, store.as_ref()), show);
            return Ok(ExitCode::SUCCESS);
        }
        let said = match timeout {
            Some(secs) => format!("give a check {secs}s to answer"),
            None => format!("give a check {}s to answer, as by default", locallibrary::plugin::TIMEOUT.as_secs()),
        };
        let set = || {
            let store = crate::open_store(store_path)?;
            set_check_timeout(&store, timeout)?;
            Ok(Checks::of(plugins, Some(&store)))
        };
        return out.one_step(&said, "Change it?", set, show);
    };
    let Some((plugin, name)) = check.split_once('.') else {
        return Err(usage(format!(
            "name a check as PLUGIN.CHECK (`locallibrary plugin checks` lists them), not {check:?}"
        )));
    };
    // Named wrong, or changed as it can't be, it's said before anything's asked.
    plugins.check_change(plugin, name, c)?;
    if c.on.is_none() && c.blocking.is_none() && !c.reset {
        let store = crate::diff_cmd::existing_store(store_path)?;
        let mut listed = Checks::of(plugins, store.as_ref());
        listed.checks.retain(|x| x.plugin == plugin && x.name == name);
        out.emit(&listed, show);
        return Ok(ExitCode::SUCCESS);
    }
    let mut said = Vec::new();
    if c.reset {
        said.push("put it back as it is by default".to_string());
    }
    said.extend(c.on.map(|on| if on { "run it" } else { "don't run it" }.to_string()));
    said.extend(
        c.blocking
            .map(|b| if b { "stop changes it finds something about" } else { "only warn about changes" }.to_string()),
    );
    let set = || {
        let store = crate::open_store(store_path)?;
        plugins.set_check(&store, plugin, name, c)?;
        Ok(Checks::of(plugins, Some(&store)))
    };
    out.one_step(&format!("{check}: {}", said.join(", ")), "Change it?", set, show)
}

fn show(c: &Checks) {
    if c.checks.is_empty() {
        say!("{}", dim("no plugin has a check"));
    }
    let width = c.checks.iter().map(|x| x.plugin.width() + x.name.width() + 1).max().unwrap_or(0);
    for x in &c.checks {
        let name = format!("{}.{}", x.plugin, x.name);
        let when: Vec<&str> = x.when.iter().map(|w| w.name()).collect();
        let mut state = vec![if x.settings.on { "on" } else { "off" }];
        if x.settings.blocking {
            state.push("blocking");
        }
        say!(
            "{}  {:<9}  {}",
            output::fit(&name, width),
            state.join(", "),
            dim(format!("{}, when: {}", x.title, when.join(", ")))
        );
    }
    say!("{}", dim(format!("a check has {}s to answer", c.timeout)));
}

/// The command line that runs `fix`, for the finding on `drive`: its plugin's operation.
pub fn fix_line(plugins: &Plugins, fix: &PluginFix, drive: &str) -> String {
    let manifest = plugins.manifest(&fix.plugin).ok();
    let operation = manifest.and_then(|m| m.contributes.operations.iter().find(|o| o.name == fix.operation));
    let run = invocation(&fix.plugin, &fix.operation, operation.and_then(|o| o.cli.as_deref()));
    // Its params hold no control characters: the host takes in what a plugin says printable.
    let json = fix.params.to_string();
    let params = match fix.params.as_object().is_some_and(|p| !p.is_empty()) {
        true => format!(" --params '{}'", json.replace('\'', r"'\''")),
        false => String::new(),
    };
    format!("{run} {drive:?}{params}")
}
