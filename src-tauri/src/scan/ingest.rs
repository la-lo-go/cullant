//! Two-phase ingest.
//!
//! Opening a project must feel instant, so ingest is split by *what gates the
//! grid*:
//!
//! - **Phase A — metadata (gated).** EXIF/RAW metadata (capture_time,
//!   orientation, camera, …) for every file that lacks it. This is the only
//!   blocking phase: the grid is ordered by `COALESCE(capture_time, mtime)`, so
//!   once metadata is in the grid can show, correctly sorted. Metadata is far
//!   cheaper than decoding+resizing+encoding a thumbnail, so this gate is short.
//!   Progress drives `metadata:progress`; the gate releases on `metadata:done`.
//! - **Phase B — thumbnails then previews (background).** With the gate already
//!   open, the 384px grid thumbnails and then the 2560px loupe previews are
//!   generated for the whole project, in the grid's display order (top first) so
//!   what the user is looking at fills in first. Interactive requests for a cell
//!   not yet generated are served immediately by the ThumbPool (LIFO) and are
//!   race-safe with this phase (temp-file+rename writes, idempotent upserts).
//!   Progress drives `thumbs:progress`/`thumbs:done` and `previews:progress`,
//!   all non-blocking indicators.
//!
//! Trade-off: exact ordering from the first frame means metadata is read before
//! any pixels, so a RAW container is parsed once here for metadata and reopened
//! in Phase B for its embedded preview — one extra cheap `get_decoder` per RAW,
//! in the background, invisible to time-to-interactive. (The old fused pass did
//! one read/parse per file but could not release the grid until every thumbnail
//! was done.)

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Instant;

use rayon::prelude::*;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::db::Db;
use crate::decode;
use crate::error::AppResult;
use crate::store::ProjectStore;
use crate::thumbs::{CacheVersion, ThumbKind, ThumbPool, ThumbRequest};

/// Files handled per parallel burst; also the metadata write-batch size.
/// Smaller on Android to bound the number of in-flight decode buffers.
const CHUNK: usize = if cfg!(target_os = "android") { 8 } else { 32 };

/// Minimum gap between metadata progress emits. Progress is counted per file
/// (inside the parallel burst); this throttle coalesces the emits so a fast
/// parse can't flood IPC. The final item always emits regardless.
const PROGRESS_THROTTLE_MS: u64 = 30;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    done: usize,
    total: usize,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct MetadataDone {
    updated: usize,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ThumbsDone {
    total: usize,
}

/// A file whose metadata still needs reading (Phase A).
struct MetaPending {
    id: i64,
    rel_path: String,
    kind: i64,
}

/// EXIF fields extracted during Phase A, batched to the writer thread.
struct Extracted {
    id: i64,
    capture_time: Option<i64>,
    orientation: Option<u16>,
    camera: Option<String>,
    lens: Option<String>,
    iso: Option<u32>,
    focal_length: Option<f32>,
    f_number: Option<f32>,
    exposure_time: Option<f32>,
    width: Option<u32>,
    height: Option<u32>,
}

impl Extracted {
    fn empty(id: i64) -> Self {
        Extracted {
            id,
            capture_time: None,
            orientation: None,
            camera: None,
            lens: None,
            iso: None,
            focal_length: None,
            f_number: None,
            exposure_time: None,
            width: None,
            height: None,
        }
    }
}

/// Event-emitting entry point used by the scanner thread.
pub fn run_ingest_pass(
    app: &AppHandle,
    db: &Arc<Db>,
    store: &dyn ProjectStore,
    thumbs: &ThumbPool,
) -> AppResult<()> {
    use tauri::Manager;
    // Whether to pregenerate video posters at all (they always run last). The
    // frontend pushes this from its localStorage setting; default on.
    let generate_videos = app
        .state::<crate::AppState>()
        .generate_video_thumbs
        .load(std::sync::atomic::Ordering::Relaxed);

    run_ingest_inner(
        db,
        store,
        thumbs,
        // Metadata progress is reported per file from inside the parallel burst,
        // so it must be Sync; `app.emit` already is.
        &|done, total| {
            let _ = app.emit("metadata:progress", Progress { done, total });
        },
        &mut |updated| {
            let _ = app.emit("metadata:done", MetadataDone { updated });
        },
        &mut |done, total| {
            let _ = app.emit("thumbs:progress", Progress { done, total });
            if done >= total {
                let _ = app.emit("thumbs:done", ThumbsDone { total });
            }
        },
        &mut |done, total| {
            let _ = app.emit("previews:progress", Progress { done, total });
        },
        &mut |done, total| {
            let _ = app.emit("videos:progress", Progress { done, total });
        },
        generate_videos,
    )
}

/// Testable core: closures instead of an AppHandle.
///
/// `meta_done(updated)` fires between the phases — that is the moment the
/// preload gate should release and the grid should show.
#[allow(clippy::too_many_arguments)]
pub fn run_ingest_inner(
    db: &Arc<Db>,
    store: &dyn ProjectStore,
    thumbs: &ThumbPool,
    // Sync (not FnMut): metadata progress is reported per file from inside the
    // parallel burst, so this may be called concurrently from rayon workers.
    meta_progress: &(dyn Fn(usize, usize) + Sync),
    meta_done: &mut dyn FnMut(usize),
    thumb_progress: &mut dyn FnMut(usize, usize),
    preview_progress: &mut dyn FnMut(usize, usize),
    video_progress: &mut dyn FnMut(usize, usize),
    generate_videos: bool,
) -> AppResult<()> {
    // --- Phase A: metadata, gated ---
    let pending: Vec<MetaPending> = db.call(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, rel_path, kind FROM files
             WHERE status = 0 AND kind IN (0, 1, 2) AND capture_time IS NULL",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(MetaPending {
                id: r.get(0)?,
                rel_path: r.get(1)?,
                kind: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })?;

    let started = Instant::now();
    let total = pending.len();
    // Announce the total up front so the preload panel shows `0 / N` immediately
    // instead of `0 / ?` for the whole first-chunk window.
    meta_progress(0, total);

    let mut meta_updated = 0usize;
    let done = AtomicUsize::new(0);
    let last_emit_ms = AtomicU64::new(0);
    for chunk in pending.chunks(CHUNK) {
        let extracted: Vec<Extracted> = chunk
            .par_iter()
            .map(|p| {
                let e = extract_metadata(store, p);
                // Advance the shared counter and emit — throttled so a fast parse
                // can't flood IPC, but the last file always reports so the bar
                // reaches `total / total`.
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                let now = started.elapsed().as_millis() as u64;
                let emit = d == total || {
                    let prev = last_emit_ms.load(Ordering::Relaxed);
                    now.saturating_sub(prev) >= PROGRESS_THROTTLE_MS
                        && last_emit_ms
                            .compare_exchange(prev, now, Ordering::Relaxed, Ordering::Relaxed)
                            .is_ok()
                };
                if emit {
                    meta_progress(done.load(Ordering::Relaxed), total);
                }
                e
            })
            .collect();
        meta_updated += extracted.len();
        write_metadata_batch(db, extracted)?;
    }
    tracing::info!(
        "ingest metadata: {total} files in {:.1?}",
        started.elapsed()
    );
    meta_done(meta_updated);

    // --- Phase B: photos first, videos last, all background (gate already open) ---
    // Submitted to the shared ThumbPool as *background* work: interactive
    // requests (the cells/photo the user is looking at) always preempt it, and
    // there is no second thread pool to oversubscribe the CPU.
    //
    // Order matters: photo grid thumbnails, then photo loupe previews, and only
    // THEN video poster thumbnails. Video posters go through ffmpeg (a separate
    // process spawn + frame extraction per file) — much slower per item than a
    // JPEG/RAW decode — so deferring them keeps the whole photo library sharp and
    // browsable before the video tier starts competing for the pool.
    generate_pass(
        db,
        thumbs,
        ThumbKind::Thumb,
        "SELECT f.id, f.mtime, f.orientation
         FROM files f
         LEFT JOIN thumbnails tt ON tt.file_id = f.id AND tt.kind = 0
         WHERE f.status = 0 AND f.kind IN (0, 1)
           AND (tt.file_id IS NULL OR tt.source_mtime <> f.mtime)
         ORDER BY COALESCE(f.capture_time, f.mtime) ASC, f.rel_path ASC",
        thumb_progress,
    )?;

    generate_pass(
        db,
        thumbs,
        ThumbKind::Preview,
        // Previews (2560px loupe) are only ever generated for stills; a video's
        // loupe plays the file itself.
        "SELECT f.id, f.mtime, f.orientation
         FROM files f
         LEFT JOIN thumbnails tp ON tp.file_id = f.id AND tp.kind = 1
         WHERE f.status = 0 AND f.kind IN (0, 1)
           AND (tp.file_id IS NULL OR tp.source_mtime <> f.mtime)
         ORDER BY COALESCE(f.capture_time, f.mtime) ASC, f.rel_path ASC",
        preview_progress,
    )?;

    // Video poster thumbnails last, and only when enabled (the ffmpeg tier is the
    // slow one — see AppState::generate_video_thumbs). Skipping here only skips
    // *pregeneration*; a poster is still produced on demand when a video's cell
    // scrolls into view.
    if generate_videos {
        generate_pass(
            db,
            thumbs,
            ThumbKind::Thumb,
            "SELECT f.id, f.mtime, f.orientation
             FROM files f
             LEFT JOIN thumbnails tt ON tt.file_id = f.id AND tt.kind = 0
             WHERE f.status = 0 AND f.kind = 2
               AND (tt.file_id IS NULL OR tt.source_mtime <> f.mtime)
             ORDER BY COALESCE(f.capture_time, f.mtime) ASC, f.rel_path ASC",
            video_progress,
        )?;
    }

    Ok(())
}

/// One background artifact tier: select the still-missing files (in grid order)
/// and submit them to the ThumbPool as background work. The pool's worker calls
/// `produce_with_mtime`, which short-circuits on the disk cache and is race-safe
/// with interactive requests. Tombstoned/undecodable files are filtered out by
/// the caller's query (a `failed = 1` row matches the same-mtime join), so they
/// aren't retried.
///
/// Blocks on a completion channel until every submission finishes (or is drained
/// by a pool shutdown), reporting progress one file at a time — throttled so a
/// burst of disk-cache hits can't flood IPC, but the last file always reports.
fn generate_pass(
    db: &Arc<Db>,
    thumbs: &ThumbPool,
    kind: ThumbKind,
    select_sql: &str,
    progress: &mut dyn FnMut(usize, usize),
) -> AppResult<()> {
    let select = select_sql.to_string();
    let pending: Vec<(i64, CacheVersion)> = db.call(move |conn| {
        let mut stmt = conn.prepare(&select)?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get(0)?,
                CacheVersion {
                    mtime: r.get(1)?,
                    orientation: r.get::<_, Option<i64>>(2)?.unwrap_or(1),
                },
            ))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })?;

    let total = pending.len();
    progress(0, total);
    if total == 0 {
        return Ok(());
    }

    let started = Instant::now();
    let (tx, rx) = mpsc::channel::<()>();
    for (file_id, version) in pending {
        let tx = tx.clone();
        thumbs.enqueue_background(ThumbRequest {
            file_id,
            kind,
            known_version: Some(version),
            // The disk cache is the point; the bytes are discarded here.
            respond: Box::new(move |_| {
                let _ = tx.send(());
            }),
        });
    }
    drop(tx);

    let mut done = 0usize;
    let mut last_emit_ms = 0u64;
    while rx.recv().is_ok() {
        done += 1;
        let now = started.elapsed().as_millis() as u64;
        if done == total || now.saturating_sub(last_emit_ms) >= PROGRESS_THROTTLE_MS {
            last_emit_ms = now;
            progress(done, total);
        }
    }
    tracing::info!("ingest {total} artifacts in {:.1?}", started.elapsed());
    Ok(())
}

/// Read one file's metadata (Phase A). Opens the source once and parses the
/// container once. Failures are logged and never abort the pass; the returned
/// [`Extracted`] always carries at least the id, so the batched COALESCE update
/// falls capture_time back to mtime and unreadable files aren't retried forever.
fn extract_metadata(store: &dyn ProjectStore, p: &MetaPending) -> Extracted {
    let mut e = Extracted::empty(p.id);

    // Videos carry no image-path EXIF; capture_time falls back to mtime via the
    // batched COALESCE update, no file read needed.
    if p.kind == 2 {
        return e;
    }

    let source = match decode::open_source(store, &p.rel_path) {
        Ok(s) => s,
        Err(err) => {
            tracing::debug!("metadata could not open {}: {err}", p.rel_path);
            return e; // mtime fallback still applies
        }
    };

    if p.kind == 0 {
        match decode::raw::RawSession::open(&source) {
            Ok(session) => {
                if let Ok(meta) = session.metadata(&p.rel_path) {
                    e.capture_time = meta.capture_time;
                    e.orientation = meta.orientation;
                    e.camera = meta.camera;
                    e.lens = meta.lens;
                    e.iso = meta.iso;
                    e.focal_length = meta.focal_length;
                    e.f_number = meta.f_number;
                    e.exposure_time = meta.exposure_time;
                }
            }
            Err(err) => tracing::debug!("metadata could not parse {}: {err}", p.rel_path),
        }
    } else if let Ok(meta) = decode::exif::read_metadata(source.buf()) {
        e.capture_time = meta.capture_time;
        e.orientation = meta.orientation;
        e.camera = meta.camera;
        e.lens = meta.lens;
        e.iso = meta.iso;
        e.focal_length = meta.focal_length;
        e.f_number = meta.f_number;
        e.exposure_time = meta.exposure_time;
        e.width = meta.width;
        e.height = meta.height;
    }

    e
}

/// Batched, transactional metadata update on the writer thread. Files whose
/// metadata was unreadable fall back to mtime so the pass never retries them
/// forever.
fn write_metadata_batch(db: &Arc<Db>, extracted: Vec<Extracted>) -> AppResult<()> {
    if extracted.is_empty() {
        return Ok(());
    }
    let batch: Vec<_> = extracted
        .iter()
        .map(|e| {
            (
                e.id,
                e.capture_time,
                e.orientation.map(i64::from),
                e.camera.clone(),
                e.lens.clone(),
                e.iso.map(i64::from),
                e.focal_length,
                e.f_number,
                e.exposure_time,
                e.width.map(i64::from),
                e.height.map(i64::from),
            )
        })
        .collect();
    db.call(move |conn| {
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "UPDATE files SET
                   capture_time = COALESCE(?2, mtime),
                   orientation = COALESCE(?3, orientation),
                   camera = COALESCE(?4, camera),
                   lens = COALESCE(?5, lens),
                   iso = COALESCE(?6, iso),
                   focal_length = COALESCE(?7, focal_length),
                   f_number = COALESCE(?8, f_number),
                   exposure_time = COALESCE(?9, exposure_time),
                   width = COALESCE(?10, width),
                   height = COALESCE(?11, height)
                 WHERE id = ?1",
            )?;
            for row in &batch {
                stmt.execute(rusqlite::params![
                    row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9, row.10
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::LocalFsStore;
    use rusqlite::params;
    use std::path::Path;

    fn project_with_jpegs(root: &Path, n: u32) -> Arc<Db> {
        for i in 0..n {
            let img = image::RgbImage::from_fn(640, 480, move |x, _| {
                image::Rgb([(x % 255) as u8, (i * 40) as u8, 90])
            });
            img.save(root.join(format!("photo{i}.jpg"))).unwrap();
        }
        let db = Arc::new(crate::db::Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        db
    }

    fn thumb_count(db: &Arc<Db>, kind: i64) -> i64 {
        db.call(move |c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM thumbnails WHERE kind = ?1",
                params![kind],
                |r| r.get(0),
            )?)
        })
        .unwrap()
    }

    /// A shared store + its ThumbPool (Phase B routes background work through it).
    fn store_and_pool(db: &Arc<Db>, root: &Path) -> (Arc<dyn ProjectStore>, ThumbPool) {
        let store: Arc<dyn ProjectStore> = Arc::new(LocalFsStore::new(root));
        let pool = ThumbPool::start(db.clone(), store.clone(), root.to_path_buf());
        (store, pool)
    }

    /// Run the whole pass, returning `(meta_updated, thumbs_done, previews_done)`.
    fn run_all(db: &Arc<Db>, root: &Path) -> (usize, usize, usize) {
        let (store, pool) = store_and_pool(db, root);
        let mut meta = 0usize;
        let mut thumbs = 0usize;
        let mut previews = 0usize;
        run_ingest_inner(
            db,
            store.as_ref(),
            &pool,
            &|_, _| {},
            &mut |u| meta = u,
            &mut |d, _| thumbs = d,
            &mut |d, _| previews = d,
            &mut |_, _| {},
            true,
        )
        .unwrap();
        (meta, thumbs, previews)
    }

    #[test]
    fn metadata_gate_fires_before_any_thumbnail() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let db = project_with_jpegs(root, 3);
        let (store, pool) = store_and_pool(&db, root);

        // Capture the thumbnail count at the exact moment the gate releases.
        let mut gate = None;
        run_ingest_inner(
            &db,
            store.as_ref(),
            &pool,
            &|_, _| {},
            &mut |updated| gate = Some((updated, thumb_count(&db, 0))),
            &mut |_, _| {},
            &mut |_, _| {},
            &mut |_, _| {},
            false,
        )
        .unwrap();
        // Metadata written for all three; no grid thumbnail exists yet.
        assert_eq!(gate, Some((3, 0)));
    }

    #[test]
    fn background_phase_generates_thumbs_then_previews() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let db = project_with_jpegs(root, 3);

        let (meta, thumbs, previews) = run_all(&db, root);
        assert_eq!(meta, 3);
        assert_eq!(thumbs, 3);
        assert_eq!(previews, 3);
        assert_eq!(thumb_count(&db, 0), 3);
        assert_eq!(thumb_count(&db, 1), 3);

        // Synthetic JPEGs carry no EXIF: capture_time must fall back to mtime.
        let missing: i64 = db
            .call(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM files WHERE capture_time IS NULL",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(missing, 0);
    }

    #[test]
    fn reopening_an_ingested_project_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let db = project_with_jpegs(root, 3);

        run_all(&db, root);
        // Everything fresh: a second pass finds nothing to do.
        assert_eq!(run_all(&db, root), (0, 0, 0));
    }

    #[test]
    fn exif_metadata_flows_through_ingest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // 800x600 landscape with EXIF: known capture time, rotation 6 (90 CW),
        // camera and ISO. The value 1718461805 is pinned by the exif unit test.
        let img =
            image::RgbImage::from_fn(800, 600, |x, _| image::Rgb([((x * 255) / 800) as u8; 3]));
        let bytes = crate::bench::jpeg_with_exif(
            &img,
            &crate::bench::SyntheticExif {
                date_time_original: "2024:06:15 14:30:05",
                orientation: 6,
                make: "Canon",
                model: "EOS R5",
                iso: 400,
            },
        );
        std::fs::write(root.join("photo.jpg"), bytes).unwrap();

        let db = Arc::new(crate::db::Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        run_all(&db, root);

        let (capture, orientation, camera, iso): (i64, i64, String, i64) = db
            .call(|c| {
                Ok(c.query_row(
                    "SELECT capture_time, orientation, camera, iso FROM files",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?)
            })
            .unwrap();
        assert_eq!(
            capture, 1718461805,
            "capture_time must come from EXIF, not mtime"
        );
        assert_eq!(orientation, 6);
        assert_eq!(camera, "Canon EOS R5");
        assert_eq!(iso, 400);

        // The metadata orientation (written in Phase A) must reach the Phase B
        // render: landscape 800x600 + rotation 6 = portrait 288x384 thumbnail.
        let (w, h): (i64, i64) = db
            .call(|c| {
                Ok(c.query_row(
                    "SELECT width, height FROM thumbnails WHERE kind = 0",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!((w, h), (288, 384));
    }

    #[test]
    fn undecodable_source_is_tombstoned_not_retried() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // A .jpg extension over bytes that are not a valid JPEG.
        std::fs::write(root.join("broken.jpg"), b"not really a jpeg at all").unwrap();

        let db = Arc::new(crate::db::Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();

        // First pass: metadata falls back to mtime, the thumbnail fails to decode
        // in Phase B and gets tombstoned.
        run_all(&db, root);
        let tombstones: i64 = db
            .call(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM thumbnails WHERE failed = 1",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert!(tombstones >= 1, "a decode failure must leave a tombstone");

        // Second pass at the same mtime: nothing to retry.
        assert_eq!(run_all(&db, root), (0, 0, 0));
    }

    #[test]
    fn video_is_ingested_without_preview_and_never_panics() {
        // Exercises the video branch end-to-end. The .mp4 bytes are not a real
        // video, so the outcome depends on whether ffmpeg is installed:
        //   - ffmpeg present: extraction fails -> thumbnail tombstoned;
        //   - ffmpeg absent:  extraction skipped -> no thumbnail, no tombstone.
        // Either way the pass must complete, the video must get capture_time from
        // mtime, and never receive a preview.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("clip.mp4"), b"not a real mp4").unwrap();

        let db = Arc::new(crate::db::Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();

        let (meta, _thumbs, previews) = run_all(&db, root);
        assert_eq!(meta, 1);
        // Videos never get a 2560px preview, in any case.
        assert_eq!(previews, 0);
        assert_eq!(thumb_count(&db, 1), 0);

        // capture_time falls back to mtime so the video isn't reprocessed forever.
        let null_capture: i64 = db
            .call(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM files WHERE capture_time IS NULL",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(null_capture, 0);
    }
}
