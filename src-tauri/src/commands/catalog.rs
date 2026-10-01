use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::AppResult;
use crate::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemLite {
    pub id: i64,
    pub group_id: i64,
    pub kind: i64,
    pub rel_path: String,
    pub name: String,
    pub ext: String,
    pub mtime: i64,
    pub source_version: String,
    pub capture_time: Option<i64>,
    pub rating: i64,
    pub flag: i64,
    pub label: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    /// Raw EXIF orientation (1-8) as stored in the DB, or None. Values 5-8 mean
    /// the displayed image is rotated 90°, so displayed width/height are the
    /// swap of the raw `width`/`height` fields (which are never swapped). The
    /// frontend applies this to classify portrait/landscape.
    pub orientation: Option<i64>,
    /// Camera body ("Make Model"), lens model, ISO, focal length (mm) and
    /// aperture (bare f-number). Photographic-settings facets for the filter
    /// panel; null on videos and on images without the relevant EXIF tag.
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<i64>,
    pub focal_length: Option<f64>,
    pub f_number: Option<f64>,
    pub exposure_time: Option<f64>,
    pub video_codec: Option<String>,
    pub video_frame_rate: Option<f64>,
    pub video_duration: Option<f64>,
    pub is_primary: bool,
    pub group_size: i64,
    pub decoupled: bool,
    pub tag_ids: Vec<i64>,
    /// True when the grid thumbnail could not be decoded (unsupported/corrupt
    /// source), so the UI shows a placeholder instead of requesting an image.
    pub thumb_failed: bool,
    /// 16-char hex of the thumbnail's 64-bit difference hash, or None until the
    /// thumbnail has been generated. Used to tell apart consecutive frames of a
    /// burst from unrelated shots taken moments apart.
    pub phash: Option<String>,
    /// True when a usable grid thumbnail already exists on disk. Lets the grid
    /// tell "this is still being generated" apart from "this is cached and is
    /// about to appear", which are the same skeleton otherwise.
    pub thumb_ready: bool,
    /// True when the loupe preview was tried and could not be produced. Without
    /// it the "generating preview" spinner has no way to stop.
    pub preview_failed: bool,
}

/// Lowercase hex of exactly 8 bytes.
fn hex8(bytes: Vec<u8>) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(16), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SortKey {
    #[default]
    Capture,
    Name,
    Size,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum MediaTab {
    #[default]
    Photos,
    Videos,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaCounts {
    pub photos: i64,
    pub videos: i64,
}

/// Total present-file counts per media kind, independent of the active tab —
/// used to disable the Photos/Videos toggle when one side is empty.
#[tauri::command]
pub fn media_counts(state: State<'_, AppState>) -> AppResult<MediaCounts> {
    let db = {
        let guard = state.project.lock().unwrap();
        guard
            .as_ref()
            .ok_or(crate::error::AppError::NoProject)?
            .db
            .clone()
    };
    db.call_read(|conn| {
        let photos: i64 = conn.query_row(
            "SELECT COUNT(*) FROM files WHERE status = 0 AND kind IN (0, 1)",
            [],
            |r| r.get(0),
        )?;
        let videos: i64 = conn.query_row(
            "SELECT COUNT(*) FROM files WHERE status = 0 AND kind = 2",
            [],
            |r| r.get(0),
        )?;
        Ok(MediaCounts { photos, videos })
    })
}

/// File ids with a valid cached preview for the current source and quality.
/// Drives the grid's per-cell "full preview still generating" spinner. The
/// payload is just a list of ids (a few hundred at most), cheap to re-query as
/// the background preview pass progresses.
#[tauri::command(async)]
pub fn preview_ready_ids(state: State<'_, AppState>) -> AppResult<Vec<i64>> {
    let (db, root) = {
        let guard = state.project.lock().unwrap();
        let project = guard.as_ref().ok_or(crate::error::AppError::NoProject)?;
        (project.db.clone(), project.root.clone())
    };
    let candidates = db.call_read(|conn| {
        let mut stmt = conn.prepare(&format!(
            "SELECT t.file_id, t.cache_path, f.mtime, COALESCE(f.orientation, 1), {source}
             FROM thumbnails t
             JOIN files f ON f.id = t.file_id
             JOIN groups g ON g.id = f.group_id
             WHERE t.kind = 1 AND t.failed = 0
               AND t.source_mtime = f.mtime AND f.status = 0
               AND (t.long_edge = 0 OR t.long_edge = {edge})",
            source = crate::thumbs::source_version_sql(),
            edge = crate::thumbs::preview_long_edge()
        ))?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                crate::thumbs::CacheVersion {
                    mtime: r.get(2)?,
                    orientation: r.get(3)?,
                },
                r.get::<_, String>(4)?,
            ))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })?;
    Ok(candidates
        .into_iter()
        .filter(|(id, path, version, source)| {
            crate::thumbs::cached_row_ready(
                &root,
                path,
                *id,
                *version,
                crate::thumbs::ThumbKind::Preview,
                crate::thumbs::source_version(source),
            )
        })
        .map(|(id, _, _, _)| id)
        .collect())
}

/// Return the light-weight index of all present files for one media tab.
/// ~100 bytes per item; the whole catalog crosses IPC once and the frontend
/// filters/virtualizes locally.
#[tauri::command(async)]
pub fn query_items(
    sort: Option<SortKey>,
    media: Option<MediaTab>,
    desc: Option<bool>,
    state: State<'_, AppState>,
) -> AppResult<Vec<ItemLite>> {
    let (db, root) = {
        let guard = state.project.lock().unwrap();
        let project = guard.as_ref().ok_or(crate::error::AppError::NoProject)?;
        (project.db.clone(), project.root.clone())
    };

    let sort = sort.unwrap_or_default();
    let media = media.unwrap_or_default();
    let desc = desc.unwrap_or(false);
    db.call_read(move |conn| {
        let kind_filter = match media {
            MediaTab::Photos => "kind IN (0, 1)",
            MediaTab::Videos => "kind = 2",
        };
        // The tie-breaker follows the primary direction so a reversed sort is a
        // true mirror of the ascending one.
        let dir = if desc { "DESC" } else { "ASC" };
        let order = match sort {
            SortKey::Capture => format!("COALESCE(capture_time, mtime) {dir}, rel_path {dir}"),
            SortKey::Name => format!("rel_path {dir}"),
            SortKey::Size => format!("f.size {dir}, rel_path {dir}"),
        };
        let sql = format!(
            "SELECT f.id, f.group_id, f.kind, f.rel_path, f.basename, f.ext, f.mtime,
                    f.capture_time, f.rating, f.flag, f.label, f.width, f.height,
                    (g.primary_file_id = f.id) AS is_primary,
                    (SELECT COUNT(*) FROM files m
                      WHERE m.group_id = f.group_id AND m.status = 0) AS group_size,
                    g.decoupled,
                    (SELECT GROUP_CONCAT(tag_id) FROM file_tags t
                      WHERE t.file_id = f.id) AS tag_ids,
                    EXISTS(SELECT 1 FROM thumbnails th
                      WHERE th.file_id = f.id AND th.kind = 0
                        AND th.failed = 1 AND th.source_mtime = f.mtime) AS thumb_failed,
                    f.orientation AS orientation,
                    f.camera, f.lens, f.iso, f.focal_length, f.f_number, f.exposure_time,
                    fa.phash,
                    (SELECT tr.cache_path FROM thumbnails tr
                      WHERE tr.file_id = f.id AND tr.kind = 0
                        AND tr.failed = 0 AND tr.source_mtime = f.mtime) AS thumb_cache_path,
                    EXISTS(SELECT 1 FROM thumbnails pf
                      WHERE pf.file_id = f.id AND pf.kind = 1
                        AND pf.failed = 1 AND pf.source_mtime = f.mtime) AS preview_failed,
                    {source_version} AS source_version,
                    f.video_codec, f.video_frame_rate, f.video_duration
             FROM files f
             JOIN groups g ON g.id = f.group_id
             -- A plain join avoids another correlated subquery in this wide row.
             LEFT JOIN file_analysis fa ON fa.file_id = f.id
             WHERE f.status = 0 AND {kind_filter}
             ORDER BY {order}",
            source_version = crate::thumbs::source_version_sql()
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            let id = r.get(0)?;
            let mtime = r.get(6)?;
            let orientation: Option<i64> = r.get(18)?;
            let source_version = crate::thumbs::source_version(&r.get::<_, String>(28)?);
            let thumb_ready = r.get::<_, Option<String>>(26)?.is_some_and(|path| {
                crate::thumbs::cached_row_ready(
                    &root,
                    &path,
                    id,
                    crate::thumbs::CacheVersion {
                        mtime,
                        orientation: orientation.unwrap_or(1),
                    },
                    crate::thumbs::ThumbKind::Thumb,
                    source_version,
                )
            });
            Ok(ItemLite {
                id,
                group_id: r.get(1)?,
                kind: r.get(2)?,
                rel_path: r.get(3)?,
                name: r.get(4)?,
                ext: r.get(5)?,
                mtime,
                source_version: format!("{source_version:016x}"),
                capture_time: r.get(7)?,
                rating: r.get(8)?,
                flag: r.get(9)?,
                label: r.get(10)?,
                width: r.get(11)?,
                height: r.get(12)?,
                orientation,
                camera: r.get(19)?,
                lens: r.get(20)?,
                iso: r.get(21)?,
                focal_length: r.get(22)?,
                f_number: r.get(23)?,
                exposure_time: r.get(24)?,
                video_codec: r.get(29)?,
                video_frame_rate: r.get(30)?,
                video_duration: r.get(31)?,
                is_primary: r.get::<_, Option<bool>>(13)?.unwrap_or(true),
                group_size: r.get(14)?,
                decoupled: r.get(15)?,
                tag_ids: r
                    .get::<_, Option<String>>(16)?
                    .map(|csv| csv.split(',').filter_map(|s| s.parse().ok()).collect())
                    .unwrap_or_default(),
                thumb_failed: r.get::<_, i64>(17)? != 0,
                // Hex rather than a number: a u64 does not survive the trip
                // through a JavaScript number intact.
                phash: r
                    .get::<_, Option<Vec<u8>>>(25)?
                    .filter(|b| b.len() == 8)
                    .map(hex8),
                thumb_ready,
                preview_failed: r.get::<_, i64>(27)? != 0,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })
}
