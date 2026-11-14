//! A plugin wanting more of an account it has access to (`accounts/ensure`), asked about in the
//! terminal a command runs in, with its progress bar out of the way: allowing it, or signing in
//! again to, in the browser or with keys read as `account TYPE sign-in` reads them, when the
//! account wasn't granted what it asks. And a plugin writing a record in the Atmosphere account
//! (`atmosphere/put_record`): the record is shown, then the user asked (`--yes` answers).

use locallibrary::plugin::{AccessQuestion, AccountSignIn, AskAccess, EnteredKeys, RecordWrite};

use crate::browser;
use crate::output::{Drawing, Output, dim};

/// Asks on stderr, reading the answer from the terminal.
pub struct Terminal {
    pub out: Output,
    pub drawing: Drawing,
}

/// What `q`'s plugin wants, and why.
pub fn wants(q: &AccessQuestion) -> String {
    let a = &q.access;
    let (titles, why) = (q.titles.join(", "), q.why.join("; "));
    format!("{} wants to {titles} with the {} account {} ({why})", q.plugin, q.title, a.name)
}

impl AskAccess for Terminal {
    fn allow(&self, q: &AccessQuestion) -> Option<bool> {
        let allow = if q.sign_in { "Sign in again to allow it?" } else { "Allow it?" };
        Some(self.drawing.suspend(|| self.out.confirm(&format!("{}. {allow}", wants(q)))))
    }

    fn sign_in(&self, url: &str) {
        self.drawing.suspend(|| {
            browser::open(url, true);
            eprintln!("{}", dim("Waiting for you to approve (Ctrl-C stops)…"));
        });
    }

    fn enter_keys(&self, start: &AccountSignIn) -> Option<EnteredKeys> {
        self.drawing.suspend(|| super::accounts::read_keys(start).ok())
    }

    fn write_record(&self, write: &RecordWrite) -> Option<bool> {
        Some(self.drawing.suspend(|| {
            eprintln!("{}", write.shown());
            self.out.confirm(&format!("{}. Write it?", write.summary()))
        }))
    }
}
