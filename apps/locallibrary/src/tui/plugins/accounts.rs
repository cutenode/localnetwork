//! Plugins' accounts in the player: signing in to each in the browser, and out, from the palette
//! beside the plugin's settings. Both run in the background: nothing here reads the keychain or
//! reaches a service while the player draws.

use std::time::Duration;

use locallibrary::Store;
use loclib_plugin::Manifest;

use crate::tui::app::{App, Level};
use crate::tui::keymap::Act;
use crate::tui::palette::Item;

/// How long the browser is waited for, as `plugin account ... sign-in` waits by default.
const BROWSER_WAIT: Duration = Duration::from_secs(300);

/// `m`'s accounts as the palette offers them: signing in to each, and out.
pub(super) fn items(m: &'static Manifest) -> Vec<(Act, Item)> {
    let accounts = m.contributes.accounts.iter().flat_map(|a| {
        [
            (Act::PluginSignIn(&m.id, &a.id), Item::new(format!("sign in to {}…", a.title), &m.name)),
            (Act::PluginSignOut(&m.id, &a.id), Item::new(format!("sign out of {}", a.title), &m.name)),
        ]
    });
    accounts.collect()
}

impl App {
    /// Sign in to a plugin's account in the background: the browser opens on the service's
    /// sign-in page (the link is shown when it can't), and once it comes back the account's
    /// tokens are kept. Esc stops waiting.
    pub(in crate::tui) fn plugin_sign_in(&mut self, plugin: &'static str, account: &'static str) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let store_root = self.store_root.clone();
        self.task_showing("Signing in".into(), move |ctx, show| {
            let store = Store::open_or_init(&store_root)?;
            let start = plugins.begin_sign_in(plugin, account, &store)?;
            if locallibrary::opener::open_url(&start.url).is_err() {
                let link = format!("open this in your browser to sign in: {}", start.url);
                show(Box::new(move |app: &mut App| app.toast(link, Level::Info)));
            }
            let said = format!("{} signed in", plugins.finish_sign_in(&start.id, BROWSER_WAIT, ctx)?.title);
            Ok(Box::new(move |app: &mut App| app.toast(said, Level::Info)))
        });
    }

    /// Sign out of a plugin's account in the background: its tokens are forgotten here, and the
    /// service is told to forget them too when it has a way to be.
    pub(in crate::tui) fn plugin_sign_out(&mut self, plugin: &'static str, account: &'static str) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let store_root = self.store_root.clone();
        self.task("Signing out".into(), move |_| {
            let store = Store::open_or_init(&store_root)?;
            let out = plugins.sign_out(plugin, account, &store)?;
            let title = &out.status.title;
            let said = match (out.was_signed_in, out.revoked) {
                (false, _) => format!("not signed in to {title}"),
                (true, true) => format!("signed out of {title}"),
                (true, false) => format!("signed out of {title} on this computer"),
            };
            Ok(Box::new(move |app: &mut App| app.toast(said, Level::Info)))
        });
    }
}
