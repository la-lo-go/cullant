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

/// Return the light-weight index of all present files for one media tab.
/// ~100 bytes per item; the whole catalog crosses IPC once and the frontend
/// filters/virtualizes locally.
#[tauri::command]
pub fn query_items(
    sort: Option<SortKey>,
    media: Option<MediaTab>,
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
    db.call(move |conn| {
        let kind_filter = match media {
            MediaTab::Photos => "kind IN (0, 1)",
            MediaTab::Videos => "kind = 2",
        };
        let order = match sort {
            SortKey::Capture => "COALESCE(capture_time, mtime) ASC, rel_path ASC",
            SortKey::Name => "rel_path ASC",
        };
        let sql = format!(
            "SELECT id, group_id, kind, rel_path, basename, ext, mtime, capture_time,
                    rating, flag, label, width, height
             FROM files WHERE status = 0 AND {kind_filter} ORDER BY {order}"
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
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })
}
