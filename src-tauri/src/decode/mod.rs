pub mod exif;
pub mod heif;
pub mod jpeg;
pub mod raw;
pub mod video;

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
pub fn open_source(store: &dyn ProjectStore, rel: &str) -> AppResult<RawSource> {
    let started = std::time::Instant::now();
    let source = open_source_inner(store, rel)?;
    crate::store::stats::source_opened(source.buf().len(), started.elapsed());
    Ok(source)
}

fn open_source_inner(store: &dyn ProjectStore, rel: &str) -> AppResult<RawSource> {
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
    "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "raf", "orf", "ors", "ori", "dng",
    "rw2", "pef", "srw", "x3f", "3fr", "fff", "iiq", "erf", "mef", "mos", "kdc", "dcr", "raw",
];

/// Companion RAWs a camera writes *next to* the real RAW, never instead of it.
/// OM System stores the untouched frame as `.ORI` beside the `.ORF` in Live ND,
/// Live Composite and hand-held Hi-Res, so one shot lands as three files. They
/// decode like any other RAW; they just have no claim to represent the shot.
pub const SECONDARY_RAW_EXTS: &[&str] = &["ori"];

/// Still images Cullant can decode.
pub const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "tif", "tiff", "webp", "bmp", "gif"];

/// Still images Cullant catalogues and reads metadata from, but cannot yet
/// decode: HEIF and its camera spellings. They are `FileKind::Image` so that
/// grouping, deletion, move rules and XMP treat them as the photos they are —
/// before this, an unknown extension was skipped outright, and deleting a
/// rejected shot left its `.HIF` behind as an orphan the user never knew existed.
///
/// EXIF *is* readable: `kamadak-exif` parses the ISO-BMFF item structure, so a
/// HEIF-only library sorts by real capture time and fills every filter facet.
/// Only the pixels are out of reach, so the sibling-borrow queries filter on
/// [`IMAGE_EXTS`] rather than on `kind`, and the cells stay empty.
pub const OPAQUE_IMAGE_EXTS: &[&str] = &["heic", "heif", "hif", "hsp"];

pub const VIDEO_EXTS: &[&str] = &["mp4", "mov", "m4v"];
pub const SIDECAR_EXTS: &[&str] = &["xmp"];

pub fn classify(ext: &str) -> Option<FileKind> {
    if RAW_EXTS.contains(&ext) {
        Some(FileKind::Raw)
    } else if IMAGE_EXTS.contains(&ext) || OPAQUE_IMAGE_EXTS.contains(&ext) {
        Some(FileKind::Image)
    } else if VIDEO_EXTS.contains(&ext) {
        Some(FileKind::Video)
    } else if SIDECAR_EXTS.contains(&ext) {
        Some(FileKind::Sidecar)
    } else {
        None
    }
}

/// The lowercased extension of a relative path, if it has one. A name that is
/// only a leading dot (`.gitignore`) has none.
///
/// The one parser in the codebase: the scan classifies files with it, and the
/// ingest asks it whether a file is worth opening. Two copies would let those
/// two answers drift apart.
pub fn ext_of(rel: &str) -> Option<String> {
    let last = rel.rsplit('/').next().unwrap_or(rel);
    match last.rfind('.') {
        Some(i) if i > 0 => Some(last[i + 1..].to_lowercase()),
        _ => None,
    }
}

/// Whether this path names a still image that is catalogued but undecodable.
/// Phase A skips opening one entirely, the way it skips videos: nothing here can
/// read the bytes, and on Android SAF the file would have to be read whole to
/// learn that.
pub fn is_opaque_image(rel: &str) -> bool {
    ext_of(rel).is_some_and(|e| OPAQUE_IMAGE_EXTS.contains(&e.as_str()))
}

/// A group member's claim to be the group's primary — the file whose frame
/// stands for the shot in the grid, and the only member whose thumbnail is
/// pregenerated. Lower wins.
///
/// Four tiers, not three. A RAW leads (it is the master, and its embedded
/// preview is the same frame), then a decodable image, then a companion `.ORI`,
/// and last whatever cannot be decoded at all. The companion and the opaque file
/// must not share a tier: a shot left holding only `IMG.ORI` and `IMG.HIF` would
/// then pick between them on filename alone, and `hif` sorts first — so the cell
/// would be permanently blank while the `.ORI` beside it decodes fine.
pub fn primary_rank(ext: &str) -> u8 {
    if RAW_EXTS.contains(&ext) && !SECONDARY_RAW_EXTS.contains(&ext) {
        0
    } else if IMAGE_EXTS.contains(&ext) {
        1
    } else if SECONDARY_RAW_EXTS.contains(&ext) {
        2
    } else {
        3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heif_spellings_are_catalogued_as_images() {
        for ext in ["heic", "heif", "hif", "hsp"] {
            assert_eq!(classify(ext), Some(FileKind::Image), "{ext}");
        }
    }

    #[test]
    fn an_ori_is_a_raw_that_never_represents_the_shot() {
        assert_eq!(classify("ori"), Some(FileKind::Raw));
        assert!(primary_rank("orf") < primary_rank("ori"));
        assert!(primary_rank("jpg") < primary_rank("ori"));
    }

    #[test]
    fn a_raw_outranks_a_jpeg_which_outranks_an_opaque_heif() {
        assert!(primary_rank("cr3") < primary_rank("jpg"));
        assert!(primary_rank("jpg") < primary_rank("hif"));
    }

    /// A shot reduced to its companion and an undecodable sibling still has one
    /// member that renders. That one must take the cell, and a filename tie
    /// ("hif" sorts before "ori") must never decide it.
    #[test]
    fn a_companion_raw_outranks_an_opaque_image() {
        assert!(primary_rank("ori") < primary_rank("hif"));
        assert!(primary_rank("ori") < primary_rank("heic"));
    }

    #[test]
    fn an_opaque_image_is_recognised_by_path() {
        assert!(is_opaque_image("shoot/IMG_0421.HIF"));
        assert!(!is_opaque_image("shoot/IMG_0421.CR3"));
        assert!(!is_opaque_image(".gitignore"));
    }
}
