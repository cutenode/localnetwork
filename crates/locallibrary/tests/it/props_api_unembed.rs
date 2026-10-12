//! Taking embedded art back out: each file is put back as it was before the art went in, unless
//! it changed since or is gone, which are left as they are and said so.

use std::fs;

use hegel::Generator;
use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::Store;
use locallibrary::api;
use locallibrary::op::Operation;
use locallibrary::progress::Ctx;

use crate::common::media;

/// What happened to a file after the art went in.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Since {
    Untouched,
    Rewritten,
    Removed,
}

/// Art embedded in some WAV files, each of them then left alone, rewritten or removed, in any
/// mix: the plan to take it back out puts back exactly the untouched ones, skips the rest with
/// the reason for each ("changed since the art went in", "gone"), has nothing to do when none is
/// untouched, and applied, gives the untouched files their bytes from before the art and leaves
/// the others as they were left.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(since = vec![Since::Untouched, Since::Rewritten, Since::Removed])]
fn embedded_art_comes_out_of_the_files_left_alone(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let image = dir.path().join("cover.png");
    fs::write(&image, media::png(16, 16, 3)).unwrap();
    let since: Vec<Since> = tc.draw(
        gs::vecs(gs::sampled_from(vec![Since::Untouched, Since::Rewritten, Since::Removed]).print_as_debug())
            .min_size(1)
            .max_size(4),
    );
    let files: Vec<_> = (0..since.len()).map(|i| dir.path().join(format!("{i}.wav"))).collect();
    for f in &files {
        fs::write(f, media::wav()).unwrap();
    }
    let report = api::artwork_embed(&store, &api::EmbedArtwork { files: files.clone(), image }).unwrap();
    assert_eq!(report.embedded, files);
    let with_art: Vec<Vec<u8>> = files.iter().map(|f| fs::read(f).unwrap()).collect();
    for (f, s) in files.iter().zip(&since) {
        match s {
            Since::Untouched => {}
            Since::Rewritten => fs::write(f, b"rewritten since").unwrap(),
            Since::Removed => fs::remove_file(f).unwrap(),
        }
    }

    let plan = api::artwork_unembed_plan(&store, &api::UnembedRequest { backup: report.backup.clone() }).unwrap();
    let untouched: Vec<_> = files.iter().zip(&since).filter(|(_, s)| **s == Since::Untouched).map(|(f, _)| f).collect();
    assert_eq!(plan.plan.restored.iter().collect::<Vec<_>>(), untouched);
    assert_eq!(plan.noop, untouched.is_empty());
    let skipped: Vec<(String, String)> = plan.plan.skipped.iter().map(|s| (s.path.clone(), s.reason.clone())).collect();
    let expected: Vec<(String, String)> = files
        .iter()
        .zip(&since)
        .filter_map(|(f, s)| {
            let reason = match s {
                Since::Untouched => return None,
                Since::Rewritten => "changed since the art went in",
                Since::Removed => "gone",
            };
            Some((f.display().to_string(), reason.to_string()))
        })
        .collect();
    assert_eq!(skipped, expected);
    assert_eq!(plan.warnings.len(), expected.len());

    if untouched.is_empty() {
        return;
    }
    let r = locallibrary::api::UnembedPlan::apply(&plan, Some(&store), &Ctx::silent()).unwrap();
    assert_eq!(r.restored.len(), untouched.len());
    for ((f, s), art) in files.iter().zip(&since).zip(&with_art) {
        match s {
            Since::Untouched => {
                assert_eq!(fs::read(f).unwrap(), media::wav(), "{} came back", f.display());
                assert_ne!(&media::wav(), art, "the art went in");
            }
            Since::Rewritten => assert_eq!(fs::read(f).unwrap(), b"rewritten since"),
            Since::Removed => assert!(!f.exists()),
        }
    }
}
