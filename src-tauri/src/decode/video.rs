//! Video poster-frame extraction.
//!
//! Cullant has no in-process video decoder. Rather than take on a heavy,
//! fragile native dependency (which would fight the pinned `rawler` build and
//! the MSVC toolchain), it borrows one from the platform, and which one depends
//! on the store the project lives in:
//!
//! - **A real filesystem** (every desktop project): shell out to the `ffmpeg`
//!   binary when it is present on `PATH` and read a single early frame as PNG
//!   on stdout.
//! - **Android SAF**: ask the store, which goes through Android's
//!   `MediaMetadataRetriever`. There is no ffmpeg binary on a phone, and it
//!   could not read a `content://` URI if there were.
//!
//! Either way the bytes go to the `image` crate and then down the exact same
//! resize/JPEG/cache path every image thumbnail already uses.
//!
//! ffmpeg is an OPTIONAL runtime dependency: it is probed for once, and
//! [`poster_possible`] lets callers degrade gracefully when it is missing
//! (video thumbnails are simply skipped — never a crash, never a hard build
//! dependency).

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use image::DynamicImage;

use crate::error::{AppError, AppResult};
use crate::store::ProjectStore;

/// Seek offsets (seconds) tried in order. A tiny skip past the very start
/// avoids the black/leader frames many clips open on; a 0s fallback still
/// yields a poster for clips shorter than the first offset.
const SEEK_SECONDS: &[&str] = &["1", "0"];

/// Configure a spawned ffmpeg process. On Windows this suppresses the console
/// window that would otherwise flash for every extracted frame.
#[cfg(windows)]
fn configure(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn configure(_cmd: &mut Command) {}

fn ffmpeg() -> Command {
    let mut cmd = Command::new("ffmpeg");
    configure(&mut cmd);
    cmd
}

/// Whether an `ffmpeg` binary is reachable on `PATH`. Probed once and cached
/// for the process lifetime — the answer cannot change mid-session, and the
/// probe would otherwise run for every video in the project.
fn is_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        let mut cmd = ffmpeg();
        cmd.args(["-hide_banner", "-loglevel", "error", "-version"]);
        cmd.stdin(std::process::Stdio::null());
        cmd.output().map(|o| o.status.success()).unwrap_or(false)
    })
}

/// Run ffmpeg to grab one frame at `seek` seconds, decoded from the PNG it
/// writes to stdout. Returns `Ok(None)` when ffmpeg produced no frame (e.g. the
/// seek landed past the end of a short clip) so the caller can try an earlier
/// offset; `Err` only for a genuine spawn/IO failure.
fn frame_at(path: &Path, seek: &str) -> AppResult<Option<DynamicImage>> {
    // `-ss` before `-i` = fast keyframe seek; `-autorotate` (ffmpeg default)
    // already bakes in display rotation, so posters need no extra orient pass.
    let output = ffmpeg()
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            seek,
        ])
        .arg("-i")
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-an",
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "-",
        ])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| AppError::Decode(format!("ffmpeg spawn failed: {e}")))?;

    if !output.status.success() || output.stdout.is_empty() {
        return Ok(None);
    }
    match image::load_from_memory(&output.stdout) {
        Ok(img) => Ok(Some(img)),
        Err(e) => Err(AppError::Decode(format!("ffmpeg frame decode: {e}"))),
    }
}

fn extract_poster_ffmpeg(path: &Path) -> AppResult<DynamicImage> {
    for seek in SEEK_SECONDS {
        if let Some(img) = frame_at(path, seek)? {
            return Ok(img);
        }
    }
    Err(AppError::Decode(format!(
        "ffmpeg produced no frame for {}",
        path.display()
    )))
}

/// Whether a poster frame can be produced for `rel_path` at all — asked before
/// any work is queued, so that a missing extractor is reported as "not now"
/// rather than as an undecodable file. Installing ffmpeg does not change any
/// file's mtime, so the answer must stay cheap enough to ask on every attempt.
pub fn poster_possible(store: &dyn ProjectStore, rel_path: &str) -> bool {
    match store.local_path(rel_path) {
        Some(_) => is_available(),
        None => store.extracts_video_posters(),
    }
}

/// Extract a poster frame from a video as a decoded image, together with the
/// clip's real display dimensions — which are NOT the returned image's when the
/// extractor scaled the frame down on the way out.
///
/// `max_long_edge` is the largest frame the caller can use; extractors that can
/// scale during extraction (rather than decode a 4K frame and throw most of it
/// away) are told so. Callers must first confirm [`poster_possible`]; a
/// corrupt/unsupported video yields `Err`, which the ingest pass tombstones
/// exactly like an undecodable image.
pub fn extract_poster(
    store: &dyn ProjectStore,
    rel_path: &str,
    max_long_edge: u32,
) -> AppResult<(DynamicImage, (u32, u32))> {
    let Some(path) = store.local_path(rel_path) else {
        // `u32::MAX` is how the thumbnail pipeline spells "never scale down";
        // the platform extractors spell it 0.
        let max_edge = if max_long_edge == u32::MAX {
            0
        } else {
            max_long_edge
        };
        let poster = store.video_poster(rel_path, max_edge)?;
        let img = image::load_from_memory(&poster.jpeg)
            .map_err(|e| AppError::Decode(format!("{rel_path}: poster frame decode: {e}")))?;
        return Ok((img, (poster.width, poster.height)));
    };
    // ffmpeg hands back the frame at the video's own resolution, so the image
    // itself is the authority on the clip's dimensions.
    let img = extract_poster_ffmpeg(&path)?;
    let dims = (img.width(), img.height());
    Ok((img, dims))
}
