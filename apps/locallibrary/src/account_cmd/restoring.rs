//! Signing in again to an account plugins had access to before it was signed out: what came back
//! is said in a line, and what's still remembered is asked about in a terminal (restore it all,
//! choose from it, or reset it). Elsewhere it stays off, and the command that restores it is said.

use std::io::BufRead;

use anyhow::Result;
use locallibrary::Store;
use locallibrary::plugin::{Access, AccountId, AccountSignedIn, Chosen, Plugins};
use loclib_plugin::{Choice, ChoiceItem};
use serde_json::{Value, json};

use super::accounts::{account_type, called, titles};
use crate::output::{Output, dim, hint, ok};
use crate::plugin_cmd::choose::{answer, interactive};

/// After signing in to `done`'s account: the access restored said, and in a terminal, what's
/// still remembered asked about and done as the user says (see [`ask`]); elsewhere, how to
/// restore it.
pub(super) fn after_sign_in(plugins: &Plugins, done: &AccountSignedIn, store: &Store, out: Output) -> Result<()> {
    if out.json {
        return Ok(());
    }
    restored(plugins, &done.restored);
    if done.remembered.is_empty() {
        return Ok(());
    }
    let a = &done.account;
    if !interactive(out) {
        let names: Vec<&str> = done.remembered.iter().map(|x| x.plugin.as_str()).collect();
        hint(format!(
            "{} had access to it: `locallibrary account {}:{} restore` gives it back",
            names.join(", "),
            a.kind,
            a.subject
        ));
        return Ok(());
    }
    say!("Plugins had access to the {} before it was signed out:", called(plugins, a));
    for x in &done.remembered {
        say!("  {}", shown(plugins, x));
    }
    let chosen = ask(plugins, &done.remembered, &mut std::io::stdin().lock())?;
    let account = AccountId { plugin: a.plugin.clone(), subject: a.subject.clone() };
    let back = plugins.restore_access(&account, chosen.as_deref(), store)?;
    match back.is_empty() {
        true => ok("Reset: no plugin has access to it"),
        false => restored(plugins, &back),
    }
    Ok(())
}

/// Says in a line which plugins got their access back, and to what, if any did.
pub(super) fn restored(plugins: &Plugins, back: &[Access]) {
    if !back.is_empty() {
        let each: Vec<String> = back.iter().map(|x| shown(plugins, x)).collect();
        ok(format!("Restored access: {}", each.join("; ")));
    }
}

/// `x`'s plugin, whether it's its default, and its scopes' titles.
fn shown(plugins: &Plugins, x: &Access) -> String {
    let t = account_type(plugins, &x.account.plugin, &x.kind);
    let default = if x.default { " (default)" } else { "" };
    format!("{}{default} {}", x.plugin, dim(format!("· {}", titles(t, &x.scopes).join(", "))))
}

/// What to do with the access `remembered`, read from `input`: restore it all (`None`), or as
/// chosen, plugin by plugin and then the scopes each can do without (none chosen: reset).
pub(super) fn ask(plugins: &Plugins, remembered: &[Access], input: &mut impl BufRead) -> Result<Option<Vec<Chosen>>> {
    let item = |label: &str, value: &str| ChoiceItem { label: label.into(), detail: None, value: json!(value) };
    let what = Choice {
        title: "Give it back?".into(),
        param: "restore".into(),
        multi: false,
        items: vec![item("Restore", "restore"), item("Choose", "choose"), item("Reset", "reset")],
        more: Vec::new(),
    };
    match answer(&what, "", input)?.first().and_then(Value::as_str) {
        Some("restore") => return Ok(None),
        Some("reset") => return Ok(Some(Vec::new())),
        _ => {}
    }
    let items = remembered.iter().enumerate();
    let items = items.map(|(i, x)| ChoiceItem { label: x.plugin.clone(), detail: None, value: json!(i) }).collect();
    let which =
        Choice { title: "Which plugins?".into(), param: "plugins".into(), multi: true, items, more: Vec::new() };
    let mut chosen = Vec::new();
    for i in answer(&which, "", input)?.iter().filter_map(Value::as_u64) {
        let x = &remembered[i as usize];
        chosen.push(Chosen { plugin: x.plugin.clone(), scopes: scopes(plugins, x, input)? });
    }
    Ok(Some(chosen))
}

/// The scopes `x`'s plugin gets back: those it can do without that are chosen from `input`
/// (besides those it needs), else those it had when it can do without none.
fn scopes(plugins: &Plugins, x: &Access, input: &mut impl BufRead) -> Result<Vec<String>> {
    let Ok(plugin) = plugins.manifest(&x.plugin) else { return Ok(x.scopes.clone()) };
    let need = plugin.accounts.iter().find(|n| n.name == x.kind);
    let optional: Vec<&String> = need.iter().flat_map(|n| n.optional.iter().flat_map(|o| &o.scopes)).collect();
    if optional.is_empty() {
        return Ok(x.scopes.clone());
    }
    let t = account_type(plugins, &x.account.plugin, &x.kind);
    let items = optional.iter().map(|s| {
        let had = if x.scopes.contains(s) { Some("had it".to_string()) } else { None };
        let label = titles(t, std::slice::from_ref(*s)).join("");
        ChoiceItem { label, detail: had, value: json!(s) }
    });
    // Named from its manifest: what the question prints comes from the plugin, none of it from
    // the sign-in's answer.
    let title = format!("Which may {} do besides what it needs?", plugin.id);
    let choice = Choice { title, param: "scopes".into(), multi: true, items: items.collect(), more: Vec::new() };
    Ok(answer(&choice, "", input)?.iter().filter_map(Value::as_str).map(String::from).collect())
}
