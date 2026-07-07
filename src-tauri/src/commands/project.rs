use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params;
use serde::Serialize;
use tauri::State;

use crate::db::{migrations, Db};
use crate::error::{AppError, AppResult};
use crate::{AppState, ProjectState};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub root_path: String,
    pub db_path: String,
    pub schema_version: i64,
    pub file_count: i64,
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[tauri::command]
pub fn open_project(path: String, state: State<'_, AppState>) -> AppResult<ProjectInfo> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(AppError::Other(format!("not a directory: {path}")));
    }

    let db = Db::open(&root)?;
    let db_path = db.path().to_string_lossy().into_owned();
    let root_str = root.to_string_lossy().into_owned();

    let root_for_db = root_str.clone();
    let (schema_version, file_count) = db.call(move |conn| {
        conn.execute(
            "INSERT INTO project (id, root_path, created_at) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET root_path = excluded.root_path",
            params![root_for_db, unix_now()],
        )?;
        let version = migrations::current_version(conn)?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
        Ok((version, count))
    })?;

    *state.project.lock().unwrap() = Some(ProjectState { root, db });
    tracing::info!("opened project at {root_str}");

    Ok(ProjectInfo {
        root_path: root_str,
        db_path,
        schema_version,
        file_count,
    })
}

#[tauri::command]
pub fn close_project(state: State<'_, AppState>) {
    *state.project.lock().unwrap() = None;
}
