//! Two-phase ingest. Nothing here gates the grid — that appears as soon as the
//! walk reports back, ordered by mtime — so both phases are free to be ordered
//! by what the user gets soonest rather than by what unblocks them.
//!
//! - **Phase A — metadata.** EXIF/RAW metadata (capture_time, orientation,
//!   camera, …) for every file that lacks it. The grid re-sorts by capture time
//!   when it finishes (`metadata:done`); on a camera card mtime order already
//!   matches, so the resort is near-identity. Progress drives
//!   `metadata:progress`.
//! - **Phase B — thumbnails then previews.** The 384px grid thumbnails and then
//!   the loupe previews, in the grid's display order (top first) so what the
//!   user is looking at fills in first. Interactive requests for a cell not yet
//!   generated are served immediately by the ThumbPool (LIFO) and are race-safe
//!   with this phase (temp-file+rename writes, idempotent upserts). Progress
//!   drives `thumbs:progress`/`thumbs:done` and `previews:progress`.
//!
//! Both phases are organised around a single fact: a file that cannot be
//! memory-mapped must be read whole, and on Android SAF *no* file can be
//! memory-mapped. So the ingest reads as few bytes as it can get away with:
//!
//! - **A RAW+JPEG pair reads only the JPEG.** Both halves take their metadata
//!   from that one read (a camera writes the same EXIF into both), and the
//!   pair's grid thumbnail and loupe preview are rendered from it too — the RAW
//!   fast path renders its *embedded* JPEG anyway, so the two are the same
//!   frame. The RAW itself is opened only when someone zooms in.
//! - **Only what the grid shows is pregenerated.** In mirror mode a live pair is
//!   one cell, so the other half is left to be generated on demand if it is ever
//!   displayed.
//!
//! Together those take a 500-pair shoot from six file reads per pair to one.

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

/// Photos whose grid thumbnail is generated before the fused pass starts.
/// Roughly the first two or three screens: enough that the grid stops looking
/// empty immediately, few enough that paying a second read for them is noise.
const LEAD_WINDOW: usize = 60;

/// Minimum gap between metadata progress emits. Progress is counted per file
/// (inside the parallel burst); this throttle coalesces the emits so a fast
/// parse can't flood IPC. The final item always emits regardless.
///
/// A progress bar does not need more than a few updates a second, and on Android
/// every emit is an `evaluateJavascript` job on the main-thread looper — the same
/// one every SAF file open is queued on, so the emits compete directly with the
/// work they are reporting.
const PROGRESS_THROTTLE_MS: u64 = if cfg!(target_os = "android") {
    250
} else {
    100
};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    project_root: String,
    done: usize,
    total: usize,
    /// Files finished since the previous emit. Lets the frontend learn which
    /// previews exist without re-querying the whole catalogue on a timer.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    ids: Vec<i64>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct MetadataDone {
    project_root: String,
    updated: usize,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ThumbsDone {
    project_root: String,
    total: usize,
}

/// One row of the Phase-A selection: the file, plus the siblings whose read it
/// could take instead of its own. Both siblings are live members of the same
/// coupled group.
struct MetaRow {
    id: i64,
    kind: i64,
    rel_path: String,
    /// The decodable image half — what a RAW borrows, because it is the same
    /// frame for a fraction of the bytes.
    image_sibling: Option<(i64, String)>,
}

/// One source file to open, and every row that takes its metadata from that one
/// read (Phase A).
///
/// A RAW that is paired with a JPEG reads the JPEG: a camera writes the same
/// capture time, camera, lens and exposure into both halves, and the JPEG is a
/// fraction of the bytes -- decisive where a file cannot be memory-mapped and
/// must be read whole, which is every file on Android SAF. So a RAW+JPEG pair
/// costs one read of the small half instead of one of each.
struct MetaWork {
    source_rel: String,
    source_kind: i64,
    /// The row that IS this file, when it needs metadata too. Only this one
    /// takes the decoded pixel dimensions; a RAW's own dimensions are the
    /// sensor's and are backfilled later if it is ever decoded.
    source_id: Option<i64>,
    /// Rows borrowing this read. Empty for an unpaired file.
    borrowers: Vec<(i64, String)>,
}

/// EXIF fields extracted during Phase A, batched to the writer thread.
#[derive(Clone)]
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
    video_codec: Option<String>,
    video_frame_rate: Option<f64>,
    video_duration: Option<f64>,
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
            video_codec: None,
            video_frame_rate: None,
            video_duration: None,
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

    let project_root = db.project_root();
    let thumb_project_root = project_root.clone();
    run_ingest_inner(
        db,
        store,
        thumbs,
        // Metadata progress is reported per file from inside the parallel burst,
        // so it must be Sync; `app.emit` already is.
        &|done, total| {
            let _ = app.emit(
                "metadata:progress",
                Progress {
                    project_root: project_root.clone(),
                    done,
                    total,
                    ids: Vec::new(),
                },
            );
        },
        &mut |updated| {
            let _ = app.emit(
                "metadata:done",
                MetadataDone {
                    project_root: project_root.clone(),
                    updated,
                },
            );
        },
        // `done >= total` also holds for an empty pass (0 >= 0), so reopening a
        // project with nothing left to do announced the phase complete on every
        // emit. Latch it: one `thumbs:done` per pass, whatever the totals.
        &mut {
            let mut announced = false;
            move |done, total, _ids: &[i64]| {
                let _ = app.emit(
                    "thumbs:progress",
                    Progress {
                        project_root: thumb_project_root.clone(),
                        done,
                        total,
                        ids: Vec::new(),
                    },
                );
                if done >= total && !announced {
                    announced = true;
                    let _ = app.emit(
                        "thumbs:done",
                        ThumbsDone {
                            project_root: thumb_project_root.clone(),
                            total,
                        },
                    );
                }
            }
        },
        &mut |done, total, ids: &[i64]| {
            let _ = app.emit(
                "previews:progress",
                Progress {
                    project_root: project_root.clone(),
                    done,
                    total,
                    ids: ids.to_vec(),
                },
            );
        },
        &mut |done, total, _ids| {
            let _ = app.emit(
                "videos:progress",
                Progress {
                    project_root: project_root.clone(),
                    done,
                    total,
                    ids: Vec::new(),
                },
            );
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
    thumb_progress: &mut dyn FnMut(usize, usize, &[i64]),
    preview_progress: &mut dyn FnMut(usize, usize, &[i64]),
    video_progress: &mut dyn FnMut(usize, usize, &[i64]),
    generate_videos: bool,
) -> AppResult<()> {
    let profile_metadata = crate::photo_profile::span("metadata");
    crate::photo_profile::report("metadata_start");
    // Each row reports the file itself plus, for a RAW in a live pair, its JPEG
    // sibling. Grouping by whichever file will actually be opened turns a pair
    // into a single read.
    //
    // The sibling must be a decodable image, not merely `kind = 1`: a RAW that
    // borrowed the HEIF half of a RAW+HEIF shot would get no pixels to read a
    // header from, and the point of the borrow is to read the cheaper file.
    let video_metadata_available =
        store.extracts_video_posters() || decode::video::ffprobe_available();
    let rows: Vec<MetaRow> = db.call_read(move |conn| {
        let mut stmt = conn.prepare(&format!(
            "SELECT f.id, f.kind, f.rel_path,
                    (SELECT s.id FROM files s
                      WHERE s.group_id = f.group_id AND s.kind = 1 AND s.status = 0
                        AND s.ext IN ({exts}) AND g.decoupled = 0 LIMIT 1),
                    (SELECT s.rel_path FROM files s
                      WHERE s.group_id = f.group_id AND s.kind = 1 AND s.status = 0
                        AND s.ext IN ({exts}) AND g.decoupled = 0 LIMIT 1)
             FROM files f
             JOIN groups g ON g.id = f.group_id
             WHERE f.status = 0 AND f.kind IN (0, 1, 2)
               AND (f.capture_time IS NULL OR
                    (?1 AND f.kind = 2 AND f.video_codec IS NULL
                     AND NOT EXISTS(SELECT 1 FROM thumbnails t
                       WHERE t.file_id=f.id AND t.failed=1 AND t.source_mtime=f.mtime)))",
            exts = crate::db::sql::image_exts()
        ))?;
        let rows = stmt.query_map([video_metadata_available], |r| {
            Ok(MetaRow {
                id: r.get(0)?,
                kind: r.get(1)?,
                rel_path: r.get(2)?,
                image_sibling: r.get::<_, Option<i64>>(3)?.zip(r.get(4)?),
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })?;

    let mut by_source: std::collections::HashMap<String, MetaWork> =
        std::collections::HashMap::new();

    for row in rows {
        // A RAW with a live JPEG sibling borrows that read: the camera wrote the
        // same capture time, camera, lens and exposure into both, and the JPEG is
        // a fraction of the bytes. Everything else is its own source — including
        // a HEIF, which carries its own EXIF and is read for it like any photo.
        //
        // A companion RAW borrows nothing. It is the frame *before* the camera
        // composited it, so the sibling JPEG's EXIF describes a different
        // exposure — a Live Composite `.ORI` would report the composite's 60 s
        // instead of its own 2 s, and the shutter-speed filter would repeat the
        // lie. `PAIRED_JPEG_SQL` refuses the same borrow for the same reason.
        let from_image = row
            .image_sibling
            .filter(|(sid, _)| *sid != row.id)
            .map(|(_, rel)| (rel, 1));
        let companion_raw = crate::decode::ext_of(&row.rel_path)
            .is_some_and(|e| crate::decode::SECONDARY_RAW_EXTS.contains(&e.as_str()));
        let borrow = if row.kind == 0 && !companion_raw {
            from_image
        } else {
            None
        };
        match borrow {
            Some((source_rel, source_kind)) => {
                by_source
                    .entry(source_rel.clone())
                    .or_insert_with(|| MetaWork {
                        source_rel,
                        source_kind,
                        source_id: None,
                        borrowers: Vec::new(),
                    })
                    .borrowers
                    .push((row.id, row.rel_path));
            }
            None => {
                let entry = by_source
                    .entry(row.rel_path.clone())
                    .or_insert_with(|| MetaWork {
                        source_rel: row.rel_path,
                        source_kind: row.kind,
                        source_id: None,
                        borrowers: Vec::new(),
                    });
                entry.source_id = Some(row.id);
                entry.source_kind = row.kind;
            }
        }
    }
    let pending: Vec<MetaWork> = by_source.into_values().collect();

    let started = Instant::now();
    // Counted in reads, not files: a RAW+JPEG pair is one read, and reporting
    // what the loop actually advances keeps the bar honest.
    let total = pending.len();
    // Announce the total up front so the preload panel shows `0 / N` immediately
    // instead of `0 / ?` for the whole first-chunk window.
    meta_progress(0, total);
    crate::store::stats::reset();

    let mut meta_updated = 0usize;
    let done = AtomicUsize::new(0);
    let last_emit_ms = AtomicU64::new(0);
    for chunk in pending.chunks(CHUNK) {
        let extracted: Vec<Extracted> = chunk
            .par_iter()
            .flat_map(|p| {
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
        "ingest metadata: {total} files in {:.1?} [{}]",
        started.elapsed(),
        crate::store::stats::report()
    );
    drop(profile_metadata);
    crate::photo_profile::report("metadata_complete");
    meta_done(meta_updated);

    // Submitted to the shared ThumbPool as *background* work: interactive
    // requests (the cells/photo the user is looking at) always preempt it, and
    // there is no second thread pool to oversubscribe the CPU.
    //
    // Three passes, in this order for a reason:
    //
    // 1. A short thumbnails-only lead, so the first screens of the grid fill
    //    within a second instead of waiting behind full-size preview decodes.
    // 2. The fused pass: one read and one decode per photo, producing the loupe
    //    preview AND the grid thumbnail. This is where the whole library gets
    //    done, at one read each instead of two.
    // 3. A thumbnails straggler pass, normally empty -- it only catches photos
    //    whose preview was already cached (so the fused pass short-circuited
    //    before rendering anything) but whose thumbnail was not.
    //
    // Video posters come last: extracting one spins up a whole video decoder per
    // file (an ffmpeg process on desktop, a platform decoder on Android), so
    // deferring them keeps the photo library browsable before that tier competes.
    let decodable = crate::db::sql::decodable_photo("f", decode::heif::available(store));
    let thumb_sql = |extra: &str| {
        format!(
            "SELECT f.id, f.mtime, f.orientation
             FROM files f
             JOIN groups g ON g.id = f.group_id
             LEFT JOIN thumbnails tt ON tt.file_id = f.id AND tt.kind = 0
             WHERE f.status = 0 AND f.kind IN (0, 1) AND {decodable}
           AND (g.primary_file_id = f.id OR g.decoupled = 1)
               AND (tt.file_id IS NULL OR tt.source_mtime <> f.mtime)
             ORDER BY COALESCE(f.capture_time, f.mtime) ASC, f.rel_path ASC
             {extra}"
        )
    };

    // 1. Lead window. Small on purpose: every file here is read twice (once for
    //    its thumbnail, once by the fused pass for its preview), which is a fine
    //    price for two or three screens and a bad one for a whole library.
    generate_pass(
        db,
        thumbs,
        ThumbKind::Thumb,
        false,
        &thumb_sql(&format!("LIMIT {LEAD_WINDOW}")),
        thumb_progress,
    )?;

    // 2. The fused pass. Previews are only ever generated for stills; a video's
    //    loupe plays the file itself. A preview is stale when it was generated
    //    for a different long edge than the one now configured; `long_edge = 0`
    //    means "generated before the column existed" and counts as matching, so
    //    upgrading never regrinds a library that is perfectly fine.
    generate_pass(
        db,
        thumbs,
        ThumbKind::Preview,
        true,
        &format!(
            "SELECT f.id, f.mtime, f.orientation
             FROM files f
             JOIN groups g ON g.id = f.group_id
             LEFT JOIN thumbnails tp ON tp.file_id = f.id AND tp.kind = 1
             WHERE f.status = 0 AND f.kind IN (0, 1) AND {decodable}
           AND (g.primary_file_id = f.id OR g.decoupled = 1)
               AND (tp.file_id IS NULL OR tp.source_mtime <> f.mtime
                    OR (tp.long_edge <> 0 AND tp.long_edge <> {}))
             ORDER BY COALESCE(f.capture_time, f.mtime) ASC, f.rel_path ASC",
            crate::thumbs::preview_long_edge()
        ),
        preview_progress,
    )?;

    generate_pass(
        db,
        thumbs,
        ThumbKind::Thumb,
        false,
        &thumb_sql(""),
        thumb_progress,
    )?;

    crate::photo_profile::report("photos_complete");
    // Video poster thumbnails last, and only when enabled (the poster tier is the
    // slow one — see AppState::generate_video_thumbs). Skipping here only skips
    // *pregeneration*; a poster is still produced on demand when a video's cell
    // scrolls into view.
    if generate_videos {
        generate_pass(
            db,
            thumbs,
            ThumbKind::Thumb,
            false,
            "SELECT f.id, f.mtime, f.orientation
             FROM files f
             LEFT JOIN thumbnails tt ON tt.file_id = f.id AND tt.kind = 0
             WHERE f.status = 0 AND f.kind = 2
               AND (tt.file_id IS NULL OR tt.source_mtime <> f.mtime)
             ORDER BY COALESCE(f.capture_time, f.mtime) ASC, f.rel_path ASC",
            video_progress,
        )?;
    }

    crate::photo_profile::report("ingest_complete");
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
    also_thumb: bool,
    select_sql: &str,
    progress: &mut dyn FnMut(usize, usize, &[i64]),
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
    crate::photo_profile::report(match kind {
        ThumbKind::Preview => "preview_pass_start",
        _ => "thumb_pass_start",
    });

    progress(0, total, &[]);
    if total == 0 {
        return Ok(());
    }

    let _profile = crate::photo_profile::span(match kind {
        ThumbKind::Preview => "pass.preview",
        _ => "pass.thumb",
    });
    let started = Instant::now();
    // The channel carries which file finished, so the emit can tell the frontend
    // exactly what became available rather than making it re-read the catalogue.
    let (tx, rx) = mpsc::channel::<(i64, bool)>();
    // Submitted in one batch: one lock and one wake for the whole tier, rather
    // than both per file for the entire library.
    let requests: Vec<ThumbRequest> = pending
        .into_iter()
        .map(|(file_id, version)| {
            let tx = tx.clone();
            ThumbRequest {
                file_id,
                kind,
                also_thumb,
                // The pass has no fast path of its own; the worker does the read.
                cache_checked: false,
                known_version: Some(version),
                // The disk cache is the point; the bytes are discarded here.
                respond: Box::new(move |result| {
                    let success = result.is_ok();
                    if let Err(error) = result {
                        tracing::debug!("background {kind:?} {file_id} failed: {error}");
                    }
                    let _ = tx.send((file_id, success));
                }),
            }
        })
        .collect();
    thumbs.enqueue_background_batch(requests);
    drop(tx);

    let mut done = 0usize;
    let mut last_emit_ms = 0u64;
    let mut since_emit: Vec<i64> = Vec::new();
    while let Ok((file_id, success)) = rx.recv() {
        done += 1;
        if success {
            since_emit.push(file_id);
        }
        let now = started.elapsed().as_millis() as u64;
        if done == total || now.saturating_sub(last_emit_ms) >= PROGRESS_THROTTLE_MS {
            last_emit_ms = now;
            progress(done, total, &since_emit);
            since_emit.clear();
        }
    }
    tracing::info!(
        "ingest {total} artifacts in {:.1?} [{}]",
        started.elapsed(),
        crate::store::stats::report()
    );
    Ok(())
}

/// Read one source file's metadata (Phase A) and hand it to every row that
/// takes it: itself, and any RAW paired with it. Opens and parses once.
///
/// Failures are logged and never abort the pass; every target still gets a row,
/// so the batched COALESCE update falls capture_time back to mtime and
/// unreadable files aren't retried forever.
fn extract_metadata(store: &dyn ProjectStore, work: &MetaWork) -> Vec<Extracted> {
    let blank = || metadata_rows(store, work, &Extracted::empty(0));

    // A bounded container probe can supply a video date; absent or invalid
    // metadata still falls back to mtime in the batched update.
    if work.source_kind == 2 {
        let mut e = Extracted::empty(0);
        if let Some(metadata) = decode::video::metadata(store, &work.source_rel) {
            e.capture_time = metadata.capture_time;
            // Posters and playback already apply the container rotation.
            let rotated = metadata
                .rotation
                .is_some_and(|rotation| rotation.rem_euclid(180) == 90);
            e.width = if rotated {
                metadata.height
            } else {
                metadata.width
            };
            e.height = if rotated {
                metadata.width
            } else {
                metadata.height
            };
            e.video_codec = metadata.video_codec;
            e.video_frame_rate = metadata.video_frame_rate;
            e.video_duration = metadata.video_duration;
        }
        return metadata_rows(store, work, &e);
    }

    let source = match decode::open_source(store, &work.source_rel) {
        Ok(s) => s,
        Err(err) => {
            tracing::debug!("metadata could not open {}: {err}", work.source_rel);
            return blank(); // mtime fallback still applies
        }
    };

    let mut e = Extracted::empty(0);
    if work.source_kind == 0 {
        match decode::raw::RawSession::open(&source) {
            Ok(session) => {
                if let Ok(meta) = session.metadata(&work.source_rel) {
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
            Err(err) => tracing::debug!("metadata could not parse {}: {err}", work.source_rel),
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

    metadata_rows(store, work, &e)
}

fn metadata_rows(
    store: &dyn ProjectStore,
    work: &MetaWork,
    extracted: &Extracted,
) -> Vec<Extracted> {
    let mut rows: Vec<_> = work
        .source_id
        .into_iter()
        .map(|id| {
            let mut row = extracted.clone();
            row.id = id;
            row
        })
        .collect();
    for (id, relative) in &work.borrowers {
        let mut row = if extracted.width.is_none() || extracted.height.is_none() {
            extract_metadata(
                store,
                &MetaWork {
                    source_rel: relative.clone(),
                    source_kind: 0,
                    source_id: Some(*id),
                    borrowers: Vec::new(),
                },
            )
            .pop()
            .unwrap_or_else(|| Extracted::empty(*id))
        } else {
            extracted.clone()
        };
        row.id = *id;
        row.width = None;
        row.height = None;
        rows.push(row);
    }
    rows
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
                e.video_codec.clone(),
                e.video_frame_rate,
                e.video_duration,
            )
        })
        .collect();
    db.call(move |conn| {
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "UPDATE files SET
                   capture_time = COALESCE(?2, mtime),
                   orientation = CASE WHEN orientation IS NULL OR
                     (COALESCE(state_updated_at, 0) = 0 AND xmp_source_mtime IS NULL)
                     THEN COALESCE(?3, orientation, 1) ELSE orientation END,
                   camera = COALESCE(?4, camera),
                   lens = COALESCE(?5, lens),
                   iso = COALESCE(?6, iso),
                   focal_length = COALESCE(?7, focal_length),
                   f_number = COALESCE(?8, f_number),
                   exposure_time = COALESCE(?9, exposure_time),
                   width = COALESCE(?10, width),
                   height = COALESCE(?11, height),
                   video_codec = ?12,
                   video_frame_rate = ?13,
                   video_duration = ?14
                 WHERE id = ?1",
            )?;
            for row in &batch {
                stmt.execute(rusqlite::params![
                    row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9, row.10,
                    row.11, row.12, row.13
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
    use std::sync::Mutex;

    #[test]
    fn media_ingest_regression_explicit_orientation_survives_metadata() {
        let _guard = ingest_guard();
        let dir = tempfile::tempdir().unwrap();
        let image = image::RgbImage::from_pixel(80, 60, image::Rgb([20, 80, 120]));
        let exif = crate::bench::SyntheticExif {
            date_time_original: "2024:06:15 14:30:05",
            orientation: 6,
            make: "Test",
            model: "Camera",
            iso: 100,
        };
        let jpeg = crate::bench::jpeg_with_exif(&image, &exif);
        std::fs::write(dir.path().join("imported.jpg"), &jpeg).unwrap();
        std::fs::write(dir.path().join("edited.jpg"), &jpeg).unwrap();
        std::fs::write(dir.path().join("imported.xmp"), r#"<rdf:Description xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:tiff="http://ns.adobe.com/tiff/1.0/" tiff:Orientation="8"/>"#).unwrap();
        let db = Arc::new(Db::open(dir.path()).unwrap());
        crate::scan::scan_project_inner(&db, dir.path(), &mut |_| {}).unwrap();
        let id: i64 = db
            .call_read(|c| {
                Ok(c.query_row(
                    "SELECT id FROM files WHERE rel_path='edited.jpg'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        crate::engine::culling::rotate(
            &db,
            crate::engine::culling::Targets {
                ids: vec![id],
                as_groups: false,
            },
            2,
        )
        .unwrap();
        run_all(&db, dir.path());
        let orientations: (i64, i64) = db.call_read(|c| Ok(c.query_row("SELECT (SELECT orientation FROM files WHERE rel_path='imported.jpg'),(SELECT orientation FROM files WHERE rel_path='edited.jpg')", [], |r| Ok((r.get(0)?,r.get(1)?)))?)).unwrap();
        assert_eq!(
            orientations,
            (8, 3),
            "EXIF cannot replace imported or edited rotation"
        );
    }

    #[test]
    fn media_ingest_regression_bad_sibling_falls_back_to_raw_metadata() {
        let _guard = ingest_guard();
        let dir = tempfile::tempdir().unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/media/Olympus - E-M1MarkII - 16bit (4-3).ORF");
        if !fixture.is_file() {
            eprintln!("skipped: local RAW corpus is not installed");
            return;
        }
        std::fs::copy(fixture, dir.path().join("shot.orf")).unwrap();
        std::fs::write(dir.path().join("shot.jpg"), b"corrupt JPEG sibling").unwrap();
        let db = Arc::new(Db::open(dir.path()).unwrap());
        crate::scan::scan_project_inner(&db, dir.path(), &mut |_| {}).unwrap();
        run_all(&db, dir.path());
        let (camera, capture, mtime): (Option<String>, i64, i64) = db
            .call_read(|c| {
                Ok(c.query_row(
                    "SELECT camera,capture_time,mtime FROM files WHERE kind=0",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?)
            })
            .unwrap();
        assert!(
            camera.is_some(),
            "A corrupt sibling must not remove readable RAW metadata"
        );
        assert_ne!(capture, mtime);
    }

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

    /// The configured preview size is global process state, so a test that
    /// changes it would otherwise restage another test's previews mid-run.
    /// Every test that runs a pass takes this first.
    static INGEST: Mutex<()> = Mutex::new(());

    fn ingest_guard() -> std::sync::MutexGuard<'static, ()> {
        INGEST.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Run the whole pass, returning `(meta_updated, thumbs_done, previews_done)`.
    ///
    /// Thumbnails are reported by more than one pass (the lead window and the
    /// straggler sweep), and each pass opens by reporting 0 — so bank the
    /// previous pass's final count whenever that happens, and sum them.
    fn run_all(db: &Arc<Db>, root: &Path) -> (usize, usize, usize) {
        let (store, pool) = store_and_pool(db, root);
        let mut meta = 0usize;
        let mut thumbs = 0usize;
        let mut thumb_pass = 0usize;
        let mut previews = 0usize;
        run_ingest_inner(
            db,
            store.as_ref(),
            &pool,
            &|_, _| {},
            &mut |u| meta = u,
            &mut |d, _, _| {
                if d == 0 {
                    thumbs += thumb_pass;
                    thumb_pass = 0;
                } else {
                    thumb_pass = d;
                }
            },
            &mut |d, _, _| previews = d,
            &mut |_, _, _| {},
            true,
        )
        .unwrap();
        (meta, thumbs + thumb_pass, previews)
    }

    #[test]
    fn metadata_gate_fires_before_any_thumbnail() {
        let _guard = ingest_guard();
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
            &mut |_, _, _| {},
            &mut |_, _, _| {},
            &mut |_, _, _| {},
            false,
        )
        .unwrap();
        // Metadata written for all three; no grid thumbnail exists yet.
        assert_eq!(gate, Some((3, 0)));
    }

    #[test]
    fn background_phase_generates_thumbs_then_previews() {
        let _guard = ingest_guard();
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
        let _guard = ingest_guard();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let db = project_with_jpegs(root, 3);

        run_all(&db, root);
        // Everything fresh: a second pass finds nothing to do.
        assert_eq!(run_all(&db, root), (0, 0, 0));
    }

    /// A HEIF-only library — every stock iPhone — is the case this exists for.
    /// Nothing can decode the pixels, so the cells stay empty; but the EXIF is
    /// reachable, so the photos must still carry their real capture time, camera
    /// and dimensions. Without that they sort by copy time, which on a freshly
    /// imported card is no order at all, and every filter facet is empty.
    #[test]
    fn a_heif_only_library_is_dated_and_described() {
        let _guard = ingest_guard();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let heif = crate::bench::heif_with_exif(
            &crate::bench::SyntheticExif {
                date_time_original: "2024:06:15 14:30:05",
                orientation: 1,
                make: "Apple",
                model: "iPhone 15 Pro",
                iso: 125,
            },
            (4032, 3024),
        );
        std::fs::write(root.join("IMG_0100.heic"), &heif).unwrap();

        let db = Arc::new(crate::db::Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        run_all(&db, root);

        let (capture, camera, iso, w, h): (i64, String, i64, i64, i64) = db
            .call(|c| {
                Ok(c.query_row(
                    "SELECT capture_time, camera, iso, width, height FROM files",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )?)
            })
            .unwrap();
        assert_eq!(capture, 1718461805, "the shot's own time, not its mtime");
        assert_eq!(camera, "Apple iPhone 15 Pro");
        assert_eq!(iso, 125);
        // No decoder can read a HEIF header, so these came from EXIF.
        assert_eq!((w, h), (4032, 3024));
    }

    /// The invariant the whole HEIF ladder rests on. When no rung can decode a
    /// container, that is "not now", never "undecodable" — so no tombstone is
    /// written. A tombstone is keyed on mtime, and installing a decoder changes
    /// no file's mtime, so one written here would leave the photo blank forever
    /// after the upgrade. Asserted for the grid cell and for the focus check,
    /// which reach the decode by different routes.
    #[test]
    fn a_heif_without_a_decoder_is_never_tombstoned() {
        let _guard = ingest_guard();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Contents are irrelevant: the refusal happens before the read.
        std::fs::write(root.join("IMG_0200.heic"), b"").unwrap();

        let db = Arc::new(crate::db::Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();

        crate::decode::heif::disable_for_test();
        let store = LocalFsStore::new(root);
        let id: i64 = db
            .call(|c| Ok(c.query_row("SELECT id FROM files", [], |r| r.get(0))?))
            .unwrap();
        for kind in [ThumbKind::Thumb, ThumbKind::Preview, ThumbKind::Full] {
            let out = crate::thumbs::produce_cached(
                &db,
                &store,
                root,
                crate::thumbs::Produce {
                    file_id: id,
                    kind,
                    known: None,
                    record_hit: false,
                    also_thumb: false,
                    cache_checked: false,
                },
            );
            assert!(out.is_err(), "{kind:?} must refuse without a decoder");
        }
        crate::decode::heif::reenable_for_test();

        let tombstones: i64 = db
            .call(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM thumbnails WHERE failed = 1",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(tombstones, 0, "a refusal must not outlive the decoder");
    }

    /// Encode `src` as HEVC in a HEIF-branded container, using the machine's own
    /// ffmpeg. `None` when it has none, or none that can encode HEVC — no
    /// encoder in this build can produce a HEIF, so the only alternative would be
    /// checking a camera file into the repo.
    fn ffmpeg_made_heif(src: &Path, dest: &Path) -> Option<()> {
        let out = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(src)
            .args([
                "-c:v", "libx265", "-tag:v", "hvc1", "-f", "mp4", "-brand", "mif1",
            ])
            .arg(dest)
            .stdin(std::process::Stdio::null())
            .output()
            .ok()?;
        (out.status.success() && dest.exists()).then_some(())
    }

    /// The ladder, end to end: a real HEVC frame becomes a real thumbnail.
    /// Which rung answers depends on the machine — WIC where the HEIF extensions
    /// are installed, ffmpeg otherwise — and that is the point: the pipeline
    /// above them must not care. Everything else about HEIF is testable with a
    /// synthetic fixture; the pixels are not, so this builds one with the
    /// machine's own ffmpeg and skips where there is none.
    #[test]
    fn the_platform_ladder_renders_a_heif() {
        let _guard = ingest_guard();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let img = image::RgbImage::from_fn(800, 600, |x, y| {
            image::Rgb([(x % 255) as u8, (y % 255) as u8, 120])
        });
        let png = root.join("source.png");
        img.save(&png).unwrap();
        let heif = root.join("IMG_0300.heic");
        if ffmpeg_made_heif(&png, &heif).is_none() {
            eprintln!("skipped: no ffmpeg that can encode HEVC");
            return;
        }
        std::fs::remove_file(&png).unwrap();

        let db = Arc::new(crate::db::Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        let (_, thumbs, previews) = run_all(&db, root);
        assert_eq!((thumbs, previews), (1, 1), "the HEIF is a rendered cell");

        let (cache_rel, failed): (String, i64) = db
            .call(|c| {
                Ok(c.query_row(
                    "SELECT cache_path, failed FROM thumbnails WHERE kind = 0",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!(failed, 0);
        let cached = root.join(".cullant").join("thumbs").join(&cache_rel);
        let bytes = std::fs::read(&cached).expect("the thumbnail reached the cache");
        let decoded = image::load_from_memory(&bytes).expect("a real JPEG");
        assert_eq!(decoded.width().max(decoded.height()), 384);
    }

    /// Run the whole ingest over `fixtures/media/` — files a camera actually
    /// wrote. See `fixtures/README.md`; `npm run fixtures` populates it.
    ///
    /// Skips when the corpus is absent, so it never fails on a machine that has
    /// not fetched a few hundred megabytes. Run it deliberately:
    ///
    /// ```sh
    /// cargo test the_corpus -- --nocapture
    /// ```
    ///
    /// What it protects: a synthetic JPEG has no maker notes, no embedded
    /// preview at a vendor's private offset, and no sibling belonging to the
    /// same shot. This asserts every real file is dated, sized and rendered, and
    /// prints what it read from each so a regression names the camera.
    #[test]
    fn the_corpus_of_real_files_is_read_and_rendered() {
        let corpus = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .join("fixtures")
            .join("media");
        let Ok(entries) = std::fs::read_dir(&corpus) else {
            eprintln!(
                "skip: no corpus at {} — run `npm run fixtures`",
                corpus.display()
            );
            return;
        };
        let files: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        if files.is_empty() {
            eprintln!("skip: corpus is empty");
            return;
        }

        let _guard = ingest_guard();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for src in &files {
            std::fs::copy(src, root.join(src.file_name().unwrap())).unwrap();
        }

        let db = Arc::new(crate::db::Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        run_all(&db, root);

        #[allow(clippy::type_complexity)]
        let rows: Vec<(String, i64, Option<String>, Option<i64>, i64, i64, i64)> = db
            .call(|c| {
                let mut stmt = c.prepare(
                    "SELECT f.rel_path, f.capture_time, f.camera, f.width,
                            (SELECT COUNT(*) FROM files m WHERE m.group_id = f.group_id),
                            COALESCE((SELECT MAX(t.failed) FROM thumbnails t
                                       WHERE t.file_id = f.id), -1),
                            f.mtime
                     FROM files f ORDER BY f.rel_path",
                )?;
                let rows = stmt.query_map([], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                })?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .unwrap();

        assert_eq!(rows.len(), files.len(), "every file must be catalogued");

        // Files the corpus has already proved this build cannot render. Listed
        // one by one rather than tolerated in bulk, so anything NEW that stops
        // rendering fails the test -- and so the list itself is the to-do.
        //
        // - Panasonic `.RAW` (DMC-FZ8): holds no JPEG at all. Only a demosaic
        //   would render it, which the embedded-preview path never asks for.
        // - Sigma `.DNG` (fp): holds an 8 KB thumbnail and nothing larger.
        const KNOWN_UNRENDERABLE: &[&str] = &[
            "Panasonic - DMC-FZ8 - 4-3.RAW",
            "Sigma - fp - 8bit (16-9).DNG",
        ];

        let mut undated = Vec::new();
        let mut unexpected = Vec::new();
        let mut fixed = Vec::new();
        for (rel, capture, camera, width, group, failed, mtime) in &rows {
            let known = KNOWN_UNRENDERABLE.contains(&rel.as_str());
            eprintln!(
                "  {rel:56} capture={:<11} camera={:24} dims={:?} group={group} thumb={}",
                if capture == mtime {
                    "MTIME".into()
                } else {
                    capture.to_string()
                },
                camera.as_deref().unwrap_or("-"),
                width,
                match failed {
                    -1 => "not attempted",
                    0 => "ok",
                    _ if known => "failed (known)",
                    _ => "FAILED",
                }
            );
            if capture == mtime {
                undated.push(rel.clone());
            }
            if *failed > 0 && !known {
                unexpected.push(rel.clone());
            }
            if *failed == 0 && known {
                fixed.push(rel.clone());
            }
        }

        // Dimensions are deliberately NOT asserted: a RAW's own are the sensor's,
        // and `files.width/height` are backfilled only when the decode was
        // full-size, so most rows here legitimately have none.
        assert!(
            undated.is_empty(),
            "no capture time, fell back to mtime: {undated:?}"
        );
        assert!(unexpected.is_empty(), "newly undecodable: {unexpected:?}");
        assert!(
            fixed.is_empty(),
            "these render now -- drop them from KNOWN_UNRENDERABLE: {fixed:?}"
        );

        // The Live ND pair shares a basename, so it must be one group of two.
        if let Some((rel, .., group, _, _)) = rows.iter().find(|r| r.0.ends_with(".ORI")) {
            assert_eq!(*group, 2, "{rel} must be grouped with its ORF");
        }
    }

    /// Run the whole ladder against a real camera or phone file.
    ///
    /// Nothing in this repo can produce one — no encoder here writes HEVC, and
    /// checking a camera file in has provenance questions — so this is the hook
    /// for verifying by hand:
    ///
    /// ```sh
    /// CULLANT_HEIF_FIXTURE=/path/to/IMG_1234.HEIC cargo test a_real_heif -- --nocapture
    /// ```
    ///
    /// It reports the capture time and dimensions it read, so an EXIF block too
    /// large for `kamadak-exif` shows up as a missing date rather than silently.
    #[test]
    fn a_real_heif_renders_and_is_described() {
        let Ok(fixture) = std::env::var("CULLANT_HEIF_FIXTURE") else {
            eprintln!("skip: set CULLANT_HEIF_FIXTURE to a real .HEIC/.HIF");
            return;
        };
        let _guard = ingest_guard();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let src = Path::new(&fixture);
        let name = src.file_name().expect("a file name");
        std::fs::copy(src, root.join(name)).expect("fixture is readable");

        let db = Arc::new(crate::db::Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        let (_, thumbs, _) = run_all(&db, root);

        let (capture, camera, w, h, failed): (i64, Option<String>, Option<i64>, Option<i64>, i64) =
            db.call(|c| {
                Ok(c.query_row(
                    "SELECT f.capture_time, f.camera, f.width, f.height,
                            COALESCE((SELECT MAX(t.failed) FROM thumbnails t
                                       WHERE t.file_id = f.id), 0)
                     FROM files f",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )?)
            })
            .unwrap();
        eprintln!("capture_time={capture} camera={camera:?} dims={w:?}x{h:?} thumbs={thumbs}");

        assert_eq!(failed, 0, "a real HEIF must not tombstone");
        assert_eq!(thumbs, 1, "the ladder must render it");
        assert!(w.is_some() && h.is_some(), "dimensions must be known");

        // The mean colour of what actually reached the cache. Compare it against
        // the same file decoded by something else: a swapped channel or a
        // misread stride still produces a plausible-looking JPEG, and only the
        // numbers give it away.
        let cache_rel: String = db
            .call(|c| {
                Ok(c.query_row(
                    "SELECT cache_path FROM thumbnails WHERE kind = 0",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        let bytes = std::fs::read(root.join(".cullant").join("thumbs").join(&cache_rel)).unwrap();
        let rgb = image::load_from_memory(&bytes).unwrap().to_rgb8();
        let n = (rgb.width() * rgb.height()) as u64;
        let mut sum = [0u64; 3];
        for px in rgb.pixels() {
            for (s, v) in sum.iter_mut().zip(px.0) {
                *s += u64::from(v);
            }
        }
        eprintln!("rendered mean RGB = {:?}", sum.map(|s| s / n));
    }

    /// A HEIF next to a JPEG is one cell, and the JPEG renders it. The HEIF is
    /// not pregenerated even where a decoder exists: it is not the group's
    /// primary, and paying a subprocess for a frame no grid cell shows would be
    /// the most expensive way to render nothing.
    #[test]
    fn a_heif_beside_a_jpeg_is_not_pregenerated() {
        let _guard = ingest_guard();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let img = image::RgbImage::from_fn(640, 480, |x, _| image::Rgb([(x % 255) as u8, 10, 90]));
        img.save(root.join("IMG_0002.jpg")).unwrap();
        std::fs::copy(root.join("IMG_0002.jpg"), root.join("IMG_0002.hif")).unwrap();

        let db = Arc::new(crate::db::Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();

        let (_, thumbs, previews) = run_all(&db, root);
        // The shot is one cell, rendered from the JPEG.
        assert_eq!((thumbs, previews), (1, 1));

        // A tombstone is keyed on mtime, so one written now would survive the
        // arrival of a decoder and leave the photo blank forever.
        let tombstones: i64 = db
            .call(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM thumbnails WHERE failed = 1",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(tombstones, 0);
    }

    /// The common arrival order: the shot is scanned and dated, and its `.hif`
    /// turns up on a later pass, when nothing else in the group is still
    /// pending. It must end up dated all the same. Falling back to mtime would
    /// strand it at the copy time — the far end of a capture-time sort — and the
    /// value would never correct itself, because it is no longer NULL.
    #[test]
    fn an_opaque_latecomer_is_dated_from_its_own_exif() {
        let _guard = ingest_guard();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let meta = crate::bench::SyntheticExif {
            date_time_original: "2024:06:15 14:30:05",
            orientation: 1,
            make: "OM Digital",
            model: "OM-1",
            iso: 200,
        };
        let img = image::RgbImage::from_fn(320, 240, |x, _| image::Rgb([(x % 255) as u8, 20, 60]));
        std::fs::write(
            root.join("IMG_7.jpg"),
            crate::bench::jpeg_with_exif(&img, &meta),
        )
        .unwrap();

        let db = Arc::new(crate::db::Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        run_all(&db, root);

        // The HEIF arrives afterwards. Its own mtime is "now", not the shot's,
        // and no sibling of it is pending any more.
        std::fs::write(
            root.join("IMG_7.hif"),
            crate::bench::heif_with_exif(&meta, (4032, 3024)),
        )
        .unwrap();
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        run_all(&db, root);

        let (jpg_time, hif_time): (i64, i64) = db
            .call(|c| {
                Ok(c.query_row(
                    "SELECT (SELECT capture_time FROM files WHERE ext = 'jpg'),
                            (SELECT capture_time FROM files WHERE ext = 'hif')",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!(hif_time, 1718461805);
        assert_eq!(hif_time, jpg_time, "the shot keeps one capture time");
    }

    /// A RAW+JPEG pair costs one read of the JPEG half, and the RAW is never
    /// opened at all: metadata, grid thumbnail and loupe preview all come from
    /// the sibling. Asserted the hard way — the .raf holds garbage, so anything
    /// that opens it fails loudly.
    #[test]
    fn a_paired_raw_is_never_opened() {
        let _guard = ingest_guard();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let img = image::RgbImage::from_fn(640, 480, |x, _| image::Rgb([(x % 255) as u8, 10, 90]));
        img.save(root.join("IMG_0001.jpg")).unwrap();
        std::fs::write(root.join("IMG_0001.raf"), b"not a raw file at all").unwrap();

        let db = Arc::new(crate::db::Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();

        let (meta, thumbs, previews) = run_all(&db, root);
        // One read for the pair, not one per file.
        assert_eq!(meta, 2, "both rows get metadata from a single read");
        // The pair is one cell, so one thumbnail and one preview.
        assert_eq!((thumbs, previews), (1, 1));

        // Both halves are dated, and identically — the JPEG's EXIF is the pair's.
        let (raw_time, jpg_time): (Option<i64>, Option<i64>) = db
            .call(|c| {
                Ok(c.query_row(
                    "SELECT (SELECT capture_time FROM files WHERE kind = 0),
                            (SELECT capture_time FROM files WHERE kind = 1)",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!(raw_time, jpg_time);
        assert!(raw_time.is_some());

        // The RAW's own dimensions are the sensor's, so borrowing must not
        // record the JPEG's as if they were the RAW's.
        let raw_dims: (Option<i64>, Option<i64>) = db
            .call(|c| {
                Ok(
                    c.query_row("SELECT width, height FROM files WHERE kind = 0", [], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })?,
                )
            })
            .unwrap();
        assert_eq!(raw_dims, (None, None));

        // The generated artifacts belong to the RAW: it is the group primary,
        // so it is the half the grid shows.
        let owner: i64 = db
            .call(|c| {
                Ok(c.query_row(
                    "SELECT f.kind FROM thumbnails t JOIN files f ON f.id = t.file_id
                     WHERE t.kind = 0",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(owner, 0, "the RAW is the primary, so it owns the thumbnail");

        // Settled: nothing left to do on a second pass.
        assert_eq!(run_all(&db, root), (0, 0, 0));
    }

    #[test]
    fn changing_the_preview_size_restages_only_the_previews() {
        let _guard = ingest_guard();
        use crate::thumbs::{set_preview_long_edge, PREVIEW_LONG_EDGE_DEFAULT};

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let db = project_with_jpegs(root, 3);

        assert_eq!(run_all(&db, root), (3, 3, 3));

        // Same size: still a no-op, so an upgrade never regrinds a good library.
        assert_eq!(run_all(&db, root), (0, 0, 0));

        // A different size makes every preview stale — and nothing else.
        set_preview_long_edge(1600);
        let (meta, thumbs, previews) = run_all(&db, root);
        assert_eq!((meta, thumbs), (0, 0));
        assert_eq!(previews, 3);

        // Regenerated at the new size, so a third pass settles again.
        assert_eq!(run_all(&db, root), (0, 0, 0));

        // Rows built before the column existed (long_edge = 0) count as current,
        // whatever the setting is, so upgrading does not restage them.
        db.call(|c| {
            c.execute("UPDATE thumbnails SET long_edge = 0 WHERE kind = 1", [])?;
            Ok(())
        })
        .unwrap();
        set_preview_long_edge(3840);
        assert_eq!(run_all(&db, root), (0, 0, 0));

        set_preview_long_edge(PREVIEW_LONG_EDGE_DEFAULT);
    }

    #[test]
    fn exif_metadata_flows_through_ingest() {
        let _guard = ingest_guard();
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
        let _guard = ingest_guard();
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
    fn media_ingest_regression_video_creation_date_and_missing_probe_fallback() {
        let _guard = ingest_guard();
        const CHILD_ROOT: &str = "CULLANT_TEST_MISSING_FFPROBE_PROJECT";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let root = Path::new(&root);
            let db = Arc::new(Db::open(root).unwrap());
            db.call(|c| {
                c.execute("UPDATE files SET capture_time=NULL WHERE kind=2", [])?;
                Ok(())
            })
            .unwrap();
            run_all(&db, root);
            let (capture, mtime): (i64, i64) = db
                .call_read(|c| {
                    Ok(c.query_row(
                        "SELECT capture_time,mtime FROM files WHERE kind=2",
                        [],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )?)
                })
                .unwrap();
            assert_eq!(
                capture, mtime,
                "Missing ffprobe must preserve the mtime fallback"
            );
            return;
        }
        let ffmpeg_ready = decode::video::ffmpeg()
            .arg("-version")
            .output()
            .is_ok_and(|output| output.status.success());
        let mut probe = std::process::Command::new("ffprobe");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            probe.creation_flags(0x0800_0000);
        }
        let probe_ready = probe
            .arg("-version")
            .output()
            .is_ok_and(|output| output.status.success());
        if !ffmpeg_ready || !probe_ready {
            eprintln!("skipped: container date regression needs ffmpeg and ffprobe");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dated.mp4");
        let status = decode::video::ffmpeg()
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=160x120:d=2",
                "-c:v",
                "mpeg4",
                "-metadata",
                "creation_time=2024-06-15T14:30:05Z",
            ])
            .arg(&path)
            .status()
            .expect("This real-video regression requires ffmpeg");
        assert!(status.success());
        let db = Arc::new(Db::open(dir.path()).unwrap());
        crate::scan::scan_project_inner(&db, dir.path(), &mut |_| {}).unwrap();
        run_all(&db, dir.path());
        let capture: i64 = db
            .call_read(|c| {
                Ok(
                    c.query_row("SELECT capture_time FROM files WHERE kind=2", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();
        assert_eq!(
            capture, 1718461805,
            "Video capture time must come from container creation_time, not copy mtime"
        );
        drop(db);
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                std::thread::current().name().unwrap(),
                "--nocapture",
            ])
            .env(CHILD_ROOT, dir.path())
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Missing-probe child failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn video_is_ingested_without_preview_and_never_panics() {
        let _guard = ingest_guard();
        // Exercises the video branch end-to-end. This is a local-filesystem
        // project, so the extractor is ffmpeg; the .mp4 bytes are not a real
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
