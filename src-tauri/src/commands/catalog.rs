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
    pub is_primary: bool,
    pub group_size: i64,
    pub decoupled: bool,
    pub tag_ids: Vec<i64>,
    /// True when the grid thumbnail could not be decoded (unsupported/corrupt
    /// source), so the UI shows a placeholder instead of requesting an image.
    pub thumb_failed: bool,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SortKey {
    #[default]
    Capture,
    Name,
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

/// Return the light-weight index of all present files for one media tab.
/// ~100 bytes per item; the whole catalog crosses IPC once and the frontend
/// filters/virtualizes locally.
#[tauri::command]
pub fn query_items(
    sort: Option<SortKey>,
    media: Option<MediaTab>,
    desc: Option<bool>,
    state: State<'_, AppState>,
) -> AppResult<Vec<ItemLite>> {
    let db = {
        let guard = state.project.lock().unwrap();
        guard
            .as_ref()
            .ok_or(crate::error::AppError::NoProject)?
            .db
            .clone()
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
                    f.orientation AS orientation
             FROM files f
             JOIN groups g ON g.id = f.group_id
             WHERE f.status = 0 AND {kind_filter}
             ORDER BY {order}"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            Ok(ItemLite {
                id: r.get(0)?,
                group_id: r.get(1)?,
                kind: r.get(2)?,
                rel_path: r.get(3)?,
                name: r.get(4)?,
                ext: r.get(5)?,
                mtime: r.get(6)?,
                capture_time: r.get(7)?,
                rating: r.get(8)?,
                flag: r.get(9)?,
                label: r.get(10)?,
                width: r.get(11)?,
                height: r.get(12)?,
                orientation: r.get(18)?,
                is_primary: r.get::<_, Option<bool>>(13)?.unwrap_or(true),
                group_size: r.get(14)?,
                decoupled: r.get(15)?,
                tag_ids: r
                    .get::<_, Option<String>>(16)?
                    .map(|csv| csv.split(',').filter_map(|s| s.parse().ok()).collect())
                    .unwrap_or_default(),
                thumb_failed: r.get::<_, i64>(17)? != 0,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })
}
