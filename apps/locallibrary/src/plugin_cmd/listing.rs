//! `plugin list`, `plugin info` and `plugin settings`.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use console::style;
use locallibrary::plugin::Plugins;
use loclib_plugin::{Setting, Where};
use unicode_width::UnicodeWidthStr;

use crate::jobs_cmd::usage;
use crate::output::{self, Output, dim, heading, ok};
use crate::plugins::params::{self, cell};
use crate::plugins::{Described, Listed, Settings};

/// `name`s of a kind (`command`), in a phrase: `commands echo, titles`.
fn named(what: &str, names: &[String]) -> Option<String> {
    let s = if names.len() == 1 { "" } else { "s" };
    (!names.is_empty()).then(|| format!("{what}{s} {}", names.join(", ")))
}

pub(super) fn list(all: &[Listed]) {
    if all.is_empty() {
        say!("{}", dim("no plugins"));
    }
    for (i, p) in all.iter().enumerate() {
        if i > 0 {
            say!();
        }
        say!("{}  {} {}", style(&p.id).bold(), p.name, dim(&p.version));
        for g in &p.capabilities {
            say!("  {} {}", dim("may"), g.means);
        }
        let adds: Vec<String> = [
            named("command", &p.commands),
            named("operation", &p.operations),
            named("analyzer", &p.analyzers),
            named("export format", &p.exporters),
            named("setting", &p.settings),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !adds.is_empty() {
            say!("  {} {}", dim("adds"), adds.join("; "));
        }
    }
}

/// How a plugin's command or operation is run: `locallibrary import`, `locallibrary echo titles`.
fn invocation(id: &str, name: &str, cli: Option<&str>) -> String {
    format!("locallibrary {}", cli.map_or_else(|| format!("{id} {name}"), String::from))
}

pub(super) fn info(d: &Described) {
    let m = &d.manifest;
    let c = &m.contributes;
    heading(&format!("{} ({}) {}", m.name, m.id, m.version));
    let section = |title: &str, lines: Vec<(String, String)>| {
        if lines.is_empty() {
            return;
        }
        say!("  {title}");
        let width = lines.iter().map(|(l, _)| l.width()).max().unwrap_or(0);
        for (l, r) in lines {
            match r.is_empty() {
                true => say!("    {l}"),
                false => say!("    {}  {}", output::fit(&l, width), dim(r)),
            }
        }
    };
    section("May:", d.capabilities.iter().map(|g| (g.means.clone(), String::new())).collect());
    // With the menus that offer it.
    let offered = |title: &str, places: &[Where]| {
        let places: Vec<&str> = places.iter().map(|w| w.name()).collect();
        format!("{title} (menus: {})", places.join(", "))
    };
    let commands =
        c.commands.iter().map(|x| (invocation(&m.id, &x.name, x.cli.as_deref()), offered(&x.title, &x.offered)));
    section("Commands:", commands.collect());
    let operations =
        c.operations.iter().map(|x| (invocation(&m.id, &x.name, x.cli.as_deref()), offered(&x.title, &x.offered)));
    section("Operations (planned, then confirmed):", operations.collect());
    let analyzers = c.analyzers.iter().map(|a| {
        let fields: Vec<String> = a.fields.keys().cloned().collect();
        (a.name.clone(), format!("{}: {}", a.title, fields.join(", ")))
    });
    section(&format!("Analyzers (locallibrary analyze --plugin {}):", m.id), analyzers.collect());
    let exporters = c.exporters.iter().map(|e| {
        let writes = e.extension.as_ref().map_or_else(|| "a folder".to_string(), |x| format!("a .{x} file"));
        (e.format.clone(), format!("{}, {writes}", e.title))
    });
    section("Export formats (locallibrary export LIBRARY --format FORMAT -o ...):", exporters.collect());
    let settings = c.settings.iter().map(|s| {
        (s.key.clone(), format!("{}: {}", s.title, d.settings.get(&s.key).map_or_else(|| "not set".into(), cell)))
    });
    section(&format!("Settings (locallibrary plugin settings {}):", m.id), settings.collect());
}

/// `plugin settings`: shows them (or one), or sets one or puts it back to its default.
pub(super) fn settings(
    plugins: &Plugins,
    plugin: String,
    key: Option<String>,
    value: Option<String>,
    reset: bool,
    store_path: &Option<PathBuf>,
    out: Output,
) -> Result<ExitCode> {
    let manifest = plugins.manifest(&plugin)?;
    let setting = key
        .as_deref()
        .map(|k| {
            manifest.contributes.settings.iter().find(|s| s.key == k).ok_or_else(|| {
                usage(format!(
                    "the {plugin} plugin has no setting {k:?} (`locallibrary plugin settings {plugin}` lists them)"
                ))
            })
        })
        .transpose()?;
    let change = match (setting, value) {
        (Some(s), Some(v)) => Some((s, Some(params::setting_value(s, &v).map_err(usage)?))),
        (Some(s), None) if reset => Some((s, None)),
        _ => None,
    };
    let Some((setting, value)) = change else {
        let store = crate::diff_cmd::existing_store(store_path)?;
        let mut settings = plugins.settings(&plugin, store.as_ref())?;
        settings.retain(|k, _| setting.is_none_or(|s| s.key == *k));
        let shown: Vec<&Setting> =
            manifest.contributes.settings.iter().filter(|s| setting.is_none_or(|o| o.key == s.key)).collect();
        out.emit(&Settings { plugin, settings }, |s| show_settings(&shown, s));
        return Ok(ExitCode::SUCCESS);
    };
    let change = match &value {
        Some(v) => format!("set the {plugin} plugin's {} to {}", setting.key, cell(v)),
        None => format!("put the {plugin} plugin's {} back to its default", setting.key),
    };
    let question = if value.is_some() { "Set it?" } else { "Put it back?" };
    out.one_step(
        &change,
        question,
        || {
            let settings =
                plugins.set_setting(&plugin, &crate::open_store(store_path)?, &setting.key, value.clone())?;
            Ok(Settings { plugin: plugin.clone(), settings })
        },
        |s| ok(format!("{} is {}", setting.key, s.settings.get(&setting.key).map_or_else(|| "not set".into(), cell))),
    )
}

fn show_settings(shown: &[&Setting], s: &Settings) {
    if shown.is_empty() {
        say!("{}", dim(format!("the {} plugin has no settings", s.plugin)));
    }
    let width = shown.iter().map(|s| s.key.width()).max().unwrap_or(0);
    for setting in shown {
        let value = s.settings.get(&setting.key);
        let default = value.is_some() && value == setting.default.as_ref();
        say!(
            "{}  {}  {}",
            output::fit(&setting.key, width),
            value.map_or_else(|| "not set".into(), cell),
            dim(format!("{}{}", setting.title, if default { " (default)" } else { "" }))
        );
    }
}
