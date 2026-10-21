//! The commands and operations plugins add to the command line: `locallibrary <plugin> <name>`,
//! or the place a manifest gives one (`import`, `playlist import`), with a flag for each param
//! (see [`params`]), and `--choose PARAM=VALUE` for a question one asks (see [`super::choose`]).

use std::cell::Cell;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context as _, Result};
use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction, ArgMatches};
use locallibrary::plugin::{Cli, CommandOutput, OpPlanned, Plugins};
use locallibrary::progress::Ctx;
use loclib_plugin::{Context, Manifest, Selection, View};
use serde_json::{Map, Value};

use super::{choose, view};
use crate::jobs_cmd::usage;
use crate::output::{Output, heading};
use crate::plugins::params::{self, Takes, properties, takes};
use crate::plugins::{self, CHOOSE, DRY_RUN, PARAMS};

/// The id of the argument a contributed command's param `name` is given with: never one of the
/// command's own (`drive`, `params`, `dry_run`).
fn param_id(name: &str) -> String {
    format!("param.{name}")
}

/// The positional argument naming the drive, and `--dry-run`'s and `--choose`'s ids (what
/// `--json`'s help reads).
const DRIVE: &str = "drive";
const DRY_RUN_ID: &str = "dry_run";
const CHOOSE_ID: &str = "choose";

/// The command line's command for a plugin's command or operation, called `name`: the drive,
/// then the positional param if there's one, and a flag for each other param it can take as
/// text, in the schema's order; required ones unless `--params` gives them, or they're the drive
/// (`x-from = "selection.drive"`), which the drive given stands for. The command line has no
/// other selection. `--choose` answers a question it asks.
fn contributed(name: &str, title: &str, params: Option<&Value>, operation: bool) -> clap::Command {
    let drive = match operation {
        true => {
            "A library's name, or a drive's mount path, name or id prefix [default: the only mounted \
rekordbox drive, or the one library plugged in]"
        }
        false => "A library's name, or a drive's mount path, name or id prefix, for it to read",
    };
    let required: Vec<&str> =
        params.and_then(|p| p["required"].as_array()).into_iter().flatten().filter_map(Value::as_str).collect();
    let mut c = clap::Command::new(name.to_string())
        .about(title.to_string())
        .arg(Arg::new(DRIVE).value_name("DRIVE").help(drive))
        .arg(Arg::new(PARAMS).long(PARAMS).value_name("JSON").help("Params as a JSON object (flags given win)"))
        .arg(
            Arg::new(CHOOSE_ID)
                .long(CHOOSE)
                .value_name("PARAM=VALUE")
                .action(ArgAction::Append)
                .help("Answer a question it asks, VALUE as JSON or text (asked in a terminal otherwise)"),
        );
    for (param, schema) in properties(params) {
        let Some(kind) = takes(schema) else { continue };
        let about = schema["description"].as_str().or(schema["title"].as_str()).unwrap_or_default();
        let the_drive = Selection::of(schema) == Ok(Some(Selection::Drive));
        let about = match the_drive {
            true => format!("{about} [default: the drive]").trim_start().to_string(),
            false => about.to_string(),
        };
        let mut arg = Arg::new(param_id(param)).help(about.clone());
        arg = match params::positional(schema) {
            true => arg.index(2),
            false => arg.long(params::flag(param, schema)),
        };
        // Usage doesn't show a flag `--params` can stand in for as required: its help says so.
        if kind != Takes::Switch && required.contains(&param.as_str()) && !the_drive {
            let help = format!("{about} (required)").trim_start().to_string();
            arg = arg.required_unless_present(PARAMS).help(help);
        }
        let choices = |a: Arg, s: &Value| match s["enum"].as_array() {
            Some(e) => {
                let named = e.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), String::from));
                a.value_parser(PossibleValuesParser::new(named))
            }
            None => a,
        };
        c = c.arg(match kind {
            Takes::Switch => arg.action(ArgAction::SetTrue),
            Takes::One(_) => choices(arg.value_name(param.to_uppercase()), schema),
            Takes::Many(_) => choices(arg.value_name(param.to_uppercase()), &schema["items"]).action(ArgAction::Append),
            Takes::Pairs(_) => arg.value_name("KEY=VALUE").action(ArgAction::Append),
        });
    }
    if operation {
        let dry_run = Arg::new(DRY_RUN_ID).long(DRY_RUN).action(ArgAction::SetTrue);
        c = c.arg(dry_run.help("Show the plan without changing anything"));
    }
    c
}

/// `cmd` with the commands and operations plugins add: `locallibrary <plugin> <name>` for each,
/// and each at the place its manifest gives it (`import`, `playlist import`).
pub fn add_to(mut cmd: clap::Command, plugins: &Plugins) -> clap::Command {
    for m in plugins.manifests() {
        let c = &m.contributes;
        let commands =
            c.commands.iter().map(|x| (x.name.as_str(), &x.title, x.params.as_ref(), x.cli.as_deref(), false));
        let operations =
            c.operations.iter().map(|x| (x.name.as_str(), &x.title, x.params.as_ref(), x.cli.as_deref(), true));
        let all: Vec<_> = commands.chain(operations).collect();
        if !all.is_empty() {
            let own = all.iter().map(|&(name, title, params, _, op)| contributed(name, title, params, op));
            let group = clap::Command::new(m.id.clone()).about(format!("{} (a plugin)", m.name));
            cmd = cmd.subcommand(group.subcommand_required(true).subcommands(own));
        }
        for &(_, title, params, cli, op) in &all {
            let Some(place) = cli else { continue };
            cmd = match place.split_once(' ') {
                Some((group, name)) => {
                    cmd.mut_subcommand(group, |g| g.subcommand(contributed(name, title, params, op)))
                }
                None => cmd.subcommand(contributed(place, title, params, op)),
            };
        }
    }
    cmd
}

/// A command or operation a plugin adds, as the command line gave it: where (its place, or the
/// plugin's id) and its own arguments.
pub struct Contributed {
    place: String,
    matches: ArgMatches,
}

/// Takes a plugin's command or operation out of `matches`, which [`crate::Cli`] doesn't know.
pub fn take(matches: &mut ArgMatches) -> Option<Contributed> {
    let plugins = plugins::host().ok()?;
    let (name, sub) = matches.subcommand()?;
    let nested = sub.subcommand_name().map(|n| format!("{name} {n}")).filter(|p| plugins.cli(p).is_some());
    let place = nested.unwrap_or_else(|| name.to_string());
    let ours = plugins.cli(&place).is_some() || plugins.manifest(&place).is_ok();
    if !ours {
        return None;
    }
    let (_, mut sub) = matches.remove_subcommand()?;
    let matches = if place.contains(' ') { sub.remove_subcommand()?.1 } else { sub };
    Some(Contributed { place, matches })
}

/// Runs a plugin's command (showing what it answers) or operation (planned, shown, asked about
/// and applied as any edit is).
pub fn run_contributed(c: Contributed, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let plugins = plugins::host()?;
    let (plugin, what, m) = match plugins.cli(&c.place) {
        Some((plugin, what)) => (plugin, what, &c.matches),
        None => {
            let (name, m) = c.matches.subcommand().context("a plugin's command")?;
            let manifest: &Manifest = plugins.manifest(&c.place)?;
            let operation = manifest.contributes.operations.iter().find(|o| o.name == name).map(Cli::Operation);
            let command = || manifest.contributes.commands.iter().find(|x| x.name == name).map(Cli::Command);
            (manifest.id.as_str(), operation.or_else(command).context("a plugin's command")?, m)
        }
    };
    let store = crate::diff_cmd::existing_store(store_path)?;
    let drive = m.get_one::<String>(DRIVE).cloned();
    match what {
        Cli::Command(command) => {
            let mut params = params_of(command.params.as_ref(), m)?;
            let context = Context { drive, ..Context::default() };
            // The rows (and items) drawn as they came, which the view has first.
            let drawn = Cell::new((0, 0));
            let shown = |p: loclib_plugin::Partial| {
                if !out.json {
                    view::plain(&p.rows, &p.items);
                    let (rows, items) = drawn.get();
                    drawn.set((rows + p.rows.len(), items + p.items.len()));
                }
            };
            let answer = loop {
                let run = plugins.run_command(
                    plugin,
                    &command.name,
                    params.clone(),
                    context.clone(),
                    store.as_ref(),
                    ctx,
                    &shown,
                );
                match run? {
                    CommandOutput::View(View::Choose(c)) => {
                        // Run again, its rows come again.
                        drawn.set((0, 0));
                        params = c.answered(params, choose::ask(&c, out)?);
                    }
                    answer => break answer,
                }
            };
            let names = view::names_on(context.drive.as_deref());
            match answer {
                CommandOutput::Headless(v) => out.emit(&v, view::headless),
                CommandOutput::View(v) if drawn.get() == (0, 0) => out.emit_paged(&v, |v| view::view(v, &names)),
                CommandOutput::View(v) => out.emit(&v, |v| view::rest(v, drawn.get(), &names)),
            }
            Ok(ExitCode::SUCCESS)
        }
        Cli::Operation(op) => {
            let drive = crate::jobs_cmd::drive_or_default(drive, store_path, &op.name)?;
            let mut params = params_of(op.params.as_ref(), m)?;
            let context = Context { drive: Some(drive.clone()), ..Context::default() };
            let planned = loop {
                match plugins.plan_op(plugin, &op.name, params.clone(), context.clone(), store.as_ref(), ctx)? {
                    OpPlanned::Choose { choose: c } => params = c.answered(params, choose::ask(&c, out)?),
                    OpPlanned::Plan(planned) => break planned,
                }
            };
            if !out.json {
                heading(&planned.summary[0]);
            }
            crate::edit_cmd::confirm_and_apply(&drive, planned, m.get_flag(DRY_RUN_ID), out, ctx)
        }
    }
}

/// The params the command line gives: `--params`' object, with the flags given over it, and
/// `--choose`'s answers over those.
fn params_of(schema: Option<&Value>, m: &ArgMatches) -> Result<Value> {
    let mut params = match m.get_one::<String>(PARAMS) {
        Some(json) => match serde_json::from_str(json) {
            Ok(Value::Object(o)) => o,
            _ => return Err(usage(format!("--{PARAMS} takes a JSON object, not {json}"))),
        },
        None => Map::new(),
    };
    for (name, schema) in properties(schema) {
        let id = param_id(name);
        let raws: Vec<&str> = match takes(schema) {
            None => continue,
            Some(Takes::Switch) if m.get_flag(&id) => vec!["true"],
            Some(Takes::Switch) => continue,
            Some(_) => m.get_many::<String>(&id).into_iter().flatten().map(String::as_str).collect(),
        };
        if raws.is_empty() {
            continue;
        }
        let what = match params::positional(schema) {
            true => name.to_uppercase(),
            false => format!("--{}", params::flag(name, schema)),
        };
        params.insert(name.clone(), params::value(schema, &raws, &what).map_err(usage)?);
    }
    let answers: Vec<String> = m.get_many::<String>(CHOOSE_ID).into_iter().flatten().cloned().collect();
    params.extend(choose::given(&answers)?);
    Ok(Value::Object(params))
}

/// An exporter's options from `--option KEY=VALUE`s (one given again adds to a list or a map),
/// each read as its schema says. A key it doesn't take is refused.
pub fn options(schema: Option<&Value>, given: &[String]) -> Result<Value> {
    let mut raws: Vec<(&str, Vec<&str>)> = Vec::new();
    for pair in given {
        let (key, raw) =
            pair.split_once('=').ok_or_else(|| usage(format!("--option takes KEY=VALUE, not {pair:?}")))?;
        let key = key.trim();
        match raws.iter_mut().find(|(k, _)| *k == key) {
            Some((_, values)) => values.push(raw),
            None => raws.push((key, vec![raw])),
        }
    }
    let mut options = Map::new();
    for (key, raws) in raws {
        let known = properties(schema).find(|(name, _)| *name == key).map(|(_, p)| p);
        let property = known.ok_or_else(|| {
            let all: Vec<&str> = properties(schema).map(|(name, _)| name.as_str()).collect();
            usage(match all.is_empty() {
                true => format!("this format has no options, not even {key:?}"),
                false => format!("this format has no option {key:?}: its options are {}", all.join(", ")),
            })
        })?;
        options.insert(key.to_string(), params::value(property, &raws, &format!("the {key} option")).map_err(usage)?);
    }
    Ok(Value::Object(options))
}

#[cfg(test)]
mod tests;
