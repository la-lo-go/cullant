use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use image::DynamicImage;
use rusqlite::params;

use crate::db::Db;
use crate::decode;
use crate::error::{AppError, AppResult};

pub const THUMB_LONG_EDGE: u32 = 384;
pub const PREVIEW_LONG_EDGE: u32 = 2560;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThumbKind {
    Thumb = 0,
    Preview = 1,
    /// Full-resolution pixels for focus checks: the unresized embedded
    /// preview for RAWs, the original bytes for plain images.
    Full = 3,
}

pub struct ThumbRequest {
    pub file_id: i64,
    pub kind: ThumbKind,
    pub respond: Box<dyn FnOnce(AppResult<Vec<u8>>) + Send>,
}

struct Queue {
    // LIFO: the most recently requested thumb is what the user is looking at
    // right now, so it goes first.
    items: Mutex<Option<Vec<ThumbRequest>>>,
    signal: Condvar,
}

/// Worker pool that turns thumbnail requests (from the cullant:// protocol)
/// into cached JPEG bytes. Owns nothing exclusive: DB access goes through the
/// shared writer thread, files are read-only.
pub struct ThumbPool {
    queue: Arc<Queue>,
}

impl ThumbPool {
    pub fn start(db: Arc<Db>, root: PathBuf) -> ThumbPool {
        let queue = Arc::new(Queue {
            items: Mutex::new(Some(Vec::new())),
            signal: Condvar::new(),
        });

        let workers = thread::available_parallelism()
            .map(|n| (n.get().saturating_sub(1)).max(2))
            .unwrap_or(4);
        for i in 0..workers {
            let queue = queue.clone();
            let db = db.clone();
            let root = root.clone();
            thread::Builder::new()
                .name(format!("thumb-{i}"))
                .spawn(move || worker_loop(queue, db, root))
                .expect("failed to spawn thumb worker");
        }

        ThumbPool { queue }
    }

    pub fn enqueue(&self, request: ThumbRequest) {
        let mut guard = self.queue.items.lock().unwrap();
        if let Some(items) = guard.as_mut() {
            items.push(request);
            self.queue.signal.notify_one();
        } else {
            (request.respond)(Err(AppError::Other("thumb pool shut down".into())));
        }
    }

    /// Stop accepting work and unblock all workers (they exit).
    pub fn shutdown(&self) {
        let mut guard = self.queue.items.lock().unwrap();
        if let Some(dropped) = guard.take() {
            drop(guard);
            for req in dropped {
                (req.respond)(Err(AppError::Other("thumb pool shut down".into())));
            }
            self.queue.signal.notify_all();
        }
    }
}

impl Drop for ThumbPool {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn worker_loop(queue: Arc<Queue>, db: Arc<Db>, root: PathBuf) {
    loop {
        let request = {
            let mut guard = queue.items.lock().unwrap();
            loop {
                match guard.as_mut() {
                    None => return, // pool shut down
                    Some(items) => {
                        if let Some(req) = items.pop() {
                            break req;
                        }
                        guard = queue.signal.wait(guard).unwrap();
                    }
                }
            }
        };

        let result = produce(&db, &root, request.file_id, request.kind);
        (request.respond)(result);
    }
}

fn cache_rel_path(file_id: i64, mtime: i64, kind: ThumbKind) -> String {
    let bucket = (file_id % 256) as u8;
    let suffix = match kind {
        ThumbKind::Thumb => "t",
        ThumbKind::Preview => "p",
        ThumbKind::Full => "f",
    };
    format!("{bucket:02x}/{file_id}_{mtime}_{suffix}.jpg")
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Return cached bytes for a thumbnail, generating (and caching) on miss.
fn produce(db: &Arc<Db>, root: &Path, file_id: i64, kind: ThumbKind) -> AppResult<Vec<u8>> {
    let (rel_path, file_kind, mtime, orientation) = db.call(move |conn| {
        Ok(conn.query_row(
            "SELECT rel_path, kind, mtime, orientation FROM files WHERE id = ?1 AND status = 0",
            params![file_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                ))
            },
        )?)
    })?;

    let source_path = root.join(&rel_path);

    // Full view of a plain image: stream the original, no transcode, no cache.
    if kind == ThumbKind::Full && file_kind == 1 {
        return Ok(std::fs::read(&source_path)?);
    }

    let cache_rel = cache_rel_path(file_id, mtime, kind);
    let cache_abs = root.join(".cullant").join("thumbs").join(&cache_rel);
    if let Ok(bytes) = std::fs::read(&cache_abs) {
        return Ok(bytes);
    }
    let decoded: DynamicImage = match file_kind {
        0 => decode::raw::embedded_preview(&source_path)?,
        1 => image::open(&source_path)
            .map_err(|e| AppError::Decode(format!("{}: {e}", source_path.display())))?,
        _ => {
            return Err(AppError::Decode(format!(
                "no thumbnail source for kind {file_kind}"
            )))
        }
    };

    let (src_w, src_h) = (decoded.width(), decoded.height());
    let oriented = apply_orientation(decoded, orientation.unwrap_or(1));
    let (long_edge, quality) = match kind {
        ThumbKind::Thumb => (THUMB_LONG_EDGE, 80),
        ThumbKind::Preview => (PREVIEW_LONG_EDGE, 80),
        // Unresized: u32::MAX long edge means "never scale down".
        // TODO(post-MVP): true demosaic via rawler develop as a fallback for
        // cameras whose embedded preview is smaller than the sensor.
        ThumbKind::Full => (u32::MAX, 90),
    };
    let jpeg = encode_scaled_jpeg(&oriented, long_edge, quality)?;

    if let Some(parent) = cache_abs.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Write via temp file + rename so a crashed worker never leaves a
    // truncated JPEG in the cache.
    let tmp = cache_abs.with_extension("tmp");
    std::fs::write(&tmp, &jpeg)?;
    std::fs::rename(&tmp, &cache_abs).or_else(|_| {
        // Another worker may have won the race; that's fine.
        std::fs::remove_file(&tmp)
    })?;

    let kind_i = kind as i64;
    let (out_w, out_h) = scaled_dims(oriented.width(), oriented.height(), long_edge);
    db.call(move |conn| {
        conn.execute(
            "INSERT INTO thumbnails (file_id, kind, cache_path, width, height, source_mtime, generated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(file_id, kind) DO UPDATE SET
               cache_path = excluded.cache_path, width = excluded.width,
               height = excluded.height, source_mtime = excluded.source_mtime,
               generated_at = excluded.generated_at",
            params![file_id, kind_i, cache_rel, out_w, out_h, mtime, now_secs()],
        )?;
        // Original dimensions come for free here; keep them if unknown.
        conn.execute(
            "UPDATE files SET width = COALESCE(width, ?2), height = COALESCE(height, ?3)
             WHERE id = ?1",
            params![file_id, src_w, src_h],
        )?;
        Ok(())
    })?;

    Ok(jpeg)
}

fn scaled_dims(w: u32, h: u32, long_edge: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= long_edge {
        return (w, h);
    }
    let scale = long_edge as f64 / long as f64;
    (
        ((w as f64 * scale).round() as u32).max(1),
        ((h as f64 * scale).round() as u32).max(1),
    )
}

fn apply_orientation(img: DynamicImage, orientation: i64) -> DynamicImage {
    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

/// Resize with SIMD (fast_image_resize) and encode as JPEG.
fn encode_scaled_jpeg(img: &DynamicImage, long_edge: u32, quality: u8) -> AppResult<Vec<u8>> {
    use fast_image_resize::{ResizeAlg, ResizeOptions, Resizer};

    let (dst_w, dst_h) = scaled_dims(img.width(), img.height(), long_edge);

    // fast_image_resize works on 8/16-bit buffers; normalize exotic formats.
    let src: DynamicImage = match img {
        DynamicImage::ImageRgb8(_) | DynamicImage::ImageRgba8(_) | DynamicImage::ImageLuma8(_) => {
            img.clone()
        }
        other => DynamicImage::ImageRgb8(other.to_rgb8()),
    };

    let resized = if (dst_w, dst_h) == (src.width(), src.height()) {
        src
    } else {
        let mut dst = DynamicImage::new(dst_w, dst_h, src.color());
        let mut resizer = Resizer::new();
        resizer
            .resize(
                &src,
                &mut dst,
                &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(
                    fast_image_resize::FilterType::Bilinear,
                )),
            )
            .map_err(|e| AppError::Decode(format!("resize: {e}")))?;
        dst
    };

    let rgb = resized.to_rgb8();
    let mut out = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    rgb.write_with_encoder(encoder)
        .map_err(|e| AppError::Decode(format!("jpeg encode: {e}")))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_down_long_edge_only() {
        assert_eq!(scaled_dims(4000, 3000, 384), (384, 288));
        assert_eq!(scaled_dims(3000, 4000, 384), (288, 384));
        assert_eq!(scaled_dims(200, 100, 384), (200, 100));
    }

    #[test]
    fn generates_and_caches_thumbnail() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // A real JPEG on disk.
        let img = image::RgbImage::from_fn(800, 600, |x, _| image::Rgb([(x % 255) as u8, 80, 120]));
        img.save(root.join("photo.jpg")).unwrap();

        let db = Arc::new(Db::open(root).unwrap());
        let done = crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        assert_eq!(done.file_count, 1);

        let id: i64 = db
            .call(|c| Ok(c.query_row("SELECT id FROM files", [], |r| r.get(0))?))
            .unwrap();

        let bytes = produce(&db, root, id, ThumbKind::Thumb).unwrap();
        let thumb = image::load_from_memory(&bytes).unwrap();
        assert_eq!(thumb.width(), 384);
        assert_eq!(thumb.height(), 288);

        // Second call must hit the disk cache (row exists + same bytes).
        let again = produce(&db, root, id, ThumbKind::Thumb).unwrap();
        assert_eq!(bytes, again);
        let rows: i64 = db
            .call(|c| Ok(c.query_row("SELECT COUNT(*) FROM thumbnails", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(rows, 1);
    }
}
