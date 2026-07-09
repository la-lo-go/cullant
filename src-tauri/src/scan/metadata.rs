use std::sync::Arc;

use rayon::prelude::*;
use rusqlite::params;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::db::Db;
use crate::decode;
use crate::error::AppResult;
use crate::store::{read_all, ProjectStore};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MetadataDone {
    pub updated: usize,
}

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

/// Fill capture_time/orientation/camera/dims for files that don't have them
/// yet. Runs on the rayon pool; DB updates are batched through the writer
/// thread. Sort order in the UI improves progressively as this completes.
pub fn run_metadata_pass(
    app: &AppHandle,
    db: &Arc<Db>,
    store: &dyn ProjectStore,
) -> AppResult<usize> {
    let pending: Vec<(i64, String, i64)> = db.call(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, rel_path, kind FROM files
             WHERE status = 0 AND kind IN (0, 1) AND capture_time IS NULL",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })?;

    if pending.is_empty() {
        return Ok(0);
    }
    let total = pending.len();
    tracing::info!("metadata pass: {total} files to inspect");

    let extracted: Vec<Extracted> = pending
        .par_iter()
        .map(|(id, rel_path, kind)| {
            let mut out = Extracted {
                id: *id,
                capture_time: None,
                orientation: None,
                camera: None,
                lens: None,
                iso: None,
                width: None,
                height: None,
            };
            let Ok(bytes) = read_all(store, rel_path) else {
                return out;
            };
            if *kind == 0 {
                if let Ok(meta) = decode::raw::read_metadata(Arc::new(bytes), rel_path) {
                    out.capture_time = meta.capture_time;
                    out.orientation = meta.orientation;
                    out.camera = meta.camera;
                    out.lens = meta.lens;
                    out.iso = meta.iso;
                }
            } else if let Ok(meta) = decode::exif::read_metadata(&bytes) {
                out.capture_time = meta.capture_time;
                out.orientation = meta.orientation;
                out.camera = meta.camera;
                out.lens = meta.lens;
                out.iso = meta.iso;
                out.width = meta.width;
                out.height = meta.height;
            }
            out
        })
        .collect();

    let updated = extracted.len();
    for chunk in extracted.chunks(200) {
        let batch: Vec<_> = chunk
            .iter()
            .map(|e| {
                (
                    e.id,
                    // Files with unreadable metadata fall back to mtime so the
                    // pass doesn't retry them forever.
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
        })?;
    }

    tracing::info!("metadata pass complete: {updated} files updated");
    let _ = app.emit("metadata:done", MetadataDone { updated });
    Ok(updated)
}
