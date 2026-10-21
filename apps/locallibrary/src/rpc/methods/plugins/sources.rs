//! `plugins.source.*`: a drive built from a plugin's library source, planned (what it reads from
//! the folders granted, and what's copied and removed) and written.

use locallibrary::collection_export::{CollectionExportOptions, CollectionExportPlan};
use locallibrary::op::Planned;
use locallibrary::plugin::SourceExport;

use crate::plugins::host;
use crate::rpc::RpcError;
use crate::rpc::methods::{Call, PlanStore, Registry};

pub(super) fn register(r: &mut Registry) {
    r.plan("plugins.source.plan", plan).long().destructive();
    r.apply("plugins.source.apply", PlanStore::Optional, |c, req: SourceExport| {
        Ok(plan(c, req)?.apply(c.store_opt().as_ref(), c.ctx)?)
    })
    .long()
    .destructive();
}

fn plan(c: &Call<'_>, req: SourceExport) -> Result<Planned<CollectionExportPlan, CollectionExportOptions>, RpcError> {
    Ok(host()?.plan_source_export(&req, c.existing()?.as_ref(), c.ctx)?)
}
