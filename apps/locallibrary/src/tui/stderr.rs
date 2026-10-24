//! What other code writes to stderr while the player draws on the terminal would draw over the
//! screen: alsa-lib, for one, prints its complaints there from any thread when there's no sound
//! card. It goes to the messages (`!`) instead, as plugins' logs do, until the screen is put back.
//! Only on Unix; stderr redirected elsewhere (`2>file`) is captured all the same.

/// Send what's written to stderr from here on to `log`, a line at a time (once).
pub fn capture(log: impl Fn(&str) + Send + 'static) {
    #[cfg(unix)]
    unix::capture(log);
    #[cfg(not(unix))]
    let _ = log;
}

/// Point stderr back where it was, for what's printed after the player (or as it panics).
pub fn restore() {
    #[cfg(unix)]
    unix::restore();
}

#[cfg(unix)]
mod unix {
    use std::io::{BufRead, BufReader};
    use std::os::fd::AsRawFd;
    use std::sync::atomic::{AtomicI32, Ordering};

    /// stderr as it was, while it's captured (-1 otherwise).
    static SAVED: AtomicI32 = AtomicI32::new(-1);

    pub fn capture(log: impl Fn(&str) + Send + 'static) {
        if SAVED.load(Ordering::Relaxed) >= 0 {
            return;
        }
        let Ok((read, write)) = std::io::pipe() else { return };
        // SAFETY: plain file-descriptor calls on descriptors this process owns; the pipe's write
        // end stays open as stderr until `restore`.
        let saved = unsafe { libc::dup(2) };
        if saved < 0 || unsafe { libc::dup2(write.as_raw_fd(), 2) } < 0 {
            if saved >= 0 {
                unsafe { libc::close(saved) };
            }
            return;
        }
        drop(write);
        SAVED.store(saved, Ordering::Relaxed);
        // Not joined: it ends once nothing writes to the pipe, when stderr is put back and any
        // program started meanwhile (which got it as its stderr) has exited.
        let _ = std::thread::Builder::new().name("stderr".into()).spawn(move || {
            for line in BufReader::new(read).split(b'\n').map_while(Result::ok) {
                let line = String::from_utf8_lossy(&line);
                if !line.trim().is_empty() {
                    log(line.trim_end());
                }
            }
        });
    }

    pub fn restore() {
        let saved = SAVED.swap(-1, Ordering::Relaxed);
        if saved < 0 {
            return;
        }
        // SAFETY: stderr goes back to what it was, closing this end of the pipe.
        unsafe {
            libc::dup2(saved, 2);
            libc::close(saved);
        }
    }
}
