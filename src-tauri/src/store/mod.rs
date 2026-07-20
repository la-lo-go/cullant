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

use crate::error::AppResult;

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

/// Backend-agnostic operations on a project's files. All paths are
/// project-root-relative, `/`-separated.
pub trait ProjectStore: Send + Sync {
    /// Every media file anywhere under the root, skipping any directory whose
    /// name is in `skip_dirs` or begins with `.`. Directories are not returned.
    fn list_recursive(&self, skip_dirs: &[&str]) -> AppResult<Vec<StoreEntry>>;

    /// Open a file for reading. Returns a real `std::fs::File` on every backend
    /// (on SAF via a detached ParcelFileDescriptor), so callers get normal
    /// `Read`/`Seek`.
    fn open_read(&self, rel: &str) -> AppResult<std::fs::File>;

    /// Create (or truncate) a file for writing and return it.
    fn open_write(&self, rel: &str, mime_type: &str) -> AppResult<std::fs::File>;

    /// Ensure the directory at `rel` (and any missing ancestors) exists.
    fn create_dir_all(&self, rel: &str) -> AppResult<()>;

    /// Rename a file/dir in place (same parent). Returns the new rel_path.
    fn rename_in_place(&self, rel: &str, new_name: &str) -> AppResult<String>;

    /// Move a file/dir into `new_parent_rel` (which must already exist).
    /// Returns the new rel_path.
    fn move_to(&self, rel: &str, new_parent_rel: &str) -> AppResult<String>;

    /// Copy the file at `from_rel` to `to_rel` (parent of `to_rel` must exist).
    fn copy(&self, from_rel: &str, to_rel: &str) -> AppResult<()>;

    /// Permanently delete the file at `rel`.
    fn remove_file(&self, rel: &str) -> AppResult<()>;

    /// Whether anything exists at `rel`.
    fn exists(&self, rel: &str) -> AppResult<bool>;

    /// The real OS path for `rel`, if this backend has one. `Some` only for the
    /// local filesystem; `None` for SAF. Used by the few operations that must
    /// hand a concrete path to an OS API (e.g. memory-mapping the file to decode).
    fn local_path(&self, _rel: &str) -> Option<std::path::PathBuf> {
        None
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
