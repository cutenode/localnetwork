//! The Profile view without the Atmosphere account: this locallibrary was built without the
//! `account` feature. The bar at the top says so, and so does `L`.

use std::path::Path;

use ratatui::crossterm::event::KeyEvent;
use ratatui::text::Line;

use super::app::{App, Level};
use super::theme::dim;

const NONE: &str = "this locallibrary was built without the Atmosphere account";

#[derive(Default)]
pub struct Account {}

impl Account {
    pub fn load(&mut self, _store_root: &Path) {}

    pub fn busy(&self) -> bool {
        false
    }

    /// No handle is ever typed.
    pub fn typing(&self) -> bool {
        false
    }

    pub fn stop_typing(&mut self) {}

    /// The bar at the top of the view.
    pub fn line(&self, _width: u16) -> Line<'static> {
        Line::styled(" ○ No Atmosphere account: this locallibrary was built without it", dim())
    }
}

/// No key goes to signing in.
pub fn on_input(_app: &mut App, _key: KeyEvent) -> bool {
    false
}

/// `L`: there's no account to sign in to.
pub fn press(app: &mut App) {
    app.toast(NONE, Level::Info);
}
