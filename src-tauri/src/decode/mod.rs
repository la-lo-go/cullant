pub mod exif;
pub mod jpeg;
pub mod raw;

use std::sync::Arc;

use rawler::rawsource::RawSource;

use crate::error::AppResult;
use crate::store::{read_all, ProjectStore};

/// Open project media as a `RawSource`, the cheapest way available:
/// - Desktop (`local_path` is `Some`): memory-map the file. Container parsing
///   and embedded-preview extraction then only page in the bytes they touch —
///   a few MB of a RAW instead of the whole sensor payload. Caveat (accepted):
///   an in-page I/O error on a vanishing removable/network drive surfaces as
///   a hard fault rather than an `Err`, same exposure as rawler's own tools.
/// - Android SAF (`None`): read the whole file into memory as before; rawler
///   has no reader-based source, so bytes are the only option there.
///
/// `RawSource` derefs to `&[u8]`, so the same source also feeds the EXIF and
/// plain-JPEG decode paths without another read.
#[allow(dead_code)] // wired in by the thumbs/ingest refactor (next commits)
pub fn open_source(store: &dyn ProjectStore, rel: &str) -> AppResult<RawSource> {
    if let Some(path) = store.local_path(rel) {
        if let Ok(source) = RawSource::new(&path) {
            return Ok(source);
        }
        // mmap can fail on exotic filesystems; fall through to a plain read.
    }
    Ok(RawSource::new_from_shared_vec(Arc::new(read_all(store, rel)?)).with_path(rel))
}

/// File classification by extension. Mirrors `files.kind` in the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Raw = 0,
    Image = 1,
    Video = 2,
    Sidecar = 3,
}

pub const RAW_EXTS: &[&str] = &[
    "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "raf", "orf", "ors", "dng", "rw2",
    "pef", "srw", "x3f", "3fr", "fff", "iiq", "erf", "mef", "mos", "kdc", "dcr", "raw",
];
pub const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "tif", "tiff", "webp", "bmp", "gif"];
pub const VIDEO_EXTS: &[&str] = &["mp4", "mov", "m4v"];
pub const SIDECAR_EXTS: &[&str] = &["xmp"];

pub fn classify(ext: &str) -> Option<FileKind> {
    if RAW_EXTS.contains(&ext) {
        Some(FileKind::Raw)
    } else if IMAGE_EXTS.contains(&ext) {
        Some(FileKind::Image)
    } else if VIDEO_EXTS.contains(&ext) {
        Some(FileKind::Video)
    } else if SIDECAR_EXTS.contains(&ext) {
        Some(FileKind::Sidecar)
    } else {
        None
    }
}
