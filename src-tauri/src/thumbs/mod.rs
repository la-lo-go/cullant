pub mod memcache;

use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use image::DynamicImage;
use rusqlite::{params, Connection, OptionalExtension};

use crate::db::Db;
use crate::decode;
use crate::error::{AppError, AppResult};
use crate::store::ProjectStore;

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
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
    /// See [`Produce::cache_checked`].
    pub cache_checked: bool,
    pub respond: Box<dyn FnOnce(AppResult<Vec<u8>>) + Send>,
}

type Responder = Box<dyn FnOnce(AppResult<Vec<u8>>) + Send>;
type WorkKey = (i64, u8, Option<CacheVersion>, u64, u32);

/// One unit of work — a `(file, kind)` pair — plus everyone waiting on it.
/// Requests are coalesced into these, so the same artifact is never decoded
/// twice concurrently no matter how many callers ask for it.
struct Pending {
    key: WorkKey,
    kind: ThumbKind,
    also_thumb: bool,
    cache_checked: bool,
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
    in_flight: HashMap<WorkKey, Vec<Responder>>,
}

struct Queue {
    cancelled: Arc<AtomicBool>,
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
    db: Mutex<Option<Arc<Db>>>,
    queue: Arc<Queue>,
    /// Kept so shutdown can wait for the workers rather than merely ask them to
    /// stop — see `shutdown`. Taken on the first shutdown, so a later drop joins
    /// nothing and cannot block.
    handles: Mutex<Vec<thread::JoinHandle<()>>>,
}

impl ThumbPool {
    pub fn start(db: Arc<Db>, store: Arc<dyn ProjectStore>, root: PathBuf) -> ThumbPool {
        // Mirrors the rayon cap in lib.rs, which this pool was never covered by:
        // each in-flight decode holds the source bytes plus a full-size decode
        // plus a resize buffer, so an 8-core phone spawning 7 workers reached
        // ~400 MB of native allocations in a process with no largeHeap.
        let workers = if cfg!(any(target_os = "android", target_os = "ios")) {
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
            cancelled: Arc::new(AtomicBool::new(false)),
            items: Mutex::new(Some(Queues {
                interactive: VecDeque::new(),
                background: VecDeque::new(),
                in_flight: HashMap::new(),
            })),
            signal: Condvar::new(),
            // The floor has to exceed a screenful, or eviction starts dropping
            // cells the user is looking at right now: a dense grid on a large
            // display can ask for well over a hundred at once, and they all
            // arrive before any of them completes. A flick-scroll generates
            // thousands, so this still bounds the thing it is meant to bound.
            max_interactive: (workers * 4).max(128),
        });

        let mut handles = Vec::with_capacity(workers);
        for i in 0..workers {
            let queue = queue.clone();
            let db = db.clone();
            let store = store.clone();
            let root = root.clone();
            handles.push(
                thread::Builder::new()
                    .name(format!("thumb-{i}"))
                    .spawn(move || worker_loop(queue, db, store, root))
                    .expect("failed to spawn thumb worker"),
            );
        }

        ThumbPool {
            db: Mutex::new(Some(db)),
            queue,
            handles: Mutex::new(handles),
        }
    }

    /// Enqueue an interactive request (served before any background work, LIFO).
    ///
    /// Coalescing happens here: if the same artifact is already being decoded,
    /// or is already queued anywhere, this caller joins it rather than adding a
    /// second decode. A request already sitting in the background tier is
    /// *promoted* — the user is looking at it now.
    pub fn enqueue(&self, request: ThumbRequest) {
        let key = match self.request_key(&request) {
            Ok(key) => key,
            Err(error) => {
                (request.respond)(Err(error));
                return;
            }
        };
        let mut guard = self.queue.items.lock().unwrap();
        let Some(q) = guard.as_mut() else {
            (request.respond)(Err(AppError::Other("thumb pool shut down".into())));
            return;
        };

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
                cache_checked: request.cache_checked,
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
        let requests: Vec<_> = requests
            .into_iter()
            .filter_map(|request| match self.request_key(&request) {
                Ok(key) => Some((request, key)),
                Err(error) => {
                    (request.respond)(Err(error));
                    None
                }
            })
            .collect();
        let mut guard = self.queue.items.lock().unwrap();
        let Some(q) = guard.as_mut() else {
            for (request, _) in requests {
                (request.respond)(Err(AppError::Other("thumb pool shut down".into())));
            }
            return;
        };
        for (request, key) in requests {
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
                cache_checked: request.cache_checked,
                known_version: request.known_version,
                responders: vec![request.respond],
                needs_row: true,
            });
        }
        drop(guard);
        self.queue.signal.notify_all();
    }

    fn request_key(&self, request: &ThumbRequest) -> AppResult<WorkKey> {
        let db = self
            .db
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| AppError::Other("thumb pool shut down".into()))?;
        let version = match request.known_version {
            Some(version) => version,
            None => {
                let (_, _, mtime, orientation) = file_row(&db, request.file_id)?;
                CacheVersion {
                    mtime,
                    orientation: orientation.unwrap_or(1),
                }
            }
        };
        let source = source_version_for(&db, request.file_id, request.kind)?;
        Ok((
            request.file_id,
            request.kind as u8,
            Some(version),
            source,
            preview_long_edge(),
        ))
    }

    /// Stop accepting work and unblock all workers (they exit). Draining calls
    /// every pending request's `respond` with an error, so a caller blocked on a
    /// completion channel (ingest Phase B) is always released.
    pub fn shutdown(&self) {
        self.stop_queue();
        self.db.lock().unwrap().take();
        // Then WAIT for the workers to go. Each holds its own `Arc<Db>`, so
        // until they have exited the database is still open — which on Windows
        // means its file cannot be deleted, and reimporting a project is exactly
        // an attempt to delete it. Waking them is not the same as them being
        // gone: one mid-decode finishes the frame it is on first.
        let handles = std::mem::take(&mut *self.handles.lock().unwrap());
        for h in handles {
            let _ = h.join();
        }
    }

    fn stop_queue(&self) {
        self.queue.cancelled.store(true, Ordering::Relaxed);
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
    decode::video::set_cancellation(queue.cancelled.clone());
    loop {
        let pending = {
            let mut guard = queue.items.lock().unwrap();
            loop {
                match guard.as_mut() {
                    None => return,
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
                cache_checked: pending.cache_checked,
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

#[cfg(test)]
pub(crate) fn cache_rel_path(file_id: i64, version: CacheVersion, kind: ThumbKind) -> String {
    cache_rel_path_for_edge(file_id, version, kind, preview_long_edge())
}

fn cache_rel_path_for_edge(
    file_id: i64,
    version: CacheVersion,
    kind: ThumbKind,
    preview_edge: u32,
) -> String {
    let bucket = (file_id % 256) as u8;
    // The preview's target size is a user setting, so it belongs in the path:
    // two sizes then occupy different files instead of one overwriting the
    // other, and a project last opened under a different setting can never be
    // served the wrong one from a cache hit.
    let suffix = match kind {
        ThumbKind::Thumb => "t".to_string(),
        ThumbKind::Preview => format!("p{preview_edge}"),
        ThumbKind::Full => "f".to_string(),
    };
    let CacheVersion { mtime, orientation } = version;
    format!("{bucket:02x}/{file_id}_{mtime}_{orientation}_r2_{suffix}.jpg")
}

pub(crate) fn source_version_sql() -> String {
    format!(
        "CASE WHEN f.kind = 0 THEN CAST(f.group_id AS TEXT) || ':' || g.decoupled || ':' ||
        CASE WHEN g.decoupled = 0 AND f.ext NOT IN ({secondary}) THEN
          COALESCE((SELECT CAST(s.id AS TEXT) || ':' || s.mtime || ':' || s.size || ':' ||
              COALESCE(s.orientation, 1) || ':' || s.rel_path
            FROM files s WHERE s.group_id=f.group_id AND s.kind=1 AND s.status=0
              AND s.ext IN ({images}) ORDER BY s.id LIMIT 1), 'self')
          ELSE 'self' END ELSE '' END",
        secondary = crate::db::sql::secondary_raw_exts(),
        images = crate::db::sql::image_exts()
    )
}

pub(crate) fn source_version(value: &str) -> u64 {
    if value.is_empty() {
        0
    } else {
        xxhash_rust::xxh3::xxh3_64(value.as_bytes())
    }
}

pub(crate) fn source_version_for(db: &Arc<Db>, file_id: i64, kind: ThumbKind) -> AppResult<u64> {
    if kind == ThumbKind::Full {
        return Ok(0);
    }
    db.call_read(move |conn| {
        let sql = format!(
            "SELECT {} FROM files f JOIN groups g ON g.id=f.group_id WHERE f.id=?1 AND f.status=0",
            source_version_sql()
        );
        let value: String = conn.query_row(&sql, [file_id], |r| r.get(0))?;
        Ok(source_version(&value))
    })
}

fn source_cache_rel_path(
    file_id: i64,
    version: CacheVersion,
    kind: ThumbKind,
    source_version: u64,
) -> String {
    source_cache_rel_path_for_edge(file_id, version, kind, source_version, preview_long_edge())
}

fn source_cache_rel_path_for_edge(
    file_id: i64,
    version: CacheVersion,
    kind: ThumbKind,
    source_version: u64,
    preview_edge: u32,
) -> String {
    let base = cache_rel_path_for_edge(file_id, version, kind, preview_edge);
    if source_version == 0 {
        base
    } else {
        base.replace("_r2_", &format!("_r2s{source_version:016x}_"))
    }
}

pub(crate) fn resolved_cache_rel_path(
    db: &Arc<Db>,
    file_id: i64,
    version: CacheVersion,
    kind: ThumbKind,
) -> AppResult<String> {
    Ok(source_cache_rel_path(
        file_id,
        version,
        kind,
        source_version_for(db, file_id, kind)?,
    ))
}

/// Call before thumbnail workers start. Delete only unreferenced cache artifacts.
pub(crate) fn prune_cache(db: &Arc<Db>, root: &Path) -> AppResult<usize> {
    db.call(|conn| {
        let tx = conn.transaction()?;
        let first = tx.execute(
            "INSERT OR IGNORE INTO settings (key, value) VALUES ('cacheFailureRecovery', '1')",
            [],
        )?;
        // Old failure rows can contain temporary source errors. Retry them once.
        if first != 0 {
            tx.execute("DELETE FROM thumbnails WHERE failed = 1", [])?;
        }
        tx.commit()?;
        Ok(())
    })?;
    invalidate_cache_rows(db)?;
    let referenced: std::collections::HashSet<String> = db.call_read(|conn| {
        let mut statement = conn.prepare("SELECT t.cache_path FROM thumbnails t JOIN files f ON f.id=t.file_id WHERE t.failed=0 AND f.status=0 AND t.source_mtime=f.mtime")?;
        let paths = statement.query_map([], |row| row.get(0))?;
        Ok(paths.collect::<Result<_, _>>()?)
    })?;
    let directory = root.join(".cullant/thumbs");
    for path in [root.join(".cullant"), directory.clone()] {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AppError::Other(
                    "refuse to prune a linked cache directory".into(),
                ))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error.into()),
        }
    }
    let buckets = match std::fs::read_dir(directory) {
        Ok(buckets) => buckets,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    let mut removed = 0;
    for bucket in buckets {
        let bucket = bucket?;
        let name = bucket.file_name().to_string_lossy().into_owned();
        if name.len() != 2
            || !name.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !bucket.file_type()?.is_dir()
        {
            continue;
        }
        for entry in std::fs::read_dir(bucket.path())? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            if !matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("jpg" | "tmp")
            ) {
                continue;
            }
            let relative = format!("{name}/{}", entry.file_name().to_string_lossy());
            if !referenced.contains(&relative) {
                std::fs::remove_file(path)?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

pub(crate) fn invalidate_cache_rows(db: &Arc<Db>) -> AppResult<()> {
    let root = db
        .path()
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| AppError::Other("project database has no data directory".into()))?
        .to_path_buf();
    db.call(move |conn| {
        conn.execute("UPDATE files SET width=NULL, height=NULL WHERE kind=0 AND EXISTS(SELECT 1 FROM thumbnails t WHERE t.file_id=files.id AND t.failed=0 AND t.cache_path NOT LIKE '%_r2_%' AND t.cache_path NOT LIKE '%_r2s%')", [])?;
        let sql = format!("SELECT t.file_id, t.kind, t.cache_path, f.mtime, COALESCE(f.orientation,1), {}, t.failed, t.source_mtime, t.long_edge FROM thumbnails t JOIN files f ON f.id=t.file_id JOIN groups g ON g.id=f.group_id WHERE f.status=0", source_version_sql());
        let mut statement = conn.prepare(&sql)?;
        let entries = statement.query_map([], |row| Ok((row.get::<_,i64>(0)?, row.get::<_,i64>(1)?, row.get::<_,String>(2)?, row.get::<_,i64>(3)?, row.get::<_,i64>(4)?, row.get::<_,String>(5)?, row.get::<_,bool>(6)?, row.get::<_,i64>(7)?, row.get::<_,u32>(8)?)))?.collect::<Result<Vec<_>,_>>()?;
        drop(statement);
        for (id, kind, path, mtime, orientation, source, failed, source_mtime, long_edge) in entries {
            let kind = match kind { 0 => ThumbKind::Thumb, 1 => ThumbKind::Preview, 3 => ThumbKind::Full, _ => continue };
            let source = if kind == ThumbKind::Full { 0 } else { source_version(&source) };
            let current = path == source_cache_rel_path(id, CacheVersion { mtime, orientation }, kind, source)
                && source_mtime == mtime
                && (kind != ThumbKind::Preview || long_edge == 0 || long_edge == preview_long_edge());
            // A decode tombstone has no JPEG. It must survive a valid cache check.
            if !current || (!failed && !cached_artifact_ready(&root, &path)) {
                conn.execute("DELETE FROM thumbnails WHERE file_id=?1 AND kind=?2", params![id, kind as i64])?;
            }
        }
        Ok(())
    })
}

pub(crate) fn cached_artifact_path(root: &Path, relative: &str) -> Option<PathBuf> {
    let (bucket, name) = relative.split_once('/')?;
    if bucket.len() != 2
        || !bucket.bytes().all(|byte| byte.is_ascii_hexdigit())
        || name.contains(['/', '\\'])
        || !name.ends_with(".jpg")
    {
        return None;
    }
    Some(root.join(".cullant/thumbs").join(bucket).join(name))
}

pub(crate) fn cached_row_ready(
    root: &Path,
    relative: &str,
    file_id: i64,
    version: CacheVersion,
    kind: ThumbKind,
    source_version: u64,
) -> bool {
    relative == source_cache_rel_path(file_id, version, kind, source_version)
        && cached_artifact_ready(root, relative)
}

pub(crate) fn cached_artifact_ready(root: &Path, relative: &str) -> bool {
    cached_artifact_path(root, relative).is_some_and(|path| cached_jpeg_dimensions(&path).is_some())
}

fn cached_jpeg_dimensions(path: &Path) -> Option<(u32, u32)> {
    let file = std::fs::File::open(path).ok()?;
    cache_jpeg_dimensions(std::io::BufReader::new(file))
}

fn cache_jpeg_dimensions<R: std::io::BufRead + std::io::Seek>(mut reader: R) -> Option<(u32, u32)> {
    use std::io::SeekFrom;
    let mut marker = [0; 2];
    reader.read_exact(&mut marker).ok()?;
    if marker != [0xff, 0xd8] {
        return None;
    }
    let image_end = reader.seek(SeekFrom::End(-2)).ok()?;
    reader.read_exact(&mut marker).ok()?;
    if marker != [0xff, 0xd9] {
        return None;
    }
    reader.seek(SeekFrom::Start(2)).ok()?;
    cache_jpeg_header(&mut reader, image_end)?;
    reader.seek(SeekFrom::Start(0)).ok()?;
    let mut decoder = jpeg_decoder::Decoder::new(reader);
    decoder.read_info().ok()?;
    let info = decoder.info()?;
    (info.width > 0 && info.height > 0).then_some((u32::from(info.width), u32::from(info.height)))
}

fn cache_jpeg_header<R: std::io::BufRead + std::io::Seek>(
    reader: &mut R,
    image_end: u64,
) -> Option<()> {
    let mut components = Vec::new();
    let mut quantization = 0;
    let mut huffman = 0;
    loop {
        let mut header = [0; 4];
        reader.read_exact(&mut header).ok()?;
        if header[0] != 0xff {
            return None;
        }
        let length = u16::from_be_bytes([header[2], header[3]]).checked_sub(2)?;
        let segment_end = reader
            .stream_position()
            .ok()?
            .checked_add(u64::from(length))?;
        // Generated cache headers are small. Bound malformed segment reads.
        if segment_end >= image_end || segment_end > 64 * 1024 {
            return None;
        }
        let mut payload = vec![0; usize::from(length)];
        reader.read_exact(&mut payload).ok()?;
        match header[1] {
            0xc0 if components.is_empty() => components = cache_jpeg_components(&payload)?,
            0xdb => quantization |= cache_jpeg_quantization(&payload)?,
            0xc4 => huffman |= cache_jpeg_huffman(&payload)?,
            0xda => return cache_jpeg_scan(&payload, &components, quantization, huffman),
            0xe0..=0xef | 0xfe => {}
            0xdd if payload.len() == 2 => {}
            _ => return None,
        }
    }
}

fn cache_jpeg_components(payload: &[u8]) -> Option<Vec<(u8, u8)>> {
    let count = usize::from(*payload.get(5)?);
    if !(1..=4).contains(&count) || payload[0] != 8 || payload.len() != 6 + 3 * count {
        return None;
    }
    let mut components = Vec::with_capacity(count);
    for component in payload[6..].chunks_exact(3) {
        if component[2] > 3 || components.iter().any(|&(id, _)| id == component[0]) {
            return None;
        }
        components.push((component[0], component[2]));
    }
    Some(components)
}

fn cache_jpeg_quantization(payload: &[u8]) -> Option<u8> {
    let tables = payload.chunks_exact(65);
    if payload.is_empty() || !tables.remainder().is_empty() {
        return None;
    }
    let mut mask = 0;
    for table in tables {
        if table[0] > 3 || table[1..].contains(&0) {
            return None;
        }
        mask |= 1 << table[0];
    }
    Some(mask)
}

fn cache_jpeg_huffman(mut payload: &[u8]) -> Option<u8> {
    if payload.is_empty() {
        return None;
    }
    let mut mask = 0;
    while !payload.is_empty() {
        let definition = *payload.first()?;
        let class = definition >> 4;
        let id = definition & 0x0f;
        if class > 1 || id > 3 {
            return None;
        }
        let counts = payload.get(1..17)?;
        let mut available = 1i32;
        for &count in counts {
            available = available * 2 - i32::from(count);
            if available < 0 {
                return None;
            }
        }
        let symbols = counts
            .iter()
            .map(|&count| usize::from(count))
            .sum::<usize>();
        if symbols == 0 || symbols > 256 {
            return None;
        }
        let values = payload.get(17..17 + symbols)?;
        let invalid_symbol = |&value: &u8| {
            if class == 0 {
                value > 11
            } else {
                let size = value & 0x0f;
                size > 10 || (size == 0 && value != 0 && value != 0xf0)
            }
        };
        if values.iter().any(invalid_symbol) {
            return None;
        }
        payload = payload.get(17 + symbols..)?;
        mask |= 1 << (4 * class + id);
    }
    Some(mask)
}

fn cache_jpeg_scan(
    payload: &[u8],
    components: &[(u8, u8)],
    quantization: u8,
    huffman: u8,
) -> Option<()> {
    let count = usize::from(*payload.first()?);
    if components.is_empty()
        || count != components.len()
        || payload.len() != 4 + 2 * count
        || !payload.ends_with(&[0, 63, 0])
    {
        return None;
    }
    let mut seen = Vec::with_capacity(count);
    for scan in payload[1..1 + 2 * count].chunks_exact(2) {
        let &(_, quantization_id) = components.iter().find(|&&(id, _)| id == scan[0])?;
        let dc = scan[1] >> 4;
        let ac = scan[1] & 0x0f;
        if dc > 3 || ac > 3 || seen.contains(&scan[0]) {
            return None;
        }
        let required_huffman = (1 << dc) | (1 << (4 + ac));
        if quantization & (1 << quantization_id) == 0
            || huffman & required_huffman != required_huffman
        {
            return None;
        }
        seen.push(scan[0]);
    }
    Some(())
}

pub(crate) fn read_cached_jpeg(path: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    cache_jpeg_dimensions(std::io::Cursor::new(&bytes))?;
    Some(bytes)
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
///
/// The sibling must be a *decodable* image, not merely `kind = 1`: a RAW+HEIF
/// shot has an image-kind sibling whose pixels no decoder here can reach, and
/// borrowing it would turn a RAW that renders perfectly into a failed thumbnail.
///
/// A companion RAW is excluded, because for it the premise is false. An `.ORI`
/// is the frame *before* the camera composited it, and the sibling JPEG is the
/// frame after — showing the JPEG would draw the two files identically and hide
/// the only difference the user opened the `.ORI` to see.
///
/// Built once, because this runs per rendered RAW: over the whole library in the
/// fused ingest pass, and again on every interactive zoom into a RAW cell.
static PAIRED_JPEG_SQL: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    format!(
        "SELECT s.rel_path
         FROM files f
         JOIN groups g ON g.id = f.group_id
         JOIN files s ON s.group_id = f.group_id AND s.kind = 1 AND s.status = 0
           AND s.ext IN ({images})
         WHERE f.id = ?1 AND f.kind = 0 AND f.status = 0 AND g.decoupled = 0
           AND f.ext NOT IN ({secondary})
         ORDER BY s.id LIMIT 1",
        images = crate::db::sql::image_exts(),
        secondary = crate::db::sql::secondary_raw_exts()
    )
});

fn paired_jpeg(db: &Arc<Db>, file_id: i64) -> Option<String> {
    db.call_read(move |conn| {
        Ok(conn
            .query_row(&PAIRED_JPEG_SQL, params![file_id], |r| {
                r.get::<_, String>(0)
            })
            .optional()?)
    })
    .ok()
    .flatten()
}

pub(crate) struct Decoded {
    pub image: DynamicImage,
    /// The ORIGINAL pixel dimensions, when reliably known (a JPEG/PNG header, a
    /// full-size embedded RAW image, or a platform decoder reporting them).
    /// A scaled decode must never be recorded as the file's real dimensions.
    pub src_dims: Option<(u32, u32)>,
    /// See [`SourceMeta::pre_oriented`].
    pub pre_oriented: bool,
}

impl Decoded {
    /// A decode that leaves rotation to the caller — every in-process one.
    fn new(image: DynamicImage, src_dims: Option<(u32, u32)>) -> Self {
        Decoded {
            image,
            src_dims,
            pre_oriented: false,
        }
    }
}

/// Decode a source image with a long edge of at least `min_long_edge`,
/// reading as little as possible (mmap + scaled/adequate-size decodes).
pub(crate) fn decode_for(
    store: &dyn ProjectStore,
    rel_path: &str,
    file_kind: i64,
    min_long_edge: u32,
) -> AppResult<Decoded> {
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
                        return Ok(Decoded::new(img, None));
                    }
                    // A corrupt embedded JPEG is rare; fall back to rawler rather
                    // than tombstoning a file rawler might still decode.
                    Err(e) => tracing::debug!(
                        "{rel_path}: embedded JPEG decode failed, falling back to rawler: {e}"
                    ),
                }
            }
            let raw = decode::raw::embedded_preview_scaled(&source, min_long_edge, rel_path)?;
            Ok(Decoded::new(raw.image, None))
        }
        1 if decode::is_heif(rel_path) => {
            // A HEIF is never opened here: no in-process decoder can read one,
            // so the bytes go to whichever rung of the platform ladder this
            // machine has. Callers guard on `heif::possible` first, so an absent
            // decoder never reaches here as a "failure".
            let (img, dims, pre_oriented) = decode::heif::decode(store, rel_path, min_long_edge)?;
            Ok(Decoded {
                image: img,
                src_dims: Some(dims),
                pre_oriented,
            })
        }
        1 => {
            let source = decode::open_source(store, rel_path)?;
            // Header-only original dimensions (cheap, no decode).
            let dims = image::ImageReader::new(std::io::Cursor::new(source.buf()))
                .with_guessed_format()
                .ok()
                .and_then(|r| r.into_dimensions().ok());
            let img = decode::jpeg::decode_scaled(source.buf(), min_long_edge, rel_path)?;
            Ok(Decoded::new(img, dims))
        }
        2 => {
            // Videos: extract a poster frame through whichever extractor this
            // store has (ffmpeg on a real filesystem, the platform media API on
            // Android SAF). It reports the clip's own display resolution, so
            // those dims are trustworthy. Callers guard on availability first
            // (see `produce`), so an absent extractor never reaches here as a
            // "failure".
            let (img, dims) = decode::video::extract_poster(store, rel_path, min_long_edge)?;
            // Every poster extractor bakes in the clip's display rotation.
            Ok(Decoded {
                image: img,
                src_dims: Some(dims),
                pre_oriented: true,
            })
        }
        _ => Err(AppError::Decode(format!(
            "no thumbnail source for kind {file_kind}"
        ))),
    }
}

/// Identity + trusted facts about a decoded source, shared by every artifact
/// rendered from the same decode.
pub(crate) struct SourceMeta {
    pub file_id: i64,
    pub mtime: i64,
    pub orientation: i64,
    /// The decoder already applied the source's rotation, so `orientation` must
    /// not be applied a second time. True for a platform HEIF decoder, which
    /// honours the container's own `irot` property, and for a video poster.
    ///
    /// It suppresses the rotation and nothing else: `orientation` still keys the
    /// cache path, because that is what the DB holds and what the frontend puts
    /// in the request URL.
    pub pre_oriented: bool,
    pub source_orientation: i64,
    pub source_version: u64,
    pub preview_edge: u32,
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
    orientation: i64,
    source_version: u64,
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
        pre_oriented,
        source_orientation,
        source_version,
        preview_edge,
        src_dims,
    } = *meta;
    let (long_edge, quality) = match kind {
        ThumbKind::Thumb => (THUMB_LONG_EDGE, 80),
        ThumbKind::Preview => (preview_edge, 80),
        ThumbKind::Full => (u32::MAX, 90),
    };
    let resized = resize_long_edge(decoded, long_edge)?;
    let unrotated = if pre_oriented {
        apply_orientation(
            resized,
            match source_orientation {
                6 => 8,
                8 => 6,
                other => other,
            },
        )
    } else {
        resized
    };
    let oriented = apply_orientation(unrotated, orientation);
    let jpeg = encode_jpeg(&oriented, quality)?;

    let cache_rel = source_cache_rel_path_for_edge(
        file_id,
        CacheVersion { mtime, orientation },
        kind,
        source_version,
        preview_edge,
    );
    let cache_abs = root.join(".cullant").join("thumbs").join(&cache_rel);
    if let Some(parent) = cache_abs.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Write via temp file + rename so a crashed worker never leaves a
    // truncated JPEG in the cache.
    static TEMP_ID: AtomicU64 = AtomicU64::new(0);
    let tmp = cache_abs.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        file.write_all(&jpeg)?;
        drop(file);
        match std::fs::rename(&tmp, &cache_abs) {
            Ok(()) => Ok(()),
            Err(_) if cached_jpeg_dimensions(&cache_abs).is_some() => std::fs::remove_file(&tmp),
            Err(_) if cache_abs.is_file() => {
                std::fs::remove_file(&cache_abs)?;
                std::fs::rename(&tmp, &cache_abs)
            }
            Err(error) => Err(error),
        }
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written?;

    let row = ThumbRow {
        file_id,
        kind_i: kind as i64,
        cache_rel,
        out_w: oriented.width(),
        out_h: oriented.height(),
        mtime,
        orientation,
        source_version,
        src_dims,
        phash: (kind == ThumbKind::Thumb).then(|| dhash(&oriented)),
        long_edge,
    };
    Ok((jpeg, row))
}

/// Apply one rendered thumbnail's row writes on the DB thread's connection.
fn write_thumb_row(conn: &Connection, row: &ThumbRow) -> rusqlite::Result<()> {
    let sql = format!("SELECT f.mtime, COALESCE(f.orientation,1), {} FROM files f JOIN groups g ON g.id=f.group_id WHERE f.id=?1 AND f.status=0", source_version_sql());
    let current = conn
        .query_row(&sql, [row.file_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .optional()?;
    let Some((mtime, orientation, source)) = current else {
        return Ok(());
    };
    let source = if row.kind_i == ThumbKind::Full as i64 {
        0
    } else {
        source_version(&source)
    };
    if mtime != row.mtime
        || orientation != row.orientation
        || source != row.source_version
        || (row.kind_i == ThumbKind::Preview as i64 && row.long_edge != preview_long_edge())
    {
        return Ok(());
    }
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
    let old_path: Option<String> = db.call(move |conn| {
        let old = conn
            .query_row(
                "SELECT cache_path FROM thumbnails WHERE file_id=?1 AND kind=?2",
                params![row.file_id, row.kind_i],
                |r| r.get(0),
            )
            .optional()?;
        write_thumb_row(conn, &row)?;
        let current: Option<String> = conn
            .query_row(
                "SELECT cache_path FROM thumbnails WHERE file_id=?1 AND kind=?2",
                params![row.file_id, row.kind_i],
                |r| r.get(0),
            )
            .optional()?;
        Ok(if current.as_deref() == Some(row.cache_rel.as_str()) {
            old
        } else {
            None
        })
    })?;
    if let Some(old) = old_path.filter(|old| {
        *old != source_cache_rel_path_for_edge(
            meta.file_id,
            CacheVersion {
                mtime: meta.mtime,
                orientation: meta.orientation,
            },
            kind,
            meta.source_version,
            meta.preview_edge,
        )
    }) {
        if let Some(path) = cached_artifact_path(root, &old) {
            let _ = std::fs::remove_file(path);
        }
    }
    Ok(jpeg)
}

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
    /// The caller already tried `known`'s cache path and missed. Set by the
    /// protocol handler, whose whole fast path is that same read -- without
    /// this the worker repeats it for every miss, which is exactly the case
    /// where it is guaranteed to fail again.
    pub cache_checked: bool,
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
            cache_checked: false,
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
/// the authoritative `file_row` lookup runs. If its version differs from the
/// URL, return an error. An optimistic rotation can request the next orientation
/// before the database write completes. Returning current pixels would cache
/// the wrong rotation under that immutable URL.
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
        cache_checked,
    } = req;
    // Fast path: a trusted version lets us try the cache before touching the DB.
    // Full-of-plain-image has no cache file, so it's left to the miss path
    // below (it needs rel_path anyway); every other kind can hit here.
    if let Some(version) = known.filter(|_| !cache_checked) {
        let cache_rel = resolved_cache_rel_path(db, file_id, version, kind)?;
        let cache_abs = root.join(".cullant").join("thumbs").join(&cache_rel);
        if let Some(bytes) = read_cached_jpeg(&cache_abs) {
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
                        orientation: version.orientation,
                        source_version: source_version_for(db, file_id, kind)?,
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
    if known.is_some_and(|version| version != (CacheVersion { mtime, orientation })) {
        return Err(AppError::Other("image version changed".into()));
    }

    if kind == ThumbKind::Full
        && file_kind == 1
        && matches!(
            Path::new(&rel_path)
                .extension()
                .and_then(|ext| ext.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("jpg" | "jpeg")
        )
    {
        let source = decode::open_source(store, &rel_path)?;
        let original = decode::exif::read_metadata(source.buf())?
            .orientation
            .map(i64::from)
            .unwrap_or(1);
        if original == orientation && source.buf().starts_with(&[255, 216]) {
            return Ok(source.buf().to_vec());
        }
    }
    let source_version = source_version_for(db, file_id, kind)?;
    let cache_rel = source_cache_rel_path(
        file_id,
        CacheVersion { mtime, orientation },
        kind,
        source_version,
    );
    let cache_abs = root.join(".cullant").join("thumbs").join(&cache_rel);
    // Re-check the cache under the authoritative version. Skipped work only when
    // `known` was present AND equal to it AND already missed above; the
    // redundant read is a cheap stat on the cold/miss path.
    if let Some(bytes) = read_cached_jpeg(&cache_abs) {
        return Ok(bytes);
    }

    // A previous attempt at this exact mtime already failed — don't grind on an
    // undecodable file every time its cell scrolls back into view.
    if is_tombstoned(db, file_id, kind, mtime)? {
        return Err(AppError::Decode(format!(
            "{rel_path}: previously undecodable"
        )));
    }

    // A poster needs an extractor this store can reach. When there is none, fail
    // without tombstoning so a later ffmpeg install (which changes no file's
    // mtime) still gets a chance next scan.
    if file_kind == 2 && !decode::video::poster_possible(store, &rel_path) {
        return Err(AppError::Decode(format!(
            "{rel_path}: no video poster extractor available"
        )));
    }

    // Same shape, same reason: a HEIF needs a decoder borrowed from the platform,
    // and every rung of that ladder can be absent. It must fail BEFORE the read
    // (on Android SAF there is no mmap, so a HEIC-only library would be pulled
    // into memory one cell at a time to learn nothing) and WITHOUT a tombstone.
    // A tombstone is keyed on mtime, and installing a decoder changes no file's
    // mtime — every one of those photos would stay an empty cell after the
    // upgrade.
    if decode::is_heif(&rel_path) && !decode::heif::possible(store, &rel_path) {
        return Err(AppError::Decode(format!(
            "{rel_path}: no decoder for this container yet"
        )));
    }

    // A grid thumbnail or a loupe preview of a paired RAW is rendered from the
    // JPEG half instead: same frame, a fraction of the bytes. `Full` is
    // excluded — zooming in is exactly when the RAW's own pixels are the point.
    let (mut source_rel, mut source_kind) = match kind {
        ThumbKind::Full => (rel_path.clone(), file_kind),
        _ => match paired_jpeg(db, file_id) {
            Some(sibling) => (sibling, 1),
            None => (rel_path.clone(), file_kind),
        },
    };

    let preview_edge = preview_long_edge();
    let min_edge = min_long_edge_for(kind);
    let attempt = decode_for(store, &source_rel, source_kind, min_edge).or_else(|error| {
        if source_rel == rel_path {
            return Err(error);
        }
        tracing::debug!("{source_rel}: sibling decode failed, trying {rel_path}: {error}");
        source_rel = rel_path.clone();
        source_kind = file_kind;
        decode_for(store, &source_rel, source_kind, min_edge)
    });
    let decoded = match attempt {
        Ok(d) => d,
        Err(e) => {
            if decode::video::cancelled() {
                return Err(e);
            }
            // A disconnected source or SAF access error can succeed on the next scan.
            if matches!(&e, AppError::Decode(_)) {
                record_decode_failure(db, file_id, mtime, kind)?;
            }
            return Err(e);
        }
    };
    if decode::video::cancelled() {
        return Err(AppError::Decode("thumbnail cancelled".into()));
    }
    let meta = SourceMeta {
        file_id,
        mtime,
        orientation,
        pre_oriented: decoded.pre_oriented,
        source_orientation: if decoded.pre_oriented && decode::is_heif(&source_rel) {
            decode::open_source(store, &source_rel)
                .ok()
                .and_then(|source| decode::exif::read_metadata(source.buf()).ok())
                .and_then(|meta| meta.orientation)
                .map(i64::from)
                .unwrap_or(1)
        } else {
            1
        },
        source_version,
        preview_edge,
        // Dimensions describe whatever was decoded. Backfilling a RAW's row
        // from its JPEG would record the wrong numbers, so only a decode of the
        // file itself may claim them.
        src_dims: if source_rel == rel_path {
            decoded.src_dims
        } else {
            None
        },
    };
    let decoded = decoded.image;
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
    let (_, _, _, orientation) = file_row(db, file_id)?;
    let cache_rel = resolved_cache_rel_path(
        db,
        file_id,
        CacheVersion {
            mtime,
            orientation: orientation.unwrap_or(1),
        },
        kind,
    )?;
    db.call(move |conn| {
        conn.execute(
            "INSERT INTO thumbnails (file_id, kind, cache_path, width, height, source_mtime, generated_at, failed)
             VALUES (?1, ?2, ?5, NULL, NULL, ?3, ?4, 1)
             ON CONFLICT(file_id, kind) DO UPDATE SET
               cache_path = excluded.cache_path, width = NULL, height = NULL,
               source_mtime = ?3, generated_at = ?4, failed = 1",
            params![file_id, kind_i, mtime, now_secs(), cache_rel],
        )?;
        Ok(())
    })
}

fn is_tombstoned(db: &Arc<Db>, file_id: i64, kind: ThumbKind, mtime: i64) -> AppResult<bool> {
    let kind_i = kind as i64;
    let (_, _, _, orientation) = file_row(db, file_id)?;
    let cache_rel = resolved_cache_rel_path(
        db,
        file_id,
        CacheVersion {
            mtime,
            orientation: orientation.unwrap_or(1),
        },
        kind,
    )?;
    db.call_read(move |conn| {
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM thumbnails
             WHERE file_id = ?1 AND kind = ?2 AND failed = 1 AND source_mtime = ?3 AND cache_path=?4",
            params![file_id, kind_i, mtime, cache_rel],
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

/// Apply an EXIF orientation. Takes a `Cow` so the overwhelmingly common
/// neutral orientation costs nothing: every rotation allocates a new buffer
/// anyway, but orientation 1 hands back exactly what it was given.
fn apply_orientation(img: Cow<'_, DynamicImage>, orientation: i64) -> Cow<'_, DynamicImage> {
    let rotated = match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => return img,
    };
    Cow::Owned(rotated)
}

/// Downscale with SIMD (fast_image_resize) so the long edge is `long_edge`.
/// Borrows the source so multiple targets can be rendered from one decode.
///
/// Returns a `Cow` because "no resize needed" is not a rare case: it is ALWAYS
/// true for `ThumbKind::Full`, whose long edge is `u32::MAX`. Cloning there cost
/// a full second copy of the image -- around 72 MB on a 6000x4000 RAW, on every
/// zoom-in, immediately before the encode copied it again.
fn resize_long_edge(img: &DynamicImage, long_edge: u32) -> AppResult<Cow<'_, DynamicImage>> {
    use fast_image_resize::{ResizeAlg, ResizeOptions, Resizer};

    let (dst_w, dst_h) = scaled_dims(img.width(), img.height(), long_edge);
    if (dst_w, dst_h) == (img.width(), img.height()) {
        return Ok(Cow::Borrowed(img));
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
    Ok(Cow::Owned(dst))
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

    fn media_project(files: &[(&str, &[u8])]) -> (tempfile::TempDir, Arc<Db>) {
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in files {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        let db = Arc::new(Db::open(dir.path()).unwrap());
        crate::scan::scan_project_inner(&db, dir.path(), &mut |_| {}).unwrap();
        (dir, db)
    }

    fn corpus_raw() -> Option<Vec<u8>> {
        let result = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../fixtures/media/Olympus - E-M1MarkII - 16bit (4-3).ORF"),
        );
        match result {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("skipped: local RAW corpus is not installed");
                None
            }
            Err(error) => panic!("could not read local RAW fixture: {error}"),
        }
    }

    #[test]
    fn media_regression_bad_sibling_preserves_good_raw() {
        let Some(raw) = corpus_raw() else { return };
        let (dir, db) = media_project(&[("shot.orf", &raw), ("shot.jpg", b"corrupt sibling")]);
        let store = crate::store::LocalFsStore::new(dir.path());
        let id = db
            .call_read(|c| Ok(c.query_row("SELECT id FROM files WHERE kind=0", [], |r| r.get(0))?))
            .unwrap();
        for kind in [ThumbKind::Thumb, ThumbKind::Preview, ThumbKind::Full] {
            let bytes = produce_cached(
                &db,
                &store,
                dir.path(),
                Produce::interactive(id, kind, None),
            )
            .unwrap();
            assert!(image::load_from_memory(&bytes).is_ok());
        }
        let failed: i64 = db
            .call_read(move |c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM thumbnails WHERE file_id=?1 AND failed=1",
                    [id],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(failed, 0, "A bad sibling must not tombstone the RAW");
    }

    #[test]
    fn media_regression_source_and_pair_changes_refresh_raw_cache() {
        let Some(raw) = corpus_raw() else { return };
        let jpeg = |color| {
            encode_jpeg(
                &DynamicImage::ImageRgb8(image::RgbImage::from_pixel(600, 400, image::Rgb(color))),
                90,
            )
            .unwrap()
        };
        let (dir, db) = media_project(&[("shot.orf", &raw), ("shot.jpg", &jpeg([250, 5, 5]))]);
        let store = crate::store::LocalFsStore::new(dir.path());
        let id: i64 = db
            .call_read(|c| Ok(c.query_row("SELECT id FROM files WHERE kind=0", [], |r| r.get(0))?))
            .unwrap();
        let before = produce_cached(
            &db,
            &store,
            dir.path(),
            Produce::interactive(id, ThumbKind::Preview, None),
        )
        .unwrap();
        std::fs::write(dir.path().join("shot.jpg"), jpeg([5, 5, 250])).unwrap();
        db.call(|c| {
            c.execute("UPDATE files SET mtime=mtime+1 WHERE kind=1", [])?;
            Ok(())
        })
        .unwrap();
        let after = produce_cached(
            &db,
            &store,
            dir.path(),
            Produce::interactive(id, ThumbKind::Preview, None),
        )
        .unwrap();
        assert_ne!(
            before, after,
            "The selected sibling version must key the RAW cache"
        );
        db.call(|c| {
            c.execute("UPDATE groups SET decoupled=1", [])?;
            Ok(())
        })
        .unwrap();
        let decoupled = produce_cached(
            &db,
            &store,
            dir.path(),
            Produce::interactive(id, ThumbKind::Preview, None),
        )
        .unwrap();
        assert_ne!(
            after, decoupled,
            "Decoupling must stop serving the borrowed image"
        );
    }

    #[test]
    fn media_regression_raw_preview_is_not_sensor_dimensions() {
        let Some(raw) = corpus_raw() else { return };
        let (dir, db) = media_project(&[("shot.orf", &raw)]);
        let store = crate::store::LocalFsStore::new(dir.path());
        let decoded = decode_for(&store, "shot.orf", 0, 384).unwrap();
        assert!(
            decoded.src_dims.is_none() || decoded.src_dims == Some((5184, 3888)),
            "A reduced Olympus preview is not the sensor size: {:?}",
            decoded.src_dims
        );
        drop(db);
    }

    #[test]
    fn media_regression_full_honors_rotation_and_decodes_tiff() {
        let img = image::RgbImage::from_pixel(80, 60, image::Rgb([30, 100, 200]));
        let jpeg = encode_jpeg(&DynamicImage::ImageRgb8(img.clone()), 90).unwrap();
        let mut tiff = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(img)
            .write_to(&mut tiff, image::ImageFormat::Tiff)
            .unwrap();
        let (dir, db) = media_project(&[("photo.jpg", &jpeg), ("photo.tiff", &tiff.into_inner())]);
        let store = crate::store::LocalFsStore::new(dir.path());
        db.call(|c| {
            c.execute("UPDATE files SET orientation=6", [])?;
            Ok(())
        })
        .unwrap();
        let ids: Vec<i64> = db
            .call_read(|c| {
                Ok(c.prepare("SELECT id FROM files")?
                    .query_map([], |r| r.get(0))?
                    .collect::<Result<Vec<_>, _>>()?)
            })
            .unwrap();
        for id in ids {
            let bytes = produce_cached(
                &db,
                &store,
                dir.path(),
                Produce::interactive(id, ThumbKind::Full, None),
            )
            .unwrap();
            assert_eq!(
                &bytes[..2],
                &[255, 216],
                "Full output must match its JPEG MIME"
            );
            let image = image::load_from_memory(&bytes).unwrap();
            assert_eq!((image.width(), image.height()), (60, 80));
        }
    }

    #[test]
    fn media_regression_rotation_request_cannot_cache_previous_orientation() {
        let jpeg = encode_jpeg(
            &DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                600,
                800,
                image::Rgb([30, 100, 200]),
            )),
            90,
        )
        .unwrap();
        let (dir, db) = media_project(&[("photo.jpg", &jpeg)]);
        db.call(|conn| {
            conn.execute("UPDATE files SET orientation=6", [])?;
            Ok(())
        })
        .unwrap();
        let (id, mtime): (i64, i64) = db
            .call_read(|conn| {
                Ok(conn.query_row("SELECT id, mtime FROM files", [], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?)
            })
            .unwrap();
        let pool = ThumbPool::start(
            db.clone(),
            Arc::new(crate::store::LocalFsStore::new(dir.path())),
            dir.path().to_path_buf(),
        );
        let request = |kind, orientation| {
            let (sender, receiver) = std::sync::mpsc::channel();
            pool.enqueue(ThumbRequest {
                file_id: id,
                kind,
                known_version: Some(CacheVersion { mtime, orientation }),
                also_thumb: false,
                cache_checked: true,
                respond: Box::new(move |result| sender.send(result).unwrap()),
            });
            receiver
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap()
        };
        for kind in [ThumbKind::Thumb, ThumbKind::Preview, ThumbKind::Full] {
            let before = image::load_from_memory(&request(kind, 6).unwrap()).unwrap();
            assert!(before.width() > before.height());
            assert!(
                request(kind, 3).is_err(),
                "A future orientation URL must not receive previous-orientation pixels: {kind:?}"
            );
        }
        crate::engine::culling::rotate(
            &db,
            crate::engine::culling::Targets {
                ids: vec![id],
                as_groups: false,
            },
            1,
        )
        .unwrap();
        for kind in [ThumbKind::Thumb, ThumbKind::Preview, ThumbKind::Full] {
            let after = image::load_from_memory(&request(kind, 3).unwrap()).unwrap();
            assert!(after.width() < after.height());
        }
    }

    #[test]
    fn media_regression_concurrent_atomic_cache_writes() {
        let dir = tempfile::tempdir().unwrap();
        let decoded = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            600,
            400,
            image::Rgb([90, 60, 30]),
        ));
        let meta = SourceMeta {
            file_id: 7,
            mtime: 1,
            orientation: 1,
            pre_oriented: false,
            source_orientation: 1,
            source_version: 0,
            preview_edge: preview_long_edge(),
            src_dims: None,
        };
        let barrier = std::sync::Barrier::new(16);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..16)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        for _ in 0..8 {
                            render_to_cache(dir.path(), &meta, &decoded, ThumbKind::Thumb).unwrap();
                        }
                    })
                })
                .collect();
            for handle in handles {
                handle.join().unwrap();
            }
        });
        let path = dir.path().join(".cullant/thumbs").join(cache_rel_path(
            7,
            CacheVersion {
                mtime: 1,
                orientation: 1,
            },
            ThumbKind::Thumb,
        ));
        assert!(image::load_from_memory(&std::fs::read(path).unwrap()).is_ok());
    }

    #[test]
    fn media_regression_old_disk_cache_versions_are_removed() {
        let jpeg = encode_jpeg(
            &DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                80,
                60,
                image::Rgb([10, 20, 30]),
            )),
            90,
        )
        .unwrap();
        let (dir, db) = media_project(&[("photo.jpg", &jpeg)]);
        let store = crate::store::LocalFsStore::new(dir.path());
        let id: i64 = db
            .call_read(|c| Ok(c.query_row("SELECT id FROM files", [], |r| r.get(0))?))
            .unwrap();
        for orientation in [1, 6, 3, 8] {
            db.call(move |c| {
                c.execute("UPDATE files SET orientation=?1", [orientation])?;
                Ok(())
            })
            .unwrap();
            produce_cached(
                &db,
                &store,
                dir.path(),
                Produce::interactive(id, ThumbKind::Thumb, None),
            )
            .unwrap();
        }
        let bucket = dir
            .path()
            .join(".cullant/thumbs")
            .join(format!("{:02x}", id % 256));
        assert_eq!(
            std::fs::read_dir(bucket).unwrap().count(),
            1,
            "Only the current rendered version is retained"
        );
    }

    /// The queue's coalescing/bounding logic, exercised directly against
    /// `Queues` so it needs neither worker threads nor real image files.
    mod queue {
        use super::*;
        use std::sync::mpsc;

        /// Stand-in for `ThumbPool::enqueue`, minus the pool plumbing.
        fn enqueue(q: &mut Queues, max: usize, key_id: i64, respond: Responder) {
            let key = (key_id, ThumbKind::Thumb as u8, None, 0, 0);
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
                cache_checked: false,
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
            let key = (9, ThumbKind::Thumb as u8, None, 0, 0);
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

        let bytes = produce_cached(
            &db,
            &store,
            root,
            Produce::interactive(id, ThumbKind::Thumb, Some(version)),
        )
        .unwrap();

        let hit = produce_cached(
            &db,
            &store,
            root,
            Produce::interactive(id, ThumbKind::Thumb, Some(version)),
        )
        .unwrap();
        assert_eq!(bytes, hit);

        // A stale URL must not receive bytes from a different cache version.
        let stale = CacheVersion {
            mtime: mtime + 999,
            orientation: 1,
        };
        let stale_result = produce_cached(
            &db,
            &store,
            root,
            Produce::interactive(id, ThumbKind::Thumb, Some(stale)),
        );
        assert!(stale_result.is_err());

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

        let shifted = image::RgbImage::from_fn(200, 150, |x, y| {
            let x = (x + 2).min(199);
            image::Rgb([((x * 255) / 200) as u8, ((y * 255) / 150) as u8, 60])
        });
        let shifted = DynamicImage::ImageRgb8(shifted);

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

        let img =
            image::RgbImage::from_fn(800, 600, |x, _| image::Rgb([((x * 255) / 800) as u8; 3]));
        img.save(root.join("photo.jpg")).unwrap();

        let db = Arc::new(Db::open(root).unwrap());
        let store = crate::store::LocalFsStore::new(root);
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        let id: i64 = db
            .call(|c| Ok(c.query_row("SELECT id FROM files", [], |r| r.get(0))?))
            .unwrap();
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
        assert_eq!((thumb.width(), thumb.height()), (288, 384));
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

        let decoded = image::DynamicImage::ImageRgb8(img);
        let meta = SourceMeta {
            file_id: id,
            mtime,
            orientation: 1,
            pre_oriented: false,
            source_orientation: 1,
            source_version: 0,
            preview_edge: preview_long_edge(),
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
