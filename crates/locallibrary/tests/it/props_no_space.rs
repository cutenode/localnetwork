//! A plan the drive has no room for is refused when applied, whatever the command: editing
//! playlists, cleaning, merging, converting, setting a cue. Nothing on the drive changes, the
//! error says the space the plan needs, and the same plan with room applies.

use crate::{common, drawn};

use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::convert::{self, ConvertOptions, Target};
use locallibrary::cue_edit::{self, CueEdit, CuePosition, HotCue, SetCue};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions};
use locallibrary::merge::{self, MergeOptions};
use locallibrary::progress::Ctx;
use locallibrary::{Error, Result, clean};
use rekordbox_pdb::NewTrack;

/// Apply a plan of the drawn command on `root` (with `fits` as given): what applying said.
fn attempt(command: u8, root: &Path, guest: &Path, fits: bool) -> (Result<()>, u64) {
    let mut d = Device::open_export(root).unwrap();
    let silent = Ctx::silent();
    match command {
        0 => {
            let edits = [Edit::CreatePlaylist { path: "Sets/New".into(), folder: false }];
            let mut p = edit::plan(&d, &edits, &EditOptions::default(), &silent).unwrap();
            p.fits = fits;
            (edit::apply(&p, &mut d, &silent).map(drop), p.needed_bytes)
        }
        1 => {
            let mut p = clean::plan(&d, &silent).unwrap();
            p.fits = fits;
            (clean::apply(&p, &mut d, &silent).map(drop), p.needed_bytes)
        }
        2 => {
            let src = Device::open_export(guest).unwrap();
            let mut p = merge::plan(&src, &d, &MergeOptions::default(), &silent).unwrap();
            p.fits = fits;
            (merge::apply(&p, &src, &mut d, &MergeOptions::default(), &silent).map(drop), p.needed_bytes)
        }
        3 => {
            let opts = ConvertOptions { to: Target::OneLibrary, replace: false };
            let mut p = convert::plan(&d, &opts, &silent).unwrap();
            p.fits = fits;
            (convert::apply(&p, &mut d, &opts, &silent).map(drop), p.needed_bytes)
        }
        _ => {
            let lib = d.library().unwrap();
            let ids: Vec<u32> =
                lib.tracks.iter().filter(|t| t.analyze_path.starts_with("/PIONEER/")).map(|t| t.id).take(1).collect();
            let set = SetCue {
                hot_cue: Some(HotCue(0)),
                at: CuePosition::Ms(1000),
                snap: false,
                loop_beats: None,
                name: None,
                color: None,
            };
            let mut p = cue_edit::plan(&d, &ids, &CueEdit::Set(set), &silent).unwrap();
            p.fits = fits;
            (cue_edit::apply(&p, &mut d, &silent).map(drop), p.needed_bytes)
        }
    }
}

#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn a_plan_the_drive_has_no_room_for_changes_nothing(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = locallibrary::device::dunce_canonicalize(dir.path());
    // The demo export (real analysis, for cues) or the twelve-track one drawn on, with a demo
    // sample to clean.
    let main = match tc.draw(gs::booleans()) {
        true => common::demo_drive(&root, "Main"),
        false => drawn::drive(&tc, &root, "Main").0,
    };
    let sample = "/Contents/rekordbox/GROOVE CIRCUIT FACTORY SAMPLES/House1.wav";
    let track = NewTrack { artist: Some("rekordbox".into()), ..NewTrack::new("House 1", sample) };
    common::add_track(&main, track, &common::placeholder_audio(sample));
    // A track with its analysis where rekordbox keeps it, for the cue.
    drawn::track(&tc, &main, 7);
    let guest = common::bigger_drive(&root, "Guest");
    for command in 0..5 {
        let before = common::Before::take(&main);
        let (refused, needed) = attempt(command, &main, &guest, false);
        match refused {
            Err(Error::NoSpace { needed: n, path, .. }) => {
                assert_eq!((n, path.as_path()), (needed, main.as_path()), "command {command}");
            }
            other => panic!("command {command} without room: {other:?}"),
        }
        before.untouched(&main).unwrap();
        let (applied, _) = attempt(command, &main, &guest, true);
        applied.unwrap_or_else(|e| panic!("command {command} with room: {e}"));
    }
}
