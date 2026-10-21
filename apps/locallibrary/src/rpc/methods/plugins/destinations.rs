//! `plugins.store.*`: the store copied to a plugin's store destination, brought back from it, and
//! drives verified and repaired with its good copies; `plugins.publish.*`: a set published at a
//! plugin's set destination, planned with the plugin's dry run.

use locallibrary::api::{self, CopyOptions, RepairRequest, VerifyRequest};
use locallibrary::offsite::{self, RestoreOptions};
use locallibrary::op::Planned;
use locallibrary::plugin::SetPublish;
use locallibrary::scrub;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::plugins::host;
use crate::rpc::RpcError;
use crate::rpc::methods::{Call, PlanStore, Registry};

#[derive(Deserialize, JsonSchema)]
struct CopyTo {
    /// `<plugin>:<destination>`: one of a plugin's store destinations.
    destination: String,
    /// Copy over a copy that has snapshots the store doesn't, or that's another store's.
    #[serde(default)]
    force: bool,
    /// Say what would be copied, and copy nothing.
    #[serde(default)]
    dry_run: bool,
}

#[derive(Deserialize, JsonSchema)]
struct RestoreFrom {
    destination: String,
    #[serde(flatten)]
    options: RestoreOptions,
}

#[derive(Deserialize, JsonSchema)]
struct VerifyFrom {
    /// Whose good copies the files the store and the drives lack are offered from.
    destination: String,
    #[serde(flatten)]
    verify: VerifyRequest,
}

#[derive(Deserialize, JsonSchema)]
struct RepairFrom {
    /// Whose good copies are fetched into the store first.
    destination: String,
    #[serde(flatten)]
    repair: RepairRequest,
}

#[derive(Deserialize, JsonSchema)]
struct PublishSetAt {
    /// `<plugin>:<destination>`: one of a plugin's set destinations.
    destination: String,
    /// The set: its id (or the start of it), title or day, or `last`.
    set: String,
}

pub(super) fn register(r: &mut Registry) {
    r.immediate("plugins.store.copy", |c, req: CopyTo| {
        let store = c.store()?;
        let dest = host()?.store_destination(&req.destination, Some(&store))?;
        let options = CopyOptions { force: req.force, dry_run: req.dry_run, ..CopyOptions::default() };
        Ok(offsite::copy_to(&store, &dest, &options, c.ctx)?)
    })
    .long()
    .destructive();
    r.immediate("plugins.store.restore", |c, req: RestoreFrom| {
        let store = c.store()?;
        let dest = host()?.store_destination(&req.destination, Some(&store))?;
        Ok(offsite::restore(&store, &dest, &req.options, c.ctx)?)
    })
    .long();
    r.read("plugins.store.verify", |c, req: VerifyFrom| {
        let store = c.store()?;
        let dest = host()?.store_destination(&req.destination, Some(&store))?;
        let mut report = api::drive_verify(Some(&store), &req.verify, c.ctx)?;
        scrub::offer(&mut report, &dest, c.ctx)?;
        Ok(report)
    })
    .long();
    r.immediate("plugins.store.repair", |c, req: RepairFrom| {
        let store = c.store()?;
        let dest = host()?.store_destination(&req.destination, Some(&store))?;
        scrub::fetch(&store, &dest, &req.repair.problems, c.ctx)?;
        Ok(api::drive_repair(Some(&store), &req.repair, c.ctx)?)
    })
    .long()
    .destructive();
    // Publishing reaches outside this computer: a front end always confirms it.
    r.plan("plugins.publish.plan", publish_plan).long();
    r.apply("plugins.publish.apply", PlanStore::Required, |c, req: PublishSetAt| {
        Ok(publish_plan(c, req)?.apply(Some(&c.store()?), c.ctx)?)
    })
    .long();
}

fn publish_plan(c: &Call<'_>, req: PublishSetAt) -> Result<Planned<SetPublish>, RpcError> {
    Ok(host()?.publish_plan(&req.destination, &req.set, &c.store()?, c.ctx)?)
}
