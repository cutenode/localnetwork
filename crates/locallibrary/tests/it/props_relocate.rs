//! Properties of relocating missing files (`relocate`): a track gets a file only when exactly one
//! of those with its name survives narrowing by size and then length, and then that one, unless
//! it isn't as long as the track.

use crate::common::{self, media};

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::api::{self, RelocateRequest};
use locallibrary::paths;
use locallibrary::progress::Ctx;
use locallibrary::relocate::RelocateOptions;
use rekordbox_pdb::NewTrack;

/// Of `files` (size, seconds), the indexes narrowing keeps for a track of `size` and `secs`: those
/// of its size, then of those, those less than a second from its length, each step skipped when
/// it would keep none.
fn survivors(files: &[(u64, u16)], size: u64, secs: u16) -> Vec<usize> {
    let narrow = |from: Vec<usize>, fits: &dyn Fn(usize) -> bool| {
        let kept: Vec<usize> = from.iter().copied().filter(|&i| fits(i)).collect();
        if kept.is_empty() { from } else { kept }
    };
    let by_size = narrow((0..files.len()).collect(), &|i| files[i].0 == size);
    narrow(by_size, &|i| files[i].1 == secs)
}

#[hegel::test(test_cases = 40)]
fn a_track_gets_a_file_only_when_one_fits(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    // Silent WAVs: 8 kHz for 2 s is the size of 16 kHz for 1 s.
    let n = tc.draw(gs::integers::<usize>().max_value(4));
    let files: Vec<(u32, u16)> = (0..n)
        .map(|_| {
            let rate = tc.draw(gs::sampled_from(vec![8_000u32, 16_000]));
            (rate, tc.draw(gs::integers::<u16>().min_value(1).max_value(3)))
        })
        .collect();
    let size = tc.draw(gs::sampled_from(vec![16_044u64, 32_044, 48_044, 64_044, 96_044, 12_345]));
    let secs = tc.draw(gs::integers::<u16>().min_value(1).max_value(3));

    let track = NewTrack { duration: secs, ..NewTrack::new("Tone", "/Contents/Tones/Tone.wav") };
    let id = common::add_track(&root, track, &vec![0; size as usize]);
    std::fs::remove_file(paths::to_fs(&root, "Contents/Tones/Tone.wav")).unwrap();
    let music = dir.path().join("Music");
    let mut sized = Vec::new();
    for (i, (rate, secs)) in files.iter().enumerate() {
        let wav = media::wav_of(*rate, 16, 1, f64::from(*secs));
        sized.push((wav.len() as u64, *secs));
        let path = music.join(i.to_string()).join("Tone.wav");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, wav).unwrap();
    }
    std::fs::create_dir_all(&music).unwrap();

    let req = RelocateRequest { drive: common::path(&root), options: RelocateOptions { folders: vec![music.clone()] } };
    let plan = api::relocate_plan(&req, &Ctx::silent()).unwrap();
    let copied: Vec<&str> = plan.copied.iter().filter(|r| r.track.id == id).map(|r| r.file.path.as_str()).collect();
    let listed = plan.ambiguous.iter().find(|a| a.track.id == id).map(|a| a.candidates.len());
    let unfit = plan.unfit.iter().find(|a| a.track.id == id).map(|a| a.candidates.len());
    match (n, survivors(&sized, size, secs).as_slice()) {
        (0, _) => assert!(copied.is_empty() && listed.is_none() && plan.not_found.iter().any(|t| t.id == id)),
        (_, [one]) if sized[*one].1 != secs => {
            assert!(copied.is_empty(), "picked {one}, of another length: {copied:?}");
            assert_eq!(unfit, Some(n), "every file found is listed");
        }
        (_, [one]) => {
            let want = music.join(one.to_string()).join("Tone.wav");
            assert_eq!(copied, [want.display().to_string()], "{sized:?} for {size} B, {secs} s");
        }
        (_, several) => {
            assert!(copied.is_empty(), "picked one of {several:?}: {copied:?}");
            assert_eq!(listed, Some(n), "every file found is listed");
        }
    }
}
