//! Recorded mixes through the service layer: a drive's tracks fingerprinted, a recording matched
//! against them and its report kept, listed, read back, written as a page and as a data
//! document. The drive's audio here is placeholder bytes, so nothing decodes and a recording of
//! silence plays none of it: what's tested is the API around the identifier.

use crate::common::media::wav_of;
use crate::common::{self, path};

use locallibrary::api::{self, IdentifyMix, LibraryCache};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;

/// A recording is matched against the drive, kept in the store and found there again: listed,
/// read back, and written as a page; as a data document it's written once unless replacing.
#[test]
fn a_recordings_report_is_kept_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    locallibrary::testing::volumes_on_this_thread(Some(vec![dir.path().join("volumes")]));
    let main = common::demo_drive(&dir.path().join("volumes"), "Main");
    let mix = dir.path().join("Friday night.wav");
    std::fs::write(&mix, wav_of(44_100, 16, 2, 2.0)).unwrap();
    let ctx = Ctx::silent();

    let index = api::mix_index(&store, &api::IndexMixLibrary { drive: None }, &ctx).unwrap();
    assert_eq!((index.tracks, index.indexed), (2, 0));
    assert_eq!(index.failed.len(), 2, "placeholder audio doesn't decode");

    let cache = LibraryCache::new(1);
    let identify = IdentifyMix { mix: mix.clone(), drive: Some(path(&main)), tempo_range: None };
    let req = api::IdentifyMixRequest { identify: identify.clone(), extras: true, save: true };
    let found = api::mix_identify_cached(&cache, &store, &req, &ctx).unwrap();
    assert!(found.report.plays.is_empty() && found.extras.is_empty());
    assert_eq!(found.audio.as_deref(), Some(mix.as_path()), "the recording plays in a browser itself");
    let id = found.saved.expect("kept");

    let kept = api::mix_reports_list(store.root()).unwrap();
    assert_eq!(kept.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), [id.as_str()]);
    assert_eq!(kept[0].title, "Friday night");
    let got = api::mix_report_get(store.root(), &api::ReportRef { id: id.clone(), extras: true }).unwrap();
    assert_eq!((got.saved.as_deref(), got.report.mix.path.as_path()), (Some(id.as_str()), mix.as_path()));
    let page = api::mix_report_html(&store, &api::ReportHtml { id: id.clone(), inline: true }, None).unwrap();
    assert!(page.path.is_file() && page.path.ends_with(format!("{id}.html")));
    assert_eq!(page.html.map(|h| h.len() as u64), Some(page.bytes));

    let out = dir.path().join("friday.json");
    let export = |replace| api::ExportMix { identify: identify.clone(), output: Some(out.clone()), replace };
    let doc = api::mix_export_cached(&cache, &store, &export(false), &ctx).unwrap();
    let written: serde_json::Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(written["format"], doc.format);
    assert!(api::mix_export_cached(&cache, &store, &export(false), &ctx).is_err(), "not replaced unasked");
    assert!(api::mix_export_cached(&cache, &store, &export(true), &ctx).is_ok());
}
