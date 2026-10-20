//! `plugin list`, `plugin info`, `plugin settings` and `plugin roles`.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use console::style;
use locallibrary::fields::{self, Role, roles::RoleChoice};
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
        let fields: Vec<&str> = a.fields.keys().map(String::as_str).collect();
        // Each with its field, where that's named otherwise: `loudness (integrated_lufs)`.
        let roles: Vec<String> = (a.roles.iter())
            .map(|(r, f)| if r.name() == f { f.clone() } else { format!("{} ({f})", r.name()) })
            .collect();
        let roles = if roles.is_empty() { String::new() } else { format!("; roles {}", roles.join(", ")) };
        (a.name.clone(), format!("{}: {}{roles}", a.title, fields.join(", ")))
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

/// `plugin roles`: shows which analyzer each role (or one) is read from, or chooses one, or puts
/// one back to its default.
pub(super) fn roles(
    role: Option<String>,
    analyzer: Option<String>,
    reset: bool,
    store_path: &Option<PathBuf>,
    out: Output,
) -> Result<ExitCode> {
    let role: Option<Role> = role.map(|r| r.parse().map_err(usage)).transpose()?;
    let change = match (role, analyzer) {
        (Some(r), Some(a)) => Some((r, Some(a))),
        (Some(r), None) if reset => Some((r, None)),
        _ => None,
    };
    let Some((role, analyzer)) = change else {
        let store = crate::diff_cmd::existing_store(store_path)?;
        let mut all = fields::roles::choices(store.as_ref());
        all.retain(|c| role.is_none_or(|r| r == c.role));
        out.emit(&all, |all| show_roles(all));
        return Ok(ExitCode::SUCCESS);
    };
    let change = match &analyzer {
        Some(a) => format!("read {} from {a}", role.name()),
        None => format!("read {} from its default analyzer again", role.name()),
    };
    out.one_step(
        &change,
        "Change it?",
        || Ok(fields::roles::choose(&crate::open_store(store_path)?, role, analyzer.as_deref())?),
        |all| show_roles(&all.iter().filter(|c| c.role == role).cloned().collect::<Vec<_>>()),
    )
}

fn show_roles(all: &[RoleChoice]) {
    let width = all.iter().map(|c| c.role.name().width()).max().unwrap_or(0);
    for c in all {
        let others: Vec<&String> = c.offered.iter().filter(|o| Some(*o) != c.chosen.as_ref()).collect();
        let how = match (c.set, others.is_empty()) {
            (true, _) => "chosen".to_string(),
            (false, true) => "default".to_string(),
            (false, false) => {
                format!("default; also {}", others.iter().map(|o| o.as_str()).collect::<Vec<_>>().join(", "))
            }
        };
        match &c.chosen {
            Some(a) => say!("{}  {a}  {}", output::fit(c.role.name(), width), dim(how)),
            None => say!("{}  {}", output::fit(c.role.name(), width), dim("no analyzer works it out")),
        }
    }
}
