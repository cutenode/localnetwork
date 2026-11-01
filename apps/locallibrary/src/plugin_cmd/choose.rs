//! Questions plugins ask on the command line (a command's `View::Choose`, an operation's
//! `{"choose": ...}`, a track's candidates from providers): asked in a terminal, the items
//! numbered from 1; otherwise, or with `--json`, refused saying how to answer them with
//! `--choose PARAM=VALUE`.

use std::io::{BufRead, IsTerminal, Write};

use anyhow::Result;
use console::style;
use indexmap::IndexMap;
use loclib_plugin::Choice;
use serde_json::Value;

use crate::jobs_cmd::usage;
use crate::output::{Output, dim};
use crate::plugins::CHOOSE;

/// `--choose PARAM=VALUE`s, by param, in the order given: each value as JSON, else as text
/// (`--choose name=Peak`).
pub fn given(pairs: &[String]) -> Result<IndexMap<String, Value>> {
    let mut given = IndexMap::new();
    for pair in pairs {
        let (param, raw) =
            pair.split_once('=').ok_or_else(|| usage(format!("--{CHOOSE} takes PARAM=VALUE, not {pair:?}")))?;
        let value = serde_json::from_str(raw).unwrap_or_else(|_| Value::from(raw));
        given.insert(param.trim().to_string(), value);
    }
    Ok(given)
}

/// Whether questions can be asked here: in a terminal, without `--json`.
pub fn interactive(out: Output) -> bool {
    !out.json && std::io::stdin().is_terminal()
}

/// The values chosen for each of `choice`'s questions, asked on stderr and read from stdin (see
/// [`answers`]); refused, saying how to answer every one, when questions can't be asked here
/// (see [`unanswered`]).
pub fn ask(choice: &Choice, out: Output) -> Result<Vec<Vec<Value>>> {
    if !interactive(out) {
        return Err(usage(unanswered(&choice.questions().collect::<Vec<_>>())));
    }
    answers(choice, &mut std::io::stdin().lock())
}

/// Each of `choice`'s questions asked in turn (see [`answer`]), with how far along it is when
/// there are several.
pub(super) fn answers(choice: &Choice, input: &mut impl BufRead) -> Result<Vec<Vec<Value>>> {
    let n = choice.more.len() + 1;
    let count = |i: usize| match n {
        1 => String::new(),
        _ => format!(" {}", dim(format!("({} of {n})", i + 1))),
    };
    choice.questions().enumerate().map(|(i, q)| answer(q, &count(i), input)).collect()
}

/// `choice` asked on stderr (`count` after its title), its items numbered, and its answer read
/// from `input`, asked again until it's one: refused when `input` ends first.
pub(super) fn answer(choice: &Choice, count: &str, input: &mut impl BufRead) -> Result<Vec<Value>> {
    eprintln!("{} {}{count}", style("?").cyan().bold(), choice.title);
    for (i, item) in choice.items.iter().enumerate() {
        let detail = item.detail.as_deref().map(|d| format!("  {}", dim(d))).unwrap_or_default();
        eprintln!("  {:>3}. {}{detail}", i + 1, item.label);
    }
    loop {
        match choice.multi {
            true => eprint!("  numbers, with commas (none: empty): "),
            false => eprint!("  number: "),
        }
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Err(usage(format!("{}: no answer", choice.title)));
        }
        match picked(choice, &line) {
            Ok(values) => return Ok(values),
            Err(why) => eprintln!("  {why}"),
        }
    }
}

/// The values of the items `line` names, numbered from 1: one, or with `multi` any of them,
/// with commas (none, empty).
pub(super) fn picked(choice: &Choice, line: &str) -> std::result::Result<Vec<Value>, String> {
    let numbers: Vec<&str> = line.split(',').map(str::trim).filter(|n| !n.is_empty()).collect();
    let n = choice.items.len();
    if !choice.multi && numbers.len() != 1 {
        return Err(format!("one number, 1 to {n}"));
    }
    let item = |raw: &str| match raw.parse::<usize>() {
        Ok(i) if (1..=n).contains(&i) => Ok(choice.items[i - 1].value.clone()),
        _ => Err(format!("{raw:?} isn't a number from 1 to {n}")),
    };
    numbers.into_iter().map(item).collect()
}

/// What to give the command line to answer `questions`: `--choose PARAM=VALUE` for each, with
/// the values each takes.
pub fn unanswered(questions: &[&Choice]) -> String {
    let mut said = Vec::new();
    for c in questions {
        let takes = match c.multi {
            true => "a JSON list of any of",
            false => "one of",
        };
        said.push(format!("{}: answer with --{CHOOSE} {}=VALUE, VALUE {takes}:", c.title, c.param));
        let values: Vec<String> = c.items.iter().map(|i| i.value.to_string()).collect();
        let width = values.iter().map(String::len).max().unwrap_or(0);
        for (value, item) in values.iter().zip(&c.items) {
            let detail = item.detail.as_deref().map(|d| format!(" ({d})")).unwrap_or_default();
            said.push(format!("  {value:<width$}  {}{detail}", item.label));
        }
    }
    said.join("\n")
}

#[cfg(test)]
mod tests;
