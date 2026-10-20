//! The plugin the plugin host's tests run (`tests/it/plugins.rs`): loclib-plugin's echo example,
//! with what they need besides. Its `echo` command logs (`log`), fails (`fail`, an error kind,
//! with `message` and `fields`), answers an environment variable (`env`), takes this many
//! milliseconds to stop once its input closes (`linger`), writes a file once it has stopped
//! (`stopped`), exits (`exit`), never stops (`stuck`), answers its context (`context`) or the
//! folder it runs in (`cwd`), or calls back (`call` with `params`) when its params say so; its
//! operation answers the `edits` (and `inputs`) it's given; an analyzer named `order` answers
//! fields `a` (null for a one-byte audio file) then `b`, or fails naming an empty audio file;
//! and its exporter names the playlists it's given in a note, and would write to `outside`, or
//! writes `extra` too without saying so first. `--manifest <file>` serves another manifest,
//! `--protocol <version>` answers `hello` with another protocol and stops, `--silent` never
//! answers, `--wrong-fields` answers fields of the wrong type, and `--without <feature>` leaves
//! out that feature's hooks.

mod plugin;

use std::io::BufRead;

use loclib_plugin::serve;
use plugin::{Test, echo};
use serde_json::{Value, json};

fn main() {
    let (mut manifest, mut wrong_fields, mut without) = (echo::MANIFEST.to_string(), false, Vec::new());
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().expect("a value");
        match arg.as_str() {
            "--manifest" => manifest = std::fs::read_to_string(value()).unwrap(),
            "--protocol" => return hello(&value()),
            "--silent" => return std::io::stdin().lines().for_each(drop),
            "--wrong-fields" => wrong_fields = true,
            "--without" => without.push(value()),
            other => panic!("unknown argument {other}"),
        }
    }
    serve(&manifest, Test { wrong_fields }.hooks(&without), std::io::stdin(), std::io::stdout());
}

/// Answers `hello` speaking `protocol`, and stops.
fn hello(protocol: &str) {
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).unwrap();
    let hello: Value = serde_json::from_str(&line).unwrap();
    let plugin = json!({ "id": "echo", "version": "0.1.0" });
    println!(
        "{}",
        json!({ "jsonrpc": "2.0", "id": hello["id"], "result": { "protocol": protocol, "plugin": plugin } })
    );
}
