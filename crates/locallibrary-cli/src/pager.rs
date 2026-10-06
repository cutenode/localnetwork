//! A long listing in a terminal goes through a pager, as git's do: `$PAGER` (run by the shell),
//! else `less` with `LESS=FRX` unless `LESS` is set, so a listing that fits on the screen is
//! printed and the pager quits. Only on Unix, and only when stdout is a terminal; `--no-pager`
//! and `PAGER=` (empty) or `PAGER=cat` turn it off.

/// Send stdout through the pager from here on (once; nothing when stdout isn't a terminal).
pub fn start() {
    #[cfg(unix)]
    unix::start();
}

/// Let the pager show what was written, and wait for it to be closed.
pub fn finish() {
    #[cfg(unix)]
    unix::finish();
}

/// Writing to a closed pipe (`locallibrary tracks Main | head`) ends the process quietly, as it
/// does for other command-line tools, instead of failing the write.
pub fn default_sigpipe() {
    #[cfg(unix)]
    // SAFETY: setting a signal's disposition to the default has no preconditions.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(unix)]
mod unix {
    use std::io::{IsTerminal, Write};
    use std::os::fd::AsRawFd;
    use std::process::{Child, Command, Stdio};
    use std::sync::Mutex;

    /// The pager, and stdout as it was before.
    static PAGER: Mutex<Option<(Child, i32)>> = Mutex::new(None);

    pub fn start() {
        let mut pager = PAGER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if pager.is_some() || !std::io::stdout().is_terminal() {
            return;
        }
        let program = std::env::var("PAGER").unwrap_or_else(|_| "less".into());
        if program.trim().is_empty() || program.trim() == "cat" {
            return;
        }
        let mut command = Command::new("sh");
        command.arg("-c").arg(&program).stdin(Stdio::piped());
        if std::env::var_os("LESS").is_none() {
            command.env("LESS", "FRX");
        }
        // Coloured for the terminal, coloured through the pager (`less -R` shows it).
        let colors = console::colors_enabled();
        let Ok(mut child) = command.spawn() else { return };
        let Some(input) = child.stdin.take() else { return };
        let _ = std::io::stdout().flush();
        // SAFETY: plain file-descriptor calls on descriptors this process owns; the pager's end
        // of the pipe stays open as stdout until `finish`.
        let saved = unsafe { libc::dup(1) };
        if saved < 0 || unsafe { libc::dup2(input.as_raw_fd(), 1) } < 0 {
            let _ = child.kill();
            return;
        }
        drop(input);
        console::set_colors_enabled(colors);
        *pager = Some((child, saved));
    }

    pub fn finish() {
        let mut pager = PAGER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some((mut child, saved)) = pager.take() else { return };
        let _ = std::io::stdout().flush();
        // SAFETY: stdout goes back to what it was, which closes the pipe: the pager sees the end.
        unsafe {
            libc::dup2(saved, 1);
            libc::close(saved);
        }
        let _ = child.wait();
    }
}
