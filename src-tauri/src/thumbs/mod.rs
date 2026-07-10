use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use image::DynamicImage;
use rusqlite::params;

use crate::db::Db;
use crate::decode;
use crate::error::{AppError, AppResult};
use crate::store::{read_all, ProjectStore};

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
    pub fn start(db: Arc<Db>, store: Arc<dyn ProjectStore>, root: PathBuf) -> ThumbPool {
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
            let store = store.clone();
            let root = root.clone();
            thread::Builder::new()
                .name(format!("thumb-{i}"))
                .spawn(move || worker_loop(queue, db, store, root))
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

fn worker_loop(queue: Arc<Queue>, db: Arc<Db>, store: Arc<dyn ProjectStore>, root: PathBuf) {
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

        let result = produce(&db, store.as_ref(), &root, request.file_id, request.kind);
        (request.respond)(result);
    }
}

pub(crate) fn cache_rel_path(file_id: i64, mtime: i64, kind: ThumbKind) -> String {
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

/// The smallest decoded long edge that can serve a given output kind.
fn min_long_edge_for(kind: ThumbKind) -> u32 {
    match kind {
        ThumbKind::Thumb => THUMB_LONG_EDGE,
        ThumbKind::Preview => PREVIEW_LONG_EDGE,
        // Unresized: u32::MAX long edge means "never scale down".
        // TODO(post-MVP): true demosaic via rawler develop as a fallback for
        // cameras whose embedded preview is smaller than the sensor.
        ThumbKind::Full => u32::MAX,
    }
}

/// The `files` row a thumbnail worker needs.
fn file_row(db: &Arc<Db>, file_id: i64) -> AppResult<(String, i64, i64, Option<i64>)> {
    db.call(move |conn| {
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
    })
}

/// Decode a source image with a long edge of at least `min_long_edge`,
/// reading as little as possible (mmap + scaled/adequate-size decodes).
/// Also returns the ORIGINAL pixel dimensions when they are reliably known
/// (JPEG/PNG header, or a full-size embedded RAW image) — scaled decodes must
/// never be recorded as the file's real dimensions.
pub(crate) fn decode_for(
    store: &dyn ProjectStore,
    rel_path: &str,
    file_kind: i64,
    min_long_edge: u32,
) -> AppResult<(DynamicImage, Option<(u32, u32)>)> {
    match file_kind {
        0 => {
            let source = decode::open_source(store, rel_path)?;
            let raw = decode::raw::embedded_preview_scaled(&source, min_long_edge, rel_path)?;
            let dims = raw.is_full.then(|| (raw.image.width(), raw.image.height()));
            Ok((raw.image, dims))
        }
        1 => {
            let source = decode::open_source(store, rel_path)?;
            // Header-only original dimensions (cheap, no decode).
            let dims = image::ImageReader::new(std::io::Cursor::new(source.buf()))
                .with_guessed_format()
                .ok()
                .and_then(|r| r.into_dimensions().ok());
            let img = decode::jpeg::decode_scaled(source.buf(), min_long_edge, rel_path)?;
            Ok((img, dims))
        }
        2 => {
            // Videos: extract a poster frame via ffmpeg (needs a real local
            // file). The frame is the video's display resolution, so its dims
            // are trustworthy. Callers guard on availability first (see
            // `produce`) so a missing ffmpeg never reaches here as a "failure".
            let Some(path) = store.local_path(rel_path) else {
                return Err(AppError::Decode(format!(
                    "{rel_path}: video thumbnails require a local file"
                )));
            };
            let img = decode::video::extract_poster(&path)?;
            let dims = Some((img.width(), img.height()));
            Ok((img, dims))
        }
        _ => Err(AppError::Decode(format!(
            "no thumbnail source for kind {file_kind}"
        ))),
    }
}

/// Whether a video poster can be produced right now: ffmpeg on PATH and a real
/// local file to feed it. When false, video thumbnailing is skipped WITHOUT a
/// tombstone, so installing ffmpeg later (which doesn't change any file's
/// mtime) still gets a chance on the next scan.
pub(crate) fn video_poster_possible(store: &dyn ProjectStore, rel_path: &str) -> bool {
    decode::video::is_available() && store.local_path(rel_path).is_some()
}

/// Identity + trusted facts about a decoded source, shared by every artifact
/// rendered from the same decode.
pub(crate) struct SourceMeta {
    pub file_id: i64,
    pub mtime: i64,
    pub orientation: i64,
    /// ORIGINAL image dimensions when reliably known (JPEG header or full-size
    /// embedded RAW image); only then are `files.width/height` backfilled.
    pub src_dims: Option<(u32, u32)>,
}

/// Resize `decoded` for `kind`, apply orientation (on the small image — 90°
/// rotations and flips commute with resizing), JPEG-encode, write to the disk
/// cache and record the row. Borrows `decoded` so several kinds can be
/// rendered from one decode.
pub(crate) fn render_and_store(
    db: &Arc<Db>,
    root: &Path,
    meta: &SourceMeta,
    decoded: &DynamicImage,
    kind: ThumbKind,
) -> AppResult<Vec<u8>> {
    let SourceMeta {
        file_id,
        mtime,
        orientation,
        src_dims,
    } = *meta;
    let (long_edge, quality) = match kind {
        ThumbKind::Thumb => (THUMB_LONG_EDGE, 80),
        ThumbKind::Preview => (PREVIEW_LONG_EDGE, 80),
        ThumbKind::Full => (u32::MAX, 90),
    };
    let resized = resize_long_edge(decoded, long_edge)?;
    let oriented = apply_orientation(resized, orientation);
    let jpeg = encode_jpeg(&oriented, quality)?;

    let cache_rel = cache_rel_path(file_id, mtime, kind);
    let cache_abs = root.join(".cullant").join("thumbs").join(&cache_rel);
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
    let (out_w, out_h) = (oriented.width(), oriented.height());
    db.call(move |conn| {
        conn.execute(
            "INSERT INTO thumbnails (file_id, kind, cache_path, width, height, source_mtime, generated_at, failed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)
             ON CONFLICT(file_id, kind) DO UPDATE SET
               cache_path = excluded.cache_path, width = excluded.width,
               height = excluded.height, source_mtime = excluded.source_mtime,
               generated_at = excluded.generated_at, failed = 0",
            params![file_id, kind_i, cache_rel, out_w, out_h, mtime, now_secs()],
        )?;
        // Original dimensions come for free when the decode was full-size;
        // keep existing values, and never record a scaled decode's dims.
        if let Some((src_w, src_h)) = src_dims {
            conn.execute(
                "UPDATE files SET width = COALESCE(width, ?2), height = COALESCE(height, ?3)
                 WHERE id = ?1",
                params![file_id, src_w, src_h],
            )?;
        }
        Ok(())
    })?;

    Ok(jpeg)
}

/// Return cached bytes for a thumbnail, generating (and caching) on miss.
/// Source media is read through `store`; the JPEG cache lives on the real
/// filesystem under `root/.cullant/thumbs` (private app storage on Android).
/// Also used by the ingest pass's background preview tier — the disk-cache
/// short-circuit and temp+rename writes make it race-safe with the pool.
pub(crate) fn produce(
    db: &Arc<Db>,
    store: &dyn ProjectStore,
    root: &Path,
    file_id: i64,
    kind: ThumbKind,
) -> AppResult<Vec<u8>> {
    let (rel_path, file_kind, mtime, orientation) = file_row(db, file_id)?;

    // Full view of a plain image: stream the original, no transcode, no cache.
    if kind == ThumbKind::Full && file_kind == 1 {
        return read_all(store, &rel_path);
    }

    let cache_rel = cache_rel_path(file_id, mtime, kind);
    let cache_abs = root.join(".cullant").join("thumbs").join(&cache_rel);
    if let Ok(bytes) = std::fs::read(&cache_abs) {
        return Ok(bytes);
    }

    // A previous attempt at this exact mtime already failed — don't grind on an
    // undecodable file every time its cell scrolls back into view.
    if is_tombstoned(db, file_id, kind, mtime)? {
        return Err(AppError::Decode(format!(
            "{rel_path}: previously undecodable"
        )));
    }

    // Video posters need ffmpeg + a local file. When unavailable, fail without
    // tombstoning so a later ffmpeg install still gets a chance next scan.
    if file_kind == 2 && !video_poster_possible(store, &rel_path) {
        return Err(AppError::Decode(format!(
            "{rel_path}: video thumbnails unavailable (ffmpeg missing?)"
        )));
    }

    let (decoded, src_dims) = match decode_for(store, &rel_path, file_kind, min_long_edge_for(kind))
    {
        Ok(d) => d,
        Err(e) => {
            record_decode_failure(db, file_id, mtime, kind)?;
            return Err(e);
        }
    };
    let meta = SourceMeta {
        file_id,
        mtime,
        orientation: orientation.unwrap_or(1),
        src_dims,
    };
    render_and_store(db, root, &meta, &decoded, kind)
}

/// Record that a source could not be decoded for `kind` at `mtime`, so the
/// ingest pass and the on-demand worker stop retrying it until the file
/// changes. Stored as a `thumbnails` row with `failed = 1` and an empty
/// cache_path.
pub(crate) fn record_decode_failure(
    db: &Arc<Db>,
    file_id: i64,
    mtime: i64,
    kind: ThumbKind,
) -> AppResult<()> {
    let kind_i = kind as i64;
    db.call(move |conn| {
        conn.execute(
            "INSERT INTO thumbnails (file_id, kind, cache_path, width, height, source_mtime, generated_at, failed)
             VALUES (?1, ?2, '', NULL, NULL, ?3, ?4, 1)
             ON CONFLICT(file_id, kind) DO UPDATE SET
               cache_path = '', width = NULL, height = NULL,
               source_mtime = ?3, generated_at = ?4, failed = 1",
            params![file_id, kind_i, mtime, now_secs()],
        )?;
        Ok(())
    })
}

/// Whether a decode-failure tombstone exists for this (file, kind) at `mtime`.
fn is_tombstoned(db: &Arc<Db>, file_id: i64, kind: ThumbKind, mtime: i64) -> AppResult<bool> {
    let kind_i = kind as i64;
    db.call(move |conn| {
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM thumbnails
             WHERE file_id = ?1 AND kind = ?2 AND failed = 1 AND source_mtime = ?3",
            params![file_id, kind_i, mtime],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    })
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

/// Downscale with SIMD (fast_image_resize) so the long edge is `long_edge`.
/// Borrows the source so multiple targets can be rendered from one decode;
/// only clones when no resize is needed (i.e. the image is already small, or
/// the Full kind's "never scale down").
fn resize_long_edge(img: &DynamicImage, long_edge: u32) -> AppResult<DynamicImage> {
    use fast_image_resize::{ResizeAlg, ResizeOptions, Resizer};
    use std::borrow::Cow;

    let (dst_w, dst_h) = scaled_dims(img.width(), img.height(), long_edge);
    if (dst_w, dst_h) == (img.width(), img.height()) {
        return Ok(img.clone());
    }

    // fast_image_resize works on 8/16-bit buffers; normalize exotic formats.
    let src: Cow<'_, DynamicImage> = match img {
        DynamicImage::ImageRgb8(_) | DynamicImage::ImageRgba8(_) | DynamicImage::ImageLuma8(_) => {
            Cow::Borrowed(img)
        }
        other => Cow::Owned(DynamicImage::ImageRgb8(other.to_rgb8())),
    };

    let mut dst = DynamicImage::new(dst_w, dst_h, src.color());
    let mut resizer = Resizer::new();
    resizer
        .resize(
            src.as_ref(),
            &mut dst,
            &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(
                fast_image_resize::FilterType::Bilinear,
            )),
        )
        .map_err(|e| AppError::Decode(format!("resize: {e}")))?;
    Ok(dst)
}

fn encode_jpeg(img: &DynamicImage, quality: u8) -> AppResult<Vec<u8>> {
    let rgb = img.to_rgb8();
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
        let store = crate::store::LocalFsStore::new(root);
        let done = crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        assert_eq!(done.file_count, 1);

        let id: i64 = db
            .call(|c| Ok(c.query_row("SELECT id FROM files", [], |r| r.get(0))?))
            .unwrap();

        let bytes = produce(&db, &store, root, id, ThumbKind::Thumb).unwrap();
        let thumb = image::load_from_memory(&bytes).unwrap();
        assert_eq!(thumb.width(), 384);
        assert_eq!(thumb.height(), 288);

        // Second call must hit the disk cache (row exists + same bytes).
        let again = produce(&db, &store, root, id, ThumbKind::Thumb).unwrap();
        assert_eq!(bytes, again);
        let rows: i64 = db
            .call(|c| Ok(c.query_row("SELECT COUNT(*) FROM thumbnails", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn orientation_is_applied_after_resize() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Horizontal gradient: dark at x=0, bright at the right edge.
        let img =
            image::RgbImage::from_fn(800, 600, |x, _| image::Rgb([((x * 255) / 800) as u8; 3]));
        img.save(root.join("photo.jpg")).unwrap();

        let db = Arc::new(Db::open(root).unwrap());
        let store = crate::store::LocalFsStore::new(root);
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        let id: i64 = db
            .call(|c| Ok(c.query_row("SELECT id FROM files", [], |r| r.get(0))?))
            .unwrap();
        // Orientation 6 = rotate 90° CW.
        db.call(move |c| {
            c.execute(
                "UPDATE files SET orientation = 6 WHERE id = ?1",
                params![id],
            )?;
            Ok(())
        })
        .unwrap();

        let bytes = produce(&db, &store, root, id, ThumbKind::Thumb).unwrap();
        let thumb = image::load_from_memory(&bytes).unwrap().to_rgb8();
        // Rotated: landscape 800x600 -> portrait thumb 288x384.
        assert_eq!((thumb.width(), thumb.height()), (288, 384));
        // After rotate90 the original bright right edge is at the BOTTOM.
        let top = thumb.get_pixel(thumb.width() / 2, 4).0[0] as i32;
        let bottom = thumb.get_pixel(thumb.width() / 2, thumb.height() - 5).0[0] as i32;
        assert!(
            bottom > top + 100,
            "expected bright bottom after rotation, top={top} bottom={bottom}"
        );
    }

    #[test]
    fn backfills_original_dims_even_when_decode_was_scaled() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Large enough that the scaled JPEG decode path engages for a 384 thumb.
        let img = image::RgbImage::from_fn(2400, 1600, |x, y| {
            image::Rgb([(x % 251) as u8, (y % 241) as u8, 90])
        });
        img.save(root.join("big.jpg")).unwrap();

        let db = Arc::new(Db::open(root).unwrap());
        let store = crate::store::LocalFsStore::new(root);
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        let id: i64 = db
            .call(|c| Ok(c.query_row("SELECT id FROM files", [], |r| r.get(0))?))
            .unwrap();

        produce(&db, &store, root, id, ThumbKind::Thumb).unwrap();
        let (w, h): (i64, i64) = db
            .call(move |c| {
                Ok(c.query_row(
                    "SELECT width, height FROM files WHERE id = ?1",
                    params![id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        // Original dims, not the reduced decode's.
        assert_eq!((w, h), (2400, 1600));
    }

    #[test]
    fn no_dims_backfill_without_trustworthy_source() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let img = image::RgbImage::from_fn(640, 480, |x, _| image::Rgb([(x % 255) as u8, 10, 20]));
        img.save(root.join("photo.jpg")).unwrap();

        let db = Arc::new(Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        let id: i64 = db
            .call(|c| Ok(c.query_row("SELECT id FROM files", [], |r| r.get(0))?))
            .unwrap();
        let mtime: i64 = db
            .call(move |c| {
                Ok(
                    c.query_row("SELECT mtime FROM files WHERE id = ?1", params![id], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();

        // Render with src_dims: None (e.g. a scaled RAW embedded preview).
        let decoded = image::DynamicImage::ImageRgb8(img);
        let meta = SourceMeta {
            file_id: id,
            mtime,
            orientation: 1,
            src_dims: None,
        };
        render_and_store(&db, root, &meta, &decoded, ThumbKind::Thumb).unwrap();
        let (w, h): (Option<i64>, Option<i64>) = db
            .call(move |c| {
                Ok(c.query_row(
                    "SELECT width, height FROM files WHERE id = ?1",
                    params![id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!((w, h), (None, None));
    }
}
