pub mod exif;
pub mod raw;

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
