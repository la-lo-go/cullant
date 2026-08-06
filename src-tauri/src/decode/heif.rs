//! HEIF still decoding, by borrowing a decoder from the platform.
//!
//! Cullant has no in-process HEIF decoder and will not grow one: every library
//! that decodes HEVC is a C dependency, which would be the first in this build
//! and would have to cross-compile for four Android ABIs. So this is a
//! capability ladder, tried in order, exactly like [`super::video`] does for
//! poster frames:
//!
//! 1. **The store's own decoder** — Android SAF, through `ImageDecoder`. First,
//!    because on SAF there is no OS path to hand anything else.
//! 2. **The platform's in-process decoder** — Windows WIC today, iOS ImageIO
//!    later. Behind `local_path`, so a new platform is a new arm and nothing
//!    else.
//! 3. **`ffmpeg`** — the generalist fallback, and the only rung that also covers
//!    Linux and macOS. Already an optional runtime dependency for video posters.
//!
//! Every rung can be absent at runtime, so [`possible`] exists and callers must
//! ask it first. A "no decoder" answer is **not** a decode failure: it must never
//! be tombstoned, because a tombstone is keyed on the file's mtime and shipping
//! a decoder changes no file's mtime — every photo touched would stay an empty
//! cell forever after the upgrade.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use image::DynamicImage;

use crate::error::{AppError, AppResult};
use crate::store::ProjectStore;

/// The first ffmpeg release whose demuxer reads a HEIF *still*. Earlier builds
/// decode HEVC video perfectly well and still cannot open a `.HEIC`, so presence
/// on `PATH` is not capability.
const FFMPEG_HEIF_MAJOR: u32 = 7;

/// Cleared when the ffmpeg rung fails on the first HEIF of the session, which
/// means the binary cannot do stills whatever its version string claimed.
///
/// Only the *first* attempt may clear it. A later failure is far more likely to
/// be one corrupt file than a bad ffmpeg, and one corrupt file must not disable
/// the rung for the whole library.
static FFMPEG_STILLS: AtomicBool = AtomicBool::new(true);
static FFMPEG_TRIED: AtomicBool = AtomicBool::new(false);

/// Pretend no rung exists, so a test can exercise the "no decoder" path on a
/// machine that does have ffmpeg. Serialise callers: the latch is process-wide.
#[cfg(test)]
pub(crate) fn disable_for_test() {
    FFMPEG_STILLS.store(false, Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) fn reenable_for_test() {
    FFMPEG_STILLS.store(true, Ordering::Relaxed);
}

/// Whether `ffmpeg` on this machine can be expected to open a HEIF still.
fn ffmpeg_capable() -> bool {
    if !FFMPEG_STILLS.load(Ordering::Relaxed) {
        return false;
    }
    let info = super::video::ffmpeg_info();
    // `None` is a git or nightly build, which is newer than any release.
    info.present && info.major.is_none_or(|m| m >= FFMPEG_HEIF_MAJOR)
}

/// Whether any rung can decode a HEIF for this store at all.
///
/// Store-level, so the thumbnail pregeneration queue can ask once instead of
/// once per file. [`possible`] is the per-file question.
///
/// It must claim only what is *built*, not what a platform could in principle
/// offer: an optimistic answer here becomes a decode failure downstream, and a
/// decode failure becomes a tombstone that survives installing the decoder.
pub fn available(store: &dyn ProjectStore) -> bool {
    store.decodes_heif() || ffmpeg_capable()
}

/// Whether this particular file can be decoded — asked before any work is
/// queued, so a missing decoder is reported as "not now" rather than as an
/// undecodable file.
pub fn possible(store: &dyn ProjectStore, rel_path: &str) -> bool {
    if store.decodes_heif() {
        return true;
    }
    // Every remaining rung needs a real path.
    store.local_path(rel_path).is_some() && ffmpeg_capable()
}

/// Decode a HEIF still, together with its real dimensions — which are NOT the
/// returned image's when the decoder scaled on the way out — and whether the
/// decoder already applied the source's rotation.
///
/// That last flag is not optional. A HEIF carries its rotation twice: in the
/// `irot` item property and in EXIF `Orientation`. Every platform decoder
/// applies `irot` itself, so a caller that then applied the EXIF orientation
/// would turn a portrait photo sideways.
///
/// `max_long_edge` is the largest frame the caller can use. Callers must first
/// confirm [`possible`].
pub fn decode(
    store: &dyn ProjectStore,
    rel_path: &str,
    max_long_edge: u32,
) -> AppResult<(DynamicImage, (u32, u32), bool)> {
    if store.decodes_heif() {
        // `u32::MAX` is how the thumbnail pipeline spells "never scale down";
        // the platform decoders spell it 0.
        let max_edge = if max_long_edge == u32::MAX {
            0
        } else {
            max_long_edge
        };
        let frame = store.heif_still(rel_path, max_edge)?;
        let img = image::load_from_memory(&frame.jpeg)
            .map_err(|e| AppError::Decode(format!("{rel_path}: heif frame decode: {e}")))?;
        return Ok((img, (frame.width, frame.height), frame.oriented));
    }

    let path = store.local_path(rel_path).ok_or_else(|| {
        AppError::Decode(format!("{rel_path}: no decoder for this container yet"))
    })?;

    ffmpeg_still(&path, rel_path)
}

/// Decode a HEIF still with `ffmpeg`, as PNG on stdout.
///
/// No scaling is requested. ffmpeg has to decode the whole frame regardless, so
/// asking it to shrink first would only move the resize off `fast_image_resize`
/// and onto a slower one.
fn ffmpeg_still(path: &Path, rel_path: &str) -> AppResult<(DynamicImage, (u32, u32), bool)> {
    let output = super::video::ffmpeg()
        .args(["-nostdin", "-hide_banner", "-loglevel", "error"])
        .arg("-i")
        .arg(path)
        .args(["-frames:v", "1", "-f", "image2pipe", "-vcodec", "png", "-"])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| AppError::Decode(format!("ffmpeg spawn failed: {e}")))?;

    let first_try = !FFMPEG_TRIED.swap(true, Ordering::Relaxed);
    if !output.status.success() || output.stdout.is_empty() {
        // The binary ran and refused the file. On the very first HEIF of the
        // session that means it cannot do stills at all, whatever it reported
        // as its version — so stop paying a process per photo to find out again.
        if first_try {
            FFMPEG_STILLS.store(false, Ordering::Relaxed);
        }
        return Err(AppError::Decode(format!(
            "{rel_path}: ffmpeg read no frame"
        )));
    }

    let img = image::load_from_memory(&output.stdout)
        .map_err(|e| AppError::Decode(format!("{rel_path}: ffmpeg frame decode: {e}")))?;
    let dims = (img.width(), img.height());
    // ffmpeg applies the container's rotation on the way out, the same as it
    // does for a video frame.
    Ok((img, dims, true))
}

#[cfg(test)]
mod tests {
    use crate::decode::video::ffmpeg_major;

    /// The version parser decides whether a machine's ffmpeg is even tried, so
    /// it has to survive the shapes real builds actually print.
    #[test]
    fn reads_the_major_version_of_a_release() {
        assert_eq!(
            ffmpeg_major("ffmpeg version 6.1.1-essentials_build"),
            Some(6)
        );
        assert_eq!(
            ffmpeg_major("ffmpeg version 7.1.1-full_build-www.gyan.dev"),
            Some(7)
        );
        assert_eq!(ffmpeg_major("ffmpeg version n7.0"), Some(7));
        assert_eq!(ffmpeg_major("ffmpeg version 8.0.1-full_build"), Some(8));
    }

    /// A git or nightly build states no release number. It must read as "newer
    /// than any release", which is `None` — never as 0, and never as the year.
    #[test]
    fn a_development_build_states_no_version() {
        assert_eq!(ffmpeg_major("ffmpeg version N-119583-g1c0f4d2e4a"), None);
        assert_eq!(ffmpeg_major("ffmpeg version 2025-01-01-git-abcdef"), None);
        assert_eq!(ffmpeg_major("some other program version 9.9"), None);
    }
}
