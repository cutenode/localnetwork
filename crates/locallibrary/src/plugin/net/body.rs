//! What a plugin's `net/fetch` sends: text, a file or part of one, or a `multipart/form-data`
//! form of text and files. Files are read as they're sent, never held whole, and opened again for
//! each redirect that asks for the body again; the bytes sent are the progress of the hook call
//! they're for (the one lent the file, or whose scratch folder it's in).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Take};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use loclib_plugin::{ByteRange, RpcError};
use reqwest::blocking::{RequestBuilder, multipart};

use super::invalid;
use crate::plugin::blobs::Under;
use crate::plugin::process::Live;
use crate::progress::CancelToken;

/// How often what's sent of a file is reported, at most.
const REPORT_EVERY: Duration = Duration::from_millis(100);

/// A request's body, as it's sent again at each redirect that keeps it.
pub(super) enum Body {
    Text(Vec<u8>),
    File(FilePart),
    Form(Vec<(String, Field)>),
}

/// A form's part, by its name.
pub(super) enum Field {
    Text(String),
    File { file: FilePart, filename: Option<String>, content_type: Option<String> },
}

/// The part of a file a request sends.
pub(super) struct FilePart {
    path: PathBuf,
    offset: u64,
    length: u64,
}

impl FilePart {
    /// The part `range` says (all of it when none) of the file `path` names (see
    /// [`crate::plugin::blobs::Scratches::open`]), once it's one the plugin may send and the part
    /// is in it.
    pub(super) fn new(live: &Live, path: &Path, range: Option<ByteRange>) -> Result<FilePart, RpcError> {
        let ByteRange { offset, length } = range.unwrap_or_default();
        let (file, _) = live.scratches.open(path).map_err(|e| about(path, e))?;
        let size = file.metadata().map_err(|e| about(path, e))?.len();
        let length = length.unwrap_or(size.saturating_sub(offset));
        if offset.checked_add(length).is_none_or(|end| end > size) {
            return Err(about(path, format!("{length} bytes from {offset} go past its end, at {size}")));
        }
        Ok(FilePart { path: path.to_path_buf(), offset, length })
    }

    /// Its bytes, read from the file opened again, as they're sent: stopping once `cancel` is.
    fn reader(&self, live: &Live, cancel: &CancelToken) -> Result<Sending, RpcError> {
        let (mut file, under) = live.scratches.open(&self.path).map_err(|e| about(&self.path, e))?;
        file.seek(SeekFrom::Start(self.offset)).map_err(|e| about(&self.path, e))?;
        let file = file.take(self.length);
        Ok(Sending { file, under, cancel: cancel.clone(), unreported: 0, reported: Instant::now() })
    }
}

impl Body {
    /// How many bytes of files it sends.
    pub(super) fn file_bytes(&self) -> u64 {
        match self {
            Body::Text(_) => 0,
            Body::File(file) => file.length,
            Body::Form(fields) => fields
                .iter()
                .map(|(_, f)| match f {
                    Field::File { file, .. } => file.length,
                    Field::Text(_) => 0,
                })
                .sum(),
        }
    }

    /// `request` sending it, its files opened again: as long as it is (`Content-Length`), and
    /// for a form, of its type.
    pub(super) fn send(
        &self,
        request: RequestBuilder,
        live: &Live,
        cancel: &CancelToken,
    ) -> Result<RequestBuilder, RpcError> {
        match self {
            Body::Text(bytes) => Ok(request.body(bytes.clone())),
            Body::File(file) => {
                Ok(request.body(reqwest::blocking::Body::sized(file.reader(live, cancel)?, file.length)))
            }
            Body::Form(fields) => {
                let mut form = multipart::Form::new();
                for (name, field) in fields {
                    let part = match field {
                        Field::Text(value) => multipart::Part::text(value.clone()),
                        Field::File { file, filename, content_type } => {
                            let mut part = multipart::Part::reader_with_length(file.reader(live, cancel)?, file.length);
                            if let Some(filename) = filename {
                                part = part.file_name(filename.clone());
                            }
                            match content_type {
                                Some(t) => {
                                    part.mime_str(t).map_err(|_| invalid(format!("{t:?} isn't a content type")))?
                                }
                                None => part,
                            }
                        }
                    };
                    form = form.part(name.clone(), part);
                }
                Ok(request.multipart(form))
            }
        }
    }
}

/// What went wrong with the file `path` names, as the plugin is told.
fn about(path: &Path, e: impl std::fmt::Display) -> RpcError {
    invalid(format!("{}: {e}", path.display()))
}

/// A file's part as it's sent: what's read reported now and then (and once it's all read) to the
/// hook call it's for, and failing once the request is cancelled.
struct Sending {
    file: Take<File>,
    under: Arc<Under>,
    cancel: CancelToken,
    unreported: u64,
    reported: Instant,
}

impl Sending {
    fn report(&mut self) {
        if self.unreported > 0 {
            self.under.sent(self.unreported);
            self.unreported = 0;
        }
        self.reported = Instant::now();
    }
}

impl Read for Sending {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.cancel.is_cancelled() {
            return Err(std::io::Error::other("cancelled"));
        }
        let n = self.file.read(buf)?;
        self.unreported += n as u64;
        // reqwest reads no further than the length it was told, so never to the end.
        if self.file.limit() == 0 || self.reported.elapsed() >= REPORT_EVERY {
            self.report();
        }
        Ok(n)
    }
}

/// What was read and not reported yet, when a request stops sending it early.
impl Drop for Sending {
    fn drop(&mut self) {
        self.report();
    }
}
