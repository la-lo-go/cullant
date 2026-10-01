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
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::{mpsc, OnceLock};
use std::time::{Duration, Instant};

use image::DynamicImage;

use crate::error::{AppError, AppResult};
use crate::store::ProjectStore;

/// Seek offsets (seconds) tried in order. A tiny skip past the very start
/// avoids the black/leader frames many clips open on; a 0s fallback still
/// yields a poster for clips shorter than the first offset.
const SEEK_SECONDS: &[&str] = &["1", "0"];

thread_local! {
    static CANCELLED: std::cell::RefCell<Option<Arc<AtomicBool>>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn set_cancellation(cancelled: Arc<AtomicBool>) {
    CANCELLED.with(|flag| *flag.borrow_mut() = Some(cancelled));
}

pub(crate) fn cancelled() -> bool {
    CANCELLED.with(|flag| {
        flag.borrow()
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
    })
}

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

pub(crate) fn ffmpeg() -> Command {
    let mut cmd = Command::new("ffmpeg");
    configure(&mut cmd);
    cmd
}

pub(crate) fn run_bounded(
    cmd: &mut Command,
    max_stdout: usize,
    timeout: Duration,
) -> AppResult<Output> {
    use std::io::Read;
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| AppError::Decode(format!("decoder spawn failed: {error}")))?;
    let (sender, receiver) = mpsc::channel();
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    fn reader(
        stream: impl Read + Send + 'static,
        limit: usize,
        stdout: bool,
        sender: mpsc::Sender<(bool, std::io::Result<Vec<u8>>)>,
    ) {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = stream
                .take(limit as u64 + 1)
                .read_to_end(&mut bytes)
                .and_then(|_| {
                    if bytes.len() > limit {
                        Err(std::io::Error::other("decoder output exceeds limit"))
                    } else {
                        Ok(bytes)
                    }
                });
            let _ = sender.send((stdout, result));
        });
    }
    reader(stdout, max_stdout, true, sender.clone());
    reader(stderr, 1024 * 1024, false, sender);
    let deadline = Instant::now() + timeout;
    let mut stdout = None;
    let mut stderr = None;
    let result = (|| loop {
        while let Ok((is_stdout, bytes)) = receiver.try_recv() {
            let bytes =
                bytes.map_err(|error| AppError::Decode(format!("decoder output: {error}")))?;
            if is_stdout {
                stdout = Some(bytes);
            } else {
                stderr = Some(bytes);
            }
        }
        if let Some(status) = child.try_wait()? {
            if stdout.is_some() && stderr.is_some() {
                return Ok(Output {
                    status,
                    stdout: stdout.take().unwrap(),
                    stderr: stderr.take().unwrap(),
                });
            }
        }
        if cancelled() {
            return Err(AppError::Decode("decoder cancelled".into()));
        }
        if Instant::now() >= deadline {
            return Err(AppError::Decode("decoder deadline exceeded".into()));
        }
        std::thread::sleep(Duration::from_millis(10));
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

/// What the one `ffmpeg -version` probe learned.
pub(crate) struct FfmpegInfo {
    /// A binary is reachable on `PATH` and ran.
    pub present: bool,
    /// Its major version, when the version line was parseable. `None` for a git
    /// or nightly build, whose version string carries no release number.
    pub major: Option<u32>,
}

/// The major version in an `ffmpeg -version` first line, when it states one.
///
/// A release says `ffmpeg version 7.1.1-full_build-…` or `ffmpeg version n7.0`.
/// A git or nightly build says `ffmpeg version N-119583-g…` or
/// `ffmpeg version 2025-01-01-git-…`, which carry no release number — a caller
/// must read `None` as "newer than any release", not as "too old".
pub(crate) fn ffmpeg_major(version_line: &str) -> Option<u32> {
    let rest = version_line.strip_prefix("ffmpeg version ")?;
    let token = rest.split_whitespace().next()?;
    let token = token.strip_prefix('n').unwrap_or(token);
    let digits = token.split(['.', '-']).next()?;
    // A date-stamped nightly ("2025-01-01-git-…") parses as a plausible major
    // version, so reject anything far past a real release number.
    match digits.parse::<u32>() {
        Ok(n) if n < 1000 => Some(n),
        _ => None,
    }
}

/// Probe `ffmpeg` once and cache the answer for the process lifetime — it cannot
/// change mid-session, and the probe would otherwise run for every media file in
/// the project.
pub(crate) fn ffmpeg_info() -> &'static FfmpegInfo {
    static INFO: OnceLock<FfmpegInfo> = OnceLock::new();
    INFO.get_or_init(|| {
        let mut cmd = ffmpeg();
        cmd.args(["-hide_banner", "-version"]);
        cmd.stdin(std::process::Stdio::null());
        let Ok(out) = run_bounded(&mut cmd, 64 * 1024, Duration::from_secs(5)) else {
            return FfmpegInfo {
                present: false,
                major: None,
            };
        };
        if !out.status.success() {
            return FfmpegInfo {
                present: false,
                major: None,
            };
        }
        let first = String::from_utf8_lossy(&out.stdout);
        FfmpegInfo {
            present: true,
            major: first.lines().next().and_then(ffmpeg_major),
        }
    })
}

fn is_available() -> bool {
    ffmpeg_info().present
}

/// Read a container date without decoding frames or loading the video in memory.
/// SAF has no date bridge; missing ffprobe or invalid metadata uses the mtime fallback.
pub fn capture_time(store: &dyn ProjectStore, rel_path: &str) -> Option<i64> {
    let path = store.local_path(rel_path)?;
    let mut cmd = Command::new("ffprobe");
    configure(&mut cmd);
    cmd.args([
        "-v",
        "error",
        "-protocol_whitelist",
        "file,pipe",
        "-show_entries",
        "format_tags=creation_time:stream_tags=creation_time",
        "-of",
        "json",
    ])
    .arg(path);
    let output = run_bounded(&mut cmd, 64 * 1024, Duration::from_secs(5)).ok()?;
    if !output.status.success() {
        return None;
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let containers = std::iter::once(metadata.get("format")).flatten().chain(
        metadata
            .get("streams")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten(),
    );
    containers
        .filter_map(|container| {
            let date = container.get("tags")?.get("creation_time")?.as_str()?;
            time::OffsetDateTime::parse(date, &time::format_description::well_known::Rfc3339)
                .ok()
                .map(|date| date.unix_timestamp())
        })
        .next()
}

/// Run ffmpeg to grab one frame at `seek` seconds, decoded from the PNG it
/// writes to stdout. Returns `Ok(None)` when ffmpeg produced no frame (e.g. the
/// seek landed past the end of a short clip) so the caller can try an earlier
/// offset; `Err` only for a genuine spawn/IO failure.
fn frame_at(
    path: &Path,
    seek: &str,
    max_long_edge: u32,
) -> AppResult<Option<(DynamicImage, (u32, u32))>> {
    // `-ss` before `-i` = fast keyframe seek; `-autorotate` (ffmpeg default)
    // already bakes in display rotation, so posters need no extra orient pass.
    let mut cmd = ffmpeg();
    cmd
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "info",
            "-ss",
            seek,
        ])
        .arg("-i")
        .arg(path)
        .arg("-vf")
        .arg(if max_long_edge == u32::MAX {
            "showinfo".to_string()
        } else {
            format!("showinfo,scale='min(iw,{max_long_edge})':'min(ih,{max_long_edge})':force_original_aspect_ratio=decrease")
        })
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
        .stdin(std::process::Stdio::null());
    let output = run_bounded(&mut cmd, 128 * 1024 * 1024, Duration::from_secs(12))?;

    if !output.status.success() || output.stdout.is_empty() {
        return Ok(None);
    }
    match image::load_from_memory(&output.stdout) {
        Ok(img) => {
            let log = String::from_utf8_lossy(&output.stderr);
            let dims = log
                .lines()
                .filter(|line| line.contains("Parsed_showinfo"))
                .find_map(|line| {
                    let size = line
                        .split_whitespace()
                        .find_map(|token| token.strip_prefix("s:"))?;
                    let (width, height) = size.split_once('x')?;
                    Some((width.parse::<u32>().ok()?, height.parse::<u32>().ok()?))
                })
                .ok_or_else(|| AppError::Decode("ffmpeg omitted source dimensions".into()))?;
            Ok(Some((img, dims)))
        }
        Err(e) => Err(AppError::Decode(format!("ffmpeg frame decode: {e}"))),
    }
}

fn extract_poster_ffmpeg(path: &Path, max_long_edge: u32) -> AppResult<(DynamicImage, (u32, u32))> {
    for seek in SEEK_SECONDS {
        if let Some(img) = frame_at(path, seek, max_long_edge)? {
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
    extract_poster_ffmpeg(&path, max_long_edge)
}

#[cfg(test)]
mod media_regressions {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn media_regression_child_has_a_deadline_and_output_limit() {
        let start = std::time::Instant::now();
        let mut sleep = Command::new("powershell.exe");
        configure(&mut sleep);
        sleep.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 10",
        ]);
        assert!(run_bounded(&mut sleep, 1024, std::time::Duration::from_millis(100)).is_err());
        assert!(start.elapsed() < std::time::Duration::from_secs(3));
        let mut output = Command::new("powershell.exe");
        configure(&mut output);
        output.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::Write(('x' * 4096))",
        ]);
        assert!(run_bounded(&mut output, 1024, std::time::Duration::from_secs(5)).is_err());
    }

    #[test]
    fn media_regression_video_poster_obeys_size_cap() {
        if !ffmpeg_info().present {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.mp4");
        let status = ffmpeg()
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=1280x720:d=2",
                "-c:v",
                "mpeg4",
            ])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let store = crate::store::LocalFsStore::new(dir.path());
        let (image, dims) = extract_poster(&store, "clip.mp4", 384).unwrap();
        assert!(
            image.width().max(image.height()) <= 384,
            "ffmpeg must not send a full-resolution PNG to the process"
        );
        assert_eq!(
            dims,
            (1280, 720),
            "The cap must not replace source dimensions"
        );
    }
}
