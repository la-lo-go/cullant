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
    let _profile = crate::photo_profile::span("source_open");
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

/// HEIF and its camera spellings: still images whose pixels only a *borrowed*
/// decoder can reach (see [`heif`]), never one in this process.
///
/// They are `FileKind::Image` so that grouping, deletion, move rules and XMP
/// treat them as the photos they are — before that, an unknown extension was
/// skipped outright, and deleting a rejected shot left its `.HIF` behind as an
/// orphan the user never knew existed. Their EXIF is read like any photo's:
/// `kamadak-exif` parses the ISO-BMFF item structure, so a HEIF-only library
/// sorts by real capture time and fills every filter facet even where no rung of
/// the ladder can render it.
///
/// Deliberately NOT merged into [`IMAGE_EXTS`], which means "decodable in
/// process" and governs the sibling borrow: rendering a RAW's thumbnail from a
/// HEIF would trade the RAW's own embedded JPEG for a subprocess or a JNI hop.
pub const HEIF_EXTS: &[&str] = &["heic", "heif", "hif", "hsp"];

pub const VIDEO_EXTS: &[&str] = &["mp4", "mov", "m4v"];
pub const SIDECAR_EXTS: &[&str] = &["xmp"];

pub fn classify(ext: &str) -> Option<FileKind> {
    if RAW_EXTS.contains(&ext) {
        Some(FileKind::Raw)
    } else if IMAGE_EXTS.contains(&ext) || HEIF_EXTS.contains(&ext) {
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

/// One camera name from the EXIF Make and Model, which the RAW path and the
/// image path both have to build.
///
/// Most cameras write Make "Canon" and Model "EOS R6", so the pair reads as one
/// name. Some repeat themselves — a Sigma fp writes Make "SIGMA" and Model
/// "SIGMA fp" — and joining those blindly puts "SIGMA SIGMA fp" in the camera
/// filter as its own entry, beside whatever the same body wrote elsewhere.
pub fn camera_name(make: &str, model: &str) -> Option<String> {
    let (make, model) = (make.trim(), model.trim());
    match (make.is_empty(), model.is_empty()) {
        (true, true) => None,
        (true, false) => Some(model.to_string()),
        (false, true) => Some(make.to_string()),
        (false, false) if model.to_lowercase().starts_with(&make.to_lowercase()) => {
            Some(model.to_string())
        }
        (false, false) => Some(format!("{make} {model}")),
    }
}

/// Whether this path names a HEIF, and so needs a borrowed decoder rather than
/// an in-process one.
pub fn is_heif(rel: &str) -> bool {
    ext_of(rel).is_some_and(|e| HEIF_EXTS.contains(&e.as_str()))
}

/// A group member's claim to be the group's primary — the file whose frame
/// stands for the shot in the grid, and the only member whose thumbnail is
/// pregenerated. Lower wins.
///
/// Five tiers: a RAW leads (it is the master, and its embedded preview is the
/// same frame), then an in-process decodable image, then a companion `.ORI`,
/// then a HEIF, and last whatever nothing can open.
///
/// A companion `.ORI` sits ABOVE a HEIF even though a HEIF is the better
/// picture, because the `.ORI` decodes *unconditionally* — rawler, in process,
/// on every platform — while a HEIF needs a rung of the ladder that this machine
/// may not have. A shot left holding only `IMG.ORI` and `IMG.HIF` must give the
/// cell to the member that is certain to render.
///
/// This ranking is **static on purpose**. It is persisted through
/// `groups.primary_file_id`, which travels with the project folder, so a
/// runtime-dependent rank would silently move which frame stands for the shot
/// every time the project opened on a different machine.
pub fn primary_rank(ext: &str) -> u8 {
    if RAW_EXTS.contains(&ext) && !SECONDARY_RAW_EXTS.contains(&ext) {
        0
    } else if IMAGE_EXTS.contains(&ext) {
        1
    } else if SECONDARY_RAW_EXTS.contains(&ext) {
        2
    } else if HEIF_EXTS.contains(&ext) {
        3
    } else {
        4
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
    fn a_raw_outranks_a_jpeg_which_outranks_a_heif() {
        assert!(primary_rank("cr3") < primary_rank("jpg"));
        assert!(primary_rank("jpg") < primary_rank("hif"));
    }

    /// A companion decodes everywhere and a HEIF only where the ladder has a
    /// rung, so a shot reduced to `IMG.ORI` + `IMG.HIF` must give the cell to
    /// the `.ORI`. A filename tie ("hif" sorts before "ori") must never decide it.
    #[test]
    fn a_companion_raw_outranks_a_heif() {
        assert!(primary_rank("ori") < primary_rank("hif"));
        assert!(primary_rank("ori") < primary_rank("heic"));
    }

    /// Every tier is distinct, and an unknown extension loses to all of them.
    #[test]
    fn a_heif_still_outranks_an_unknown_extension() {
        assert!(primary_rank("heic") < primary_rank("xyz"));
    }

    /// Real files, from the corpus: a Sigma fp repeats its maker in the model
    /// and would otherwise get its own "SIGMA SIGMA fp" entry in the filter.
    #[test]
    fn a_camera_name_never_repeats_its_maker() {
        assert_eq!(
            camera_name("SIGMA", "SIGMA fp").as_deref(),
            Some("SIGMA fp")
        );
        assert_eq!(
            camera_name("Canon", "EOS R6").as_deref(),
            Some("Canon EOS R6")
        );
        assert_eq!(
            camera_name("  Nikon ", " Z 8 ").as_deref(),
            Some("Nikon Z 8")
        );
        assert_eq!(camera_name("", "OM-1").as_deref(), Some("OM-1"));
        assert_eq!(camera_name("Leica", "").as_deref(), Some("Leica"));
        assert_eq!(camera_name("", ""), None);
    }

    #[test]
    fn a_heif_is_recognised_by_path() {
        assert!(is_heif("shoot/IMG_0421.HIF"));
        assert!(!is_heif("shoot/IMG_0421.CR3"));
        assert!(!is_heif(".gitignore"));
    }
}
