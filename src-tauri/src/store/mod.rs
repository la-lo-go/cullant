//! Project storage abstraction.
//!
//! The "project root" is either a real filesystem directory (desktop, and the
//! private `.cullant` sidecar on every platform) or an Android SAF `content://`
//! tree (a picked SD card / USB volume). Every module that touches project media
//! goes through [`ProjectStore`] instead of `std::fs`/`PathBuf` directly, so the
//! same culling logic works against both.
//!
//! Paths are always project-root-relative, `/`-separated strings — matching the
//! `files.rel_path` column the DB already stores.

use crate::error::{AppError, AppResult};

pub mod stats;

mod local;
pub use local::LocalFsStore;

#[cfg(target_os = "android")]
mod saf;
#[cfg(target_os = "android")]
pub use saf::SafStore;

/// A single media file discovered by a recursive listing. `mtime` is in whole
/// seconds since the Unix epoch (0 if unknown), matching `files.mtime`.
#[derive(Debug, Clone)]
pub struct StoreEntry {
    pub rel_path: String,
    pub size: i64,
    pub mtime: i64,
}

/// The MIME type SAF uses for directories; also handy as a sentinel for
/// `create` calls that should make a folder. Only referenced by the Android
/// backend.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub const MIME_DIRECTORY: &str = "vnd.android.document/directory";

/// A frame produced by a backend's own platform media API, for the backends
/// that have one — a video's poster, or a still nothing here can decode.
/// `width`/`height` are the source's real dimensions, which `jpeg` may be scaled
/// down from.
pub struct PlatformFrame {
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Whether the platform already applied the source's rotation. Android's
    /// decoders do; the caller must then not apply the EXIF orientation again,
    /// or a portrait photo comes out sideways.
    pub oriented: bool,
}

/// How often a walk reports its running count. Frequent enough to look alive on
/// slow storage, rare enough that the emit itself costs nothing.
pub const WALK_PROGRESS_EVERY: usize = 200;

/// Backend-agnostic operations on a project's files. All paths are
/// project-root-relative, `/`-separated.
pub trait ProjectStore: Send + Sync {
    /// Every media file anywhere under the root, skipping any directory whose
    /// name is in `skip_dirs` or begins with `.`. Directories are not returned.
    ///
    /// `progress` is called with the running entry count as the walk proceeds.
    /// The walk is the longest silent stretch of opening a project — on SAF it
    /// is one binder round-trip per directory — so it has to report from inside
    /// rather than only once it returns.
    fn list_recursive(
        &self,
        skip_dirs: &[&str],
        progress: &mut dyn FnMut(usize),
    ) -> AppResult<Vec<StoreEntry>>;

    /// Open a file for reading. Returns a real `std::fs::File` on every backend
    /// (on SAF via a detached ParcelFileDescriptor), so callers get normal
    /// `Read`/`Seek`.
    fn open_read(&self, rel: &str) -> AppResult<std::fs::File>;

    /// Create or truncate a file and return it for writing.
    fn open_write(&self, rel: &str, mime_type: &str) -> AppResult<std::fs::File>;

    fn create_dir_all(&self, rel: &str) -> AppResult<()>;

    fn rename_in_place(&self, rel: &str, new_name: &str) -> AppResult<String>;

    /// Move an entry into an existing directory and return its new relative path.
    fn move_to(&self, rel: &str, new_parent_rel: &str) -> AppResult<String>;

    /// Copy a file. The parent of `to_rel` must already exist.
    fn copy(&self, from_rel: &str, to_rel: &str) -> AppResult<()>;

    fn remove_file(&self, rel: &str) -> AppResult<()>;

    fn exists(&self, rel: &str) -> AppResult<bool>;

    /// The real OS path for `rel`, if this backend has one. `Some` only for the
    /// local filesystem; `None` for SAF. Used by the few operations that must
    /// hand a concrete path to an OS API (e.g. memory-mapping the file to decode).
    fn local_path(&self, _rel: &str) -> Option<std::path::PathBuf> {
        None
    }

    /// Return an identifier that opens this folder as a separate project.
    fn folder_project_path(&self, _rel: &str) -> AppResult<String> {
        Err(AppError::Other(
            "folder projects are not supported by this store".into(),
        ))
    }

    /// Whether this backend extracts video poster frames itself, through a
    /// platform media API. True only for Android SAF, which has neither an
    /// ffmpeg binary to shell out to nor a real path to hand it (see
    /// [`local_path`](ProjectStore::local_path)).
    fn extracts_video_posters(&self) -> bool {
        false
    }

    /// Extract a poster frame from the video at `rel`, scaled so its longest
    /// edge is at most `max_edge` (0 = the frame's native size). Only backends
    /// that report [`extracts_video_posters`](ProjectStore::extracts_video_posters)
    /// implement this.
    fn video_poster(&self, _rel: &str, _max_edge: u32) -> AppResult<PlatformFrame> {
        Err(crate::error::AppError::Other(
            "video_poster is not supported by this store".into(),
        ))
    }

    /// Whether this backend decodes HEIF stills itself, through a platform image
    /// API. Unlike [`extracts_video_posters`](ProjectStore::extracts_video_posters)
    /// this is a runtime fact, not a property of the backend: Android decodes
    /// HEIF only from API 28, and the app supports 24.
    fn decodes_heif(&self) -> bool {
        false
    }

    /// Decode the HEIF still at `rel`, scaled so its longest edge is at most
    /// `max_edge` (0 = native size). Only backends that report
    /// [`decodes_heif`](ProjectStore::decodes_heif) implement this.
    fn heif_still(&self, _rel: &str, _max_edge: u32) -> AppResult<PlatformFrame> {
        Err(crate::error::AppError::Other(
            "heif_still is not supported by this store".into(),
        ))
    }

    /// Hand the file at `rel` to the OS to open in an external default app (used
    /// as the "play in an external video player" escape hatch when the in-app
    /// WebView can't decode a clip). Only the SAF backend implements this —
    /// backends that expose a [`local_path`](ProjectStore::local_path) are opened
    /// through the `opener` plugin by the caller instead.
    fn open_external(&self, _rel: &str) -> AppResult<()> {
        Err(crate::error::AppError::Other(
            "open_external is not supported by this store".into(),
        ))
    }
}

/// Read an entire file from the store into memory. Used by the decoders, which
/// need the whole file (RAW container parsing, EXIF, image decode) and cannot
/// stream from a `content://` URI lazily.
pub fn read_all(store: &dyn ProjectStore, rel: &str) -> AppResult<Vec<u8>> {
    use std::io::Read;
    let mut f = store.open_read(rel)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(buf)
}

/// Split a `/`-separated rel_path into (parent, last_segment). Parent is `""`
/// for a top-level entry.
pub(crate) fn split_parent(rel: &str) -> (&str, &str) {
    match rel.rfind('/') {
        Some(i) => (&rel[..i], &rel[i + 1..]),
        None => ("", rel),
    }
}

pub(crate) fn validate_relative(rel: &str) -> AppResult<()> {
    if !rel.is_empty()
        && (rel.contains(['\\', ':', '\0'])
            || rel
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == ".."))
    {
        return Err(AppError::Other(format!(
            "invalid project-relative path: {rel}"
        )));
    }
    Ok(())
}
