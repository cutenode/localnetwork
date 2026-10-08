//! The drive commands (`drive ...`, `status`, `eject`, `provision`), copying between drives
//! (`compare`, `sync`, `merge`), the store's upkeep, `watch`, undo and restore, `library`,
//! `players`, and the reports that read drives, run as a DJ runs them in a terminal: what they
//! print, how they exit, and what they leave on the drives and in the store.

mod health;
mod identity;
mod library;
mod on_drive;
mod pager;
mod players;
mod provision;
mod reports;
mod restore;
mod status;
mod store;
mod sync;
mod watch;
