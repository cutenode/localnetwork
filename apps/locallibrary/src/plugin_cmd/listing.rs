//! `plugin list`, `plugin info`, `plugin settings`, `plugin roles` and `plugin providers`.

use std::io::{BufRead, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use console::style;
use indexmap::IndexMap;
use locallibrary::fields::{self, Role, roles::RoleChoice};
use locallibrary::plugin::Plugins;
use loclib_plugin::{ExportFrom, Setting, SettingType, Where};
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use crate::jobs_cmd::usage;
use crate::output::{self, Output, dim, heading, ok};
use crate::plugins::params::{self, cell};
use crate::plugins::providers::Providers;
use crate::plugins::{Described, Listed, Secrets, Settings};

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
            (!p.events.is_empty()).then(|| format!("listens for {}", p.events.join(", "))),
            named("operation", &p.operations),
            named("analyzer", &p.analyzers),
            named("export format", &p.exporters),
            named("import format", &p.importers),
            named("provider", &p.providers),
            named("panel", &p.panels),
            named("service", &p.services),
            named("theme", &p.themes),
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
    let events = c.events.iter().flat_map(|x| &x.on).map(|e| (e.name().to_string(), String::new()));
    section("Listens for:", events.collect());
    let operations =
        c.operations.iter().map(|x| (invocation(&m.id, &x.name, x.cli.as_deref()), offered(&x.title, &x.offered)));
    section("Operations (planned, then confirmed):", operations.collect());
    let panels = c.panels.iter().map(|p| (p.name.clone(), format!("{}, under a track's details", p.title)));
    section("Panels (locallibrary track show, and the player):", panels.collect());
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
    // Each kind of what they write out, and how it's run.
    let of = [
        (ExportFrom::Drive, "Export formats (locallibrary export LIBRARY --format FORMAT -o ...):"),
        (ExportFrom::Set, "Set export formats (locallibrary sets export SET --format FORMAT -o ...):"),
        (ExportFrom::Mix, "Mix export formats (locallibrary mix RECORDING --format FORMAT --export ...):"),
    ];
    for (source, title) in of {
        let exporters = c.exporters.iter().filter(|e| e.source == source).map(|e| {
            let writes = e.extension.as_ref().map_or_else(|| "a folder".to_string(), |x| format!("a .{x} file"));
            (e.format.clone(), format!("{}, {writes}", e.title))
        });
        section(title, exporters.collect());
    }
    let importers = c.importers.iter().map(|i| {
        let reads = i.extension.as_ref().map_or_else(|| "a file".to_string(), |x| format!("a .{x} file"));
        let provides: Vec<&str> = i.provides.iter().map(|k| k.name()).collect();
        (i.format.clone(), format!("{}, {reads}: {}", i.title, provides.join(", ")))
    });
    section("Import formats (locallibrary import FILE LIBRARY --format FORMAT):", importers.collect());
    let providers = c.providers.iter().map(|p| {
        let verb = match p.kind {
            loclib_plugin::ProviderKind::Artwork => "art fetch",
            loclib_plugin::ProviderKind::Metadata => "track fetch",
        };
        (p.name.clone(), format!("{}: {}, for locallibrary {verb}", p.title, p.kind.found()))
    });
    section("Providers:", providers.collect());
    let services = c.services.iter().map(|s| {
        let on = d.settings.get(&format!("{}{}", locallibrary::plugin::SERVICES, s.name)) == Some(&Value::Bool(true));
        (s.name.clone(), format!("{}: {}", s.title, if on { "on" } else { "off" }))
    });
    section(&format!("Services (locallibrary plugin settings {} services.NAME on):", m.id), services.collect());
    let themes = c.themes.iter().map(|t| (t.name.clone(), format!("{}, in the player", t.title)));
    section("Themes:", themes.collect());
    let settings =
        c.settings.iter().map(|s| (s.key.clone(), format!("{}: {}", s.title, shown(s, &d.settings, &d.secrets))));
    section(&format!("Settings (locallibrary plugin settings {}):", m.id), settings.collect());
    unknown(&d.secrets);
}

/// Why whether its secrets are set isn't known, when it isn't.
fn unknown(secrets: &Secrets) {
    if let Some(why) = &secrets.unknown {
        say!("{}", dim(format!("whether its secrets are set isn't known: {why}")));
    }
}

/// How `plugin settings` changes a setting: to `value`, back to its default (`reset`), or to a
/// secret read from stdin (`secret`).
pub(super) struct Change {
    pub(super) value: Option<String>,
    pub(super) reset: bool,
    pub(super) secret: bool,
}

/// A setting's value as `plugin settings` and `plugin info` show it: a secret only as set or not
/// (or unknown, when the keychain can't say).
fn shown(s: &Setting, settings: &IndexMap<String, Value>, secrets: &Secrets) -> String {
    match (s.kind, secrets.set.get(&s.key)) {
        (SettingType::Secret, Some(Some(true))) => "set".into(),
        (SettingType::Secret, Some(None)) => "unknown".into(),
        _ => settings.get(&s.key).map_or_else(|| "not set".into(), cell),
    }
}

/// A secret read from stdin: typed without being shown in a terminal, else its first line.
fn read_secret(key: &str) -> Result<String> {
    let secret = match std::io::stdin().is_terminal() {
        true => {
            eprint!("{key}: ");
            console::Term::stderr().read_secure_line()?
        }
        false => {
            let mut line = String::new();
            std::io::stdin().lock().read_line(&mut line)?;
            line
        }
    };
    let secret = secret.trim_end_matches(['\r', '\n']);
    if secret.is_empty() {
        return Err(usage(format!("no {key} came on stdin")));
    }
    Ok(secret.into())
}

/// `plugin settings`: shows them (or one), or sets one or puts it back to its default.
pub(super) fn settings(
    plugins: &Plugins,
    plugin: String,
    key: Option<String>,
    change: Change,
    store_path: &Option<PathBuf>,
    out: Output,
) -> Result<ExitCode> {
    let listed = plugins.setting_list(&plugin)?;
    let setting = key
        .as_deref()
        .map(|k| {
            listed.iter().find(|s| s.key == k).ok_or_else(|| {
                usage(format!(
                    "the {plugin} plugin has no setting {k:?} (`locallibrary plugin settings {plugin}` lists them)"
                ))
            })
        })
        .transpose()?;
    // Not named `secret`: CodeQL would take it for the secret itself, and the key it prompts with for a logged secret.
    let keychained = setting.filter(|s| s.kind == SettingType::Secret);
    let value = match keychained {
        Some(s) if change.secret => Some(read_secret(&s.key)?),
        None if change.secret => {
            return Err(usage(format!("{} isn't a secret: give its value", key.unwrap_or_default())));
        }
        Some(s) if change.value.is_some() => {
            return Err(usage(format!(
                "a secret isn't given on the command line, which keeps it in your shell's history: `locallibrary plugin settings {plugin} {} --secret` reads it from stdin",
                s.key
            )));
        }
        _ => change.value,
    };
    let change = match (setting, value) {
        (Some(s), Some(v)) => Some((s, Some(params::setting_value(s, &v).map_err(usage)?))),
        (Some(s), None) if change.reset => Some((s, None)),
        _ => None,
    };
    let Some((setting, value)) = change else {
        let store = crate::diff_cmd::existing_store(store_path)?;
        let mut settings = plugins.settings(&plugin, store.as_ref())?;
        settings.retain(|k, _| setting.is_none_or(|s| s.key == *k));
        let mut secrets = Secrets::of(plugins, &plugin, store.as_ref())?;
        secrets.set.retain(|k, _| setting.is_none_or(|s| s.key == *k));
        let shown: Vec<&Setting> = listed.iter().filter(|s| setting.is_none_or(|o| o.key == s.key)).collect();
        out.emit(&Settings { plugin, settings, secrets }, |s| show_settings(&shown, s));
        return Ok(ExitCode::SUCCESS);
    };
    let change = match (&value, keychained.is_some()) {
        (Some(_), true) => format!("set the {plugin} plugin's {}", setting.key),
        (Some(v), false) => format!("set the {plugin} plugin's {} to {}", setting.key, cell(v)),
        (None, true) => format!("forget the {plugin} plugin's {}", setting.key),
        (None, false) => format!("put the {plugin} plugin's {} back to its default", setting.key),
    };
    let question = match (value.is_some(), keychained.is_some()) {
        (true, _) => "Set it?",
        (false, true) => "Forget it?",
        (false, false) => "Put it back?",
    };
    out.one_step(
        &change,
        question,
        || {
            let store = crate::open_store(store_path)?;
            let settings = plugins.set_setting(&plugin, &store, &setting.key, value.clone())?;
            let secrets = Secrets::of(plugins, &plugin, Some(&store))?;
            Ok(Settings { plugin: plugin.clone(), settings, secrets })
        },
        |s| ok(format!("{} is {}", setting.key, shown(setting, &s.settings, &s.secrets))),
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
            self::shown(setting, &s.settings, &s.secrets),
            dim(format!("{}{}", setting.title, if default { " (default)" } else { "" }))
        );
    }
    unknown(&s.secrets);
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

/// `plugin providers`: shows the providers and the score a candidate alone scoring it or more is
/// chosen with, or sets that score, or puts it back to its default.
pub(super) fn providers(
    plugins: &Plugins,
    auto_pick: Option<f64>,
    reset: bool,
    store_path: &Option<PathBuf>,
    out: Output,
) -> Result<ExitCode> {
    if auto_pick.is_none() && !reset {
        let store = crate::diff_cmd::existing_store(store_path)?;
        out.emit(&Providers::of(plugins, store.as_ref()), show_providers);
        return Ok(ExitCode::SUCCESS);
    }
    let change = match auto_pick {
        Some(score) => format!("choose a candidate alone scoring {score} or more"),
        None => format!("choose a candidate alone scoring {} or more, as by default", locallibrary::plugin::AUTO_PICK),
    };
    out.one_step(
        &change,
        "Change it?",
        || {
            let store = crate::open_store(store_path)?;
            locallibrary::plugin::set_auto_pick(&store, auto_pick)?;
            Ok(Providers::of(plugins, Some(&store)))
        },
        show_providers,
    )
}

fn show_providers(p: &Providers) {
    if p.providers.is_empty() {
        say!("{}", dim("no plugin has a provider"));
    }
    let width = p.providers.iter().map(|x| x.plugin.width() + x.name.width() + 1).max().unwrap_or(0);
    let kinds = p.providers.iter().map(|x| x.kind.name().len()).max().unwrap_or(0);
    for x in &p.providers {
        let name = format!("{}.{}", x.plugin, x.name);
        say!("{}  {}  {}", output::fit(&name, width), output::fit(x.kind.name(), kinds), dim(&x.title));
    }
    say!("{}", dim(format!("a candidate alone scoring {} or more is chosen for you", p.auto_pick)));
}
