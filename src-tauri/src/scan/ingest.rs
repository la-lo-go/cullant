//! Fused ingest pass.
//!
//! One read/parse/decode per file produces everything the file needs: EXIF
//! metadata, the 384px grid thumbnail and — mode permitting — the 2560px
//! loupe preview. This replaces the old sequential metadata pass + thumbnail
//! pregeneration, which read and parsed every file twice.
//!
//! Two tiers keep the preload gate honest:
//! - **Tier 1** (gated): files needing metadata or a thumbnail — the fresh
//!   import set. Progress drives `thumbs:progress`, and the gate releases on
//!   `thumbs:done`.
//! - **Tier 2** (background, after the gate): previews still missing — the
//!   whole project in `Background` mode, or only stale/legacy leftovers in
//!   `All` mode (e.g. a project imported before previews were pregenerated).
//!   Progress drives `previews:progress` for a small toolbar indicator.
//!   Interactive requests for a not-yet-generated preview are served
//!   immediately by the ThumbPool (LIFO) and are race-safe with this tier
//!   (temp-file+rename writes, idempotent upserts).

use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use rayon::prelude::*;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::db::Db;
use crate::decode;
use crate::error::AppResult;
use crate::store::ProjectStore;
use crate::thumbs::{self, SourceMeta, ThumbKind, PREVIEW_LONG_EDGE, THUMB_LONG_EDGE};

/// Files handled per parallel burst; also the metadata write-batch size.
/// Smaller on Android to bound the number of in-flight decode buffers.
const CHUNK: usize = if cfg!(target_os = "android") { 8 } else { 32 };

/// Minimum gap between tier-1 progress emits. Progress is now counted per file
/// (inside the parallel burst) instead of once per chunk, so the bar advances
/// one-by-one; this throttle coalesces the emits so a fast decode can't flood
/// IPC. The final item always emits regardless.
const PROGRESS_THROTTLE_MS: u64 = 30;

/// How 2560px previews are pregenerated. Chosen in the frontend settings and
/// passed with open/rescan; `Background` is the default (matches the frontend
/// default and the CULLANT_OPEN_PROJECT hook).
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
#[serde(rename_all = "lowercase")]
pub enum PreviewMode {
    /// Previews are generated together with thumbnails during the gated
    /// import: slowest to open, but the loupe is warm from the first photo.
    All,
    /// The gate releases after thumbnails; previews fill in afterwards in the
    /// background while `previews:progress` drives an indicator.
    #[default]
    Background,
    /// No bulk previews: the frontend warms a window around the focused photo
    /// and the protocol generates on demand.
    Window,
}

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

/// What the selection query says a file still needs.
struct Pending {
    id: i64,
    rel_path: String,
    kind: i64,
    mtime: i64,
    orientation: Option<i64>,
    needs_meta: bool,
    needs_thumb: bool,
    needs_preview: bool,
}

/// EXIF fields extracted during ingest, batched to the writer thread.
struct Extracted {
    id: i64,
    capture_time: Option<i64>,
    orientation: Option<u16>,
    camera: Option<String>,
    lens: Option<String>,
    iso: Option<u32>,
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
            width: None,
            height: None,
        }
    }
}

/// What one file's parallel ingest produced: the optional metadata update and
/// the 0–2 rendered-thumbnail rows (thumb and/or preview). Both are flushed in
/// per-chunk batched transactions by the caller instead of one commit each.
struct IngestOutput {
    extracted: Option<Extracted>,
    thumb_rows: Vec<thumbs::ThumbRow>,
}

impl IngestOutput {
    /// Metadata only, no rendered thumbnails (early-out and skip paths).
    fn meta_only(extracted: Option<Extracted>) -> Self {
        IngestOutput {
            extracted,
            thumb_rows: Vec::new(),
        }
    }
}

/// Event-emitting entry point used by the scanner thread.
pub fn run_ingest_pass(
    app: &AppHandle,
    db: &Arc<Db>,
    store: &dyn ProjectStore,
    root: &Path,
    mode: PreviewMode,
) -> AppResult<()> {
    run_ingest_inner(
        db,
        store,
        root,
        mode,
        // Called from the parallel ingest burst, so it must be Sync; `app.emit`
        // already is.
        &|done, total| {
            let _ = app.emit("thumbs:progress", Progress { done, total });
        },
        &mut |meta_updated, thumb_total| {
            let _ = app.emit(
                "metadata:done",
                MetadataDone {
                    updated: meta_updated,
                },
            );
            let _ = app.emit("thumbs:done", ThumbsDone { total: thumb_total });
        },
        &mut |done, total| {
            let _ = app.emit("previews:progress", Progress { done, total });
        },
    )
}

/// Testable core: closures instead of an AppHandle.
/// `tier1_done(meta_updated, thumb_total)` fires between the tiers — that is
/// the moment the preload gate should release.
pub fn run_ingest_inner(
    db: &Arc<Db>,
    store: &dyn ProjectStore,
    root: &Path,
    mode: PreviewMode,
    // Sync (not FnMut): tier-1 progress is now reported per file from inside the
    // parallel burst, so this may be called concurrently from rayon workers.
    thumb_progress: &(dyn Fn(usize, usize) + Sync),
    tier1_done: &mut dyn FnMut(usize, usize),
    preview_progress: &mut dyn FnMut(usize, usize),
) -> AppResult<()> {
    let pending: Vec<Pending> = db.call(|conn| {
        let mut stmt = conn.prepare(
            // Previews (2560px loupe) are only ever generated for stills; a
            // video's loupe plays the file itself, so `needs_preview` is forced
            // false for kind 2 — videos need only the grid thumbnail.
            "SELECT f.id, f.rel_path, f.kind, f.mtime, f.orientation,
                    (f.capture_time IS NULL) AS needs_meta,
                    (tt.file_id IS NULL OR tt.source_mtime <> f.mtime) AS needs_thumb,
                    (f.kind IN (0, 1)
                     AND (tp.file_id IS NULL OR tp.source_mtime <> f.mtime)) AS needs_preview
             FROM files f
             LEFT JOIN thumbnails tt ON tt.file_id = f.id AND tt.kind = 0
             LEFT JOIN thumbnails tp ON tp.file_id = f.id AND tp.kind = 1
             WHERE f.status = 0 AND f.kind IN (0, 1, 2)
               AND (f.capture_time IS NULL
                    OR tt.file_id IS NULL OR tt.source_mtime <> f.mtime
                    OR (f.kind IN (0, 1)
                        AND (tp.file_id IS NULL OR tp.source_mtime <> f.mtime)))",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Pending {
                id: r.get(0)?,
                rel_path: r.get(1)?,
                kind: r.get(2)?,
                mtime: r.get(3)?,
                orientation: r.get(4)?,
                needs_meta: r.get(5)?,
                needs_thumb: r.get(6)?,
                needs_preview: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })?;

    let previews_inline = mode == PreviewMode::All;
    let (tier1, rest): (Vec<Pending>, Vec<Pending>) = pending
        .into_iter()
        .partition(|p| p.needs_meta || p.needs_thumb);

    // Previews that will still be missing once tier 1 finishes. Carry mtime so
    // the tier-2 loop can use the cache fast path without a DB round-trip.
    let tier2: Vec<(i64, i64)> = match mode {
        PreviewMode::Window => Vec::new(),
        PreviewMode::All => rest
            .iter()
            .filter(|p| p.needs_preview)
            .map(|p| (p.id, p.mtime))
            .collect(),
        PreviewMode::Background => tier1
            .iter()
            .chain(rest.iter())
            .filter(|p| p.needs_preview)
            .map(|p| (p.id, p.mtime))
            .collect(),
    };

    // --- Tier 1: metadata + thumbs (+ previews in All mode), gated ---
    let started = Instant::now();
    let total = tier1.len();
    // Announce the total up front so the preload panel shows `0 / N`
    // immediately instead of `0 / ?` for the whole first-chunk window.
    thumb_progress(0, total);

    let mut meta_updated = 0usize;
    // Per-file progress counter, shared across the parallel burst. Emitting one
    // event per file (throttled) makes the preload bar advance one-by-one
    // instead of jumping a whole chunk at a time.
    let done = AtomicUsize::new(0);
    let last_emit_ms = AtomicU64::new(0);
    for chunk in tier1.chunks(CHUNK) {
        // Decode + render in parallel, then flush this chunk's metadata and
        // thumbnail rows each in a single transaction (instead of one commit
        // per file per artifact).
        let outputs: Vec<IngestOutput> = chunk
            .par_iter()
            .map(|p| {
                let out = ingest_file(db, store, root, p, previews_inline);
                // Advance the shared counter and emit — throttled so a fast
                // decode can't flood IPC, but the last file always reports so
                // the bar reaches `total / total`.
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
                    thumb_progress(done.load(Ordering::Relaxed), total);
                }
                out
            })
            .collect();
        let mut extracted = Vec::with_capacity(outputs.len());
        let mut rows = Vec::with_capacity(outputs.len());
        for out in outputs {
            if let Some(e) = out.extracted {
                extracted.push(e);
            }
            rows.extend(out.thumb_rows);
        }
        meta_updated += extracted.len();
        write_metadata_batch(db, extracted)?;
        thumbs::write_thumb_rows(db, rows)?;
    }
    tracing::info!(
        "ingest tier 1: {total} files (meta for {meta_updated}) in {:.1?}",
        started.elapsed()
    );
    tier1_done(meta_updated, total);

    // --- Tier 2: leftover previews, background (gate already open) ---
    let total2 = tier2.len();
    if total2 > 0 {
        let started2 = Instant::now();
        preview_progress(0, total2);
        let mut done2 = 0usize;
        for chunk in tier2.chunks(CHUNK) {
            chunk.par_iter().for_each(|(file_id, mtime)| {
                // produce_with_mtime() short-circuits on the disk cache without a
                // DB round-trip, so previews the user already pulled
                // interactively cost one file stat here.
                if let Err(e) = thumbs::produce_with_mtime(
                    db,
                    store,
                    root,
                    *file_id,
                    ThumbKind::Preview,
                    Some(*mtime),
                ) {
                    tracing::debug!("preview pregeneration skipped file {file_id}: {e}");
                }
            });
            done2 += chunk.len();
            preview_progress(done2, total2);
        }
        tracing::info!(
            "ingest tier 2: {total2} previews in {:.1?}",
            started2.elapsed()
        );
    }

    Ok(())
}

/// Ingest one tier-1 file: open the source once, parse the container once,
/// extract metadata if missing, and render the thumbnail (and, when
/// `previews_inline`, the preview) from a single decode. Failures are logged
/// and never abort the pass; the returned `Extracted` (when the file needed
/// metadata) always carries at least the mtime fallback via the batched
/// COALESCE update, so unreadable files aren't retried forever.
fn ingest_file(
    db: &Arc<Db>,
    store: &dyn ProjectStore,
    root: &Path,
    p: &Pending,
    previews_inline: bool,
) -> IngestOutput {
    // Videos take a wholly separate path: ffmpeg poster extraction, no EXIF,
    // no preview, and no `RawSource` (which would read the whole clip).
    if p.kind == 2 {
        return ingest_video(db, store, root, p);
    }

    let render_thumb = p.needs_thumb;
    let render_preview = previews_inline && p.needs_preview;
    let mut extracted = p.needs_meta.then(|| Extracted::empty(p.id));

    let source = match decode::open_source(store, &p.rel_path) {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!("ingest could not open {}: {e}", p.rel_path);
            // mtime fallback still applies when metadata was due
            return IngestOutput::meta_only(extracted);
        }
    };

    // Decode + render, sharing one parsed container per RAW.
    let mut src_dims: Option<(u32, u32)> = None;
    let mut decoded = None;
    if p.kind == 0 {
        match decode::raw::RawSession::open(&source) {
            Ok(session) => {
                if let Some(out) = extracted.as_mut() {
                    if let Ok(meta) = session.metadata(&p.rel_path) {
                        out.capture_time = meta.capture_time;
                        out.orientation = meta.orientation;
                        out.camera = meta.camera;
                        out.lens = meta.lens;
                        out.iso = meta.iso;
                    }
                }
                if render_thumb || render_preview {
                    let min_edge = if render_preview {
                        PREVIEW_LONG_EDGE
                    } else {
                        THUMB_LONG_EDGE
                    };
                    match session.decode_adequate(min_edge, &p.rel_path) {
                        Ok(raw) => {
                            src_dims = raw.is_full.then(|| (raw.image.width(), raw.image.height()));
                            decoded = Some(raw.image);
                        }
                        Err(e) => tracing::debug!("ingest decode skipped {}: {e}", p.rel_path),
                    }
                }
            }
            Err(e) => tracing::debug!("ingest could not parse {}: {e}", p.rel_path),
        }
    } else {
        if let Some(out) = extracted.as_mut() {
            if let Ok(meta) = decode::exif::read_metadata(source.buf()) {
                out.capture_time = meta.capture_time;
                out.orientation = meta.orientation;
                out.camera = meta.camera;
                out.lens = meta.lens;
                out.iso = meta.iso;
                out.width = meta.width;
                out.height = meta.height;
            }
        }
        if render_thumb || render_preview {
            let min_edge = if render_preview {
                PREVIEW_LONG_EDGE
            } else {
                THUMB_LONG_EDGE
            };
            // Header-only original dimensions (cheap, no decode).
            src_dims = image::ImageReader::new(std::io::Cursor::new(source.buf()))
                .with_guessed_format()
                .ok()
                .and_then(|r| r.into_dimensions().ok());
            match decode::jpeg::decode_scaled(source.buf(), min_edge, &p.rel_path) {
                Ok(img) => decoded = Some(img),
                Err(e) => tracing::debug!("ingest decode skipped {}: {e}", p.rel_path),
            }
        }
    }

    let mut thumb_rows = Vec::new();
    if let Some(img) = decoded {
        // Orientation extracted just now beats the (possibly NULL) DB column.
        let orientation = extracted
            .as_ref()
            .and_then(|e| e.orientation)
            .map(i64::from)
            .or(p.orientation)
            .unwrap_or(1);
        let meta = SourceMeta {
            file_id: p.id,
            mtime: p.mtime,
            orientation,
            src_dims,
        };
        // Render + cache in parallel here; the row writes are batched by the
        // caller (one transaction per chunk) instead of one commit per file.
        if render_thumb {
            match thumbs::render_to_cache(root, &meta, &img, ThumbKind::Thumb) {
                Ok((_, row)) => thumb_rows.push(row),
                Err(e) => tracing::debug!("thumb render skipped {}: {e}", p.rel_path),
            }
        }
        if render_preview {
            match thumbs::render_to_cache(root, &meta, &img, ThumbKind::Preview) {
                Ok((_, row)) => thumb_rows.push(row),
                Err(e) => tracing::debug!("preview render skipped {}: {e}", p.rel_path),
            }
        }
    } else if render_thumb || render_preview {
        // The source could not be decoded (unsupported/corrupt). Tombstone the
        // due artifacts so later scans don't retry until the file changes, and
        // the grid can flag it instead of requesting a thumb that 404s. Rare, so
        // left as immediate per-file writes rather than batched.
        if render_thumb {
            let _ = thumbs::record_decode_failure(db, p.id, p.mtime, ThumbKind::Thumb);
        }
        if render_preview {
            let _ = thumbs::record_decode_failure(db, p.id, p.mtime, ThumbKind::Preview);
        }
    }

    IngestOutput {
        extracted,
        thumb_rows,
    }
}

/// Ingest one video: fall capture_time back to mtime (videos carry no
/// image-path EXIF) and, when a thumbnail is due, extract a poster frame via
/// ffmpeg and render it through the identical resize/JPEG/cache path as image
/// thumbnails. Poster extraction is best-effort:
/// - ffmpeg missing or no local file → skip WITHOUT a tombstone, so installing
///   ffmpeg later (no mtime change) still gets a retry on the next scan;
/// - a genuinely undecodable/corrupt video → tombstone the thumbnail so the
///   pass and the on-demand pool stop grinding on it, exactly like a broken
///   image.
fn ingest_video(db: &Arc<Db>, store: &dyn ProjectStore, root: &Path, p: &Pending) -> IngestOutput {
    // capture_time = COALESCE(NULL, mtime) via the batched update.
    let extracted = p.needs_meta.then(|| Extracted::empty(p.id));

    if !p.needs_thumb {
        return IngestOutput::meta_only(extracted);
    }
    if !thumbs::video_poster_possible(store, &p.rel_path) {
        tracing::debug!(
            "video thumbnail skipped {} (ffmpeg unavailable)",
            p.rel_path
        );
        return IngestOutput::meta_only(extracted);
    }

    // Guaranteed Some by video_poster_possible.
    let Some(path) = store.local_path(&p.rel_path) else {
        return IngestOutput::meta_only(extracted);
    };
    let mut thumb_rows = Vec::new();
    match decode::video::extract_poster(&path) {
        Ok(img) => {
            let meta = SourceMeta {
                file_id: p.id,
                mtime: p.mtime,
                // ffmpeg auto-rotates on decode; the poster is already upright.
                orientation: 1,
                src_dims: Some((img.width(), img.height())),
            };
            match thumbs::render_to_cache(root, &meta, &img, ThumbKind::Thumb) {
                Ok((_, row)) => thumb_rows.push(row),
                Err(e) => tracing::debug!("video thumb render skipped {}: {e}", p.rel_path),
            }
        }
        Err(e) => {
            tracing::debug!("video poster extraction failed {}: {e}", p.rel_path);
            let _ = thumbs::record_decode_failure(db, p.id, p.mtime, ThumbKind::Thumb);
        }
    }
    IngestOutput {
        extracted,
        thumb_rows,
    }
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
                   width = COALESCE(?7, width),
                   height = COALESCE(?8, height)
                 WHERE id = ?1",
            )?;
            for row in &batch {
                stmt.execute(params![
                    row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7
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

    #[test]
    fn all_mode_produces_thumbs_previews_and_metadata_in_one_pass() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let db = project_with_jpegs(root, 3);
        let store = LocalFsStore::new(root);

        let thumb_last = std::sync::Mutex::new((9, 9));
        let mut gate = None;
        let mut preview_last = None;
        run_ingest_inner(
            &db,
            &store,
            root,
            PreviewMode::All,
            &|d, t| *thumb_last.lock().unwrap() = (d, t),
            &mut |meta, total| gate = Some((meta, total)),
            &mut |d, t| preview_last = Some((d, t)),
        )
        .unwrap();

        assert_eq!(*thumb_last.lock().unwrap(), (3, 3));
        assert_eq!(gate, Some((3, 3)));
        // Fresh import: previews were rendered inline, so no tier 2.
        assert_eq!(preview_last, None);
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

        // Everything fresh: a second pass finds nothing to do.
        let thumb_last2 = std::sync::Mutex::new((9, 9));
        run_ingest_inner(
            &db,
            &store,
            root,
            PreviewMode::All,
            &|d, t| *thumb_last2.lock().unwrap() = (d, t),
            &mut |_, _| {},
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(*thumb_last2.lock().unwrap(), (0, 0));
    }

    #[test]
    fn upgrade_previews_fill_in_tier_two_without_holding_the_gate() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let db = project_with_jpegs(root, 3);
        let store = LocalFsStore::new(root);

        // Seed the "upgrade" state: thumbs + metadata exist, previews don't.
        run_ingest_inner(
            &db,
            &store,
            root,
            PreviewMode::Window,
            &|_, _| {},
            &mut |_, _| {},
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(thumb_count(&db, 0), 3);
        assert_eq!(thumb_count(&db, 1), 0);

        // Now an All-mode pass: the gate must see zero tier-1 work, and the
        // previews must arrive via tier 2.
        let mut gate = None;
        let mut preview_last = None;
        run_ingest_inner(
            &db,
            &store,
            root,
            PreviewMode::All,
            &|_, _| {},
            &mut |meta, total| gate = Some((meta, total)),
            &mut |d, t| preview_last = Some((d, t)),
        )
        .unwrap();
        assert_eq!(gate, Some((0, 0)));
        assert_eq!(preview_last, Some((3, 3)));
        assert_eq!(thumb_count(&db, 1), 3);
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
        let store = LocalFsStore::new(root);
        run_ingest_inner(
            &db,
            &store,
            root,
            PreviewMode::All,
            &|_, _| {},
            &mut |_, _| {},
            &mut |_, _| {},
        )
        .unwrap();

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

        // The freshly-extracted orientation must reach the same-pass render:
        // landscape 800x600 + rotation 6 = portrait 288x384 thumbnail.
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
        let store = LocalFsStore::new(root);

        // First pass: the file needs a thumb, fails to decode, gets tombstoned.
        let mut gate = None;
        run_ingest_inner(
            &db,
            &store,
            root,
            PreviewMode::All,
            &|_, _| {},
            &mut |_, total| gate = Some(total),
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(gate, Some(1));

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
        let mut gate2 = None;
        run_ingest_inner(
            &db,
            &store,
            root,
            PreviewMode::All,
            &|_, _| {},
            &mut |_, total| gate2 = Some(total),
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(gate2, Some(0), "tombstoned files must not be retried");
    }

    #[test]
    fn video_is_ingested_without_preview_and_never_panics() {
        // Exercises the video branch end-to-end. The .mp4 bytes are not a real
        // video, so the outcome depends on whether ffmpeg is installed:
        //   - ffmpeg present: extraction fails -> thumbnail tombstoned;
        //   - ffmpeg absent:  extraction skipped -> no thumbnail, no tombstone.
        // Either way the pass must complete, the video must be counted in
        // tier 1, get capture_time from mtime, and never receive a preview.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("clip.mp4"), b"not a real mp4").unwrap();

        let db = Arc::new(crate::db::Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        let store = LocalFsStore::new(root);

        let mut gate = None;
        let mut preview_last = None;
        run_ingest_inner(
            &db,
            &store,
            root,
            PreviewMode::All,
            &|_, _| {},
            &mut |_, total| gate = Some(total),
            &mut |d, t| preview_last = Some((d, t)),
        )
        .unwrap();

        // The video was tier-1 work (needs meta + thumb).
        assert_eq!(gate, Some(1));
        // Videos never get a 2560px preview, in any mode.
        assert_eq!(preview_last, None);
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

    #[test]
    fn background_mode_defers_all_previews_to_tier_two() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let db = project_with_jpegs(root, 2);
        let store = LocalFsStore::new(root);

        let mut gate = None;
        let mut preview_last = None;
        run_ingest_inner(
            &db,
            &store,
            root,
            PreviewMode::Background,
            &|_, _| {},
            &mut |meta, total| {
                // At gate time the previews must NOT exist yet.
                gate = Some((meta, total, thumb_count(&db, 1)));
            },
            &mut |d, t| preview_last = Some((d, t)),
        )
        .unwrap();
        assert_eq!(gate, Some((2, 2, 0)));
        assert_eq!(preview_last, Some((2, 2)));
        assert_eq!(thumb_count(&db, 1), 2);
    }
}
