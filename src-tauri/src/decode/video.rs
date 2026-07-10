//! Video poster-frame extraction, isolated behind `ffmpeg`.
//!
//! Cullant has no in-process video decoder. Rather than take on a heavy,
//! fragile native dependency (which would fight the pinned `rawler` build and
//! the MSVC toolchain), we shell out to the `ffmpeg` binary when it is present
//! on `PATH` and extract a single early frame as PNG on stdout, then hand the
//! bytes to the `image` crate — the exact same decode/resize/JPEG path every
//! image thumbnail already uses.
//!
//! ffmpeg is an OPTIONAL runtime dependency: [`is_available`] probes for it
//! once and callers degrade gracefully when it is missing (video thumbnails
//! are simply skipped — never a crash, never a hard build dependency).

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use image::DynamicImage;

use crate::error::{AppError, AppResult};

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
pub fn is_available() -> bool {
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

/// Extract a poster frame from a video file as a decoded image. Callers must
/// first confirm [`is_available`]; a corrupt/unsupported video yields `Err`,
/// which the ingest pass tombstones exactly like an undecodable image.
pub fn extract_poster(path: &Path) -> AppResult<DynamicImage> {
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
