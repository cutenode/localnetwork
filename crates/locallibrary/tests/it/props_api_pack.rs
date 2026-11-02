//! Packing tracks for the players remembered, through the service layer: the same plan as for
//! the players named, said in its summary, and applied with the store.

use crate::common;
use crate::props_transcode::source;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api::{self, PackRequest, SetPlayers};
use locallibrary::device::Device;
use locallibrary::pack::{Container, PackOptions, Transcode};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;

fn options(playlist: &str, players: Vec<String>) -> PackOptions {
    PackOptions {
        playlists: vec![playlist.into()],
        transcode: Some(Transcode { players, container: Container::Aiff, decode_lossy: false }),
        ..Default::default()
    }
}

/// The masters of every format and rate, for players remembered in the store (replacing models
/// remembered before, which the plan says): packing for no players named plans what packing for
/// those players does, and the summary says how many copies it makes and how many tracks stay
/// unplayable. The high-rate WAVs, packed with the store, come out as copies, and the target is
/// registered.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn packing_for_the_players_remembered_plans_what_naming_them_does(tc: TestCase) {
    let models: Vec<String> = tc
        .draw(gs::vecs(gs::sampled_from(vec!["CDJ-2000", "CDJ-900", "XDJ-700"])).min_size(1).max_size(2).unique(true))
        .into_iter()
        .map(String::from)
        .collect();

    let dir = tempfile::tempdir().unwrap();
    let ctx = Ctx::silent();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let volumes = common::isolate(dir.path(), Some(store.root()));
    let main = source().write(&volumes.join("Main"));
    let club = common::blank(&volumes, "Club");
    api::players_set(&store, &SetPlayers { models: vec!["CDJ-3000".into()], library: None }).unwrap();
    let plan = api::players_set_plan(Some(&store), &SetPlayers { models: models.clone(), library: None }).unwrap();
    let instead = plan.summary[0].ends_with("(instead of CDJ-3000)");
    assert!(instead, "{:?}", plan.summary);
    plan.apply(Some(&store), &ctx).unwrap();

    let req = |players: Vec<String>, playlist: &str| PackRequest {
        source: common::path(&main),
        target: common::path(&club),
        options: options(playlist, players),
    };
    let remembered = api::pack_plan(&req(Vec::new(), "Set"), &ctx).unwrap();
    let named = api::pack_plan(&req(models.clone(), "Set"), &ctx).unwrap();
    let froms = |p: &locallibrary::pack::PackPlan| {
        p.transcodes.iter().map(|t| (t.from.clone(), t.recipe.clone())).collect::<Vec<_>>()
    };
    assert_eq!(froms(&remembered.plan), froms(&named.plan), "the players remembered are the ones named");
    let n = remembered.plan.transcodes.len();
    let makes = remembered.summary.iter().find(|l| l.starts_with(&format!("pack {n} track(s) the players can't play")));
    assert_eq!(makes.is_some(), n > 0, "{:?}", remembered.summary);
    let left = remembered.plan.unplayable.len();
    let stays = format!("{left} track(s) stay unplayable on these players");
    assert_eq!(remembered.summary.contains(&stays), left > 0, "{:?}", remembered.summary);
    assert!(left > 0, "no player plays the OGG");

    let wavs = api::pack_plan(&req(Vec::new(), "Wavs"), &ctx).unwrap();
    let copies = wavs.plan.transcodes.len();
    let report = wavs.apply(Some(&store), &ctx).unwrap();
    assert_eq!(report.transcoded, copies);
    let id = Device::open(&club).unwrap().id().expect("given an identity");
    assert!(store.drives().unwrap().contains_key(&id), "registered");
}
