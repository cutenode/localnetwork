//! Packs that can't be made as asked: onto the source itself, with nothing chosen for a drive never
//! packed, from a source whose OneLibrary can't be read (the pack would lose it), without the
//! room, or over a pack a player wrote to with no store to keep what it wrote. A source track whose
//! audio is gone is packed with a warning, and artwork comes with the tracks.

use crate::common::{self, media};

use std::fs;
use std::path::{Path, PathBuf};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::edit::Edit;
use locallibrary::error::Reason;
use locallibrary::pack::{self, PackOptions};
use locallibrary::progress::Ctx;
use locallibrary::{Error, paths};

/// The twelve-track export with a playlist "Set" of its first `n` tracks, the first with artwork.
fn source(dir: &Path, n: usize, plus: bool) -> PathBuf {
    let root = common::bigger_drive(dir, "Main");
    let lib = common::library(&root);
    let files: Vec<String> = lib.tracks.iter().take(n).map(|t| t.file_path.clone()).collect();
    common::apply(&root, &[Edit::SetEntries { playlist: "Set".into(), paths: files, create: true }]);
    media::set_art(&root, &[lib.tracks[0].id]);
    if plus {
        common::add_onelibrary(&root);
    }
    root
}

#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn packs_that_cant_be_made_as_asked_are_refused_or_said(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let plus = tc.draw(gs::booleans());
    let root = source(dir.path(), n, plus);
    let lib = common::library(&root);
    let src = Device::open_export(&root).unwrap();
    let club = Device::open(common::blank(dir.path(), "Club")).unwrap();
    let set = PackOptions { playlists: vec!["Set".into()], ..Default::default() };
    let silent = Ctx::silent();

    let err = pack::plan(&src, &Device::open(&root).unwrap(), &set, &silent).unwrap_err();
    assert!(err.to_string().contains("the same drive"), "{err}");
    let err = pack::plan(&src, &club, &PackOptions::default(), &silent).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::MissingParam), "{err}");
    assert_eq!(err.detail().fields["param"], "playlists");

    // A track of the set whose audio is gone: packed, said.
    let gone = tc.draw(gs::booleans()) && n > 1;
    if gone {
        fs::remove_file(paths::to_fs(&root, &paths::key(&lib.tracks[n - 1].file_path))).unwrap();
    }
    let src = Device::open_export(&root).unwrap();
    let plan = pack::plan(&src, &club, &set, &silent).unwrap();
    let said = plan.warnings.iter().any(|w| w.contains("have no audio file on"));
    assert_eq!(said, gone, "{:?}", plan.warnings);
    let mut full = plan.clone();
    full.fits = false;
    let mut club = Device::open(club.root()).unwrap();
    assert!(matches!(pack::apply(&full, &src, &mut club, None, &silent), Err(Error::NoSpace { .. })));
    pack::apply(&plan, &src, &mut club, None, &silent).unwrap();
    let packed = Device::open_export(club.root()).unwrap().library().unwrap();
    let art = packed.tracks.iter().find(|t| t.file_path == lib.tracks[0].file_path).unwrap().artwork_path.clone();
    assert!(art.is_some_and(|a| paths::to_fs(club.root(), &paths::key(&a)).is_file()), "the artwork comes along");

    // A player writes to the pack (its database, behind locallibrary's back): packing again would
    // replace that, which needs the store to keep it first.
    let pdb = club.root().join(paths::EXPORT_PDB);
    let mut bytes = fs::read(&pdb).unwrap();
    bytes.extend([0; 4096]);
    fs::write(&pdb, bytes).unwrap();
    let again = pack::plan(&src, &Device::open(club.root()).unwrap(), &PackOptions::default(), &silent).unwrap();
    assert!(again.unsaved.is_some(), "{:?}", again.warnings);
    let err = pack::apply(&again, &src, &mut Device::open(club.root()).unwrap(), None, &silent).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::NotSaved), "{err}");
    let methods: Vec<_> = err.detail().actions.iter().map(|a| a.method.clone()).collect();
    assert_eq!(methods, [Some("pull".to_string())]);

    // A OneLibrary that can't be read: packing export.pdb alone would leave the players that read
    // only OneLibrary nothing.
    if plus {
        fs::write(root.join(paths::LIBRARY_PLUS_DB), b"not a database").unwrap();
        let other = Device::open(common::blank(dir.path(), "Other")).unwrap();
        let err = pack::plan(&Device::open_export(&root).unwrap(), &other, &set, &silent).unwrap_err();
        assert!(err.to_string().contains("exportLibrary.db can't be read"), "{err}");
        let methods: Vec<_> = err.detail().actions.iter().map(|a| a.method.clone()).collect();
        assert_eq!(methods, [Some("convert.plan".to_string())]);
    }
}
