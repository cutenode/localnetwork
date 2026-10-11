//! The commands for a drive's library (tracks, playlists, cues, My Tags, artwork), the settings
//! profile, `setup` and `refresh`, and what's made from what was played (plays, sets, mixes, the
//! profile), run as a DJ runs them in a terminal: what they print, and what they leave behind. One
//! module per command or group; their `--json` output is tested in the modules beside this folder.

mod analysis;
mod art;
mod cues;
mod edit;
mod jobs;
mod listings;
mod mix;
mod plays;
mod profile;
mod sets;
mod settings;
mod tags;
