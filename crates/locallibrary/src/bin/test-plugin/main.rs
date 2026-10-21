//! The plugin the plugin host's tests run (`tests/it/plugins.rs`): loclib-plugin's echo example,
//! with what they need besides. Its `echo` command logs (`log`), fails (`fail`, an error kind,
//! with `message` and `fields`), answers an environment variable (`env`), takes this many
//! milliseconds to stop once its input closes (`linger`), writes a file once it has stopped
//! (`stopped`), exits (`exit`), never stops (`stuck`), answers its context (`context`) or the
//! folder it runs in (`cwd`), calls back (`call` with `params`), fetches (`fetch`, a `net/fetch`
//! request, its blobs named in its scratch folder, logging what it's answered and answering what
//! it fetched into a blob) or sends its rows in parts (`partial`, `partial_items`) when its
//! params say so; its operation answers the `edits` (and `inputs`) it's given; both ask the
//! question `ask` (a `Choice`) until they're given its param (or, `ask_again`, always), the
//! command then answering that param as text; both first write the `blobs` (by name, their
//! bytes), `hard_links` and (on Unix) `links` they're given to the scratch folder; a provider
//! other than echo's answers as its `answer` setting says (see `Test::provide`); an analyzer
//! named `heard` answers the fields written in the audio file as JSON (beat grids, phrases and
//! waveforms too, writing the files its `blobs` give first), and one named `order` answers fields
//! `a` (null for a one-byte audio file) then `b`, or fails naming an empty audio file; and its
//! exporter names the playlists it's given in a note, and would write to `outside`, or writes
//! `extra` too without saying so first. Any library source reads the files in the folders it's
//! granted, and what `.json` files beside them say (see `plugin::read_library`).
//! `--manifest <file>` serves another manifest, `--protocol <version>` answers `hello` with
//! another protocol and stops, `--silent` never answers, `--wrong-fields` answers fields of the
//! wrong type, and `--without <feature>` leaves out that feature's hooks, and the feature from
//! its `hello` (the SDK's own too).

mod plugin;

use std::io::{BufRead, Write};

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
    let hooks = Test { wrong_fields }.hooks(&without);
    serve(&manifest, hooks, std::io::stdin(), Saying { out: std::io::stdout(), without, greeted: Vec::new() });
}

/// Its output, the `hello` reply (its first line) without the features it does `without`.
struct Saying {
    out: std::io::Stdout,
    without: Vec<String>,
    /// The first line so far, until it's written.
    greeted: Vec<u8>,
}

impl Write for Saying {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.without.is_empty() {
            return self.out.write(buf);
        }
        self.greeted.extend_from_slice(buf);
        if let Some(end) = self.greeted.iter().position(|&b| b == b'\n') {
            let mut reply: Value = serde_json::from_slice(&self.greeted[..end]).unwrap();
            if let Some(Value::Array(features)) = reply["result"].get_mut("features") {
                features.retain(|f| !self.without.iter().any(|w| f == w));
            }
            let rest = self.greeted.split_off(end);
            writeln!(self.out, "{reply}")?;
            self.out.write_all(&rest[1..])?;
            self.without.clear();
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
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
