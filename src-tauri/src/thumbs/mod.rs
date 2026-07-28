use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use image::DynamicImage;
use rusqlite::{params, Connection, OptionalExtension};

use crate::db::Db;
use crate::decode;
use crate::error::{AppError, AppResult};
use crate::store::{read_all, ProjectStore};

pub const THUMB_LONG_EDGE: u32 = 384;

/// Loupe preview long edge, and the only artifact size the user can choose.
///
/// It matters beyond sharpness: `decode_scaled` only engages the IDCT fast path
/// when the source long edge is at least twice the target (`decode/jpeg.rs`), so
/// 2560 makes a 4000px JPEG take a full decode while 1600 takes the 1/2 path —
/// roughly four times fewer decoded pixels, and a proportionally smaller peak per
/// in-flight worker. That is the trade the setting exposes.
///
/// Runtime state rather than a `const` because of that, but the default is
/// unchanged on every platform: nobody gets worse previews without asking.
pub const PREVIEW_LONG_EDGE_DEFAULT: u32 = 2560;

/// Preview sizes offered in Settings. Anything else is rejected, so a stale or
/// hand-edited localStorage value can never put the cache in a state the ingest
/// will not converge on.
pub const PREVIEW_LONG_EDGE_CHOICES: [u32; 3] = [1600, 2560, 3840];

static PREVIEW_LONG_EDGE: AtomicU32 = AtomicU32::new(PREVIEW_LONG_EDGE_DEFAULT);

/// The configured preview long edge.
pub fn preview_long_edge() -> u32 {
    PREVIEW_LONG_EDGE.load(Ordering::Relaxed)
}

/// Apply a preview long edge. Returns the value actually in force, which is the
/// old one when `value` is not an offered choice.
pub fn set_preview_long_edge(value: u32) -> u32 {
    if !PREVIEW_LONG_EDGE_CHOICES.contains(&value) {
        return preview_long_edge();
    }
    PREVIEW_LONG_EDGE.store(value, Ordering::Relaxed);
    value
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThumbKind {
    Thumb = 0,
    Preview = 1,
    /// Full-resolution pixels for focus checks: the unresized embedded
    /// preview for RAWs, the original bytes for plain images.
    Full = 3,
}

/// Everything that decides which cached JPEG a request maps to. Orientation
/// belongs here as much as mtime: rotating a photo changes the rendered pixels
/// without touching the file on disk, so keying the cache on mtime alone would
/// keep serving the old rotation forever.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheVersion {
    pub mtime: i64,
    pub orientation: i64,
}

pub struct ThumbRequest {
    pub file_id: i64,
    pub kind: ThumbKind,
    /// Cache version carried by the request URL (`?v=` and `?o=`), when present.
    /// Lets the worker build the cache path and serve a cache hit without a DB
    /// round-trip. `None` (params absent/unparseable on some platform) falls back
    /// to the authoritative `file_row` lookup — no behavior change, just no
    /// speedup.
    pub known_version: Option<CacheVersion>,
    /// Render the grid thumbnail from the same decode as this request's
    /// artifact. Only meaningful with `kind: Preview`, whose decode is already
    /// at least as large as a thumbnail needs — so the thumbnail costs one more
    /// resize and encode of a buffer already in hand, and no second read of the
    /// source. Used by the pregeneration pass; interactive requests want one
    /// artifact and leave it false.
    pub also_thumb: bool,
    pub respond: Box<dyn FnOnce(AppResult<Vec<u8>>) + Send>,
}

type Responder = Box<dyn FnOnce(AppResult<Vec<u8>>) + Send>;

/// One unit of work — a `(file, kind)` pair — plus everyone waiting on it.
/// Requests are coalesced into these, so the same artifact is never decoded
/// twice concurrently no matter how many callers ask for it.
struct Pending {
    key: (i64, u8),
    kind: ThumbKind,
    also_thumb: bool,
    known_version: Option<CacheVersion>,
    responders: Vec<Responder>,
    /// Set when the pregeneration pass asked for this artifact. The pass only
    /// selects files whose `thumbnails` row is missing or stale, so a disk-cache
    /// hit here means the cache outlived the row -- and the row has to be
    /// written, or every future open re-enqueues the same file forever.
    /// Interactive requests leave it false: they are the hot scroll path and
    /// must not touch the DB on a hit.
    needs_row: bool,
}

/// Deliver one result to every waiter. `AppResult<Vec<u8>>` is not `Clone`, so
/// all but the last waiter get a copy of the bytes (or a rebuilt error) and the
/// last is handed the original.
fn fan_out(result: AppResult<Vec<u8>>, mut responders: Vec<Responder>) {
    let Some(last) = responders.pop() else { return };
    match &result {
        Ok(bytes) => {
            for r in responders {
                r(Ok(bytes.clone()));
            }
        }
        Err(err) => {
            let msg = err.to_string();
            for r in responders {
                r(Err(AppError::Other(msg.clone())));
            }
        }
    }
    last(result);
}

/// Two priority tiers sharing one worker pool, so interactive and background
/// work never fight over separate thread pools (which oversubscribes the CPU and
/// scatters completion order).
struct Queues {
    /// Interactive requests from the `cullant://` protocol (the cells/photo the
    /// user is looking at). LIFO: the most recently requested is what's on screen
    /// right now, so it goes first — a fast scroll never waits on stale cells.
    interactive: VecDeque<Pending>,
    /// Bulk pregeneration (ingest Phase B). FIFO, so thumbnails are produced in
    /// the grid's display order (top first), and only ever served when no
    /// interactive request is waiting.
    background: VecDeque<Pending>,
    /// Keys a worker is decoding right now, holding the responders that arrived
    /// after the decode started. The worker drains this when it finishes, so a
    /// late caller adopts the in-flight result instead of starting a second one.
    in_flight: HashMap<(i64, u8), Vec<Responder>>,
}

struct Queue {
    items: Mutex<Option<Queues>>,
    signal: Condvar,
    /// Ceiling on the interactive queue. A fast scroll can ask for hundreds of
    /// cells; without a bound every stale one is still decoded in full long
    /// after it left the screen, which is how a phone ends up minutes behind
    /// the user. Overflow drops the OLDEST — the least likely to still be
    /// visible — and the frontend re-requests it if it is.
    max_interactive: usize,
}

/// Worker pool that turns thumbnail requests (from the cullant:// protocol AND
/// the ingest pregeneration pass) into cached JPEG bytes. Owns nothing
/// exclusive: DB access goes through the shared writer thread, files are
/// read-only. Interactive requests always preempt background ones.
pub struct ThumbPool {
    queue: Arc<Queue>,
}

impl ThumbPool {
    pub fn start(db: Arc<Db>, store: Arc<dyn ProjectStore>, root: PathBuf) -> ThumbPool {
        // Mirrors the rayon cap in lib.rs, which this pool was never covered by:
        // each in-flight decode holds the source bytes plus a full-size decode
        // plus a resize buffer, so an 8-core phone spawning 7 workers reached
        // ~400 MB of native allocations in a process with no largeHeap.
        let workers = if cfg!(target_os = "android") {
            thread::available_parallelism()
                .map(|n| n.get().min(3))
                .unwrap_or(2)
                .max(2)
        } else {
            thread::available_parallelism()
                .map(|n| (n.get().saturating_sub(1)).max(2))
                .unwrap_or(4)
        };

        let queue = Arc::new(Queue {
            items: Mutex::new(Some(Queues {
                interactive: VecDeque::new(),
                background: VecDeque::new(),
                in_flight: HashMap::new(),
            })),
            signal: Condvar::new(),
            // Deep enough that every worker has work queued behind it plus a
            // screenful of slack, shallow enough that a flick-scroll cannot
            // bank minutes of decoding.
            max_interactive: (workers * 4).max(16),
        });

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

    /// Enqueue an interactive request (served before any background work, LIFO).
    ///
    /// Coalescing happens here: if the same artifact is already being decoded,
    /// or is already queued anywhere, this caller joins it rather than adding a
    /// second decode. A request already sitting in the background tier is
    /// *promoted* — the user is looking at it now.
    pub fn enqueue(&self, request: ThumbRequest) {
        let mut guard = self.queue.items.lock().unwrap();
        let Some(q) = guard.as_mut() else {
            (request.respond)(Err(AppError::Other("thumb pool shut down".into())));
            return;
        };
        let key = (request.file_id, request.kind as u8);

        if let Some(waiters) = q.in_flight.get_mut(&key) {
            waiters.push(request.respond);
            return;
        }
        if let Some(i) = q.interactive.iter().position(|p| p.key == key) {
            // Re-requested, so it is the freshest thing on screen: move it to
            // the front of the line (the LIFO end) instead of leaving it where
            // it was.
            let mut pending = q.interactive.remove(i).expect("index from position");
            pending.responders.push(request.respond);
            q.interactive.push_back(pending);
            return;
        }
        let pending = match q.background.iter().position(|p| p.key == key) {
            Some(i) => {
                let mut pending = q.background.remove(i).expect("index from position");
                pending.responders.push(request.respond);
                pending
            }
            None => Pending {
                key,
                kind: request.kind,
                also_thumb: request.also_thumb,
                known_version: request.known_version,
                responders: vec![request.respond],
                needs_row: false,
            },
        };
        q.interactive.push_back(pending);

        if q.interactive.len() > self.queue.max_interactive {
            if let Some(dropped) = q.interactive.pop_front() {
                fan_out(
                    Err(AppError::Other("thumbnail request superseded".into())),
                    dropped.responders,
                );
            }
        }
        self.queue.signal.notify_one();
    }

    /// Enqueue a background pregeneration request (served only when no
    /// interactive request is waiting, FIFO / in submission order).
    pub fn enqueue_background(&self, request: ThumbRequest) {
        self.enqueue_background_batch(vec![request]);
    }

    /// Submit a whole pregeneration tier at once: one lock acquisition and one
    /// `notify_all`, instead of both per request for the entire library.
    ///
    /// Background work is exempt from `max_interactive` — it is already bounded
    /// by the size of the pass — but not from coalescing, so a file the user has
    /// already looked at is not decoded a second time here.
    pub fn enqueue_background_batch(&self, requests: Vec<ThumbRequest>) {
        let mut guard = self.queue.items.lock().unwrap();
        let Some(q) = guard.as_mut() else {
            for request in requests {
                (request.respond)(Err(AppError::Other("thumb pool shut down".into())));
            }
            return;
        };
        for request in requests {
            let key = (request.file_id, request.kind as u8);
            if let Some(waiters) = q.in_flight.get_mut(&key) {
                waiters.push(request.respond);
                continue;
            }
            if let Some(p) = q
                .interactive
                .iter_mut()
                .chain(q.background.iter_mut())
                .find(|p| p.key == key)
            {
                p.responders.push(request.respond);
                p.needs_row = true;
                continue;
            }
            q.background.push_back(Pending {
                key,
                kind: request.kind,
                also_thumb: request.also_thumb,
                known_version: request.known_version,
                responders: vec![request.respond],
                needs_row: true,
            });
        }
        drop(guard);
        self.queue.signal.notify_all();
    }

    /// Stop accepting work and unblock all workers (they exit). Draining calls
    /// every pending request's `respond` with an error, so a caller blocked on a
    /// completion channel (ingest Phase B) is always released.
    pub fn shutdown(&self) {
        let mut guard = self.queue.items.lock().unwrap();
        if let Some(dropped) = guard.take() {
            drop(guard);
            for pending in dropped.interactive.into_iter().chain(dropped.background) {
                fan_out(
                    Err(AppError::Other("thumb pool shut down".into())),
                    pending.responders,
                );
            }
            // Callers that joined a decode already in progress are released
            // here too; the worker itself finds its key gone and drops its own.
            for (_, waiters) in dropped.in_flight {
                fan_out(Err(AppError::Other("thumb pool shut down".into())), waiters);
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
        let pending = {
            let mut guard = queue.items.lock().unwrap();
            loop {
                match guard.as_mut() {
                    None => return, // pool shut down
                    Some(q) => {
                        // Interactive first (LIFO), then background (FIFO).
                        if let Some(p) = q
                            .interactive
                            .pop_back()
                            .or_else(|| q.background.pop_front())
                        {
                            // Claim the key before releasing the lock, so anyone
                            // asking for it while this decode runs waits on it
                            // instead of starting a second one.
                            q.in_flight.insert(p.key, Vec::new());
                            break p;
                        }
                        guard = queue.signal.wait(guard).unwrap();
                    }
                }
            }
        };

        let result = produce_cached(
            &db,
            store.as_ref(),
            &root,
            Produce {
                file_id: pending.key.0,
                kind: pending.kind,
                known: pending.known_version,
                record_hit: pending.needs_row,
                also_thumb: pending.also_thumb,
            },
        );

        let mut responders = pending.responders;
        {
            let mut guard = queue.items.lock().unwrap();
            if let Some(q) = guard.as_mut() {
                if let Some(late) = q.in_flight.remove(&pending.key) {
                    responders.extend(late);
                }
            }
            // On shutdown the drain already answered the late waiters.
        }
        fan_out(result, responders);
    }
}

pub(crate) fn cache_rel_path(file_id: i64, version: CacheVersion, kind: ThumbKind) -> String {
    let bucket = (file_id % 256) as u8;
    // The preview's target size is a user setting, so it belongs in the path:
    // two sizes then occupy different files instead of one overwriting the
    // other, and a project last opened under a different setting can never be
    // served the wrong one from a cache hit.
    let suffix = match kind {
        ThumbKind::Thumb => "t".to_string(),
        ThumbKind::Preview => format!("p{}", preview_long_edge()),
        ThumbKind::Full => "f".to_string(),
    };
    let CacheVersion { mtime, orientation } = version;
    format!("{bucket:02x}/{file_id}_{mtime}_{orientation}_{suffix}.jpg")
}

/// Pixel dimensions from an encoded image's header, without decoding it.
fn jpeg_dims(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
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
        ThumbKind::Preview => preview_long_edge(),
        // Unresized: u32::MAX long edge means "never scale down".
        // TODO(post-MVP): true demosaic via rawler develop as a fallback for
        // cameras whose embedded preview is smaller than the sensor.
        ThumbKind::Full => u32::MAX,
    }
}

/// The `files` row a thumbnail worker needs. A pure read, run on the pooled
/// reader so a cache-miss lookup during ingest doesn't queue behind writes.
fn file_row(db: &Arc<Db>, file_id: i64) -> AppResult<(String, i64, i64, Option<i64>)> {
    db.call_read(move |conn| {
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

/// The JPEG half of a live RAW+JPEG pair, when `file_id` is the RAW.
///
/// Cullant's RAW fast path already renders the embedded JPEG rather than
/// demosaicing, and a camera's sibling JPEG is that same frame — so for a thumb
/// or a preview the two are visually equivalent, while the JPEG is a fraction of
/// the bytes and needs no container parse. That is worth little where a file can
/// be memory-mapped and decisive where it cannot, which is every file on Android
/// SAF.
///
/// Deliberately a fact about the database, not about the mirror-mode view: a
/// decoupled pair is excluded because the user has said those two files are to
/// be treated as separate photos.
fn paired_jpeg(db: &Arc<Db>, file_id: i64) -> Option<String> {
    db.call_read(move |conn| {
        Ok(conn
            .query_row(
                "SELECT s.rel_path
                 FROM files f
                 JOIN groups g ON g.id = f.group_id
                 JOIN files s ON s.group_id = f.group_id AND s.kind = 1 AND s.status = 0
                 WHERE f.id = ?1 AND f.kind = 0 AND f.status = 0 AND g.decoupled = 0
                 LIMIT 1",
                params![file_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?)
    })
    .ok()
    .flatten()
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
            // Fast path: many RAW containers embed a full-resolution JPEG
            // (Fujifilm .RAF, etc.) that rawler only surfaces via a FULL-res
            // decode. Extracting the bytes and running our IDCT-scaled JPEG
            // decode is far cheaper for a thumbnail/preview. Its header carries
            // the ORIGINAL dimensions (cheap, no decode), independent of the
            // scaled decode below.
            if let Some(jpeg) = decode::raw::embedded_jpeg(source.buf()) {
                match decode::jpeg::decode_scaled(jpeg, min_long_edge, rel_path) {
                    Ok(img) => {
                        let dims = image::ImageReader::new(std::io::Cursor::new(jpeg))
                            .with_guessed_format()
                            .ok()
                            .and_then(|r| r.into_dimensions().ok());
                        return Ok((img, dims));
                    }
                    // A corrupt embedded JPEG is rare; fall back to rawler rather
                    // than tombstoning a file rawler might still decode.
                    Err(e) => tracing::debug!(
                        "{rel_path}: embedded JPEG decode failed, falling back to rawler: {e}"
                    ),
                }
            }
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

/// The DB write a rendered thumbnail produces: the `thumbnails` upsert plus the
/// optional `files.width/height` backfill. Kept separate from rendering so the
/// ingest pass can batch many of these into one transaction (like metadata),
/// while the on-demand path writes one immediately.
pub(crate) struct ThumbRow {
    file_id: i64,
    kind_i: i64,
    cache_rel: String,
    out_w: u32,
    out_h: u32,
    mtime: i64,
    /// ORIGINAL dims to backfill into `files`, when the decode was full-size.
    src_dims: Option<(u32, u32)>,
    /// Perceptual hash of the rendered thumbnail; `None` for every kind but
    /// `Thumb`, so one hash is stored per file rather than three.
    phash: Option<u64>,
    /// Target long edge this artifact was rendered for. Only meaningful for
    /// `Preview`, whose target is a user setting: a row generated for a
    /// different one is stale even though its source never changed.
    long_edge: u32,
}

/// A 64-bit difference hash of `img`: downscale to 9x8 grey, then record
/// whether each pixel is brighter than the one to its right. Two frames of the
/// same burst land within a few bits of each other; an unrelated shot does not.
///
/// Computed from the already-resized, already-oriented thumbnail buffer, so it
/// costs one tiny resize and no extra decode or disk read. Hand-rolled rather
/// than pulling in `image_hasher`, which would risk a second `image` version
/// for what amounts to twenty lines.
fn dhash(img: &DynamicImage) -> u64 {
    // Grey first, then resample: the filter then runs over one channel instead
    // of three, and the hash only ever looks at luminance anyway.
    let small =
        image::imageops::resize(&img.to_luma8(), 9, 8, image::imageops::FilterType::Triangle);
    let mut bits = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            let left = small.get_pixel(x, y).0[0];
            let right = small.get_pixel(x + 1, y).0[0];
            bits = (bits << 1) | u64::from(left > right);
        }
    }
    bits
}

/// Resize `decoded` for `kind`, apply orientation (on the small image — 90°
/// rotations and flips commute with resizing), JPEG-encode, and write to the
/// disk cache. Returns the encoded bytes and the DB-row payload but does NOT
/// touch the database — callers either batch the rows (ingest) or write one
/// immediately (`render_and_store`). Borrows `decoded` so several kinds can be
/// rendered from one decode.
pub(crate) fn render_to_cache(
    root: &Path,
    meta: &SourceMeta,
    decoded: &DynamicImage,
    kind: ThumbKind,
) -> AppResult<(Vec<u8>, ThumbRow)> {
    let SourceMeta {
        file_id,
        mtime,
        orientation,
        src_dims,
    } = *meta;
    let (long_edge, quality) = match kind {
        ThumbKind::Thumb => (THUMB_LONG_EDGE, 80),
        ThumbKind::Preview => (preview_long_edge(), 80),
        ThumbKind::Full => (u32::MAX, 90),
    };
    let resized = resize_long_edge(decoded, long_edge)?;
    let oriented = apply_orientation(resized, orientation);
    let jpeg = encode_jpeg(&oriented, quality)?;

    let cache_rel = cache_rel_path(file_id, CacheVersion { mtime, orientation }, kind);
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

    let row = ThumbRow {
        file_id,
        kind_i: kind as i64,
        cache_rel,
        out_w: oriented.width(),
        out_h: oriented.height(),
        mtime,
        src_dims,
        phash: (kind == ThumbKind::Thumb).then(|| dhash(&oriented)),
        long_edge,
    };
    Ok((jpeg, row))
}

/// Apply one rendered thumbnail's row writes on the DB thread's connection.
fn write_thumb_row(conn: &Connection, row: &ThumbRow) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO thumbnails (file_id, kind, cache_path, width, height, source_mtime, generated_at, failed, long_edge)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8)
         ON CONFLICT(file_id, kind) DO UPDATE SET
           cache_path = excluded.cache_path, width = excluded.width,
           height = excluded.height, source_mtime = excluded.source_mtime,
           generated_at = excluded.generated_at, failed = 0,
           long_edge = excluded.long_edge",
        params![
            row.file_id, row.kind_i, row.cache_rel, row.out_w, row.out_h, row.mtime, now_secs(),
            row.long_edge
        ],
    )?;
    // Original dimensions come for free when the decode was full-size;
    // keep existing values, and never record a scaled decode's dims.
    if let Some((src_w, src_h)) = row.src_dims {
        conn.execute(
            "UPDATE files SET width = COALESCE(width, ?2), height = COALESCE(height, ?3)
             WHERE id = ?1",
            params![row.file_id, src_w, src_h],
        )?;
    }
    // Stored big-endian so the BLOB sorts the same way the number does.
    if let Some(phash) = row.phash {
        conn.execute(
            "INSERT INTO file_analysis (file_id, phash, analyzed_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(file_id) DO UPDATE SET
               phash = excluded.phash, analyzed_at = excluded.analyzed_at",
            params![row.file_id, phash.to_be_bytes().to_vec(), now_secs()],
        )?;
    }
    Ok(())
}

/// Resize/orient/encode/cache one thumbnail AND record its row immediately.
/// Both the on-demand path (`produce_cached`) and the background ingest
/// pass reach this through [`produce_cached`].
pub(crate) fn render_and_store(
    db: &Arc<Db>,
    root: &Path,
    meta: &SourceMeta,
    decoded: &DynamicImage,
    kind: ThumbKind,
) -> AppResult<Vec<u8>> {
    let (jpeg, row) = render_to_cache(root, meta, decoded, kind)?;
    db.call(move |conn| Ok(write_thumb_row(conn, &row)?))?;
    Ok(jpeg)
}

/// What one worker was asked to produce.
#[derive(Clone, Copy)]
pub(crate) struct Produce {
    pub file_id: i64,
    pub kind: ThumbKind,
    /// Cache version from the request URL, when the caller could supply one.
    pub known: Option<CacheVersion>,
    /// Write the `thumbnails` row even on a disk-cache hit. Only the
    /// pregeneration pass sets this; see [`Pending::needs_row`].
    pub record_hit: bool,
    /// Also render the grid thumbnail from this decode.
    pub also_thumb: bool,
}

impl Produce {
    /// A plain interactive request: one artifact, no row on a cache hit.
    #[cfg(test)]
    fn interactive(file_id: i64, kind: ThumbKind, known: Option<CacheVersion>) -> Produce {
        Produce {
            file_id,
            kind,
            known,
            record_hit: false,
            also_thumb: false,
        }
    }
}

/// Return cached bytes for a thumbnail, generating (and caching) on miss.
/// Source media is read through `store`; the JPEG cache lives on the real
/// filesystem under `root/.cullant/thumbs` (private app storage on Android).
/// Also used by the ingest pass's background preview tier — the disk-cache
/// short-circuit and temp+rename writes make it race-safe with the pool.
/// With an optional `known` version supplied by the caller (from the request
/// URL's `?v=` and `?o=`). When present, the disk-cache path is built
/// from it and read FIRST — a cache hit returns with zero DB access, keeping the
/// single writer thread out of the hot scroll path. On a miss (or when `None`),
/// the authoritative `file_row` lookup runs and the full generate path proceeds
/// exactly as before. `known` always mirrors `files.mtime`/`files.orientation`
/// (the frontend sources both from the same columns), so the cache path is
/// identical to the one `render_and_store` wrote — correct even when mtime is 0
/// (SAF providers).
pub(crate) fn produce_cached(
    db: &Arc<Db>,
    store: &dyn ProjectStore,
    root: &Path,
    req: Produce,
) -> AppResult<Vec<u8>> {
    let Produce {
        file_id,
        kind,
        known,
        record_hit,
        also_thumb,
    } = req;
    // Fast path: a trusted version lets us try the cache before touching the DB.
    // Full-of-plain-image has no cache file, so it's left to the miss path
    // below (it needs rel_path anyway); every other kind can hit here.
    if let Some(version) = known {
        let cache_rel = cache_rel_path(file_id, version, kind);
        let cache_abs = root.join(".cullant").join("thumbs").join(&cache_rel);
        if let Ok(bytes) = std::fs::read(&cache_abs) {
            // Only the pregeneration pass gets here with `record_hit`, and only
            // for a file it already established has no usable row. Rebuilding a
            // project's DB while `.cullant/thumbs` survives would otherwise
            // re-enqueue every file on every open, forever, because the cache
            // hit satisfied the request without ever writing the row back.
            if record_hit {
                if let Some((w, h)) = jpeg_dims(&bytes) {
                    let row = ThumbRow {
                        file_id,
                        kind_i: kind as i64,
                        cache_rel,
                        out_w: w,
                        out_h: h,
                        mtime: version.mtime,
                        // Both come from decoding the source, which is exactly
                        // what this path skipped.
                        src_dims: None,
                        phash: None,
                        long_edge: match kind {
                            ThumbKind::Thumb => THUMB_LONG_EDGE,
                            ThumbKind::Preview => preview_long_edge(),
                            ThumbKind::Full => 0,
                        },
                    };
                    db.call(move |conn| Ok(write_thumb_row(conn, &row)?))?;
                }
            }
            return Ok(bytes);
        }
    }

    let (rel_path, file_kind, mtime, orientation) = file_row(db, file_id)?;
    let orientation = orientation.unwrap_or(1);

    // Full view of a plain image: stream the original, no transcode, no cache.
    if kind == ThumbKind::Full && file_kind == 1 {
        return read_all(store, &rel_path);
    }

    let cache_rel = cache_rel_path(file_id, CacheVersion { mtime, orientation }, kind);
    let cache_abs = root.join(".cullant").join("thumbs").join(&cache_rel);
    // Re-check the cache under the authoritative version. Skipped work only when
    // `known` was present AND equal to it AND already missed above; the
    // redundant read is a cheap stat on the cold/miss path.
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

    // A grid thumbnail or a loupe preview of a paired RAW is rendered from the
    // JPEG half instead: same frame, a fraction of the bytes. `Full` is
    // excluded — zooming in is exactly when the RAW's own pixels are the point.
    let (source_rel, source_kind) = match kind {
        ThumbKind::Full => (rel_path.clone(), file_kind),
        _ => match paired_jpeg(db, file_id) {
            Some(sibling) => (sibling, 1),
            None => (rel_path.clone(), file_kind),
        },
    };

    let (decoded, src_dims) =
        match decode_for(store, &source_rel, source_kind, min_long_edge_for(kind)) {
            Ok(d) => d,
            Err(e) => {
                record_decode_failure(db, file_id, mtime, kind)?;
                return Err(e);
            }
        };
    let meta = SourceMeta {
        file_id,
        mtime,
        orientation,
        // Dimensions describe whatever was decoded. Backfilling a RAW's row
        // from its JPEG would record the wrong numbers, so only a decode of the
        // file itself may claim them.
        src_dims: if source_rel == rel_path {
            src_dims
        } else {
            None
        },
    };
    // The preview decode is at least as large as a thumbnail needs, so the grid
    // thumbnail is one more resize and encode of a buffer already in hand --
    // against a whole second read and decode of the source if it were left to
    // its own pass. A failure here is not fatal: the straggler thumbnail pass
    // picks it up.
    if also_thumb && kind == ThumbKind::Preview {
        if let Err(err) = render_and_store(db, root, &meta, &decoded, ThumbKind::Thumb) {
            tracing::debug!("fused thumbnail for {rel_path} failed: {err}");
        }
    }
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
    db.call_read(move |conn| {
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
    use std::borrow::Cow;

    // After resize_long_edge the buffer is already RGB8 in the common path, so
    // borrow it — to_rgb8() would allocate and copy the whole image (~20 MB for
    // a 2560px preview). Only exotic formats hit the owned conversion.
    let rgb: Cow<'_, image::RgbImage> = match img.as_rgb8() {
        Some(rgb) => Cow::Borrowed(rgb),
        None => Cow::Owned(img.to_rgb8()),
    };
    let mut out = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    rgb.as_ref()
        .write_with_encoder(encoder)
        .map_err(|e| AppError::Decode(format!("jpeg encode: {e}")))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The queue's coalescing/bounding logic, exercised directly against
    /// `Queues` so it needs neither worker threads nor real image files.
    mod queue {
        use super::*;
        use std::sync::mpsc;

        /// Stand-in for `ThumbPool::enqueue`, minus the pool plumbing.
        fn enqueue(q: &mut Queues, max: usize, key_id: i64, respond: Responder) {
            let key = (key_id, ThumbKind::Thumb as u8);
            if let Some(waiters) = q.in_flight.get_mut(&key) {
                waiters.push(respond);
                return;
            }
            if let Some(i) = q.interactive.iter().position(|p| p.key == key) {
                let mut pending = q.interactive.remove(i).unwrap();
                pending.responders.push(respond);
                q.interactive.push_back(pending);
                return;
            }
            q.interactive.push_back(Pending {
                key,
                kind: ThumbKind::Thumb,
                also_thumb: false,
                known_version: None,
                responders: vec![respond],
                needs_row: false,
            });
            if q.interactive.len() > max {
                if let Some(dropped) = q.interactive.pop_front() {
                    fan_out(
                        Err(AppError::Other("superseded".into())),
                        dropped.responders,
                    );
                }
            }
        }

        fn empty() -> Queues {
            Queues {
                interactive: VecDeque::new(),
                background: VecDeque::new(),
                in_flight: HashMap::new(),
            }
        }

        #[test]
        fn two_requests_for_one_artifact_share_a_single_decode() {
            let mut q = empty();
            let (tx, rx) = mpsc::channel();
            let tx2 = tx.clone();
            enqueue(
                &mut q,
                16,
                7,
                Box::new(move |r| tx.send(r.is_ok()).unwrap()),
            );
            enqueue(
                &mut q,
                16,
                7,
                Box::new(move |r| tx2.send(r.is_ok()).unwrap()),
            );

            // One unit of work, two waiters on it.
            assert_eq!(q.interactive.len(), 1);
            assert_eq!(q.interactive[0].responders.len(), 2);

            let pending = q.interactive.pop_back().unwrap();
            fan_out(Ok(vec![1, 2, 3]), pending.responders);
            assert!(rx.recv().unwrap());
            assert!(rx.recv().unwrap());
        }

        #[test]
        fn re_requesting_moves_it_to_the_front_of_the_line() {
            let mut q = empty();
            for id in [1, 2, 3] {
                enqueue(&mut q, 16, id, Box::new(|_| {}));
            }
            enqueue(&mut q, 16, 1, Box::new(|_| {}));
            // LIFO pops the back, so the re-requested one is served next.
            assert_eq!(q.interactive.back().unwrap().key.0, 1);
            assert_eq!(q.interactive.len(), 3);
        }

        #[test]
        fn overflow_drops_the_oldest_never_the_newest() {
            let mut q = empty();
            let (tx, rx) = mpsc::channel();
            // Capacity 2. The third request evicts the first.
            enqueue(&mut q, 2, 1, Box::new(move |r| tx.send(r.is_ok()).unwrap()));
            enqueue(&mut q, 2, 2, Box::new(|_| {}));
            enqueue(&mut q, 2, 3, Box::new(|_| {}));

            assert!(!rx.recv().unwrap(), "evicted request must be answered");
            assert_eq!(q.interactive.len(), 2);
            let ids: Vec<i64> = q.interactive.iter().map(|p| p.key.0).collect();
            assert_eq!(ids, vec![2, 3]);
        }

        #[test]
        fn a_late_caller_adopts_the_decode_already_running() {
            let mut q = empty();
            let key = (9, ThumbKind::Thumb as u8);
            // A worker has claimed this key and is decoding it.
            q.in_flight.insert(key, Vec::new());

            let (tx, rx) = mpsc::channel();
            enqueue(
                &mut q,
                16,
                9,
                Box::new(move |r| tx.send(r.is_ok()).unwrap()),
            );

            // No second unit of work was queued.
            assert!(q.interactive.is_empty());
            assert_eq!(q.in_flight[&key].len(), 1);

            // The worker finishes and drains the late waiter.
            let late = q.in_flight.remove(&key).unwrap();
            fan_out(Ok(vec![0]), late);
            assert!(rx.recv().unwrap());
        }
    }

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

        let bytes = produce_cached(
            &db,
            &store,
            root,
            Produce::interactive(id, ThumbKind::Thumb, None),
        )
        .unwrap();
        let thumb = image::load_from_memory(&bytes).unwrap();
        assert_eq!(thumb.width(), 384);
        assert_eq!(thumb.height(), 288);

        // Second call must hit the disk cache (row exists + same bytes).
        let again = produce_cached(
            &db,
            &store,
            root,
            Produce::interactive(id, ThumbKind::Thumb, None),
        )
        .unwrap();
        assert_eq!(bytes, again);
        let rows: i64 = db
            .call(|c| Ok(c.query_row("SELECT COUNT(*) FROM thumbnails", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn known_version_serves_cache_fast_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let img = image::RgbImage::from_fn(800, 600, |x, _| image::Rgb([(x % 255) as u8, 70, 30]));
        img.save(root.join("photo.jpg")).unwrap();

        let db = Arc::new(Db::open(root).unwrap());
        let store = crate::store::LocalFsStore::new(root);
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        let (id, mtime): (i64, i64) = db
            .call(|c| {
                Ok(c.query_row("SELECT id, mtime FROM files", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?)
            })
            .unwrap();
        let version = CacheVersion {
            mtime,
            orientation: 1,
        };

        // Generate + cache the thumb.
        let bytes = produce_cached(
            &db,
            &store,
            root,
            Produce::interactive(id, ThumbKind::Thumb, Some(version)),
        )
        .unwrap();

        // Fast path with the correct version returns the cached bytes.
        let hit = produce_cached(
            &db,
            &store,
            root,
            Produce::interactive(id, ThumbKind::Thumb, Some(version)),
        )
        .unwrap();
        assert_eq!(bytes, hit);

        // A stale mtime misses the fast path and falls back to the authoritative
        // lookup, which regenerates against the real version — still succeeds.
        let stale = CacheVersion {
            mtime: mtime + 999,
            orientation: 1,
        };
        let fallback = produce_cached(
            &db,
            &store,
            root,
            Produce::interactive(id, ThumbKind::Thumb, Some(stale)),
        )
        .unwrap();
        assert_eq!(bytes, fallback);

        // Same file, same mtime, different orientation: the cache must NOT be
        // reused, because the rendered pixels differ. This is the rotate case.
        let rotated = CacheVersion {
            mtime,
            orientation: 6,
        };
        assert_ne!(
            cache_rel_path(id, version, ThumbKind::Thumb),
            cache_rel_path(id, rotated, ThumbKind::Thumb)
        );
    }

    #[test]
    fn dhash_tracks_similarity_not_identity() {
        let base = image::RgbImage::from_fn(200, 150, |x, y| {
            image::Rgb([((x * 255) / 200) as u8, ((y * 255) / 150) as u8, 60])
        });
        let base = DynamicImage::ImageRgb8(base);

        // The next frame of a burst: the same scene nudged slightly.
        let shifted = image::RgbImage::from_fn(200, 150, |x, y| {
            let x = (x + 2).min(199);
            image::Rgb([((x * 255) / 200) as u8, ((y * 255) / 150) as u8, 60])
        });
        let shifted = DynamicImage::ImageRgb8(shifted);

        // A different scene entirely: gradient running the other way.
        let other = image::RgbImage::from_fn(200, 150, |x, y| {
            image::Rgb([(255 - (x * 255) / 200) as u8, 30, ((y * 255) / 150) as u8])
        });
        let other = DynamicImage::ImageRgb8(other);

        let near = (dhash(&base) ^ dhash(&shifted)).count_ones();
        let far = (dhash(&base) ^ dhash(&other)).count_ones();
        assert!(near <= 8, "a nudged frame should stay close, got {near}");
        assert!(
            far > near,
            "an unrelated scene must be further away: near={near} far={far}"
        );
    }

    #[test]
    fn a_thumbnail_records_a_hash_and_a_preview_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let img = image::RgbImage::from_fn(800, 600, |x, y| {
            image::Rgb([(x % 255) as u8, (y % 255) as u8, 10])
        });
        img.save(root.join("photo.jpg")).unwrap();
        let db = Arc::new(Db::open(root).unwrap());
        let store = crate::store::LocalFsStore::new(root);
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        let id: i64 = db
            .call(|c| Ok(c.query_row("SELECT id FROM files", [], |r| r.get(0))?))
            .unwrap();

        produce_cached(
            &db,
            &store,
            root,
            Produce::interactive(id, ThumbKind::Preview, None),
        )
        .unwrap();
        let after_preview: i64 = db
            .call(|c| Ok(c.query_row("SELECT COUNT(*) FROM file_analysis", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(after_preview, 0, "only the thumb kind is hashed");

        produce_cached(
            &db,
            &store,
            root,
            Produce::interactive(id, ThumbKind::Thumb, None),
        )
        .unwrap();
        let blob: Vec<u8> = db
            .call(move |c| {
                Ok(c.query_row(
                    "SELECT phash FROM file_analysis WHERE file_id = ?1",
                    params![id],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(blob.len(), 8, "a 64-bit hash, stored big-endian");
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

        let bytes = produce_cached(
            &db,
            &store,
            root,
            Produce::interactive(id, ThumbKind::Thumb, None),
        )
        .unwrap();
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

        produce_cached(
            &db,
            &store,
            root,
            Produce::interactive(id, ThumbKind::Thumb, None),
        )
        .unwrap();
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
