//! Plugins' destinations (the `destinations` feature). A store destination keeps a copy of the
//! store ([`crate::offsite::copy_to`], [`crate::offsite::restore`], [`crate::scrub::offer`]): its
//! stored files are given by path, to read, and fetched back into the request's scratch folder;
//! its records are given and fetched there as files. What comes back is checked by whoever asked
//! for it (see [`crate::offsite::Destination`]), and only the host writes the store. A set
//! destination publishes a set (see [`publish`]).

mod publish;

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use loclib_plugin::{
    Blob, Context, DestinationKind, GetBlob, GetManifest, Got, GotManifest, HasBlobs, Missing, PutBlob, PutManifest,
    method,
};
use serde_json::Value;

use super::Plugins;
use super::process::Slot;
use crate::error::IoContext;
use crate::hash::Hash;
use crate::offsite::Destination;
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result};

pub use publish::{SetPublish, SetPublished};

/// What a record goes in as, in the request's scratch folder.
const RECORD: &str = "record";

/// A plugin's store destination, as [`crate::offsite`] and [`crate::scrub`] use it: its requests
/// carry the plugin's settings in `store`, and start it there.
pub struct StoreDestination {
    slot: Arc<Slot>,
    name: String,
    store: Option<Store>,
}

/// `<plugin>:<destination>`, as the user names a plugin's destination, split.
pub fn destination_spec(spec: &str) -> Result<(&str, &str)> {
    spec.split_once(':')
        .filter(|(plugin, name)| !plugin.is_empty() && !name.is_empty())
        .ok_or_else(|| Error::invalid(format!("{spec:?} names no destination: <plugin>:<destination>")))
}

impl Plugins {
    /// The destination `spec` names (`<plugin>:<destination>`), which must be of `kind`.
    fn destination(&self, spec: &str, kind: DestinationKind) -> Result<(&Arc<Slot>, String)> {
        let (plugin, name) = destination_spec(spec)?;
        let slot = self.slot(plugin)?;
        let declared = slot.declared("destination", name, &slot.manifest.contributes.destinations, |d| &d.name)?;
        if declared.kind != kind {
            let (title, kind) = (&declared.title, kind.name());
            return Err(Error::invalid(format!("{spec} ({title}) isn't a destination for a {kind}")));
        }
        Ok((slot, name.to_string()))
    }

    /// The store destination `spec` names (`<plugin>:<destination>`), for copying the store to,
    /// and restoring and repairing from.
    pub fn store_destination(&self, spec: &str, store: Option<&Store>) -> Result<StoreDestination> {
        let (slot, name) = self.destination(spec, DestinationKind::Store)?;
        Ok(StoreDestination { slot: slot.clone(), name, store: store.cloned() })
    }
}

impl StoreDestination {
    fn context(&self) -> Context {
        Context { settings: self.slot.settings(self.store.as_ref()), ..Context::default() }
    }

    /// The file `blob` names in the scratch folder, read whole.
    fn read(&self, scratch: &super::blobs::Scratch, blob: &Blob) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut file = scratch.blob(&blob.blob).map_err(|e| self.slot.misbehaved(e))?;
        file.read_to_end(&mut bytes).map_err(|e| self.slot.misbehaved(format!("{}: {e}", blob.blob)))?;
        Ok(bytes)
    }
}

impl Destination for StoreDestination {
    fn plugin(&self) -> &str {
        &self.slot.manifest.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    /// Refuses an answer naming what it wasn't asked about.
    fn missing(&self, hashes: &[Hash], ctx: &Ctx<'_>) -> Result<Vec<Hash>> {
        let asked: Vec<String> = hashes.iter().map(Hash::to_hex).collect();
        let req = HasBlobs { destination: self.name.clone(), hashes: asked.clone(), context: self.context() };
        let Missing { missing } = self.slot.call(method::BLOBS_HAS, &req, self.store.as_ref(), ctx)?;
        let known = |h: &String| asked.contains(h).then(|| h.parse::<Hash>().ok()).flatten();
        missing
            .iter()
            .map(|h| {
                known(h).ok_or_else(|| self.slot.misbehaved(format!("it lacks {h:?}, which it wasn't asked about")))
            })
            .collect()
    }

    fn put(&self, hash: &Hash, path: &Path, ctx: &Ctx<'_>) -> Result<()> {
        let req =
            PutBlob { destination: self.name.clone(), hash: hash.to_hex(), path: path.into(), context: self.context() };
        self.slot.call::<Value>(method::BLOBS_PUT, &req, self.store.as_ref(), ctx).map(drop)
    }

    fn get(&self, hash: &Hash, into: &Path, ctx: &Ctx<'_>) -> Result<()> {
        let req = GetBlob { destination: self.name.clone(), hash: hash.to_hex(), context: self.context() };
        let copy = |got: Got, scratch: &super::blobs::Scratch, _: &_| {
            let mut file = scratch.blob(&got.blob.blob).map_err(|e| self.slot.misbehaved(e))?;
            let mut out = std::fs::File::create(into).at(into)?;
            std::io::copy(&mut file, &mut out).at(into).map(drop)
        };
        self.slot.call_with(method::BLOBS_GET, &req, self.store.as_ref(), ctx, &|_| {}, copy)
    }

    fn put_manifest(&self, name: &str, bytes: &[u8], ctx: &Ctx<'_>) -> Result<()> {
        let blob = Blob { blob: RECORD.into() };
        let req = PutManifest { destination: self.name.clone(), name: name.into(), blob, context: self.context() };
        let given = [(RECORD, bytes)];
        let store = self.store.as_ref();
        self.slot.call_giving(method::MANIFEST_PUT, &req, &given, &[], store, ctx, &|_| {}, |_: Value, _, _| Ok(()))
    }

    /// A record is taken in as what the plugin answers is (`process::printed`), when
    /// it's JSON, but a snapshot's: that's checked against its id, the hash of what the store
    /// wrote, once it's read.
    fn get_manifest(&self, name: &str, ctx: &Ctx<'_>) -> Result<Option<Vec<u8>>> {
        let req = GetManifest { destination: self.name.clone(), name: name.into(), context: self.context() };
        let take_in = |bytes: Vec<u8>| {
            let record = serde_json::from_slice(&bytes).ok().filter(|_| !name.starts_with("snapshots/"));
            match record.and_then(super::process::printed) {
                Some(taken) => serde_json::to_vec_pretty(&taken).map_err(|e| Error::invalid(e.to_string())),
                None => Ok(bytes),
            }
        };
        let read = |got: GotManifest, scratch: &super::blobs::Scratch, _: &_| {
            got.blob.map(|blob| self.read(scratch, &blob).and_then(take_in)).transpose()
        };
        self.slot.call_with(method::MANIFEST_GET, &req, self.store.as_ref(), ctx, &|_| {}, read)
    }
}

#[cfg(test)]
mod tests;
