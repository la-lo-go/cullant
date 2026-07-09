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
const CHUNK: usize = 32;

/// How 2560px previews are pregenerated. Chosen in the frontend settings and
/// passed with open/rescan; `All` is the default (matches CULLANT_OPEN_PROJECT).
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
#[serde(rename_all = "lowercase")]
pub enum PreviewMode {
    /// Previews are generated together with thumbnails during the gated
    /// import: slowest to open, but the loupe is warm from the first photo.
    #[default]
    All,
    /// The gate releases after thumbnails; previews fill in afterwards in the
    /// background while `previews:progress` drives an indicator.
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
        &mut |done, total| {
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
    thumb_progress: &mut dyn FnMut(usize, usize),
    tier1_done: &mut dyn FnMut(usize, usize),
    preview_progress: &mut dyn FnMut(usize, usize),
) -> AppResult<()> {
    let pending: Vec<Pending> = db.call(|conn| {
        let mut stmt = conn.prepare(
            "SELECT f.id, f.rel_path, f.kind, f.mtime, f.orientation,
                    (f.capture_time IS NULL) AS needs_meta,
                    (tt.file_id IS NULL OR tt.source_mtime <> f.mtime) AS needs_thumb,
                    (tp.file_id IS NULL OR tp.source_mtime <> f.mtime) AS needs_preview
             FROM files f
             LEFT JOIN thumbnails tt ON tt.file_id = f.id AND tt.kind = 0
             LEFT JOIN thumbnails tp ON tp.file_id = f.id AND tp.kind = 1
             WHERE f.status = 0 AND f.kind IN (0, 1)
               AND (f.capture_time IS NULL
                    OR tt.file_id IS NULL OR tt.source_mtime <> f.mtime
                    OR tp.file_id IS NULL OR tp.source_mtime <> f.mtime)",
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

    // Previews that will still be missing once tier 1 finishes.
    let tier2_ids: Vec<i64> = match mode {
        PreviewMode::Window => Vec::new(),
        PreviewMode::All => rest
            .iter()
            .filter(|p| p.needs_preview)
            .map(|p| p.id)
            .collect(),
        PreviewMode::Background => tier1
            .iter()
            .chain(rest.iter())
            .filter(|p| p.needs_preview)
            .map(|p| p.id)
            .collect(),
    };

    // --- Tier 1: metadata + thumbs (+ previews in All mode), gated ---
    let started = Instant::now();
    let total = tier1.len();
    // Announce the total up front so the preload panel shows `0 / N`
    // immediately instead of `0 / ?` for the whole first-chunk window.
    thumb_progress(0, total);

    let mut meta_updated = 0usize;
    let mut done = 0usize;
    for chunk in tier1.chunks(CHUNK) {
        let extracted: Vec<Extracted> = chunk
            .par_iter()
            .filter_map(|p| ingest_file(db, store, root, p, previews_inline))
            .collect();
        meta_updated += extracted.len();
        write_metadata_batch(db, extracted)?;
        done += chunk.len();
        thumb_progress(done, total);
    }
    tracing::info!(
        "ingest tier 1: {total} files (meta for {meta_updated}) in {:.1?}",
        started.elapsed()
    );
    tier1_done(meta_updated, total);

    // --- Tier 2: leftover previews, background (gate already open) ---
    let total2 = tier2_ids.len();
    if total2 > 0 {
        let started2 = Instant::now();
        preview_progress(0, total2);
        let mut done2 = 0usize;
        for chunk in tier2_ids.chunks(CHUNK) {
            chunk.par_iter().for_each(|&file_id| {
                // produce() short-circuits on the disk cache, so previews the
                // user already pulled interactively cost one file stat here.
                if let Err(e) = thumbs::produce(db, store, root, file_id, ThumbKind::Preview) {
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
) -> Option<Extracted> {
    let render_thumb = p.needs_thumb;
    let render_preview = previews_inline && p.needs_preview;
    let mut extracted = p.needs_meta.then(|| Extracted::empty(p.id));

    let source = match decode::open_source(store, &p.rel_path) {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!("ingest could not open {}: {e}", p.rel_path);
            return extracted; // mtime fallback still applies when metadata was due
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
        if render_thumb {
            if let Err(e) = thumbs::render_and_store(db, root, &meta, &img, ThumbKind::Thumb) {
                tracing::debug!("thumb render skipped {}: {e}", p.rel_path);
            }
        }
        if render_preview {
            if let Err(e) = thumbs::render_and_store(db, root, &meta, &img, ThumbKind::Preview) {
                tracing::debug!("preview render skipped {}: {e}", p.rel_path);
            }
        }
    }

    extracted
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

        let mut thumb_last = (9, 9);
        let mut gate = None;
        let mut preview_last = None;
        run_ingest_inner(
            &db,
            &store,
            root,
            PreviewMode::All,
            &mut |d, t| thumb_last = (d, t),
            &mut |meta, total| gate = Some((meta, total)),
            &mut |d, t| preview_last = Some((d, t)),
        )
        .unwrap();

        assert_eq!(thumb_last, (3, 3));
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
        let mut thumb_last2 = (9, 9);
        run_ingest_inner(
            &db,
            &store,
            root,
            PreviewMode::All,
            &mut |d, t| thumb_last2 = (d, t),
            &mut |_, _| {},
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(thumb_last2, (0, 0));
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
            &mut |_, _| {},
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
            &mut |_, _| {},
            &mut |meta, total| gate = Some((meta, total)),
            &mut |d, t| preview_last = Some((d, t)),
        )
        .unwrap();
        assert_eq!(gate, Some((0, 0)));
        assert_eq!(preview_last, Some((3, 3)));
        assert_eq!(thumb_count(&db, 1), 3);
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
            &mut |_, _| {},
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
