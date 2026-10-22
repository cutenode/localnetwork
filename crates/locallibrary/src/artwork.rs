//! Cover art.
//!
//! On a drive, players show artwork from `PIONEER/Artwork/<folder>/a<id>.jpg` (80x80) and the
//! `a<id>_m.jpg` variant (240x240), referenced by an Artwork row in `export.pdb`. OneLibrary
//! (`exportLibrary.db`) has its own `b<id>.jpg` / `b<id>_m.jpg` pairs and image rows. Setting art
//! on a drive renders both sizes and updates whichever databases the drive has; the audio files
//! are left alone. It's an edit ([`crate::edit::Edit::SetArtwork`]): the image is staged on the
//! drive by its hash, so the journal names it and a replay sets it again.
//!
//! **Why not embed art into the audio files on the drive?** Rewriting a file's tags shifts the
//! audio data, which invalidates the seek index rekordbox stored in the analysis files for VBR
//! MP3s (and the file size in `export.pdb`). [`embed_in_file`] exists for the audio files in your
//! *computer's* collection: embed there, "Reload Tag" in rekordbox, and every future export
//! carries the art.

use rustc_hash::{FxHashMap, FxHashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use base64::prelude::*;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};
use loclib_model::audio::Format;
use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::tag::Tag;
use rekordbox_pdb::PdbEditor;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::commit::{self, Changes, NewDatabases, NewFile, Options};
use crate::device::{Device, LineageOp};
use crate::edit::Replayed;
use crate::error::IoContext;
use crate::fsutil;
use crate::hash::Hash;
use crate::model::{Library, Track, TrackRef};
use crate::onelibrary;
use crate::paths;
use crate::pdb;
use crate::progress::{Ctx, Event, Phase};
use crate::schema::{Failure, Skip};
use crate::staged::IMAGES;
use crate::store::Store;
use crate::{Error, Result};

pub const THUMB_SIZE: u32 = 80;
pub const MEDIUM_SIZE: u32 = 240;
/// Size used when embedding into audio files.
pub const EMBED_SIZE: u32 = 600;

/// `a1.jpg` -> `a1_m.jpg`.
pub fn medium_variant(key: &str) -> String {
    match key.rsplit_once('.') {
        Some((stem, ext)) if !ext.contains('/') => format!("{stem}_m.{ext}"),
        _ => format!("{key}_m"),
    }
}

/// A free key for `export.pdb` artwork `id`, following rekordbox's layout of 20 images per folder
/// (`a1`..`a20` in `00001`, `a21`.. in `00002`, ...), moving to later folders if the name is
/// somehow taken.
pub fn free_artwork_key(root: &Path, id: u32, ext: &str) -> String {
    free_image_key(root, 'a', id, ext)
}

/// Like [`free_artwork_key`] with another file prefix: OneLibrary's images are `b<id>.jpg`.
pub fn free_image_key(root: &Path, prefix: char, id: u32, ext: &str) -> String {
    (id.saturating_sub(1) / 20 + 1..)
        .map(|folder| image_key_in(folder, prefix, id, ext))
        .find(|k| !paths::to_fs(root, k).exists() && !paths::to_fs(root, &medium_variant(k)).exists())
        .expect("unbounded")
}

/// Where rekordbox's layout puts image `id` (see [`free_artwork_key`]), taken or not.
pub fn image_key(prefix: char, id: u32, ext: &str) -> String {
    image_key_in(id.saturating_sub(1) / 20 + 1, prefix, id, ext)
}

fn image_key_in(folder: u32, prefix: char, id: u32, ext: &str) -> String {
    format!("PIONEER/Artwork/{folder:05}/{prefix}{id}.{ext}")
}

/// Decoded source image.
pub struct Artwork {
    image: DynamicImage,
}

impl Artwork {
    pub fn from_bytes(bytes: &[u8]) -> Result<Artwork> {
        let image = image::load_from_memory(bytes).map_err(|e| Error::Image(e.to_string()))?;
        if image.width() < 16 || image.height() < 16 {
            return Err(Error::Image("image is too small to use as artwork".into()));
        }
        Ok(Artwork { image })
    }

    /// Load an image file, or the embedded cover of an audio file.
    pub fn from_path(path: &Path) -> Result<Artwork> {
        if is_audio_path(path) {
            let bytes = read_embedded(path)?
                .ok_or_else(|| Error::NotFound(format!("no embedded artwork in {}", path.display())))?;
            return Artwork::from_bytes(&bytes);
        }
        let bytes = std::fs::read(path).at(path)?;
        Artwork::from_bytes(&bytes)
    }

    /// Square JPEG of `size` x `size`, center-cropped.
    pub fn jpeg(&self, size: u32) -> Result<Vec<u8>> {
        let img = self.image.resize_to_fill(size, size, FilterType::Lanczos3).to_rgb8();
        let mut out = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
        img.write_with_encoder(enc).map_err(|e| Error::Image(e.to_string()))?;
        Ok(out)
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.image.width(), self.image.height())
    }
}

fn is_audio_path(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).and_then(Format::from_extension).is_some()
}

/// Embedded front cover (or first picture) of an audio file.
pub fn read_embedded(path: &Path) -> Result<Option<Vec<u8>>> {
    let tagged = lofty::read_from_path(path).map_err(|e| Error::Tags(format!("{}: {e}", path.display())))?;
    let pictures: Vec<&Picture> = tagged.tags().iter().flat_map(|t| t.pictures()).collect();
    let chosen = pictures.iter().find(|p| p.pic_type() == PictureType::CoverFront).or_else(|| pictures.first());
    Ok(chosen.map(|p| p.data().to_vec()))
}

/// Embed `art` as the front cover of an audio file (replacing any existing front cover).
/// Intended for files in the computer's collection, not files on an exported drive.
pub fn embed_in_file(path: &Path, art: &Artwork) -> Result<()> {
    let jpeg = art.jpeg(EMBED_SIZE.min(art.dimensions().0.max(art.dimensions().1)).max(THUMB_SIZE))?;
    let tag_err = |e: &dyn std::fmt::Display| Error::Tags(format!("{}: {e}", path.display()));
    let mut tagged = lofty::read_from_path(path).map_err(|e| tag_err(&e))?;
    if tagged.primary_tag().is_none() {
        let tag_type = tagged.primary_tag_type();
        tagged.insert_tag(Tag::new(tag_type));
    }
    let tag = tagged.primary_tag_mut().expect("inserted above");
    tag.remove_picture_type(PictureType::CoverFront);
    tag.push_picture(Picture::unchecked(jpeg).pic_type(PictureType::CoverFront).mime_type(MimeType::Jpeg).build());
    // Into a copy next to the file (with its extension, which lofty may go by), renamed over it
    // once written: lofty rewrites a file in place, moving the audio when the tag grows, and a
    // crash or a full disk then would leave it cut short.
    let dir = path.parent().ok_or_else(|| Error::invalid(format!("{} has no parent", path.display())))?;
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let tmp = fsutil::temp_for(dir, path, &ext)?;
    std::fs::copy(path, tmp.path()).at(path)?;
    tag.save_to_path(tmp.path(), WriteOptions::default()).map_err(|e| tag_err(&e))?;
    tmp.as_file().sync_all().at(tmp.path())?;
    fsutil::replace(tmp.into_temp_path(), path)
}

// ---------------------------------------------------------------------------------------------
// Art embedded in the computer's audio files, and taking it back out
// ---------------------------------------------------------------------------------------------

/// Copies of the audio files one embed rewrote, as they were, kept in the store
/// (`embed-backups/<id>/`) so [`restore_embedded`] can put them back.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EmbedBackup {
    pub id: String,
    pub created: chrono::DateTime<chrono::Utc>,
    /// The image that went in.
    pub image: PathBuf,
    pub files: Vec<EmbeddedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EmbeddedFile {
    /// The audio file.
    pub path: PathBuf,
    /// Its copy as it was, in the backup's folder.
    pub saved: String,
    /// Its modification time before (put back with it).
    pub mtime_before: i64,
    /// The file once the art went in: it's only put back while it's still that.
    pub size: u64,
    pub mtime: i64,
}

/// Embeds kept, newest first.
const KEEP_EMBEDS: usize = 5;

fn embeds_dir(store: &Store) -> PathBuf {
    store.root().join("embed-backups")
}

/// The embeds that can be taken back, newest first.
pub fn embed_backups(store: &Store) -> Result<Vec<EmbedBackup>> {
    let dir = embeds_dir(store);
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else { return Ok(out) };
    for e in entries.flatten() {
        if let Some(b) = fsutil::read_json_opt::<EmbedBackup>(&e.path().join("backup.json"))? {
            out.push(b);
        }
    }
    out.sort_by_key(|b| std::cmp::Reverse(b.created));
    Ok(out)
}

/// Space the embeds kept take in the store: whole copies of the files they rewrote.
pub fn embed_backup_bytes(store: &Store) -> u64 {
    crate::space::dir_size(&embeds_dir(store)).1
}

/// What embedding art into files did.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct EmbedReport {
    pub embedded: Vec<PathBuf>,
    pub failed: Vec<Failure>,
    /// The copies of the files as they were (see [`restore_embedded`]).
    pub backup: Option<String>,
}

/// Embed `art` in each file, keeping a copy of each as it was in the store first. A file that
/// fails is reported, and the others go on. The backup lists each file as soon as it's done, so
/// one stopped part way (cancelled between files, or a crash) can still be taken back.
pub fn embed_with_backup(
    store: &Store,
    files: &[PathBuf],
    art: &Artwork,
    image: &Path,
    ctx: &Ctx<'_>,
) -> Result<EmbedReport> {
    let created = chrono::Utc::now();
    let mut id = created.format("%Y%m%dT%H%M%S%.3fZ").to_string();
    let dir = loop {
        let d = embeds_dir(store).join(&id);
        if !d.exists() {
            break d;
        }
        id.push('_');
    };
    let mut report = EmbedReport { embedded: Vec::new(), failed: Vec::new(), backup: None };
    let mut backup = EmbedBackup { id: id.clone(), created, image: image.to_path_buf(), files: Vec::new() };
    let mut stopped = Ok(());
    ctx.phase_start(Phase::Write, files.len() as u64, 0);
    for (i, f) in files.iter().enumerate() {
        stopped = ctx.check();
        if stopped.is_err() {
            break;
        }
        ctx.emit(Event::Item { phase: Phase::Write, path: f.display().to_string(), bytes: 0 });
        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let saved = format!("{i:04} {name}");
        let result = (|| -> Result<EmbeddedFile> {
            let before = std::fs::metadata(f).at(f)?;
            fsutil::copy_file(f, &dir.join(&saved), Some(fsutil::mtime_secs(&before)), None, true, &Ctx::silent())?;
            if let Err(e) = embed_in_file(f, art) {
                let _ = std::fs::remove_file(dir.join(&saved));
                return Err(e);
            }
            let after = std::fs::metadata(f).at(f)?;
            Ok(EmbeddedFile {
                path: f.clone(),
                saved: saved.clone(),
                mtime_before: fsutil::mtime_secs(&before),
                size: after.len(),
                mtime: fsutil::mtime_secs(&after),
            })
        })();
        match result {
            Ok(e) => {
                report.embedded.push(f.clone());
                backup.files.push(e);
                fsutil::write_json_atomic(&dir.join("backup.json"), &backup)?;
            }
            Err(e) => report.failed.push(Failure::new(f.display().to_string(), e)),
        }
    }
    ctx.phase_end(Phase::Write);
    if backup.files.is_empty() {
        let _ = std::fs::remove_dir_all(&dir);
        stopped?;
        return Ok(report);
    }
    stopped?;
    report.backup = Some(id);
    for old in embed_backups(store)?.into_iter().skip(KEEP_EMBEDS) {
        let d = embeds_dir(store).join(&old.id);
        std::fs::remove_dir_all(&d).at(&d)?;
    }
    Ok(report)
}

/// What taking embedded art back out did.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct UnembedReport {
    pub backup: String,
    pub restored: Vec<PathBuf>,
    /// Files changed since (or gone), left as they are.
    pub skipped: Vec<Skip>,
}

/// Put back the files an embed rewrote (the latest, or `id`), each only if it hasn't changed
/// since. The backup goes once every file is back.
pub fn restore_embedded(store: &Store, id: Option<&str>) -> Result<UnembedReport> {
    let backups = embed_backups(store)?;
    let b = match id {
        Some(id) => {
            backups.into_iter().find(|b| b.id == id).ok_or_else(|| Error::NotFound(format!("embed backup {id}")))?
        }
        None => backups.into_iter().next().ok_or_else(|| Error::NotFound("no embedded art to take back".into()))?,
    };
    let dir = embeds_dir(store).join(&b.id);
    let mut report = UnembedReport { backup: b.id.clone(), restored: Vec::new(), skipped: Vec::new() };
    for f in &b.files {
        match std::fs::metadata(&f.path) {
            Ok(m) if m.len() == f.size && fsutil::mtime_eq(fsutil::mtime_secs(&m), f.mtime) => {
                fsutil::copy_file(&dir.join(&f.saved), &f.path, Some(f.mtime_before), None, true, &Ctx::silent())?;
                report.restored.push(f.path.clone());
            }
            Ok(_) => report.skipped.push(Skip::new(f.path.display().to_string(), "changed since the art went in")),
            Err(_) => report.skipped.push(Skip::new(f.path.display().to_string(), "gone")),
        }
    }
    if report.skipped.is_empty() {
        std::fs::remove_dir_all(&dir).at(&dir)?;
    }
    Ok(report)
}

// ---------------------------------------------------------------------------------------------
// Drive artwork
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ArtworkReport {
    pub tracks_updated: usize,
    pub artwork_rows_added: usize,
    pub files_written: Vec<String>,
    pub backup: Option<String>,
    pub fingerprint: Option<Hash>,
}

/// The image an artwork edit gives tracks (see [`crate::edit::Edit::SetArtwork`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImageSource {
    /// An image file on this computer, or an audio file whose embedded art to use.
    Path(PathBuf),
    /// The image file's bytes, in base64.
    Base64(String),
    /// An image staged on the drive (`.locallibrary/artwork/<hash>.jpg`, a JPEG of the larger
    /// size players show), by its hash: how the journal records the others.
    Staged(Hash),
    /// A file a plugin's answer names in its request's scratch folder: how plugins give images.
    /// The plugin host reads its bytes once it has checked it.
    #[schemars(skip)]
    Blob(String),
}

/// The image an artwork edit sets, as the drive keeps it for the journal: a JPEG of the larger
/// size players show, named by its hash, with its bytes unless it was given staged (the plan holds
/// them, so pruning the drive's copy before it's applied can't lose them). The players' two sizes
/// are made from those bytes, so a replay sets exactly the same artwork.
pub(crate) fn stage(device: &Device, image: &ImageSource) -> Result<(Hash, Option<Vec<u8>>)> {
    let art = match image {
        ImageSource::Staged(hash) if IMAGES.path(device, hash).is_file() => return Ok((*hash, None)),
        ImageSource::Staged(hash) => {
            return Err(Error::NotFound(format!("the staged image {} on {}", hash.short(), device.name())));
        }
        ImageSource::Path(path) => Artwork::from_path(path)?,
        ImageSource::Blob(_) => return Err(Error::invalid("only a plugin's plan gives an image as a blob")),
        ImageSource::Base64(text) => Artwork::from_bytes(
            &BASE64_STANDARD.decode(text).map_err(|e| Error::Image(format!("the image isn't base64: {e}")))?,
        )?,
    };
    let bytes = art.jpeg(MEDIUM_SIZE)?;
    Ok((Hash::of(&bytes), Some(bytes)))
}

/// The images, in each database, of the tracks of these audio files (device paths): what setting
/// their artwork may replace in place, for the backup made first.
pub(crate) fn image_files(device: &Device, files: &[String]) -> Result<Vec<String>> {
    let wanted: FxHashSet<String> = files.iter().map(|f| paths::file_key(f)).collect();
    let mut libraries = vec![device.library()?];
    libraries.extend(crate::clean::open_plus(device, device.has_device_library(), &mut Vec::new())?.map(|(l, _)| l));
    let mut out = Vec::new();
    for t in libraries.iter().flat_map(|l| &l.tracks) {
        let Some(image) = t.artwork_path.as_deref().filter(|_| wanted.contains(&paths::file_key(&t.file_path))) else {
            continue;
        };
        let key = paths::key(image);
        for k in [medium_variant(&key), key] {
            if paths::is_safe_key(&k) && device.checked_path(&k).is_ok_and(|p| p.is_file()) {
                out.push(k);
            }
        }
    }
    Ok(out)
}

/// Make an artwork edit of a plan's on the tracks of these audio files (device paths), its image
/// staged as `hash` (from `images`, else the drive's copy), under the backup and lock the plan
/// took (see [`crate::edit::apply`]). The image is staged first, for the journal entry to name.
/// Replaying (not `strict`), one that no longer applies is skipped with a warning, and tracks that
/// are gone are left out; an error is a failure while writing.
pub(crate) fn replay(
    device: &mut Device,
    files: &[String],
    hash: &Hash,
    images: &FxHashMap<Hash, Vec<u8>>,
    strict: bool,
    ctx: &Ctx<'_>,
) -> Result<Replayed> {
    let mut out = Replayed::default();
    let described = format!("set the artwork of {} track(s)", files.len());
    let library = device.library()?;
    let by_key: FxHashMap<String, u32> = library.tracks.iter().map(|t| (paths::file_key(&t.file_path), t.id)).collect();
    let id = |f: &String| by_key.get(&paths::file_key(f)).copied();
    let ids: Vec<u32> = files.iter().filter_map(id).collect();
    if let Some(example) = files.iter().find(|f| id(f).is_none()) {
        out.tracks_gone(strict, files.len() - ids.len(), example, &described)?;
    }
    if ids.is_empty() {
        return out.skipped(strict, &described, Error::NotFound("the tracks".into()));
    }
    let path = IMAGES.path(device, hash);
    let medium = match images.get(hash).cloned().map_or_else(|| std::fs::read(&path), Ok) {
        Ok(bytes) => bytes,
        Err(_) => {
            let gone = Error::NotFound(format!("the staged image {} on {}", hash.short(), device.name()));
            return out.skipped(strict, &described, gone);
        }
    };
    let thumb = Artwork::from_bytes(&medium)?.jpeg(THUMB_SIZE)?;
    let fresh = !path.is_file();
    if fresh {
        fsutil::write_atomic(&path, &medium)?;
    }
    let swaps = device.swaps();
    let set = apply_batches(device, &[(ids, Sizes { thumb, medium })], None, &mut out.warnings, ctx);
    // Failing before it changed the drive, it leaves no staged image either (best effort: the
    // error that got here is what's reported).
    if set.is_err() && fresh && device.swaps() == swaps {
        let _ = fsutil::remove_if_exists(&path);
        let _ = path.parent().map(std::fs::remove_dir);
    }
    let (report, databases) = set?;
    out.databases = report.files_written.into_iter().chain(databases).collect();
    out.fingerprint = report.fingerprint;
    Ok(out)
}

/// An image as the JPEGs players show: an 80x80 thumbnail and a 240x240 one.
struct Sizes {
    thumb: Vec<u8>,
    medium: Vec<u8>,
}

/// For each batch of track ids (sorted), the image key to overwrite in place when the tracks are
/// exactly the users of one artwork row that nothing else (`shared`) uses, else `None`.
fn plan_in_place(library: &Library, shared: &FxHashSet<u32>, batches: &[Vec<u32>]) -> Vec<Option<String>> {
    let mut users: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
    for t in &library.tracks {
        if let Some(a) = t.artwork_id {
            users.entry(a).or_default().push(t.id);
        }
    }
    batches
        .iter()
        .map(|sorted| {
            let existing: Vec<Option<u32>> = library.tracks_by_id(sorted).iter().map(|t| t.artwork_id).collect();
            let exclusive = match existing.first() {
                Some(Some(a)) if existing.iter().all(|e| e == &Some(*a)) && !shared.contains(a) => {
                    let mut u = users[a].clone();
                    u.sort_unstable();
                    (&u == sorted).then_some(*a)
                }
                _ => None,
            };
            exclusive
                .and_then(|a| library.tracks.iter().find(|t| t.artwork_id == Some(a))?.artwork_path.clone())
                .map(|p| paths::key(&p))
                .filter(|k| paths::is_safe_key(k) && paths::classify(k) == Some(paths::FileClass::Artwork))
        })
        .collect()
}

/// The batches in OneLibrary's track ids, and the image each overwrites in place (if any).
struct PlusBatches {
    ids: Vec<Vec<u32>>,
    in_place: Vec<Option<String>>,
}

/// Give each batch of tracks (ids from [`Device::library`]) its image, on a drive the caller
/// locked, backed up first with `backup` (the reason). If a batch's tracks are exactly the users
/// of one artwork row, its image files are replaced in place (no database change). Otherwise a
/// new row is added and the tracks point at it, so tracks that merely shared the old art (the rest
/// of an album, say) keep theirs. Both databases are updated when the drive has both (OneLibrary's
/// tracks are matched by file path). Returns the databases written too.
fn apply_batches(
    device: &mut Device,
    batches: &[(Vec<u32>, Sizes)],
    backup: Option<&str>,
    warnings: &mut Vec<String>,
    ctx: &Ctx<'_>,
) -> Result<(ArtworkReport, Vec<String>)> {
    let library = device.library()?;
    let sorted: Vec<Vec<u32>> = batches
        .iter()
        .map(|(ids, _)| {
            let mut v = ids.clone();
            v.sort_unstable();
            v.dedup();
            v
        })
        .collect();
    let has_pdb = device.has_device_library();
    let mut plus = crate::clean::open_plus(device, has_pdb, warnings)?;

    // The same batches in OneLibrary's ids. `library` is export.pdb's when the drive has one.
    let mut plus_plan: Option<PlusBatches> = None;
    if let Some((plus_lib, one)) = &plus {
        let batches_plus: Vec<Vec<u32>> = if has_pdb {
            let by_path: FxHashMap<String, u32> =
                plus_lib.tracks.iter().map(|t| (paths::file_key(&t.file_path), t.id)).collect();
            let mut missing = 0;
            let mapped = sorted
                .iter()
                .map(|ids| {
                    let mut v: Vec<u32> = library
                        .tracks_by_id(ids)
                        .into_iter()
                        .filter_map(|t| {
                            let found = by_path.get(&paths::file_key(&t.file_path)).copied();
                            missing += usize::from(found.is_none());
                            found
                        })
                        .collect();
                    v.sort_unstable();
                    v.dedup();
                    v
                })
                .collect();
            if missing > 0 {
                warnings.push(format!(
                    "{missing} selected track(s) aren't in exportLibrary.db; players that read OneLibrary won't show their new artwork"
                ));
            }
            mapped
        } else {
            sorted.clone()
        };
        let shared: FxHashSet<u32> = one.images_used_outside_tracks()?.into_iter().map(|(id, _)| id).collect();
        let in_place = plan_in_place(plus_lib, &shared, &batches_plus);
        plus_plan = Some(PlusBatches { ids: batches_plus, in_place });
    }
    let pdb_in_place = if has_pdb { plan_in_place(&library, &FxHashSet::default(), &sorted) } else { Vec::new() };

    let mut report = ArtworkReport {
        tracks_updated: sorted.iter().map(Vec::len).sum(),
        artwork_rows_added: 0,
        files_written: Vec::new(),
        backup: None,
        fingerprint: None,
    };

    // 1. The images rendered and the databases edited and read back in memory, before anything
    // on the drive changes: a failure leaves no backup and no interrupted operation.
    let root = device.root().to_path_buf();
    // Image key, thumbnail and medium size, to write.
    let mut images: Vec<(String, Vec<u8>, Vec<u8>)> = Vec::new();
    let mut stage_images = |key: &str, thumb: &[u8], medium: &[u8]| -> Result<()> {
        device.checked_path(key)?;
        device.checked_path(&medium_variant(key))?;
        images.push((key.to_string(), thumb.to_vec(), medium.to_vec()));
        Ok(())
    };
    ctx.phase_start(Phase::Write, batches.len() as u64, 0);
    let mut pdb_bytes = None;
    let mut editor = if has_pdb { Some(PdbEditor::from_bytes(device.pdb_bytes()?)) } else { None };
    let mut pdb_changed = false;
    for (i, (_, Sizes { thumb, medium })) in batches.iter().enumerate() {
        ctx.check()?;
        if let Some(editor) = &mut editor {
            let key = match &pdb_in_place[i] {
                Some(k) => k.clone(),
                None => {
                    pdb_changed = true;
                    report.artwork_rows_added += 1;
                    pdb::guard_editor(|| {
                        let next_id = editor.database()?.artwork.iter().map(|a| a.id).max().unwrap_or(0) + 1;
                        let key = free_artwork_key(&root, next_id, "jpg");
                        let id = editor.add_artwork(&paths::device_path(&key))?;
                        for t in &sorted[i] {
                            editor.set_track_field(*t, "artwork_id", id)?;
                        }
                        Ok(key)
                    })?
                }
            };
            stage_images(&key, thumb, medium)?;
        }
        if let (Some((_, one)), Some(p)) = (&mut plus, &plus_plan) {
            if p.ids[i].is_empty() {
                continue;
            }
            let key = match &p.in_place[i] {
                Some(k) => k.clone(),
                None => {
                    report.artwork_rows_added += 1;
                    let key = free_image_key(&root, 'b', one.next_image_id()?, "jpg");
                    let id = one.add_image(&paths::device_path(&key))?;
                    one.set_track_image(&p.ids[i], id)?;
                    key
                }
            };
            stage_images(&key, thumb, medium)?;
        }
    }
    if let Some(mut editor) = editor.filter(|_| pdb_changed) {
        let bytes = pdb::guard_editor(|| Ok(editor.to_bytes()))?;
        pdb::parse_database(bytes.clone(), &device.path(paths::EXPORT_PDB))?;
        pdb_bytes = Some(bytes);
    }
    let plus_changed = plus_plan
        .as_ref()
        .is_some_and(|p| p.ids.iter().zip(&p.in_place).any(|(ids, k)| !ids.is_empty() && k.is_none()));
    let plus_bytes = match &plus {
        Some((_, one)) if plus_changed => {
            let bytes = one.to_encrypted()?;
            onelibrary::OneLibrary::from_encrypted(&bytes, None)?.library()?;
            Some(bytes)
        }
        _ => None,
    };

    ctx.phase_end(Phase::Write);

    // 2. Back up (with the images overwritten in place), then swap the images in before the
    // databases that name them.
    let mut files = Vec::new();
    for (key, thumb, medium) in images {
        files.push(NewFile::bytes(medium_variant(&key), medium));
        files.push(NewFile::bytes(key, thumb));
    }
    let changes = Changes {
        databases: NewDatabases { pdb: pdb_bytes, ext: None, plus: plus_bytes },
        files,
        deletes: Vec::new(),
        remove_first: Vec::new(),
    };
    let mut opts = Options::new(LineageOp::Artwork, "set artwork");
    opts.backup = backup.map(str::to_string);
    let done = commit::commit(device, changes, opts, ctx)?;
    report.backup = done.backup;
    report.fingerprint = done.fingerprint;
    report.files_written = done.written;
    Ok((report, done.databases))
}

/// Tracks whose artwork is missing (no row, or the image file is gone).
pub fn missing(device: &Device, library: &Library) -> Vec<TrackRef> {
    library
        .tracks
        .iter()
        .filter(|t| match &t.artwork_path {
            None => true,
            Some(p) => !paths::to_fs(device.root(), &paths::key(p)).is_file(),
        })
        .map(TrackRef::from)
        .collect()
}

/// Read a track's artwork from a drive (medium size if present), e.g. for a UI preview.
pub fn read_from_drive(device: &Device, library: &Library, track_id: u32) -> Result<Option<Vec<u8>>> {
    let t = library.track(track_id).ok_or_else(|| Error::no_track(track_id))?;
    file_on_drive(device, t).map(|f| std::fs::read(&f).at(&f)).transpose()
}

/// A track's artwork file on a drive: the medium size players show when there is one, else the
/// thumbnail.
pub fn file_on_drive(device: &Device, t: &Track) -> Option<PathBuf> {
    let key = paths::key(t.artwork_path.as_ref()?);
    [medium_variant(&key), key].iter().filter_map(|k| device.checked_path(k).ok()).find(|f| f.is_file())
}

const FOLDER_IMAGES: &[&str] = &["cover", "folder", "front", "album", "artwork"];

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AutoArtworkPlan {
    pub found: Vec<AutoArtworkItem>,
    pub not_found: Vec<TrackRef>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AutoArtworkItem {
    pub track: TrackRef,
    /// `embedded` or the image file's key.
    pub source: String,
}

/// For tracks without artwork, find art embedded in their audio file or a cover image
/// (`cover.jpg`, `folder.png`, ...) next to it on the drive.
pub fn plan_auto(device: &Device, library: &Library) -> AutoArtworkPlan {
    let mut plan = AutoArtworkPlan { found: Vec::new(), not_found: Vec::new() };
    let wanted: rustc_hash::FxHashSet<u32> = missing(device, library).iter().map(|t| t.id).collect();
    for t in library.tracks.iter().filter(|t| wanted.contains(&t.id)) {
        let Ok(audio) = device.checked_path(&paths::key(&t.file_path)) else {
            plan.not_found.push(t.into());
            continue;
        };
        let source = if read_embedded(&audio).ok().flatten().is_some() {
            Some("embedded".to_string())
        } else {
            folder_image(&audio)
                .and_then(|p| p.strip_prefix(device.root()).ok().map(|r| paths::key(&r.to_string_lossy())))
        };
        match source {
            Some(source) => plan.found.push(AutoArtworkItem { track: t.into(), source }),
            None => plan.not_found.push(t.into()),
        }
    }
    plan
}

fn folder_image(audio: &Path) -> Option<PathBuf> {
    let dir = audio.parent()?;
    let entries: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.into_iter().find(|p| {
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
        let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
        FOLDER_IMAGES.contains(&stem.as_str()) && matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp")
    })
}

/// Apply [`plan_auto`]. Tracks whose source image is byte-identical share one artwork row.
pub fn apply_auto(device: &mut Device, plan: &AutoArtworkPlan, ctx: &Ctx<'_>) -> Result<ArtworkReport> {
    let mut groups: FxHashMap<Hash, (Vec<u32>, Vec<u8>)> = FxHashMap::default();
    let library = device.library()?;
    for item in &plan.found {
        let Some(t) = library.track(item.track.id) else { continue };
        let bytes = if item.source == "embedded" {
            read_embedded(&device.checked_path(&paths::key(&t.file_path))?)?
        } else {
            let p = device.checked_path(&item.source)?;
            Some(std::fs::read(&p).at(&p)?)
        };
        if let Some(bytes) = bytes {
            groups.entry(Hash::of(&bytes)).or_insert_with(|| (Vec::new(), bytes)).0.push(t.id);
        }
    }
    let mut decoded: Vec<(Vec<u32>, Sizes)> = Vec::new();
    for (ids, bytes) in groups.into_values() {
        match Artwork::from_bytes(&bytes) {
            Ok(a) => decoded.push((ids, Sizes { thumb: a.jpeg(THUMB_SIZE)?, medium: a.jpeg(MEDIUM_SIZE)? })),
            Err(e) => ctx.warn(format!("skipping undecodable artwork for {} track(s): {e}", ids.len())),
        }
    }
    // Nothing to set (no art found, or none of it decodes): the drive isn't written, not even a
    // backup.
    if decoded.is_empty() {
        return Ok(ArtworkReport {
            tracks_updated: 0,
            artwork_rows_added: 0,
            files_written: Vec::new(),
            backup: None,
            fingerprint: None,
        });
    }
    let _lock = commit::lock(device, None)?;
    let mut warnings = Vec::new();
    let (report, _) = apply_batches(device, &decoded, Some("before setting artwork"), &mut warnings, ctx)?;
    for w in warnings {
        ctx.warn(w);
    }
    Ok(report)
}

/// The longest side of a [`to_png`] preview, in pixels.
pub const PREVIEW_SIZE: u32 = 512;

/// Encode any supported image as PNG (handy for UIs that want a lossless preview), shrunk to
/// [`PREVIEW_SIZE`] on its longest side if it's bigger. The bytes may come from anywhere (a
/// plugin's picture), so one wider or taller than 8192 pixels, or needing more than 256 MiB
/// decoded, is refused without being decoded.
pub fn to_png(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut limits = image::Limits::default();
    (limits.max_image_width, limits.max_image_height, limits.max_alloc) = (Some(8192), Some(8192), Some(256 << 20));
    let mut reader =
        image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| Error::Image(e.to_string()))?;
    reader.limits(limits);
    let mut img = reader.decode().map_err(|e| Error::Image(e.to_string()))?;
    if img.width().max(img.height()) > PREVIEW_SIZE {
        img = img.resize(PREVIEW_SIZE, PREVIEW_SIZE, FilterType::Triangle);
    }
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).map_err(|e| Error::Image(e.to_string()))?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests;
