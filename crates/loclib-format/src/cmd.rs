use std::io::Write as _;
use std::process::{Command, Stdio};

use crate::{Error, Result};

/// A command line, run without a shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cmd {
    /// The tool: a name looked up on `PATH`, or a path.
    pub program: String,
    /// Its arguments, each passed as it is.
    pub args: Vec<String>,
    /// What to write to its standard input (a script, a partition table).
    pub input: Option<String>,
}

impl Cmd {
    /// `program` with `args`, and nothing on its standard input.
    pub fn new(program: &str, args: &[&str]) -> Cmd {
        Cmd { program: program.into(), args: args.iter().map(|s| s.to_string()).collect(), input: None }
    }

    /// The same command with `input` on its standard input.
    pub fn input(mut self, input: impl Into<String>) -> Cmd {
        self.input = Some(input.into());
        self
    }

    /// Shell-style rendering, for plans and error messages.
    pub fn display(&self) -> String {
        let mut s = self.program.clone();
        for a in &self.args {
            s.push(' ');
            if a.is_empty() || a.contains(|c: char| c.is_whitespace() || "\"'$\\".contains(c)) {
                s.push_str(&format!("{a:?}"));
            } else {
                s.push_str(a);
            }
        }
        if self.input.is_some() {
            s.push_str(" <<< (script)");
        }
        s
    }

    /// Run to completion and return standard output. A non-zero exit becomes
    /// [`Error::External`] carrying the tool's own message.
    pub fn run(&self) -> Result<String> {
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(if self.input.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::External {
                command: self.program.clone(),
                message: if e.kind() == std::io::ErrorKind::NotFound {
                    format!("not found{}", install_hint(&self.program))
                } else {
                    e.to_string()
                },
            })?;
        if let (Some(input), Some(mut stdin)) = (&self.input, child.stdin.take()) {
            let _ = stdin.write_all(input.as_bytes());
        }
        let out = child
            .wait_with_output()
            .map_err(|e| Error::External { command: self.program.clone(), message: e.to_string() })?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let message = [stderr.trim(), stdout.trim()].into_iter().find(|s| !s.is_empty()).unwrap_or("no output");
            return Err(Error::External { command: self.display(), message: message.to_string() });
        }
        Ok(stdout)
    }
}

pub(crate) fn install_hint(program: &str) -> &'static str {
    match program {
        "mkfs.vfat" => " (install dosfstools)",
        "mkfs.exfat" => " (install exfatprogs)",
        "mkfs.hfsplus" => " (install hfsprogs)",
        "sfdisk" | "wipefs" | "lsblk" => " (install util-linux)",
        _ => "",
    }
}
