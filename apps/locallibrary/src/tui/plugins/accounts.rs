//! Accounts in the player: signing in to an account of each type the account plugins provide,
//! and out of each the store keeps, from the palette beside the plugins' settings. Each runs in the background: nothing here reads
//! the keychain or reaches a service while the player draws.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::time::Duration;

use locallibrary::Store;
use locallibrary::plugin::{Account, Plugins};
use loclib_plugin::Manifest;

use crate::tui::app::{App, Level};
use crate::tui::keymap::Act;
use crate::tui::palette::Item;

/// How long the browser is waited for, as `account TYPE sign-in` waits by default.
pub(super) const BROWSER_WAIT: Duration = Duration::from_secs(300);

/// Signing in to an account of each type the account plugin `m` provides.
pub(super) fn sign_in_items(m: &'static Manifest) -> Vec<(Act, Item)> {
    let types = m.contributes.account_types.iter();
    types
        .map(|t| (Act::AccountSignIn(&t.name, &m.id), Item::new(format!("sign in to a {} account…", t.title), &m.name)))
        .collect()
}

/// Signing out of each account the store at `store_root` keeps.
pub(super) fn sign_out_items(plugins: &Plugins, store_root: &Path) -> Vec<(Act, Item)> {
    let Ok(Some(store)) = Store::open_existing(store_root) else { return Vec::new() };
    let kept = plugins.kept_accounts(&store).unwrap_or_default();
    let item = |a: &Account| Item::new(format!("sign out of {}", a.name), title(plugins, a));
    kept.iter().map(|a| (Act::AccountSignOut(mark(a)), item(a))).collect()
}

/// What `a` is, as the palette says it: its type's title and `account`.
fn title(plugins: &Plugins, a: &Account) -> String {
    let types = plugins.manifest(&a.plugin).map(|m| m.contributes.account_types.as_slice()).unwrap_or_default();
    let title = types.iter().find(|t| t.name == a.kind).map_or(a.kind.as_str(), |t| &t.title);
    format!("{title} account")
}

/// What the palette names `a` by: its account plugin and subject, hashed, so a palette open as
/// accounts come and go still signs out of the one picked.
fn mark(a: &Account) -> u64 {
    let mut h = DefaultHasher::new();
    (&a.plugin, &a.subject).hash(&mut h);
    h.finish()
}

/// Opens `url` in the browser to sign in, or shows it when it can't be.
pub(super) fn open(url: &str, show: &dyn Fn(crate::tui::edits::Done)) {
    if locallibrary::opener::open_url(url).is_err() {
        let link = format!("open this in your browser to sign in: {url}");
        show(Box::new(move |app: &mut App| app.toast(link, Level::Info)));
    }
}

impl App {
    /// Sign in to an account of type `kind` through the account plugin `with` in the background:
    /// the browser opens on the service's sign-in page (the link is shown when it can't), and
    /// once it comes back the store keeps the account. Esc stops waiting.
    pub(in crate::tui) fn account_sign_in(&mut self, kind: &'static str, with: &'static str) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let store_root = self.store_root.clone();
        self.task_showing("Signing in".into(), move |ctx, show| {
            let store = Store::open_or_init(&store_root)?;
            let start = plugins.begin_account_sign_in(kind, with, None, None, &[], &store)?;
            open(&start.url, show);
            let said =
                format!("{} signed in", plugins.finish_account_sign_in(&start.id, BROWSER_WAIT, ctx)?.account.name);
            Ok(Box::new(move |app: &mut App| app.toast(said, Level::Info)))
        });
    }

    /// Sign out of the account the palette marked `mark` in the background: it's forgotten here,
    /// and its service is told to forget its tokens too when it has a way to be. Plugins keep
    /// their access to it.
    pub(in crate::tui) fn account_sign_out(&mut self, marked: u64) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let store_root = self.store_root.clone();
        self.task("Signing out".into(), move |_| {
            let store = Store::open_or_init(&store_root)?;
            let kept = plugins.kept_accounts(&store)?;
            let a = kept.iter().find(|a| mark(a) == marked);
            let a = a.ok_or_else(|| locallibrary::Error::NotFound("that account: it was signed out".into()))?;
            let out = plugins.sign_out_account(&a.plugin, &a.subject, &store)?;
            let said = match out.revoked {
                true => format!("signed out of {}", a.name),
                false => format!("signed out of {} on this computer", a.name),
            };
            Ok(Box::new(move |app: &mut App| app.toast(said, Level::Info)))
        });
    }
}
